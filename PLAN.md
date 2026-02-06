# Implementation Plan: Windows OS Support

> Generated from SPEC.md on 2026-02-06

## Overview
Port RPDictation to Windows by gating Linux-specific code behind `#[cfg(unix)]`, adding Windows-specific implementations behind `#[cfg(windows)]`, and making shared code platform-neutral. The `cpal` audio capture and HTTP-based transcription providers already work cross-platform and need no changes.

## Phase 1: Cargo.toml and Dependency Setup
- [x] Move `nix` to `[target.'cfg(unix)'.dependencies]`
- [x] Add `windows` crate under `[target.'cfg(windows)'.dependencies]` with features: `Win32_UI_Input_KeyboardAndMouse`, `Win32_Foundation`, `Win32_System_Threading`, `Win32_Security`
- [x] ~~Remove `tokio`'s `signal` feature~~ — kept `signal` in main tokio features since it compiles on both platforms (provides `ctrl_c()` on Windows); unix-specific `tokio::signal::unix` imports will be gated in Phase 3

## Phase 2: Platform-Neutral Temp Paths
- [x] Replace `const RECORDING_FILENAME: &str = "/tmp/rpdictation.wav"` with a function that uses `std::env::temp_dir().join("rpdictation.wav")`
- [x] Gate `const FIFO_PATH` behind `#[cfg(unix)]` (only used on Linux)

## Phase 3: Gate Linux-Specific Code in main.rs
- [x] Gate `use nix::*` imports behind `#[cfg(unix)]`
- [x] Gate `use tokio::signal::unix::*` behind `#[cfg(unix)]`
- [x] Gate `send_notification()` behind `#[cfg(unix)]`; on Windows, make it a no-op or skip notification calls
- [x] Gate `get_pid_path()` behind `#[cfg(unix)]`
- [x] Gate `stop_recording()` behind `#[cfg(unix)]`; create Windows version using named event
- [x] Gate `is_instance_running()` behind `#[cfg(unix)]`; create Windows version using named mutex
- [x] Gate the FIFO setup/teardown code behind `#[cfg(unix)]`
- [x] Gate the SIGUSR1 signal handler behind `#[cfg(unix)]`
- [x] Gate the `notify-send` notification process spawn behind `#[cfg(unix)]`
- [x] Gate the `nix::sys::signal::kill` call for killing notify-send behind `#[cfg(unix)]`
- [x] Gate the `wtype` availability check behind `#[cfg(unix)]`
- [x] Gate the `wtype` execution behind `#[cfg(unix)]`

## Phase 4: Windows-Specific Implementations

### 4a: Windows IPC Module (`src/ipc.rs`)
- [x] Create named event (`Global\rpdictation_stop`) using `CreateEventW`
- [x] Implement `wait_for_stop_event()` — `StopEvent::wait()` using `tokio::task::spawn_blocking` with `WaitForSingleObject` polling at 100ms with cancellation token
- [x] Implement `signal_stop_event()` — `ipc::signal_stop()` opens existing event and calls `SetEvent`
- [x] Implement `stop_recording()` for Windows — calls `ipc::signal_stop()`

### 4b: Windows Instance Detection
- [x] Create named mutex (`Global\rpdictation_instance`) using `CreateMutexW`
- [x] Implement `is_instance_running()` for Windows — try to create mutex, check `ERROR_ALREADY_EXISTS`
- [x] Hold the mutex handle for the lifetime of the recording (drop on exit)

### 4c: Windows Text Typing (`src/typing.rs`)
- [x] Implement `type_text(text: &str)` using `SendInput` with `KEYEVENTF_UNICODE`
  - For each character: send `KEYEVENTF_UNICODE` key-down, then key-up
  - Handle surrogate pairs for characters outside the BMP (U+10000+)
- [x] Implement `press_enter()` using `SendInput` with `VK_RETURN`
- [x] ~~Add small delay between keystrokes if needed for reliability~~ — all inputs sent in a single `SendInput` call (batch), which is the recommended approach; delays can be added later if empirical testing shows issues

### 4d: Windows Recording Flow in main.rs
- [x] On Windows, after transcription, always call `type_text()` (no `--wtype` flag check needed)
- [x] If `--enter` is set, call `press_enter()` after `type_text()`
- [x] If `--track-window` is set, print warning "focus tracking not supported on Windows" and continue
- [x] If `--wtype` is set on Windows, silently ignore it (don't error)
- [x] Replace the `tokio::select!` stop mechanism: on Windows, select between stdin and named event (no FIFO, no SIGUSR1, no notification)

## Phase 5: Focus Module Platform Gating
- [x] Gate `focus/niri.rs` behind `#[cfg(unix)]`
- [x] Gate `pub mod niri` in `focus/mod.rs` behind `#[cfg(unix)]`
- [x] `detect_focus_provider()` returns `None` on Windows (or gate entirely)

## Phase 6: CI Update
- [x] Update `.github/workflows/rust.yml` to use a build matrix: `[ubuntu-latest, windows-latest]`
- [x] Conditionally install ALSA only on Linux (`if: runner.os == 'Linux'`)
- [x] Verify `cargo build` and `cargo test` pass on both (verified on Linux; Windows will be verified when CI runs)

## Phase 7: Testing
- [x] Verify compilation on Linux (`cargo build` — no regressions)
- [x] Verify compilation on Windows (`cargo build --target x86_64-pc-windows-gnu` cross-compiled from Linux — zero errors, zero warnings; also fixed typing.rs pattern mismatch and gated focus module behind `#[cfg(unix)]`)
- [ ] Test microphone recording on Windows
- [ ] Test transcription (OpenAI or Google) on Windows
- [ ] Test auto-typing on Windows (open Notepad, run rpdictation, verify text appears)
- [ ] Test `--enter` flag on Windows
- [ ] Test `rpdictation stop` on Windows
- [ ] Test `rpdictation toggle` on Windows
- [ ] Test `--wtype` is silently ignored on Windows
- [ ] Test `--track-window` prints warning on Windows
- [x] Verify Linux still works end-to-end (no regressions) — `cargo build`, `cargo test`, `cargo clippy` all pass with zero warnings

## Phase 8: Cleanup
- [x] Ensure no dead-code warnings on either platform (all `#[cfg]` gates correct) — verified on both Linux and Windows (cross-compiled): `cargo build` produces zero warnings
- [x] Review all `#[allow(unused)]` if any were added temporarily — none found, clean
- [x] Verify `cargo clippy` passes on both platforms — verified on both Linux and Windows (cross-compiled): `cargo clippy` produces zero warnings

## Verification Checklist
- [ ] All acceptance criteria from SPEC.md are met
- [ ] CI passes on both ubuntu-latest and windows-latest
- [x] No platform-specific code runs on the wrong platform (verified via code review: all unix-specific code gated with `#[cfg(unix)]`, all windows-specific code gated with `#[cfg(windows)]`)
- [ ] Unicode typing works (test with accented characters)
- [ ] `rpdictation stop` works across processes on Windows

## Files to Modify
- `Cargo.toml` — platform-conditional dependencies, add `windows` crate
- `src/main.rs` — `#[cfg]` gates throughout, Windows recording flow, temp path function
- `src/focus/mod.rs` — gate niri module behind `#[cfg(unix)]`
- `src/focus/niri.rs` — gate entire file behind `#[cfg(unix)]` (or just the mod declaration)
- `.github/workflows/rust.yml` — add Windows to build matrix

## Files to Create
- `src/typing.rs` — Windows `SendInput` text typing implementation (gated behind `#[cfg(windows)]`)
- `src/ipc.rs` — Windows named event/mutex IPC (gated behind `#[cfg(windows)]`); could also hold Linux-specific IPC if refactored, but for minimal port, Windows-only is fine
