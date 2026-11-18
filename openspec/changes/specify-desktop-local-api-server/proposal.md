## Why

The book's adopted target design (`docs/chiral-book.md`, Part I *Direction, precedence, and status*) makes the desktop a local-first LLM client. Milestone 2 of *Desktop local-client workflow → Status and milestones* is an optional **local API server**: a loopback OpenAI-compatible endpoint that tools on the same machine can use with a loaded local model or with a funded network account. It is also the book's answer to SDK compatibility for the peer-to-peer transport (*Peer-to-peer provider transport → SDK and tool compatibility*). Stock OpenAI SDKs cannot dial libp2p, so the local server forwards to the provider and keeps the provider credential in native storage.

No specification or code covers a local server today. The desktop's only inference paths are native commands (`infer`, `inference_stream`). An HTTP listener that holds spending credentials adds new risks: browser pages reaching it (CSRF), DNS rebinding, leaked bearers, spending beyond what the user delegated, automatic paid retries and confusion with provider listeners. Release gate 7 requires "local API server isolation" to be qualified before the target design is presented as available. The contract therefore has to be written down before implementation.

## What Changes

- Add an optional local OpenAI-compatible API server to the native desktop. It is off by default, binds only `127.0.0.1` on a user-chosen port, never offers a LAN bind and treats an occupied port as an error. It runs only while the desktop process runs: closing the window keeps it running, and Quit stops it.
- Authenticate every request with a locally generated bearer of at least 256 bits, stored as a protected file under `<app data>/local-server/`. Regenerating it replaces the file atomically. The bearer never reaches logs, URLs, argv or providers. It reaches the webview only through an explicit **Reveal** on the Local server page (A.1.1), shown masked and cleared on Hide, on leaving the page, on Regenerate, on logout and on profile change.
- Refuse any request carrying an `Origin` header, and any request whose `Host` is not `127.0.0.1:<port>` or `localhost:<port>`. Both checks run before the body is read or the bearer is evaluated. The server emits no CORS headers.
- Serve exactly the book's routes:
  - `GET /v1/models`, `POST /v1/chat/completions`, `POST /v1/completions` and `POST /v1/embeddings`;
  - for paid (network) routes only, `GET /v1/requests/:id` and `POST /v1/requests/:id/cancel`. These resolve only requests the server itself forwarded, and use only that route's credential.

  No engine, administration, control or proxy routes are exposed.
- Fix the bounds. Connections: a 5-second header read, a 16 KiB header block, at most 64 open connections and a 30-second keep-alive idle timeout. Requests: body, concurrency, response size and duration limits.
- Route each model name, through a user-managed table, to exactly one pinned target: a loaded local model (free, no accounting, never touching the network), or a funded network account.
  - A network route uses one dedicated credential issued for it through the existing native `credential_issue`: scopes `infer` and `read`, an explicit spend cap and an expiry. It never falls back to the account's active credential. The Local server page shows each route's cap.
  - Network accounts are reached through the existing native consumer path. That path keeps the provider credential native, enforces the client version and the HTTPS origin boundary, persists request keys before dispatch, never retries paid requests automatically, and never passes a provider 3xx or `Location` header to SDK clients.
  - A free account's allowance refusals (`free_allowance_exhausted`, `free_allowance_expired`, from `specify-p2p-provider-transport`) and a provider's qualification-pause refusal (`qualification_in_progress`, from `specify-desktop-share-online`) are returned with their own codes and the native consumer's fixed messages and logged as provider refusals. Only `qualification_in_progress` passes the provider's `Retry-After`, as integer seconds from 1 to 3600 and otherwise dropped. They are never retried by the server and never lead the server to open or fund another account.
- Route a shared model's local routes through `specify-desktop-share-online` admission instead of to an engine, including a free share that publishes without qualification under the book's 2026-11-16 *Author decisions* item 1, and refuse local routes to an installation under a qualification run with 503 `share_unavailable`. No local route carries a speed guarantee, and a network route to a generation offer without a serving profile, such as a free unqualified share, is labeled *Unqualified — no speed guarantee*.
- Forward over libp2p only through `specify-p2p-provider-transport`, with peer-ID authentication, a `provider_authentication_failed` refusal, a `provider_unreachable` response for other requests the transport reports as not sent, and path disclosure. Until then, and from profiles with no libp2p node, routes to accounts reachable only over libp2p are refused.
- Never advertise the server, and keep it separate from every provider listener. Keep a request log of metadata only, never content, and never write server requests to local chat history. Add the Local server page and its `#/local-server` navigation item (Appendix A.7). The page controls everything through native commands. Its snippets keep the bearer out of URLs and every process's argv.

## Capabilities

### New Capabilities

- `desktop-local-api-server`: the loopback local OpenAI-compatible API server. It covers:
  - lifecycle, bearer authentication and the explicit bearer reveal;
  - origin and host isolation;
  - the endpoint subset, including the paid request-status and cancel routes, and the connection and request bounds;
  - the routing table with dedicated, spend-capped route credentials;
  - local, shared-model and network route behavior;
  - paid-request identity and recovery;
  - libp2p forwarding through the peer-to-peer transport;
  - session scoping, non-advertisement and the metadata-only request log;
  - the Local server page and its navigation item.

### Modified Capabilities

None.

- `desktop-window-lifecycle` is unchanged. The server stops because the confirmed Quit ends the process, and closing the window keeps it running, as that capability already requires.
- Consumer funding, credential and request contracts are relied on, not changed. This includes the pending `specify-inference-request-accounting` duplicate-request semantics and `specify-owner-control-authorization`. The server never performs owner-authenticated operations; the Local server page issues a route credential only through the existing native `credential_issue`.
- No capability that exists only in a pending change is modified. The archive order is stated under *Dependencies*.

## Impact

- **Native (planned):**
  - New local-server module in `src-tauri/src/`: listener with connection bounds, gate, routing, request routes and log.
  - `src-tauri/src/consumer.rs`:
    - Accept a validated caller-supplied request key.
    - Accept an explicit credential ID for inference, `request_status` and `cancel_request` with no fallback to the active credential. Today `credential(&saved, None)` always picks the active credential.
    - Issue a credential without making it the active one.
    - Extend the local request record with route ID and provider request ID, keyed by account and request key with file names that are safe for keys containing `:`.
    - Expose the shared dispatch and classified-error path to the server.
  - `src-tauri/src/desktop.rs`: native commands including Reveal, lifecycle, and cancellation on logout and profile change.
  - Reuse of the request-schema validation in `src-tauri/src/inference_gateway.rs` / `inference_backend.rs` for local routes.
- **Frontend (planned):**
  - New Local server page and its `#/local-server` navigation item.
  - Usage & API keys, Requests: marks requests that came through the local server.
  - No new core webview permission.
- **State:** `<app data>/local-server/` holds the bearer file and settings: enablement, port, and the routing table with credential IDs and their issued scopes, cap and expiry, but no secrets.
- **Dependencies:**
  - `specify-desktop-local-models` (M1): installed-model identity, per-model engine children and the navigation order, which local routes and the navigation item need.
  - `specify-p2p-provider-transport` (M3): libp2p forwarding, transport selection, peer-ID authentication, the connectivity ladder, the circuit-budget check and path disclosure, and the free allowance with its 402 refusals.
  - `specify-desktop-share-online` (M4): admission for a local route to a shared model, whose dispatch its task 2.17 implements while this change defines the response mapping; the qualification-run refusal; the qualification pause's 503 `qualification_in_progress`; and excluding the local server as a share endpoint.
  - Local routes can be implemented only after M1; libp2p routes only after M3.
  - Archive this change after `specify-desktop-local-models`, whose `desktop-model-acquisition`, `desktop-local-runtime`, `desktop-local-workbench` and `desktop-local-chat` capabilities its requirements reference. It references `desktop-share-online` and `p2p-provider-transport` only conditionally, for behavior that applies once those capabilities are implemented (a shared or qualifying installation, the share workflow, libp2p forwarding), and may be archived before them.
- **Documentation:** `docs/chiral-book.md`, as committed in `fe473508` plus the uncommitted 2026-11-16 *Author decisions* edit (items 1–11) and the author's later 2026-11-16 chat decisions recorded in *Wire change*, *Share online* step 4 and *Launch reputation* (checked against the working copy with SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`), is read-only authority for this change and is not edited.
- **Out of scope:**
  - LAN exposure.
  - The CLI loopback gateway, specified by the follow-up change `specify-cli-loopback-gateway` (capability `cli-loopback-gateway`), which cites this capability's requirements by name and is archived after this change.
  - Extra OpenAI endpoints.
  - Funding, or any credential management other than issuing a route's credential.
  - Model download or loading (see design).
