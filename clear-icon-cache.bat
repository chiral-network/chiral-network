@echo off
REM Windows Icon Cache Clearing Script
REM Run this if you don't see updated app icons after rebuilding

echo ============================================
echo Clearing Windows Icon Cache
echo ============================================
echo.

echo [1/4] Stopping Windows Explorer...
taskkill /F /IM explorer.exe >nul 2>&1

echo [2/4] Clearing icon cache files...
cd /d "%userprofile%\AppData\Local\Microsoft\Windows\Explorer"
del iconcache*.db /a /f /q >nul 2>&1
del thumbcache*.db /a /f /q >nul 2>&1

echo [3/4] Refreshing system icons...
ie4uinit.exe -show >nul 2>&1

echo [4/4] Restarting Windows Explorer...
start explorer.exe

echo.
echo ============================================
echo Icon cache cleared successfully!
echo Please restart the Tauri app to see the new icons.
echo ============================================
echo.
pause
