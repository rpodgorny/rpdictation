use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::env;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
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

async fn send_notification(message: &str, expire: bool) {
    let expire_time = if expire { "3000" } else { "0" };
    let _ = tokio::process::Command::new("notify-send")
        .args([
            "--hint=string:x-canonical-private-synchronous:rpdictation",
            &format!("--expire-time={}", expire_time),
        ])
        .arg(message)
        .status()
        .await;
}

fn get_pid_path() -> PathBuf {
    let uid = nix::unistd::getuid();
    PathBuf::from(format!("/run/user/{}/rpdictation.pid", uid))
}

async fn stop_recording() -> Result<()> {
    let pid_path = get_pid_path();

    // Check PID file exists
    let pid_str = tokio::fs::read_to_string(&pid_path)
        .await
        .context("No recording in progress (PID file not found)")?;
    let pid = pid_str.trim().parse::<i32>().context("Invalid PID file")?;

    // Check process exists and is rpdictation by reading /proc/<pid>/comm
    let comm_path = format!("/proc/{}/comm", pid);
    let comm = tokio::fs::read_to_string(&comm_path)
        .await
        .context("No recording in progress (process not running)")?;

    if comm.trim() != "rpdictation" {
        // PID was reused by another process, clean up stale file
        tokio::fs::remove_file(&pid_path).await?;
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

async fn is_instance_running() -> Option<i32> {
    let pid_path = get_pid_path();
    let pid_str = tokio::fs::read_to_string(&pid_path).await.ok()?;
    let pid: i32 = pid_str.trim().parse().ok()?;

    let comm_path = format!("/proc/{}/comm", pid);
    let comm = tokio::fs::read_to_string(&comm_path).await.ok()?;

    if comm.trim() == "rpdictation" {
        Some(pid)
    } else {
        // Stale PID file, clean it up
        let _ = tokio::fs::remove_file(&pid_path).await;
        None
    }
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Typing backend to use (e.g., wtype, ydotool)
    #[arg(long, value_name = "TOOL")]
    typer: Option<String>,

    /// Transcription provider: "openai" or "google" (auto-detects based on API key availability if not specified)
    #[arg(long)]
    provider: Option<String>,

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

    /// Press Enter after typing the transcription (requires --typer)
    #[arg(long)]
    enter: bool,
}

#[derive(Subcommand, Clone)]
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
    let command = args.command.clone().unwrap_or(Command::Start);

    match command {
        Command::Stop => {
            return stop_recording().await;
        }
        Command::Toggle => {
            if is_instance_running().await.is_some() {
                return stop_recording().await;
            }
            // Fall through to start recording
        }
        Command::Start => {
            if let Some(pid) = is_instance_running().await {
                anyhow::bail!("Already running (pid {})", pid);
            }
            // Fall through to start recording
        }
    }

    async fn command_exists(name: &str) -> bool {
        tokio::process::Command::new("which")
            .arg(name)
            .stdout(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    if let Some(ref typer) = args.typer {
        if !command_exists(typer).await {
            eprintln!("{} command not found. Please install it.", typer);
            return Ok(());
        }
    }

    // Helper to get OpenAI API key from CLI arg or environment
    fn get_openai_api_key(args: &Args) -> Option<String> {
        // Check CLI argument first
        if let Some(ref key) = args.openai_api_key {
            if !key.is_empty() {
                return Some(key.clone());
            }
        }
        // Check environment variable
        if let Ok(key) = env::var("OPENAI_API_KEY") {
            if !key.is_empty() {
                return Some(key);
            }
        }
        None
    }

    // Create the appropriate provider with auto-detection
    let provider: Box<dyn TranscriptionProvider> = match args.provider.as_deref() {
        Some("openai") => {
            // Explicit --provider openai: require API key
            let api_key = get_openai_api_key(&args).context(
                "OPENAI_API_KEY environment variable not set or --openai-api-key not provided",
            )?;
            eprintln!("Using OpenAI provider");
            Box::new(OpenAIProvider::new(api_key))
        }
        Some("google") => {
            // Explicit --provider google
            eprintln!("Using Google provider");
            Box::new(GoogleProvider::new(
                args.google_api_key.clone(),
                args.language.clone(),
            ))
        }
        Some(other) => {
            anyhow::bail!(
                "Invalid provider '{}'. Valid options: openai, google",
                other
            );
        }
        None => {
            // Auto-detect based on API key availability
            if let Some(api_key) = get_openai_api_key(&args) {
                eprintln!("Using OpenAI provider (API key found)");
                Box::new(OpenAIProvider::new(api_key))
            } else {
                eprintln!("Using Google provider (no OpenAI API key configured)");
                Box::new(GoogleProvider::new(
                    args.google_api_key.clone(),
                    args.language.clone(),
                ))
            }
        }
    };

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
    let path = PathBuf::from(RECORDING_FILENAME);
    let spec = hound::WavSpec {
        channels: CHANNELS,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let wav_writer = tokio::task::spawn_blocking(move || {
        hound::WavWriter::create(path, spec).context("Failed to create WAV file")
    })
    .await
    .context("WAV writer task panicked")??;
    let writer = Arc::new(Mutex::new(Some(wav_writer)));

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
            if let Ok(mut guard) = writer_clone.try_lock() {
                if let Some(writer) = &mut *guard {
                    for &sample in data {
                        writer
                            .write_sample((sample * i16::MAX as f32) as i16)
                            .unwrap();
                    }
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
                        let _ = tokio::io::stdout().flush().await;
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
    send_notification("Saving recording...", false).await;
    let writer_to_finalize = writer.lock().unwrap().take();
    if let Some(w) = writer_to_finalize {
        tokio::task::spawn_blocking(move || w.finalize())
            .await
            .context("WAV writer finalize task panicked")??;
    }
    drop(writer); // TODO: not really needed

    send_notification("Analyzing audio...", false).await;
    println!("Recording saved. Analyzing...");

    let file_size = tokio::fs::metadata(RECORDING_FILENAME).await?.len();
    let duration_seconds = tokio::task::spawn_blocking(|| {
        let reader = hound::WavReader::open(RECORDING_FILENAME)?;
        let duration = reader.duration() as f64 / reader.spec().sample_rate as f64;
        Ok::<_, anyhow::Error>(duration)
    })
    .await
    .context("WAV reader task panicked")??;
    println!("Recording length: {:.1} seconds", duration_seconds);
    println!("File size: {:.1} MB", file_size as f64 / 1_048_576.0);
    let audio_duration = duration_seconds;

    if duration_seconds < MIN_RECORDING_DURATION_SECONDS {
        eprintln!(
            "Recording too short ({:.1} seconds), discarding.",
            duration_seconds
        );
        send_notification("Recording too short, discarding", true).await;
        tokio::fs::remove_file(RECORDING_FILENAME).await?;
        return Ok(());
    }

    send_notification(&format!("Transcribing ({})...", provider.name()), false).await;
    println!("\nTranscribing ({})...", provider.name());

    let file_bytes = tokio::fs::read(RECORDING_FILENAME).await?;
    tokio::fs::remove_file(RECORDING_FILENAME).await?;

    let text = provider.transcribe(&file_bytes, SAMPLE_RATE).await?;

    println!();
    println!("Transcription:");
    println!("{}", text);

    if let Some(ref typer) = args.typer {
        send_notification("Typing text...", false).await;
        println!("\nTyping text using {}...", typer);

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

        // Type the text (and optionally press Enter)
        match typer.as_str() {
            "wtype" => {
                let mut cmd = tokio::process::Command::new("wtype");
                cmd.arg(&text);
                if args.enter {
                    cmd.arg("-k").arg("Return");
                }
                cmd.status().await.context("Failed to run wtype")?;
            }
            "ydotool" => {
                tokio::process::Command::new("ydotool")
                    .args(["type", "--", &text])
                    .status()
                    .await
                    .context("Failed to run ydotool")?;
                if args.enter {
                    tokio::process::Command::new("ydotool")
                        .args(["key", "28:1", "28:0"])
                        .status()
                        .await
                        .context("Failed to run ydotool key")?;
                }
            }
            _ => {
                eprintln!("Unknown typer '{}'. Supported: wtype, ydotool", typer);
                return Ok(());
            }
        }

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
    send_notification(&format!("Done: {}", preview), true).await;

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
    // Load .env file before starting async runtime (blocking but only at startup)
    if std::path::Path::new(".env").exists() {
        println!("loading environment from .env");
        if let Err(e) = dotenvy::dotenv() {
            eprintln!("Warning: Failed to load .env file: {}", e);
        }
    }

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
