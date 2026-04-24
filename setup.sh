#!/usr/bin/env bash
# Windows-compatible setup script with fallback for Chocolatey/winget
set -e

echo "=== Realtime Call Translator — Setup ==="
echo ""

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

ok()   { echo -e "${GREEN}[OK]${NC} $1"; }
warn() { echo -e "${YELLOW}[!]${NC} $1"; }
fail() { echo -e "${RED}[ERR]${NC} $1"; }

# ---- 1. Xcode Command Line Tools ----
echo "--- Checking Xcode CLT..."
if xcode-select -p &>/dev/null; then
  ok "Xcode Command Line Tools"
else
  warn "Installing Xcode Command Line Tools..."
  xcode-select --install
  echo "Press Enter after installation completes..."
  read -r
fi

# ---- 2. Homebrew ----
echo "--- Checking Homebrew..."
if command -v brew &>/dev/null; then
  ok "Homebrew"
else
  fail "Homebrew not found. Install from https://brew.sh"
  exit 1
fi

# ---- 3. System packages ----
echo "--- Installing system packages..."
PACKAGES="elixir rustup espeak-ng onnxruntime python@3"

for pkg in $PACKAGES; do
  if brew list "$pkg" &>/dev/null; then
    ok "$pkg (already installed)"
  else
    echo "    Installing $pkg..."
    brew install "$pkg"
    ok "$pkg"
  fi
done

# Ensure Rust toolchain
if command -v rustc &>/dev/null; then
  ok "Rust toolchain ($(rustc --version | cut -d' ' -f2))"
else
  rustup-init -y --default-toolchain stable
  source "$HOME/.cargo/env"
  ok "Rust toolchain installed"
fi

# ---- 4. Python venv + packages ----
echo "--- Setting up Python virtual environment..."
VENV_DIR="$(cd "$(dirname "$0")" && pwd)/.venv"
if [ -d "$VENV_DIR" ]; then
  ok "venv (exists)"
else
  python3 -m venv "$VENV_DIR"
  ok "venv created"
fi
source "$VENV_DIR/bin/activate"
pip install --quiet -r "$(cd "$(dirname "$0")" && pwd)/requirements.txt"
ok "Python packages"

# ---- 5. BlackHole audio driver ----
echo ""
if system_profiler SPAudioDataType 2>/dev/null | grep -q "BlackHole"; then
  ok "BlackHole audio driver"
else
  warn "BlackHole audio driver not found."
  echo "    Download and install from: https://existential.audio/blackhole/"
  echo "    You need both BlackHole 2ch and BlackHole 16ch."
  echo "    Press Enter after installing (or skip with 's')..."
  read -r response
  if [ "$response" = "s" ]; then
    warn "Skipped BlackHole — audio routing won't work without it"
  fi
fi

# ---- 6. Download default TTS voice models (1 per language) ----
echo "--- Downloading default TTS voice models..."
python3 - <<'PYEOF'
import urllib.request, json, os, sys

MODELS_DIR = os.path.join(os.getcwd(), "models")
HF_BASE = "https://huggingface.co/rhasspy/piper-voices/resolve/v1.0.0"

# One default voice per language — others can be downloaded from the web UI
DEFAULTS = {
    "en": "en_US-ryan-medium",
    "ru": "ru_RU-denis-medium",
}

print("    Fetching voice catalog...")
req = urllib.request.Request(
    f"{HF_BASE}/voices.json",
    headers={"User-Agent": "translator/1.0"}
)
catalog = json.loads(urllib.request.urlopen(req, timeout=30).read())

for lang, default_voice in DEFAULTS.items():
    lang_dir = os.path.join(MODELS_DIR, f"piper-{lang}")
    os.makedirs(lang_dir, exist_ok=True)

    if default_voice not in catalog:
        print(f"    \033[1;33m[!]\033[0m {default_voice} not found in catalog, skipping")
        continue

    info = catalog[default_voice]
    files = info.get("files", {})
    all_exist = all(
        os.path.exists(os.path.join(lang_dir, fpath.split("/")[-1]))
        for fpath in files
    )
    if all_exist:
        print(f"    \033[0;32m[OK]\033[0m {default_voice} (already downloaded)")
        continue

    for fpath, finfo in files.items():
        dest = os.path.join(lang_dir, fpath.split("/")[-1])
        if os.path.exists(dest):
            continue
        url = f"{HF_BASE}/{fpath}"
        mb = round(finfo.get("size_bytes", 0) / 1048576, 1)
        sys.stdout.write(f"\r    [{lang}] {default_voice} ({mb} MB)...                    ")
        sys.stdout.flush()
        req = urllib.request.Request(url, headers={"User-Agent": "translator/1.0"})
        with urllib.request.urlopen(req, timeout=120) as resp:
            with open(dest, "wb") as f:
                while True:
                    chunk = resp.read(65536)
                    if not chunk:
                        break
                    f.write(chunk)

    print(f"\r    \033[0;32m[OK]\033[0m {default_voice}                                   ")

print("    More voices can be downloaded from the Settings panel in the web UI.")
PYEOF

# ---- 7. Environment file ----
echo "--- Setting up environment..."
ENV_FILE="$(cd "$(dirname "$0")" && pwd)/.env"
if [ -f "$ENV_FILE" ]; then
  ok ".env file (exists)"
else
  cp "$(cd "$(dirname "$0")" && pwd)/.env.example" "$ENV_FILE"
  warn ".env created from template — edit it with your API keys:"
  echo "    $ENV_FILE"
  echo ""
  echo "    DEEPGRAM_API_KEY  — get at https://console.deepgram.com"
  echo "    GROQ_API_KEY      — get at https://console.groq.com"
fi

# ---- 8. Build ----
echo "--- Building project..."
cd "$(dirname "$0")"

echo "    Fetching Elixir dependencies..."
mix deps.get --quiet 2>/dev/null || mix deps.get
ok "Elixir deps"

echo "    Compiling (Elixir + Rust)... this may take a few minutes on first run."
mix compile
ok "Build complete"

# ---- Done ----
echo ""
echo -e "${GREEN}=== Setup complete! ===${NC}"
echo ""
echo "Next steps:"
echo "  1. Edit .env with your Deepgram and Groq API keys"
echo "  2. Run: ./run.sh"
echo "  3. Open: http://127.0.0.1:5050"
echo ""
