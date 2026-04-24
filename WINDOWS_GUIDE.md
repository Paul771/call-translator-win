# 🎧 Real-time Call Translator: Windows Installation Guide

This application allows you to translate voice calls in real-time. It captures your microphone, translates it, and sends the synthesized voice into your call app. It also captures the other person's voice, translates it, and plays it through your speakers.

## 📋 1. Prerequisites & Dependencies

### A. Virtual Audio Cable (Critical)
Since Windows does not have a built-in virtual audio router, you **must** install **VB-Cable**. This acts as the "bridge" between the translator and your call app (Zoom, Google Meet, Teams).
1. Download **VB-Cable** from [vb-audio.com/Cable/](https://vb-audio.com/Cable/).
2. Install the driver and **restart your computer**.

### B. API Keys
You need two free API keys to power the AI:
1. **Deepgram API Key**: For Speech-to-Text (STT). Get it at [console.deepgram.com](https://console.deepgram.com).
2. **Groq API Key**: For ultra-fast LLM translation. Get it at [console.groq.com](https://console.groq.com).

---

## 🚀 2. Step-by-Step Installation

### Step 1: Clone the Repository
Open PowerShell or Git Bash and run:
```powershell
git clone https://github.com/Paul771/call-translator-win.git
cd call-translator-win
```

### Step 2: Automated Setup
The project includes a native Windows setup script. Open **PowerShell as Administrator** and run:
```powershell
Set-ExecutionPolicy RemoteSigned -Scope Process
.\setup.ps1
```
**What this script does:**
- Installs **Elixir** (App runtime) via Winget.
- Installs **Rust** (Audio engine) via Rustup.
- Installs **Python 3** and **espeak-ng** (TTS phonemizer).
- Creates a Python virtual environment (`.venv`) and installs Flask.
- Compiles the Rust audio engine and Elixir orchestrator.

### Step 3: Configuration (`.env`)
1. Find the `.env.example` file in the root folder.
2. Create a copy and name it `.env`.
3. Open `.env` in a text editor and fill in your keys:
```env
DEEPGRAM_API_KEY=your_actual_key_here
GROQ_API_KEY=your_actual_key_here
# Standard path for ONNX Runtime (adjust if you installed it elsewhere)
ORT_DYLIB_PATH=C:\onnxruntime\bin\onnxruntime.dll

# Windows Audio Routing
TRANSLATOR_MIC_DEVICE=default
TRANSLATOR_SPEAKER_DEVICE=default
TRANSLATOR_MEET_INPUT=CABLE Output
TRANSLATOR_MEET_OUTPUT=CABLE Input
```

---

## 🛠️ 3. Audio Routing Configuration (The "Magic" Part)
To make the translation flow into your call, configure your **Call App** (e.g., Google Meet) as follows:

| Setting in Call App | Select this Device | Why? |
| :--- | :--- | :--- |
| **Microphone** | `CABLE Output (VB-Cable)` | The app will "hear" the translated voice coming from the translator. |
| **Speakers** | `CABLE Input (VB-Cable)` | The call audio is sent to the virtual cable so the translator can capture it. |

**In the Translator Web UI (http://127.0.0.1:5050):**
- **Mic Device**: Your actual physical microphone.
- **Speaker Device**: Your actual headphones/speakers.

---

## 🏁 4. Running the Application
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
