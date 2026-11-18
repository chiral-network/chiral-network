## ADDED Requirements

### Requirement: The desktop GUI is a thin view over the native command boundary

The desktop webview SHALL reach wallets, chains, catalogs, relays, providers and engines only through the native desktop commands (today `consumer_command`, `inference_stream`, `inference_cancel` and `get_version_status`) (book lines 799–804 and 972). It SHALL NOT sign, compute balances, fees or request costs, verify signatures or chain evidence, or decide admission or funding eligibility; it SHALL display what the native commands return (book line 804). The webview SHALL open no network connection of its own other than the native command channel and its bundled assets; it SHALL NOT load fonts, stylesheets, scripts or images from, or preconnect to, a remote origin. Its granted capabilities SHALL be limited to those native commands, listening to and unlistening from native events, and the window permissions that `desktop-window-lifecycle` allows (*Window permissions stay minimal*); a GUI feature SHALL NOT add a shell, filesystem, HTTP or other plugin permission.

When a native command that a page starts fails, the page SHALL show the native error text in full, with its line breaks, in an error callout, and SHALL NOT replace it with a fixed phrase (book lines 859 and 873). When a shell action fails (logging out, quitting, or minimizing the window on a close request), the GUI SHALL show the native error text in full in an error notification, as `desktop-window-lifecycle` requires for closing and quitting (book line 843).

Every requirement of this capability SHALL apply to every current page, to any page added later and to any replacement GUI, whatever its layout or toolkit (book lines 797–802 and 969–974). A replacement SHALL keep every required disclosure, acceptance control and recovery path: a different layout is allowed, but a missing warning or an extra unconfirmed payment path is not (book line 971).

#### Scenario: Network effects originate in the native process

- **GIVEN** the native desktop with a loaded consumer profile
- **WHEN** the user discovers offers, inspects a provider, requests a quote, funds an account, tops it up, issues a credential and streams a generation while the webview's network activity is recorded
- **THEN** every connection to a catalog, relay, RPC endpoint or provider is made by the native process
- **AND** the webview sends requests only to the native command channel and its bundled assets

#### Scenario: A native error is shown verbatim

- **GIVEN** a native command that fails with a two-line error message
- **WHEN** the page reports the failure
- **THEN** both lines appear unchanged in an error callout

#### Scenario: A replacement layout keeps the required controls

- **GIVEN** a redesigned page that offers paid opening funding
- **WHEN** the user reaches its funding step
- **THEN** every disclosure and acceptance control required by this capability and by the referenced funding capabilities is present
- **AND** no control prepares or broadcasts a payment before that acceptance

### Requirement: Secrets stay native except for an explicitly revealed credential

Wallet private keys, provider administrator tokens, account API credential secrets, RPC bearer tokens and the contents of any key or bearer file SHALL NOT enter webview state, URLs, webview storage (local storage, session storage, IndexedDB or cookies) or logs (book lines 808 and 474). The GUI SHALL pass the file path of such a file to the native command that needs it (wallet import and export, provider administration, the consumer profile) and SHALL receive only public data: native results SHALL carry addresses, metadata and status, never a private key, token or credential secret (book lines 474, 937–939 and 944). Credential issuance, rotation, revocation and listing SHALL return credential metadata only (book lines 345 and 930). Persisted GUI preferences SHALL contain no secret (book line 963). Diagnostics SHALL contain no key, bearer secret, prompt or generated text (book line 967).

An account's API key SHALL enter the webview only as the result of an explicit reveal action that the user takes for SDK use (book lines 808 and 930). The revealed key SHALL be shown in a masked field. It SHALL NOT be written to webview storage, a URL or a log, and SHALL NOT be embedded in a displayed code snippet. It SHALL be removed from webview state when the user hides it, selects another account or credential, or leaves the page, and on logout or a profile change (requirement *Logout and a profile change end the previous session's view*). Hiding SHALL NOT revoke the credential, and the GUI SHALL say so.

The other exception that book line 808 names, the target-design local API server bearer revealed on the Local server page, is specified by `desktop-local-api-server` (*Local API requests are authenticated by a protected, locally generated bearer*). This capability adds nothing to it.

#### Scenario: Native results carry no secret

- **GIVEN** a loaded wallet, a provider admin-token file, an RPC bearer file and an issued account credential whose secret values are known
- **WHEN** every native command other than the explicit credential reveal is exercised, and the webview's state, storage, URLs and console log are searched
- **THEN** none of the known secret values appears

#### Scenario: The administrator token stays native

- **GIVEN** a protected admin-token file
- **WHEN** the user runs every provider administration action
- **THEN** the webview sends only the file's path
- **AND** no native result or error returned to the webview contains the token

#### Scenario: A revealed key is masked and cleared

- **GIVEN** an account with a valid credential that has not been revealed
- **WHEN** the user reveals it
- **THEN** the key is shown in a masked field and is absent from webview storage, URLs and logs
- **AND** after Hide, selecting another account or credential, leaving the page, logout or a profile change, the key is no longer held in webview state
- **AND** after Hide the credential still authorizes inference

#### Scenario: Preferences hold no secret

- **GIVEN** stored preferences that also contain unknown fields such as a private key
- **WHEN** the GUI loads and saves its preferences
- **THEN** only the theme, accent, navigation layout, reduced-motion and notification choices are kept, and the unknown fields are not written back

### Requirement: The browser preview simulates nothing

Outside the native runtime the GUI SHALL show a notice that it is a browser preview in which native functionality is unavailable, and its status indicator SHALL report the preview state (book lines 809 and 840). Every native action SHALL either be disabled while that notice is shown or fail with an error that says the native desktop is required; no native action SHALL fail with an unexplained runtime error (book line 809). The GUI SHALL NOT fabricate wallets, addresses, offers, balances, quotes, accounts, credentials, receipts, request results, transport paths or model output, and SHALL NOT present placeholder values as observed data (book line 809).

#### Scenario: Every native action is refused with an explanation

- **GIVEN** the GUI running outside the native runtime
- **WHEN** each page is opened and each of its actions is attempted, including starting and cancelling a stream
- **THEN** the preview notice is visible
- **AND** each action is disabled or fails with an error that says the native desktop is required

#### Scenario: Nothing is fabricated

- **GIVEN** the GUI running outside the native runtime
- **WHEN** the marketplace, playground, usage, wallet, account, network and diagnostics pages are opened
- **THEN** no offer, balance, address, account, credential, receipt, request result or model output is shown
- **AND** every value that needs native data is shown as empty or unavailable

### Requirement: Network actions wait for an explicitly loaded profile

The GUI SHALL NOT select a network, RPC endpoint, catalog, faucet or provider implicitly (book lines 449 and 810). A consumer profile SHALL be loaded only from a file path that the user enters in Settings (`configure {path}`) (book lines 449 and 960). Until a profile is loaded, the GUI SHALL show a banner that points to Settings, SHALL disable every control that starts a network action, and SHALL NOT start a network action on its own, for example when a page opens (book lines 810 and 873). A network action is any native operation that needs the loaded consumer profile: discovery, provider inspection, quotes, funding, top-up, resume, funding cancellation and recovery history, account, usage, receipt and credential operations, inference and request status, feedback, wallet creation, import, export, balance and history, the network profile and network status, provider administration and diagnostics.

The native layer SHALL independently refuse every such operation while no profile is loaded.

In the target design, A.7 limits this rule to Chiral network actions (book line 978). Local-model pages and operations need no profile, as `desktop-local-workbench` (*Local mode works without a profile, wallet, chain or connectivity*) and `desktop-local-api-server` (*The Local server page controls the server without receiving secrets*) specify.

#### Scenario: No profile, no network actions

- **GIVEN** the native desktop with no consumer profile loaded
- **WHEN** each page is opened
- **THEN** a banner that points to Settings is shown
- **AND** every control that starts a network action is disabled
- **AND** opening the page invokes no network action

#### Scenario: The native layer refuses without a profile

- **GIVEN** no consumer profile is loaded
- **WHEN** a network operation is sent directly to the native command boundary
- **THEN** it is refused with an error that points to Settings
- **AND** nothing is sent to a catalog, relay, RPC endpoint or provider

#### Scenario: An explicit load enables network actions

- **GIVEN** the user enters a consumer profile path in Settings
- **WHEN** the profile loads successfully
- **THEN** the banner disappears and network actions become available
- **AND** every RPC, catalog, relay and provider inference endpoint used comes from that profile or from a natively verified offer, and provider administration uses only the operator endpoint the user entered on the Provider page (book line 944)

### Requirement: Money crosses the native boundary as exact wei

Every amount the GUI sends to a native command SHALL be a canonical decimal wei string as defined by `inference-wire-foundation` (*Exact monetary representation and CFX conversion*) (book lines 466 and 811). A CFX amount that the user enters SHALL be converted to wei with exact integer arithmetic. It SHALL be a non-negative decimal number with at most 18 fractional digits. Any other input, including a sign, an exponent, more than 18 fractional digits, a non-digit or a value outside the wei range, SHALL be refused with an explanatory error before any native call, and SHALL never be rounded or truncated (book line 811).

Every wei amount SHALL be displayed in CFX by exact conversion, without floating point and without dropping digits. A value that is not a canonical wei string SHALL be shown as unavailable, never as zero or as an estimate (book line 811). Comparisons that the GUI makes for display, such as the lowest offered price, SHALL use integer arithmetic. The GUI SHALL NOT compute balances, fees or costs; it SHALL display the amounts that native commands return (book line 804).

#### Scenario: Eighteen decimal places are exact

- **WHEN** the user enters `0.000000000000000001` CFX as a funding or top-up amount
- **THEN** the native command receives the wei string `"1"`

#### Scenario: Excess precision and exponents are refused before any native call

- **WHEN** the user enters `1.0000000000000000001`, `1e3` or `-1` as a funding or top-up amount
- **THEN** an explanatory error is shown
- **AND** no quote, fund or top-up command is invoked, and no native command receives the amount

#### Scenario: Large amounts display exactly

- **GIVEN** a native account whose available balance is `"9007199254740993000000000000000001"` wei
- **WHEN** the balance is displayed
- **THEN** it reads `9007199254740993.000000000000000001` CFX

#### Scenario: A malformed amount is not shown as a number

- **GIVEN** a native result whose amount is `"-1"`, `"1.5"`, `"00"`, `"01"` or `"340282366920938463463374607431768211456"`
- **WHEN** it is displayed
- **THEN** it is shown as unavailable, not as zero or as any other number

### Requirement: Untrusted content renders as plain text

Model output, provider and catalog metadata (model identifiers, endpoints, health, readiness and reputation messages, serving profiles and version-policy messages) and raw JSON responses SHALL be rendered as escaped plain text (book line 812). The GUI SHALL NOT interpret them as HTML, convert Markdown in them to HTML, load resources they reference, or execute or dispatch tool calls they contain (book line 812). Only the text content of a streamed response SHALL be displayed; tool calls and other structured content in a response SHALL NOT be rendered as actions or executed. Unparsed native responses SHALL appear only as JSON text in a raw result panel (book line 869).

The target-design chat and model-discovery pages follow the same rule through `desktop-local-chat` (*Chat output renders as untrusted plain text*) and `desktop-model-acquisition` (*Hugging Face discovery runs natively and renders as untrusted text*).

#### Scenario: Markup in model output

- **GIVEN** a streamed response whose text contains `<img src=x onerror=alert(1)>`, a `<script>` element and a Markdown image
- **WHEN** the response is displayed
- **THEN** each appears literally as text
- **AND** no element is created from it, no referenced URL is requested and no script runs

#### Scenario: Tool calls are not executed

- **GIVEN** a streamed response chunk that carries a tool call instead of text
- **WHEN** the chunk is processed
- **THEN** nothing is displayed for the tool call
- **AND** nothing is executed or dispatched

#### Scenario: Markup in provider metadata

- **GIVEN** an offer whose model identifier and endpoint contain HTML markup, and a raw native response that contains markup
- **WHEN** the offer card and the raw result panel are shown
- **THEN** the markup appears as text and no element is created from it

### Requirement: Every opening and top-up discloses product risks and requires active acceptance

Before any opening or top-up, every desktop surface that starts one SHALL show the following disclosure and SHALL require the user to actively accept it (book line 813):

- the funds are non-refundable (book line 33);
- the funds are usable only with this provider and its endpoint, naming the provider wallet and the endpoint; for a libp2p offer, the declared transports, peer ID and active path that `p2p-provider-transport` requires take the place of an endpoint (book lines 33 and 813);
- the provider sees prompts in plaintext (book line 33);
- signed metadata does not prove model execution (book line 33);
- the agreement's platform fee rate, shown exactly (50 basis points as 0.5%) (book lines 528 and 813);
- network gas is extra (book lines 528 and 813).

Today these surfaces are the Marketplace funding panel for an opening and the Usage top-up card for a top-up (book lines 883–888 and 929). Any surface added later that starts an opening or a top-up is included.

The acceptance control SHALL be shown with the complete disclosure, on the same surface and before the control that starts funding. It SHALL NOT be pre-selected, and it SHALL be reset when a different offer, quote or account is selected. The control that starts funding SHALL stay disabled until the user accepts.

The other A.1.6 items are specified elsewhere and are not restated here: the reorganization statement by `inference-funding-settlement` (*Settlement risk is disclosed before desktop funding and shown on affected accounts*), the opening registration risk by `inference-funding-settlement` (*Funding ambiguity is disclosed and recovered on native surfaces*), and the libp2p path and relay disclosure by `p2p-provider-transport` (*The active path is disclosed before funding and during use*). A surface SHALL meet those requirements as well. One acceptance control MAY cover all of them.

#### Scenario: Opening disclosure

- **GIVEN** a quoted offer from provider wallet `W` at endpoint `E` with a fee of 50 basis points
- **WHEN** the funding step is shown
- **THEN** it states that the funds are non-refundable, usable only with `W` at `E`, that the provider sees prompts in plaintext, that signed metadata does not prove model execution, that the platform fee is 0.5% and that gas is extra
- **AND** funding stays disabled until the user checks the acceptance control shown with that disclosure

#### Scenario: Top-up disclosure

- **GIVEN** an active account with provider wallet `W`, endpoint `E` and an agreed fee of 50 basis points
- **WHEN** the top-up control is shown
- **THEN** it states that the funds are non-refundable, usable only with `W` at `E`, that the provider sees prompts in plaintext, that signed metadata does not prove model execution, that the platform fee is 0.5% and that gas is extra
- **AND** the top-up stays disabled until the user checks the acceptance control shown with that disclosure

#### Scenario: Acceptance does not carry over

- **GIVEN** the user accepted the disclosure for one offer, quote or account
- **WHEN** the user selects a different offer, requests a new quote or selects a different account
- **THEN** the acceptance control is cleared and funding is disabled until the user accepts again

### Requirement: Ambiguous paid requests are reconciled, never resent

The GUI SHALL NOT resend a paid inference request on its own after an error, timeout, interrupted stream, lost response or cancellation, and SHALL NOT send it to another provider (book lines 520 and 814). After such an outcome, when a request ID was received, the GUI SHALL show it with a way to read that request's status and accounting through `request_status`, and SHALL tell the user to inspect the request before retrying (book lines 353, 520 and 814). When no request ID was received, for example because dispatch timed out or failed before a stream opened, the GUI SHALL tell the user to inspect Usage before retrying and SHALL point to where the account's local request keys, including ambiguous dispatches, are listed and their status can be read (book lines 913 and 931). A cancelled request SHALL be presented as possibly billed for generated tokens, whether a stream was cancelled or the request was cancelled by its ID (book lines 355, 913 and 931). Sending again SHALL require a new explicit user action (book line 520).

The funding part of book line 814 is specified elsewhere and is not restated here: surfacing journaled funding as *Resume* and never prompting for a second payment while one is unresolved by `inference-funding-settlement` (*Funding ambiguity is disclosed and recovered on native surfaces*), and labeling cancellation as gas-spending, racy and not a refund by `inference-funding-settlement` (*Explicit cancellation is a fixed-fee same-nonce self-transfer, not a refund* and *Resume and cancellation remain distinct and recovery history is retained*). Native no-replay is specified by `inference-request-accounting` (*Request identity prevents automatic paid replay*), and target-design chat threads by `desktop-local-chat` (*Network threads keep every consumer rule*).

#### Scenario: A stream error is not resent

- **GIVEN** a stream that received request ID `R` and then ended with an error
- **WHEN** the error is shown
- **THEN** `R` is shown with an action that reads its status, and the user is told to inspect it before retrying
- **AND** no further inference request is sent until the user starts one

#### Scenario: A failure before a request ID points to Usage

- **GIVEN** a chat or completion request that failed before any request ID was received, because its dispatch timed out or was interrupted with a native error that names local request key `K`
- **WHEN** the failure is shown
- **THEN** it tells the user to inspect Usage before retrying, the native error (including `K` when named) is shown, and the GUI points to where the account's local request keys and their status can be read
- **AND** nothing is resent

#### Scenario: A cancelled request may be billed

- **GIVEN** a stream that received request ID `R`
- **WHEN** the user cancels it and cancellation completes
- **THEN** the request is presented as possibly billed for generated tokens
- **AND** `R` and its status action remain available

#### Scenario: Cancelling a request by its ID

- **GIVEN** an account and a request ID `R` entered in the request reconciliation view
- **WHEN** the user cancels the provider request
- **THEN** the GUI states that the request may still be billed for generated tokens
- **AND** reading the status of `R` remains available

### Requirement: Logout and a profile change end the previous session's view

Logging out, or loading a consumer profile (including the same profile again), SHALL cancel every active inference stream of the previous session in the native layer (book line 815). The GUI SHALL clear displayed credentials, including a revealed API key, and the previous wallet identity, and SHALL remount the current page, so that no account, balance, credential, request, offer or other data of the previous network or session remains visible (book line 815). Logout SHALL route to Settings (book line 842). Only a successful profile load is a profile change: a load that fails SHALL leave the current session and its view unchanged.

In the target design, A.7 limits this rule to network sessions (book line 978); `desktop-local-chat` (*Chat streams follow session scoping and the window lifecycle*), `desktop-local-api-server` (*Network routes follow the native session and network scope*) and `desktop-share-online` (*Closing the window keeps sharing running*, which refuses logout and profile switching while a share is active) specify the target-design pages. Closing the main window is not a logout (`desktop-window-lifecycle`).

#### Scenario: Logout clears the view and cancels streams

- **GIVEN** an active stream, a revealed API key and a page showing an account
- **WHEN** the user logs out
- **THEN** the native layer cancels the stream
- **AND** Settings is shown and no account data, credential or wallet address of the previous session is visible

#### Scenario: Loading another profile replaces the session

- **GIVEN** profile A is loaded, a page shows one of its accounts and a stream is active
- **WHEN** the user loads profile B
- **THEN** the stream is cancelled and the current page is remounted
- **AND** no data from profile A is visible

#### Scenario: A failed load changes nothing

- **GIVEN** profile A is loaded and a stream is active
- **WHEN** the user tries to load a profile path that fails to load
- **THEN** the error is shown
- **AND** profile A stays loaded and the stream continues

### Requirement: The GUI is accessible

The GUI SHALL meet these rules on every page (book line 816):

- Every input, select and text area SHALL have a visible label that is programmatically associated with it.
- A status change SHALL be exposed with `role="status"` (or an equivalent polite live region), and a failure with `role="alert"`. This includes notifications: a failure notification SHALL use `role="alert"`, and a success or information notification SHALL use `role="status"`.
- Navigation SHALL expose `aria-current="page"` on the active item in every navigation layout and menu.
- A "Skip to content" link SHALL be the first focusable element and SHALL move focus to the main content region.
- Color SHALL never be the only signal: every status badge and status indicator SHALL carry text. Where a layout shows an indicator as an icon or dot only (book line 838), its text SHALL be its accessible name and its tooltip.
- When the operating system requests reduced motion, or the in-app Reduce motion setting is on (book line 961), the GUI SHALL NOT animate transitions, slide notifications in or out, or scroll smoothly; changes SHALL appear without motion.

#### Scenario: Every control has a visible label

- **WHEN** every input, select and text area on every page, in every state that shows it, is enumerated
- **THEN** each has a non-empty visible label that is associated with it

#### Scenario: Failures and status changes are announced with the right role

- **WHEN** a native action fails
- **THEN** its error is in a `role="alert"` region
- **AND** when an action succeeds or a status changes (a funding stage, an account or stream status, a pending-funding notice, or a success or information notification), the message is in a `role="status"` region

#### Scenario: Navigation and skip link

- **WHEN** a page is opened from navigation in the top bar, the More menu, the sidebar or the mobile menu
- **THEN** that page's item carries `aria-current="page"`
- **AND** the first Tab press focuses "Skip to content", which moves focus to the main region

#### Scenario: Reduced motion

- **GIVEN** the operating system requests reduced motion, or the Reduce motion setting is on
- **WHEN** a notification appears and the page scrolls to a newly opened panel
- **THEN** the notification appears without sliding and the page moves without smooth scrolling

#### Scenario: Status is not conveyed by color alone

- **WHEN** a status badge or status indicator is shown in any layout
- **THEN** its status is available as text, visibly or as its accessible name and tooltip
