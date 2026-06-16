# Known Bugs and Limitations

A running list of bugs and rough edges that have been found but not yet fixed.
Anything here is fair game to pick up. Closed items live in `git log` — keep
this file lean by removing entries when their fix lands on `main`.

Format per entry:

- **Short title** — one-line summary
  - **Where:** file:line or component
  - **Symptom:** what the user / operator sees
  - **Why it's still here:** the constraint or trade-off blocking the fix
  - **Workaround:** if any

---

## High priority

### Folder bundles — child files downloadable for free with bare hash

- **Where:** `src-tauri/src/lib.rs::publish_drive_folder`, chunked-transfer protocol in `dht.rs`
- **Symptom:** When a seller publishes a folder bundle for X CHI, the seller's child files are individually published at `price_wei = 0`. A buyer who knows a child file's hash (separate from the folder hash) can download that single file directly without paying the folder owner.
- **Why it's still here:** Closing the gap requires extending `ChunkRequest::PaymentProof` and `ChunkResponse::FileInfo` with folder context (`folder_hash`, folder-level `priceWei`/`walletAddress`), plus a per-folder spent-tx ledger so one folder payment unlocks every file in the manifest. Sketched in an earlier prototype but reverted to ship the V1 UX flow first.
- **Workaround:** Don't put files in folders that you'd object to being downloaded for free outside the folder context.

### Policy public key is the 32-byte placeholder

- **Where:** `src-tauri/src/version.rs::POLICY_PUBLIC_KEY`
- **Symptom:** Every binary prints `[VERSION] WARNING: policy-signing public key is the placeholder zeros` at startup. Signed `VersionPolicy` updates can't verify, so only the unsigned-but-permissive transition path is active — a remote policy can relax constraints (set `recommended` higher, add a download URL) but can never raise `min_required` past the bundled floor.
- **Why it's still here:** Generating the keypair, persisting the private half securely (CI secret / offline laptop), and rolling the public half into all three binaries (desktop, daemon, relay) is an operational task, not a code fix.
- **Workaround:** Operators can override the public key via the `CHIRAL_POLICY_PUBLIC_KEY` env var without recompiling, once a real keypair is generated with `chiral-policy-sign keygen`.

---

## Medium priority

### Drive share relay proxy needs sharer reachable on port 9419

- **Where:** `src-tauri/src/relay_share_proxy.rs`
- **Symptom:** A user behind NAT can publish a drive share (the relay accepts the registration), but the relay's reverse-proxy fetch to the sharer's local server at `<sharer_ip>:9419` fails — visitors hitting `https://relay/drive/:token/...` get a 502 or timeout.
- **Why it's still here:** Real fix needs hole-punching or a libp2p stream relay for the HTTP traffic. Architecturally bigger than a quick patch.
- **Workaround:** Sharers either expose port 9419 (UPnP / port forwarding) or upload to the always-on CDN instead.

### Reputation verdict signature can't be verified from `issuer_id` alone

- **Where:** `src-tauri/src/reputation.rs`, `DhtService.ed25519_secret_key`
- **Symptom:** Reputation verdicts are Ed25519-signed by the issuer (`DhtService.ed25519_secret_key` — separate from the libp2p identity key), and the verdict carries `issuer_id` (a libp2p PeerId string). Verifiers don't have a way to map `issuer_id` → public key, so signature verification is effectively skipped on retrieval.
- **Why it's still here:** Needs an exchange step where each peer publishes its Ed25519 verifying key under a well-known DHT key, signed by something the verifier already trusts (the libp2p identity key, or the wallet's secp256k1 key). Non-trivial.
- **Workaround:** Reputation panel currently displays raw verdicts without signature verification; consumers should treat scores as advisory.

### CDN file metadata blob lookup is timing-sensitive across daemon restarts

- **Where:** `src-tauri/src/cdn_server.rs::register_in_dht` and the matching paths in `lib.rs` / `chiral_daemon.rs`
- **Symptom:** The recent always-republish fix (`168f7539`) closed the obvious case (`blob_present` short-circuit + first-hit Kademlia made the publisher skip its own put), but there's still a window during DHT bootstrap where a put can fail silently if the swarm hasn't connected to the bootstrap peer yet. The CDN's startup re-seed runs on a 15s timer, which usually beats this, but on cold starts with very slow DNS / TCP handshake the put can fire before any remote peer is in the routing table.
- **Why it's still here:** Bigger fix is to gate the re-seed on a confirmed Kademlia bootstrap event rather than a fixed delay.
- **Workaround:** A second restart almost always succeeds. The auto-reseed background task (every 30s after startup) catches it on the next tick.

---

## Low priority / cosmetic

### Diagnostics page event log limited to 500 entries

- **Where:** `src/pages/Diagnostics.svelte::maxLogEntries`
- **Symptom:** During heavy network activity (lots of peer-discovered events), older entries scroll off and can't be recovered without re-triggering the activity. Export captures the current 500 max.
- **Why it's still here:** Bounded to keep the Svelte reactive update cost low; an unbounded buffer would chew GC.
- **Workaround:** Operators investigating a long-running issue should `Export` periodically or use the `Copy snapshot` button which captures recent state at a point in time.

---

## Test gaps (not bugs in product code, but flagged so contributors know)

- **DHT node spawn tests** require a live network and skip / fail in offline CI runs.
- **FTP URL parsing test** has been failing pre-existing for several months — see `src-tauri/tests/`.
- **File transfer retry test** is flaky due to timing assumptions.
- **Playwright e2e specs** rely on a real freshnet relay + chain; only run them when the canonical relay is up.
- **Large file upload test** allocates ~500 MB and is gated to CI runs with sufficient memory.

---

## How to add a new entry

1. Reproduce the bug; confirm it's not already fixed on `main`.
2. Find the file:line where the buggy code lives.
3. Add an entry under the right priority band:
   - **High** = silent data loss, payment redirection, security gap, breaks core flows.
   - **Medium** = noticeable UX problem, edge case that affects real users.
   - **Low** = cosmetic, perf-with-workaround, or docs / tooling rough edges.
4. Use the same `Where / Symptom / Why it's still here / Workaround` template so triage is uniform.
5. When the fix lands, **delete the entry in the same PR** so this file doesn't drift.
