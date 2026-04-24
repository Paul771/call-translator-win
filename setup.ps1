# Realtime Call Translator - Windows Setup Script
# This script installs dependencies and prepares the environment for Windows 10/11

$ErrorActionPreference = "Stop"

Write-Host "=== Realtime Call Translator - Windows Setup ===" -ForegroundColor Cyan

# 1. Check for Package Manager (Winget is default on Win10/11)
if (!(Get-Command winget -ErrorAction SilentlyContinue)) {
    Write-Error "Winget not found. Please install App Installer from Microsoft Store."
    exit 1
}

# 2. Install System Dependencies
$dependencies = @{
    "Elixir" = "Elixir.Elixir"
    "Python" = "Python.Python.3"
    "espeak-ng" = "espeak-ng"
}

foreach ($dep in $dependencies.GetEnumerator()) {
    Write-Host "Checking $($dep.Key)..." -ForegroundColor Yellow
    if (!(Get-Command $dep.Key.ToLower() -ErrorAction SilentlyContinue)) {
        Write-Host "Installing $($dep.Key) via winget..." -ForegroundColor Gray
        winget install --silent --accept-source-agreements --accept-package-agreements $dep.Value
    } else {
        Write-Host "$($dep.Key) is already installed." -ForegroundColor Green
    }
}

# 3. Install Rust
if (!(Get-Command rustc -ErrorAction SilentlyContinue)) {
    Write-Host "Installing Rust..." -ForegroundColor Yellow
    # Download and install rustup
    Invoke-WebRequest -Uri "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe" -OutFile "rustup-init.exe"
    Start-Process "rustup-init.exe" -ArgumentList "-y --default-toolchain stable" -Wait
    Remove-Item "rustup-init.exe"
    # Refresh environment variables for current session
    $env:Path += ";$env:USERPROFILE\.cargo\bin"
} else {
    Write-Host "Rust is already installed." -ForegroundColor Green
}

# 4. Setup Python Virtual Environment
Write-Host "Setting up Python virtual environment..." -ForegroundColor Yellow
if (!(Test-Path ".venv")) {
    python -m venv .venv
    Write-Host "Venv created." -ForegroundColor Green
}

& .\.venv\Scripts\pip install -r requirements.txt
Write-Host "Python packages installed." -ForegroundColor Green

# 5. Environment File
if (!(Test-Path ".env")) {
    Copy-Item ".env.example" ".env"
    Write-Host ".env created from template. Please edit it with your API keys." -ForegroundColor Yellow
}

# 6. Build Project
Write-Host "Building project..." -ForegroundColor Yellow
mix deps.get
mix compile

Write-Host ""
Write-Host "=== Setup Complete! ===" -ForegroundColor Green
Write-Host "Next steps:"
Write-Host "1. Edit .env with your API keys"
Write-Host "2. Run: ./run.sh (or use a bash emulator/WSL)"
Write-Host "3. Open: http://127.0.0.1:5050"
