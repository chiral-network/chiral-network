## Scope and authority

Authority is the author's decision recorded in `docs/chiral-book.md` Appendix A: A.1.10 ("Closing is not quitting") and the A.2 *Window close* and *Quit* entries. The book's global GUI rules (A.1) remain in force, in particular session scoping (A.1.8): quitting still closes the native session before the process ends.

Before this change, `src/App.svelte` registered `getCurrentWindow().onCloseRequested`, always called `preventDefault()`, and opened a "Close Chiral?" `<dialog>`; *Cancel requests and close* ran `logout`, cleared the session and called `destroy()`. The capability granted only `core:window:allow-destroy` for window control.

## Decisions

1. **Minimize, not hide or tray.** The request was to keep the program running in the system taskbar. Minimizing keeps the standard taskbar/Dock entry that restores the window with one click on every supported OS, needs no tray icon, menu or extra Tauri feature, and keeps the window discoverable. A tray icon remains possible future work.
2. **Intercept in the existing JS close listener.** Tauri 2's `onCloseRequested` destroys the window after the handler unless the handler calls `preventDefault()`. The handler now calls `preventDefault()` synchronously and then `minimize()`. Keeping the policy in one small module (`src/lib/windowLifecycle.ts`) with an injected window object makes it unit-testable without a native runtime.
3. **Fail closed, not quit.** If `minimize()` rejects (missing permission, unsupported window manager), the close stays prevented and the GUI shows an error toast. An accidental quit is worse than a window that stays visible.
4. **Explicit quit reuses the confirmed path.** The Quit control opens the existing confirmation (retitled "Quit Chiral?", buttons *Keep running* / *Cancel requests and quit*). Confirmation runs `logout` (which cancels active streams natively) and clears the webview session before `destroy()`. While a confirmed quit is in progress, a concurrent close request is allowed through instead of being minimized, so the process can end.
5. **One-time hint.** The first close-to-minimize in an app session shows an info toast ("Chiral is still running in the taskbar. Use Quit to exit."); later closes stay silent. The flag is in memory only.
6. **Minimal permission.** Add only `core:window:allow-minimize`. Restoring from the taskbar is an operating-system action and needs no webview permission.

## Non-goals and limitations

- Operating-system quit commands that bypass the window (macOS application-menu Quit / Cmd+Q, session logout/shutdown) are not routed through the in-app confirmation. Pending funding remains recoverable from the native journal; streams end with the process as before.
- No system-tray icon or menu, no start-at-login, no headless background operation after Quit.
- Tiling window managers that ignore minimize requests may leave the window visible; that is acceptable under decision 3.

## Verification plan

- Unit tests for the lifecycle helpers: close is prevented and minimized; minimize failure keeps the close prevented and reports the error; a close during confirmed quit is allowed through; quit closes the session before destroying the window and does not destroy it if closing the session fails.
- `npm run check`, `npm test`, `npm run build`.
- Native desktop build (Tauri validates capability identifiers at build time) and a native smoke test on macOS: a close request leaves the process running with the window minimized.
- Windows and Linux desktop smoke tests are recorded as not yet performed on this machine.

## Verification results

All six new unit tests and the existing suite pass (22 frontend tests in 5 files); Svelte checking reports zero errors and warnings; the frontend build and the native debug desktop build pass, the latter confirming `core:window:allow-minimize` is a valid capability identifier. A native macOS smoke test on the debug build, driven through System Events against the real window, observed: close → process alive and window minimized; restore → not minimized; close again → minimized; Quit → *Keep running* → process alive; Quit → *Cancel requests and quit* → process exited. The desktop log contained only the expected placeholder policy-key notice. The headless Rust suite was not rerun because no Rust source changed. Windows/Linux window-manager behavior, operating-system quit commands and the once-per-session toast's visibility after restore were not exercised natively.
