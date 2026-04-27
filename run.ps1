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

# Start Flask web UI
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
        try {
            Stop-Process -Id $flaskProcess.Id -Force -ErrorAction SilentlyContinue
        } catch {}
    }
    
    # Stop audio_engine and Erlang processes
    Get-Process | Where-Object { 
        $_.Name -like "*audio_engine*" -or 
        $_.Name -like "*erl*" -or
        $_.Name -like "*beam*"
    } | Stop-Process -Force -ErrorAction SilentlyContinue
    
    # Restore original directory
    Set-Location $originalDir
    
    Write-Host "Shutdown complete." -ForegroundColor Yellow
}

# Register cleanup handlers
$null = Register-ObjectEvent -InputObject $flaskProcess -EventName Exited -Action { Cleanup }

# Start Elixir application
Write-Host "Starting Elixir application..." -ForegroundColor Yellow
Write-Host "Open http://127.0.0.1:5050 in your browser" -ForegroundColor Green
Write-Host "Press Ctrl+C to stop the application" -ForegroundColor Yellow

# Check if elixir is available
try {
    $elixirPath = (Get-Command elixir -ErrorAction Stop).Source
    Write-Host "Found Elixir at: $elixirPath" -ForegroundColor Gray
} catch {
    Write-Error "Elixir not found in PATH. Please ensure Elixir is installed."
    Write-Host "Install with: winget install Elixir.Elixir" -ForegroundColor Yellow
    Cleanup
    exit 1
}

# Define the evaluation code for Elixir
$evalCode = 'spawn(fn ->
  wait = fn wait, n ->
    case Process.whereis(Translator.AudioEngine) do
      nil when n > 0 -> Process.sleep(100); wait.(wait, n - 1)
      nil -> IO.puts("AudioEngine not started after 30s")
      _pid -> IO.puts("AudioEngine ready (waiting for Start)")
    end
  end
  wait.(wait, 300)
end)'

try {
    # Run Elixir with mix using iex
    Write-Host "Starting Elixir with mix..." -ForegroundColor Gray
    
    # Use iex to run the application
    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = "iex"
    $startInfo.Arguments = "--eval `"$evalCode`" -S mix run --no-halt"
    $startInfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Normal
    $startInfo.UseShellExecute = $true
    $startInfo.WorkingDirectory = (Get-Location).Path
    
    $elixirProcess = [System.Diagnostics.Process]::Start($startInfo)
    
    if ($null -eq $elixirProcess) {
        throw "Failed to start Elixir process"
    }
    
    Write-Host "Elixir process started (PID: $($elixirProcess.Id))" -ForegroundColor Green
    
    # Wait for Elixir process to exit
    while (!$elixirProcess.HasExited) {
        Start-Sleep -Milliseconds 1000
        
        # Check if we need to exit
        if ([Console]::KeyAvailable) {
            $key = [Console]::ReadKey($true)
            if ($key.Key -eq 'C' -and $key.Modifiers -eq [ConsoleModifiers]::Control) {
                Write-Host "`nCtrl+C detected, shutting down..." -ForegroundColor Yellow
                break
            }
        }
    }
} catch {
    Write-Error "Failed to start Elixir application: $_"
    Write-Host "`nTroubleshooting:" -ForegroundColor Yellow
    Write-Host "1. Check if Elixir is installed: elixir --version" -ForegroundColor Yellow
    Write-Host "2. Check if Mix is installed: mix --version" -ForegroundColor Yellow
    Write-Host "3. Try running: iex -S mix run --no-halt" -ForegroundColor Yellow
    Cleanup
    exit 1
}

# Cleanup and exit
Cleanup
exit 0
