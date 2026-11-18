## ADDED Requirements

### Requirement: Page actions invoke their documented native operations

Each page action in the table below SHALL invoke the named native operation with the listed arguments (book lines 804 and 871–967). The listed arguments are those the native operation reads. A replacement GUI SHALL keep this mapping, whatever its pages, labels or layout (book lines 969–972). Opening a page or selecting an item SHALL invoke only operations that read. An operation that signs or pays, cancels a provider request or a funding transaction, issues, rotates or revokes a credential, submits feedback, writes a file, administers a provider or loads a profile SHALL run only when the user takes that action. This does not restrict the stream cancellations that other rules require without a cancel action: at the display limit (requirement *The Playground uses a funded account within its advertised terms*) and when a session ends (`desktop-gui-global-rules`, *Logout and a profile change end the previous session's view*). When an action fails, the page SHALL show the native error as `desktop-gui-global-rules` (*The desktop GUI is a thin view over the native command boundary*) requires (book line 873). A page MAY also read public data through a read-only native operation that returns no secret, as Usage reads `agreement` for the top-up disclosure that `desktop-gui-global-rules` (*Every opening and top-up discloses product risks and requires active acceptance*) requires. Amounts follow `desktop-gui-global-rules` (*Money crosses the native boundary as exact wei*). Every operation except `configure` waits for a loaded profile (`desktop-gui-global-rules`, *Network actions wait for an explicitly loaded profile*). Shell actions (log out, quit and the version gate) are outside this capability; `desktop-window-lifecycle` specifies quitting.

| Page | Action | Native operation and arguments |
|---|---|---|
| Marketplace | Discover | `discover {model}`: the filter text, or `null` for all models |
| Marketplace | Inspect an offer | `provider_info {offer}` |
| Marketplace | Read the funding journal | `pending_accounts` |
| Marketplace | Request a signed quote | `quote {offer, funding_wei}` |
| Marketplace | Fund after acceptance | `fund {offer, quote}` |
| Marketplace | Resume funding | `resume {contract_id}` |
| Playground | List accounts; select one | `accounts`; `account {contract_id}` |
| Playground | Generate (chat or completions) | `inference_stream` with the contract ID, the endpoint kind and a request with `stream: true`, usage included and the chosen maximum output tokens |
| Playground | Cancel stream | `inference_cancel` with the stream ID |
| Playground | Embed text | `infer {contract_id, kind: "embeddings", request}` |
| Playground | Reconcile | `request_status {contract_id, request_id}` |
| Usage | Open or refresh | `accounts`; `pending_accounts` |
| Usage | Select an account | `account`, `usage`, `credentials`, `funding` and `local_requests`, each with `{contract_id}` |
| Usage | Top up after acceptance | `topup {contract_id, amount_wei}` |
| Usage | Resume pending or journaled funding | `resume {contract_id}` |
| Usage | Fetch the signed receipt | `receipt {contract_id}` |
| Usage | Issue, rotate or revoke a credential | `credential_issue {contract_id, options}`; `credential_rotate {contract_id, credential_id, options}`; `credential_revoke {contract_id, credential_id}` |
| Usage | Reveal the SDK credential | `credential_export {contract_id, credential_id}`, where `null` selects the local default |
| Usage | Read status; cancel a provider request | `request_status {contract_id, request_id}`; `cancel_request {contract_id, request_id}`, where `request_id` is a request ID or a local request key |
| Usage | Inspect wallet recovery history | `funding_recovery {contract_id}` |
| Usage | Cancel unresolved funding after acceptance | `cancel_funding {contract_id, operation_id, accept_gas_and_race_risk: true}`, where `operation_id` is `null` when left blank |
| Usage | Submit feedback | `feedback {contract_id, score, revision}` |
| Wallet | Generate; import; back up | `wallet_create`; `wallet_import {path}`; `wallet_export {path}` |
| Wallet | Balance; local journal; faucet and explorer | `balance`; `history`; `network_profile` |
| Provider | Each operator action | `provider_admin {endpoint, token_file, action}`, where `action` is one of `status`, `offers`, `accounts`, `requests`, `fees`, `drain`, `resume`, `refresh` and `remit` |
| Account | Load local account index | `accounts` |
| Network | Check configured network | `network_status` |
| Settings | Load native configuration | `configure {path}` |
| Diagnostics | Collect native diagnostics | `diagnostics` |

#### Scenario: Each action reaches its documented operation

- **GIVEN** a loaded profile and a stand-in native boundary that records every call
- **WHEN** each action in the table is taken on its page
- **THEN** the recorded operation is the one listed for that action and carries the listed arguments with the values the user entered or selected

#### Scenario: Opening a page only reads

- **GIVEN** a loaded profile with a saved account and a journaled pending payment
- **WHEN** each page is opened, and an account is selected where the page offers one
- **THEN** only read operations are recorded
- **AND** no quote, fund, top-up, resume, credential, cancellation, feedback, wallet, provider administration or configure operation is recorded

### Requirement: The Marketplace discovers, inspects and funds in one gated order

The Marketplace SHALL describe discovery as a bounded search for candidate offers, not a complete directory (book lines 29 and 891). It SHALL show only offers that `discover` returned. Before any search it SHALL show an empty state that invites a search. After a search that returned no offer it SHALL show a different empty state, which says that no verified, unexpired offer was found (book line 891).

Inspecting an offer SHALL show, from `provider_info`, the provider's health (live, reachable but not live, or unreachable, with any reason), its readiness from `ready` only, and its testnet reputation as the Elo score and vote count or the reason that it is unavailable (book line 892). Readiness SHALL NOT be presented as new-funding readiness (book lines 252 and 254). An unavailable reputation SHALL be shown as unavailable with its reason, never as zero or a baseline score, and SHALL NOT be presented as evidence that the provider is untrustworthy (book line 443). The page SHALL state that readiness is a point-in-time observation, not a guarantee, and that reputation is experimental and weak against Sybil identities (book lines 33 and 892). The unparsed response SHALL stay available as a raw result (book line 869).

The funding amount SHALL default to the offer's minimum funding (book line 892). Funding SHALL follow this order (book lines 885–888 and 893):

1. The user requests a signed quote.
2. The quoted funding amount, the contract ID and the quote expiry are shown with the disclosure and the acceptance control.
3. Funding is enabled only after acceptance.

The disclosure and acceptance are those of `desktop-gui-global-rules` (*Every opening and top-up discloses product risks and requires active acceptance*) and `inference-funding-settlement` (*Funding ambiguity is disclosed and recovered on native surfaces* and *Settlement risk is disclosed before desktop funding and shown on affected accounts*). After a successful opening the page SHALL show the account's status and available balance, with a way to the Playground and a way to Usage (book line 893). After an interrupted opening, or while the journal lists an unresolved payment, the page SHALL meet *Funding ambiguity is disclosed and recovered on native surfaces*: it SHALL show the journaled stage from `pending_accounts`, warn the user not to send another payment, keep new quotes and funding disabled, and offer *Resume funding* (`resume {contract_id}`) instead (book line 894).

#### Scenario: Before and after a search

- **GIVEN** a loaded profile
- **WHEN** the Marketplace opens, and then a search returns no offer
- **THEN** the first state invites a search and the second says that no verified, unexpired offer was found
- **AND** the page says that discovery is a bounded search, not a directory

#### Scenario: Inspection shows observations with their limits

- **GIVEN** an offer whose provider is live and reports `ready: true`, and whose reputation is unavailable with the reason "no reputation catalog configured"
- **WHEN** the user inspects the offer
- **THEN** health is shown as live, readiness as ready, and reputation as unavailable with that reason, not as a score
- **AND** the page states that readiness is not a guarantee and that reputation is experimental and weak against Sybil identities

#### Scenario: The amount defaults to the minimum funding

- **GIVEN** an offer whose minimum funding is `1000000000000000000` wei
- **WHEN** the user inspects it
- **THEN** the amount field reads 1 CFX until the user changes it

#### Scenario: A successful opening leads to use

- **WHEN** funding succeeds and the native result is an `active` account with an available balance
- **THEN** the status and the available balance in CFX are shown, with a way to the Playground and a way to Usage

#### Scenario: A journaled payment offers only Resume

- **GIVEN** the native journal lists an unresolved opening for contract `X`, written in an earlier session
- **WHEN** the Marketplace opens and the user inspects an offer
- **THEN** the journaled stage of `X` and a warning not to send another payment are shown, with *Resume funding* for `X`
- **AND** requesting a quote and funding are disabled, and no `quote` or `fund` call is made

### Requirement: Offer and serving terms are shown where a page offers funding or use

Before the user requests a quote for an offer, the Marketplace SHALL show that offer's signed terms that a quote copies (book lines 480 and 867). Whether they appear in the listing, as today's offer cards show them, or only once the offer is inspected is presentation:

- the model ID and revision, the advertised capabilities and the context length;
- the provider wallet, the endpoint and the offer revision;
- the input and output prices per 1,000 tokens, the minimum funding and the expiry;
- the network identity: chain ID, deployment ID and genesis hash;
- the serving profile, or the statement that none applies.

The fee rate is part of the funding disclosure (`desktop-gui-global-rules`). For a libp2p offer (target design), the declared transports and peer ID take the place of an absent endpoint, as `p2p-provider-transport` (*The active path is disclosed before funding and during use*) requires. The target-design launch reputation on offer cards is specified by `provider-launch-reputation` (*The Marketplace shows the launch reputation beside, never blended into, the Elo score*).

A serving profile is shown compactly for an offer and for the Playground's account, and in full on the Usage overview (book lines 868, 906 and 929). Wherever it is shown, a profile with a serving commitment SHALL show (book line 868):

- its output tokens per second as a provider-qualified target per admitted request;
- the capacity tier and the admitted load (concurrency and requests per second);
- the engine and its version, the platform and backend, and the quantization;
- the location and the measured hardware: devices, memory, unified or dedicated memory and reserved headroom.

The qualification and remedy terms SHALL always be reachable, inline or behind one disclosure. The signed qualification provenance SHALL be labeled as provider measurements, not attestations. A model with no serving profile SHALL be shown with a statement that no generation-speed guarantee applies (book line 868). The target SHALL NOT be presented as end-to-end or Internet latency, and the capacity tier SHALL NOT be presented as a speed class (`inference-generation-performance`: *Policy `warm-generation-v1` defines a provider-side warm-generation class* and *A speed class rests only on qualification of the exact profile*). The profile fields are defined by `inference-commercial-terms` (*Complete serving profile identity*).

#### Scenario: Offer terms before a quote

- **GIVEN** a discovered offer
- **WHEN** the user inspects it, before requesting a quote
- **THEN** its model ID and revision, capabilities, context length, provider wallet, endpoint, offer revision, input and output prices per 1,000 tokens, minimum funding, expiry, chain ID, deployment ID, genesis hash and serving profile (or the statement that none applies) are visible

#### Scenario: A qualified serving profile

- **GIVEN** an offer whose serving profile declares 20 output tokens per second on one dedicated 8,188 MiB device, capacity tier `8gb`
- **WHEN** its serving profile is shown compactly
- **THEN** it reads as a provider-qualified target of 20 output tokens per second per admitted request, with the tier, load, engine, location and hardware
- **AND** the qualification and remedy terms, and the provenance labeled as provider measurements and not attestations, are reachable

#### Scenario: No serving profile

- **GIVEN** an embeddings offer whose `serving` is `null`
- **WHEN** it is shown
- **THEN** a statement that no generation-speed guarantee applies is shown, and no speed figure is shown

### Requirement: The Playground uses a funded account within its advertised terms

The Playground SHALL offer only endpoints that the selected account's model advertises: chat, completions or embeddings (book line 911). It SHALL NOT send a maximum output token count below 1 or above the model's `max_output_tokens` (book line 911). *Generate* SHALL be enabled only for an account whose status is `active` or `free` and whose model advertises the selected endpoint (book line 912). Chat and completions SHALL stream through the native stream with usage included, and *Cancel stream* SHALL cancel that stream natively (book line 912). Embeddings SHALL be requested without streaming through `infer`. Their input SHALL be 1 to 32 non-empty lines, one input per line; other input SHALL be refused with an explanation before any native call. The result SHALL be previewed with at most the first eight components of each vector (book line 912).

The request status SHALL distinguish these states (book line 913):

- idle, connecting, generating and completed;
- cancelling, and cancelled with generated tokens possibly billed;
- failed after a request ID, with the instruction to inspect that request's status before retrying;
- failed before a request ID, with the instruction to inspect Usage before retrying.

The failure and cancellation states follow `desktop-gui-global-rules` (*Ambiguous paid requests are reconciled, never resent*). The quoted status labels in the book are current presentation.

For an account that is not `active` or `free`, the page SHALL say that its funding cannot be spent and point to Usage. For a priced model whose available balance is zero, it SHALL point to Usage for a top-up (book line 914). Output beyond the page's display limit SHALL cancel the stream and tell the user to inspect usage before retrying; nothing is sent again (book line 914). *Reconcile* SHALL read the status of the shown request (book line 915). The page SHALL NOT persist prompts or responses: it SHALL NOT write them to webview storage, files or a history (book line 915).

#### Scenario: Only advertised endpoints

- **GIVEN** an account whose model advertises only `chat`
- **WHEN** the Playground shows it
- **THEN** completions and embeddings cannot be selected

#### Scenario: A frozen account cannot generate

- **GIVEN** a `frozen` account
- **WHEN** it is selected
- **THEN** *Generate* is disabled, and the page says that the account's funding cannot be spent and points to Usage

#### Scenario: Zero balance on a priced model

- **GIVEN** an `active` account with available balance `0` and a nonzero price
- **WHEN** it is selected
- **THEN** the page points to Usage for a top-up

#### Scenario: Embedding input is checked before sending

- **WHEN** the user submits 33 lines, or two lines one of which is empty, as embedding input
- **THEN** an explanation is shown and no `infer` call is made

#### Scenario: Maximum output tokens stay within the model limit

- **GIVEN** a model whose `max_output_tokens` is 256
- **WHEN** the user enters 257, or 0, and submits a chat request
- **THEN** no stream is started

#### Scenario: Output beyond the display limit

- **WHEN** a stream's text exceeds the page's display limit
- **THEN** the stream is cancelled, the user is told to inspect usage before retrying, and nothing is sent again

#### Scenario: Prompts are not persisted

- **WHEN** a generation completes, and the page is left and opened again
- **THEN** neither the prompt nor the output is shown, and webview storage holds neither

### Requirement: Usage shows each account and tops it up only through the disclosed form

With a loaded profile, Usage SHALL read `accounts` and `pending_accounts` when it opens. When the user selects an account, it SHALL read that account's `account`, `usage`, `credentials`, `funding` and `local_requests` (book line 927). The selected account's status SHALL be shown as text (book line 921). While the native journal lists pending funding, every section except the funding and recovery section SHALL show how many operations are pending, with a way to that section (book line 922). This is the Usage surface of `inference-funding-settlement` (*Funding ambiguity is disclosed and recovered on native surfaces*).

The overview SHALL show the selected account's funded amount, platform fee, spent amount, waived inference charges, amount reserved in flight, available balance, input and output token counts and contract ID, as the native account reports them, together with its full serving profile (book line 929). It SHALL explain that waived inference charges are not a refund or transferable credit and that funding fees and gas are unchanged (book line 929). It SHALL NOT describe the platform fee as received by the platform (`inference-account-snapshot`, *Account snapshots have one complete named-field schema*; `inference-fee-remittance`, *Fee collection is provider-remitted and not trustless*). An affected account SHALL show the warning that `inference-funding-settlement` (*Settlement risk is disclosed before desktop funding and shown on affected accounts*) requires.

The top-up form SHALL take a CFX amount and SHALL invoke `topup` only after the disclosure and acceptance that `desktop-gui-global-rules` (*Every opening and top-up discloses product risks and requires active acceptance*) and `inference-funding-settlement` require. While the account's own top-up is unresolved, the form SHALL offer only *Resume pending funding*. While another payment is unresolved, it SHALL offer no top-up (*Funding ambiguity is disclosed and recovered on native surfaces*; book lines 814 and 929). *Fetch signed receipt* SHALL show the natively verified receipt as an attributable provider statement, not as proof of execution or as a complete request history (book lines 357 and 929; `inference-request-accounting`, *Request and receipt history supports attributable recovery*).

#### Scenario: Selecting an account reads its data

- **GIVEN** a loaded profile with account `C`
- **WHEN** the user selects `C`
- **THEN** `account`, `usage`, `credentials`, `funding` and `local_requests` are read for `C`
- **AND** its status is shown as text

#### Scenario: The overview shows the native amounts

- **GIVEN** an account with funded `1000`, fee `5`, spent `0`, waived `0`, reserved `0` and available `995` wei, and 12 input and 34 output tokens
- **WHEN** its overview is shown
- **THEN** each amount appears in CFX as the native account reports it, with the token counts and the contract ID
- **AND** the waiver explanation and the full serving profile, or the statement that none applies, are shown

#### Scenario: Pending funding is reachable from every section

- **GIVEN** the native journal lists one pending operation
- **WHEN** the user is on the overview, API keys, requests or feedback section
- **THEN** the page says that one funding operation is pending and offers a way to the funding and recovery section

#### Scenario: The receipt is fetched on request

- **WHEN** the user fetches the signed receipt
- **THEN** `receipt` is invoked for the selected account and its natively verified result is shown

### Requirement: API credentials are managed as metadata and revealed only on request

Usage SHALL show the selected account's credentials as metadata from `credentials`, never as secrets (book line 930). *Issue* and *Rotate* SHALL send options with no spend cap (`spend_cap_wei: null`), concurrency 1, the scopes `infer` and `read`, and an expiry equal to the current time plus a lifetime that the user chooses (book line 930). The lifetime SHALL be a whole number of seconds from 60 seconds to 30 days (2,592,000 seconds), and SHALL default to one day (86,400 seconds). Any other value SHALL be refused with an explanation before any native call (book line 930). *Rotate* and *Revoke* SHALL act on the credential ID that the user names (book line 930). The page SHALL state that resume never rotates keys, and that hiding a revealed key does not revoke it, which `desktop-gui-global-rules` (*Secrets stay native except for an explicitly revealed credential*) also requires (book line 930). Issuing and resume follow `inference-funding-settlement` (*Fund and resume recover the saved agreement without rotating credentials*).

The key SHALL be obtained only through the explicit reveal (`credential_export`). It SHALL be shown masked, with its base URL, its model and a *Hide* action, as `desktop-gui-global-rules` (*Secrets stay native except for an explicitly revealed credential*) requires (book lines 808 and 930). Because the user reveals the key for SDK use (book line 808), the user SHALL be able to copy the revealed key while it stays masked on screen, for example with an explicit copy action (book lines 808 and 930).

The page SHALL offer Python and JavaScript OpenAI-SDK examples. They SHALL read `CHIRAL_API_KEY`, `CHIRAL_BASE_URL` and `CHIRAL_MODEL` from the environment, SHALL disable automatic retries and SHALL contain no key (book lines 520 and 930).

#### Scenario: Default issue options

- **WHEN** the user issues a credential without changing the lifetime
- **THEN** `credential_issue` receives an expiry one day after the current time, `spend_cap_wei` `null`, `max_concurrency` 1 and the scopes `infer` and `read`

#### Scenario: A lifetime outside the range is refused

- **WHEN** the user enters a lifetime of 5 seconds, 2,592,001 seconds or 1.5 seconds and issues or rotates a credential
- **THEN** an explanation is shown
- **AND** no `credential_issue` or `credential_rotate` call is made

#### Scenario: A revealed key can be copied while masked

- **GIVEN** a revealed key
- **WHEN** the user takes the copy action
- **THEN** the key is on the system clipboard
- **AND** the key is still masked on screen

#### Scenario: SDK examples hold no key

- **WHEN** the SDK examples are shown, before and after a reveal
- **THEN** they read the key, base URL and model from the environment, disable retries and contain no key

### Requirement: Usage reconciles requests and recovers journaled funding

For the selected account, Usage SHALL let the user read the status of a request by its request ID or its local request key (`request_status`) and cancel a provider request (`cancel_request`), presenting cancellation as possibly billed as `desktop-gui-global-rules` (*Ambiguous paid requests are reconciled, never resent*) requires. It SHALL show the account's metered usage and its local request keys, including ambiguous dispatches (book line 931).

The funding and recovery section SHALL be available whether or not an account is selected (book line 932). It SHALL:

- show the native pending journal with each operation's stage and transaction hash;
- let the user resume a journaled operation by its contract ID (`resume`);
- let the user inspect the wallet's recovery history for a contract (`funding_recovery`);
- show the selected account's settlement and reorganization evidence (`funding`).

Cancelling an unresolved funding transaction SHALL be separate from resuming and SHALL NOT be the primary recovery action. It SHALL be offered only after the warning that cancellation spends gas, can lose the race to the original payment and is not a refund, with an optional exact operation ID and an acceptance control that is not pre-selected. `cancel_funding` SHALL be invoked only after that acceptance (book lines 516 and 932). The cancellation rules are specified by `inference-funding-settlement` (*Explicit cancellation is a fixed-fee same-nonce self-transfer, not a refund*; *Cancellation targets only the intended operation*; *Resume and cancellation remain distinct and recovery history is retained*).

#### Scenario: Recovery without a selected account

- **GIVEN** a loaded profile with no saved account and a journaled opening for contract `X`
- **WHEN** the user opens the funding and recovery section, enters `X` and resumes
- **THEN** `resume {contract_id: X}` is invoked
- **AND** inspecting the recovery history invokes `funding_recovery {contract_id: X}`

#### Scenario: Cancellation needs its own acceptance

- **GIVEN** contract `X` entered in the funding and recovery section, with no operation ID
- **WHEN** the user tries to cancel without accepting, and then accepts and cancels
- **THEN** no `cancel_funding` call is made before the acceptance
- **AND** after it, `cancel_funding` receives `X`, `operation_id` `null` and `accept_gas_and_race_risk: true`
- **AND** the warning that cancellation spends gas, can lose the race and is not a refund was shown before the acceptance

#### Scenario: A local request key is reconciled

- **GIVEN** a local request key `K` listed for the account after an ambiguous dispatch
- **WHEN** the user reads its status
- **THEN** `request_status` receives `K` and the result is shown

### Requirement: Feedback is payment-backed and revisable

The feedback section SHALL submit a score that is a whole number from 0 to 100 and a revision that is a positive whole number, with `feedback {contract_id, score, revision}`. It SHALL refuse other values with an explanation before any native call (book line 933). It SHALL describe the feedback as payment-backed and subjective, and SHALL say that a revision must increase to replace earlier feedback (book lines 365, 413 and 933).

#### Scenario: Submitting feedback

- **GIVEN** a selected account `C`
- **WHEN** the user submits score 80 with revision 2
- **THEN** `feedback {contract_id: C, score: 80, revision: 2}` is invoked and the result is shown

#### Scenario: Out-of-range feedback is refused

- **WHEN** the user submits score 101, score 50.5 or revision 0
- **THEN** no `feedback` call is made

### Requirement: The Wallet page manages keys only through native file paths

With no wallet loaded, the Wallet page SHALL offer to generate a native wallet (`wallet_create`) and to import a protected private-key file by its path (`wallet_import {path}`). It SHALL state that importing does not convert old CHI or CHR balances (book line 937). With a wallet loaded, it SHALL show:

- the public address, with a copy action;
- on request, the verified network's balance in CFX, from `balance`;
- on request, the local transaction journal from `history`, listing each transaction's hash, recipient, value, nonce, status as text and creation time, with the statement that this is the local submitted-transaction journal, not complete chain history, and that pending, rejected and orphaned transactions are not spendable credit (book line 938).

Backup SHALL write the key to a new file path that the user enters (`wallet_export {path}`), after the statement that the file is owner-only but not encrypted by Chiral and must never be uploaded (book line 939). The native layer SHALL create the backup readable and writable only by its owner (on Unix, not group- or world-accessible; on Windows, owner-protected) and SHALL refuse to overwrite an existing file, so that the page's owner-only statement is true (book lines 474 and 939). The testnet funding section SHALL show the configured faucet and explorer from `network_profile`, or that none is configured. It SHALL state that no faucet authority is embedded, that Core and eSpace balances are separate, and that funding an eSpace wallet from Core needs an explicit transfer (book lines 31 and 940). No private key SHALL appear on the page (`desktop-gui-global-rules`, *Secrets stay native except for an explicitly revealed credential*).

#### Scenario: Import sends only a path

- **WHEN** the user imports `/keys/wallet.key`
- **THEN** `wallet_import` receives `{path: "/keys/wallet.key"}`
- **AND** the page shows the returned public address and no key

#### Scenario: Backup never overwrites

- **GIVEN** an existing file at the backup path
- **WHEN** the user exports the wallet to that path
- **THEN** the native error is shown and the existing file is unchanged
- **AND** an export to a new path creates a file that only its owner can read or write

#### Scenario: The journal is labeled as local

- **WHEN** the local transaction journal is shown
- **THEN** the page states that it is the local submitted-transaction journal, not complete chain history, and that pending, rejected and orphaned transactions are not spendable credit

#### Scenario: No faucet configured

- **GIVEN** a profile without `faucet_url`
- **WHEN** the user shows the configured faucet and explorer
- **THEN** the faucet is shown as not configured, and no other faucet is suggested

### Requirement: The Provider page administers a loopback operator listener through a token file

The Provider page SHALL take an operator endpoint, defaulting to `http://127.0.0.1:9421`, and the path of the protected administrator token file. It SHALL say that a remote provider is reached through an SSH tunnel and that the token is read natively and never returned (book line 944). Each of the nine operator actions SHALL invoke `provider_admin` and show the response as a raw result (book line 945). The native rules (a literal loopback endpoint only; the token file read natively and never returned) are `inference-fee-remittance` (*Explicit remittance is a local operator action on the loopback administration listener*) and `desktop-gui-global-rules` (*Secrets stay native except for an explicitly revealed credential*). Fee state follows `inference-fee-remittance` (*The operator fee view separates obligations from actual remittances*). The page SHALL state that not-ready models must not admit paid requests, that the operator drains before changing the native configuration, that existing prices and model revisions stay pinned, and that the dashboard installs nothing and exposes no shell (book lines 621–645 and 946).

In the target design, the Share dashboard of `desktop-share-online` (*The Share dashboard reports provider state through native administration*) is separate from this page.

#### Scenario: Actions send the token path, not the token

- **GIVEN** an administrator token file at `/secure/admin.token`
- **WHEN** the user runs the readiness and status action
- **THEN** `provider_admin` receives the endpoint, `token_file: "/secure/admin.token"` and `action: "status"`
- **AND** the response is shown as a raw result

#### Scenario: A non-loopback endpoint is refused

- **WHEN** the user enters `http://192.0.2.10:9421` and runs any action
- **THEN** the native error is shown in an error callout and no request is sent to that address

#### Scenario: The change-control notice is shown

- **WHEN** the Provider page is open
- **THEN** the not-ready, drain-first, pinned-terms and no-shell statements are visible

### Requirement: The Account page lists provider accounts without combining them

The Account page SHALL show the wallet's public address, or that no wallet is loaded, with a way to the Wallet page and a way to the Marketplace. It SHALL state that provider balances are separate, non-refundable and not wallet funds (book lines 33 and 950). On request it SHALL read `accounts` and show the number of accounts and the number that are `active` or `free`. For each account it SHALL show the model, provider wallet, status as text, available balance in CFX and contract ID, with a way to Usage for live data (book line 950). It SHALL NOT show a total of balances across providers (book line 950).

#### Scenario: Accounts of two providers

- **GIVEN** an `active` account with provider `P1` and 2 CFX available, and a `frozen` account with provider `P2` and 3 CFX available
- **WHEN** the local account index is loaded
- **THEN** 2 accounts and 1 active-or-free account are shown, each account with its own provider, status and balance
- **AND** no combined balance, such as 5 CFX, is shown

### Requirement: The Network page reports the settlement frontier of the configured chain

On request, the Network page SHALL read `network_status` and show the chain ID, the head, `safe` and `finalized` heights, the genesis hash, the acceptance level `safe`, whether the node is syncing and the observation time, with the raw result (book lines 954 and 973). It SHALL state that first execution is not spendable credit, that funding requires a successful canonical receipt at the safe frontier, and that finalized is reported separately (book line 954).

#### Scenario: Safe and finalized are separate

- **GIVEN** a native status with head 120, safe 110 and finalized 90
- **WHEN** the configured network is checked
- **THEN** 120, 110 and 90 are shown as separate heights, with the acceptance level `safe`
- **AND** the statement about first execution, the safe frontier and finality is shown

### Requirement: Settings loads a profile by path and states whether one is loaded

Settings SHALL offer the network profile, appearance and notification settings; a section menu showing one panel at a time is presentation (book line 958). Where Settings indicates whether a profile is loaded, today with a green or amber dot on the network profile menu entry (book line 958), the indicator SHALL carry that state as text, visibly or as its accessible name and tooltip, as `desktop-gui-global-rules` (*The GUI is accessible*) requires of every status indicator (book line 816). The network profile section SHALL take a consumer configuration file path and invoke `configure {path}` (book line 960). After a successful load the session is configured, the version policy is refreshed and pages remount, as `desktop-gui-global-rules` (*Logout and a profile change end the previous session's view*) specifies; that requirement also covers a failed load. The user SHALL be told of the successful load in a status message, as `desktop-gui-global-rules` (*The GUI is accessible*) requires for every status change, and that message SHALL still be shown after the remount (book lines 816 and 960). The section SHALL explain the profile fields (`network.identity`, `rpc_url`, the optional RPC bearer file, the optional faucet, explorer and policy URLs, `data_dir`, `catalog_url` and DHT) and state that no network, endpoint or funding source is invented or selected automatically (book lines 449 and 960).

The appearance section SHALL offer the light, dark and system themes, the accent colors, the top bar and sidebar layouts, and a reduce-motion switch (book line 961). The notification section SHALL offer a switch each for inference, funding and network status notifications, and a way to turn all of them on or off. Event-driven notifications SHALL respect these switches, and direct feedback to a user action SHALL always be shown (book lines 847 and 962). Preferences SHALL persist locally with only these choices (book line 963; `desktop-gui-global-rules`, *Secrets stay native except for an explicitly revealed credential*).

#### Scenario: Profile state is text

- **GIVEN** a Settings page that indicates the profile state on its network profile entry, as today's menu does
- **WHEN** it is shown with and without a loaded profile, with each section open
- **THEN** the entry's visible text or accessible name states whether a profile is loaded

#### Scenario: A successful load is announced

- **GIVEN** a valid consumer profile path
- **WHEN** the user loads it
- **THEN** after the pages remount, a `role="status"` message says that the profile was loaded

#### Scenario: Event notifications follow the switches

- **GIVEN** inference notifications are off
- **WHEN** a stream completes, and separately an action the user took fails
- **THEN** no notification is shown for the completed stream
- **AND** the failure is still reported

### Requirement: Diagnostics reports runtime state without secrets

The Diagnostics page SHALL show whether the native runtime is available without any native call (book line 967). On request it SHALL read `diagnostics` and show the application version, whether the configured chain is reachable, the number of active streams and whether a wallet is loaded, with the raw result (book line 967). It SHALL point to Usage and the Playground for request billing and to the Provider page for engine and fee state. It SHALL state that diagnostics contain no keys, bearer secrets, prompts or generated text (book line 967; `desktop-gui-global-rules`, *Secrets stay native except for an explicitly revealed credential*).

#### Scenario: Diagnostics tiles

- **GIVEN** a diagnostics result with version `2.0.0`, an unreachable configured chain, 2 active streams and a loaded wallet
- **WHEN** it is shown
- **THEN** the version, the chain as unavailable, 2 streams and the loaded wallet are shown as text, with the raw result

### Requirement: Presentation may change while required elements stay present and reachable

Layout, wording, color, iconography and the order of pages, cards and sections MAY change. A quoted label or message is required only where another capability requires its wording (book lines 802, 849–863 and 971). Every action, state, disclosure, acceptance control, confirmation and recovery path that this capability requires SHALL be present in every navigation layout and at every window width that the shell supports, including phone widths (book lines 838 and 971). Each SHALL be reachable by pointer and by keyboard. A section switcher that does not fit its width SHALL scroll or wrap so that every section can be selected.

#### Scenario: Usage at phone width

- **GIVEN** a window 390 pixels wide and a selected account
- **WHEN** the user moves through the Usage sections by pointer and by keyboard
- **THEN** each of the overview, API keys, requests, funding and recovery, and feedback sections can be selected, and its controls can be reached

#### Scenario: A redesign with different wording

- **GIVEN** a replacement GUI whose funding control is labeled "Pay and open"
- **WHEN** it is checked against this capability
- **THEN** it passes if that control invokes `fund` only after the required disclosure and acceptance, whatever its label
