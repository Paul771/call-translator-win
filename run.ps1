# Realtime Call Translator - Windows Runner Script
# This script runs the application on Windows PowerShell

$ErrorActionPreference = "Stop"

Write-Host "=== Realtime Call Translator - Starting ===" -ForegroundColor Cyan

# Change to script directory
$originalDir = Get-Location
Set-Location -Path (Split-Path -Parent $MyInvocation.MyCommand.Path)

# Load environment variables from .env file
if (Test-Path ".env") {
    Write-Host "Loading environment variables from .env..." -ForegroundColor Yellow
    Get-Content .env | ForEach-Object {
        if ($_ -match '^\s*([^#][^=]+)=(.*)$') {
            $key = $matches[1].Trim()
            $value = $matches[2].Trim()
            Set-Item -Path "ENV:$key" -Value $value
        }
    }
} else {
    Write-Warning ".env file not found. Using existing environment variables."
}

# Check if virtual environment exists
if (Test-Path ".venv\Scripts\Activate.ps1") {
    Write-Host "Activating Python virtual environment..." -ForegroundColor Yellow
    & .\.venv\Scripts\Activate.ps1
} else {
    Write-Error "Virtual environment not found. Please run setup.ps1 first."
    exit 1
}

# Start Flask web UI in background
Write-Host "Starting Flask web UI..." -ForegroundColor Yellow
$flaskProcess = Start-Process python -ArgumentList "web.py" -PassThru -WindowStyle Hidden
Write-Host "Flask started (PID: $($flaskProcess.Id))" -ForegroundColor Green

# Wait for Flask to start
Start-Sleep -Seconds 2

# Cleanup function
function Cleanup {
    Write-Host "`nShutting down..." -ForegroundColor Yellow
    
    # Stop Flask process
    if ($flaskProcess -and !$flaskProcess.HasExited) {
        Stop-Process -Id $flaskProcess.Id -Force
    }
    
    # Stop audio_engine processes
    Get-Process | Where-Object { $_.Name -like "*audio_engine*" } | Stop-Process -Force
    
    # Restore original directory
    Set-Location $originalDir
    
    Write-Host "Shutdown complete." -ForegroundColor Yellow
    exit 0
}

# Register cleanup on exit
Register-ObjectEvent -InputObject $flaskProcess -EventName Exited -Action { Cleanup } | Out-Null
Ctrl+C { Cleanup }
Ctrl+Break { Cleanup }

# Start Elixir application
Write-Host "Starting Elixir application..." -ForegroundColor Yellow
Write-Host "Open http://127.0.0.1:5050 in your browser" -ForegroundColor Green

# Define the evaluation code for Elixir
$evalCode = @'
spawn(fn ->
  wait = fn wait, n ->
    case Process.whereis(Translator.AudioEngine) do
      nil when n > 0 -> Process.sleep(100); wait.(wait, n - 1)
      nil -> IO.puts("AudioEngine not started after 30s")
      _pid -> IO.puts("AudioEngine ready (waiting for Start)")
    end
  end
  wait.(wait, 300)
end)
'@

# Run Elixir with mix
try {
    # Note: This requires Elixir to be in PATH
    $elixirArgs = @("--eval", $evalCode, "-S", "mix", "run", "--no-halt")
    & elixir $elixirArgs
} catch {
    Write-Error "Failed to start Elixir application: $_"
    Cleanup
}

Cleanup
