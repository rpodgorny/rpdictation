# RPDictation

![License](https://img.shields.io/badge/license-GPL-blue.svg)

RPDictation is a simple, efficient speech-to-text transcription tool for Linux that leverages OpenAI's Whisper API to provide accurate transcriptions directly from your microphone.

## Features

- **Real-time audio recording** from your default microphone
- **High-quality transcription** using OpenAI's Whisper API
- **Multiple ways to control recording**:
  - Press Enter
  - Send a command to a FIFO
  - Click a desktop notification
- **Optional text insertion** directly into applications using `wtype`
- **Cost tracking** to monitor your API usage
- **Clean, simple interface** with recording time display

## Installation

### Prerequisites

- Rust and Cargo
- OpenAI API key
- Linux with PulseAudio/PipeWire
- (Optional) `wtype` for text insertion capability

### Build from source

```bash
git clone https://github.com/yourusername/rpdictation.git
cd rpdictation
cargo build --release
```

The executable will be available at `./target/release/rpdictation`

## Usage

### Basic usage

```bash
export OPENAI_API_KEY=your_api_key_here
./rpdictation
```

Or specify the API key directly:

```bash
./rpdictation --openai-api-key your_api_key_here
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
3. Submits the recording to OpenAI's Whisper API for transcription
4. Displays the transcription result
5. Optionally types the text into your active application using `wtype`
6. Calculates and displays the cost of the API call

## Roadmap

- [ ] Support for macOS and Windows
- [ ] Integration with other speech recognition backends
- [ ] Customizable hotkeys
- [ ] Continuous dictation mode
- [ ] Wake word detection
- [ ] GUI interface option

## License

This project is licensed under the GNU General Public License v3.0 (GPL-3.0) - see the LICENSE file for details.

## Acknowledgments

- [OpenAI Whisper](https://openai.com/research/whisper) for the speech recognition API
- [CPAL](https://github.com/RustAudio/cpal) for cross-platform audio
- [Tokio](https://tokio.rs/) for async runtime
- [Clap](https://github.com/clap-rs/clap) for command-line argument parsing
