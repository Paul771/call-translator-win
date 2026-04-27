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

# Start Flask web UI in background
Write-Host "Starting Flask web UI..." -ForegroundColor Yellow
$flaskStartInfo = New-Object System.Diagnostics.ProcessStartInfo
$flaskStartInfo.FileName = "python"
$flaskStartInfo.Arguments = "web.py"
$flaskStartInfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
$flaskStartInfo.UseShellExecute = $true
$flaskProcess = [System.Diagnostics.Process]::Start($flaskStartInfo)
Write-Host "Flask started (PID: $($flaskProcess.Id))" -ForegroundColor Green

# Wait for Flask to start
Start-Sleep -Seconds 3

# Cleanup function
function Cleanup {
    Write-Host "`nShutting down..." -ForegroundColor Yellow
    
    # Stop Flask process
    if ($flaskProcess -and !$flaskProcess.HasExited) {
        Stop-Process -Id $flaskProcess.Id -Force -ErrorAction SilentlyContinue
    }
    
    # Stop audio_engine processes
    Get-Process | Where-Object { $_.Name -like "*audio_engine*" -or $_.Name -like "*erl*" } | 
        Stop-Process -Force -ErrorAction SilentlyContinue
    
    # Restore original directory
    Set-Location $originalDir
    
    Write-Host "Shutdown complete." -ForegroundColor Yellow
}

# Register cleanup on exit
$null = Register-ObjectEvent -InputObject $flaskProcess -EventName Exited -Action { Cleanup } -Force

# Start Elixir application
Write-Host "Starting Elixir application..." -ForegroundColor Yellow
Write-Host "Open http://127.0.0.1:5050 in your browser" -ForegroundColor Green
Write-Host "Press Ctrl+C to stop" -ForegroundColor Yellow

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

try {
    # Run Elixir with mix in interactive mode
    $elixirArgs = @("--eval", $evalCode, "-S", "mix", "run", "--no-halt")
    
    # Use Start-Process for better control
    $elixirStartInfo = New-Object System.Diagnostics.ProcessStartInfo
    $elixirStartInfo.FileName = "elixir"
    $elixirStartInfo.Arguments = ($elixirArgs | ForEach-Object { "`"$_`" " }) -join ' '
    $elixirStartInfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Normal
    $elixirStartInfo.UseShellExecute = $true
    $elixirProcess = [System.Diagnostics.Process]::Start($elixirStartInfo)
    
    # Wait for Elixir process
    while (!$elixirProcess.HasExited) {
        Start-Sleep -Milliseconds 500
    }
} catch {
    Write-Error "Failed to start Elixir application: $_"
    Cleanup
    exit 1
}

Cleanup
