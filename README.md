# RPDictation

![License](https://img.shields.io/badge/license-GPL-blue.svg)

RPDictation is a simple, efficient speech-to-text transcription tool for Linux that provides accurate transcriptions directly from your microphone. It supports multiple speech recognition providers including OpenAI's Whisper API, Mistral's Voxtral API, Groq's Whisper API, and Google's Chromium Speech API (free but limited alternative).

## Features

- **Real-time audio recording** from your default microphone
- **Multiple transcription providers**:
  - OpenAI's Whisper API (high quality, paid)
  - Mistral's Voxtral API (high quality, half the price of OpenAI)
  - Groq's Whisper API (very fast, very cheap)
  - Google Chromium Speech API (free alternative, limited)
- **Provider fallback chain** — pass a comma-separated list to `--provider` (e.g. `google,google,groq,mistral`) and rpdictation will retry the next provider on failure, so a flaky API or transient outage doesn't cost you a dictation
- **Multiple ways to control recording**:
  - Press Enter
  - Run `rpdictation stop` or `rpdictation toggle`
  - Send SIGUSR1 to the recording process
  - Send a command to a FIFO
  - Click a desktop notification
- **Optional text insertion** directly into applications using `wtype` or `ydotool` (`--typer`)
- **Clipboard paste mode** (`--paste`) that inserts text via `wl-copy` + Shift+Insert instead of direct typing — works around `wtype`'s broken keymap handling on Niri and `ydotool`'s diacritic stripping. Implicitly enabled for non-English languages.
- **Optional Enter key press** after typing (`--enter`)
- **Window focus tracking** to ensure text is typed into the correct window
- **Cost tracking** for API usage (OpenAI and Mistral providers)
- **Clean, simple interface** with recording time display
- **Environment variable support** via `.env` file or plain environment

## Installation

### Prerequisites

- Rust and Cargo
- Linux with PulseAudio/PipeWire
- API key requirements (depends on provider):
  - **OpenAI provider**: Requires OpenAI API key
  - **Mistral provider**: Requires Mistral API key
  - **Groq provider**: Requires Groq API key
  - **Google provider**: Works without API key (uses default Chromium key)
- (Optional) `wtype` or `ydotool` for text insertion capability

### Build from source

```bash
git clone https://github.com/yourusername/rpdictation.git
cd rpdictation
cargo build --release
```

The executable will be available at `./target/release/rpdictation`

## Usage

### Basic usage with Google (free)

```bash
./rpdictation --provider google
```

With language specification:

```bash
./rpdictation --provider google --language cs-CZ
```

**Note:** The Google provider requires explicit language specification and does not support automatic language detection. If you need automatic language detection, use the OpenAI provider which includes this capability.

### Basic usage with Mistral

Using environment variable:

```bash
export MISTRAL_API_KEY=your_api_key_here
./rpdictation --provider mistral
```

Or specify the API key directly:

```bash
./rpdictation --provider mistral --mistral-api-key your_api_key_here
```

### Basic usage with Groq

Using environment variable:

```bash
export GROQ_API_KEY=your_api_key_here
./rpdictation --provider groq
```

Or specify the API key directly:

```bash
./rpdictation --provider groq --groq-api-key your_api_key_here
```

### Basic usage with OpenAI

Using environment variable:

```bash
export OPENAI_API_KEY=your_api_key_here
./rpdictation --provider openai
```

Or specify the API key directly:

```bash
./rpdictation --provider openai --openai-api-key your_api_key_here
```

### Environment file

You can create a `.env` file in the project directory:

```bash
OPENAI_API_KEY=your_api_key_here
MISTRAL_API_KEY=your_api_key_here
GROQ_API_KEY=your_api_key_here
```

Then run:

```bash
./rpdictation
```

**Note:** If `--provider` is omitted, rpdictation builds a best-effort fallback chain from every provider whose API key is available, ordered cheapest-first: Groq, then OpenAI, then Mistral. Google is always appended as the final fallback (it works without an API key via the built-in Chromium key).

### Provider fallback chain

`--provider` accepts a comma-separated list. Each entry is tried in order and the first one that succeeds wins; on failure, rpdictation logs the error and moves on to the next. An entry may repeat if you want more than one attempt at the same provider.

```bash
./rpdictation --provider google,google,groq,mistral,mistral
```

The example above tries Google twice, then Groq once, then Mistral twice, and only fails if all five attempts fail. Useful for pairing a free/cheap primary with a paid backup — e.g. let Google do most of the work and fall back to a paid provider only when it hiccups. Cost reporting reflects the provider that actually produced the transcript.

### Text insertion mode

To automatically insert the transcribed text using `wtype`:

```bash
./rpdictation --typer=wtype
```

Or using `ydotool`:

```bash
./rpdictation --typer=ydotool
```

To also press Enter after typing the transcription:

```bash
./rpdictation --typer=wtype --enter
```

### Clipboard paste mode

The `--paste` flag inserts the transcribed text via the clipboard (`wl-copy` + Shift+Insert) instead of the typer's direct-type path:

```bash
./rpdictation --typer=wtype --paste
```

This works around two issues:

- `wtype` direct-type is broken on Niri because of how the compositor handles keymaps.
- `ydotool` direct-type strips diacritics, so non-ASCII text comes out mangled.

Paste mode is implicitly enabled whenever `--language` is set to anything that doesn't start with `en`, so non-English dictations get the correct characters by default. `wl-copy` must be available for this to work.

### Window focus tracking

When using `--typer`, you may switch to a different window while recording or during transcription. The `--track-window` flag ensures text is typed into the window that was focused when you started recording:

```bash
./rpdictation --typer=wtype --track-window
```

This captures the focused window when recording starts. Before typing, it switches focus back to that window, types the text, then restores focus to where you were. Currently supports the Niri compositor.

### During recording

While recording, you can:
- Run `rpdictation stop` in another terminal
- Press Enter to stop recording
- Run `echo x > /tmp/rpdictation_stop` in another terminal
- Click the notification in your desktop environment

You can also use `rpdictation toggle` to start/stop recording from a single keybinding.

## How it works

1. Records audio from your default microphone as a WAV file
2. Saves the recording temporarily to `/tmp/rpdictation.wav`
3. Submits the recording to your chosen provider (OpenAI Whisper, Mistral Voxtral, or Google Speech API) for transcription
4. Displays the transcription result
5. Optionally types the text into your active application using the configured typing backend (`wtype` or `ydotool`)
6. Calculates and displays the cost of the API call (OpenAI and Mistral providers)

## Similar projects

- **[Coe (聲)](https://github.com/quailyquaily/coe)** — A feature-rich Linux voice dictation tool written in Go, targeting GNOME/Wayland. Compared to rpdictation, Coe offers LLM-based post-processing (punctuation, casing, formatting correction), local/offline ASR via whisper.cpp, Fcitx5 IME integration, hold-to-talk mode, a personal dictionary, XDG Portal-first design, and context-aware paste shortcuts (terminal vs regular apps). It runs as a background daemon with a YAML config file. rpdictation is lighter-weight and more Unix-y by comparison: single invocation (no daemon), free Google STT fallback, Mistral provider support, provider fallback chain with automatic retries across providers, cost tracking, multiple stop methods (FIFO, signals, notifications), and Niri compositor support.

## License

This project is licensed under the GNU General Public License v3.0 (GPL-3.0) - see the LICENSE file for details.

## Acknowledgments

- [OpenAI Whisper](https://openai.com/research/whisper) for the speech recognition API
- [Mistral Voxtral](https://mistral.ai/) for the speech recognition API
- [Google Cloud Speech API](https://cloud.google.com/speech-to-text) for the free Chromium speech recognition
- [CPAL](https://github.com/RustAudio/cpal) for cross-platform audio
- [Tokio](https://tokio.rs/) for async runtime
- [Clap](https://github.com/clap-rs/clap) for command-line argument parsing
