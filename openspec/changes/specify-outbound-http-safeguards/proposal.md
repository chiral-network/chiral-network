## Why

The read-only whitepaper (`docs/chiral-book.md`) fixes the current runtime's HTTP trust boundaries in *HTTP origins and trust boundaries* (lines 262–279). Remote provider and catalog origins must be public HTTPS on port 443, with a development exception only for literal loopback or exact `localhost` (line 264). An advertised endpoint and a catalog URL are origins, and an SDK's `base_url` adds `/v1` only after the origin is validated (line 266). `safe_http_client` resolves once under a five-second deadline, inspects up to 33 answers, rejects an empty set, more than 32 answers or any disallowed answer, pins the accepted addresses, disables environment proxies and every redirect, and sets a ten-second connect timeout (line 268). `offer_registry.rs::public_ip` is the exact address predicate, and it governs outbound HTTP, not libp2p addresses (line 270). Administration is a distinct nonzero loopback-only listener with its own protected bearer, the private engine URL is operator configuration on a private literal address or exact `localhost`, private-engine clients disable proxies and redirects, and a TLS reverse proxy may front only the public listener (lines 272–276).

No normative or pending specification states these rules as a whole. The comparison pass of 2026-11-18 (`docs/apas-worker/comparison-2026-11-18.md`) lists them as missing specification item 11. Existing changes cover only parts:

- `inference-commercial-terms` (pending `specify-inference-commercial-terms`) owns the static advertised-origin rule of the signed offer `endpoint` (*Offer freshness and advertised origin*), and says that "outbound transport safeguards remain additionally required" without specifying them;
- `managed-engine-runtime` (pending `specify-managed-engine-launch`) owns the managed engine's literal-loopback endpoint, its key separation and its startup health probe;
- `inference-funding-settlement` (pending `specify-funding-settlement`) owns the chain RPC transport, a different boundary (book line 458);
- `chain-rpc-gateway` (pending `specify-chain-rpc-gateway`) owns the standalone RPC gateway's inbound bearer;
- `client-version-policy` (pending `specify-version-policy`) owns the consumer `policy_url` source rule, and `payment-gated-reputation` (pending `specify-payment-gated-reputation`) sends feedback "under the outbound HTTPS origin rules" without stating them;
- the target-design changes `specify-desktop-local-api-server` and `specify-cli-loopback-gateway` name "the outbound HTTPS origin boundary" without a normative home, and `specify-p2p-provider-transport` reuses `public_ip` for libp2p addresses and replaces the public-HTTPS rule when implemented.

A later change could therefore follow redirects, honour an environment proxy, widen the development exception, accept an internal engine hostname or mount an administration route publicly without a failing requirement.

## What Changes

- Add the `outbound-http-safeguards` capability: 8 requirements and 29 scenarios for the current runtime. Each requirement cites its whitepaper lines and comes from the whitepaper or `CLAUDE.md`, not from current code. Values that the book does not state are recorded in `design.md` as current implementation values.
- Specify that every remote provider and catalog origin, including `catalog_url` and the provider's own `public_endpoint`, passes the advertised-origin rule of `inference-commercial-terms` before any DNS query or connection.
- Specify the development exception, including that a `localhost` origin must resolve only to loopback addresses, and that a development provider's public listener binds loopback.
- Specify that advertised endpoints and catalog URLs are origins, and that credential export produces the SDK `base_url` only from an origin that passes validation under the current mode.
- Specify the per-origin DNS-pinned client: the 5-second resolution deadline, the 33-answer inspection and 32-answer bound, refusal of any disallowed answer, pinning, no environment proxies, no redirects, the 10-second connect timeout, and normal TLS verification.
- Specify the public address predicate exactly as `offer_registry::public_ip` implements it, and its scope (outbound HTTP, not libp2p).
- Specify the loopback-only administration listener and its bearer, the private engine origin and its clients, and the TLS ingress boundary of the current runtime.
- Record a coverage table in `design.md` (requirement → code → existing tests, or the missing case). Record two code differences and the missing acceptance evidence as unchecked tasks.

## Capabilities

### New Capabilities

- `outbound-http-safeguards`: The current runtime's HTTP trust boundaries: validation of remote provider and catalog origins, the development loopback exception, origins versus SDK and catalog paths, the per-origin DNS-pinned client without proxies or redirects, the exact public address predicate, the loopback-only administration listener and its bearer, the private engine origin and its clients, and the TLS ingress boundary.

### Modified Capabilities

None. This capability references the pending `inference-commercial-terms` (*Offer freshness and advertised origin*) and `managed-engine-runtime` (*The engine listens only on its literal-loopback endpoint, and an occupied port is refused*; *The child receives a minimal environment and its API key only as a protected file path*), and names the target-design change `specify-p2p-provider-transport`. It adds no obligation to them and does not change their requirements. The chain RPC endpoint is a separate boundary owned by `inference-funding-settlement` and `chain-rpc-gateway`; this capability does not cover it.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements:
  - `src-tauri/src/offer_registry.rs`: `public_ip`, `validate_endpoint`, `safe_http_client`;
  - `src-tauri/src/consumer.rs`: `endpoint_url`, `transport`, `Consumer::new`, the provider, catalog and policy requests, `credential_export` and `provider_admin`;
  - `src-tauri/src/provider_config.rs`: listener, `public_endpoint` and `engine_url` validation, `parse_origin` and `is_private_engine_ip`;
  - `src-tauri/src/app.rs` and `src-tauri/src/bin/chiral_daemon.rs`: the public and administration routers, `admin_auth` and the listener binds;
  - `src-tauri/src/inference_backend.rs` `Backend::new` and `src-tauri/src/engine_runtime.rs` `await_health`: the private engine clients;
  - `scripts/provision-provider.py` (reference Caddyfile and binds) and `scripts/provider-preflight.py` (public-route and listener checks).
- Two code differences were found. Tasks 2.1 and 2.2 track them:
  - under development mode, a `localhost` origin whose resolution includes a public address is accepted and pinned, so a request can go to a remote host in cleartext;
  - credential export derives the SDK `base_url` with the consumer's syntax check instead of the origin validator, so a saved account whose origin no longer validates still yields a `base_url` and its API secret.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.8. The main gaps: nothing tests the pinned client (answer bounds, deadline, pinning, proxies, redirects), the administration refusals, the general engine-origin rule, the engine client's redirect refusal, the SDK `base_url`, or the reference ingress and preflight checks.
- Deployment evidence through real TLS ingress is task 4.1. It needs an authorized private deployment and is never satisfied by loopback fixtures.
- Two questions for the whitepaper author are recorded in `design.md`: whether the chain RPC client should also ignore environment proxies, and whether the version-policy source falls under the provider/catalog origin rule (a question for `client-version-policy`, which owns that source). Both are nonblocking.
- No wire format, deployment or whitepaper change is part of this specification step. This change grants no live, public-testnet or release qualification.
