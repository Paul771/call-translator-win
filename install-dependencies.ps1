# Realtime Call Translator - Install Dependencies Script
# This script installs all required dependencies for Windows

$ErrorActionPreference = "Continue"

Write-Host "=== Realtime Call Translator - Install Dependencies ===" -ForegroundColor Cyan

# 1. Check Chocolatey
Write-Host "`n[1/5] Checking Chocolatey..." -ForegroundColor Yellow
if (!(Get-Command choco -ErrorAction SilentlyContinue)) {
    Write-Host "Chocolatey not found. Installing..." -ForegroundColor Gray
    try {
        Set-ExecutionPolicy Bypass -Scope Process -Force
        [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072
        Invoke-Expression ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))
        Write-Host "Chocolatey installed." -ForegroundColor Green
    } catch {
        Write-Error "Failed to install Chocolatey. Please install manually from chocolatey.org"
        exit 1
    }
} else {
    Write-Host "Chocolatey is already installed." -ForegroundColor Green
}

# 2. Install Elixir
Write-Host "`n[2/5] Checking Elixir..." -ForegroundColor Yellow
if (!(Get-Command elixir -ErrorAction SilentlyContinue)) {
    Write-Host "Installing Elixir via Chocolatey..." -ForegroundColor Gray
    choco install elixir -y
    Write-Host "Elixir installed." -ForegroundColor Green
} else {
    Write-Host "Elixir is already installed." -ForegroundColor Green
}

# 3. Install Rust
Write-Host "`n[3/5] Checking Rust..." -ForegroundColor Yellow
if (!(Get-Command rustc -ErrorAction SilentlyContinue)) {
    Write-Host "Installing Rust..." -ForegroundColor Gray
    Invoke-WebRequest -Uri "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe" -OutFile "rustup-init.exe"
    Start-Process "rustup-init.exe" -ArgumentList "-y --default-toolchain stable" -Wait
    Remove-Item "rustup-init.exe" -Force
    $env:Path += ";$env:USERPROFILE\.cargo\bin"
    Write-Host "Rust installed." -ForegroundColor Green
} else {
    Write-Host "Rust is already installed." -ForegroundColor Green
}

# 4. Install Python dependencies
Write-Host "`n[4/5] Checking Python dependencies..." -ForegroundColor Yellow
if (Test-Path ".venv\Scripts\Activate.ps1") {
    Write-Host "Activating virtual environment..." -ForegroundColor Gray
    & .\.venv\Scripts\Activate.ps1
    Write-Host "Installing Python packages..." -ForegroundColor Gray
    pip install -r requirements.txt -q
    Write-Host "Python dependencies installed." -ForegroundColor Green
} else {
    Write-Warning "Virtual environment not found. Run setup.ps1 first."
}

# 5. Build project
Write-Host "`n[5/5] Building project..." -ForegroundColor Yellow
Write-Host "Fetching Elixir dependencies..." -ForegroundColor Gray
mix deps.get
Write-Host "Compiling Elixir and Rust..." -ForegroundColor Gray
mix compile

Write-Host "`n=== Installation Complete! ===" -ForegroundColor Green
Write-Host "Next steps:"
Write-Host "1. Copy .env.example to .env and add your API keys"
Write-Host "2. Run: .\run.ps1"
Write-Host "3. Open: http://127.0.0.1:5050"
