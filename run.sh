#!/usr/bin/env bash
# Cross-platform run script for macOS and Windows (WSL/PowerShell)
cd "$(dirname "$0")"

# Load environment variables from .env file
if [ -f ".env" ]; then
  set -a
  source .env
  set +a
fi

# Activate venv if it exists
if [ -d ".venv" ] && [ -f ".venv/bin/activate" ]; then
  source .venv/bin/activate
elif [ -d ".venv" ] && [ -f ".venv\Scripts\activate.bat" ]; then
  # Windows path
  echo "Windows: Run '.venv\Scripts\activate' in PowerShell first"
  exit 1
fi

# Start Flask web UI in background (cross-platform)
if command -v python3 &>/dev/null; then
  python3 web.py &
  FLASK_PID=$!
elif command -v python &>/dev/null; then
  python web.py &
  FLASK_PID=$!
else
  echo "Error: Python not found"
  exit 1
fi

# Cleanup on exit (cross-platform)
cleanup() {
  # Kill Flask process
  if [ -n "$FLASK_PID" ]; then
    kill $FLASK_PID 2>/dev/null || true
  fi
  
  # Kill audio_engine processes (cross-platform)
  pkill -f "audio_engine" 2>/dev/null || taskkill /F /IM audio_engine.exe 2>/dev/null || true
  
  exit 0
}
trap cleanup EXIT INT TERM

EVAL='spawn(fn ->
  wait = fn wait, n ->
    case Process.whereis(Translator.AudioEngine) do
      nil when n > 0 -> Process.sleep(100); wait.(wait, n - 1)
      nil -> IO.puts("AudioEngine not started after 30s")
      _pid -> IO.puts("AudioEngine ready (waiting for Start)")
    end
  end
  wait.(wait, 300)
end)'

if [ "$1" = "--bg" ]; then
  elixir --eval "$EVAL" -S mix run --no-halt
else
  iex --eval "$EVAL" -S mix
fi
