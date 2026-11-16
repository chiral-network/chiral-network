## MODIFIED Requirements

### Requirement: Quitting is an explicit, confirmed action

The native desktop shell SHALL expose a Quit control in every navigation layout, and the system tray menu SHALL offer Quit Chiral… as a second entry point to the same confirmation. Activating either SHALL open a confirmation stating that active streams are cancelled, generated tokens can still be charged, pending funding remains in the native journal, quitting does not refund provider balances, and closing the window instead keeps Chiral running.

When a share is active or a qualification run is in progress, the confirmation SHALL also list each active share, with its model, path (direct, hole-punched or relayed), in-flight request count and exact outstanding prepaid liability, and each qualification run in progress, with its in-flight paid request count. It SHALL state that quitting stops sharing: new admission stops, published offers are withdrawn, in-flight requests may be interrupted and then settle under the known-usage or unknown-usage policy, the provider becomes unavailable, offers already distributed expire within minutes, and existing balances and obligations remain. For a share on a relayed path, it SHALL also give the relayed-grace statement that `desktop-share-online` defines (book *Author decisions* item 6): while the share's daemon drains before it stops, serving relays keep carrying consumers' request status, receipts, cancellation and use of existing accounts until 24 hours after this machine's most recent offer verified by the relay expires; after that, consumers may lose access to them through the relay unless a direct or hole-punched path is established; and a relay restart can end the grace earlier, because `p2p-provider-transport` keeps a relay's grace evidence in memory only. Once the daemon has stopped, the provider is unavailable on every path until this machine shares again. It SHALL also state that a qualification run is stopped and keeps its journal for reconciliation, so a later attempt needs a new budget approval.

Only confirmation SHALL quit. The desktop SHALL first stop every qualification run in progress: it stops new dispatch, waits for in-flight paid requests to complete or reach their request deadline, keeps the run's journal, and waits for the calibration daemon to exit through its graceful shutdown. It SHALL then stop every active share: it drains the share, withdraws its offers, and waits for the managed provider daemon to exit through its graceful shutdown with all provider state preserved. It SHALL release sleep prevention. It SHALL then close the native session (`logout`, which cancels active streams) and clear the webview session, and only then destroy the main window so the process ends. Declining SHALL leave everything running, including every share and qualification run. If stopping a qualification run or a share fails, or closing the session fails, the desktop SHALL NOT destroy the window and SHALL report the error, showing which shares or runs are still running. While shares and runs are being stopped during a confirmed quit, a close request SHALL minimize the window as usual. Once every share and run has stopped and the session is being closed, a close request SHALL be allowed to proceed rather than minimized.

While a share is active or a qualification run is in progress, and no confirmed quit has stopped it, the desktop SHALL prevent any application quit request that the platform lets it intercept, such as Cmd+Q or the macOS application menu's Quit. It SHALL keep running, show and focus the main window, and open this confirmation. Operating-system shutdown, restart or session end, and termination signals, SHALL NOT be prevented; `desktop-share-online` specifies how shares and runs are stopped for them. When no share is active and no run is in progress, this requirement does not change how application quit requests are handled. The browser preview SHALL NOT show the Quit control.

#### Scenario: Confirmed quit closes the session before exiting

- **GIVEN** the user activates Quit and confirms
- **WHEN** the desktop quits
- **THEN** `logout` completes and the session is cleared before the window is destroyed
- **AND** the process ends

#### Scenario: Declined quit keeps running

- **WHEN** the user activates Quit and chooses to keep running
- **THEN** no session, stream, share, qualification run or window state changes

#### Scenario: Session close failure aborts quit

- **GIVEN** `logout` fails during a confirmed quit
- **THEN** the window is not destroyed and the error is shown

#### Scenario: Tray Quit uses the same confirmation

- **WHEN** the user chooses Quit Chiral… from the tray menu and then confirms
- **THEN** the same logout-before-destroy sequence runs and the process ends

#### Scenario: Quit lists and stops active shares first

- **GIVEN** an active share with an account holding available balance
- **WHEN** the user activates Quit
- **THEN** the confirmation lists the share, its path, its in-flight requests and its exact outstanding liability
- **AND** after confirmation the share is drained and its daemon has exited before `logout` runs

#### Scenario: Quit with a relayed share gives the relayed-grace statement

- **GIVEN** an active share whose active path is relayed
- **WHEN** the user activates Quit
- **THEN** the confirmation states that while the share drains, serving relays keep carrying request status, receipts, cancellation and existing accounts until 24 hours after this machine's most recent offer verified by the relay expires, that a relay restart can end that earlier, and that after that consumers may lose access to them through the relay unless a direct or hole-punched path is established
- **AND** it states that once the daemon has stopped, the provider is unavailable on every path until this machine shares again

#### Scenario: Share stop failure aborts quit

- **GIVEN** a confirmed quit with an active share whose daemon does not exit
- **THEN** `logout` does not run, the window is not destroyed, and the error names the share that is still running

#### Scenario: Quit stops a qualification run first

- **GIVEN** a qualification run with paid requests in flight
- **WHEN** the user activates Quit and confirms
- **THEN** no new qualification request is dispatched, the in-flight requests complete or reach their deadline, and the calibration daemon exits before `logout` runs
- **AND** if the calibration daemon does not exit, `logout` does not run, the window is not destroyed and the error names the run

#### Scenario: Application quit command while sharing

- **GIVEN** an active share
- **WHEN** the user presses Cmd+Q or chooses Quit from the macOS application menu
- **THEN** the process keeps running, the main window is shown and the Quit confirmation lists the share
- **AND** the share keeps serving until the user confirms

### Requirement: The system tray offers Show and Quit

When the native desktop starts, it SHALL create a system tray icon (macOS menu bar, Windows notification area, Linux status area) showing the application icon with the tooltip "Chiral Network". While a share is active, the tooltip SHALL also state the sharing state and path, for example "Chiral Network — sharing (relayed)". It SHALL return to "Chiral Network" when no share is active. Activating the icon SHALL open a menu with exactly two items, **Show Chiral** and **Quit Chiral…**. *Show Chiral* SHALL unminimize, show and focus the main window without changing the native session, active streams, active shares or pending funding. *Quit Chiral…* SHALL show and focus the main window and open the same quit confirmation as the in-window Quit control; it SHALL NOT end the process without that confirmation. If the platform cannot create a tray icon, the desktop SHALL log the reason and continue running without one; startup SHALL NOT fail because of the tray.

#### Scenario: Show restores a minimized window

- **GIVEN** the main window was minimized by closing it
- **WHEN** the user chooses Show Chiral from the tray menu
- **THEN** the window is restored, visible and focused
- **AND** the session, streams, shares and pending funding are unchanged

#### Scenario: Tray Quit asks for confirmation

- **WHEN** the user chooses Quit Chiral… from the tray menu
- **THEN** the main window is shown and the quit confirmation opens
- **AND** the process keeps running until the user confirms

#### Scenario: Tray unavailable

- **GIVEN** the platform cannot create a system tray icon
- **WHEN** the desktop starts
- **THEN** it logs that the tray is unavailable and continues running
- **AND** closing the window still minimizes it to the taskbar

#### Scenario: Tooltip shows sharing state

- **GIVEN** an active share over a relayed path
- **WHEN** the user hovers over the tray icon
- **THEN** the tooltip states that Chiral is sharing over a relayed path
- **AND** after sharing stops the tooltip is "Chiral Network" again
