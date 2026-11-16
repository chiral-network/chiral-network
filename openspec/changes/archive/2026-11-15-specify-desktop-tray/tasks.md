## 1. Specify the tray

- [x] 1.1 Record the author's tray decision in the book (Appendix A.1.10, A.2) and compare it with the archived window-lifecycle non-goal and permission requirement.
- [x] 1.2 Specify the tray requirement and the modified quit/permission requirements; validate with `openspec validate specify-desktop-tray --strict --no-interactive`.

## 2. Implement

- [x] 2.1 Enable Tauri's `tray-icon` feature. `Cargo.lock` needed no change: the tray crates were already locked as optional Tauri dependencies, so `--locked` builds are unaffected.
- [x] 2.2 Build the tray in `desktop.rs` (`setup` hook): app icon, tooltip, Show/Quit menu, pure `tray_action` dispatch, native show/unminimize/focus, `chiral://quit-requested` event, non-fatal failure logging inside `catch_unwind`.
- [x] 2.3 Listen for the quit-request event in `src/App.svelte` and open the existing confirmation; update the close hint to mention the tray.

## 3. Verify

- [x] 3.1 Rust unit test `tray_menu_ids_map_to_show_and_confirmed_quit_only`; frontend tests that the event name matches `desktop.rs` and that the tray is native (`tray-icon` feature) with no `core:tray` permission.
- [x] 3.2 `npm run check` (0 errors, 0 warnings), `npm test` (24 tests in 5 files), `npm run build`, `desktop.rs` unit tests with the desktop feature (3 passed), and the native debug desktop build (no warnings) pass.
- [x] 3.3 Native macOS (Apple Silicon) smoke test through System Events: the status-menu item exists with help text "Chiral Network" and menu items exactly "Show Chiral", "Quit Chiral…"; closing minimized the window and *Show Chiral* restored it; *Quit Chiral…* from a minimized window restored it and opened the confirmation; *Keep running* left the process alive; a second tray Quit with *Cancel requests and quit* ended the process. Windows and Linux tray behavior (including the no-AppIndicator fallback) were not exercised here.
