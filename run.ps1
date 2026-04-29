# Realtime Call Translator - Windows Runner Script
# This script runs the application on Windows PowerShell

$ErrorActionPreference = "Stop"

Write-Host "=== Realtime Call Translator - Starting ===" -ForegroundColor Cyan

# Change to script directory
Set-Location -Path C:\Users\Pavel\AppData\Roaming\npm\call-translator-win

# Load environment variables from .env file
if (Test-Process ".env") {
    Write-Host "Loading environment variables from .env..." -ForegroundColor Yellow
    $envContent = Get-Content ".env"
    $envContent | ForEach-Object {
        if ($_ -match '^\s*([^#][^=]+)=(.*)$') {
            $key, $value = $matches[1].Trim(), $matches[2].Trim()
            [Environment]::SetEnvironmentVariable($key, $value, "Process")
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

# Start Flask web UI
Write-Host "Starting Flask web UI..." -ForegroundColor Yellow
$flaskProcess = Start-Process python -ArgumentList "web.py" -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 3

# Cleanup function
function Cleanup {
    Write-Host "Shutting down..." -ForegroundColor Yellow
    # Stop Flask process
    if ($flaskProcess -and !$flaskProcess.HasExited) {
        Stop-Process -Id $flaskProcess.Id -Force
    }
    
    # Stop audio_engine processes
    Get-Process | Where-Object { 
        $_.Name -like "*audio_engine*" -or 
        $_.Name -like "*erl*" -or
        $_.Name -like "*beam*"
    } | Stop-Process
}

# Register cleanup handler
$null = Register-ObjectEvent -InputObject $flaskProcess -EventName Exited -Action { Cleanup }

Write-Host "Starting Elixir application..." -ForegroundColor Green
Write-Host "Open http://127.0.0.1:5050 in your browser" -ForegroundColor Green
Write-Host "Press Ctrl+C to stop" -ForegroundColor Yellow

# Run Elixir with mix
$elixirArgs = @("--eval", $evalCode, "-S", "mix", "run", "--no-halt")
& elixir $elixirArgs