# RPDictation

![License](https://img.shields.io/badge/license-GPL-blue.svg)

RPDictation is a simple, efficient speech-to-text transcription tool for Linux that provides accurate transcriptions directly from your microphone. It supports multiple speech recognition providers including OpenAI's Whisper API and Google's Chromium Speech API (free but limited alternative).

## Features

- **Real-time audio recording** from your default microphone
- **Multiple transcription providers**:
  - OpenAI's Whisper API (high quality, paid)
  - Google Chromium Speech API (free alternative, limited)
- **Multiple ways to control recording**:
  - Press Enter
  - Send a command to a FIFO
  - Click a desktop notification
- **Optional text insertion** directly into applications using `wtype`
- **Cost tracking** for API usage (OpenAI provider)
- **Clean, simple interface** with recording time display
- **Environment variable support** via `.env` file or plain environment

## Installation

### Prerequisites

- Rust and Cargo
- Linux with PulseAudio/PipeWire
- API key requirements (depends on provider):
  - **OpenAI provider**: Requires OpenAI API key
  - **Google provider**: Works without API key (uses default Chromium key)
- (Optional) `wtype` for text insertion capability

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
```

Then run:

```bash
./rpdictation
```

### Text insertion mode

To automatically insert the transcribed text (requires `wtype`):

```bash
./rpdictation --wtype
```

### During recording

While recording, you can:
- Press Enter to stop recording
- Run `echo x > /tmp/rpdictation_stop` in another terminal
- Click the notification in your desktop environment

## How it works

1. Records audio from your default microphone as a WAV file
2. Saves the recording temporarily to `/tmp/rpdictation.wav`
3. Submits the recording to your chosen provider (OpenAI Whisper or Google Speech API) for transcription
4. Displays the transcription result
5. Optionally types the text into your active application using `wtype`
6. Calculates and displays the cost of the API call (OpenAI provider only)

## License

This project is licensed under the GNU General Public License v3.0 (GPL-3.0) - see the LICENSE file for details.

## Acknowledgments

- [OpenAI Whisper](https://openai.com/research/whisper) for the speech recognition API
- [Google Cloud Speech API](https://cloud.google.com/speech-to-text) for the free Chromium speech recognition
- [CPAL](https://github.com/RustAudio/cpal) for cross-platform audio
- [Tokio](https://tokio.rs/) for async runtime
- [Clap](https://github.com/clap-rs/clap) for command-line argument parsing
