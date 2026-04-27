# 🎧 Real-time Call Translator: Windows Installation Guide

Complete guide to install and run the application with all dependencies on Windows 10/11.

## 📋 1. Prerequisites

### A. Install VB-Cable (Required for Audio Routing)
1. Download **VB-Cable** from [vb-audio.com/Cable/](https://vb-audio.com/Cable/)
2. Right-click the downloaded file and select **"Run as Administrator"**
3. Follow the installation wizard
4. **Restart your computer** after installation

### B. Install Git (if not already installed)
1. Download [Git for Windows](https://git-scm.com/download/win)
2. Run the installer with default settings
3. This will also install **Git Bash**, which we'll use later

---

## 🚀 2. Step-by-Step Installation

### Step 1: Clone the Repository
Open **PowerShell** and run:
```powershell
git clone https://github.com/Paul771/call-translator-win.git
cd call-translator-win
```

### Step 2: Run the Setup Script
1. Open **PowerShell as Administrator** (right-click → Run as Administrator)
2. Navigate to the project folder:
   ```powershell
   cd C:\Users\YourUsername\call-translator-win
   ```
3. Set execution policy (only needed once):
   ```powershell
   Set-ExecutionPolicy RemoteSigned -Scope Process
   ```
4. Run the setup script:
   ```powershell
   .\setup.ps1
   ```

**What the setup script does:**
- ✅ Checks for and installs Elixir via Winget
- ✅ Installs Rust via Rustup
- ✅ Installs Python 3 and espeak-ng
- ✅ Creates Python virtual environment (`.venv`)
- ✅ Installs Flask and other Python dependencies
- ✅ Compiles the Rust audio engine
- ✅ Compiles the Elixir application

**Expected output:** You should see green "Setup Complete!" message if successful.

### Step 3: Configure API Keys
1. Copy the example environment file:
   ```powershell
   copy .env.example .env
   ```
2. Edit `.env` file in a text editor (e.g., Notepad):
   ```powershell
   notepad .env
   ```
3. Add your API keys:
   ```env
   DEEPGRAM_API_KEY=your_deepgram_key_here
   GROQ_API_KEY=your_groq_key_here
   ORT_DYLIB_PATH=C:\onnxruntime\bin\onnxruntime.dll
   TRANSLATOR_MIC_DEVICE=default
   TRANSLATOR_SPEAKER_DEVICE=default
   TRANSLATOR_MEET_INPUT=CABLE Output
   TRANSLATOR_MEET_OUTPUT=CABLE Input
   ```
4. Save and close the file

---

## ▶️ 3. Running the Application

### Option A: Using Git Bash (Recommended)
1. Open **Git Bash** (not regular PowerShell)
2. Navigate to the project:
   ```bash
   cd /c/Users/YourUsername/call-translator-win
   ```
3. Run the application:
   ```bash
   ./run.sh
   ```

### Option B: Using PowerShell Native Script
If you prefer to stay in PowerShell, use the native script:
```powershell
.\run.ps1
```

### Option C: Manual Steps
If the scripts don't work, run manually:

1. **Activate Python venv:**
   ```powershell
   .\.venv\Scripts\Activate.ps1
   ```

2. **Start Flask web UI:**
   ```powershell
   python web.py
   ```
   (Keep this terminal open)

3. **In a new PowerShell window**, activate venv again and run Elixir:
   ```powershell
   .\.venv\Scripts\Activate.ps1
   iex -S mix run --no-halt
   ```

### Access the Web Interface
1. Open your browser (Chrome recommended)
2. Go to: **http://127.0.0.1:5050**
3. In the **Settings** panel:
   - Verify API keys are loaded
   - Select your language pair (e.g., Russian ↔ English)
   - Download required Piper voice models via the UI
   - Click **Save & Restart Engine**

---

## 🛠️ 4. Audio Device Configuration

### Configure Your Call App (Google Meet, Zoom, etc.)
| Setting in Call App | Select this Device | Why? |
| :--- | :--- | :--- |
| **Microphone** | `CABLE Output (VB-Cable)` | The app will "hear" the translated voice |
| **Speakers** | `CABLE Input (VB-Cable)` | The call audio is sent to the virtual cable |

### Configure in Translator Web UI
- **Mic Device**: Your actual physical microphone (e.g., "Microphone (Realtek Audio)")
- **Speaker Device**: Your actual headphones/speakers
- **Meet Input**: `CABLE Output`
- **Meet Output**: `CABLE Input`

---

## ⚠️ Troubleshooting

### "No Audio" Issues
1. Verify VB-Cable is installed correctly
2. Check call app audio settings use VB-Cable devices
3. In the web UI, verify device names match exactly (check dropdown lists)

### "Engine failed to start"
1. Ensure `.env` file exists with valid API keys
2. Verify `ORT_DYLIB_PATH` points to actual `onnxruntime.dll` location
3. Check if the file exists: `Test-Path $env:ORT_DYLIB_PATH`

### "Permission denied" or "Cannot run script"
1. Run PowerShell **as Administrator**
2. Set execution policy: `Set-ExecutionPolicy RemoteSigned -Scope Process`

### "Python not found" or "Module not found"
1. Ensure virtual environment is activated: `.venv\Scripts\Activate.ps1`
2. Reinstall dependencies: `pip install -r requirements.txt`

### "Mix command not found"
1. Ensure Elixir is installed: `elixir --version`
2. If not installed, run `setup.ps1` again

### Audio routing not working
1. Make sure VB-Cable drivers are properly installed (reinstall if needed)
2. Restart computer after VB-Cable installation
3. In Windows Sound settings, ensure VB-Cable devices are not muted
4. Test VB-Cable: Record a sound and play it back through the virtual cable

---

## 📝 Notes
- The application requires **Windows 10 version 1903+** or **Windows 11**
- Administrator privileges are only needed for the initial setup
- First compilation may take 5-10 minutes
- Subsequent runs will be much faster
