# Windows OS Support

## Summary
Add minimal Windows support to RPDictation so that microphone recording, transcription, and auto-typing work on Windows. Linux functionality remains unchanged.

## Background
RPDictation currently only runs on Linux. It relies heavily on Unix-specific APIs (`nix` crate for signals/FIFOs/PIDs, `wtype` for Wayland text input, `notify-send` for desktop notifications, niri compositor for focus tracking). A minimal Windows port needs to replace the platform-specific pieces while keeping the cross-platform core (audio capture via `cpal`, transcription via HTTP APIs) intact.

## Requirements

### Functional Requirements

#### Core (must work on Windows)
- **Microphone input**: Record audio from the default input device using `cpal` (already cross-platform).
- **Transcription**: Both OpenAI and Google providers work identically (HTTP-only, no platform code).
- **Auto-typing**: On Windows, transcription is **always auto-typed** using the Windows `SendInput` API with `KEYEVENTF_UNICODE` flag (supports all languages/characters). No `--wtype` flag needed on Windows.
- **Enter key**: The `--enter` flag works on Windows, sending `VK_RETURN` via `SendInput` after the transcription text.
- **Stop recording**: Stdin (Enter key) works as a stop mechanism. Additionally, `rpdictation stop` and `rpdictation toggle` work via a **Windows named event** (`Global\rpdictation_stop`).
- **Single-instance detection**: Use a **Windows named mutex** (`Global\rpdictation_instance`) to prevent multiple simultaneous recordings. Automatically released on crash.
- **Temp file paths**: Use `std::env::temp_dir()` for the WAV recording file path on all platforms (replaces hardcoded `/tmp/`).

#### Linux behavior (unchanged)
- `--wtype` flag continues to work on Linux using the `wtype` command.
- `--wtype` flag is accepted but **is a no-op** on Windows (does not cause an error).
- FIFO, SIGUSR1, `notify-send`, PID file, niri focus tracking — all continue to work on Linux.

#### Explicitly out of scope for minimal port
- Windows desktop notifications (no toast notifications).
- Windows focus/window tracking (no `--track-window` on Windows).
- Cross-compilation from Linux to Windows (nice to have, not required).

### Non-Functional Requirements
- The project must compile on both `x86_64-pc-windows-msvc` and Linux targets.
- CI (GitHub Actions) builds and tests on both `ubuntu-latest` and `windows-latest`.
- No runtime panics from platform-specific code running on the wrong OS.

## Design Decisions

### Code Structure
Use `#[cfg(unix)]` / `#[cfg(windows)]` with platform-specific modules:
- Platform-conditional dependencies in `Cargo.toml` (`nix` under `[target.'cfg(unix)'.dependencies]`, `windows` crate under `[target.'cfg(windows)'.dependencies]`).
- Shared traits/interfaces where both platforms need different implementations (IPC, text input).
- `#[cfg]` attributes on imports, functions, and code blocks in `main.rs` for platform-specific logic.

### Windows Text Typing
- Use the `windows` crate (official Microsoft crate) for `SendInput` with `INPUT_KEYBOARD` and `KEYEVENTF_UNICODE`.
- Each character of the transcription is sent as a Unicode keystroke (key down + key up).
- `--enter` sends an additional `VK_RETURN` keystroke.

### Windows IPC (Stop Signal)
- Use `CreateEventW` / `SetEvent` / `WaitForSingleObject` from the `windows` crate.
- Event name: `Global\rpdictation_stop`.
- Recording process creates the event and waits on it (with timeout for polling).
- `rpdictation stop` opens the existing event and signals it.

### Windows Single-Instance Detection
- Use `CreateMutexW` from the `windows` crate.
- Mutex name: `Global\rpdictation_instance`.
- If mutex already exists (`GetLastError() == ERROR_ALREADY_EXISTS`), another instance is running.
- `rpdictation stop`/`toggle` checks the mutex to determine if an instance is running.

## Constraints
- The `nix` crate does not compile on Windows — must be gated behind `#[cfg(unix)]`.
- `tokio::signal::unix` is unavailable on Windows — must be gated.
- The `windows` crate is only available on Windows — must be gated behind `#[cfg(windows)]`.
- `cpal` on Windows uses WASAPI by default; no additional system dependencies needed (unlike ALSA on Linux).
- GitHub Actions `windows-latest` does not have ALSA; the build matrix must not install Linux-specific packages on Windows.

## Edge Cases

| Edge Case | Expected Behavior |
|-----------|-------------------|
| No audio input device on Windows | `cpal` returns error, displayed to user with "Failed to get default input device" (same as Linux) |
| SendInput fails (e.g., typing into UAC-elevated window) | `SendInput` returns 0 for failed inputs; log a warning but don't crash |
| Named mutex left from a crashed process | Windows automatically releases named mutexes when the owning process exits; no stale state |
| Named event left from a crashed process | Events are reference-counted by Windows; auto-cleaned when no handles remain open |
| `--wtype` passed on Windows | Accepted silently, treated as no-op. Auto-typing via SendInput happens regardless |
| `--track-window` passed on Windows | Print warning "focus tracking not supported on Windows", continue without it |
| `--enter` without `--wtype` on Windows | `--enter` works (auto-typing is always on), sends Enter after transcription |
| `--enter` without `--wtype` on Linux | Same behavior as before (enter requires wtype, already validated) |
| Very long transcription text on Windows | SendInput handles it fine; no practical character limit |
| Non-ASCII/Unicode transcription on Windows | `KEYEVENTF_UNICODE` flag handles full Unicode range including CJK, accented chars, emoji |
| `rpdictation stop` when no instance running on Windows | Named event doesn't exist; print "No recording in progress" error (same UX as Linux) |
| FIFO path constant on Windows | `FIFO_PATH` is only used behind `#[cfg(unix)]`; Windows uses named event instead |
| PID path on Windows | PID file logic is only used behind `#[cfg(unix)]`; Windows uses named mutex instead |

## Acceptance Criteria
- [ ] `cargo build` succeeds on Windows (x86_64-pc-windows-msvc)
- [ ] `cargo build` succeeds on Linux (unchanged)
- [ ] `cargo test` passes on both platforms
- [ ] Microphone recording works on Windows (WAV file is created with valid audio)
- [ ] OpenAI transcription works on Windows
- [ ] Google transcription works on Windows
- [ ] Transcribed text is auto-typed into the focused window on Windows via SendInput
- [ ] Unicode characters (e.g., accented, CJK) are typed correctly on Windows
- [ ] `--enter` sends Enter keystroke after typing on Windows
- [ ] `rpdictation stop` stops a running recording on Windows via named event
- [ ] `rpdictation toggle` works on Windows
- [ ] Only one instance can record at a time on Windows (named mutex)
- [ ] `--wtype` is silently ignored on Windows
- [ ] `--track-window` prints a warning on Windows and continues
- [ ] GitHub Actions CI builds on both `ubuntu-latest` and `windows-latest`
- [ ] No compilation warnings related to dead code on either platform (proper `#[cfg]` gating)
- [ ] All existing Linux functionality continues to work unchanged
