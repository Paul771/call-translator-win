# 🎧 Real-time Call Translator: Windows Installation Guide

This is a complete guide to install the application and all its dependencies on Windows.

## 📋 1. Prerequisites

### A. Install VB-Cable (Required for Audio Routing)
1. Download **VB-Cable** from [vb-audio.com/C1. 
2. Install the driver and **restart your computer** after installation.

### B. Install System Dependencies

**Option 1: Automatic Installation (Recommended)**
1. Download and run the `setup.ps1` script which will automatically install:
   - Elixir 1.16+ (Erlang/OTP 26+)
   - Python 3.10+
   - Rust (latest stable)
   - espeak-ng

**Option 2: Manual Installation**
If the automatic setup fails, install these manually:
1. **Elixir**: Download from [elixir-lang.org](https://elixir-lang.org/)
2. **Python 3**: Download from [python.org](https://www.python.org/)
3. **Rust**: Install using [rustup](https://rustup.rs/)
4. **espeak-ng**: Download from [espeak.sourceforge.net](http://espeak.sourceforge.net/)

### C. Audio Device Configuration

To make the translation flow into your call, configure your **Call App** (e.g., Google Meet) as follows:

| Setting in Call App | Select this Device | Why? |
| :--- | :--- | :--- |
| **Microphone** | `CABLE Output (VB-Cable)` | The app will "hear" the translated voice coming from the translator. |
| **Speakers** | `CABLE Input (VB-Cable)` | The call audio is sent to the virtual cable so the translator can capture it. |

**In the Translator Web UI (http://127.0.0.1:5050):**
- **Mic Device**: Your actual physical microphone.
- **Speaker Device**: Your actual headphones/speakers.

---

## 🚀 2. Step-by-Step Installation

### Step 1: Clone the Repository
Open PowerShell or Git Bash and run:
```powershell
git clone https://github.com/Paul771/call-translator-win.git
cd call-translator-win
```

### Step 2: Run Setup
```powershell
Set-ExecutionPolicy RemoteSigned -Scope Process
.\setup.ps1
```

**What this script does:**
- Installs **Elixir** (App runtime) via package manager
- Installs **Rust** (Audio engine) via Rustup
- Installs **Python 3** and **espeak-ng**
- Creates a Python virtual environment (`.venv`) and installs Flask
- Compiles the Rust audio engine and Elixir orchestrator

### Step 3: Configuration (`.env`)
1. Find the `.env.example` file in the root folder.
2. Create a copy and name it `.env`.
3. Open `.env` in a text editor and fill in your keys:
```env
DEEPGRAM_API_KEY=your_actual_key_here
GROQ_API_KEY=your_actual_key_here
# Standard path for ONNX Runtime (adjust if you installed it elsewhere)
ORT_DYLIB_PATH=C:\onnxruntime\bin\onnxruntime.dll
```

---

## 🏁 3. Running the Application
1. Open a terminal (Git Bash or WSL is recommended for the run script) and execute:
```bash
./run.sh
```
2. Open your browser (Google Chrome recommended) and go to: `http://127.0.0.1:5050`
3. In the **Settings** panel:
   - Enter your API keys (if not set in `.env`).
   - Select your language pair (e.g., Russian <-> English).
   - Download the required Piper voice models via the UI.
   - Click **Save & Restart Engine**.

### ⚠️ Troubleshooting
- **No Audio?** Double-check that your call app's output is set to `CABLE Input` and your translator's input is set to `CABLE Output`.
- **Engine failed to start?** Ensure `ORT_DYLIB_PATH` in `.env` points to the correct location of `onnxruntime.dll`.
- **Permission Error?** Ensure you ran `setup.ps1` as Administrator.