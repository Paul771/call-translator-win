Write-Host "Killing audio_engine.exe..."
Stop-Process -Name "audio_engine" -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

$exePath = "C:\Users\Pavel\AppData\Roaming\npm\call-translator-win\native\audio_engine\target\release\audio_engine.exe"
$backupPath = $exePath + ".old"

if (Test-Path $exePath) {
    Write-Host "Renaming old binary..."
    Rename-Item -Path $exePath -NewName "audio_engine.exe.old" -Force
}

Write-Host "Compiling..."
cd "C:\Users\Pavel\AppData\Roaming\npm\call-translator-win\native\audio_engine"
cargo build --release 2>&1 | Select-Object -Last 3

cd "C:\Users\Pavel\AppData\Roaming\npm\call-translator-win"
Write-Host "`nNew binary info:"
$f = Get-Item $exePath -ErrorAction SilentlyContinue
if ($f) {
    Write-Host "  Compiled: $($f.LastWriteTime)"
    Write-Host "  Size: $($f.Length) bytes"
} else {
    Write-Host "  NOT FOUND!"
}
