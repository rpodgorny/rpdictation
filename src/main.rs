use anyhow::{Context, Result};
use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};
use std::path::Path;
use std::env;
use std::process::Command;
use std::fs;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncBufReadExt, BufReader};
use notify_rust::Notification;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Use wtype to type out the transcription
    #[arg(long)]
    wtype: bool,
}
const SAMPLE_RATE: u32 = 16000;
const CHANNELS: u16 = 1;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize audio host and device
    let host = cpal::default_host();
    let device = host.default_input_device()
        .context("Failed to get default input device")?;

    // Prepare WAV writer
    let path = Path::new("recording.wav");
    let spec = hound::WavSpec {
        channels: CHANNELS,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let writer = Arc::new(Mutex::new(Some(hound::WavWriter::create(
        path,
        spec,
    ).context("Failed to create WAV file")?)));

    // Configure input stream
    let config = cpal::StreamConfig {
        channels: CHANNELS,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };

    // Create and run the input stream
    let writer_clone = Arc::clone(&writer);
    let err_fn = move |err| eprintln!("An error occurred on stream: {}", err);
    
    let stream = device.build_input_stream(
        &config,
        move |data: &[f32], _: &_| {
            if let Some(writer) = &mut *writer_clone.lock().unwrap() {
                for &sample in data {
                    writer.write_sample((sample * i16::MAX as f32) as i16).unwrap();
                }
            }
        },
        err_fn,
        None,
    )?;

    stream.play()?;

    // Create named pipe for stop signal
    let fifo_path = "/tmp/whisper_stop";
    if fs::metadata(fifo_path).is_ok() {
        fs::remove_file(fifo_path)?;
    }
    nix::unistd::mkfifo(fifo_path, nix::sys::stat::Mode::S_IRWXU)?;

    println!("Recording... Stop with:");
    println!("- Press Enter, or");
    println!("- Run: echo x > {}", fifo_path);
    
    // Start recording timer
    let start_time = Instant::now();
    
    // Set up notification with action
    let notification_handle = Notification::new()
        .summary("Recording in progress")
        .body("Recording in progress. Use Enter key or named pipe to stop.")
        .icon("audio-input-microphone")
        .timeout(0) // 0 means the notification won't time out
        .show()?;

    // Set up on_close handler
    notification_handle.on_close(|| {
        println!("Notification closed");
        // No need to explicitly close the notification
    });
    
    // Spawn a timer to display recording length
    let (timer_tx, mut timer_rx) = tokio::sync::oneshot::channel();
    let timer_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let elapsed = start_time.elapsed();
                    let minutes = elapsed.as_secs() / 60;
                    let seconds = elapsed.as_secs() % 60;
                    print!("\rRecording length: {:02}:{:02}", minutes, seconds);
                    std::io::Write::flush(&mut std::io::stdout()).unwrap();
                }
                _ = &mut timer_rx => {
                    println!(); // Print a newline before exiting
                    break;
                }
            }
        }
        Ok::<_, anyhow::Error>(())
    });

    // Set up async readers for input sources
    let (stdin_tx, mut stdin_rx) = tokio::sync::oneshot::channel();
    let (fifo_tx, mut fifo_rx) = tokio::sync::oneshot::channel();

    // Spawn stdin reader
    tokio::spawn(async move {
        let mut stdin = BufReader::new(tokio::io::stdin());
        let mut buf = String::new();
        stdin.read_line(&mut buf).await?;
        stdin_tx.send(()).map_err(|_| anyhow::anyhow!("Failed to send stdin signal"))?;
        Ok::<_, anyhow::Error>(())
    });

    // Spawn fifo reader
    tokio::spawn(async move {
        let mut fifo = File::open(fifo_path).await?;
        let mut buf = [0u8; 1];
        fifo.read_exact(&mut buf).await?;
        fifo_tx.send(()).map_err(|_| anyhow::anyhow!("Failed to send fifo signal"))?;
        Ok::<_, anyhow::Error>(())
    });

    // Wait for any input method
    match tokio::select! {
        _ = &mut stdin_rx => "Enter key",
        _ = &mut fifo_rx => "named pipe",
    } {
        source => println!("Stopped by {}", source),
    }
    
    // Stop the timer
    let _ = timer_tx.send(());
    let _ = timer_handle.await?;
    
    // Create a new notification to replace the old one
    Notification::new()
        .summary("Recording finished")
        .body("Recording has been stopped")
        .icon("audio-input-microphone")
        .timeout(3000) // 3 seconds timeout
        .show()?;
    
    // Clean up the pipe - don't fail if it's already gone
    let _ = fs::remove_file(fifo_path);
    
    // Stop recording and close file
    drop(stream);
    if let Some(writer) = writer.lock().unwrap().take() {
        writer.finalize()?;
    }

    println!("Recording saved. Analyzing...");

    // Get recording stats
    let file_size = std::fs::metadata("recording.wav")?.len();
    let reader = hound::WavReader::open("recording.wav")?;
    let duration_seconds = reader.duration() as f64 / reader.spec().sample_rate as f64;
    
    println!("Recording length: {:.1} seconds", duration_seconds);
    println!("File size: {:.1} MB", file_size as f64 / 1_048_576.0);
    println!("\nTranscribing...");

    // Store duration for later use
    let audio_duration = duration_seconds;

    // Send to Whisper API
    let client = reqwest::Client::new();
    let file_bytes = std::fs::read("recording.wav")?;
    let file_part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name("recording.wav")
        .mime_str("audio/wav")?;
    let form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("model", "whisper-1");

    let api_key = env::var("OPENAI_API_KEY")
        .context("OPENAI_API_KEY environment variable not set")?;

    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .timeout(Duration::from_secs(30))  // Add timeout to prevent hanging
        .send()
        .await?;

    let result: serde_json::Value = response.json().await?;

    if let Some(text) = result["text"].as_str() {
        println!("\nTranscription:");
        println!("{}", text);

        if args.wtype {
            println!("\nTyping text using wtype...");
            // Check if wtype is installed
            if Command::new("which").arg("wtype").status().is_ok() {
                Command::new("wtype")
                    .arg(text)
                    .status()
                    .context("Failed to run wtype")?;
            } else {
                println!("wtype command not found. Please install it to use this feature.");
            }
        }

        // Calculate cost - $0.006 per minute
        let minutes = (audio_duration / 60.0).ceil();
        let cost = minutes * 0.006;

        println!("\nAudio duration: {:.1} seconds", duration_seconds);
        println!("Cost: ${:.4}", cost);
    } else {
        println!("Failed to get transcription from response");
    }

    // Clean up the recording file - don't fail if it's already gone
    let _ = std::fs::remove_file("recording.wav");

    Ok(())
}
