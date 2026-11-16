## ADDED Requirements

### Requirement: The local API server is optional, off by default and bound to loopback only

The native desktop SHALL provide an optional local OpenAI-compatible API server for programs running on the same machine. On a fresh installation, and whenever no enablement has been recorded, the server SHALL be disabled and SHALL NOT open any listening socket. It SHALL start only after the user enables it in the native desktop. The user's enabled/disabled choice and port SHALL persist under `<app data>/local-server/`; at a later launch, a server the user left enabled SHALL start again and the desktop SHALL show that it is running.

The server SHALL bind exactly one TCP listener, on the IPv4 literal loopback address `127.0.0.1`, at the user-chosen port, which SHALL be an integer from 1,024 to 65,535. It SHALL NOT bind `0.0.0.0`, `::`, `::1`, or a LAN, VPN or other interface address, and no setting, environment variable or command-line option SHALL offer a non-loopback bind. It SHALL close any accepted connection whose peer address is not a loopback address. If the chosen port is occupied, or equals a port used by a listener the desktop itself manages (a managed engine, a provider daemon's public or administration listener, or a DHT listener), the server SHALL NOT start, SHALL report the conflict, and SHALL NOT choose another port or adopt the existing listener.

#### Scenario: A fresh installation opens no listener

- **GIVEN** a fresh desktop installation with no `<app data>/local-server/` settings
- **WHEN** the desktop starts and runs
- **THEN** no local-server listening socket exists
- **AND** the Local server page shows the server as disabled

#### Scenario: An enabled server listens on loopback only

- **GIVEN** the user enables the server on port 18080
- **WHEN** the machine's listening sockets are inspected
- **THEN** the only local-server socket is `127.0.0.1:18080`
- **AND** a connection attempt to port 18080 on the machine's LAN address does not reach the server

#### Scenario: An occupied port is an error, not a fallback

- **GIVEN** another process already listens on the port the user chose
- **WHEN** the user enables the server
- **THEN** the server does not start and the desktop reports that the port is in use
- **AND** no listener is opened on any other port

### Requirement: The local API server runs only while the desktop process runs

The local API server SHALL be part of the native desktop process. Closing or minimizing the main window SHALL NOT stop it. Disabling the server SHALL close its listener and cancel its in-flight requests; when network-routed requests are in flight, the desktop SHALL first state that cancelled requests can still be charged for generated tokens. A confirmed Quit, and any other end of the desktop process, SHALL stop the server and release its port. The desktop SHALL NOT install a background service, launch agent, scheduled task or start-at-login entry for the server, and no part of the server SHALL keep running after the desktop process ends.

#### Scenario: Closing the window keeps the server running

- **GIVEN** the server is enabled and a local tool is streaming a response through it
- **WHEN** the user closes the main window
- **THEN** the window is minimized and the stream continues
- **AND** new requests are still accepted

#### Scenario: Quit releases the port

- **GIVEN** the server is enabled
- **WHEN** the user confirms Quit
- **THEN** the desktop process ends and nothing listens on the server's port
- **AND** no background process or login item for the server remains

#### Scenario: Disabling with paid requests in flight

- **GIVEN** a network-routed request is in flight
- **WHEN** the user disables the server
- **THEN** the desktop states that cancelled requests can still be charged before it proceeds
- **AND** after the user proceeds the listener is closed and the in-flight request is cancelled

### Requirement: Local API requests are authenticated by a protected, locally generated bearer

When the server is first enabled and no bearer file exists, the native desktop SHALL generate a bearer from the operating system's cryptographically secure random source with at least 256 bits of entropy, encoded as printable URL-safe ASCII, and SHALL store it as a protected file under `<app data>/local-server/`: on Unix not group- or world-accessible, on Windows owner-protected. Every request SHALL present `Authorization: Bearer <token>`. The server SHALL compare the token in constant time and SHALL reject a missing, malformed or non-matching bearer with HTTP 401 and error code `invalid_api_key`, without dispatching anything. It SHALL NOT accept the bearer from a query string, cookie, request body or any other header.

If the bearer file exists but is unreadable, malformed or insecurely permissioned, the server SHALL NOT start and SHALL report the problem; it SHALL NOT widen or repair permissions or silently replace the file. A user-initiated **Regenerate** SHALL atomically replace the file with a newly generated bearer written with protected permissions; from then on the previous bearer SHALL be rejected for new requests, while requests already admitted MAY complete.

The bearer SHALL be distinct from, and SHALL NOT be accepted as or used as, a wallet key, provider account credential, provider administrator token, engine API key, RPC bearer or Hugging Face token. It SHALL NOT be forwarded to any provider, engine or other process. It SHALL NOT appear in URLs, command-line arguments, process logs, diagnostics, the request log or error messages. The bearer value SHALL cross the native boundary to the webview only as the result of an explicit **Reveal** action on the Local server page (Appendix A.1 rule 1). The revealed value SHALL be shown in a masked field. It SHALL NOT be written to webview storage, URLs or logs, and SHALL NOT be embedded in a snippet. It SHALL be cleared from webview state on **Hide**, on leaving the page, on **Regenerate**, on logout and on a profile change. No other native command result SHALL contain it.

#### Scenario: Missing or wrong bearer is refused before dispatch

- **GIVEN** the server is enabled with a route to a loaded local model
- **WHEN** a request arrives with no `Authorization` header, a wrong bearer, or the correct bearer only in a query parameter
- **THEN** the server responds 401 `invalid_api_key`
- **AND** nothing is dispatched to an engine or any provider

#### Scenario: Regenerate invalidates the previous bearer

- **GIVEN** a tool authenticated with the current bearer
- **WHEN** the user regenerates the bearer and the tool sends a new request with the old bearer
- **THEN** the request is refused with 401
- **AND** a request with the new bearer read from the protected file succeeds

#### Scenario: An insecure bearer file fails closed

- **GIVEN** the bearer file is readable by other users of the machine
- **WHEN** the server is enabled or the desktop launches with the server enabled
- **THEN** the server does not start and the desktop reports the insecure file
- **AND** the file's permissions and contents are left unchanged

#### Scenario: The bearer stays native except for an explicit reveal

- **GIVEN** a known bearer value
- **WHEN** requests are served on every route kind and every native command result returned to the webview other than a Reveal result, the webview's storage, the request log, diagnostics, process logs and all upstream requests are searched for it
- **THEN** the value appears in none of them

#### Scenario: Reveal is explicit and cleared on hide

- **GIVEN** the Local server page is open with the bearer masked and no Reveal has been requested
- **WHEN** the user chooses Reveal
- **THEN** the native Reveal result carries the bearer, which is shown in a masked field and written to no webview storage, URL or log
- **AND** after Hide, leaving the page, Regenerate, logout or a profile change, the value is no longer held in webview state

### Requirement: Browser-origin and unexpected-host requests are refused

Before reading a request body or evaluating the bearer, the server SHALL validate the request's host and origin. The server SHALL accept only a request that carries exactly one `Host` header whose value equals `127.0.0.1:<port>` or `localhost:<port>` for the bound port, with the host name compared ASCII-case-insensitively; a missing `Host`, multiple `Host` headers, any other name, address or port, or a request target in absolute form SHALL be refused with HTTP 421 and error code `host_not_allowed`. Any request carrying an `Origin` header, whatever its value (including `null`, loopback origins and the desktop webview's own origin), SHALL be refused with HTTP 403 and error code `origin_not_allowed`. The server SHALL NOT emit `Access-Control-Allow-*` or other CORS response headers, SHALL NOT answer a CORS preflight successfully, and SHALL NOT treat cookies as authentication. The desktop's own webview SHALL NOT call the local server; the GUI uses native commands.

#### Scenario: DNS rebinding is refused

- **GIVEN** a hostname controlled by a web page that resolves to `127.0.0.1`
- **WHEN** a request with the valid bearer arrives with `Host: rebind.example:<port>`
- **THEN** the server responds 421 `host_not_allowed`
- **AND** the body is not read and nothing is dispatched

#### Scenario: A web page cannot use the server

- **WHEN** a request arrives carrying `Origin: https://evil.example`, `Origin: null` or `Origin: tauri://localhost`, with or without a valid bearer
- **THEN** the server responds 403 `origin_not_allowed` and nothing is dispatched
- **AND** the response carries no CORS headers

#### Scenario: Preflight is not granted

- **WHEN** an `OPTIONS` request with `Origin` and `Access-Control-Request-Headers: authorization` arrives
- **THEN** the server responds 403 without any `Access-Control-Allow-Origin` header

#### Scenario: Ordinary local tools are accepted

- **WHEN** a stock OpenAI SDK configured with base URL `http://127.0.0.1:<port>/v1` or `http://localhost:<port>/v1` sends a request without an `Origin` header and with the valid bearer
- **THEN** the host and origin checks pass and the request proceeds to routing

### Requirement: The local API server exposes only a bounded OpenAI-compatible endpoint subset

After the host, origin and bearer checks, the server SHALL serve exactly these routes:

- `GET /v1/models`;
- `POST /v1/chat/completions`;
- `POST /v1/completions`;
- `POST /v1/embeddings`;
- for network routes only, `GET /v1/requests/:id` and `POST /v1/requests/:id/cancel`, as the requirement *Paid requests remain recoverable through request status* defines.

Any other path SHALL return 404 `not_found`, and a listed path with another method SHALL return 405 `method_not_allowed`. An inference `POST` SHALL carry `Content-Type: application/json` and a JSON object body; otherwise the server SHALL respond 415 or 400. `GET /v1/models` and both request routes SHALL have an empty body; otherwise the server SHALL respond 400.

The server SHALL NOT mount any of these:

- provider control, contract, authentication, administration, health or readiness routes;
- engine routes: tokenization, detokenization, properties, slots, metrics, health, or model loading and unloading;
- a generic proxy path.

Errors SHALL use the provider gateway's error object `{"error": {"message", "type", "param", "code"}}`. The server SHALL pass generated text and tool calls to the client as data, and SHALL NOT execute tool calls or interpret output.

An inference request SHALL name an enabled route name in `model`. Each of the following SHALL be refused without dispatch:

- a missing `model`, with 400 `model_required`;
- an unknown or disabled name, with 404 `model_not_found`;
- an endpoint the route's model does not advertise as a capability, with 400 `capability_not_supported`.

`GET /v1/models` SHALL return `{"object": "list", "data": [...]}` containing exactly the enabled route names. Each entry SHALL be `{"id", "object": "model", "created", "owned_by"}`, with `owned_by` equal to `local` or `network`. The list SHALL be answered from local state without contacting any provider, chain, catalog or Hugging Face, and SHALL contain no credentials or other secrets.

#### Scenario: Engine and control paths are not reachable

- **WHEN** an authenticated request targets `/tokenize`, `/slots`, `/metrics`, `/admin/status`, `/v1/contracts/propose` or `/v1/contracts/abc/credentials`
- **THEN** the server responds 404 `not_found`
- **AND** nothing is forwarded to an engine or any provider

#### Scenario: Request routes accept only their methods

- **WHEN** an authenticated client sends `POST /v1/requests/abc` or `GET /v1/requests/abc/cancel`
- **THEN** the server responds 405 `method_not_allowed`
- **AND** no provider is contacted

#### Scenario: The model list is local

- **GIVEN** one enabled local route and one enabled network route
- **WHEN** an authenticated client calls `GET /v1/models` with outbound network access blocked
- **THEN** the response lists exactly the two route names with `owned_by` `local` and `network`
- **AND** no outbound connection is attempted

#### Scenario: Unsupported capability is refused before dispatch

- **GIVEN** a route whose model advertises only `embeddings`
- **WHEN** a client posts to `/v1/chat/completions` naming that route
- **THEN** the server responds 400 `capability_not_supported` and nothing is dispatched

### Requirement: Local API requests have fixed size, concurrency and duration bounds

The server SHALL apply these bounds to every connection and every route.

Connection bounds:

- The request line and headers SHALL be fully received within 5 seconds, or the connection SHALL be closed without a response.
- A header block larger than 16 KiB SHALL be refused with HTTP 431.
- At most 64 connections SHALL be open at once. Further connections SHALL be accepted and closed immediately, without reading from them.
- A keep-alive connection that is idle for 30 seconds between requests SHALL be closed.

These bounds SHALL be applied before the host, origin and bearer checks.

Request bounds:

- A request body larger than 262,144 bytes SHALL be refused with 413 `request_too_large` before routing or dispatch.
- A body not fully received within 5 seconds SHALL be refused with 408 `request_timeout` and not dispatched.
- At most 16 requests SHALL be in progress at once, counted from header acceptance to response completion. A further request SHALL receive 429 `local_server_busy` immediately and SHALL NOT be queued.

Response bounds:

- A non-streaming response body SHALL NOT exceed 8 MiB.
- A streamed response SHALL NOT exceed 8 MiB cumulatively, SHALL NOT be idle for more than 120 seconds between upstream events, and SHALL NOT last more than 3,600 seconds in total.
- A network dispatch SHALL begin streaming within 120 seconds.

Exceeding a response bound SHALL terminate the client response with an error: an HTTP error for a non-streaming response, or an error event for a stream. It SHALL also cancel the upstream operation.

#### Scenario: Slow headers are cut off

- **WHEN** a client opens a connection and sends a partial request line and headers that are still incomplete 5 seconds later
- **THEN** the server closes the connection
- **AND** the connection never counted as a request in progress, and nothing was dispatched

#### Scenario: Oversized headers are refused

- **WHEN** a client sends a request whose header block exceeds 16 KiB
- **THEN** the server responds 431 and nothing is dispatched

#### Scenario: A connection flood is bounded

- **GIVEN** 64 connections to the server are open and idle
- **WHEN** a 65th connection is opened
- **THEN** it is closed immediately without being read
- **AND** after the idle connections are closed by the 30-second keep-alive bound, a new authenticated request is served

#### Scenario: Oversized bodies are never dispatched

- **WHEN** an authenticated client posts a 262,145-byte body
- **THEN** the server responds 413 `request_too_large`
- **AND** nothing is dispatched

#### Scenario: Excess concurrency is refused, not queued

- **GIVEN** 16 requests are in progress
- **WHEN** a 17th authenticated request arrives
- **THEN** it receives 429 `local_server_busy` immediately
- **AND** it is not dispatched later

#### Scenario: A stream that exceeds its duration ends with an error

- **GIVEN** an upstream stream that keeps emitting events beyond 3,600 seconds
- **WHEN** the total duration bound is reached
- **THEN** the client stream ends with an error event and no success marker
- **AND** the upstream operation is cancelled

### Requirement: Each route name maps to exactly one local model or one funded network account

The server SHALL route requests by a user-managed routing table persisted under `<app data>/local-server/`. The table SHALL hold at most 64 entries and SHALL contain no secrets. Each entry SHALL contain:

- a route ID that the desktop assigns at creation and never reuses;
- a route name of 1–256 bytes of UTF-8 without control characters, unique by exact bytes;
- an enabled flag;
- exactly one target.

The target SHALL be one of:

- **Local:** the identity of one installed model, as `specify-desktop-local-models` defines it (`desktop-model-acquisition`: repository, full commit and selected artifact).
- **Network:** a complete `NetworkIdentity`, the account ID of an account in the native consumer's account store, and the ID of exactly one credential issued for that route.

The desktop SHALL issue a network route's credential for that route from the route editor, through the native consumer's existing owner-authenticated `credential_issue` operation. The credential SHALL have:

- scopes exactly `infer` and `read`;
- a user-chosen, non-null, positive `spend_cap_wei` as a canonical decimal wei string;
- an `expires_at` within the existing credential lifetime bounds.

Issuing a route credential SHALL NOT make it the account's active credential. The route SHALL record the credential's ID together with the scopes, spend cap and expiry it was issued with.

The desktop SHALL refuse to save or enable a network route whose credential:

- lacks either scope;
- has a null or zero spend cap;
- is expired, revoked or no longer stored locally;
- is the account's active credential;
- is bound to another route.

Removing a network route, or replacing its credential, SHALL NOT revoke the old credential silently. The desktop SHALL offer to revoke it through the existing `credential_revoke` operation.

Entries SHALL be created, changed, enabled and removed only by explicit user action in the native desktop. The server SHALL NOT add routes automatically, and SHALL NOT accept a provider URL, offer, bearer or credential secret as a target. A route SHALL NOT resolve to a different model revision, account, provider or credential than the one recorded. A newer download of the same repository, another account for the same model ID or another provider SHALL require a new or edited route.

Adding or enabling a network route SHALL require the user to accept a disclosure. It SHALL state:

- that any program able to read the local bearer can spend up to the route credential's spend cap, shown exactly in CFX, from that account's prepaid, non-refundable balance until the credential expires;
- that the cap limits authority on the same balance and is neither a separate balance nor a refund guarantee;
- that the provider sees prompts and responses in plaintext;
- that a client retry without a reused `Idempotency-Key` is a new paid request.

#### Scenario: Route names are unique

- **GIVEN** a route named `assistant`
- **WHEN** the user adds another route named `assistant`
- **THEN** the desktop refuses it and the existing route is unchanged

#### Scenario: A newer model revision does not change an existing route

- **GIVEN** a local route bound to repository `R` at commit `C1`
- **WHEN** the user downloads `R` at commit `C2` and loads it
- **THEN** requests naming the route are not served by `C2`
- **AND** they are refused as not loaded until `C1` is loaded or the user edits the route

#### Scenario: A network route requires the spending disclosure

- **WHEN** the user adds a network route for a funded account without accepting the disclosure
- **THEN** the route is not created or enabled
- **AND** no request can reach that account through the server

#### Scenario: A route without a spend cap cannot be enabled

- **GIVEN** a funded account
- **WHEN** the user tries to save or enable a network route with no spend cap, or with a credential that has a null cap, lacks `read` scope or is the account's active credential
- **THEN** the desktop refuses the route and states the reason
- **AND** no request can reach that account through the server

#### Scenario: A route credential is dedicated

- **GIVEN** an account with no active credential
- **WHEN** the user creates a network route and the desktop issues its credential
- **THEN** the account still has no active credential
- **AND** the desktop's own network chats do not use the route's credential

### Requirement: Local routes are free, private and never touch the network

A local route SHALL dispatch only in one of these two ways:

- (a) to the managed engine child that has the route's exact installation loaded, as `specify-desktop-local-models` defines it (`desktop-local-runtime`);
- (b) while that installation backs an active share as `specify-desktop-share-online` defines it, through that change's admission for local use of a shared model (`desktop-share-online`, *Local use of a shared model passes through admission*), and never directly to any engine.

Otherwise the server SHALL respond 503 `model_not_loaded` without dispatch. The server SHALL NOT download, load, eject or reload a model, or change its load settings, in response to a request.

While the route's installation is under a qualification run as `specify-desktop-share-online` defines it, the server SHALL respond 503 `share_unavailable` with a fixed message stating that the model is being qualified, without dispatch. This refusal SHALL take precedence over (a), (b) and `model_not_loaded`, and the request SHALL NOT be queued.

Local routes SHALL perform no accounting or reservation. They SHALL NOT contact a wallet, chain, catalog, provider, policy endpoint or Hugging Face. They SHALL work with no consumer profile loaded and without network connectivity, and SHALL NOT send prompts or outputs off the machine.

The server SHALL validate a local request against the same bounded request schema the provider gateway applies to that endpoint. It SHALL reject unsupported fields or options with 400 rather than forward them. It SHALL bound output by the loaded context and settings. Under (a), it SHALL authenticate to the engine child with that load's engine API key, read natively. It SHALL NOT expose engine routes or return an engine key.

Concurrency limits:

- Under (a), local-server requests SHALL share that engine child's configured concurrency with local chat. When no slot is free, the server SHALL respond 429 `rate_limit` and SHALL NOT raise the loaded concurrency.
- Under (b), when admission refuses a request because every slot is held, or because the share's arrival budget is exhausted where the share has one, the server SHALL respond 429 `rate_limit`.
- Under (b), when admission refuses a request because it is draining, is paused for qualification, or the model is not ready, the server SHALL respond 503 `share_unavailable`.
- Each refusal under (b) SHALL carry the admission's refusal reason as a fixed classified message, and the request SHALL NOT be queued.

Rule (b) SHALL apply to every active share of the installation, including a free share published without qualification and with no serving profile. The server SHALL NOT dispatch a local route directly to an engine, or skip admission, because the share is unqualified or has no arrival budget.

A supplied `Idempotency-Key` SHALL NOT deduplicate local requests. A local response SHALL carry a locally generated `x-request-id`, for correlation only. Local-route responses SHALL NOT be presented as qualified performance or used as qualification evidence, and a local route SHALL NOT carry or display a speed guarantee, whether its installation is unshared, shared with a serving profile, or shared as a free unqualified share.

#### Scenario: Local use works offline without a profile

- **GIVEN** no consumer profile is loaded, outbound network access is blocked and the route's model is loaded
- **WHEN** an authenticated client posts a chat completion naming the local route
- **THEN** the managed engine child for that installation generates the response
- **AND** no outbound connection is attempted and no accounting record is created

#### Scenario: An unloaded model is not loaded on demand

- **GIVEN** the route's model is installed but neither loaded nor shared
- **WHEN** an authenticated client posts a request naming the route
- **THEN** the server responds 503 `model_not_loaded`
- **AND** the engine state, loaded models and load settings are unchanged

#### Scenario: Local requests cannot exceed loaded concurrency

- **GIVEN** the model is loaded with concurrency 1 and a local chat is generating
- **WHEN** an authenticated client posts a request naming the local route
- **THEN** the server responds 429 `rate_limit` and the loaded concurrency stays 1

#### Scenario: A shared model's local route is admitted by the daemon

- **GIVEN** `specify-desktop-share-online` is implemented and the route's installation backs an active share
- **WHEN** an authenticated client posts a request naming the route while a slot is free
- **THEN** the request is dispatched through the share's admission, not directly to any engine, and no prepaid account, fee obligation or reputation event changes
- **AND** when every slot is held by paid requests, a further request is refused with 429 `rate_limit` and the saturation reason, and is not queued

#### Scenario: A free unqualified share's local route still passes through admission

- **GIVEN** `specify-desktop-share-online` is implemented and the route's installation backs an active free share published with no serving profile
- **WHEN** an authenticated client posts a request naming the route while a slot is free
- **THEN** the request is dispatched through the share's admission, not directly to any engine, and no prepaid account, fee obligation or reputation event changes
- **AND** when every slot is held, a further request is refused with 429 `rate_limit` and the saturation reason, is not queued, and no response or routing-table entry claims a speed guarantee

#### Scenario: A local route is refused while its model is being qualified

- **GIVEN** `specify-desktop-share-online` is implemented and a qualification run of the route's installation is in progress
- **WHEN** an authenticated client posts a request naming the route
- **THEN** the server responds 503 `share_unavailable` with a fixed message stating that the model is being qualified, not 503 `model_not_loaded`
- **AND** the request is not dispatched to any engine and is not queued

### Requirement: Network routes forward through the native consumer path with the credential held natively

A network route SHALL be dispatchable only while a consumer profile whose complete `NetworkIdentity` equals the route's is loaded. Otherwise the server SHALL respond 503 `route_unavailable` without contacting any provider. Each of the following SHALL be refused without dispatch:

- a routed account that is not `active` or `free`, with 409 `account_not_spendable`;
- an endpoint whose capability the account's agreed model does not advertise, with 400 `capability_not_supported`.

The server SHALL dispatch through the same native consumer inference path the desktop uses for its own network chats. That path provides:

- client-version enforcement and the `X-Chiral-Client-Version` header;
- the request's `model` replaced by the account's agreed model ID, so a client cannot select another model;
- usage included in streams;
- the outbound HTTPS origin boundary: a validated public origin, a DNS-pinned client, no environment proxies and no redirects.

The server SHALL authenticate only with the route's recorded credential. It SHALL NOT fall back to the account's active credential or any other credential, and SHALL NOT rotate or reissue it. A route credential that is missing from local storage, expired or locally revoked SHALL be refused with 409 `no_credential` without dispatch, directing the user to edit the route on the Local server page. A provider refusal of the credential SHALL be returned as the native consumer's classified `invalid_credential`, `credential_spend_cap` or `account_forbidden` error.

In handling any request, the server SHALL NOT issue, rotate, revoke or export credentials, sign owner challenges, request quotes, fund, top up, or resume or cancel funding. The provider credential SHALL be sent only to that account's provider. It SHALL NOT be returned to the local client or written to any log. The client's `Authorization` header and other client headers SHALL NOT be forwarded, except a validated `Idempotency-Key`. Responses to the client SHALL carry only:

- the content type;
- the provider's request ID, when it is a well-formed opaque identifier;
- the request-key header;
- `Retry-After`, only on a 503 `qualification_in_progress` response and only as the *Qualification pause* rule below allows.

A provider 4xx or 5xx error SHALL be converted to the same classified error code and fixed message that the native consumer uses. The converted error SHALL carry the provider's HTTP status and the validated request identifiers, and SHALL NOT include provider-supplied free text.

The following provider refusals SHALL be mapped exactly:

- **Free allowance.** Once `specify-p2p-provider-transport` is implemented, a provider serves a free account only within its signed free allowance (`inference-commercial-terms`, *Free accounts are served only within their signed allowance*). When it refuses a request with HTTP 402 and error code `free_allowance_exhausted` or `free_allowance_expired`, the server SHALL return HTTP 402 with that same code and the native consumer's fixed message for it. That message states which of the two applies and that further service needs a new account.
- **Qualification pause.** When a provider answers HTTP 503 with error code `qualification_in_progress`, the qualification-pause response that `desktop-share-online` defines, the server SHALL return HTTP 503 with that same code and the native consumer's fixed message for it, which `specify-p2p-provider-transport` classifies as retryable and not admitted and words as "Provider is qualifying — try again later". The refusal admits and charges nothing. When the provider's response carries exactly one `Retry-After` header whose value is an integer number of seconds from 1 to 3600, written as ASCII digits without sign, leading zeros or other characters, the server SHALL pass that value unchanged in `Retry-After`. Any other `Retry-After` value, including an HTTP date, zero, a value above 3600 or more than one header, SHALL be dropped, and the response SHALL be returned without `Retry-After`. No other response, including the other refusals here and every other 4xx and 5xx, SHALL carry `Retry-After`. Passing the header SHALL NOT make the server retry: a later attempt is the client's own new request.

For each of these refusals, the response SHALL carry `x-chiral-request-key`. The request log entry SHALL record the HTTP status, the code as its classified outcome, the account ID and the request key. That outcome SHALL identify the request as refused by the provider, distinct from a request that was not sent, a transport failure or an interrupted stream. The persisted request-key record SHALL be kept. The server SHALL NOT retry the request on any path. It SHALL NOT request a quote, open, fund or top up an account, and SHALL NOT retarget the route because of the refusal. The route SHALL stay bound to its account, so later requests naming it reach the same account until the user edits the route.

A provider 3xx response SHALL NOT be followed. It SHALL NOT be returned with its 3xx status or with any `Location` header. Instead the server SHALL return 502 with code `provider_error`, the native consumer's fixed message and the validated request identifiers, on both HTTPS and libp2p routes.

#### Scenario: A client cannot redirect a route to another model

- **GIVEN** a network route whose account was agreed for model `M`
- **WHEN** a client posts a request naming the route
- **THEN** the provider receives `model` equal to `M`
- **AND** no field of the client request changes the provider, account, credential or model

#### Scenario: The provider credential stays native

- **GIVEN** a route credential with a known value
- **WHEN** a client completes, fails and interrupts requests through the route
- **THEN** the value appears in no response to the client, request log entry, diagnostic or process log
- **AND** the local bearer is never sent to the provider

#### Scenario: An expired route credential does not fall back to the active credential

- **GIVEN** a network route whose credential has expired, on an account that also has a valid active credential
- **WHEN** a client posts a request naming the route
- **THEN** the server responds 409 `no_credential`
- **AND** the provider receives no request, and the active credential is not used

#### Scenario: Provider error text is not echoed

- **GIVEN** a provider that returns 402 with error code `insufficient_balance` and a message containing arbitrary text
- **WHEN** a client's request through the route receives that error
- **THEN** the client receives 402 with code `insufficient_balance` and the native consumer's fixed message
- **AND** the provider's message text is not included

#### Scenario: A used-up free allowance is reported, not retried

- **GIVEN** `specify-p2p-provider-transport` is implemented and a network route whose account is a free account with 300 tokens of its signed allowance remaining
- **WHEN** a client posts a chat completion whose worst case exceeds 300 tokens, and the provider refuses it with HTTP 402, error code `free_allowance_exhausted` and a message containing arbitrary text
- **THEN** the client receives 402 with code `free_allowance_exhausted`, the native consumer's fixed message and `x-chiral-request-key`, without the provider's text
- **AND** the request log entry records status 402, outcome `free_allowance_exhausted`, the account ID and the request key, as refused by the provider
- **AND** the provider receives exactly one request, no quote, opening or funding is requested, and the route still names the same account

#### Scenario: An expired free allowance

- **GIVEN** `specify-p2p-provider-transport` is implemented and a network route whose account is a free account whose signed allowance lifetime has ended
- **WHEN** a client posts a request and the provider refuses it with HTTP 402 and error code `free_allowance_expired`
- **THEN** the client receives 402 with code `free_allowance_expired` and the native consumer's fixed message, and the request log entry records that outcome
- **AND** a later request naming the route is forwarded to the same account and is not redirected to any other account or provider

#### Scenario: A provider paused for qualification

- **GIVEN** a network route whose provider is in the qualification pause that `desktop-share-online` defines
- **WHEN** a client posts a request and the provider answers HTTP 503 with error code `qualification_in_progress`, `Retry-After: 120` and a message containing arbitrary text
- **THEN** the client receives 503 with code `qualification_in_progress`, the native consumer's fixed message "Provider is qualifying — try again later", `x-chiral-request-key` and `Retry-After: 120`, without the provider's text
- **AND** the provider receives exactly one request, the server does not retry it after 120 seconds or at any other time, and the request log records it as refused by the provider

#### Scenario: An out-of-range Retry-After is dropped

- **GIVEN** a network route whose provider is in the qualification pause that `desktop-share-online` defines
- **WHEN** the provider answers HTTP 503 `qualification_in_progress` with `Retry-After` equal to `0`, `7200` or an HTTP date
- **THEN** the client receives 503 with code `qualification_in_progress` and no `Retry-After` header

#### Scenario: Retry-After is not passed on other refusals

- **GIVEN** a network route to a provider
- **WHEN** the provider answers HTTP 402 `free_allowance_exhausted`, HTTP 429 `rate_limit` or HTTP 503 with any code other than `qualification_in_progress`, each with `Retry-After: 30`
- **THEN** the client receives the mapped error with no `Retry-After` header

#### Scenario: A provider redirect is not passed to the SDK

- **GIVEN** a controlled provider that answers an inference request with 302 and `Location: http://127.0.0.1:<other port>/v1/chat/completions`
- **WHEN** a client posts a request through the route
- **THEN** the client receives 502 `provider_error` with no `Location` header
- **AND** no connection is made to the redirect target, and the provider received exactly one request

#### Scenario: No profile means no network dispatch

- **GIVEN** no consumer profile is loaded
- **WHEN** a client posts a request naming a network route
- **THEN** the server responds 503 `route_unavailable` and no provider is contacted

### Requirement: Paid routes keep request identity and never retry automatically

For every network-routed inference request, the server SHALL use exactly one request key. The key SHALL be the client's `Idempotency-Key` when it is present and valid (1–128 ASCII letters, digits, `.`, `_`, `:` or `-`, in at most one header). Otherwise the server SHALL generate a new UUID. An invalid key, or multiple `Idempotency-Key` headers, SHALL be refused with 400 `invalid_idempotency_key` without dispatch.

Before dispatch, the server SHALL durably persist the account ID, route ID, request key and creation time in that account's protected local request-key store, and no prompt or output. Records SHALL be keyed by account and request key, so that the same key on another account does not overwrite them. When response headers arrive, the server SHALL add the provider's request ID to the record. A record SHALL NOT be overwritten by a later request with the same account and key, except to add a request ID it lacks. If persisting fails before dispatch, the server SHALL NOT dispatch and SHALL return an error.

Every response produced after the request key is chosen, including error responses and streams, SHALL carry the request key in `x-chiral-request-key`. When the provider returned a request ID, the response SHALL also carry it in `x-request-id`.

Once any part of a request may have reached the provider, the server SHALL NOT send it again. This holds after an error, timeout, cancellation, interrupted stream or client disconnect, whether with the same key, a new key, another transport, another provider or another account. The server SHALL return a provider `duplicate_request` (HTTP 409) to the client with the original request ID and status, and SHALL NOT replay generation.

An interrupted, timed-out or oversized upstream stream SHALL end the client stream with an error event that carries a classified code and the request identifiers, and SHALL NOT emit a success marker such as `[DONE]`. A client disconnect SHALL cancel the upstream operation, and the server SHALL record it as possibly charged. The server SHALL NOT retain response content for replay.

#### Scenario: An interrupted stream is not retried

- **GIVEN** a controlled provider that cuts the stream after the first event
- **WHEN** a client streams a chat completion through a network route
- **THEN** the provider receives exactly one request
- **AND** the client stream ends with an error event carrying `x-chiral-request-key` and the provider request ID, without `[DONE]`

#### Scenario: Request identity is persisted before dispatch

- **GIVEN** the account's local request-key store cannot be written
- **WHEN** a client posts a request through a network route
- **THEN** the server returns an error and the provider receives no request

#### Scenario: A reused client key does not create another paid request

- **GIVEN** a client request with `Idempotency-Key: job-42` was admitted by the provider
- **WHEN** the client resends a request with `Idempotency-Key: job-42` through the same route
- **THEN** the client receives 409 `duplicate_request` with the original request ID and status
- **AND** the provider creates no new hold or execution, and the persisted record keeps the original request ID

#### Scenario: The same client key on two accounts keeps both records

- **GIVEN** two network routes to different accounts
- **WHEN** a client sends one request with `Idempotency-Key: job-42` through each route
- **THEN** both accounts' request-key stores hold their own record for `job-42`
- **AND** neither record overwrote the other

### Requirement: Paid requests remain recoverable through request status

Every network-routed request SHALL appear in its account's local request keys (Usage & API keys, Requests) and in the request log. Each entry SHALL show the route, account and request key, and the provider request ID when known.

The server's `GET /v1/requests/:id` and `POST /v1/requests/:id/cancel` SHALL resolve `:id` only to a request that the server itself recorded for a network route, under the requirement *Paid routes keep request identity and never retry automatically*. `:id` SHALL equal either that record's provider request ID or, when no request ID was received, its request key.

The server SHALL refuse these cases without contacting any provider:

- An `:id` that is not 1–128 ASCII letters, digits, `.`, `_`, `:` or `-`: 400 `invalid_request_id`.
- No consumer profile loaded: 503 `route_unavailable`.
- An `:id` that matches no such record under the loaded profile's `NetworkIdentity`: 404 `not_found`, with a message stating that the local server holds no such request under the loaded profile. This covers an unknown ID, the `x-request-id` of a local-route request, the ID of a request the desktop made outside the server, and an ID whose recorded route has since been removed, disabled or retargeted to another account.
- A request key that matches records on more than one account: 409 `ambiguous_request_id`.

The server SHALL read status through the native consumer's `request_status` operation and cancel through its `cancel_request` operation. Each SHALL be sent to the recorded account's provider with the recorded route's credential only, never with the account's active credential or another account's credential. A route credential that is missing, expired or locally revoked SHALL be refused with 409 `no_credential`.

The status response SHALL contain only the fields of the provider gateway's request-status object, decoded natively, with money as canonical decimal wei strings. A provider response that does not decode SHALL return 502 `provider_error`. A cancellation SHALL return the provider's 202 `cancelling` result or its final request status, and SHALL NOT be retried automatically. The Local server page, its snippets and the request log SHALL describe cancellation as possibly charged and not a refund. Provider errors on these routes SHALL follow the same classification, header and redirect rules as inference.

For each such request, the desktop SHALL also offer, through the existing native consumer operations in Usage & API keys, reading authoritative request status and account request history, and cancelling the provider request. That cancellation SHALL be labeled as possibly charged and not a refund. Recovery through the desktop SHALL NOT require the local server to be enabled or running, and SHALL survive a desktop restart through the persisted request keys.

After any ambiguous outcome, the server's error and the desktop SHALL direct the user to inspect request status before retrying. Neither the server nor the desktop SHALL compute billing from stream fragments, visible text or usage figures received by the client.

#### Scenario: Status of a forwarded request

- **GIVEN** a network-routed request whose response carried provider request ID `R`
- **WHEN** an authenticated client calls `GET /v1/requests/R`
- **THEN** the server reads the status from that route's provider with the route's credential and returns the decoded request-status object with money as canonical wei strings
- **AND** no inference request is dispatched

#### Scenario: Status by request key after a cut before headers

- **GIVEN** a network-routed request with `Idempotency-Key: job-7` whose connection failed after the body was written but before response headers arrived
- **WHEN** an authenticated client calls `GET /v1/requests/job-7`
- **THEN** the server reads the provider's status for that key on the recorded account with the route's credential
- **AND** no inference request is resent

#### Scenario: Unknown or local request ID is 404 without provider contact

- **WHEN** an authenticated client calls `GET /v1/requests/:id` or `POST /v1/requests/:id/cancel` with an ID the server never recorded, or with the `x-request-id` of a local-route response
- **THEN** the server responds 404 `not_found`
- **AND** no provider is contacted

#### Scenario: Cancel is not a refund

- **GIVEN** a network-routed stream with provider request ID `R` in progress
- **WHEN** an authenticated client posts `/v1/requests/R/cancel`
- **THEN** the provider receives exactly one cancellation for `R`, authenticated with the route's credential, and the client receives 202 with status `cancelling`
- **AND** the request log labels the cancellation as possibly charged and not a refund, and `GET /v1/requests/R` still returns its authoritative status

#### Scenario: Recovery after restart

- **GIVEN** a network-routed stream was interrupted and the desktop was then quit and restarted with the server disabled
- **WHEN** the user opens Usage & API keys, Requests for that account
- **THEN** the request key is listed and its authoritative status can be read
- **AND** no new provider request is created by the recovery

### Requirement: libp2p forwarding follows the peer-to-peer transport contract

The server SHALL respond 503 `transport_unavailable`, and SHALL NOT dispatch by any other path, when the native consumer cannot reach a route's account over HTTPS and libp2p forwarding is unavailable. libp2p forwarding is unavailable when `specify-p2p-provider-transport` is not implemented, or when the loaded profile has no libp2p node.

Once that change's `p2p-provider-transport` capability is implemented, the server SHALL forward libp2p routes over its `/chiral/inference/http/1` stream protocol as that capability defines in these requirements:

- *Native consumers authenticate the provider by its signed peer ID*;
- *Transport selection follows the signed offer and never changes money or terms*;
- *Consumers dial along the direct, hole-punched and relayed ladder*;
- *Paid requests are dispatched only on circuits that can carry them*;
- *Transport failures are recovered through request status, never a blind paid retry*;
- *The active path is disclosed before funding and during use*.

The server SHALL use the native consumer's transport selection and SHALL NOT choose a transport itself. It SHALL refuse any stream whose authenticated remote peer ID differs from the peer ID in the account's signed offer. It SHALL NOT fall back to an address, relay or transport that cannot prove that peer ID. A peer-ID refusal SHALL return 502 `provider_authentication_failed` with the request key. The request log SHALL record it as not sent, and SHALL NOT report it as provider unavailability.

When the native consumer reports a libp2p request as not sent for any other reason, including a circuit-budget refusal and no authenticated path within the ladder deadline, the server SHALL respond 503 `provider_unreachable` with the request key and a fixed message stating that the request was not sent. It SHALL record the request in the request log as not sent, and SHALL NOT dispatch it by another path, transport or provider.

Every request-key, no-automatic-retry, recovery, header, error, redirect and bound rule of this capability SHALL apply unchanged on that transport. A relay circuit refused or cut after any request byte was written SHALL be treated as a transport failure that is recovered through request status; one refused or cut before any request byte was written SHALL be handled as not sent under the preceding paragraph. The routing table and request log SHALL show each network route's transport and, for libp2p routes, the active path disclosure that capability defines. Accounts reachable over HTTPS SHALL remain usable both through the server and directly with an explicitly exported credential.

#### Scenario: A libp2p-only account without libp2p forwarding

- **GIVEN** an account whose retained offer declares only `libp2p` (possible once the offer schema of `specify-p2p-provider-transport` exists)
- **AND** either that change's transport is not implemented or the loaded profile has no libp2p node
- **WHEN** a client posts a request naming the account's route
- **THEN** the server responds 503 `transport_unavailable`
- **AND** no connection is attempted to the provider by any path

#### Scenario: A peer-ID mismatch is refused

- **GIVEN** the peer-to-peer transport is implemented and a connection to a route's provider authenticates a peer ID different from the offer's
- **WHEN** the server would forward a request on it
- **THEN** the client receives 502 `provider_authentication_failed` with the request key, and the request log records it as not sent
- **AND** no request byte is written, and the request is not retried over another address, relay or transport

#### Scenario: A libp2p request that was not sent

- **GIVEN** the peer-to-peer transport is implemented and a route's account is reached over libp2p
- **AND** no authenticated path is established within the ladder deadline, or the only circuit fails the circuit-budget check and neither an upgrade nor a fresh circuit passes it
- **WHEN** a client posts a request naming the route
- **THEN** the client receives 503 `provider_unreachable` with the request key, and the request log records it as not sent
- **AND** no request byte is written, and the request is not dispatched by another path, transport or provider

### Requirement: Network routes follow the native session and network scope

Network routes SHALL resolve only under a loaded consumer profile with an identical `NetworkIdentity`. Logging out, or loading a different profile, SHALL cancel the server's in-flight network-routed requests in the same way as other active streams. Afterwards, routes recorded for a different identity SHALL return 503 `route_unavailable` until a profile with the matching identity is loaded. The server SHALL NOT forward them to an account, credential or provider of another network. Local routes SHALL keep working across logout and profile changes.

#### Scenario: Logout cancels network requests and keeps local routes

- **GIVEN** one network-routed and one local-routed request are in flight
- **WHEN** the user logs out
- **THEN** the network-routed request is cancelled and later requests naming that route return 503 `route_unavailable`
- **AND** the local-routed request continues and the local route keeps accepting requests

### Requirement: The local API server is never advertised and is unrelated to provider listeners

The server SHALL NOT be announced or made discoverable by any mechanism, including:

- DHT or provider records;
- manifests or signed offers;
- identify messages;
- relay registrations;
- catalog submissions;
- mDNS, DNS-SD or SSDP announcements;
- UPnP, NAT-PMP or PCP port mappings.

Its address SHALL NOT be used as an offer `endpoint` or provider listener. It SHALL be a separate listener and router from every provider daemon's public, administration and libp2p listeners, and SHALL NOT share their ports or routes. Its bearer SHALL NOT be, or be accepted as, any of their tokens. The share workflow that `specify-desktop-share-online` defines SHALL NOT offer the local server as a provider endpoint.

#### Scenario: Enabling the server publishes nothing

- **GIVEN** the DHT is running with a loaded profile
- **WHEN** the user enables the server
- **THEN** no DHT record, manifest, offer, identify message, local-network announcement or port mapping contains the server's address or port

### Requirement: The local API request log records metadata only

The server SHALL keep an in-memory request log for the current desktop session, holding at most the 1,000 most recent entries. The oldest entry SHALL be evicted first. The log SHALL be cleared on user request and when the process ends.

An entry SHALL contain only:

- time, in Unix seconds;
- route name, route kind and endpoint;
- whether the request streamed;
- HTTP status and classified outcome code;
- duration;
- token counts from the response's usage object when present, labeled as reported by the engine or provider and not as authoritative billing;
- for network routes: the account ID, request key, provider request ID when known, and the transport and path.

An entry SHALL NOT contain:

- prompts, messages, system prompts or completions;
- embedding inputs or vectors;
- tool definitions or arguments;
- request or response bodies;
- header values other than those listed;
- the bearer, a provider credential or any other secret.

The log SHALL NOT display locally computed costs. A cost SHALL be shown only from an authoritative request-status read, exactly, as canonical wei converted without floating point. Clearing or eviction SHALL NOT delete persisted request keys.

The server's process logs and diagnostics SHALL likewise contain no content or secrets. Diagnostics MAY report the enabled state, the bound address and port, the number of routes and the number of requests in flight.

Requests served through the local server SHALL NOT be written to local chat history as `desktop-local-chat` defines it, whether that history is on or off. The chat-history preference SHALL NOT change what the request log records or how long it keeps it.

#### Scenario: Content never reaches the log

- **GIVEN** a request whose prompt and generated output contain known canary strings
- **WHEN** it completes through any route
- **THEN** the canary strings appear in no request log entry, process log or diagnostic

#### Scenario: The log is bounded and does not own recovery

- **GIVEN** 1,000 log entries including a network-routed request
- **WHEN** a 1,001st request completes and the user then clears the log
- **THEN** the oldest entry was evicted first and the log is empty after clearing
- **AND** the network-routed request's persisted request key is still listed under its account

#### Scenario: Local server requests do not enter chat history

- **GIVEN** local chat history is on, as it is by default
- **WHEN** a request whose prompt and generated output contain known canary strings completes through a local route
- **THEN** no chat thread is created or changed
- **AND** the canary strings appear nowhere under `<app data>/chats/`

### Requirement: The Local server page controls the server without receiving secrets

The shell SHALL add the hash route `#/local-server` and a navigation item **Local server** with a label, a short label and an icon. The item SHALL be placed after My models and before Share (when present), in the navigation order that `desktop-local-workbench` defines. It SHALL expose `aria-current` when active. The item SHALL be added together with the native commands this capability defines, and SHALL NOT appear in a build that lacks them.

The Local server page SHALL contain:

- an enable/disable control and a port input;
- the bearer, masked, with the protected file's location and **Reveal**, **Hide** and **Regenerate** actions. Regenerate SHALL require confirmation that tools using the old bearer will stop working;
- the routing table, with add, edit, enable/disable and remove for local and network routes. Each route SHALL show its target:
  - for a local route: the model identity, and whether it is loaded or shared;
  - for a network route: the provider wallet, model, account status, transport, and its credential's ID, scopes, spend cap and expiry. The spend cap SHALL be shown exactly in CFX, converted from canonical wei without floating point. When the account's agreed signed offer carries no serving profile, the route SHALL state that no generation-speed guarantee applies; when that offer's model also advertises `chat` or `completions`, the statement SHALL read *Unqualified — no speed guarantee*, whatever the offer's prices, derived only from the signed offer. This label SHALL NOT change any admission, credential, request or recovery rule;
- `curl`, Python and JavaScript snippets for `http://127.0.0.1:<port>/v1`;
- the request log.

Snippets SHALL NOT embed the bearer value, including a revealed value. They SHALL read it from the protected file or from an environment variable, without placing it in a URL or in any process's command-line arguments. The `curl` snippet SHALL pass the `Authorization` header to curl through a configuration read on standard input (`curl -K -`) and written by a shell builtin, not by command substitution into curl's arguments. The SDK snippets SHALL disable automatic retries.

The page SHALL control the server only through native desktop commands. Apart from the result of an explicit Reveal, the webview SHALL never receive the bearer. It SHALL never receive a provider credential, an engine key or an administrator token. The webview capability SHALL NOT gain a shell, filesystem, HTTP or other core permission for this feature. Network route creation SHALL be disabled until a consumer profile is loaded, while local routes remain available.

Outside the native runtime, the page SHALL show the browser-preview notice, and every action, including Reveal, SHALL fail with an explanatory error. The page SHALL NOT fabricate a running server, bearer, route or working snippet.

The page SHALL follow these presentation rules:

- native errors are shown verbatim in an error callout;
- status changes use `role="status"` and failures `role="alert"`;
- every input has a visible label;
- route names, model names and provider metadata render as escaped plain text.

#### Scenario: The Local server navigation item

- **WHEN** the user opens `#/local-server` in the native desktop
- **THEN** the Local server page is shown and its navigation item carries `aria-current`
- **AND** the item appears after My models and before Share in the navigation order

#### Scenario: The page holds the bearer only after Reveal

- **GIVEN** the server is enabled
- **WHEN** the Local server page is opened, the bearer is regenerated and snippets are shown, without Reveal
- **THEN** no native command result, webview storage entry or rendered snippet contains the bearer value
- **AND** the snippets read the bearer from the protected file or an environment variable

#### Scenario: Snippets keep the bearer out of argv

- **GIVEN** the server is enabled and its `curl`, Python and JavaScript snippets are displayed
- **WHEN** each snippet is run while the process table is sampled
- **THEN** no process's command-line arguments contain the bearer
- **AND** the `curl` snippet passes the header to curl through `curl -K -`, not through command substitution

#### Scenario: Network routes show their credential cap

- **GIVEN** a network route whose credential was issued with scopes `infer` and `read`, spend cap `250000000000000000` wei and an expiry
- **WHEN** the routing table is shown
- **THEN** the route shows its credential ID, both scopes, a spend cap of exactly 0.25 CFX and the expiry
- **AND** no credential secret is present in the page

#### Scenario: A network route to a free unqualified share is labeled

- **GIVEN** a network route whose account's agreed offer advertises `chat`, has no serving profile, and has input price, output price and minimum funding all `"0"`
- **WHEN** the routing table is shown
- **THEN** the route reads *Unqualified — no speed guarantee*
- **AND** the route still requires its dedicated credential with a positive spend cap

#### Scenario: An embedding route without a serving profile keeps the statement

- **GIVEN** a network route whose account's agreed offer advertises only `embeddings` and has no serving profile
- **WHEN** the routing table is shown
- **THEN** the route states that no generation-speed guarantee applies
- **AND** it does not read *Unqualified — no speed guarantee*

#### Scenario: Browser preview fabricates nothing

- **GIVEN** the GUI runs outside the native runtime
- **WHEN** the user opens the Local server page and tries to enable the server or reveal the bearer
- **THEN** the preview notice is shown and the action fails with an explanatory error
- **AND** no port, bearer, route or request log entry is shown as real

#### Scenario: Network routes need a profile

- **GIVEN** no consumer profile is loaded
- **WHEN** the user opens the routing table
- **THEN** adding a network route is disabled with a pointer to Settings
- **AND** adding a local route remains available
