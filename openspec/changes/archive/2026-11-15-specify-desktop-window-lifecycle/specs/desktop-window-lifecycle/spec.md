## ADDED Requirements

### Requirement: Closing the main window minimizes it instead of quitting

When the native desktop's main window receives a close request (title-bar close button, Alt+F4, Cmd+W, or a taskbar "close window" action), the desktop SHALL prevent the window from being destroyed and SHALL minimize it to the operating-system taskbar (the Dock on macOS). The native runtime, the loaded consumer profile, active inference streams and pending funding recovery SHALL continue unchanged. A close request SHALL NOT invoke `logout`, cancel streams, clear the session or end the process. Restoring the window from the taskbar or Dock SHALL show the same session.

#### Scenario: Close keeps Chiral running

- **GIVEN** the native desktop is running with a loaded profile and an active inference stream
- **WHEN** the user closes the main window
- **THEN** the window is minimized and the process keeps running
- **AND** the stream, session and pending funding journal are unaffected

#### Scenario: First close explains how to exit

- **WHEN** the window is closed for the first time in an app session
- **THEN** the desktop shows an informational notification that Chiral is still running and that Quit exits
- **AND** later closes in the same session do not repeat it

### Requirement: A close request never silently quits

If minimizing the window fails, the desktop SHALL keep the close request prevented, leave the window open, and report the failure to the user. It SHALL NOT fall back to destroying the window.

#### Scenario: Minimize failure keeps the window open

- **GIVEN** the platform rejects the minimize request
- **WHEN** the user closes the main window
- **THEN** the window remains open and an error notification is shown
- **AND** the session and streams are unaffected

### Requirement: Quitting is an explicit, confirmed action

The native desktop shell SHALL expose a Quit control in every navigation layout. Activating it SHALL open a confirmation stating that active streams are cancelled, generated tokens can still be charged, pending funding remains in the native journal, quitting does not refund provider balances, and closing the window instead keeps Chiral running. Only confirmation SHALL quit: the desktop SHALL first close the native session (`logout`, which cancels active streams) and clear the webview session, then destroy the main window so the process ends. Declining SHALL leave everything running. If closing the session fails, the desktop SHALL NOT destroy the window and SHALL report the error. While a confirmed quit is in progress, a close request SHALL be allowed to proceed rather than minimized. The browser preview SHALL NOT show the Quit control.

#### Scenario: Confirmed quit closes the session before exiting

- **GIVEN** the user activates Quit and confirms
- **WHEN** the desktop quits
- **THEN** `logout` completes and the session is cleared before the window is destroyed
- **AND** the process ends

#### Scenario: Declined quit keeps running

- **WHEN** the user activates Quit and chooses to keep running
- **THEN** no session, stream or window state changes

#### Scenario: Session close failure aborts quit

- **GIVEN** `logout` fails during a confirmed quit
- **THEN** the window is not destroyed and the error is shown

### Requirement: Window permissions stay minimal

The main window's capability SHALL grant only the window-control permissions this lifecycle needs: minimizing (`core:window:allow-minimize`) and destroying after a confirmed quit (`core:window:allow-destroy`). It SHALL NOT add shell, filesystem, tray, global-shortcut or remote-page permissions for this feature.

#### Scenario: Capability grants minimize and destroy only

- **WHEN** the desktop capability file is inspected
- **THEN** its window permissions are exactly `core:window:allow-minimize` and `core:window:allow-destroy`
