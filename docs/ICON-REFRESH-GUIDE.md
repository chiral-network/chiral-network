# Icon Refresh Guide

This guide explains how to handle Windows icon caching issues for both developers and end users.

## Problem

Windows aggressively caches application icons. When you update your app icon and rebuild, you might still see the old icon because Windows is using a cached version.

## Solutions

### For Developers (During Development)

#### Quick Method - Use the Script
Simply run the provided batch script:
```bash
clear-icon-cache.bat
```

This will:
1. Stop Windows Explorer
2. Delete icon cache files
3. Refresh system icons
4. Restart Windows Explorer

#### Manual Method
If you prefer to do it manually:

1. **Close all instances of the app**
   - Check taskbar and Task Manager

2. **Rebuild the app**
   ```bash
   npm run tauri dev
   # or
   npm run tauri build
   ```

3. **Clear the icon cache**
   - Open Command Prompt as Administrator
   - Run: `ie4uinit.exe -show`

4. **Alternative: Delete cache files**
   - Navigate to: `%localappdata%\Microsoft\Windows\Explorer`
   - Delete all `iconcache*.db` and `thumbcache*.db` files
   - Restart Windows Explorer (Task Manager → Windows Explorer → Restart)

5. **Restart your computer** (most reliable method)

---

### For End Users (After Installation)

#### Automatic Solution ✅

The Windows installer automatically clears the icon cache during installation, so users should see the correct icons immediately.

This is handled by the NSIS installer hook (`nsis-installer-hook.nsi`).

#### Manual Solution (If Needed)

If users still see old icons after installing an update:

1. **Close the application completely**

2. **Restart Windows** (easiest solution)

3. **Or manually clear cache:**
   - Press `Win + R`
   - Type: `ie4uinit.exe -show`
   - Press Enter
   - Restart the application

---

## Technical Details

### Why This Happens

Windows caches icons in several locations:
- `%localappdata%\Microsoft\Windows\Explorer\iconcache*.db`
- `%localappdata%\Microsoft\Windows\Explorer\thumbcache*.db`
- Icon Overlay cache (for shell extensions)

These caches are designed to improve performance but can prevent new icons from appearing after updates.

### Our Solution

1. **Development:** Use `clear-icon-cache.bat` script
2. **Distribution:** NSIS installer automatically runs `ie4uinit.exe -show` on install/update
3. **User Support:** Document restart as the easiest solution

### Platform-Specific Notes

- **Windows:** Icon caching is aggressive (addressed by our solutions above)
- **macOS:** Icons update more reliably, but if issues occur: `sudo rm -rf /Library/Caches/com.apple.iconservices.store`
- **Linux:** Icons typically update immediately, cached in `~/.cache/icon-cache.kcache` (KDE) or similar

---

## Files

- `clear-icon-cache.bat` - Developer utility script
- `src-tauri/nsis-installer-hook.nsi` - NSIS installer hook for automatic cache clearing
- `src-tauri/tauri.conf.json` - Configured to use the NSIS hook via `installerHooks` property

## Icon Source Files

- `src-tauri/icons/icon_windows.svg` - Source for Windows icons (transparent background)
- `src-tauri/icons/icon_macos.svg` - Source for macOS icons (with background gradient)
- `regenerate-final.cjs` - Script to regenerate all icon files from SVG sources

---

## Troubleshooting

**Q: I cleared the cache but still see old icons**
- Try restarting your computer (most reliable)
- Make sure the app is completely closed (check Task Manager)
- Ensure you rebuilt the app after changing icons

**Q: Users report seeing old icons after updating**
- Verify the NSIS hook is properly configured in `tauri.conf.json`
- Ask users to restart their computer after installing the update
- Check that new builds include the updated icon files

**Q: How do I verify which icon file is being used?**
- Check the compiled installer or executable properties
- Look at `src-tauri/tauri.conf.json` → `bundle.icon`
- Inspect the built installer/app in `src-tauri/target/release/bundle/`
