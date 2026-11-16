## Scope and authority

Authority is the author's request for a tray icon with Show and Quit, recorded in `docs/chiral-book.md` Appendix A (A.1.10 and the A.2 *System tray icon* entry), together with the existing `desktop-window-lifecycle` specification. Closing the window keeps minimizing to the taskbar; this change adds a second, window-independent way to restore or quit.

## Decisions

1. **Native tray, no webview permission.** Build the tray in Rust (`TrayIconBuilder` in the desktop `setup` hook) rather than through the JS tray API, so `src-tauri/capabilities/default.json` stays exactly as before: window permissions are minimize and destroy, and there is no `core:tray:*` permission.
2. **Menu, not click actions.** Clicking the icon opens the menu on every platform. Linux status areas only support menus, so a click-to-restore shortcut would behave differently per OS; one consistent menu is simpler to document and test.
3. **Show restores natively.** *Show Chiral* calls `unminimize`, `show` and `set_focus` on the `main` webview window from Rust. These are native calls and need no webview permission.
4. **Quit goes through the existing confirmation.** *Quit Chiral…* (the ellipsis signals a dialog) first restores the window, then emits `chiral://quit-requested` to it. The webview's listener opens the same "Quit Chiral?" dialog as the Quit button, so the tray adds no second, unconfirmed exit path and inherits logout-before-destroy and the failure handling. If the webview cannot handle the event, nothing quits; the user still sees the restored window.
5. **Tray failure is not fatal.** Tray creation can fail (for example no AppIndicator library on a Linux desktop). The builder runs inside `catch_unwind` and any error or panic is logged as `[TRAY] system tray unavailable: …`; startup continues. The default unwind panic strategy makes this effective.
6. **Pure dispatch for testing.** Menu item IDs map to actions through a pure function (`tray_action`), unit-tested without a native runtime.

## Non-goals and limitations

- No tray-only mode, close-to-tray hiding, start-at-login, badges or balances in the tray.
- Operating-system quit commands that bypass the window remain outside the confirmation, as before.
- Windows and Linux tray behavior was not exercised natively on this machine.

## Verification plan

- Rust unit tests for menu dispatch; headless and desktop builds.
- Frontend tests: quit-request event name; window permissions unchanged (no tray permission).
- `npm run check`, `npm test`, `npm run build`.
- Native macOS smoke test via System Events: the tray icon exists; *Show Chiral* restores a minimized window; *Quit Chiral…* opens the confirmation; confirming ends the process.

## Verification results

Svelte checking reports zero errors and warnings; 24 frontend tests pass (including the event-name and native-tray/permission checks); the frontend build passes; the three `desktop.rs` unit tests pass with the desktop feature; and the native debug build compiles without warnings. `Cargo.lock` was unchanged. A native macOS smoke test on the debug build observed the tray status item ("Chiral Network") with exactly *Show Chiral* and *Quit Chiral…*; close → minimized; *Show Chiral* → restored; *Quit Chiral…* while minimized → restored with the confirmation open; *Keep running* → process alive; *Quit Chiral…* → *Cancel requests and quit* → process exited. The desktop log contained only the expected placeholder policy-key notice. The headless Rust suite was not rerun (no headless code changed). Windows/Linux trays and the tray-unavailable fallback were not exercised natively.
