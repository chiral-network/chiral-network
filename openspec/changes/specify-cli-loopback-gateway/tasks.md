## 1. Specify the CLI loopback gateway

- [x] 1.1 Read the book (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`) at the lines listed in design.md *Scope and authority*: 65, 280–323 (especially 287 and 315), 447–474, 520, 603, 627, 658, 671, 702, 719–726, 735, 775, Appendix A.1, A.5.3 and A.7. Read `specify-desktop-local-api-server` and `specify-p2p-provider-transport` in full, the `CLAUDE.md` rules, `bin/chiral.rs`, the lock, credential, request-record and request-status code in `consumer.rs`, the shutdown handling in `bin/chiral_daemon.rs`, and `openspec/specs/`. No archived capability covers the CLI, credentials or request status. Confirmed that no CLI gateway exists.
- [x] 1.2 Specify the `cli-loopback-gateway` capability: 12 requirements, citing `desktop-local-api-server` and `p2p-provider-transport` requirements by name and stating only the CLI substitutions. Record the coverage table, decisions, code differences, dependencies and open question 1 in design.md, and add one entry to `docs/apas-worker/report.md`. Validate with `openspec validate specify-cli-loopback-gateway --strict --no-interactive`.
- [x] 1.3 Point `specify-desktop-local-api-server`'s proposal non-goal and design non-goal and open question 1 at this change, keeping their meaning. Revalidate that change strictly.

## 2. Implement (after milestone 2's routing core)

- [ ] 2.1 Shared core (blocked on `specify-desktop-local-api-server` tasks 2.1–2.4, 2.6, 2.7 and 2.9). Place milestone 2's listener with connection bounds, Host/Origin/bearer gate, six-route subset, request bounds, route resolution, network forwarding through the native consumer path, request routes and log-entry construction in a library module outside the `desktop` feature. It must build with `cargo build --no-default-features --bins`. The front end supplies the bearer source, the route-table source, the event sink and the shutdown signal. Keep one implementation; do not fork it for the CLI.
- [ ] 2.2 Gateway directory and files:
  - derive `gateway/` inside the consumer's network- and wallet-scoped account directory, and refuse a profile without a wallet;
  - create the directory and files owner-only;
  - fail closed, without repairing, replacing or deleting, on insecure, symbolic-link, unreadable or malformed files;
  - a strict route-table schema: at most 64 routes, random 128-bit route IDs, unique names, unique credential IDs, and the recorded `NetworkIdentity` and consumer wallet;
  - whole-file atomic writes under a route-table lock held only for the change, with preconditions rechecked under the lock before commit.
- [ ] 2.3 Bearer:
  - generation by the first `gateway serve` from the OS CSPRNG, at least 256 bits, URL-safe ASCII;
  - `gateway regenerate-bearer` with an atomic replace, refused while the serve lock is held, printing only the path;
  - the value kept out of stdout, stderr, argv, diagnostics and every command result.
- [ ] 2.4 `gateway serve` in `bin/chiral.rs`:
  - a required `--port` of 1,024–65,535, refused when it equals a `dht.listen` port;
  - the serve lock, separate from `consumer.lock`, held for the process's life;
  - a single `127.0.0.1` bind with no fallback, and the peer-address check;
  - the JSON-lines event writer (`{"kind","data"}`, flushed per line) with exactly `ready`, `request`, `stopping` and `stopped`;
  - diagnostics on stderr, and nothing on stdout for a failure before `ready`;
  - rereading the route table when it changes before resolving each request, and 503 `route_unavailable` while it is unreadable, insecure, a symbolic link or malformed, without exiting or touching the file;
  - `request` events with `delivery` `null` for model and status reads and `possibly_charged` for cancels, plus the stderr statement that a cancellation can still be charged and is not a refund.
- [ ] 2.5 Shutdown: SIGINT and SIGTERM on Unix and Ctrl-C on Windows, reusing the `chiral_daemon` signal handling; close the listener; write `stopping` with the count; cancel dispatched requests as a client disconnect; give 503 `gateway_stopping` to responses not yet started and an error event without `[DONE]` to streams; the stderr can-still-be-charged statement; one `request` event per cancelled request; `stopped`; exit 0 within 10 seconds; immediate nonzero exit on a second signal.
- [ ] 2.6 Route commands `gateway route add|renew|enable|disable|remove`:
  - issuance through `credential_issue`, using milestone 2's non-activating extension, with scopes `infer` and `read`, a positive canonical wei `--spend-cap-wei`, `--expires-in-secs` of 60–2,592,000, and `--max-concurrency` defaulting to 1;
  - no user-supplied credential IDs;
  - milestone 2's refusal list;
  - the four-statement disclosure with the exact CFX cap on stderr on every run, and `--accept-spending-risk` required before any change or provider call;
  - `--revoke-credential` and `--revoke-old-credential` through `credential_revoke`, and otherwise output naming the still-valid credential and the revoke command;
  - when a credential was issued but the change cannot be committed after the recheck, a nonzero exit with nothing on stdout, no revocation, and stderr naming the credential and the revoke command;
  - one JSON document of metadata on stdout.
- [ ] 2.7 `gateway show`, built from local state only: `gateway_dir`, `bearer_file`, `bearer_present`, and routes with credential metadata, canonical wei cap, transports, peer ID, endpoint only when declared, and `speed_label` (milestone 2 and `specify-desktop-share-online` decision 30 predicate).
- [ ] 2.8 Network forwarding through the shared core:
  - 503 `route_unavailable` for a route whose recorded identity or wallet differs from the profile's;
  - a `no_credential` message naming `gateway route renew`;
  - request routes that resolve only records whose route ID is a current enabled route of this table for the same account;
  - every other milestone 2 forwarding, identity, header, error, redirect and no-retry rule unchanged.
- [ ] 2.9 Recovery commands: `chiral requests` shows the route ID and provider request ID from milestone 2's extended records; `chiral request <contract> <id> [--cancel] [--credential <id>]` uses exactly the named credential with no fallback; cancel writes to stderr that it can still be charged and is not a refund. Request-key records are created exclusively across processes, adding only a missing provider request ID afterwards.
- [ ] 2.10 libp2p (blocked on `specify-p2p-provider-transport`):
  - start the profile's `dht` node before `ready`, and fail without `ready`, with nothing on stdout and no listener left open, if it cannot start;
  - `transport`, `path` and `relay` members in `request` events from the native path observation, with `null` path members for not-sent requests;
  - transports and peer ID in listings, and no invented base URL.

  Before that change is implemented, every path is `HTTPS` and libp2p-only accounts give 503 `transport_unavailable`.
- [ ] 2.11 Coexistence: `gateway serve` never acquires `consumer.lock`; inference, status and cancel take no consumer process lock; route commands take it only inside `credential_issue` and `credential_revoke`; the gateway's state, bearer and credentials stay separate from the desktop's `<app data>/local-server/`.
- [ ] 2.12 Documentation: add a CLI gateway section to `README.md` covering:
  - `gateway serve`, `show`, the route commands and `regenerate-bearer`;
  - Python and JavaScript SDK snippets that read the bearer file in-process or from an environment variable set by a shell builtin, with retries disabled;
  - a `curl -K -` snippet fed by a shell builtin;
  - recovery with `chiral requests` and `chiral request --credential`;
  - the statement that cancellation and shutdown can still be charged and are not refunds.

## 3. Verify (one task per requirement, then integration)

- [ ] 3.1 *The CLI gateway is a foreground command bound to loopback only*: listener inspection on macOS (`lsof`), Linux (`ss`) and Windows (`Get-NetTCPConnection`) shows only `127.0.0.1:<port>`; a LAN attempt fails. Occupied, missing, out-of-range and DHT-listen ports, and a profile without a wallet, each exit nonzero with nothing on stdout and no listener. Nothing is daemonized or installed.
- [ ] 3.2 *The CLI gateway stops on interrupt or termination and cancels its in-flight requests*: SIGINT, SIGTERM and Windows Ctrl-C with zero, one and three requests in progress against a controlled provider. Check the event order and counts, the stderr statement, the 503 `gateway_stopping` and the error event without `[DONE]`, exactly one provider request per client request with upstream cancellation, exit 0 within 10 seconds, the second-signal exit, a released port, and no resend after restart.
- [ ] 3.3 *CLI gateway state is protected and scoped to the profile's network and consumer wallet*: owner-only creation; group- or world-readable, symbolic-link, corrupt, oversized, duplicate-ID, duplicate-name and shared-credential tables and bearer files fail closed unchanged; profiles for another network and for another wallet over one `data_dir` see none of the routes or the bearer.
- [ ] 3.4 *The CLI gateway authenticates requests with a protected bearer that no command prints*: entropy and format; missing, wrong, query-string, cookie and wrong-scheme bearers get 401 with no provider contact; regeneration is refused while serving and the old bearer is rejected after restart. Canary scans cover every command's stdout and stderr, process tables sampled while serving and while each README snippet runs, and all upstream requests.
- [ ] 3.5 *The CLI gateway applies the local API server's request gate, endpoint subset and bounds*: run milestone 2's task 3.1 Host, Origin, method and path matrices and its task 3.3 bounds tests against the CLI gateway, with controlled time. `GET /v1/models` lists only enabled routes with `owned_by` `network` and makes no outbound connection.
- [ ] 3.6 *CLI gateway routes map names to funded network accounts only*: unique names; refusal of URL, local-model, secret and unknown-contract targets with no engine started; live add, disable and remove while serving, with in-flight requests keeping their route; concurrent route commands keep both changes; a malformed, insecure or symbolic-link table while serving gives 503 `route_unavailable` until repaired, while the gateway keeps running and the file is left unchanged.
- [ ] 3.7 *Each CLI route spends through a dedicated, capped and expiring credential*: issued scopes, cap, expiry and concurrency against a controlled provider; the active credential is unchanged; the missing-flag refusal prints the exact CFX cap and makes no provider call; a zero, missing or non-canonical cap and an out-of-range lifetime are refused before any provider call; milestone 2's refusal list; no silent revoke; both revoke flags revoke exactly one credential; a credential issued for a name that another process commits first is reported on stderr, not revoked and not bound, with nothing on stdout; renewal keeps the route ID and earlier records still resolve.
- [ ] 3.8 *CLI gateway requests follow the local API server's forwarding, identity and recovery rules*: run milestone 2's task 3.4 fault injection and task 3.5 request-route tests against the CLI gateway, then:
  - records made by the desktop local server, by `chiral infer` and by removed or retargeted routes give 404 with no provider contact;
  - a route recorded under another identity or wallet gives 503 `route_unavailable`;
  - the `no_credential` message names `gateway route renew`;
  - after milestone 3, a profile whose DHT node cannot start (its `dht.listen` port held by another process) fails before `ready` with nothing on stdout and no listener, and a profile without `dht` serves and gives 503 `transport_unavailable` for libp2p-only accounts;
  - after the gateway stops, `chiral requests` lists route ID and keys, and `chiral request --credential` reads status and cancels with exactly that credential, with no fallback when it is missing or expired.
- [ ] 3.9 *The CLI gateway reports each request's transport path without adding disclosures*: transport `https` and path `HTTPS` events now. After milestone 3:
  - Direct, Hole-punched and Relayed events, with relay peer ID and reported limits or `null`;
  - a Relayed-to-Hole-punched change mid-session;
  - `null` path members for not-sent requests;
  - libp2p-only listings with no endpoint or base URL.
- [ ] 3.10 *The CLI gateway writes machine-readable events to stdout and diagnostics to stderr*:
  - every stdout line parses as `{"kind","data"}` with one of the four kinds;
  - `ready` arrives before the first request through a pipe;
  - required members are present, with `null` where they do not apply;
  - `delivery` is `null` for model and status reads, and a cancel's event has `possibly_charged` with the not-a-refund statement on stderr;
  - no event is written for unauthenticated requests;
  - canary prompts and outputs are absent from stdout and stderr;
  - no locally computed cost appears;
  - `speed_label` covers a free chat offer without a serving profile, a priced one without a profile, an embedding-only one, and one with a profile;
  - each command writes one JSON document on success and nothing on stdout on failure.
- [ ] 3.11 *The CLI gateway coexists with the desktop and other processes on one data directory*:
  - `fund`, `resume`, `topup`, `credentials issue|revoke` and desktop consumer operations succeed while serving;
  - a second gateway on one profile is refused before binding, and the first keeps serving;
  - two networks serve side by side and refuse each other's bearers;
  - the desktop local server and a CLI gateway refuse each other's bearers, and a port conflict between them fails cleanly;
  - two processes sending the same key to one account keep one record and get one execution and one 409.
- [ ] 3.12 *The CLI gateway performs no account operation over HTTP and is never advertised*: DHT, manifest, identify and local-network captures contain no gateway address or port; account-control paths give 404 with no provider contact, wallet signature or chain transaction.
- [ ] 3.13 Interoperability and isolation:
  - stock OpenAI Python and Node SDKs, with retries disabled, against an HTTPS route (and a libp2p route after milestone 3): streaming and non-streaming chat, completions, embeddings, request status and cancel;
  - a real-browser test confirms that a page on another origin, and one reached through a DNS-rebinding name, cannot use the gateway.

  Record the results in design.md as the CLI gateway's evidence for release gate 7's local API server isolation.
- [ ] 3.14 Run `cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets`, `cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bins`, `npm run check`, `npm test` and `npm run build`. Do not present the CLI gateway as available until 3.1–3.13 pass.

## 4. Validate and archive

- [ ] 4.1 After `specify-desktop-local-api-server` is archived:
  - confirm that every `desktop-local-api-server` requirement name cited here still exists in `openspec/specs/`, and update the citations if any was renamed;
  - run `openspec validate specify-cli-loopback-gateway --strict --no-interactive`;
  - archive this change once sections 2 and 3 are complete.
