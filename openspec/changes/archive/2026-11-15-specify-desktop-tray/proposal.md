## Why

Closing the window now minimizes Chiral to the taskbar and keeps it running (`desktop-window-lifecycle`). The author asked for a system tray icon with **Show** and **Quit** so a background Chiral is always one click from being restored or ended, independent of the window. The archived window-lifecycle change listed a tray as a non-goal and its permission requirement forbids tray permissions "for this feature"; this change supersedes that non-goal while keeping the webview permission set unchanged. The book's Appendix A (A.1.10, A.2) records the decision.

## What Changes

- Create a native system tray icon at desktop startup: the app icon, tooltip "Chiral Network", and a menu with **Show Chiral** and **Quit Chiral…**.
- **Show Chiral** unminimizes, shows and focuses the main window without touching the session.
- **Quit Chiral…** shows and focuses the window and asks the webview to open the existing "Quit Chiral?" confirmation; only that confirmation quits.
- If the platform cannot create a tray, log the reason and keep running without one.
- Enable Tauri's `tray-icon` feature. The tray is built in Rust; the webview capability gains no `core:tray:*` permission. The once-per-session close hint mentions the tray.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `desktop-window-lifecycle`: adds the system tray requirement; the Quit requirement gains the tray entry point; the permission requirement states that the native tray needs no webview permission.

## Impact

- Native: `src-tauri/src/desktop.rs` (tray setup, menu dispatch, show/quit-request helpers, unit tests), `src-tauri/Cargo.toml` (`tray-icon` feature), `src-tauri/Cargo.lock`.
- Frontend: `src/App.svelte` listens for the native quit-request event; `src/lib/windowLifecycle.ts` exports the event name.
- Tests: Rust menu-dispatch tests; frontend event-name and unchanged-permission tests.
- Documentation: `docs/chiral-book.md` Appendix A — already updated.
- Out of scope: tray-only mode (hiding the window from the taskbar), close-to-tray hiding, start-at-login, tray status badges or balances, and routing operating-system quit commands (macOS Cmd+Q) through the confirmation.
