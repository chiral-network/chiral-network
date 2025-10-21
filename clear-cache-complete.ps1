# Complete Icon Cache Clearing Script - Registry + Files

Write-Host "Clearing registry icon cache entries..."
$registryPaths = @(
    'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache',
    'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\BagMRU',
    'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags'
)

foreach($regPath in $registryPaths) {
    if(Test-Path $regPath) {
        Write-Host "Removing: $regPath"
        Remove-Item -Path $regPath -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host "`nStopping Explorer..."
taskkill /F /IM explorer.exe 2>&1 | Out-Null
Start-Sleep -Seconds 2

Write-Host "Clearing icon cache database files..."
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

Write-Host "`nRestarting Explorer..."
Start-Process explorer.exe
Start-Sleep -Seconds 2

Write-Host "Refreshing icon cache..."
ie4uinit.exe -show

Write-Host "`nDone! Icon cache completely cleared (registry + files)."
