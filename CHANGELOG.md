# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

### Changed

### Deprecated

### Removed

### Fixed

### Security

## [0.1.0] - 2026-04-15

Initial release. RPDictation is a lightweight Linux speech-to-text tool that
records from your microphone and transcribes via one of several cloud providers.

### Added
- Real-time audio recording from the default microphone, kept entirely in
  memory (no temporary WAV on disk).
- Transcription via four providers: OpenAI Whisper, Mistral Voxtral, Groq
  Whisper, and Google's Chromium Speech API (free, no API key required).
- Provider fallback chain: `--provider` accepts a comma-separated list and
  retries the next entry on failure, so a flaky API or transient outage
  doesn't cost a dictation.
- Auto-detected provider chain when `--provider` is omitted: built from every
  provider whose API key is available, ordered cheapest-first (Groq, OpenAI,
  Mistral), with Google always appended as the final fallback.
- Multiple ways to stop a recording: press Enter, `rpdictation stop`,
  `rpdictation toggle`, SIGUSR1, FIFO command, or clicking the desktop
  notification.
- Optional text insertion into the focused application via `--typer=wtype`
  or `--typer=ydotool`, with an `--enter` flag to press Enter after typing.
- `--track-window` flag that remembers the window focused when recording
  started and restores focus there before typing (Niri compositor).
- Desktop notifications showing recording duration, size, and transcription
  status, including error notifications when a provider fails.
- Cost tracking and reporting for paid providers (OpenAI, Mistral).
- Configuration via `.env` file or plain environment variables.

### Security
- Uses `reqwest` 0.12 to avoid the unmaintained `rustls-pemfile` 1.x chain.
- Uses `bytes` 1.11.1 to pick up the fix for RUSTSEC-2026-0007.
