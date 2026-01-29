# Simple script to run the build with proper environment settings
# Run this script as Administrator if the regular build fails

$env:CARGO_TARGET_DIR = "C:\Users\steve\.cargo-target"

Write-Host "Starting Tauri development build..." -ForegroundColor Cyan
Write-Host "Target directory: $env:CARGO_TARGET_DIR`n" -ForegroundColor Gray

# Unblock any new build scripts that may have been downloaded
if (Test-Path "C:\Users\steve\.cargo-target\debug\build\") {
    Write-Host "Unblocking build scripts..." -ForegroundColor Yellow
    Get-ChildItem -Path "C:\Users\steve\.cargo-target\debug\build\" -Filter "*.exe" -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
        Unblock-File $_.FullName -ErrorAction SilentlyContinue
    }
    Write-Host "Done`n" -ForegroundColor Green
}

npm run tauri:dev
