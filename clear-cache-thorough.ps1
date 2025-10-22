# Thorough Windows Icon Cache Clearing Script

Write-Host "Stopping Explorer..."
taskkill /F /IM explorer.exe 2>&1 | Out-Null
Start-Sleep -Seconds 2

Write-Host "Clearing icon cache databases..."
$localAppData = [Environment]::GetFolderPath('LocalApplicationData')

# Remove Explorer icon cache
Get-ChildItem "$localAppData\Microsoft\Windows\Explorer" -Filter '*.db' -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "Removing: $($_.FullName)"
    Remove-Item $_.FullName -Force -ErrorAction SilentlyContinue
}

# Remove old-style IconCache.db
if (Test-Path "$localAppData\IconCache.db") {
    Write-Host "Removing: $localAppData\IconCache.db"
    Remove-Item "$localAppData\IconCache.db" -Force -ErrorAction SilentlyContinue
}

Write-Host "Restarting Explorer..."
Start-Process explorer.exe
Start-Sleep -Seconds 2

Write-Host "Refreshing icon cache..."
ie4uinit.exe -show

Write-Host "Done! Icon cache has been cleared."
