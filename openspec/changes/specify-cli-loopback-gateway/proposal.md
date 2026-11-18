## Why

The book's *Peer-to-peer provider transport → SDK and tool compatibility* (`docs/chiral-book.md` line 315) says: "Stock OpenAI SDKs speak HTTPS to a `base_url` and cannot dial libp2p. The native desktop and CLI therefore run a **local loopback gateway** (`http://127.0.0.1:<port>/v1`, the desktop's local API server) that forwards requests for a funded network account to its provider over libp2p while holding the provider credential natively." Line 287 adds that "the local loopback gateway never passes a provider's 3xx status or `Location` header to SDK clients".

The desktop half is specified by `specify-desktop-local-api-server` (milestone 2, capability `desktop-local-api-server`). Both it and `specify-p2p-provider-transport` (milestone 3) leave the CLI half to a named follow-up, `specify-cli-loopback-gateway`, which did not exist. Without it, a machine with no desktop, such as a headless server running an SDK-based tool, has no supported way to use a libp2p-only provider, and nothing states how the CLI's own rules apply to such a server: machine-readable stdout, no secrets in argv or diagnostics, an explicit `--config` profile, network-scoped consumer state, and a shared `data_dir` with the desktop.

The `chiral` CLI has no gateway, no HTTP listener and no long-running consumer command today.

## What Changes

- Add `chiral --config <consumer profile> gateway serve --port <port>`: a foreground command that binds only `127.0.0.1` on a required, user-chosen port with no default, serves exactly the `--config` profile's network and consumer wallet, and treats an occupied port, or the profile's DHT listen port, as an error with no fallback. It never daemonizes and installs no service.
- Stop on SIGINT or SIGTERM (Ctrl-C on Windows), as `chiral_daemon` does. Shutdown closes the listener and cancels in-flight requests the way the desktop's Quit and disable do: streams end with an error event and no `[DONE]`, nothing is retried, and stderr states that cancelled requests can still be charged. The process exits within 10 seconds.
- Keep the gateway's state in a `gateway/` directory inside the consumer's network- and wallet-scoped state directory: an owner-only bearer file and a route table with no secrets, both failing closed when insecure, symbolic links or malformed. This state is separate from the desktop's `<app data>/local-server/`.
- Generate the bearer on first serve. No command prints it: the ready event and `gateway show` give only its path, and `gateway regenerate-bearer` refuses while a gateway is serving that profile.
- Reuse, by requirement name, `desktop-local-api-server`'s request gate (Host, Origin, bearer), six-route subset, connection and request bounds, network forwarding through the native consumer path, request identity, recovery, libp2p forwarding and non-advertisement, with the same status and error codes. Restate only what differs for the CLI.
- Serve network routes only. The CLI has no local model library; milestone 1 is desktop-only. Accounts are reached over HTTPS or libp2p, whichever the native consumer selects.
- Manage routes with `gateway route add | renew | enable | disable | remove`. `add` and `renew` issue a dedicated credential through the native `credential_issue`: scopes `infer` and `read`, an explicit positive spend cap, an expiry of 60 seconds to 30 days, and not made active. They require `--accept-spending-risk` after the milestone 2 spending disclosure on stderr. Removal never revokes silently. Route changes apply to a running gateway without a restart.
- Recover outside the gateway with the existing `chiral requests` and `chiral request`. The latter gains `--credential <id>` to use a route credential exactly, with no fallback.
- Write stdout as JSON lines in the existing `{"kind","data"}` event shape: `ready` (address, port, base URL, bearer file path, network, wallet, routes), one `request` event per authenticated request (milestone 2's log-entry metadata, with transport and path), `stopping` and `stopped`. Disclosures and diagnostics go to stderr. No content or secret appears in either.
- Report each request's path (HTTPS, Direct, Hole-punched or Relayed with the relay's peer ID and reported limits) in its `request` event, and each route's declared transports and peer ID in listings. No new disclosure is added: the gateway funds nothing, so the pre-funding path disclosure stays with `fund` and `topup`.
- Coexist on one `data_dir`. The gateway never holds `consumer.lock`, so funding and credential operations are never blocked by it. At most one gateway serves a profile, enforced by its own serve lock. The desktop's local server and a CLI gateway keep separate bearers, route tables and credentials. Request-key records are created exclusively.

## Capabilities

### New Capabilities

- `cli-loopback-gateway`: the CLI front end of the loopback gateway. It covers:
  - the foreground lifecycle, loopback bind and signal shutdown;
  - protected, network- and wallet-scoped state and the bearer that no command prints;
  - reuse of the local API server's gate, endpoint subset, bounds, forwarding, identity, recovery and non-advertisement rules;
  - the network-only route table, its commands and dedicated route credentials;
  - per-request path reporting;
  - stdout events and stderr diagnostics;
  - coexistence with the desktop and other processes on one `data_dir`.

### Modified Capabilities

None.

- `desktop-local-api-server` (pending in `specify-desktop-local-api-server`) is referenced by requirement name, not modified. Its proposal non-goal line and design open question 1 now name this change, with their meaning unchanged.
- `p2p-provider-transport` (pending in `specify-p2p-provider-transport`) is referenced for libp2p forwarding and path disclosure once it is implemented, and is not modified.
- Consumer funding, credential, request-accounting and owner-control contracts are relied on, not changed.

## Impact

- **Native (planned):**
  - `src-tauri/src/bin/chiral.rs`: the `gateway` subcommands (`serve`, `show`, `regenerate-bearer`, `route add|renew|enable|disable|remove`) and `request --credential`.
  - The milestone 2 local-server core (listener with connection bounds, gate, routing, forwarding, request routes, log entry), placed in the shared library so that it builds with `--no-default-features` and serves both front ends.
  - `src-tauri/src/consumer.rs`: milestone 2's extensions (caller-supplied request key, explicit credential, non-activating issuance, records keyed by account and key with route ID and provider request ID), plus exclusive creation of request-key records.
- **State:** `<data_dir>/inference/<network namespace>/consumer/<wallet address>/gateway/` holding `bearer`, the route table and the serve and route-table lock files.
- **Documentation:** a README section for the CLI gateway, with SDK and `curl` snippets that read the bearer file without placing it in argv and that disable SDK retries.
- **Dependencies:**
  - `specify-desktop-local-api-server` (M2): its routing core and consumer extensions are implemented first, and it is archived first, because this change's requirements cite its requirement names.
  - `specify-p2p-provider-transport` (M3): libp2p forwarding, the DHT node at serve time and observed paths. Until it is implemented, the gateway forwards HTTPS accounts and refuses libp2p-only accounts with 503 `transport_unavailable`. It is referenced only conditionally and need not be archived first.
- **Authority:** `docs/chiral-book.md` (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`) is read-only and is not edited.
- **Out of scope:**
  - local-model routes in the CLI (open question 1);
  - background services, LAN exposure and a bearer reveal command;
  - funding or credential management over HTTP;
  - any change to the desktop local server.
