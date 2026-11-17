## 1. Specify the local API server

- [x] 1.1 Read the governing book sections as committed in `fe473508`, plus the uncommitted 2026-11-16 *Author decisions* edit, as listed in design.md *Scope and authority*: target design, local API server, SDK compatibility, HTTP and listener boundaries, request lifecycle, credential issuance, Appendix A.1, A.2, A.5.3, A.6 and A.7. Read `docs/provider-qualification.md`'s listener conventions. Compare them with the current native dispatch, credential selection, request records, limits and command boundary in `consumer.rs`, `billing_store.rs`, `inference_gateway.rs`, `inference_backend.rs`, `desktop.rs` and `capabilities/default.json`. Confirm that no local server exists.
- [x] 1.2 Specify the `desktop-local-api-server` capability. Record the dependencies on M1, M3 and M4 and the open questions without deciding any author decision. Validate with `openspec validate specify-desktop-local-api-server --strict --no-interactive`.
- [x] 1.3 Apply the book's 2026-11-16 *Author decisions*: record each decision in design.md *Open questions*; keep local routes to a free unqualified share on M4 admission with no speed guarantee and no assumed arrival budget; label network routes to such offers *Unqualified — no speed guarantee*; keep server requests out of chat history. Leave open questions 1 and 2 open. Revalidate.
- [x] 1.4 Check the book's 2026-11-16 *Author decisions* items 5–7 (working-copy SHA-256 `3afcdb876c5422686259b0782ae2b4b3a5f0de968e7ecd60806b91523dfe743a`) against this change: record each in design.md *Open questions*; note under Decision 14 that saved v1 accounts stay reachable over the transport the native consumer selects; keep network routes to HTTPS providers; change no requirement. Revalidate.
- [x] 1.5 Apply the book's *Author decisions* items 1–11 (`fe473508` plus the uncommitted 2026-11-16 edit, working-copy SHA-256 `8ee7fd8a114a8aefc7261a5716e2e80f10e051347f575cff645acf742e53e81c`), whose items 8–11 replace the earlier items 8 and 9. Cite that text in design.md *Scope and authority* and the proposal. Record items 8–11 in design.md *Open questions*. Add Decision 22 and, to *Network routes forward through the native consumer path with the credential held natively*, the exact mappings of M3's HTTP 402 `free_allowance_exhausted` and `free_allowance_expired` and M4's HTTP 503 `qualification_in_progress`, with the scenarios *A used-up free allowance is reported, not retried*, *An expired free allowance* and *A provider paused for qualification*. Add "paused for qualification" to the 503 `share_unavailable` admission reasons of *Local routes are free, private and never touch the network*. No requirement was renamed or removed. Revalidate.
- [x] 1.6 Apply the 2026-11-16 decisions recorded in the book (working-copy SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`) and the implementation decisions E, F and H: pass `Retry-After` (1–3600 integer seconds, otherwise dropped) only on 503 `qualification_in_progress` and add it to the response-header list; map that code with the native consumer's fixed message "Provider is qualifying — try again later"; add the scenarios *An out-of-range Retry-After is dropped* and *Retry-After is not passed on other refusals*; record E–H and the author decisions A–D in design.md *Open questions*. No requirement was renamed or removed. Revalidate.
- [x] 1.7 Align with book A.1.8 (scoped by A.7) after the 2026-11-17 APAS comparison: network routes are scoped to network identity and consumer wallet, another session's routes are hidden on the Local server page, network-route log entries are removed on logout or a profile change, and the design quotes *Use the network* exactly.

## 2. Implement

- [ ] 2.1 Add the native local-server module:
  - settings and routing-table persistence under `<app data>/local-server/`, with no secrets;
  - a single `127.0.0.1` listener on a validated port (1,024–65,535);
  - refusal when the port is occupied or used by a managed listener, with no fallback;
  - peer-address check;
  - enable and disable, with the in-flight warning, and restart at launch from the persisted state;
  - shutdown on Quit or process exit;
  - no background service or login item.
- [ ] 2.2 Implement the bearer:
  - generation from the OS CSPRNG, at least 256 bits, URL-safe ASCII;
  - protected-file storage with fail-closed permission and format checks;
  - constant-time `Authorization: Bearer` comparison and atomic regeneration;
  - a native Reveal command that returns the value only on explicit request.

  Keep the bearer out of logs, diagnostics, argv, URLs and every other webview result.
- [ ] 2.3 Implement the listener and request gate:
  - connection bounds before anything else: a 5 s header read, a 16 KiB header block (431), at most 64 open connections (excess accepted and closed), and a 30 s keep-alive idle timeout;
  - then, in order: the exact `Host` allowlist and refusal of absolute-form targets (421), any `Origin` (403), then the bearer (401), all before the body is read;
  - no CORS headers;
  - the six-route subset, with 404 and 405 for anything else, content-type checks and empty bodies on `GET /v1/models` and the request routes;
  - the provider gateway's error object;
  - `GET /v1/models` answered locally;
  - the request bounds: 256 KiB body, 5 s body read, 16 requests in progress without queuing, 8 MiB response, 120 s stream start and idle, 3,600 s total.
- [ ] 2.4 Implement routing-table management:
  - stable, never-reused route IDs;
  - unique route names of 1–256 bytes, at most 64 entries;
  - pinned local targets (installed-model identity);
  - network targets naming the `NetworkIdentity`, the account ID and exactly one dedicated credential ID;
  - issuing the route credential from the route editor through the native `credential_issue`, with scopes `infer` and `read`, a positive spend cap and an expiry within the A.5.3 bounds, without making it the account's active credential, and recording its scopes, cap and expiry;
  - refusing a route credential that lacks a scope, has a null or zero cap, is expired, revoked or missing, is the active credential, or is bound to another route;
  - offering `credential_revoke` when a route is removed or its credential replaced;
  - refusal of URL, offer and secret targets;
  - the network-route spending disclosure, stating the exact cap, before a network route is created or enabled.
- [ ] 2.5 Implement local routes (blocked on `specify-desktop-local-models`):
  - dispatch only to the engine child with the route's exact installation loaded, and return 503 `model_not_loaded` otherwise, with no load, eject or download;
  - shared request-schema validation, with that load's engine key read natively;
  - slot sharing with local chat (429 when full);
  - a local `x-request-id`;
  - no accounting and no network contact;
  - the share-admission dispatch is implemented by `specify-desktop-share-online` task 2.17; this change defines the 429/503 mapping and verifies it in 3.6: 429 `rate_limit` when saturated or, where the share has one, its arrival budget is exhausted, 503 `share_unavailable` when draining, paused for qualification or not ready, and 503 `share_unavailable` with the being-qualified message while the installation is under a qualification run, never queued and never direct to an engine. The same mapping applies to a free share published with no serving profile; no route skips admission because its share is unqualified.
- [ ] 2.6 Implement network routes through the native consumer path:
  - extend `consumer.rs` to accept a validated caller-supplied `Idempotency-Key`, otherwise generate a UUID, and persist the key before dispatch, failing closed without dispatch;
  - extend `consumer.rs` to accept an explicit credential ID with no fallback to the active credential;
  - key the request record by account and request key, with file names safe for keys containing `:`;
  - record the route ID, and add the provider request ID when headers arrive, never overwriting an existing record;
  - `x-chiral-request-key` and `x-request-id` headers;
  - the header allowlist (content type, a well-formed provider request ID, the request key, and `Retry-After` only on 503 `qualification_in_progress`), and classified errors with identifiers and no provider text;
  - the exact mappings of HTTP 402 `free_allowance_exhausted` and `free_allowance_expired` (after M3), with no `Retry-After`, and HTTP 503 `qualification_in_progress` (M4's qualification pause), with the native consumer's fixed message "Provider is qualifying — try again later" and the provider's `Retry-After` passed only when it is exactly one integer of 1–3600 seconds and dropped otherwise; each with the request key, a request-log outcome that identifies a provider refusal, the request-key record kept, no retry, and no quote, opening, funding or route change;
  - provider 3xx mapped to 502 `provider_error` with no `Location`;
  - stream termination with an error event and no `[DONE]`, and upstream cancellation on client disconnect;
  - no resend on any path;
  - `route_unavailable`, `account_not_spendable` and `no_credential` responses;
  - cancellation on `logout` and `configure`, and refusal of routes recorded for a different network identity or consumer wallet; record both on each network route; hide another session's network routes on the Local server page (count only) and remove network-route log entries on `logout` and `configure` (A.1.8).
- [ ] 2.7 Implement `GET /v1/requests/:id` and `POST /v1/requests/:id/cancel`:
  - resolve `:id` only against server-recorded network requests under the loaded identity, by provider request ID or request key;
  - 400 `invalid_request_id`, 503 `route_unavailable` with no profile, 404 `not_found` with no provider contact, and 409 `ambiguous_request_id`;
  - dispatch through `request_status` and `cancel_request` with the recorded route's credential only, or 409 `no_credential`;
  - decode the provider's request-status object, with money as canonical wei;
  - pass the cancel result through without retry, labeled as possibly charged and not a refund.
- [ ] 2.8 Implement libp2p routes:
  - Now: refuse accounts reachable only over libp2p with 503 `transport_unavailable`, including from profiles with no libp2p node.
  - After M3 (blocked on `specify-p2p-provider-transport`):
    - forward over `/chiral/inference/http/1` using the native consumer's transport selection and the peer-ID check, with no fallback;
    - return 502 `provider_authentication_failed`, logged as not sent, on a peer-ID refusal;
    - return 503 `provider_unreachable` with the request key, logged as not sent and not dispatched by another path, when the native consumer reports a request as not sent for any other reason (circuit-budget refusal, or no authenticated path within the ladder deadline);
    - show the transport and active path in the routing table and request log.
- [ ] 2.9 Implement the request log:
  - in memory, the 1,000 most recent entries;
  - metadata fields only, with labeled token counts;
  - costs only from authoritative status, in exact wei;
  - a clear action, with no deletion of persisted request keys;
  - no write of server requests to local chat history, whatever the history preference.

  Add diagnostics fields: enabled state, address and port, number of routes, requests in flight.
- [ ] 2.10 Add the Local server page (A.7, built per A.6). Add its `#/local-server` route and its Local server navigation item (label, short label, icon, `aria-current`), placed after My models and before Share. The navigation placement is blocked on `specify-desktop-local-models`. The page contains:
  - enable/disable and port;
  - the masked bearer with its file location, explicit Reveal and Hide (cleared on Hide, page exit, Regenerate, logout and profile change), and Regenerate with confirmation;
  - the routing table, showing each network route's credential ID, scopes, exact CFX spend cap and expiry, and, for an agreed offer without a serving profile, the no-speed-guarantee statement (*Unqualified — no speed guarantee* for a generation offer, whatever its prices, as in `specify-desktop-share-online` decision 30);
  - `curl`, Python and JavaScript snippets that read the bearer from the file or the environment, keep it out of argv and URLs (`curl -K -` fed by a shell builtin), and disable SDK retries;
  - the request log.

  The page uses native commands only and adds no core webview permission. Network routes are disabled until a profile is loaded. The page also covers the browser-preview behavior, verbatim errors, `role` attributes, labels and escaped text. In Usage & API keys, Requests, mark requests that came through the local server and offer status and cancel.
- [ ] 2.11 Guard against advertisement: no publication path (DHT, manifest, offer, identify, relay, catalog, mDNS, DNS-SD, SSDP, UPnP, NAT-PMP, PCP) carries the server's address or port. The share workflow cannot select the local server as a provider endpoint: `specify-desktop-share-online` accepts no HTTP origin or endpoint of any kind in its libp2p-only reachability step, the local server's address included (its *Reachability is verified through a relay before publishing* and task 2.8); keep that refusal covered when either change is implemented.

## 3. Verify

- [ ] 3.1 Gate unit tests:
  - the `Host` and `Origin` matrices from design.md *Verification plan*, including DNS-rebinding names, `null` and `tauri://localhost` origins, preflight and absolute-form targets;
  - ordering before the body is read and the bearer is evaluated;
  - no CORS headers;
  - the six-route method and path matrix.
- [ ] 3.2 Authentication and reveal tests:
  - missing, wrong, query-string and cookie bearers;
  - regeneration;
  - insecure and corrupt files fail closed without modification;
  - Reveal only on explicit request, cleared on Hide, page exit, Regenerate, logout and profile change.

  Run canary scans for the bearer and route credentials across every other command result, webview storage, the request log, diagnostics, process logs and upstream requests. Sample the process table while running each displayed snippet and confirm that no argv contains the bearer.
- [ ] 3.3 Bounds tests with controlled time:
  - slow headers cut off at 5 s without counting as a request;
  - a 16 KiB + 1 header block refused with 431;
  - a 65th connection closed immediately, and idle keep-alive closed at 30 s;
  - body size, slow body and the 17th concurrent request;
  - response size, and idle and total deadlines with upstream cancellation.
- [ ] 3.4 Route-credential and paid-route fault injection against a controlled loopback provider:
  - route-credential refusals: no cap, null cap, missing `read`, active credential, or already bound;
  - route issuance leaves the active credential unchanged;
  - an expired route credential gives 409 with zero dispatches and no fallback;
  - persistence failure leads to zero dispatches;
  - a cut stream, timeout, oversized stream or client disconnect leads to exactly one provider request and an error event without `[DONE]`;
  - a reused key passes the 409 through and keeps the original record;
  - the same key on two accounts keeps both records;
  - a provider 302 gives 502 with no `Location` and no connection to the target;
  - no rerouting;
  - classified errors carry no provider text;
  - Usage & API keys recovers the request after a restart with the server disabled;
  - a route whose agreed offer is a free generation offer with no serving profile reads *Unqualified — no speed guarantee*, and still requires its dedicated, positively capped credential; a priced generation offer with no serving profile reads the same, and an embedding-only offer with no serving profile keeps the no-generation-speed-guarantee statement.
- [ ] 3.5 Request-route tests:
  - status by provider request ID and by request key, using the route's credential;
  - 404 with zero provider contact for unknown, local-route, desktop-originated, removed-route, retargeted-route and other-identity IDs;
  - 503 with no profile;
  - 409 `ambiguous_request_id`;
  - cancel sends exactly one provider cancellation, passes the 202 through, and the log labels it possibly charged and not a refund.
- [ ] 3.6 Local-route tests (blocked on M1):
  - deny-all outbound and no profile;
  - 503 for an unloaded model, with no load;
  - a different loaded revision is refused;
  - two loaded models each reach only their own engine child;
  - engine paths are unreachable;
  - slots are shared with local chat;
  - no accounting records;
  - with chat history on, a canary request through the server creates no chat thread and leaves no canary under `<app data>/chats/`.

  With M4 (its task 2.17 implements the admission dispatch):
  - a shared model's route is admitted by the daemon and never reaches an engine directly, both for a qualified share and for a free share published with no serving profile;
  - it is refused with 429 `rate_limit` and the saturation reason when every slot is held, and with 503 `share_unavailable` when admission is draining or paused for qualification or the model is not ready, without queuing;
  - while its installation is under a qualification run, a route is refused with 503 `share_unavailable` and the being-qualified message, not `model_not_loaded`, without dispatch.
- [ ] 3.7 Session tests: `logout` and `configure` cancel network-routed requests while local routes continue; a route for another network identity returns `route_unavailable` without any provider contact. Also: after logout and a profile for another wallet on the same network, that wallet's routes return `route_unavailable`, the page shows only the hidden-route count and no account, credential, request-key or provider-request ID from the previous session, network log entries are gone, and loading the original profile restores the routes and their persisted request keys.
- [ ] 3.8 Interoperability:
  - stock OpenAI Python and Node SDKs with retries disabled against local and network routes, streaming and non-streaming, including request status and cancel;
  - a real-browser test confirms that a page on another origin, and a page reached through a DNS-rebinding hostname, cannot use the server.
- [ ] 3.9 Isolation (release gate 7):
  - listener inspection on macOS (`lsof`), Linux (`ss`) and Windows (`Get-NetTCPConnection`) shows only `127.0.0.1:<port>`;
  - a LAN connection attempt fails;
  - DHT, manifest, identify and local-network captures contain no server address or port.
- [ ] 3.10 Run `npm run check`, `npm test`, `npm run build` and `cargo test --manifest-path src-tauri/Cargo.toml`. Run a native desktop smoke test on each supported OS covering:
  - enable;
  - the `#/local-server` route and its `aria-current`;
  - the window close keeping the server running;
  - Quit releasing the port;
  - Reveal and Hide;
  - regeneration.

  Record the results in design.md. Do not present the feature as available until 3.1–3.10 pass.
- [ ] 3.11 After M3 (blocked on `specify-p2p-provider-transport`):
  - a peer-ID mismatch gives 502 `provider_authentication_failed`, logged as not sent, with no fallback;
  - a circuit-budget refusal, and no authenticated path within the ladder deadline, each give 503 `provider_unreachable` with the request key, logged as not sent, with no request byte written and no dispatch by another path;
  - a libp2p-only account from a profile with no libp2p node gives 503 `transport_unavailable`;
  - the active path (direct, hole-punched, relayed) is displayed;
  - a cut relay circuit is recovered through request status, including by request key;
  - a free-account route refused with 402 `free_allowance_exhausted` or `free_allowance_expired` returns 402 with that code, the native consumer's fixed message and the request key, logs a provider refusal, sends exactly one request, requests no quote, opening or funding, and keeps the route on its account;
  - a provider answering 503 `qualification_in_progress` with `Retry-After: 120` (M4's pause, or a controlled provider returning it) returns 503 with that code, the native consumer's fixed message "Provider is qualifying — try again later", the request key and `Retry-After: 120`, with no retry by the server; `Retry-After` of `0`, `7200`, an HTTP date or two headers is dropped; and a 402 `free_allowance_*`, a 429 or any other 503 carrying `Retry-After` is returned without it.
