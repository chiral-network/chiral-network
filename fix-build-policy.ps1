# PowerShell script to fix Windows Application Control policy blocking Cargo build scripts
# This script must be run as Administrator

param(
    [switch]$Force
)

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "Chiral Network - Fix Build Policy Script" -ForegroundColor Cyan
Write-Host "========================================`n" -ForegroundColor Cyan

# Check if running as Administrator
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if (-not $isAdmin) {
    Write-Host "ERROR: This script must be run as Administrator!" -ForegroundColor Red
    Write-Host "`nTo run as Administrator:" -ForegroundColor Yellow
    Write-Host "1. Right-click on PowerShell" -ForegroundColor White
    Write-Host "2. Select 'Run as Administrator'" -ForegroundColor White
    Write-Host "3. Navigate to: cd '$PSScriptRoot'" -ForegroundColor White
    Write-Host "4. Run: .\fix-build-policy.ps1" -ForegroundColor White
    exit 1
}

Write-Host "Running as Administrator - OK`n" -ForegroundColor Green

# Step 1: Unblock all existing build scripts
Write-Host "[1/4] Unblocking existing build scripts..." -ForegroundColor Yellow
$targetDir = "C:\Users\steve\.cargo-target"
if (Test-Path $targetDir) {
    try {
        Get-ChildItem -Path "$targetDir\debug\build\" -Filter "build-script-build.exe" -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
            Unblock-File $_.FullName -ErrorAction SilentlyContinue
        }
        Write-Host "   Build scripts unblocked" -ForegroundColor Green
    } catch {
        Write-Host "   Warning: Could not unblock some files: $_" -ForegroundColor Yellow
    }
} else {
    Write-Host "   Target directory not found (will be created on build)" -ForegroundColor Cyan
}

# Step 2: Set execution policy for current user
Write-Host "`n[2/4] Setting PowerShell execution policy..." -ForegroundColor Yellow
try {
    Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser -Force
    Write-Host "   Execution policy set to RemoteSigned" -ForegroundColor Green
} catch {
    Write-Host "   Warning: Could not set execution policy: $_" -ForegroundColor Yellow
}

# Step 3: Add exclusion to Windows Defender for cargo build directory
Write-Host "`n[3/4] Adding Windows Defender exclusion..." -ForegroundColor Yellow
try {
    # Add exclusion for cargo target directory
    Add-MpPreference -ExclusionPath "C:\Users\steve\.cargo-target" -ErrorAction Stop
    Write-Host "   Added exclusion for C:\Users\steve\.cargo-target" -ForegroundColor Green
    
    # Also add exclusion for cargo cache
    Add-MpPreference -ExclusionPath "$env:USERPROFILE\.cargo" -ErrorAction Stop
    Write-Host "   Added exclusion for $env:USERPROFILE\.cargo" -ForegroundColor Green
} catch {
    Write-Host "   Warning: Could not add Defender exclusions: $_" -ForegroundColor Yellow
    Write-Host "   You may need to add these manually in Windows Security" -ForegroundColor Yellow
}

# Step 4: Clean and prepare for build
Write-Host "`n[4/4] Cleaning build artifacts..." -ForegroundColor Yellow
$projectDir = "C:\Users\steve\Documents\chiral-network\src-tauri"
Push-Location $projectDir
try {
    $env:CARGO_TARGET_DIR = "C:\Users\steve\.cargo-target"
    cargo clean
    Write-Host "   Build artifacts cleaned" -ForegroundColor Green
} catch {
    Write-Host "   Warning: Could not clean: $_" -ForegroundColor Yellow
} finally {
    Pop-Location
}

Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "Setup Complete!" -ForegroundColor Green
Write-Host "========================================`n" -ForegroundColor Cyan

Write-Host "Next steps:" -ForegroundColor Yellow
Write-Host "1. Close this PowerShell window" -ForegroundColor White
Write-Host "2. In VS Code, run: npm run tauri:dev" -ForegroundColor White
Write-Host "`nIf you still encounter issues, you may need to:" -ForegroundColor Yellow
Write-Host "- Disable 'SmartScreen' temporarily in Windows Security" -ForegroundColor White
Write-Host "- Check if your organization has Group Policy restrictions" -ForegroundColor White
Write-Host "- Contact your system administrator" -ForegroundColor White
