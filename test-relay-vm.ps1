# PowerShell script to test relay server functionality on Windows
# This script tests the auto-enable relay server feature when public IP is detected

Write-Host "🧪 Chiral Network Relay Server Test (Windows)" -ForegroundColor Green
Write-Host "==============================================" -ForegroundColor Green
Write-Host ""

# Check if we're in the right directory
if (-not (Test-Path "src-tauri")) {
    Write-Host "❌ Error: Please run this script from the project root directory" -ForegroundColor Red
    exit 1
}

# Build the application if needed
$releaseBinary = "src-tauri\target\release\chiral-network.exe"
$debugBinary = "src-tauri\target\debug\chiral-network.exe"

if (-not (Test-Path $releaseBinary) -and -not (Test-Path $debugBinary)) {
    Write-Host "📦 Building Chiral Network..." -ForegroundColor Yellow
    Set-Location src-tauri
    cargo build --release
    if ($LASTEXITCODE -ne 0) {
        Write-Host "❌ Build failed!" -ForegroundColor Red
        exit 1
    }
    Set-Location ..
}

# Use release binary if available, otherwise use debug
$binary = if (Test-Path $releaseBinary) { $releaseBinary } else { $debugBinary }

Write-Host ""
Write-Host "✅ Binary found: $binary" -ForegroundColor Green
Write-Host ""

# Set environment variable to enable AutoNAT (for VM mode)
$env:CHIRAL_ENABLE_AUTONAT = "1"
Write-Host "🔧 Environment variable set: CHIRAL_ENABLE_AUTONAT=1" -ForegroundColor Cyan
Write-Host ""

# Optional: Set VM mode flag (alternative way)
# $env:CHIRAL_VM_MODE = "1"

Write-Host "🚀 Starting Chiral Network in headless mode..." -ForegroundColor Yellow
Write-Host "   - AutoNAT will be enabled automatically" -ForegroundColor Gray
Write-Host "   - Relay server will be created in standby mode" -ForegroundColor Gray
Write-Host "   - When public IP is detected, relay service will be advertised in DHT" -ForegroundColor Gray
Write-Host ""
Write-Host "Press Ctrl+C to stop" -ForegroundColor Yellow
Write-Host ""

# Run the application in headless mode
& $binary `
    --headless `
    --dht-port 4001 `
    --log-level info `
    --show-multiaddr `
    --show-reachability

Write-Host ""
Write-Host "✅ Test completed" -ForegroundColor Green

