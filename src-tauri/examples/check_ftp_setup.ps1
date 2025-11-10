# FTP Demo Prerequisites Check
# Works on Windows

Write-Host "===================================" -ForegroundColor Cyan
Write-Host "  FTP Demo Prerequisites Check" -ForegroundColor Cyan
Write-Host "===================================" -ForegroundColor Cyan
Write-Host ""

# Check Python
Write-Host "Checking Python installation..." -ForegroundColor Yellow
$pythonFound = $false
$pythonCmd = ""

if (Get-Command python -ErrorAction SilentlyContinue) {
    $pythonVersion = python --version 2>&1
    Write-Host "  [OK] Python found: $pythonVersion" -ForegroundColor Green
    $pythonCmd = "python"
    $pythonFound = $true
} else {
    Write-Host "  [FAIL] Python NOT found" -ForegroundColor Red
    Write-Host "         Install from: https://www.python.org/downloads/" -ForegroundColor Yellow
}
Write-Host ""

# Check pyftpdlib
if ($pythonFound) {
    Write-Host "Checking pyftpdlib installation..." -ForegroundColor Yellow
    $pyftpCheck = & $pythonCmd -c "import pyftpdlib" 2>&1
    if ($LASTEXITCODE -eq 0) {
        Write-Host "  [OK] pyftpdlib found" -ForegroundColor Green
        $pyftpFound = $true
    } else {
        Write-Host "  [FAIL] pyftpdlib NOT found" -ForegroundColor Red
        Write-Host "         Install: pip install pyftpdlib" -ForegroundColor Yellow
        $pyftpFound = $false
    }
    Write-Host ""
}

# Check Rust/Cargo
Write-Host "Checking Rust installation..." -ForegroundColor Yellow
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    $cargoVersion = cargo --version
    Write-Host "  [OK] Cargo found: $cargoVersion" -ForegroundColor Green
    $cargoFound = $true
} else {
    Write-Host "  [FAIL] Cargo NOT found" -ForegroundColor Red
    Write-Host "         Install from: https://rustup.rs/" -ForegroundColor Yellow
    $cargoFound = $false
}
Write-Host ""

# Recommendation
Write-Host "===================================" -ForegroundColor Cyan
Write-Host "  Recommendation" -ForegroundColor Cyan
Write-Host "===================================" -ForegroundColor Cyan
Write-Host ""

if ($pythonFound -and $pyftpFound -and $cargoFound) {
    Write-Host "[OK] All prerequisites met!" -ForegroundColor Green
    Write-Host ""
    Write-Host "To run FTP demo:" -ForegroundColor Yellow
    Write-Host "  Terminal 1: python -m pyftpdlib -p 21 -w -d C:\FTP_Test"
    Write-Host "  Terminal 2: cargo run --example ftp_demo local"
} elseif ($cargoFound) {
    Write-Host "Some prerequisites missing." -ForegroundColor Yellow
    Write-Host ""
    if (-not $pythonFound -or -not $pyftpFound) {
        Write-Host "Option 1: Install missing prerequisites (see above)" -ForegroundColor Yellow
        Write-Host "Option 2: Use public FTP server (no Python needed):" -ForegroundColor Yellow
        Write-Host "  cargo run --example ftp_demo"
    }
} else {
    Write-Host "Rust/Cargo is required to run the demo." -ForegroundColor Red
    Write-Host "Install from: https://rustup.rs/" -ForegroundColor Yellow
}
Write-Host ""

Write-Host "Press any key to continue..."
$null = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")