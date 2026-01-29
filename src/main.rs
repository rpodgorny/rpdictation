use anyhow::{Context, Result};
use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::env;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncBufReadExt;
use tokio_util::sync::CancellationToken;

const SAMPLE_RATE: u32 = 16000;
const CHANNELS: u16 = 1;
const MIN_RECORDING_DURATION_SECONDS: f64 = 1.0;

const FIFO_PATH: &str = "/tmp/rpdictation_stop";
const RECORDING_FILENAME: &str = "/tmp/rpdictation.wav";

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Use wtype to type out the transcription
    #[arg(long)]
    wtype: bool,

    /// OpenAI API key (overrides OPENAI_API_KEY environment variable)
    #[arg(long)]
    openai_api_key: Option<String>,
}

async fn main_async() -> Result<()> {
    let args = Args::parse();

    if args.wtype
        && tokio::process::Command::new("which")
            .arg("wtype")
            .status()
            .await
            .is_err()
    {
        println!("wtype command not found. Please install it to use this feature.");
        return Ok(());
    }

    if tokio::fs::try_exists(".env").await? {
        println!("loading environment from .env");
        dotenvy::dotenv()?;
    }

    let api_key = match &args.openai_api_key {
        Some(key) => key.clone(),
        None => env::var("OPENAI_API_KEY").context(
            "OPENAI_API_KEY environment variable not set or --openai-api-key not provided",
        )?,
    };

    // Initialize audio host and device
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("Failed to get default input device")?;

    // Prepare WAV writer
    let path = Path::new(RECORDING_FILENAME);
    let spec = hound::WavSpec {
        channels: CHANNELS,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let writer = Arc::new(Mutex::new(Some(
        hound::WavWriter::create(path, spec).context("Failed to create WAV file")?,
    )));

    // Configure input stream
    let config = cpal::StreamConfig {
        channels: CHANNELS,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Default,
    };

    let writer_clone = Arc::clone(&writer);
    let stream = device.build_input_stream(
        &config,
        move |data: &[f32], _: &_| {
            if let Some(writer) = &mut *writer_clone.lock().unwrap() {
                for &sample in data {
                    writer
                        .write_sample((sample * i16::MAX as f32) as i16)
                        .unwrap();
                }
            }
        },
        move |err| eprintln!("An error occurred on stream: {}", err),
        None,
    )?;

    stream.play()?;

    if tokio::fs::metadata(FIFO_PATH).await.is_ok() {
        tokio::fs::remove_file(FIFO_PATH).await?;
    }
    nix::unistd::mkfifo(FIFO_PATH, nix::sys::stat::Mode::S_IRWXU)?;

    println!("Recording... Stop with:");
    println!("- Press Enter, or");
    println!("- Run: echo x > {}, or", FIFO_PATH);
    println!("- Click the notification");
    println!();

    let cancel_token = CancellationToken::new();

    let start_time = tokio::time::Instant::now();

    let timer_handle = tokio::spawn({
        let cancel_token = cancel_token.clone();
        async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => { break; }
                    _ = interval.tick() => {
                        let elapsed = start_time.elapsed();
                        let minutes = elapsed.as_secs() / 60;
                        let seconds = elapsed.as_secs() % 60;
                        print!("\rRecording length: {:02}:{:02}", minutes, seconds);
                        std::io::Write::flush(&mut std::io::stdout()).unwrap();
                        //tokio::io::stdout().flush().await?;
                    }
                }
            }
            println!("timer exit");
            Ok::<_, anyhow::Error>(())
        }
    });

    let (stdin_tx, mut stdin_rx) = tokio::sync::oneshot::channel::<()>();
    let stdin_handle = tokio::spawn({
        let cancel_token = cancel_token.clone();
        async move {
            let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
            let mut buf = String::new();
            //let mut stdin = tokio::io::stdin();
            //let mut buf = [0u8; 1];
            tokio::select! {
                _ = cancel_token.cancelled() => {}
                _ = stdin.read_line(&mut buf) => {
                    stdin_tx.send(()).map_err(|_| anyhow::anyhow!("Failed to send stdin signal"))?;
                }
                //_ = stdin.read(&mut buf) => {}
            }
            println!("stdin exit");
            Ok::<_, anyhow::Error>(())
        }
    });

    let (fifo_tx, mut fifo_rx) = tokio::sync::oneshot::channel();
    let fifo_handle = tokio::spawn({
        let cancel_token = cancel_token.clone();
        async move {
            println!("fifo open");
            tokio::select! {
                _ = cancel_token.cancelled() => {}
                _ = tokio::fs::File::open(FIFO_PATH) => {
                    fifo_tx.send(()).map_err(|_| anyhow::anyhow!("Failed to send fifo signal"))?;
                }
            }
            /*
            let mut fifo = File::open(FIFO_PATH).await?;
            let mut buf = [0u8; 1];
            println!("fifo select");
            tokio::select! {
                _ = cancel_token.cancelled() => {}
                /*_ = fifo.read(&mut buf) => {
                    fifo_tx.send(()).map_err(|_| anyhow::anyhow!("Failed to send fifo signal"))?;
                }*/
            }
            */
            println!("fifo exit");
            Ok::<_, anyhow::Error>(())
        }
    });

    let (notify_tx, mut notify_rx) = tokio::sync::oneshot::channel();
    let notify_handle = tokio::spawn({
        let mut proc_notify = tokio::process::Command::new("notify-send")
            .args([
                "--hint=string:x-canonical-private-synchronous:rpdictation",
                "--wait",
            ])
            .arg("Recording...")
            .spawn()
            .context("Failed to spawn notify-send")?;

        let cancel_token = cancel_token.clone();
        async move {
            tokio::select! {
                _ = cancel_token.cancelled() => {}
                _ = proc_notify.wait() => {
                    notify_tx.send(()).map_err(|_| anyhow::anyhow!("Failed to send notify signal"))?;
                }
            }
            if let Some(pid) = proc_notify.id() {
                let pid = nix::unistd::Pid::from_raw(pid as i32);
                nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGINT)?;
                proc_notify.wait().await?; // TODO: i have to keep this here - why?
            }
            //println!("notify extra kill");
            //proc_notify.kill().await?;
            //proc_notify.wait().await?;
            println!("notify exit");
            Ok::<_, anyhow::Error>(())
        }
    });

    let source = tokio::select! {
        _ = &mut stdin_rx => "stdin",
        _ = &mut fifo_rx => "fifo",
        _ = &mut notify_rx => "notify",
    };
    println!("Stopped by {}", source);

    cancel_token.cancel();

    /*
        stdin_rx.close();
        fifo_rx.close();
        notify_rx.close();
    */

    println!("joining");
    //timer_handle.await??;
    //stdin_handle.await??;
    //fifo_handle.await??;
    //notify_handle.await??;
    let _ = tokio::try_join!(timer_handle, stdin_handle, fifo_handle, notify_handle)
        .map_err(|_| anyhow::anyhow!("Failed to join"))?;
    println!("joined");

    tokio::fs::remove_file(FIFO_PATH).await?;

    drop(stream);
    if let Some(writer) = writer.lock().unwrap().take() {
        writer.finalize()?;
    }
    drop(writer); // TODO: not really needed

    println!("Recording saved. Analyzing...");

    let file_size = tokio::fs::metadata(RECORDING_FILENAME).await?.len();
    let reader = hound::WavReader::open(RECORDING_FILENAME)?;
    let duration_seconds = reader.duration() as f64 / reader.spec().sample_rate as f64;
    println!("Recording length: {:.1} seconds", duration_seconds);
    println!("File size: {:.1} MB", file_size as f64 / 1_048_576.0);
    let audio_duration = duration_seconds;

    if duration_seconds < MIN_RECORDING_DURATION_SECONDS {
        println!(
            "Recording too short ({:.1} seconds), discarding.",
            duration_seconds
        );
        tokio::fs::remove_file(RECORDING_FILENAME).await?;
        return Ok(());
    }

    println!("\nTranscribing...");

    let client = reqwest::Client::new();
    let file_bytes = tokio::fs::read(RECORDING_FILENAME).await?;
    tokio::fs::remove_file(RECORDING_FILENAME).await?;
    let file_part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(RECORDING_FILENAME)
        .mime_str("audio/wav")?;
    let form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("model", "whisper-1");

    println!("Sending request to OpenAI API...");
    let response = client
        .post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .timeout(std::time::Duration::from_secs(60)) // Increase timeout to 60 seconds
        .send()
        .await
        .context("Failed to send request to OpenAI API")?;

    println!("Got response with status: {}", response.status());
    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!("API error: {}", error_text));
    }

    let result: serde_json::Value = response
        .json()
        .await
        .context("Failed to parse API response as JSON")?;

    let Some(text) = result["text"].as_str() else {
        anyhow::bail!("Failed to get transcription from response");
    };

    println!();
    println!("Transcription:");
    println!("{}", text);

    if args.wtype {
        println!("\nTyping text using wtype...");
        tokio::process::Command::new("wtype")
            .arg(text)
            .status()
            .await
            .context("Failed to run wtype")?;
    }

    // Calculate cost - $0.006 per minute
    let minutes = (audio_duration / 60.0).ceil();
    let cost = minutes * 0.006;

    println!();
    println!("Audio duration: {:.1} seconds", duration_seconds);
    println!("Cost: ${:.4}", cost);

    println!("exit");
    Ok(())
}

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(main_async()).unwrap();
    println!("rt shutdown");
    rt.shutdown_background(); // TODO: fucking hack - this is not graceful shutdown
                              //rt.shutdown_timeout(std::time::Duration::from_secs(10));
    println!("main exit");
}
