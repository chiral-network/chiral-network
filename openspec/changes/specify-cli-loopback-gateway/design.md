## Scope and authority

`docs/chiral-book.md` is the authority and stays read-only. Its SHA-256 was `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933` when this change was written. The sections that govern it:

- **Part II, *Peer-to-peer provider transport (target design)*, lines 280–323.**
  - *SDK and tool compatibility*, line 315: "The native desktop and CLI therefore run a **local loopback gateway** (`http://127.0.0.1:<port>/v1`, the desktop's local API server) that forwards requests for a funded network account to its provider over libp2p while holding the provider credential natively. SDKs authenticate to the gateway with a local key; the provider credential never leaves native storage. HTTPS providers remain directly usable as today."
  - *Transport and identity*, line 287: "the local loopback gateway never passes a provider's 3xx status or `Location` header to SDK clients."
  - *Connectivity ladder*, line 297, and *Performance and disclosure*, line 311: consumer interfaces show the active path before funding and during use.
- **Part II, *Desktop local-client workflow → Local API server*, lines 719–726.** The endpoint list, loopback only on a user-chosen port, the protected local bearer, `Origin` and `Host` refusal, routing to a funded network account through "one account credential the user created for that route: `infer` scope (plus `read` for recovery), an explicit spend cap and an expiry — never owner or wallet authority", idempotency pass-through, no automatic paid retries, request-status recovery, and "never advertised and is unrelated to a provider's public listener". Line 315's parenthetical names this server as the gateway, so these bullets describe the CLI gateway too.
- **Part I, *Architecture and responsibilities*, line 65:** "The native desktop and CLI share Rust consumer logic."
- **Part II, *Configuration and state*, lines 447–474:** an explicit profile (`chiral --config consumer.json`, line 449); consumer accounts, credentials and request keys are network-scoped, and one signing wallet uses one `data_dir` (line 472); "Do not place private keys or bearer tokens on command lines, in prompts, source control, diagnostics, or screenshots" (line 474).
- **Part II, *Settlement and recovery risks*, line 520:** do not replay paid inference; "inspect `/v1/requests/:id` or the CLI `request`/`requests` commands"; SDK retries disabled.
- **Part II, *API compatibility and model qualification*, line 603:** for a libp2p-only provider, SDKs use the local loopback gateway; disable automatic paid retries and retain request keys.
- **Part II, *Native provider provisioning and operations*:** "An occupied private port is an error, not permission to adopt another process" (line 627); the admin helper "never places the bearer in argv" (line 658); logs carry "request IDs, statuses, token counts, costs, and timings, not content or credentials" (line 671).
- **Part II, *Desktop local-client workflow → Discover and download models*, line 702:** a model downloaded in the desktop is "usable unchanged by the CLI, the daemon and qualification". This is the source of open question 1.
- **Part II, *Share online* step 4, line 735:** a free unqualified share is labeled *Unqualified — no speed guarantee* "wherever consumers see it".
- **Part II, *Release gates and source map*, item 7, line 775:** "local API server isolation" is qualified before the target design is presented as available.
- **Appendix A:** A.1 rules 1 (the reveal exception is a GUI rule), 4 (exact money), 6 (risk disclosure before funding), 7 (recovery over retry) and 8 (session scoping), lines 808–815; A.5.3 credential lifetimes of 60 seconds to 30 days and the issuance options, line 930.
- **`CLAUDE.md`:** "CLI stdout is machine-readable JSON or explicit streaming events. Diagnostics belong on stderr."

Where the code differs, the requirement follows the book and an unchecked task records the change.

## Relationship to milestones 2 and 3

`specify-desktop-local-api-server` (milestone 2) specifies the gateway's behavior for the desktop. Its design non-goal *CLI gateway* names this change, and its open question 1 says the follow-up "reuses this capability's gate, bounds, routing, route-credential and request-identity rules by requirement name, and adds the CLI's own lifecycle and bearer location". `specify-p2p-provider-transport` (milestone 3) specifies the transport and names the same follow-up for the CLI (its design coverage row "SDKs cannot dial libp2p; desktop and CLI run a loopback gateway").

This change cites milestone 2's requirements by name instead of copying them, so the two front ends cannot drift. Each citing requirement states only the CLI substitutions, and every SHALL can be tested against the CLI binary by running milestone 2's tests with those substitutions.

| `desktop-local-api-server` requirement | Used here | CLI difference |
|---|---|---|
| *The local API server is optional, off by default and bound to loopback only* | Bind, peer-address and no-fallback rules | A foreground command with a required `--port`; no persisted enablement; the profile's DHT listen port replaces the desktop's managed listeners |
| *The local API server runs only while the desktop process runs* | Not cited | Replaced by the signal shutdown requirement, which keeps its cancel-and-disclose behavior |
| *Local API requests are authenticated by a protected, locally generated bearer* | All rules except Reveal and the webview | The file lives in the CLI gateway directory; no command prints it; regeneration waits for the gateway to stop |
| *Browser-origin and unexpected-host requests are refused* | Unchanged | — |
| *The local API server exposes only a bounded OpenAI-compatible endpoint subset* | Unchanged | Every route is `owned_by` `network` |
| *Local API requests have fixed size, concurrency and duration bounds* | Unchanged | — |
| *Each route name maps to exactly one local model or one funded network account* | Network targets, refusal rules and the spending disclosure | No local targets; command-line management with `--accept-spending-risk`; random route IDs |
| *Local routes are free, private and never touch the network* | Not cited | No local routes (open question 1) |
| *Network routes forward through the native consumer path with the credential held natively* | Unchanged | The profile is loaded for the whole process; the `no_credential` message names `gateway route renew` |
| *Paid routes keep request identity and never retry automatically* | Unchanged | Records are created exclusively across processes |
| *Paid requests remain recoverable through request status* | Unchanged | `chiral requests` and `chiral request --credential` replace Usage & API keys |
| *libp2p forwarding follows the peer-to-peer transport contract* | Unchanged | Path reported in stdout events |
| *Network routes follow the native session and network scope* | Not cited | One profile per process; the state directory is network- and wallet-scoped |
| *The local API server is never advertised and is unrelated to provider listeners* | Unchanged | Separate from `chiral_daemon` listeners |
| *The local API request log records metadata only* | Field list and exclusions | The `request` event on stdout is the log; no in-memory log |
| *The Local server page controls the server without receiving secrets* | Not cited | `gateway show`, route commands and stdout events |

From milestone 3 this change cites *The active path is disclosed before funding and during use* (the "desktop and CLI" path rule) and, through milestone 2's libp2p requirement, the peer-ID, transport-selection, ladder, circuit-budget and recovery requirements.

### Coverage of the book's statements

| Book statement | Where it is specified |
|---|---|
| Line 315: the CLI runs a local loopback gateway at `http://127.0.0.1:<port>/v1` | *The CLI gateway is a foreground command bound to loopback only* |
| Line 315: forwards requests for a funded network account to its provider over libp2p | *CLI gateway routes map names to funded network accounts only*; *CLI gateway requests follow the local API server's forwarding, identity and recovery rules* (milestone 2's libp2p requirement) |
| Line 315: the provider credential is held natively; SDKs use a local key; the credential never leaves native storage | *The CLI gateway authenticates requests with a protected bearer that no command prints*; *Each CLI route spends through a dedicated, capped and expiring credential*; forwarding requirement (milestone 2's credential rules) |
| Line 315: HTTPS providers remain directly usable as today | Decision 2: HTTPS accounts are also forwarded, and an exported credential keeps working directly |
| Line 287: the gateway never passes a provider's 3xx status or `Location` header | Forwarding requirement (milestone 2's 3xx rule), scenario *A provider redirect is not passed to the SDK* |
| Line 719: the endpoint list | *The CLI gateway applies the local API server's request gate, endpoint subset and bounds* |
| Line 723: loopback only, off by default, on a user-chosen port; no LAN | Foreground requirement (no default port, no socket unless the command runs) |
| Line 724: a locally generated bearer in a protected file; `Origin` and `Host` refused | Bearer requirement; gate requirement |
| Line 725: one dedicated `infer`+`read` credential with an explicit cap and expiry per route; never owner or wallet authority; only enabled routes reachable; the route's cap shown; `Idempotency-Key` passed through; provider request ID returned; no automatic paid retries; request-status recovery | Route-table and route-credential requirements; `gateway show` in the events requirement; forwarding requirement |
| Line 726: never advertised; unrelated to a provider's public listener | *The CLI gateway performs no account operation over HTTP and is never advertised* |
| Line 65: desktop and CLI share Rust consumer logic | Decision 3; task 2.1 |
| Lines 449 and 472: explicit profile; network-scoped consumer state; one wallet, one `data_dir` | Foreground requirement; *CLI gateway state is protected and scoped to the profile's network and consumer wallet*; *The CLI gateway coexists with the desktop and other processes on one data directory* |
| Line 474: no bearer tokens on command lines or in diagnostics; protected files | Bearer requirement; events requirement |
| Line 520: no replay; stable `Idempotency-Key`; `/v1/requests/:id` or CLI `request`/`requests` | Forwarding requirement (recovery substitution); stop requirement (no resend after restart) |
| Line 603: SDKs reach a libp2p-only provider through the gateway; retries disabled; keys retained | Path requirement (no invented base URL); task 2.12 (README snippets) |
| Line 627: an occupied port is an error | Foreground requirement |
| Line 658: never place the bearer in argv | Bearer requirement; task 2.12 |
| Line 671: logs carry request IDs, statuses, token counts and timings, not content or credentials | Events requirement |
| Lines 297 and 311: show the active path before funding and during use | *The CLI gateway reports each request's transport path without adding disclosures* (during use); funding stays with `fund` and `topup` (decision 13) |
| Line 735: *Unqualified — no speed guarantee* wherever consumers see it | Events requirement (`speed_label` in route listings) |
| A.1.7: recovery over retry | Stop requirement; forwarding requirement |
| A.1.8: session scoping | State requirement; decision 16 |
| Line 775, release gate 7: local API server isolation | Tasks 3.5 and 3.13 qualify the CLI gateway's isolation separately from the desktop's |

## Decisions

1. **Network routes only.** Line 315 describes the CLI gateway as forwarding "requests for a funded network account". Local models need installed-model identity, a managed engine per loaded model, and load and eject, which milestone 1 (`specify-desktop-local-models`) defines only for the desktop. The CLI has none of these, and adding a CLI runtime manager would be a new product surface. The gateway therefore rejects local targets and never starts an engine. Line 702 makes desktop-downloaded artifacts "usable unchanged by the CLI", which could be read as asking for CLI local serving, so open question 1 asks the author.

2. **HTTPS accounts are forwarded too.** Line 315 calls the CLI gateway "the desktop's local API server", and that server forwards network accounts "over HTTPS or libp2p" (line 725). Milestone 2 takes the transport from the native consumer's selection (its decision 14), and milestone 3 prefers HTTPS when an offer declares both (its decision 8). A libp2p-only gateway would therefore refuse an account just because its provider also has an HTTPS origin. "HTTPS providers remain directly usable as today" keeps exported credentials working; it does not forbid forwarding. Before milestone 3 is implemented, HTTPS accounts are the gateway's only working routes, which also lets the CLI gateway be built and tested first.

3. **Milestone 2's rules are cited by name, with the same codes and bounds.** One routing core serves both front ends (book line 65; milestone 2's non-goal says "The routing core is specified so that the CLI front end can reuse it"). Because the requirements are cited rather than copied, a later change to milestone 2 changes both front ends. SDK clients see the same status codes, error codes and bounds from either one. The core must build without desktop libraries, because `chiral` is a headless binary (`CLAUDE.md`; task 2.1). This change is archived after milestone 2.

4. **A foreground command with a required port.** The book wants the server "off by default, on a user-chosen port" (line 723). For a CLI, the running command is the enablement, so nothing is persisted, and `--port` has no default, because a default would be a port the user did not choose. The 1,024–65,535 range and the occupied-port error follow milestone 2's decision 1 and book line 627. The profile's `dht.listen` port takes the place of the desktop's managed listeners. No service or start-at-login entry is installed, matching milestone 2. A user may run the command under their own service manager.

5. **Signals end the gateway the way Quit ends the desktop server.** SIGINT and SIGTERM (Ctrl-C on Windows) are the signals `chiral_daemon.rs::shutdown_signal` and `relay_server.rs` already handle. Milestone 2 cancels in-flight requests on disable and Quit, after stating that cancelled requests can still be charged. A CLI has no confirmation dialog, so the statement goes to stderr when shutdown cancels a dispatched request, and every cancelled request gets its own `request` event with its request key, so recovery identifiers are on stdout. Waiting for paid streams to finish was considered and rejected: a stream can last 3,600 seconds, and the desktop does not wait either. The 10-second exit bound is new; cancellation is local work (dropping connections and ending client responses), and request keys were persisted before dispatch. A second signal exits at once, the usual convention. A completed shutdown exits with status 0.

6. **State lives in the consumer's network- and wallet-scoped directory, separate from the desktop's.** The book scopes consumer accounts, credentials and request keys by network (line 472), and the consumer already keeps each wallet's accounts and `requests/` under `<data_dir>/inference/<network namespace>/consumer/<wallet address>/`. Placing `gateway/` there gives milestone 2's session scoping (routes resolve only under the same identity and wallet) by construction: another profile resolves another directory. The desktop keeps its server state in `<app data>/local-server/`, because its local routes need no profile. The CLI has no app-data directory and always has a profile. Sharing one route table would make each front end depend on the other's lifecycle and secrets, so the two are kept apart (decision 11).

7. **The bearer is never printed; programs read the file.** Book line 474 forbids bearer tokens on command lines and in diagnostics. A long-running gateway's stdout and stderr are often captured by a terminal multiplexer or service log. A.1.1's reveal exists because a webview cannot read a protected file; a CLI user can. The ready event and `gateway show` give the path; SDK snippets read it in-process or through a shell builtin such as `$(< file)`, as milestone 2's decision 19 requires. `gateway regenerate-bearer` refuses while the profile's gateway is serving. The serving process reads the bearer once, and there is no channel to tell it about a new one. Rotation breaks every client anyway, so requiring a restart costs little and keeps one fixed bearer per process. Because the command takes the serve lock to make this check, a `gateway serve` started at that same moment fails with the already-serving error and can simply be rerun.

8. **Routes are managed by commands, and each issues its own credential.** `route add` and `route renew` call the native `credential_issue`, owner-authenticated by the profile's protected wallet key, as milestone 2's route editor does. Scopes are `infer` and `read`; the cap must be positive; the lifetime uses A.5.3's 60 seconds to 30 days; maximum concurrency defaults to 1, A.5.3's issuance option and the CLI's existing default. The CLI never binds a credential ID the user supplies. Every route credential is therefore new and bound to exactly one route, with no registry shared between the desktop's and the CLI's tables. `--accept-spending-risk` follows the existing `cancel-funding --accept-gas-and-race-risk` pattern: without it, the command prints the disclosure and does nothing. Route IDs are random 128-bit values, not a counter, so deleting the table can never cause an old request record to resolve to a new route. `route renew` is the CLI's counterpart of replacing a route's credential in milestone 2's route editor. Without it, an expired credential would force a new route ID, and gateway-side recovery of earlier requests would be lost.

9. **Route changes apply to a running gateway.** Milestone 2's route edits take effect while the server runs, and disabling or removing a route is how a user stops spending through it without stopping other routes. The serving gateway rereads the table when it has changed, before resolving each request. Commands write the whole file atomically under a route-table lock held only for the change, and recheck their preconditions under it. Issuance is a provider call made before that lock is taken, so a command that has issued a credential and then cannot commit names that credential and the revoke command instead of revoking it silently or leaving it unreported. A request in progress keeps its route. A table that becomes unreadable, insecure or malformed while serving refuses requests with 503 `route_unavailable` instead of serving from a stale copy; the gateway keeps running and leaves the file alone, which is the one exception to the state requirement's fail-closed exit.

10. **stdout is a JSON-lines event stream, and it is the request log.** `chiral infer --stream` already writes compact `{"kind","data"}` lines; the gateway uses the same shape. The kinds are fixed (`ready`, `request`, `stopping`, `stopped`) so that supervisors and tests can parse them. A `request` event carries milestone 2's log-entry fields, plus `route_id` and a `delivery` member that makes milestone 2's distinction between not sent, refused by the provider, completed and possibly charged machine-readable. `delivery` is `null` for the model list and status reads. For a cancel it is `possibly_charged`, with a stderr statement that cancellation is not a refund: that pair is how this log meets milestone 2's rule that the request log describes cancellation as possibly charged and not a refund. Milestone 2's 1,000-entry in-memory log exists to feed the Local server page. The CLI has no page, so the event stream is the log, and keeping or discarding it is the user's choice. Requests refused at the host, origin or bearer check produce no event, so unauthenticated local traffic cannot flood stdout. Everything else (disclosures, warnings, errors) goes to stderr, as `fund` and `cancel-funding` already do.

11. **Coexistence: no long-held consumer lock, and one gateway per profile.** `Consumer::process_lock` takes `consumer.lock` with a non-blocking `try_acquire` and fails with "consumer state is busy in another process". Holding it while serving would make every `fund`, `resume`, `topup` and credential operation, and the desktop's, fail for the gateway's whole life. Inference, `request_status` and `cancel_request` take no lock today, and the gateway keeps it that way. One gateway per gateway directory, enforced by a separate serve lock, mirrors milestone 2's single listener per route table and bearer. It keeps shutdown accounting complete, and it needs no shared in-memory state. Gateways for other networks or wallets, and the desktop's server, use other state and may run at the same time on other ports. Request-key records must be created exclusively, because with two front ends one client key can now reach one account from two processes at once. Milestone 2's "never overwritten" rule needs atomic creation across processes, not only within one.

12. **Recovery uses the existing CLI commands.** Book line 520 names "the CLI `request`/`requests` commands". Route credentials are never active, and `chiral request` today uses only the active credential (`consumer.rs::request_control` calls `credential(&saved, None)`). It therefore gains `--credential`, which uses milestone 2's explicit-credential consumer extension with no fallback. `chiral requests` shows the route ID and provider request ID that milestone 2 adds to each record.

13. **Path reporting without new disclosures.** Milestone 3 requires native surfaces "in the desktop and CLI" to show each request's path during use, and its task 2.11 adds "path fields in CLI output". For the gateway, that surface is the `request` event. Route listings show what the signed offer declares (transports, peer ID, endpoint only when declared), never an observed path. The gateway funds nothing, so milestone 3's pre-funding path, relay-visibility and mutual-IP disclosure stays with `chiral fund` and `chiral topup`. The route disclosure is milestone 2's, unchanged. Neither milestone 2 nor the book asks for a relay disclosure when a route is created, so none is invented here.

14. **The unqualified label appears in route listings.** The book labels a free unqualified generation share *Unqualified — no speed guarantee* "wherever consumers see it" (line 735), and milestone 2's decision 21 treats the routing table as such a view. `gateway show` and the ready event carry `speed_label`, using the predicate of milestone 2 and `specify-desktop-share-online` decision 30: no serving profile plus `chat` or `completions`, whatever the prices. It is display only.

15. **The DHT node starts before `ready`.** After milestone 3, libp2p routes need the profile's configured `dht` node (milestone 3, *Consumer libp2p use is configured explicitly*). The gateway starts it before writing `ready`. If it cannot start, the command fails rather than serving with libp2p silently unavailable, in the same spirit as the occupied-port rule (the *libp2p node* substitution in the forwarding requirement). A profile without `dht` serves HTTPS routes and refuses libp2p-only accounts with 503 `transport_unavailable`, as milestone 2 specifies.

16. **Session scoping is structural.** A gateway process has one profile for its whole life, so there is no logout or profile change. Milestone 2's session requirement reduces to the recorded identity and wallet check, which the forwarding requirement keeps as a defense against a copied route table, and to the network- and wallet-scoped directory.

## Code differences

Compared with `d064cbda`:

1. `src-tauri/src/bin/chiral.rs` has no `gateway` subcommand, no HTTP listener and no long-running consumer command (tasks 2.4–2.7).
2. `chiral.rs` handles only Ctrl-C, and only inside `infer --stream`. SIGTERM is handled by `chiral_daemon.rs::shutdown_signal` and `relay_server.rs`, not by `chiral` (task 2.5).
3. `consumer.rs::request_control` always uses the active credential (`credential(&saved, None)`), so `chiral request` cannot use a route credential (task 2.9; shares milestone 2's task 2.6 extension).
4. `consumer.rs::local_requests` lists `{contract_id, request_key, created_at}` records stored as `requests/<request_key>.json`; there is no route ID or provider request ID. `inference_response` writes the record with `private_write`, not by exclusive creation (task 2.9; milestone 2 task 2.6 for the fields).
5. `consumer.rs::credential_issue` activates a newly issued `infer` credential when no valid active credential exists. Milestone 2's non-activating issuance is needed (task 2.6).
6. Milestone 2's local-server module does not exist yet. Its proposal places it in `src-tauri/src/`; it must also be outside the `desktop` feature so that `chiral` can use it (task 2.1).
7. `Consumer::process_lock` is taken only by mutating operations (fund, resume, top-up, credential issue, revoke and rotate, and others). Inference, `request_status` and `cancel_request` take none. This must stay true (task 2.11).
8. `Consumer::account_directory` derives the wallet address from the protected wallet key file. The gateway reads the key to locate its directory, as every CLI command does, but signs nothing while serving (requirement *The CLI gateway performs no account operation over HTTP and is never advertised*).

## Non-goals and limitations

- **Local models in the CLI** (open question 1).
- **Background operation.** No daemon mode, service installation or start-at-login entry. A user who runs the command under a service manager is responsible for that service's logs. The bearer never appears in them.
- **LAN or remote exposure**, as in milestone 2.
- **A bearer reveal command** (decision 7).
- **Funding, top-up, resume, cancel-funding or credential export over HTTP.** These stay with the existing `chiral` commands and their disclosures. The A.1.6 reorganization disclosure for CLI funding is already asked in `docs/apas-worker/report.md`, entry *Funding settlement: three questions*, item 3. The gateway funds nothing, so either answer leaves this change unchanged.
- **The OS account is the trust boundary**, as in milestone 2. Any process of the same OS user that can read the bearer file can spend up to each route's cap until its expiry.
- **Changes to the desktop local server.** None.
- **Shared DHT identity.** Two processes on one profile run their DHT nodes with the same consumer peer ID, and a fixed `dht.listen` port cannot be bound twice. This is true today for any two processes on one profile, for example `chiral discover` while the desktop runs that profile's node. Milestone 3 does not specify it either; it is an M3 implementation concern, not an author question. Until a change does, a gateway whose DHT node cannot start, for example because another process holds the profile's fixed `dht.listen` port, fails before `ready` (decision 15), and the coexistence requirement promises only that the consumer process lock never blocks other processes. The gateway refuses its own port when it equals the DHT listen port.
- **Implementation and verification are not claimed.** This change specifies requirements only.

## Open questions

New question for the whitepaper author (also recorded in `docs/apas-worker/report.md`, entry *Should the CLI loopback gateway also serve local models?*):

1. **Local-model routes in the CLI gateway** (non-blocking). Line 315 describes the CLI gateway as forwarding funded network accounts. Line 702 says a desktop-downloaded model is "usable unchanged by the CLI". Should `chiral gateway serve` also route names to local models? That needs a CLI load and eject surface, which milestone 1 defines only for the desktop. **Default adopted:** network routes only (decision 1). A yes would be a later change adding a CLI runtime surface and local routes under milestone 2's *Local routes are free, private and never touch the network*.

Questions already asked elsewhere, not asked again:

- Whether CLI funding should carry the A.1.6 reorganization disclosure: `docs/apas-worker/report.md`, *Funding settlement: three questions*, item 3. It does not affect this change (non-goals).
- Whether the gateway split between milestone 2 and this change is right: milestone 2's open question 1. This change is the concrete form of that split.

**Implementation decisions** the author may revisit without changing the design: no default port (decision 4); the 10-second shutdown bound (decision 5); no reveal command, and regeneration only while stopped (decision 7); one gateway per profile, with state separate from the desktop's (decisions 6 and 11); the event kinds and the `delivery` member (decision 10).

**Blocking dependencies** (not author decisions):

- Tasks 2.1–2.11 wait for milestone 2's routing core and consumer extensions (its tasks 2.1–2.4, 2.6, 2.7 and 2.9).
- The libp2p parts of tasks 2.10 and 3.9 wait for `specify-p2p-provider-transport` to be implemented.
- Archive after `specify-desktop-local-api-server`, whose requirement names this change cites.

## Verification plan

- **Specification:** `openspec validate specify-cli-loopback-gateway --strict --no-interactive`.
- **Lifecycle:** listener inspection (`lsof`, `ss`, `Get-NetTCPConnection`) shows only `127.0.0.1:<port>`; a LAN connection fails; occupied, out-of-range, missing and DHT-listen ports fail with nothing on stdout; a profile without a wallet fails.
- **Shutdown:** SIGINT and SIGTERM (Ctrl-C on Windows) with zero, one and several requests in progress; event order; the stderr statement; error events without `[DONE]`; one provider request each and upstream cancellation; exit 0 within 10 seconds; a second signal; no resend after restart.
- **State and bearer:** owner-only creation; fail-closed insecure, symbolic-link, corrupt and oversized files left unchanged; another network or wallet sees nothing; regeneration refused while serving and effective after restart; canary scans of stdout, stderr, process tables while serving and while each README snippet runs, and upstream requests.
- **Gate, subset and bounds:** milestone 2's `Host`, `Origin`, method and path matrices and its bounds tests (its tasks 3.1 and 3.3) run against the CLI gateway; `owned_by` is `network`.
- **Routes and credentials:** unique names; refused targets; the `--accept-spending-risk` refusal with the exact CFX cap; cap and expiry bounds; non-activation; no silent revoke; an issued credential whose route cannot be committed is named on stderr and not revoked; renewal keeps the route ID; live add, disable and remove while serving; concurrent route commands; a malformed or insecure table while serving gives 503 `route_unavailable` and the gateway keeps running.
- **Forwarding and recovery:** milestone 2's paid-route fault injection and request-route tests (its tasks 3.4 and 3.5) against the CLI gateway; requests recorded by the desktop and by `chiral infer` give 404; `chiral requests` and `chiral request --credential` recover after the gateway stops, with no fallback to the active credential.
- **Path:** HTTPS events now; after milestone 3, Direct, Hole-punched and Relayed events with relay peer ID and limits, a path change during use, a not-sent request without a path, libp2p-only listings without an endpoint, and a DHT node that cannot start failing the command before `ready`.
- **Events:** each stdout line parses as `{"kind","data"}`; `ready` first and flushed; the exact kinds; field presence and `null` rules; `delivery` `null` for model and status reads and `possibly_charged` for a cancel with the not-a-refund statement on stderr; no events for unauthenticated requests; canary content absent; `speed_label` cases.
- **Coexistence:** `fund`, `resume`, `topup` and credential commands, and desktop operations, succeed while serving; a second gateway on one profile is refused; two networks serve side by side; the desktop server and the CLI gateway refuse each other's bearers; concurrent same-key requests from two processes keep one record.
- **Non-advertisement:** DHT, manifest, identify and local-network captures contain no gateway address or port; account-control paths give 404 with no provider contact, signature or chain transaction.
- **Interoperability:** stock OpenAI Python and Node SDKs, with retries disabled, against an HTTPS route now and a libp2p route after milestone 3: streaming and non-streaming chat, completions, embeddings, request status and cancel.
- **Project checks:** `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets` and `cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bins`; the desktop checks stay green.
