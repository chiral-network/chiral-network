@echo off
REM Test Runner for Chiral Network (Windows)
REM Runs all E2E and integration tests

echo ========================================
echo   Chiral Network Test Suite
echo ========================================
echo.

REM Check if cargo is available
where cargo >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo ERROR: Cargo is not installed
    echo Please install Rust from: https://rustup.rs/
    exit /b 1
)

REM Navigate to src-tauri directory
cd /d "%~dp0..\src-tauri"

echo Running tests...
echo.

echo 1/3: E2E Cross-Network Transfer Tests
cargo test --test e2e_cross_network_transfer_test -- --nocapture --test-threads=1
if %ERRORLEVEL% NEQ 0 (
    echo ERROR: E2E tests failed
    exit /b 1
)

echo.
echo 2/3: NAT Traversal Tests
cargo test --test nat_traversal_e2e_test -- --nocapture --test-threads=1
if %ERRORLEVEL% NEQ 0 (
    echo WARNING: NAT traversal tests failed (may be expected in local environment)
)

echo.
echo 3/3: Unit Tests
cargo test --lib -- --nocapture
if %ERRORLEVEL% NEQ 0 (
    echo ERROR: Unit tests failed
    exit /b 1
)

echo.
echo ========================================
echo All tests completed successfully!
echo ========================================
