use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::env;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncBufReadExt;
use tokio::signal::unix::{signal, SignalKind};
use tokio_util::sync::CancellationToken;

mod audio;
mod focus;
mod providers;
use focus::FocusProvider;
use providers::{google::GoogleProvider, openai::OpenAIProvider, TranscriptionProvider};

const SAMPLE_RATE: u32 = 16000;
const CHANNELS: u16 = 1;
const MIN_RECORDING_DURATION_SECONDS: f64 = 1.0;

const FIFO_PATH: &str = "/tmp/rpdictation_stop";
const RECORDING_FILENAME: &str = "/tmp/rpdictation.wav";

fn send_notification(message: &str, expire: bool) {
    let expire_time = if expire { "3000" } else { "0" };
    let _ = std::process::Command::new("notify-send")
        .args([
            "--hint=string:x-canonical-private-synchronous:rpdictation",
            &format!("--expire-time={}", expire_time),
        ])
        .arg(message)
        .spawn();
}

fn get_pid_path() -> PathBuf {
    let uid = nix::unistd::getuid();
    PathBuf::from(format!("/run/user/{}/rpdictation.pid", uid))
}

fn stop_recording() -> Result<()> {
    let pid_path = get_pid_path();

    // Check PID file exists
    let pid_str = std::fs::read_to_string(&pid_path)
        .context("No recording in progress (PID file not found)")?;
    let pid = pid_str.trim().parse::<i32>().context("Invalid PID file")?;

    // Check process exists and is rpdictation by reading /proc/<pid>/comm
    let comm_path = format!("/proc/{}/comm", pid);
    let comm = std::fs::read_to_string(&comm_path)
        .context("No recording in progress (process not running)")?;

    if comm.trim() != "rpdictation" {
        // PID was reused by another process, clean up stale file
        std::fs::remove_file(&pid_path)?;
        anyhow::bail!(
            "No recording in progress (stale PID, was reused by '{}')",
            comm.trim()
        );
    }

    // Send signal
    kill(Pid::from_raw(pid), Signal::SIGUSR1).context("Failed to send stop signal")?;

    println!("Stop signal sent to recording process");
    Ok(())
}

fn is_instance_running() -> Option<i32> {
    let pid_path = get_pid_path();
    let pid_str = std::fs::read_to_string(&pid_path).ok()?;
    let pid: i32 = pid_str.trim().parse().ok()?;

    let comm_path = format!("/proc/{}/comm", pid);
    let comm = std::fs::read_to_string(&comm_path).ok()?;

    if comm.trim() == "rpdictation" {
        Some(pid)
    } else {
        // Stale PID file, clean it up
        let _ = std::fs::remove_file(&pid_path);
        None
    }
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Use wtype to type out the transcription
    #[arg(long)]
    wtype: bool,

    /// Transcription provider: "openai" or "google"
    #[arg(long, default_value = "openai")]
    provider: String,

    /// OpenAI API key (overrides OPENAI_API_KEY environment variable)
    #[arg(long)]
    openai_api_key: Option<String>,

    /// Google API key (optional, uses default Chromium key if not provided)
    #[arg(long)]
    google_api_key: Option<String>,

    /// Language code for Google provider (e.g., en-us, cs-CZ)
    #[arg(long, default_value = "en-us")]
    language: String,

    /// Track window focus and restore it before typing
    #[arg(long)]
    track_window: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Start recording (default if no command specified)
    Start,
    /// Stop a running recording
    Stop,
    /// Toggle recording (start if not running, stop if running)
    Toggle,
}

async fn main_async() -> Result<()> {
    let args = Args::parse();

    // Determine effective command (default to Start)
    let command = args.command.unwrap_or(Command::Start);

    match command {
        Command::Stop => {
            return stop_recording();
        }
        Command::Toggle => {
            if is_instance_running().is_some() {
                return stop_recording();
            }
            // Fall through to start recording
        }
        Command::Start => {
            if let Some(pid) = is_instance_running() {
                anyhow::bail!("Already running (pid {})", pid);
            }
            // Fall through to start recording
        }
    }

    if args.wtype
        && !tokio::process::Command::new("which")
            .arg("wtype")
            .stdout(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    {
        eprintln!("wtype command not found. Please install it to use this feature.");
        return Ok(());
    }

    if tokio::fs::try_exists(".env").await? {
        println!("loading environment from .env");
        dotenvy::dotenv()?;
    }

    // Create the appropriate provider
    let provider: Box<dyn TranscriptionProvider> = match args.provider.as_str() {
        "google" => Box::new(GoogleProvider::new(args.google_api_key, args.language)),
        _ => {
            let api_key = match &args.openai_api_key {
                Some(key) => key.clone(),
                None => env::var("OPENAI_API_KEY").context(
                    "OPENAI_API_KEY environment variable not set or --openai-api-key not provided",
                )?,
            };
            Box::new(OpenAIProvider::new(api_key))
        }
    };

    eprintln!("Using provider: {}", provider.name());

    // Initialize focus provider if tracking is enabled
    let focus_provider: Option<Box<dyn FocusProvider>> = if args.track_window {
        match focus::detect_focus_provider().await {
            Some(fp) => {
                eprintln!("Using focus provider: {}", fp.name());
                Some(fp)
            }
            None => {
                eprintln!("Warning: --track-window enabled but no compositor detected, focus tracking disabled");
                None
            }
        }
    } else {
        None
    };

    // Capture focused window at recording start
    let saved_window_id = if let Some(ref fp) = focus_provider {
        match fp.get_focused_window().await {
            Ok(wid) => {
                if let Some(ref w) = wid {
                    eprintln!("Captured window ID: {:?}", w);
                }
                wid
            }
            Err(e) => {
                eprintln!("Warning: Failed to capture focused window: {}", e);
                None
            }
        }
    } else {
        None
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

    // Write PID file
    let pid_path = get_pid_path();
    tokio::fs::write(&pid_path, std::process::id().to_string()).await?;

    let stdin_is_tty = std::io::stdin().is_terminal();

    println!("Recording... Stop with:");
    println!("- Run: rpdictation stop, or");
    if stdin_is_tty {
        println!("- Press Enter, or");
    }
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

                        // Update notification (fire-and-forget, uses same hint to replace)
                        let _ = tokio::process::Command::new("notify-send")
                            .args([
                                "--hint=string:x-canonical-private-synchronous:rpdictation",
                                "--expire-time=0",
                            ])
                            .arg(format!("Recording {:02}:{:02}", minutes, seconds))
                            .spawn();

                        // Keep terminal output
                        print!("\rRecording length: {:02}:{:02}", minutes, seconds);
                        std::io::Write::flush(&mut std::io::stdout()).unwrap();
                        //tokio::io::stdout().flush().await?;
                    }
                }
            }
            eprintln!("timer exit");
            Ok::<_, anyhow::Error>(())
        }
    });

    let (stdin_tx, mut stdin_rx) = tokio::sync::oneshot::channel::<()>();
    let stdin_handle = tokio::spawn({
        let cancel_token = cancel_token.clone();
        async move {
            if !stdin_is_tty {
                // Not a TTY, just wait for cancellation
                cancel_token.cancelled().await;
                eprintln!("stdin exit (not a tty)");
                return Ok::<_, anyhow::Error>(());
            }

            let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
            let mut buf = String::new();
            tokio::select! {
                _ = cancel_token.cancelled() => {}
                _ = stdin.read_line(&mut buf) => {
                    stdin_tx.send(()).map_err(|_| anyhow::anyhow!("Failed to send stdin signal"))?;
                }
            }
            eprintln!("stdin exit");
            Ok::<_, anyhow::Error>(())
        }
    });

    let (fifo_tx, mut fifo_rx) = tokio::sync::oneshot::channel();
    let fifo_handle = tokio::spawn({
        let cancel_token = cancel_token.clone();
        async move {
            eprintln!("fifo open");
            tokio::select! {
                _ = cancel_token.cancelled() => {}
                _ = tokio::fs::File::open(FIFO_PATH) => {
                    fifo_tx.send(()).map_err(|_| anyhow::anyhow!("Failed to send fifo signal"))?;
                }
            }
            /*
            let mut fifo = File::open(FIFO_PATH).await?;
            let mut buf = [0u8; 1];
            eprintln!("fifo select");
            tokio::select! {
                _ = cancel_token.cancelled() => {}
                /*_ = fifo.read(&mut buf) => {
                    fifo_tx.send(()).map_err(|_| anyhow::anyhow!("Failed to send fifo signal"))?;
                }*/
            }
            */
            eprintln!("fifo exit");
            Ok::<_, anyhow::Error>(())
        }
    });

    let (notify_tx, mut notify_rx) = tokio::sync::oneshot::channel();
    let notify_handle = tokio::spawn({
        let mut proc_notify = tokio::process::Command::new("notify-send")
            .args([
                "--hint=string:x-canonical-private-synchronous:rpdictation",
                "--expire-time=0",
                "--wait",
                "--action=stop=Stop",
            ])
            .arg("Recording 00:00")
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
            //eprintln!("notify extra kill");
            //proc_notify.kill().await?;
            //proc_notify.wait().await?;
            eprintln!("notify exit");
            Ok::<_, anyhow::Error>(())
        }
    });

    let (signal_tx, mut signal_rx) = tokio::sync::oneshot::channel();
    let signal_handle = tokio::spawn({
        let cancel_token = cancel_token.clone();
        async move {
            let mut sig =
                signal(SignalKind::user_defined1()).context("Failed to create signal handler")?;
            tokio::select! {
                _ = cancel_token.cancelled() => {}
                _ = sig.recv() => {
                    signal_tx.send(()).ok();
                }
            }
            eprintln!("signal exit");
            Ok::<_, anyhow::Error>(())
        }
    });

    let source = tokio::select! {
        _ = &mut stdin_rx => "stdin",
        _ = &mut fifo_rx => "fifo",
        _ = &mut notify_rx => "notify",
        _ = &mut signal_rx => "signal",
    };
    eprintln!("Stopped by {}", source);

    cancel_token.cancel();

    /*
        stdin_rx.close();
        fifo_rx.close();
        notify_rx.close();
    */

    eprintln!("joining");
    //timer_handle.await??;
    //stdin_handle.await??;
    //fifo_handle.await??;
    //notify_handle.await??;
    let _ = tokio::try_join!(
        timer_handle,
        stdin_handle,
        fifo_handle,
        notify_handle,
        signal_handle
    )
    .map_err(|_| anyhow::anyhow!("Failed to join"))?;
    eprintln!("joined");

    tokio::fs::remove_file(FIFO_PATH).await?;
    let _ = tokio::fs::remove_file(get_pid_path()).await;

    drop(stream);
    send_notification("Saving recording...", false);
    if let Some(writer) = writer.lock().unwrap().take() {
        writer.finalize()?;
    }
    drop(writer); // TODO: not really needed

    send_notification("Analyzing audio...", false);
    println!("Recording saved. Analyzing...");

    let file_size = tokio::fs::metadata(RECORDING_FILENAME).await?.len();
    let reader = hound::WavReader::open(RECORDING_FILENAME)?;
    let duration_seconds = reader.duration() as f64 / reader.spec().sample_rate as f64;
    println!("Recording length: {:.1} seconds", duration_seconds);
    println!("File size: {:.1} MB", file_size as f64 / 1_048_576.0);
    let audio_duration = duration_seconds;

    if duration_seconds < MIN_RECORDING_DURATION_SECONDS {
        eprintln!(
            "Recording too short ({:.1} seconds), discarding.",
            duration_seconds
        );
        send_notification("Recording too short, discarding", true);
        tokio::fs::remove_file(RECORDING_FILENAME).await?;
        return Ok(());
    }

    send_notification("Transcribing...", false);
    println!("\nTranscribing...");

    let file_bytes = tokio::fs::read(RECORDING_FILENAME).await?;
    tokio::fs::remove_file(RECORDING_FILENAME).await?;

    let text = provider.transcribe(&file_bytes, SAMPLE_RATE).await?;

    println!();
    println!("Transcription:");
    println!("{}", text);

    if args.wtype {
        send_notification("Typing text...", false);
        println!("\nTyping text using wtype...");

        // Handle focus tracking if enabled
        let restore_window_id = if let (Some(ref fp), Some(ref saved_wid)) =
            (&focus_provider, &saved_window_id)
        {
            // Get current focused window
            let current_wid = fp.get_focused_window().await.ok().flatten();

            if current_wid.as_ref() != Some(saved_wid) {
                // Focus changed, need to switch back
                eprintln!(
                    "Focus changed from {:?} to {:?}, switching back",
                    saved_wid, current_wid
                );

                // Try to focus the original window
                match fp.set_focused_window(saved_wid).await {
                    Ok(true) => {
                        eprintln!("Switched focus to original window");
                        // Remember current window for restoration after typing
                        current_wid
                    }
                    Ok(false) => {
                        eprintln!(
                            "Warning: Failed to switch to original window (may be closed), typing into current"
                        );
                        None
                    }
                    Err(e) => {
                        eprintln!("Warning: Error switching focus: {}, typing into current", e);
                        None
                    }
                }
            } else {
                // Focus unchanged, no need to restore
                None
            }
        } else {
            None
        };

        // Type the text
        tokio::process::Command::new("wtype")
            .arg(&text)
            .status()
            .await
            .context("Failed to run wtype")?;

        // Restore focus to the window that was focused before we switched
        if let (Some(ref fp), Some(ref restore_wid)) = (&focus_provider, &restore_window_id) {
            eprintln!("Restoring focus to {:?}", restore_wid);
            if let Err(e) = fp.set_focused_window(restore_wid).await {
                eprintln!("Warning: Failed to restore focus: {}", e);
            }
        }
    }

    // Show first ~50 chars of transcription in notification
    let preview = if text.len() > 50 {
        format!("{}...", &text[..50])
    } else {
        text.clone()
    };
    send_notification(&format!("Done: {}", preview), true);

    println!();
    println!("Audio duration: {:.1} seconds", duration_seconds);
    if let Some(cost_per_min) = provider.cost_per_minute() {
        let minutes = (audio_duration / 60.0).ceil();
        let cost = minutes * cost_per_min;
        println!("Cost: ${:.4}", cost);
    }

    eprintln!("exit");
    Ok(())
}

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let result = rt.block_on(main_async());

    eprintln!("rt shutdown");
    rt.shutdown_background(); // TODO: fucking hack - this is not graceful shutdown
                              //rt.shutdown_timeout(std::time::Duration::from_secs(10));
    eprintln!("main exit");

    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }
}
