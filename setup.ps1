# Realtime Call Translator - Windows Setup Script
# This script installs dependencies and prepares the environment for Windows 10/11

$ErrorActionPreference = "Stop"

Write-Host "=== Realtime Call Translator - Windows Setup ===" -ForegroundColor Cyan
Write-Host "Note: After installation, you may need to restart PowerShell or your computer." -ForegroundColor Yellow
Write-Host ""

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

$elixirInstalled = $false
foreach ($dep in $dependencies.GetEnumerator()) {
    Write-Host "Checking $($dep.Key)..." -ForegroundColor Yellow
    if (!(Get-Command $dep.Key.ToLower() -ErrorAction SilentlyContinue)) {
        Write-Host "Installing $($dep.Key) via winget..." -ForegroundColor Gray
        winget install --silent --accept-source-agreements --accept-package-agreements $dep.Value
        
        # If Elixir was installed, try to add to PATH
        if ($dep.Key -eq "Elixir") {
            $elixirInstalled = $true
            # Common Elixir paths
            $elixirPaths = @(
                "C:\Program Files (x86)\Elixir\bin",
                "C:\Program Files\Elixir\bin",
                "$env:ProgramFiles\Elixir\bin",
                "${env:ProgramFiles(x86)}\Elixir\bin"
            )
            
            foreach ($path in $elixirPaths) {
                if (Test-Path $path) {
                    $env:Path += ";$path"
                    Write-Host "Added Elixir to PATH: $path" -ForegroundColor Green
                    break
                }
            }
        }
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

# Check if mix/elixir is now available
if (Get-Command mix -ErrorAction SilentlyContinue) {
    mix deps.get
    mix compile
    Write-Host "Build successful!" -ForegroundColor Green
} else {
    Write-Host ""
    Write-Host "WARNING: Elixir/Mix not found in PATH!" -ForegroundColor Red
    Write-Host ""
    if ($elixirInstalled) {
        Write-Host "Elixir was just installed but not in PATH yet." -ForegroundColor Yellow
        Write-Host "Please do ONE of the following:" -ForegroundColor Yellow
        Write-Host "  1. Close and reopen PowerShell, then run: .\setup.ps1 (again)" -ForegroundColor Yellow
        Write-Host "  OR" -ForegroundColor Yellow
        Write-Host "  2. Restart your computer and run: .\setup.ps1 (again)" -ForegroundColor Yellow
    } else {
        Write-Host "Please install Elixir manually:" -ForegroundColor Yellow
        Write-Host "  Option A: winget install Elixir.Elixir" -ForegroundColor Yellow
        Write-Host "  Option B: Download from https://elixir-lang.org/install.html" -ForegroundColor Yellow
        Write-Host ""
        Write-Host "After installing Elixir, restart PowerShell and run: .\setup.ps1" -ForegroundColor Yellow
    }
    exit 1
}

Write-Host ""
Write-Host "=== Setup Complete! ===" -ForegroundColor Green
Write-Host "Next steps:"
Write-Host "1. Edit .env with your API keys (copy .env.example to .env)"
Write-Host "2. Run the application:"
Write-Host "   - Option A (Recommended): .\run.ps1"
Write-Host "   - Option B (Git Bash): ./run.sh"
Write-Host "3. Open: http://127.0.0.1:5050"
