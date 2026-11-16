## Why

Closing the desktop window currently asks "Close Chiral?" and, if confirmed, logs out, cancels every active inference stream and ends the process. Users who close the window to tidy their screen therefore either quit by accident or face a modal on every close. The author decided that closing the window SHALL keep Chiral running by minimizing it to the operating-system taskbar (Dock on macOS), and that ending the program becomes a separate, explicit, confirmed **Quit** action. The book's Appendix A (A.1.10 and A.2) records that decision; no existing specification covers desktop window lifecycle.

## What Changes

- Intercept the main window's close request in the native desktop and minimize the window instead of destroying it. The native consumer session, loaded profile, active streams and pending funding recovery are unaffected.
- If minimizing fails, keep the window open and report the error; a close request never silently quits.
- Tell the user once per app session that Chiral is still running and how to exit.
- Add an explicit **Quit** control to the native shell (top bar, sidebar, mobile menus). It opens the existing confirmation modal, retitled "Quit Chiral?", whose confirmation closes the native session (`logout`, cancelling streams) and then destroys the window, ending the process.
- Grant the main window the `core:window:allow-minimize` permission; no other capability is added.

## Capabilities

### New Capabilities

- `desktop-window-lifecycle`: close-to-taskbar minimization, explicit confirmed quit, failure handling and the minimal window permission set for the native desktop shell.

### Modified Capabilities

None. Account, funding, credential and inference contracts are unchanged; quitting reuses the existing `logout` command and session-clearing behavior.

## Impact

- Frontend: `src/App.svelte`, new `src/lib/windowLifecycle.ts`, `src/lib/components/Navbar.svelte`, `src/lib/components/Sidebar.svelte`.
- Native permissions: `src-tauri/capabilities/default.json` gains `core:window:allow-minimize`.
- Tests: new `tests/windowLifecycle.test.ts`.
- Documentation: `docs/chiral-book.md` Appendix A (A.1.10, A.2) — already updated with the author's decision.
- Out of scope: a system-tray icon/menu, start-at-login, background operation after quit, and routing operating-system quit commands that bypass the window (for example macOS Cmd+Q) through the in-app confirmation.
