# Realtime Call Translator - Windows Setup Script
# This script installs dependencies and prepares the environment for Windows 10/11

$ErrorActionPreference = "Stop"

Write-Host "=== Realtime Call Translator - Windows Setup ===" -ForegroundColor Cyan
Write-Host "Note: If installation fails, run this script as Administrator." -ForegroundColor Yellow
Write-Host ""

# Function to safely add to PATH
function Add-ToPath {
    param([string]$Path)
    if ($env:Path -notlike "*$Path*") {
        $env:Path += ";$Path"
        [Environment]::SetEnvironmentVariable("PATH", $env:Path, "Process")
    }
}

# 1. Check and install Elixir using official installer
Write-Host "[1/5] Checking Elixir..." -ForegroundColor Yellow
$elixirInstalled = $false

if (Get-Command elixir -ErrorAction SilentlyContinue) {
    Write-Host "Elixir is already installed: $(elixir --version)" -ForegroundColor Green
    $elixirInstalled = $true
} else {
    Write-Host "Installing Elixir from official source..." -ForegroundColor Gray

    # Try winget first
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        try {
            Write-Host "Trying winget..." -ForegroundColor Gray
            winget install --silent --accept-source-agreements --accept-package-agreements Elixir.Elixir
            Start-Sleep -Seconds 5

            # Try to find and add Elixir to PATH
            $elixirPaths = @(
                "C:\Program Files\Elixir\bin",
                "C:\Program Files (x86)\Elixir\bin"
            )
            foreach ($path in $elixirPaths) {
                if (Test-Path "$path\elixir.exe") {
                    Add-ToPath $path
                    Write-Host "Added to PATH: $path" -ForegroundColor Green
                    $elixirInstalled = $true
                    break
                }
            }
        } catch {
            Write-Warning "winget installation failed: $_"
        }
    }

    # If winget didn't work, use official installer
    if (-not $elixirInstalled) {
        Write-Host "Downloading Elixir installer from GitHub..." -ForegroundColor Gray
        try {
            $installerUrl = "https://github.com/elixir-lang/elixir/releases/download/v1.16.2/elixir-otp-26-windows-x64.exe"
            Invoke-WebRequest -Uri $installerUrl -OutFile "elixir-installer.exe" -TimeoutSec 120

            Write-Host "Running installer..." -ForegroundColor Gray
            Start-Process ".\elixir-installer.exe" -ArgumentList "/S" -Wait

            # Add to PATH after installation
            $elixirPaths = @(
                "C:\Program Files\Elixir\bin",
                "C:\Program Files (x86)\Elixir\bin"
            )
            foreach ($path in $elixirPaths) {
                if (Test-Path "$path\elixir.exe") {
                    Add-ToPath $path
                    Write-Host "Elixir installed and added to PATH: $path" -ForegroundColor Green
                    $elixirInstalled = $true
                    break
                }
            }

            Remove-Item ".\elixir-installer.exe" -Force -ErrorAction SilentlyContinue
        } catch {
            Write-Warning "Failed to download/run Elixir installer: $_"
        }
    }
}

if (-not $elixirInstalled) {
    Write-Error "FAILED: Elixir installation failed. Please install manually:" -ForegroundColor Red
    Write-Host "1. Download from: https://github.com/elixir-lang/elixir/releases" -ForegroundColor Yellow
    Write-Host "2. Run the installer" -ForegroundColor Yellow
    Write-Host "3. Restart PowerShell and run this script again" -ForegroundColor Yellow
    exit 1
}

# 2. Install Rust
Write-Host "`n[2/5] Checking Rust..." -ForegroundColor Yellow
if (Get-Command rustc -ErrorAction SilentlyContinue) {
    Write-Host "Rust is already installed: $(rustc --version)" -ForegroundColor Green
} else {
    Write-Host "Installing Rust..." -ForegroundColor Gray
    Invoke-WebRequest -Uri "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe" -OutFile "rustup-init.exe"
    Start-Process ".\rustup-init.exe" -ArgumentList "-y --default-toolchain stable" -Wait
    Remove-Item ".\rustup-init.exe" -Force
    Add-ToPath "$env:USERPROFILE\.cargo\bin"
    Write-Host "Rust installed." -ForegroundColor Green
}

# 3. Install Python and espeak-ng via winget
Write-Host "`n[3/5] Checking Python..." -ForegroundColor Yellow
if (Get-Command python -ErrorAction SilentlyContinue) {
    Write-Host "Python is already installed: $(python --version)" -ForegroundColor Green
} else {
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        Write-Host "Installing Python via winget..." -ForegroundColor Gray
        winget install --silent --accept-source-agreements --accept-package-agreements Python.Python.3
    } else {
        Write-Error "Python not found and winget not available. Please install Python manually."
        exit 1
    }
}

Write-Host "`n[4/5] Checking espeak-ng..." -ForegroundColor Yellow
if (Get-Command espeak -ErrorAction SilentlyContinue) {
    Write-Host "espeak-ng is already installed." -ForegroundColor Green
} else {
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        Write-Host "Installing espeak-ng via winget..." -ForegroundColor Gray
        winget install --silent --accept-source-agreements --accept-package-agreements eSpeak.eSpeak
    } else {
        Write-Warning "espeak-ng not found and winget not available. TTS may not work."
    }
}

# 4. Setup Python Virtual Environment
Write-Host "`n[5/5] Setting up Python virtual environment..." -ForegroundColor Yellow
if (!(Test-Path ".venv")) {
    python -m venv .venv
    Write-Host "Venv created." -ForegroundColor Green
}

& .\.venv\Scripts\pip install -r requirements.txt -q
Write-Host "Python packages installed." -ForegroundColor Green

# 5. Environment File
if (!(Test-Path ".env")) {
    Copy-Item ".env.example" ".env"
    Write-Host ".env created from template." -ForegroundColor Yellow
}

# 6. Build Project
Write-Host "`n=== Building Project ===" -ForegroundColor Cyan
mix deps.get
mix compile

Write-Host ""
Write-Host "=== Setup Complete! ===" -ForegroundColor Green
Write-Host "Next steps:"
Write-Host "1. Edit .env with your API keys"
Write-Host "2. Run: .\run.ps1"
Write-Host "3. Open: http://127.0.0.1:5050"