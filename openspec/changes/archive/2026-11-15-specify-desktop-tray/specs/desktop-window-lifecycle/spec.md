## ADDED Requirements

### Requirement: The system tray offers Show and Quit

When the native desktop starts, it SHALL create a system tray icon (macOS menu bar, Windows notification area, Linux status area) showing the application icon with the tooltip "Chiral Network". Activating the icon SHALL open a menu with exactly two items, **Show Chiral** and **Quit Chiral…**. *Show Chiral* SHALL unminimize, show and focus the main window without changing the native session, active streams or pending funding. *Quit Chiral…* SHALL show and focus the main window and open the same quit confirmation as the in-window Quit control; it SHALL NOT end the process without that confirmation. If the platform cannot create a tray icon, the desktop SHALL log the reason and continue running without one; startup SHALL NOT fail because of the tray.

#### Scenario: Show restores a minimized window

- **GIVEN** the main window was minimized by closing it
- **WHEN** the user chooses Show Chiral from the tray menu
- **THEN** the window is restored, visible and focused
- **AND** the session, streams and pending funding are unchanged

#### Scenario: Tray Quit asks for confirmation

- **WHEN** the user chooses Quit Chiral… from the tray menu
- **THEN** the main window is shown and the quit confirmation opens
- **AND** the process keeps running until the user confirms

#### Scenario: Tray unavailable

- **GIVEN** the platform cannot create a system tray icon
- **WHEN** the desktop starts
- **THEN** it logs that the tray is unavailable and continues running
- **AND** closing the window still minimizes it to the taskbar

## MODIFIED Requirements

### Requirement: Quitting is an explicit, confirmed action

The native desktop shell SHALL expose a Quit control in every navigation layout, and the system tray menu SHALL offer Quit Chiral… as a second entry point to the same confirmation. Activating either SHALL open a confirmation stating that active streams are cancelled, generated tokens can still be charged, pending funding remains in the native journal, quitting does not refund provider balances, and closing the window instead keeps Chiral running. Only confirmation SHALL quit: the desktop SHALL first close the native session (`logout`, which cancels active streams) and clear the webview session, then destroy the main window so the process ends. Declining SHALL leave everything running. If closing the session fails, the desktop SHALL NOT destroy the window and SHALL report the error. While a confirmed quit is in progress, a close request SHALL be allowed to proceed rather than minimized. The browser preview SHALL NOT show the Quit control.

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

#### Scenario: Tray Quit uses the same confirmation

- **WHEN** the user chooses Quit Chiral… from the tray menu and then confirms
- **THEN** the same logout-before-destroy sequence runs and the process ends

### Requirement: Window permissions stay minimal

The main window's capability SHALL grant only the window-control permissions this lifecycle needs: minimizing (`core:window:allow-minimize`) and destroying after a confirmed quit (`core:window:allow-destroy`). The system tray SHALL be created and handled natively and SHALL NOT require any webview tray permission. The capability SHALL NOT add shell, filesystem, `core:tray:*`, global-shortcut or remote-page permissions for the window lifecycle or the tray.

#### Scenario: Capability grants minimize and destroy only

- **WHEN** the desktop capability file is inspected
- **THEN** its window permissions are exactly `core:window:allow-minimize` and `core:window:allow-destroy`
- **AND** it contains no tray permission
