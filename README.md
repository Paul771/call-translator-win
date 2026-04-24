# Realtime Call Translator

[![README на русском](https://img.shields.io/badge/README-Russian-blue)](README.ru.md)

Real-time speech translator for video/voice calls. Translates both sides of the conversation live — you speak your language, the other person hears theirs, and vice versa.

**How it works:** Your mic audio goes through Speech-to-Text, gets translated by an LLM, then synthesized back to speech and routed into your call. The same happens in reverse for the other person's audio.

Supports **29 languages** with STT, translation, and TTS. Voice models from [Piper](https://github.com/rhasspy/piper) — download any language directly from the web UI.

![macOS](https://img.shields.io/badge/platform-macOS_14+-lightgrey)
![License](https://img.shields.io/badge/license-MIT-blue)
![GitHub stars](https://img.shields.io/github/stars/LetovKai/call-translator)

> **Platforms:** macOS 14+, Windows 10/11, Linux
> - macOS: Uses CoreAudio and cpal for audio capture
> - Windows: Uses WASAPI via cpal (automatic)
> - Linux: Uses PulseAudio/JACK via cpal

---

## Quick Start

**macOS:**
```bash
git clone https://github.com/LetovKai/call-translator.git
cd call-translator
./setup.sh
```

Then:
```bash
./run.sh
```

**Windows:**
1. **Install VB-Cable** (Critical for audio routing):
   - Download and install from [vb-audio.com/Cable/](https://vb-audio.com/Cable/)
2. **Run Setup**:
   - Open PowerShell as Administrator and run:
     ```powershell
     Set-ExecutionPolicy RemoteSigned -Scope Process
     .\setup.ps1
     ```
3. **Run Application**:
   - Open a terminal (Bash/WSL/Git Bash) and run:
     ```bash
     ./run.sh
     ```

Open **http://127.0.0.1:5050** in **Google Chrome**. Settings open automatically on first launch — enter your API keys and configure languages there.

**[Usage Guide (USAGE.md)](USAGE.md)** — controls, voice management, audio setup, call history.

> **Browser:** Use **Chrome** — audio monitor works correctly on all platforms. Safari has audio output limitations that prevent monitor playback. Other browsers are untested.

> You need two free API keys (free tiers available):
> - [Deepgram](https://console.deepgram.com) — speech-to-text
> - [Groq](https://console.groq.com) — translation (LLM)

---

## Architecture

```
┌─────────────┐     ┌──────────────┐     ┌───────────┐     ┌─────────┐
│  Your Mic   │────>│ Deepgram STT │────>│ Groq LLM  │────>│ Piper   │──> Call
│  (your lang)│     │  (speech→text)│     │ (translate)│     │  TTS    │   (BlackHole)
└─────────────┘     └──────────────┘     └───────────┘     └─────────┘

┌─────────────┐     ┌──────────────┐     ┌───────────┐     ┌─────────┐
│  Call Audio  │────>│ Deepgram STT │────>│ Groq LLM  │────>│ Piper   │──> Speakers
│ (their lang)│     │  (speech→text)│     │ (translate)│     │  TTS    │
└─────────────┘     └──────────────┘     └───────────┘     └─────────┘
```

- **Elixir** — orchestrator, process supervision, port management
- **Rust** — audio capture/playback, STT streaming, TTS synthesis, translation
- **Flask** — web UI for live transcript, settings, and controls

---

## Requirements

| Dependency | Purpose | Install (macOS) | Install (Windows) |
|---|---|---|---|
| Elixir | Application runtime | `brew install elixir` | [Chocolatey](https://chocolatey.org/packages/elixir): `choco install elixir` |
| Rust | Audio engine | `rustup init` | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | rustup init` |
| Python 3 | Web UI server | `brew install python@3` | [Python.org](https://python.org) or winget: `winget install Python.Python.3` |
| espeak-ng | TTS phonemization | `brew install espeak-ng` | winget: `winget install espeak-ng` |
| ONNX Runtime | Model inference | `brew install onnxruntime` | winget or download from [Microsoft](https://learn.microsoft.com/en-us/azure/machine-learning/reference/onnx-runtime)
| Flask | Web framework | via venv (see below) | via venv (same as macOS) |
| WASAPI | Audio I/O | N/A | Built into Windows

**API Keys (free tiers available):**
- [Deepgram](https://console.deepgram.com) — speech-to-text (Nova-3 model)
- [Groq](https://console.groq.com) — translation via llama-3.3-70b

---

## Manual Installation

If you prefer to install everything step by step instead of using `setup.sh`:

### 1. System packages

**macOS:**
```bash
xcode-select --install
brew install elixir rustup espeak-ng onnxruntime python@3
rustup-init -y --default-toolchain stable
source ~/.cargo/env

# Create virtual environment and install Flask
python3 -m venv .venv
source .venv/bin/activate
pip install flask
```

**Windows:**
```powershell
# Install via winget (recommended)
winget install Elixir.Elixir
rustup-init -y --default-toolchain stable
winget install Python.Python.3
winget install espeak-ng

# OR use Chocolatey:
choco install elixir rust python espeak-ng onnxruntime

# Create virtual environment and install Flask
python -m venv .venv
.venv\Scripts\activate
pip install flask
```

### 2. Audio setup

**macOS (BlackHole):**
1. Download and install [BlackHole](https://existential.audio/blackhole/)
2. You need **both**:
   - **BlackHole 16ch** — captures audio from your call app
   - **BlackHole 2ch** — sends translated audio back to the call
3. Setup in your call app (Google Meet, Zoom, etc.):
   - Open the call in **Google Chrome**
   - Set **BlackHole 2ch** as the **microphone** in the call app
   - Set **BlackHole 16ch** as the **speakers** in the call app

> **Note:** Do NOT use a Multi-Output Device — it may cause audio issues.

**Windows (WASAPI):**
No additional drivers needed! The project uses WASAPI directly through cpal:
1. Set your desired microphone as the default input device
2. Set your speakers/headphones as the default output device
3. No virtual routing required — audio flows directly between devices

### 3. Download voice models

TTS voices come from [Piper](https://github.com/rhasspy/piper). The setup script downloads English and Russian voices automatically. Additional voices can be downloaded from the web UI — select a language and click the download button.

To download manually:

```bash
mkdir -p models/piper-en models/piper-ru

# English (default)
curl -sL https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/ryan/medium/en_US-ryan-medium.onnx \
  -o models/piper-en/en_US-ryan-medium.onnx
curl -sL https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/en/en_US/ryan/medium/en_US-ryan-medium.onnx.json \
  -o models/piper-en/en_US-ryan-medium.onnx.json

# Russian (default)
curl -sL https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/ru/ru_RU/denis/medium/ru_RU-denis-medium.onnx \
  -o models/piper-ru/ru_RU-denis-medium.onnx
curl -sL https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0/ru/ru_RU/denis/medium/ru_RU-denis-medium.onnx.json \
  -o models/piper-ru/ru_RU-denis-medium.onnx.json
```

Browse all available voices at [rhasspy.github.io/piper-samples](https://rhasspy.github.io/piper-samples/).

### 4. Environment variables
```bash
cp .env.example .env
```

Edit `.env`:

**macOS:**
```
DEEPGRAM_API_KEY=your_key_here
GROQ_API_KEY=your_key_here
ORT_DYLIB_PATH=/opt/homebrew/lib/libonnxruntime.dylib
```

**Windows:**
```powershell
copy .env.example .env
# Edit .env with your API keys:
DEEPGRAM_API_KEY=your_key_here
GROQ_API_KEY=your_key_here
# ONNX Runtime DLL location (adjust if installed elsewhere)
ORT_DYLIB_PATH=C:\ProgramData\chocolatey\lib\onnxruntime\bin\onnxruntime.dll
```

Or set `ORT_DYLIB_PATH` via environment variable in PowerShell:
```powershell
$env:ORT_DYLIB_PATH = "C:\ProgramData\chocolatey\lib\onnxruntime\bin\onnxruntime.dll"
```

Edit `.env`:

```
DEEPGRAM_API_KEY=your_key_here
GROQ_API_KEY=your_key_here
ORT_DYLIB_PATH=/opt/homebrew/lib/libonnxruntime.dylib
```

### 5. Build

```bash
mix deps.get
mix compile    # Compiles Elixir + Rust (first build takes a few minutes)
```

### 6. Run

```bash
./run.sh
```

Open **http://127.0.0.1:5050** in Chrome.

---

## Web UI Features

- **Live transcript** — chat-style bubbles with original text and translation
- **29 languages** — switch language pair from Settings, download voices with one click
- **Voice selection** — multiple voices per language with preview playback
- **Audio monitor** — hear translations in your browser (Chrome only)
- **Start/Stop** — control the engine without restarting
- **Mute** — independently mute outgoing or incoming pipelines
- **Bookmarks** — star important phrases, filter to show only starred
- **Export** — download the full transcript as a text file
- **Compact/Full view** — toggle between detailed and compact transcript
- **Latency metrics** — per-phrase STT, translation, TTS, and total latency
- **Dark/Light theme** — toggle with persistence

---

## Supported Languages

| Language | STT | Translation | TTS |
|----------|-----|-------------|-----|
| Arabic | + | + | + |
| Catalan | + | + | + |
| Chinese | + | + | + |
| Czech | + | + | + |
| Danish | + | + | + |
| Dutch | + | + | + |
| English | + | + | + |
| Finnish | + | + | + |
| French | + | + | + |
| German | + | + | + |
| Greek | + | + | + |
| Hindi | + | + | + |
| Hungarian | + | + | + |
| Indonesian | + | + | + |
| Italian | + | + | + |
| Japanese | + | + | — |
| Korean | + | + | — |
| Latvian | + | + | + |
| Norwegian | + | + | + |
| Persian | + | + | + |
| Polish | + | + | + |
| Portuguese | + | + | + |
| Romanian | + | + | + |
| Russian | + | + | + |
| Spanish | + | + | + |
| Swedish | + | + | + |
| Turkish | + | + | + |
| Ukrainian | + | + | + |
| Vietnamese | + | + | + |

TTS requires downloading a Piper voice model for the language (one-click from the web UI). Japanese and Korean have STT and translation but no Piper TTS voice available.

---

## Troubleshooting

**"Engine not starting"**
- Check that `.env` has valid API keys
- Verify `ORT_DYLIB_PATH` points to your onnxruntime library
- Run `mix compile` to check for build errors

**"No audio from call"**
- Ensure BlackHole 16ch is set up in a Multi-Output Device
- Check that your call app uses BlackHole 2ch as its microphone

**"TTS not working"**
- Verify `espeak-ng` is installed: `espeak-ng --version`
- Check that voice model files exist in `models/piper-{lang}/`
- Download voices from Settings in the web UI

**"No sound in monitor"**
- Use Chrome — Safari does not support audio output routing required for monitor
- Check your system audio output is set to speakers (not BlackHole)

**"Groq key shows invalid"**
- The key is likely valid — test by clicking "Test" in Settings
- Keys set via `.env` work automatically even if the UI field is empty

---

## License

MIT
