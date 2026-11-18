## Why

The read-only whitepaper (`docs/chiral-book.md`) defines how the current runtime finds signed offers. Lines 29 and 59 say that discovery is bounded and incomplete, "not a census of all providers", and that neither the DHT nor the relay catalog is an exhaustive directory. The section *Discovery, freshness, withdrawal, and live state* (lines 211–261) fixes the rest: Kademlia advertises candidate peers only; each peer serves its own manifest on `/chiral/inference/manifest/1` in pages of 8; searches are bounded; the verified registry holds at most 8,192 namespace entries; a higher revision replaces, a lower one is rejected, and an equal revision with different content is equivocation; withdrawal keeps the revision watermark for 3,630 seconds; "Partial or invalid manifests do not authorize deletion" (line 234); the provider refreshes every 30 seconds (line 236); a configured DHT takes precedence over the catalog without fallback; the catalog has a fixed page contract; and `/readyz` reports serving readiness apart from new-funding readiness. `CLAUDE.md` adds: "DHT discovery is bounded and signed, with network-scoped namespaces, stable peer identities, monotonic revisions, expiry, and connection admission limits. Relay catalogs are caches, not exhaustive or authoritative provider directories."

No normative or pending specification states these rules for the current runtime. The comparison pass of 2026-11-18 (`docs/apas-worker/comparison-2026-11-18.md`) lists them as missing specifications (items 9, 15 and 17) and found one code difference in them (item 4). Existing changes cover only the edges:

- `inference-commercial-terms` (pending `specify-inference-commercial-terms`) owns the offer declaration, including the `revision` field and the 16 KiB bound, and *Offer freshness and advertised origin*;
- `inference-wire-foundation` (normative) owns the complete network identity and offer IDs;
- the target-design change `specify-p2p-provider-transport` (milestone 3) replaces `/chiral/inference/manifest/1` with `/2` and keeps its framing, bounds, `next` rule and "the existing withdrawal rule", which no specification defines;
- the target-design change `specify-provider-launch-reputation` adds identity members to `/readyz` and reads manifests for availability samples;
- `desktop-gui-pages` (pending) specifies how the Marketplace describes discovery and shows inspection results.

A later change could therefore weaken a discovery bound, a revision rule or the withdrawal rule without a failing requirement.

## What Changes

- Add the `inference-offer-discovery` capability: 16 requirements and 62 scenarios for the current runtime. Each requirement cites its whitepaper lines and comes from the whitepaper or `CLAUDE.md`, not from current code. Values that the book does not state are recorded in `design.md` as current implementation values.
- Specify that discovery yields verified candidate offers only, never a census, a directory or an authority, and that a relay cannot rewrite a signed offer.
- Specify the network-scoped DHT protocols and index keys, stable peer identities kept in protected secret files, bootstrap bounds, connection admission bounds and inbound manifest request bounds.
- Specify the `/chiral/inference/manifest/1` framing, frame bounds, 64-offer manifest, 8-offer pages, offset and `next` rules, per-offer validation, responding-peer binding and a stable page order by namespace.
- Specify search bounds and merging, the 8,192-entry registry and what happens when it is full, monotonic revisions, equivocation, idempotent replay, withdrawal watermarks and the 3,630-second retention.
- Specify that only a complete, valid manifest whose pages come from one snapshot withdraws that peer's cached offers in the requested scope, and how expiry and provider withdrawal bound visibility.
- Specify provider publication: the durable revision counter, the 30-second refresh, 120-second offers, publication only while new-funding ready, and withdrawal of every offer published by a refresh that fails.
- Specify consumer discovery: a fresh registry per call, DHT precedence without merge or fallback, the catalog's separate reputation role, the relay catalog page contract and the native catalog client's pagination bounds.
- Specify `/healthz` and `/readyz` and native provider inspection as unsigned point-in-time observations that never change terms.
- Record a coverage table in `design.md` (requirement → code → existing tests, or the missing case). Record three code differences and the missing acceptance evidence as unchecked tasks.

## Capabilities

### New Capabilities

- `inference-offer-discovery`: Bounded, signed, network-scoped discovery of inference offers in the current runtime: DHT candidate indexes and peer identity, connection admission, the paged `/chiral/inference/manifest/1` protocol, bounded searches, the verified registry's bound, revision, equivocation, withdrawal and retention rules, provider publication and refresh, consumer source precedence, the relay catalog page contract, provider liveness and readiness, and native provider inspection.

### Modified Capabilities

None. This capability references the normative `inference-wire-foundation` and `inference-account-topup-request`, the pending `inference-commercial-terms`, `desktop-gui-pages` and `profile-path-resolution`, and the target-design changes `specify-p2p-provider-transport` and `specify-provider-launch-reputation`. It adds no obligation to them and does not change their requirements.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements:
  - `src-tauri/src/dht.rs`: the swarm, connection limits, manifest codec, searches, manifest serving and withdrawal;
  - `src-tauri/src/offer_registry.rs`: registry bound, revision, equivocation, withdrawal and retention;
  - `src-tauri/src/app.rs`: the revision counter, refresh and publication, drain, engine-failure withdrawal, `/healthz`, `/readyz` and `GET /admin/offers`;
  - `src-tauri/src/bin/relay_server.rs`: the catalog and its 15-second refresh;
  - `src-tauri/src/consumer.rs`: `discover`, `provider_info`, `observe_provider` and the feedback catalog check.
- Three code differences were found. Tasks 2.1–2.3 track them:
  - a manifest whose pages come from different snapshots counts as complete and can withdraw a live offer (comparison item 4), because pages are ordered by offer ID, which changes with every revision;
  - a refresh that fails after publishing some offers leaves those offers served while it reports the failure and closes new-funding readiness;
  - the DHT identity key file is not handled as a protected secret: on Windows it gets no owner-only access list and is never checked, and on Unix a symbolic link is followed and the file's owner is not checked.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.16. The main gaps are paging and partial-manifest tests, registry capacity and equivocation tests, provider publication tests, and every consumer-side source, catalog and inspection test.
- Live multi-node discovery evidence is task 4.1. It needs an authorized private deployment and is never satisfied by loopback fixtures.
- One question for the whitepaper author is recorded in `design.md` and `docs/apas-worker/report.md`: whether `/readyz` reports a provider whose every probe-ready model is performance-withdrawn as ready.
- No wire format, deployment or whitepaper change is part of this specification step. This change grants no live, public-testnet or release qualification.
