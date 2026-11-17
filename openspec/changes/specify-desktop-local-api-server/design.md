## Scope and authority

The authority is `docs/chiral-book.md` as committed in `fe473508` (file SHA-256 `f036bb62c1338b35f2cc240e9d044c0584ed883da660a0be1cf8212f3fb4db00`), which committed the adopted-target-design edits, plus the uncommitted 2026-11-16 *Author decisions* edit on disk. That edit adds the *Author decisions* section, items 1–11, in place of the former list of open decisions, and changes *Relays carry ciphertext, under explicit limits* (its 24-hour relayed-grace sentence and its *Relay operation* paragraph), *Wire change* (offer schema v1 dropped with no acceptance window, and the per-account **free allowance** that a free offer signs), the `serving` bullet of *Signed offer schema*, the Part I *Share online* bullet, the new *Launch reputation: service score and track record (target design)*, the *Modes* table's Share row, the *Load and eject* and *History* bullets of *Local chat*, *Share online* steps 3, 4 (free shares, the launch window with automatic qualification and its pause, and Auto-price) and 6 and the paragraph after the steps, the A.4 *Offer card* and *Serving profile*, and *Status and milestones* items (4) and (5). A later revision of the same edit replaced the earlier items 8 (free by default, with an explicit *Charge for this model* upgrade) and 9 (an uptime-only track record) with items 8–11; neither earlier item had changed this change. The current revision was checked against the on-disk text with SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`, which records the author's 2026-11-16 chat decisions in *Wire change*, *Share online* step 4 and *Launch reputation* (none changes a requirement here; see *Implementation decisions* under *Open questions*); an earlier revision was checked against SHA-256 `8ee7fd8a114a8aefc7261a5716e2e80f10e051347f575cff645acf742e53e81c`, *Author decisions* items 1–11; an earlier revision was checked against the items 1–7 text (SHA-256 `3afcdb876c5422686259b0782ae2b4b3a5f0de968e7ecd60806b91523dfe743a`). The book is read-only for this change. The first draft was written against an earlier, uncommitted text. An earlier revision was re-checked against the text of the *Local API server* bullet list, A.1 rule 1, A.7 and *Storage layout*, which the 2026-11-16 edit does not change. The sections that govern this milestone are:

- **Part I, Direction, precedence, and status** (*Adopted target design*). The local-first desktop is a target. Current contracts govern until each change is specified, implemented and qualified, so this change specifies behavior and does not claim it.
- **Part I, Product and trust model; Architecture and responsibilities** (*Desktop app (target)* row: "serves a loopback OpenAI-compatible API … Keys and credentials stay native"). Provider risk disclosures carry over unchanged to network routes.
- **Part I, Desktop product: a local client that can share.** *Use the network* is offered "from the same chat and local API surface", "keeping every existing disclosure and adding the transport-path and relay-metadata disclosure of the peer-to-peer transport." Milestone 3's pre-funding path disclosure supplies that addition. Local use never sends prompts off the machine.
- **Part II, HTTP origins and trust boundaries.** The outbound public-HTTPS origin rules, `safe_http_client` DNS pinning, no proxies or redirects, the loopback-only administration listener, and the private engine.
- **Part II, Signed offer schema.** The `serving` bullet: absence is not a speed or capacity claim, and a free generation share (both token prices and minimum funding zero) may publish with `serving: null` after readiness probes, unbadged.
- **Part II, Peer-to-peer provider transport (target design).**
  - *SDK and tool compatibility*: the loopback gateway, `http://127.0.0.1:<port>/v1`, holds the provider credential natively, and SDKs use a local key. The desktop *and the CLI* run such a gateway.
  - *Transport and identity*: refuse a peer-ID mismatch, with no fallback. "The local loopback gateway never passes a provider's 3xx status or `Location` header to SDK clients." A listener with no TLS ingress enforces its own header-read timeout, maximum header size and idle timeout.
  - *Relays carry ciphertext, under explicit limits*: a cut circuit is recovered through request status, never by a blind paid retry. Its *Relay operation* paragraph (author decision): relays charge no fee and add no payment, agreement or term to the inference protocol. Its 24-hour grace (author decision): serving relays keep granting serving-grade circuits to a provider for 24 hours after its last valid offer, which M3 specifies and discloses (see *Open questions*, item 6).
  - *Wire change*: offer schema v2 with a signed `transports` field and a nullable `endpoint`; schema v1 is dropped with no acceptance window, and saved accounts keep their v1 terms verifiable (see Decision 14 and *Open questions*, item 5). A free offer signs a per-account free allowance, "after which the provider stops serving that account and the consumer may fund a paid one" (see Decision 22 and *Open questions*, item 10).
  - *Connectivity ladder; Performance and disclosure*: path disclosure.
- **Part II, Consumer control, credentials, and request lifecycle.**
  - The authorization matrix: `GET /v1/requests/:id` needs `read` scope and `POST /v1/requests/:id/cancel` needs `infer` scope.
  - Credential issuance `{expires_at, spend_cap_wei, max_concurrency, scopes}`; "a credential cap limits authority on the same prepaid balance; it is not a new balance or refund guarantee."
  - `Idempotency-Key` syntax and 409 `duplicate_request`; "do not issue a new chargeable operation just because the output stream was interrupted."
- **Part II, Configuration and state; Shared signing-wallet directory.** Protected native secret files; network-scoped consumer accounts, credentials and request keys; no tokens on command lines or in URLs.
- **Part II, API compatibility and model qualification.** The endpoint table; honest errors; "disable automatic paid retries and retain request keys"; generated text and tool arguments are untrusted data.
- **Part II, Native provider provisioning and operations.**
  - *Listener and secret boundaries*: engine, tokenization, metrics, model-loading and admin routes are never public API; the admin helper "never places the bearer in argv".
  - *Provider lifecycle*: "An occupied private port is an error, not permission to adopt another process."
  - *Backup, incident response*: logs carry request IDs, statuses, token counts, costs and timings, never content or credentials.
  - *Artifact and execution identity*: pinned, immutable artifacts; no request-time downloads.
- **Part II, Desktop local-client workflow (target design).**
  - *Modes*: Local mode never contacts a chain, wallet, catalog or provider.
  - *Local API server*: the normative bullet list this change implements. It covers:
    - the six routes, with `/v1/requests/:id` and `/v1/requests/:id/cancel` "for paid routes";
    - "one account credential the user created for that route: `infer` scope (plus `read` for recovery), an explicit spend cap and an expiry";
    - "the Local server page shows each route's cap";
    - a shared model's routes go "to the provider daemon and are admitted like local chat".
  - *Local chat*: the *History* bullet, which concerns chat threads only (see *Open questions*).
  - *Share online*:
    - the desktop ejects its own engine for a shared model, and local-API-server routes mapped to it go to the daemon, where they "acquire the same per-model slot and qualified arrival budget before dispatch";
    - step 3: desktop shares are libp2p-only (author decision), and a provider that wants an HTTPS origin runs `chiral_daemon` with the provider guide (see *Open questions*, item 7);
    - step 4's free shares: a generation share with zero token prices and zero minimum funding may skip qualification, publishes with no serving profile after readiness probes, carries no speed class or waiver, and is labeled *Unqualified — no speed guarantee* wherever consumers see it;
    - step 4's launch window: qualification runs automatically in the window's first quiet hour, and "free admission pauses for the run, with a retryable busy response"; Auto-price then changes prices by new signed offer revisions, and "existing accounts keep their agreed terms" (see Decision 22 and *Open questions*, item 8).
  - *Storage layout*: `local-server/` holds the bearer and settings; "Every secret is a protected file; the webview receives none except the explicit reveals of A.1.1."
  - *Author decisions* (recorded 2026-11-16; how each touches this change is under *Open questions*); *Status and milestones* (milestone 2).
- **Part II, Release gates and source map.** Item 7, "local API server isolation".
- **Appendix A.**
  - A.1, rules 1–5 and 7–10: native boundary, no simulation, explicit network, exact money, untrusted output, recovery over retry, session scoping, accessibility, closing is not quitting. Rule 1 explicitly allows "the local API server bearer the user explicitly reveals on the Local server page (A.7) … shown in a masked field and cleared on hide or account change."
  - A.2: Window close and Quit.
  - A.4: the *Serving profile* component. Offers without a serving commitment state that no generation-speed guarantee applies, and a free unqualified generation share reads *Unqualified — no speed guarantee*.
  - A.5.3: credential issuance options and lifetimes, and the SDK snippet convention, with retries disabled.
  - A.6: adding a page "by adding a route, a navigation item (label, short label, icon)".
  - A.7: the rail order (Chat, Discover, My models, Local server, Share, Network, Wallet) and the *Local server* page, with "local bearer (masked; revealed only on explicit request under A.1.1; regenerate), model routing table with each paid route's scoped credential and spend cap".

`docs/provider-qualification.md` was read for its listener conventions: distinct integer ports from 1,024 to 65,535, and inspecting loopback listeners with `lsof`, `ss` or `Get-NetTCPConnection`. Its qualification procedure does not apply to local routes, which are unqualified and never used as evidence. Network routes rely on providers' existing qualification gates.

The current implementation was compared:

- `src-tauri/src/consumer.rs`:
  - `inference_response` already persists a `{contract_id, request_key, created_at}` record before dispatch, as `requests/<request_key>.json` in the network-scoped account directory. It sends `Idempotency-Key` and `X-Chiral-Client-Version`, rewrites `model` to the agreed model and never retries. It bounds responses to 8 MiB, a 120-second stream start and idle time and 3,600 seconds in total, and maps provider errors to fixed messages through `safe_error`. Its client disables redirects, so a provider 3xx arrives as an ordinary non-success response.
  - `credential(&saved, None)` always selects the account's active credential, for inference and for `request_status` and `cancel_request`.
  - `credential_issue` makes a newly issued `infer` credential the active one when no valid active credential exists. The locally stored credential holds only its ID, bearer, contract and expiry.
- The provider's `GET /v1/requests/:id` resolves either a request ID or a request key (`billing_store::request_view`), and cancel returns 202 `cancelling` or the request view.
- `inference_gateway.rs` / `inference_backend.rs` bound provider request bodies at 256 KiB with a 5-second body read. Every `axum::serve` listener in the codebase relies on its defaults and sets no header-read timeout, header-size or connection limit; providers have Caddy in front.
- `inference_gateway.rs` admission takes a model concurrency slot for every request (`model_saturated` when none is free) and charges the global arrival envelope only when the agreed model has a serving profile (`rate_limit`). A model without a serving profile is therefore admitted by slots alone.
- `desktop.rs` exposes native commands only and has no HTTP listener.
- `src-tauri/capabilities/default.json` grants no shell, filesystem or HTTP permission.

Nothing implements a local server.

Other milestones are dependencies and are not specified here:

- **`specify-desktop-local-models` (M1):**
  - installed-model identity and the managed runtime, with one engine child per loaded model;
  - load and eject, and local chat;
  - the navigation order that places Local server after My models. M1 also defers the Local server navigation item and the bearer reveal to this change.
- **`specify-p2p-provider-transport` (M3):** the libp2p stream protocol, transport selection, peer-ID authentication, the connectivity ladder, the circuit-budget check, path disclosure and the offer schema change, including the free allowance and its HTTP 402 `free_allowance_exhausted` and `free_allowance_expired` refusals (`inference-commercial-terms`, *Free accounts are served only within their signed allowance*).
- **`specify-desktop-share-online` (M4):** sharing, admission for local use of a shared model and its dispatch (M4 task 2.17), refusal of local use during a qualification run, the qualification pause and its HTTP 503 `qualification_in_progress` response (*Qualification runs automatically in the share's first quiet hour*), and the Quit and Stop-sharing interactions.

## Decisions

1. **One IPv4 loopback listener on a user-chosen port.**
   - The book requires "loopback only … on a user-chosen port" and documents `http://127.0.0.1:<port>/v1`. Binding only `127.0.0.1`, and not `::1`, keeps the allowed `Host` set small and exact. Clients that resolve `localhost` to `::1` first fall back to IPv4.
   - The port range is 1,024–65,535, the convention in the qualification guide for ports that do not need privileges.
   - An occupied port, or a port the desktop's own managed listeners use (engines, provider public or admin, DHT), is an error. The server never falls back to another port, consistent with the provider-lifecycle rule. Checking the peer address on accepted connections is defense in depth.

2. **Off on a fresh install; the user's choice persists.**
   - "Off by default" holds for a fresh installation and for any state with no recorded enablement.
   - The book stores the server's "settings" in `local-server/`, so the enabled flag and port persist. A server the user left enabled restarts at the next launch and is shown as running.
   - This is an implementation decision, not one of the book's *Author decisions*. Open question 2 asks the author to confirm it.

3. **Bearer generation, storage, use and reveal.**
   - The bearer has at least 256 bits from the operating system's CSPRNG and is printable URL-safe ASCII. It is a protected file with the same Unix and Windows permission rules as other native secrets.
   - It is compared in constant time, accepted only from `Authorization: Bearer`, and never from a query string or cookie.
   - An insecure or corrupt file fails closed and is never repaired silently, matching "corrupt/insecure caches fail closed."
   - Regenerating writes a temporary protected file and renames it into place. New requests must use the new value; already admitted requests are not torn down.
   - A.1 rule 1, A.7 and *Storage layout* all allow one exception: an explicit **Reveal** on the Local server page, shown in a masked field and cleared on hide or account change. The A.5.3 credential reveal already follows this pattern.
   - The value is cleared on Hide, on leaving the page, on Regenerate, on logout and on a profile change. Logout and profile change are this change's reading of "account change": they are the session boundaries A.1 rule 8 already clears credentials on.
   - Snippets never embed the value, even after a reveal. They read the protected file or an environment variable (Decision 19).

4. **Bound connections first, then check Host, Origin and the bearer, all before the body is read.**
   - Stock SDKs and `curl` send no `Origin`.
   - Browsers send `Origin` on every CORS request and on POSTs. An `Authorization` header forces a CORS preflight, which itself carries `Origin`. Rejecting every `Origin`, including `null` and the webview's `tauri://localhost`, while emitting no CORS headers, therefore stops web pages from using the server (CSRF).
   - The exact `Host` allowlist (`127.0.0.1:<port>`, `localhost:<port>`) and refusing absolute-form request targets stop DNS rebinding.
   - Because both checks run before authentication, a page learns nothing about the bearer, and no body is read for refused requests.
   - The connection bounds of Decision 6 apply before all three checks, because they protect the listener itself rather than a request.

5. **Exactly the book's six routes.**
   - The book's *Local API server* bullet lists `/v1/models`, `/v1/chat/completions`, `/v1/completions`, `/v1/embeddings` and, "for paid routes", `/v1/requests/:id` and `/v1/requests/:id/cancel`. The server serves exactly these.
   - The two request routes exist so that SDK-driven tools can recover their own paid requests. M3's recovery contract also cancels through `POST /v1/requests/:id/cancel`.
   - They resolve `:id` only to requests the server itself forwarded for a network route. A local tool therefore cannot read or cancel the desktop's own chats, or another route's requests (Decision 12).
   - Errors reuse the provider gateway's error object, so SDKs parse them the same way.
   - `GET /v1/models` is answered locally and lists route names, not provider catalogs.

6. **Bounds reuse existing values where they exist.**

   | Bound | Value | Source |
   |---|---|---|
   | Request line and headers received | 5 s | new |
   | Header block | 16 KiB (431) | new |
   | Open connections | 64 | new |
   | Keep-alive idle | 30 s | new |
   | Request body | 256 KiB | provider gateway `MAX_REQUEST_BYTES` |
   | Body read | 5 s | gateway `BODY_READ_TIMEOUT` |
   | Response size | 8 MiB | native consumer |
   | Stream start and idle | 120 s | native consumer |
   | Total duration | 3,600 s | native consumer |
   | Requests in progress | 16 | new |

   - **Connection bounds.** No TLS ingress fronts this listener. The book makes the same point for the libp2p stream server: such a listener must enforce a header-read timeout, a maximum header size and an idle timeout itself. `axum::serve` sets none of them.
   - Without these bounds, any local process could exhaust file descriptors and memory with slow or partial headers without ever reaching the in-progress count. That includes processes run by another OS user, which the bearer cannot stop at the TCP layer.
   - The values match the 5-second body deadline and the provider gateway's scale. They are new, not book values.
   - **In-progress limit.** The in-progress limit is new: twice the desktop GUI's eight-stream limit. It refuses without queuing, so memory stays bounded.
   - The 64-connection cap leaves room for SDK connection pools above the 16 requests in progress.
   - The same bounds apply to every route, so a tool sees one consistent envelope.

7. **An explicit, pinned routing table.**
   - Route names are chosen by the user and unique by exact bytes. A route name may differ from the provider's model ID; the server rewrites `model` to the agreed ID.
   - Each entry has a stable route ID that is never reused. Request records refer to the route by this ID, so renaming a route keeps its recovery records, and deleting it and re-creating one with the same name does not inherit them.
   - Targets are pinned. A local route names an exact installed revision and artifact. A network route names an exact network identity, account and dedicated credential (Decision 8). A route therefore never shifts silently to an updated model, another account, another provider or another credential. This matches "a later update is a new download with a new identity."
   - The table holds at most 64 entries and contains no secrets. Credential IDs are public metadata. Raw URLs, offers and bearers are refused as targets, so the server cannot become a generic outbound proxy around the HTTPS origin boundary.

8. **Each network route has one dedicated, capped and expiring credential.**
   - The book requires "one account credential the user created for that route: `infer` scope (plus `read` for recovery), an explicit spend cap and an expiry — never owner or wallet authority." Any process that can read the bearer file can spend through the route. The cap and the expiry are what bound that exposure.
   - The route editor issues the credential through the existing owner-authenticated native `credential_issue`, using the A.5.3 lifetime bounds. This is a desktop action, not a server action. The HTTP server never performs owner-authenticated or credential-management operations.
   - Scopes are exactly `infer` and `read`. `read` is what `GET /v1/requests/:id` requires; `infer` is what inference and cancel require.
   - The cap must be non-null and positive: a null cap is the full balance, and a zero cap could never be spent.
   - The route credential is not made the account's active credential, which today's `credential_issue` would otherwise do when none is active. Binding the active credential, or a credential already bound to another route, is refused. Otherwise the desktop's own chats would draw on the route's cap, and revoking the route would break them.
   - The local `Credential` record stores no scopes or cap, so the route records the scopes, cap and expiry it was issued with. The Local server page shows them, with the cap in exact CFX.
   - The server uses only that credential, with no fallback to the active credential and no reissue. A missing, expired or locally revoked credential is 409 `no_credential`. A credential revoked elsewhere is refused by the provider and reported as the classified `invalid_credential`.
   - Removing a route does not revoke silently; the page offers `credential_revoke`.
   - The disclosure states the cap, that the cap is authority on the same non-refundable balance and not a separate balance or refund, that the provider sees plaintext, and that a retry without a reused key is a new paid request.

9. **One native dispatch path.**
   - Network routes call the existing native consumer inference path. They do not use a second HTTP client, so client-version enforcement, `model` pinning, `safe_http_client`, request-key persistence, response bounds and `safe_error` classification stay single-sourced.
   - The consumer gains four extensions:
     - it accepts a validated caller-supplied request key;
     - it accepts an explicit credential ID for inference, `request_status` and `cancel_request`, with no fallback;
     - it can issue a credential without activating it;
     - its request record gains the route ID and provider request ID and is keyed by account and request key. Client keys may contain `:`, so file names must be encoded. Today two accounts sharing a key would overwrite one record in the network-scoped `requests/` directory.
   - The server forwards no client headers except a validated `Idempotency-Key`. It returns only allowlisted response headers (content type, a well-formed provider request ID, the request key, and `Retry-After` only on 503 `qualification_in_progress` within 1–3600 integer seconds, Decision 22) and never echoes provider free text, matching the consumer's existing "never echo provider-supplied text or secrets" rule.
   - A provider 3xx is not followed (the client already disables redirects) and is not relayed. It becomes 502 `provider_error` with no `Location`, as the book requires of the loopback gateway. On libp2p routes this complements M3's rule that the stream client follows no redirects.

10. **Client keys pass through; absent keys are generated and disclosed.**
    - A valid client `Idempotency-Key` is used as is, so a deliberate client retry with the same key hits the provider's 409 `duplicate_request` instead of creating another charge.
    - Without a key, a UUID is generated. Either way the key is persisted before dispatch and returned in `x-chiral-request-key`, alongside the provider's `x-request-id`.
    - The header name is new and specific to Chiral. The book does not name one.

11. **No resend after possible delivery.**
    - Once any byte may have reached the provider, no path resends the request: not the same key, a new key, another transport, another provider or another account.
    - An interrupted stream ends with an error event and no `[DONE]`, so clients cannot mistake it for a completed response.
    - A client disconnect cancels the upstream request, as the consumer already does by dropping the stream, and is logged as possibly charged.

12. **Recovery works through the server's request routes and through the native desktop.**
    - **Through the server.** `GET /v1/requests/:id` and `POST /v1/requests/:id/cancel` resolve only records the server persisted for network routes, under the loaded profile's network identity.
      - `:id` may be the provider request ID or, when the failure came before response headers, the persisted request key. This matches M3's recovery boundary: reconcile by ID, or by the retained key when no ID was received. The provider's `request_view` already accepts either.
      - A key matching records on more than one account is refused as ambiguous rather than guessed.
      - With no profile loaded the routes return 503 `route_unavailable`, not 404, so a tool cannot read "not found" as "never sent". A 404 states only that the local server holds no such request under the loaded profile.
      - Status and cancel use the recorded route's credential, which has the scopes the provider requires.
      - Status responses are decoded natively to the provider's request-status object, with money as canonical wei.
      - Cancellation is never retried, and the page, snippets and log label it as possibly charged and not a refund.
    - **Through the native desktop.** Persisted request keys appear in Usage & API keys, Requests, through `local_requests`, with the existing `request_status`, `usage` and `cancel_request` operations. This works with the server disabled and after a restart.
    - After a route credential expires, SDK-side recovery through the server stops with `no_credential`, but desktop-side recovery stays available. The in-memory request log is a convenience, not the recovery record.

13. **Local routes go to the route's own engine child, or through share admission; loading is explicit only.**
    - M1 runs one managed engine child per loaded model. A local route dispatches only to the child with the route's exact installation loaded.
    - While that installation backs an active share, M4 has the desktop eject its own engine and hand the model to the daemon. The book sends "every local-API-server route mapped to it" to the daemon, "admitted like local chat". The server then uses M4's admission and never reaches an engine directly.
    - Admission saturation is 429 `rate_limit`, the same code as a full local engine. Draining, paused for qualification or not ready is 503 `share_unavailable`, matching M4's refusal reasons. Neither is queued ahead of paid traffic.
    - The same path applies to a free share that the book's *Author decisions* item 1 lets publish without qualification and with no serving profile. Such a share has no qualified arrival budget: the signed arrival rate (`arrival_milli_rps`) is a `ServingProfile` field, which the share lacks, and today's gateway charges the arrival envelope only when a serving profile exists. The server therefore requires only that local use go through whatever admission M4 applies to the share, and maps an arrival-budget refusal to 429 only where the share has such a budget. It never bypasses admission because a share is unqualified. While a free share's readiness probes have not passed, M4's admission reports the model not ready, which is 503 `share_unavailable`.
    - A local route never carries a speed guarantee, whether its installation is unshared, shared with a qualified serving profile or shared as a free unqualified share.
    - M4 refuses local use of an installation while a qualification run of it is in progress, because local requests would disturb the measurement. The server returns 503 `share_unavailable` with a fixed message that the model is being qualified, ahead of both dispatch paths. It does not return `model_not_loaded`, which would tell the user to load a model that cannot be used yet.
    - M4 task 2.17 implements the admission dispatch for these routes, because M4 owns the admission route and lands after this change. This change defines the response mapping and verifies it in task 3.6.
    - The server never loads, ejects or downloads a model on demand. The book requires explicit load settings and forbids request-time downloads.
    - It validates requests with the provider gateway's request schema but performs no accounting.
    - It shares engine slots with local chat and never raises them.
    - Local routes need no profile, policy endpoint or connectivity, matching the Local mode row of *Modes*.

14. **libp2p comes only through M3.**
    - `transport_unavailable` covers both the period before M3's transport exists and profiles with no libp2p node. M3 already shows libp2p-only offers as unreachable from such profiles.
    - A libp2p-only account can only exist once M3's offer schema exists, because current-schema offers always carry `endpoint`.
    - Under the book's *Author decisions* item 5, M3 never newly selects a v1 offer but keeps saved v1 accounts usable under their retained v1 terms: such an account is reached over its retained offer's HTTPS endpoint or, when a currently valid v2 offer from the same provider wallet and deployment declares `libp2p` for the account's signed peer ID, over libp2p (M3 *Transport selection follows the signed offer and never changes money or terms*). The server takes the transport from the native consumer's selection, so it assumes neither that a retained offer is v2 nor that a v1 account is HTTPS-only.
    - After M3, the server uses the native consumer's transport selection, M3's stream protocol and peer-ID check, and displays M3's path disclosure. Every M2 identity, retry, recovery, redirect and bounds rule applies unchanged.
    - M3 requires a peer-ID refusal to be reported as a provider-authentication failure, not as unavailability, and reported as "not sent", since it happens before any request byte is written. The server therefore returns 502 `provider_authentication_failed` and logs the request as not sent.
    - M3 also reports a request as not sent when its circuit-budget check withholds it or no authenticated path is established within the ladder deadline. The server returns 503 `provider_unreachable` with the request key and logs the request as not sent, so an SDK client and the log can tell it apart from an ambiguous outcome. It does not dispatch the request by another path.
    - This change does not specify the transport.

15. **Session scoping follows A.1.8.**
    - Logout and profile changes already cancel every native stream. Server requests on network routes are included.
    - Network routes carry their full `NetworkIdentity` and resolve only under an identical profile, because consumer state is network-scoped.
    - Local routes continue, because local mode does not depend on a profile.

16. **Never advertised; separate from provider listeners.**
    - The server's address never enters any publication path: DHT, manifests, offers, identify, relay, catalog, local-network discovery or port mapping.
    - It is a separate router and port from any provider daemon listener, so it cannot become a provider endpoint or bypass provider admission.

17. **The request log is in memory, bounded and metadata only.**
    - The log holds the 1,000 most recent entries for the current session and is cleared on request and at exit. It contains no content and no secrets.
    - Token counts are labeled as reported. Costs come only from authoritative request status, displayed in exact wei. The log never computes billing from stream fragments, because stream fragments are not billing evidence.
    - The log is not persisted: persistence would add retention the book does not require, and recovery already has durable request keys (Decision 12).

18. **No change to the window lifecycle.**
    - The server is part of the native process: closing the window keeps it running (A.1.10), and the confirmed Quit ends the process and releases the port.
    - The existing Quit confirmation already says active streams are cancelled and may still be charged. This change adds no new Quit text, which avoids colliding with M4's Quit change for active shares.

19. **Snippets keep the bearer out of every argv.**
    - The book forbids tokens on command lines, and its admin helper "never places the bearer in argv". The common `curl -H "Authorization: Bearer $(cat <file>)"` idiom expands the bearer into curl's argv, where the process table shows it to other users.
    - The `curl` snippet therefore feeds curl a configuration on standard input (`curl -K -`) written by a shell builtin. For example `printf` with `$(< file)` in bash or zsh keeps the value in the shell and out of any child's argv.
    - The Python and JavaScript snippets read the file or an environment variable in-process and disable retries.
    - Verification samples the process table while each snippet runs.

20. **The Local server page has its own route and navigation item.**
    - A.6 adds a page "by adding a route, a navigation item (label, short label, icon)". A.7 places Local server after My models and before Share. M1 defers this item to this change.
    - The route is `#/local-server`. The item carries `aria-current` (A.1.9).
    - The item ships with the native commands, so no build shows a page without a server.
    - The short label and icon are presentation choices (A.3) and are not fixed here.

21. **A network route to an unqualified offer says so.**
    - The book labels a free unqualified generation share *Unqualified — no speed guarantee* "wherever consumers see it" (*Share online* step 4, A.4). The routing table is a consumer view of the account a network route spends from, so it carries the same statement.
    - The label is derived from the account's agreed, signed offer alone: no serving profile and a generation capability (`chat` or `completions`), whatever the offer's prices. This is the predicate `specify-desktop-share-online` uses for every consumer surface (its decision 30 and requirement *Unqualified generation offers are labeled for consumers*, which also covers the account views a network route spends from), so the two changes cannot disagree once both are archived. A priced generation offer without a serving profile, which production provider configuration rejects but a misconfigured or dishonest provider could still sign, is labeled the same way, which is true of it. An embedding-only offer without a serving profile shows A.4's statement that no generation-speed guarantee applies. A route whose agreed offer has a serving profile gets no extra label; the routing table does not restate qualification terms, which stay on the offer card.
    - The label is display only. It changes no admission, credential, spend-cap, request-identity or recovery rule; a free account still needs a dedicated, positively capped route credential like any other network route.

22. **Free-allowance and qualification-pause refusals are named, mapped exactly and never retried.**
    - M3 makes a free account servable only within its signed allowance (book *Wire change*; *Author decisions* item 10). A request that the remaining allowance cannot cover, or that arrives after the allowance's lifetime, is refused with HTTP 402 `free_allowance_exhausted` or `free_allowance_expired`, before dispatch and with no provider request record or hold. M4 has a provider in its automatic qualification pause answer new requests with HTTP 503 `qualification_in_progress` and `Retry-After`, admitting and charging nothing (book *Share online* step 4; *Author decisions* item 8).
    - The general rule already converts provider errors to the native consumer's classified codes. These three are named so that an SDK client gets a distinct, stable code instead of a generic `provider_error`, and so that the request log can tell a provider's refusal apart from a transport failure, a request that was not sent, and an ambiguous outcome. The two 402 codes use the native consumer's fixed messages, which M3 requires to say which applies; when both apply the provider reports `free_allowance_expired` (implementation decision H, 2026-11-16, below), and the server passes whichever code it receives. For `qualification_in_progress` the server also uses the native consumer's classification and fixed message, "Provider is qualifying — try again later": retryable, not admitted, no charge, and never retried automatically as a paid request (implementation decision E, 2026-11-16, owned by M3; M4 keeps the provider side).
    - The persisted request-key record is kept, as for every request, but nothing is retried. M3 forbids the native consumer from retrying either 402, or from funding or opening another account, without the user's explicit action. The server performs no quote, opening or funding at all (Non-goals), and a route stays pinned to its account (Decision 7). A user who wants more service opens a new account through the Network workflow and edits the route. Until then, requests naming the route keep reaching the same account and keep receiving its refusal.
    - `Retry-After` is passed only on 503 `qualification_in_progress` (implementation decision F, 2026-11-16, which the author may revisit). That refusal admits and charges nothing, so the hint is safe to give an SDK client and tells it when the run should be over. The value is passed only as an integer number of seconds from 1 to 3600 and is otherwise dropped, so a provider cannot hand a client an HTTP date, a zero that invites a tight loop, or an unbounded delay. Every other refusal, including both 402 codes, carries no `Retry-After`: a free allowance does not come back with time, and other errors are recovered through request status. The server itself never retries, whatever the header says, and SDK snippets still disable SDK retries, so a later attempt is the client's deliberate new request; a stock SDK without the snippet's setting may retry this 503 after the hint, which costs nothing because the provider admitted nothing.
    - These refusals exist only once M3's allowance and M4's pause are implemented. A retained v1 free account has no allowance and is served under its saved terms.

## Non-goals and limitations

- **Network exposure.** No LAN, remote or TLS exposure, as the book requires ("Exposure to a LAN is out of scope"). There is no bind option, port mapping or reverse-proxy guidance.
- **The OS account is the trust boundary.** Any process of the same OS user that can read the bearer file can use the server, so this change does not protect against malware running as that user. The spend cap and expiry of each route credential bound what such a process can spend; the protected file excludes other users only. The connection bounds limit what other users' processes can do at the TCP layer.
- **CLI gateway.** The book says "the native desktop and CLI therefore run a local loopback gateway." This change specifies only the desktop. `specify-p2p-provider-transport` explicitly excludes the CLI gateway. It is therefore owned by a named follow-up change, `specify-cli-loopback-gateway`, which reuses this capability's gate, bounds, routing, route-credential and request-identity rules. The routing core is specified so that a CLI front end can reuse it. See open question 1.
- **Other OpenAI surfaces.** No Responses API, images, audio, files, fine-tuning, batch, assistants or tool execution, matching the book's API scope.
- **No funding or credential management from the server.**
  - The HTTP server performs no quotes, funding, top-ups, funding recovery or owner challenges, and no credential issuance, rotation, revocation or export.
  - The Local server page issues a route's dedicated credential, and offers its revocation, only through the existing native `credential_issue` and `credential_revoke` operations.
  - Everything else remains a Network-page workflow with its A.1.6 and A.1.7 disclosures.
  - Exporting a credential for direct HTTPS SDK use is unchanged.
- **Other milestones.**
  - Model discovery, download, verification, library, runtime and load/eject, and local chat and its history, belong to M1.
  - The libp2p transport, relay limits and offer schema change belong to M3.
  - Sharing, shared-model admission, sleep prevention and the Quit and Stop-sharing interactions belong to M4.
- **No persistent log or cost computation.** The request log is not persisted and computes no cost.
- **Not qualification.** A local route is never a speed or capacity qualification and carries no speed guarantee, including a local route to an installation shared as a free unqualified share. Reaching a provider through the server adds loopback latency that is outside the provider's signed guarantee, like any client-side overhead.
- **No chat history.** Requests through the server are not chat threads and are never written to local chat history, whatever the history preference of M1's `desktop-local-chat`.
- **Implementation and verification are not claimed.** This draft specifies requirements only. Release gate 7's "local API server isolation" stays open until section 3 of `tasks.md` is executed on real native builds.

## Open questions

The book's *Author decisions* items 1–11 were decided by the author on 2026-11-16; this change decides none of them. How each touches this change:

- Item 1, free sharing without qualification: Decided 2026-11-16 (book *Author decisions* item 1): allowed for a generation share whose input price, output price and minimum funding are all zero, published with no serving profile after readiness probes and labeled *Unqualified — no speed guarantee*. A local route to an installation shared this way still passes through M4's admission and is never sent directly to an engine; it carries no speed guarantee, and an arrival-budget refusal applies only where the share has an arrival budget (Decision 13). A network route whose agreed offer is such a share, or any other generation offer without a serving profile, is labeled *Unqualified — no speed guarantee* in the routing table (Decision 21).
- Item 2, relay operation: Decided 2026-11-16 (book *Author decisions* item 2): project-operated testnet relays named in shipped profiles, plus relays anyone else operates, with no relay fee. Relayed libp2p routes therefore carry no relay fee and no extra payment, agreement or term, so the routing table, spending disclosure and request log show no relay cost. Circuit limits, quotas and the circuit-budget check still come from M3, which fixes and measures their values; task 2.8 waits on M3's transport, not on this decision.
- Item 3, local chat history default: Decided 2026-11-16 (book *Author decisions* item 3): on by default, local-only and protected, and can be turned off and cleared. It governs M1's chat threads. The request log here is in memory and metadata only, and requests through the server are never written to chat history (requirement *The local API request log records metadata only*), so this change is independent of the default.
- Item 4, sleep prevention: Decided 2026-11-16 (book *Author decisions* item 4): automatic while sharing, after preselected consent in the share wizard. It belongs to M4 and does not affect this change; an enabled local server alone does not prevent sleep.
- Item 5, offer schema v1: Decided 2026-11-16 (book *Author decisions* item 5): dropped with no acceptance window. Schema v2 replaces it at once on `/chiral/inference/manifest/2`, upgraded nodes stop publishing, discovering, quoting and funding v1 offers, and saved accounts keep their v1 offers, quotes and receipts verifiable. It belongs to M3. The server selects, quotes and funds nothing, so its routes are unaffected; a route to a saved v1 account keeps working over the transport the native consumer selects (Decision 14).
- Item 6, relayed service after offers lapse: Decided 2026-11-16 (book *Author decisions* item 6): a serving relay keeps granting serving-grade circuits to a provider until 24 hours after the signed `expires_at` of the most recent offer the relay itself verified for it, within the same per-provider quotas; M3 keeps that evidence durably, so a relay restart does not end it, but a relay whose evidence store is full or has failed can end it earlier. M3 owns the rule and its pre-funding disclosure, and M4 states it to the provider. The server adds no rule: it shows M3's path disclosure for libp2p routes, and once a relay grants only coordination-sized circuits and no other path passes the circuit-budget check, a paid request is not sent and returns 503 `provider_unreachable` (Decision 14).
- Item 7, desktop sharing: Decided 2026-11-16 (book *Author decisions* item 7): libp2p only. The share wizard offers no HTTPS origin, and a provider that wants one runs `chiral_daemon` with the provider guide. It belongs to M4. HTTPS remains a provider transport, so network routes still reach HTTPS providers through the native consumer's HTTPS path (requirement *Network routes forward through the native consumer path with the credential held natively*). M4's wizard accepts no HTTP origin or endpoint of any kind, the local server's address included (its *Reachability is verified through a relay before publishing*), which meets this change's rule that the share workflow never offers the local server as a provider endpoint.
- Item 8, launch window and Auto-price: Decided 2026-11-16 (book *Author decisions* item 8; replaces the earlier item 8, a free default with an explicit *Charge for this model* upgrade, which never changed this change). It belongs to M4. Local routes: M4's automatic qualification run, including its qualification pause, is a qualification run of the installation, so a local route to it is refused with 503 `share_unavailable` and the being-qualified message (Decision 13). A refusal because admission is paused for qualification is mapped the same way. After a passing run, M4 promotes the serving profile into the share, so its admission also applies the qualified arrival budget, which rule (b) already maps to 429. Network routes: Auto-price changes only new accounts' terms, and a route is pinned to one account (Decision 7), so routes need no change. The *Unqualified* label follows the account's agreed offer (Decision 21), so an account opened before a promotion keeps it. A provider in its qualification pause answers 503 `qualification_in_progress`, which the server returns as such without retry (Decision 22).
- Item 9, launch reputation: Decided 2026-11-16 (book *Author decisions* item 9; replaces the earlier uptime-only track record). It belongs to milestone 5, `specify-provider-launch-reputation`, and M4. The server shows no reputation, and the routing table is unaffected. Probe accounts belong to the catalog's observer, not to the desktop, so they never appear as routes.
- Item 10, free allowance: Decided 2026-11-16 (book *Author decisions* item 10): free offers sign a per-account token allowance and lifetime (default 20,000 tokens and 24 hours), after which the account stops serving. M3 owns the allowance and its enforcement. A network route to a free account maps the provider's 402 `free_allowance_exhausted` or `free_allowance_expired` exactly, logs it as a provider refusal, and neither retries nor opens or funds another account (Decision 22; requirement *Network routes forward through the native consumer path with the credential held natively*).
- Item 11, defaults: Decided 2026-11-16 (book *Author decisions* item 11): the ready-to-charge and Auto-price thresholds. They belong to M4 and milestone 5 and do not affect this change.

**Implementation decisions (2026-11-16).** Chosen by the assistant as defaults across milestones 2–5; the author may revisit them:

- E: native consumers (desktop Chat, Playground, CLI) treat a provider's 503 `qualification_in_progress` as retryable and not admitted: no charge, never retried automatically as a paid request, shown as "Provider is qualifying — try again later". M3 owns the classification and message; M4 keeps the provider side. The server returns that code with that message (Decision 22).
- F: the server passes `Retry-After` (integer seconds 1–3600, otherwise dropped) only on its 503 `qualification_in_progress` response (Decision 22; requirement *Network routes forward through the native consumer path with the credential held natively*).
- G: embedding-share readiness for Auto-price is M5's and M4's; it does not affect this change. *(Revised 2026-11-17 by M4 and M5: an embedding share is never ready to charge, because the book's ready to charge requires "a valid qualification", and Auto-price is unavailable for it. This change is still unaffected.)*
- H: M3's allowance bounds (tokens 1,000–10,000,000; lifetime 3,600–2,592,000 s) and its 402 codes `free_allowance_expired` and `free_allowance_exhausted`, expired reported first when both apply, are accepted. The server maps whichever code the provider returns and enforces no bound itself.

The author's 2026-11-16 chat decisions A–D (book SHA-256 `32f447d9…`: keeping pre-promotion free accounts served, the account cap counting only live accounts, the probe walk-back, and "Billing unchecked") belong to M3, M4 and M5 and change no requirement here; a free account served after promotion still maps its 402 refusals exactly.

The following remain open:

1. **CLI loopback gateway** (non-blocking for the desktop).
   - `specify-p2p-provider-transport` excludes the CLI gateway, and this change covers only the desktop. The book's statement is therefore tracked by a named follow-up change, `specify-cli-loopback-gateway`.
   - That follow-up reuses this capability's gate, bounds, routing, route-credential and request-identity rules, and adds the CLI's own lifecycle and bearer location.
   - The author should confirm that split, or move the CLI gateway into this change before it is archived. Only a CLI task depends on the answer, and this change includes none.
2. **Persisted enablement at launch** (non-blocking).
   - Decision 2 restarts a server the user left enabled. If the author prefers the server to start disabled at every launch, only the first requirement's persistence sentence and task 2.1's launch behavior change.

**Blocking dependencies.** These are not author decisions:

- Tasks 2.5 and 3.6 (local routes), and the navigation placement in task 2.10, are blocked until `specify-desktop-local-models` defines installed-model identity, per-model engine children and the navigation order.
- Tasks 2.8 and 3.11 (libp2p routes) are blocked until `specify-p2p-provider-transport` is specified and implemented. So are the free-allowance refusal tests in task 3.11, because the allowance and its 402 codes exist only once that change is implemented. The qualification-pause test in task 3.11 needs a provider that implements `specify-desktop-share-online`'s pause, or a controlled provider that returns its response.
- The shared-model admission dispatch is implemented by `specify-desktop-share-online` task 2.17, not by task 2.5. Its tests in task 3.6, including the qualification-run refusal, and the share-endpoint exclusion in task 2.11 are blocked until that change lands.
- Archive this change after `specify-desktop-local-models`, whose `desktop-model-acquisition`, `desktop-local-runtime`, `desktop-local-workbench` and `desktop-local-chat` capabilities its requirements reference. It references `desktop-share-online` and `p2p-provider-transport` only conditionally, for behavior that applies once those capabilities are implemented, and may be archived before them.

## Verification plan

- **Specification:** `openspec validate specify-desktop-local-api-server --strict --no-interactive`.
- **Gate unit tests:**
  - `Host` matrix: both allowed forms, case variants, wrong port, a rebinding name, an IP literal, missing or duplicate headers, absolute-form targets.
  - `Origin` matrix: `null`, `https://…`, `tauri://localhost`, loopback origins, and preflight.
  - Ordering: no body read and no bearer check before rejection.
  - No CORS headers on any response.
  - Method and path matrix over all six routes, including 405 on the request routes.
- **Authentication and reveal tests:**
  - Missing, wrong, query-string, cookie and wrong-scheme bearers.
  - Constant-time comparison helper.
  - Regenerate invalidates the old bearer; an in-flight request survives.
  - Insecure or corrupt files fail closed without being modified.
  - Reveal returns the value only to an explicit request, and it is cleared on Hide, page exit, Regenerate, logout and profile change.
  - Canary scans for the bearer and route credentials across every other native command result, webview storage, the request log, diagnostics, process logs and upstream requests.
  - Process-table sampling while each displayed snippet runs shows no argv containing the bearer.
- **Bounds tests** (controlled tokio time):
  - Connections: a header block still incomplete after 5 seconds, a 16 KiB + 1 header block, a 65th connection, and a 30-second idle keep-alive.
  - Requests: a 256 KiB body, a slow body, refusal of the 17th concurrent request.
  - Responses: 8 MiB, the 120-second idle and 3,600-second total limits, with upstream cancellation.
- **Route-credential tests:**
  - A route without a cap, with a null-cap credential, without `read` scope, bound to the account's active credential, or already bound to another route cannot be saved or enabled.
  - Issuing a route credential leaves the account's active credential unchanged.
  - An expired route credential gives 409 `no_credential` with zero dispatches, even when an active credential exists.
  - The page shows the exact cap in CFX.
- **Paid-route fault injection against a controlled loopback provider:**
  - Request-key persistence failure leads to zero dispatches.
  - Cut stream, timeout, oversized stream and client disconnect each lead to exactly one provider request, an error event without `[DONE]`, and an identifier header.
  - A reused client key passes the 409 through and keeps the original record.
  - The same key on two accounts keeps two records.
  - A provider 302 with `Location` gives the client 502 `provider_error`, no `Location`, and no connection to the target.
  - No rerouting to another account or provider.
  - Classified errors carry no provider text.
  - After restart with the server disabled, Usage & API keys, Requests shows the key and reads its status.
  - A route to a free generation offer with no serving profile reads *Unqualified — no speed guarantee* and still needs its dedicated, positively capped credential. A route to a priced generation offer with no serving profile reads the same, and a route to an embedding-only offer with no serving profile states that no generation-speed guarantee applies.
- **Request-route tests against a controlled provider:**
  - Status by request ID and by request key returns the decoded object with the route's credential.
  - Unknown IDs, local-route IDs, IDs of desktop requests made outside the server, IDs of removed or retargeted routes, and IDs under another network identity give 404 with zero provider contact.
  - No profile gives 503.
  - A key on two accounts gives 409 `ambiguous_request_id`.
  - Cancel gives exactly one provider cancellation, the 202 passes through, and the log label says possibly charged and not a refund.
- **Local-route tests** (with M1):
  - Deny-all outbound and no profile.
  - An unloaded model returns 503 with no load.
  - A different revision loaded is refused.
  - Two loaded models each reach only their own engine child.
  - Engine-only paths are unreachable.
  - Slots are shared with local chat.
  - No accounting records are created.
  - With chat history on, a canary request through the server creates no chat thread and leaves no canary under `<app data>/chats/`.
  - With M4: a shared model's route, for a qualified share and for a free share with no serving profile, is admitted through the daemon, refused with 429 and the reason when saturated, refused with 503 `share_unavailable` when draining or not ready, and never reaches an engine directly. While its installation is under a qualification run, it is refused with 503 `share_unavailable` and the being-qualified message, without dispatch.
- **Session tests:** logout and profile changes cancel network-routed requests, and local routes continue. A route for another network identity returns `route_unavailable` without contacting any provider.
- **Interoperability:**
  - Stock OpenAI Python and Node SDKs, with retries disabled, against local and network routes (streaming and non-streaming chat, completions, embeddings).
  - A real browser test: a page on another origin, and a page reached through a DNS-rebinding hostname, cannot use or read the server.
- **Isolation (release gate 7):**
  - Listener inspection with `lsof` on macOS, `ss` on Linux and `Get-NetTCPConnection` on Windows shows only `127.0.0.1:<port>`. A LAN connection attempt fails.
  - DHT, manifest, identify and local-network traffic captures contain neither the server's address nor its port.
- **Project checks:** `npm run check`, `npm test`, `npm run build`, `cargo test --manifest-path src-tauri/Cargo.toml`, and a native desktop smoke test on each OS, including the `#/local-server` route and its `aria-current`. A browser preview is not native verification (A.6).
- **After M3:**
  - A peer-ID mismatch gives 502 `provider_authentication_failed`, logged as not sent, with no fallback.
  - A circuit-budget refusal, and no authenticated path within the ladder deadline, each give 503 `provider_unreachable` with the request key, logged as not sent, with no dispatch by another path.
  - A libp2p-only account from a profile with no libp2p node gives 503 `transport_unavailable`.
  - Relayed and hole-punched paths are displayed.
  - A cut relay circuit is recovered through request status, including by request key through `GET /v1/requests/:id`.
  - A free-account route refused with 402 `free_allowance_exhausted` or `free_allowance_expired` gives the client 402 with that code, the native consumer's fixed message and the request key, without provider text. The log entry records the status, the code, the account and the request key as a provider refusal. The provider receives exactly one request, nothing requests a quote, opening or funding, and the route still names the same account.
  - A provider answering 503 `qualification_in_progress` with `Retry-After: 120` gives the client 503 with that code, the native consumer's fixed message and the request key, and `Retry-After: 120`; a value of 0, above 3600, an HTTP date or a repeated header is dropped; no other refusal carries `Retry-After`. The request is not retried and is logged as a provider refusal.
