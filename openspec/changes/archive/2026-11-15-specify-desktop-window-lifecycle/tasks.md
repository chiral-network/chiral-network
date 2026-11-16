## 1. Specify the window lifecycle

- [x] 1.1 Record the author's close-to-taskbar decision in the book (Appendix A.1.10, A.2) and compare it with the current `onCloseRequested` handler, close dialog and capability permissions.
- [x] 1.2 Specify close-to-minimize, failure handling, explicit confirmed quit and minimal permissions; validate with `openspec validate specify-desktop-window-lifecycle --strict --no-interactive`.

## 2. Implement

- [x] 2.1 Add `src/lib/windowLifecycle.ts` with injectable close-request and quit helpers (`handleCloseRequest`, `quitApplication`).
- [x] 2.2 Wire `src/App.svelte`: minimize on close with a once-per-session hint, error toast on failure, allow close during a confirmed quit, retitled "Quit Chiral?" confirmation (*Keep running* / *Cancel requests and quit*).
- [x] 2.3 Add the Quit control to the top bar, sidebar and mobile menus (native runtime only). Between 1280 and 1439 px the top bar shows Log out and the status pill icon-only (screen-reader labels kept) so the added control does not overflow.
- [x] 2.4 Grant `core:window:allow-minimize` in `src-tauri/capabilities/default.json`; window permissions are exactly minimize and destroy.

## 3. Verify

- [x] 3.1 Unit tests for close prevention/minimize, minimize failure, close during quit, quit ordering/failure and the capability permission set: `tests/windowLifecycle.test.ts` (6 tests).
- [x] 3.2 `npm run check` (0 errors, 0 warnings), `npm test` (22 tests in 5 files), `npm run build`, and the native debug desktop build (`tauri build --debug --no-bundle`, which validates capability identifiers) pass.
- [x] 3.3 Native macOS (Apple Silicon) smoke test through System Events on the debug build: clicking the window's close button left the process running with `AXMinimized = true`; restoring and closing again minimized again; Quit → *Keep running* left the process running; Quit → *Cancel requests and quit* ended the process. Windows and Linux desktop smoke tests were not performed here and remain open platform verification.
