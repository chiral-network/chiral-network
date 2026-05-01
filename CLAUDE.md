# Chiral Network Development Guide

## Overview

Chiral Network is a Tauri 2 desktop app with a Svelte 5 frontend and Rust backend.

Primary domains:

- DHT peer networking and file transfer
- Drive file management + seeding
- Download orchestration and payments
- Hosting marketplace and relay publication
- CPU/GPU mining controls
- Wallet management and reputation (Elo)
- Headless daemon + CLI operations

## Current Page Surface

- `/wallet` — wallet creation, import, backup
- `/network` — P2P connection, peer list, Geth status
- `/download` — file search, download with CHI payments
- `/drive` — local file management, seeding, sharing
- `/chiraldrop` — direct peer-to-peer file transfers
- `/hosts` — hosting marketplace: My Sites, CDN Servers, Peer Hosts, Agreements
- `/mining` — CPU/GPU mining controls
- `/account` — wallet info, reputation panel
- `/settings` — appearance, notifications, download directory
- `/diagnostics` — event log, system info

## Core Source Layout

- Frontend app shell: `src/App.svelte`
- Frontend stores/services: `src/lib/`
- Toast notifications: `src/lib/toastStore.ts`
- Backend command layer: `src-tauri/src/lib.rs` (thin wrappers)
- Wallet module: `src-tauri/src/wallet.rs` (balance, transactions, history, signing)
- Shared RPC client: `src-tauri/src/rpc_client.rs` (connection-pooled, batch requests)
- DHT/libp2p: `src-tauri/src/dht.rs`
- Drive persistence/API: `src-tauri/src/drive_storage.rs`, `src-tauri/src/drive_api.rs`
- Hosting server: `src-tauri/src/hosting_server.rs`
- Hosting types: `src-tauri/src/hosting.rs`
- Geth/mining integration: `src-tauri/src/geth.rs`
- Geth bootstrap health: `src-tauri/src/geth_bootstrap.rs`
- Speed/cost tiers: `src-tauri/src/speed_tiers.rs`
- Reputation (Elo): `src-tauri/src/rating_api.rs`, `src-tauri/src/rating_storage.rs`
- Wallet backup email: `src-tauri/src/wallet_backup_api.rs`
- Relay share proxy: `src-tauri/src/relay_share_proxy.rs`
- Encryption: `src-tauri/src/encryption.rs`
- File transfer: `src-tauri/src/file_transfer.rs`
- Chain RPC proxy: `src-tauri/src/chain_rpc_api.rs`
- Event sink: `src-tauri/src/event_sink.rs`
- Headless binaries: `src-tauri/src/bin/chiral.rs`, `src-tauri/src/bin/chiral_daemon.rs`
- Relay server: `src-tauri/src/bin/relay_server.rs`

## Architecture

```
rpc_client.rs  (shared HTTP client, batch JSON-RPC, response cache)
     ^               ^
     |               |
 wallet.rs       geth.rs  (process lifecycle, mining, RPC)
     ^               ^
     |               |
     +--- lib.rs ----+   (thin Tauri command wrappers, AppState)
              ^
              |
          dht.rs  (calls wallet::send_payment for file downloads)
```

No circular dependencies. `wallet.rs` imports from `rpc_client` and `geth` (for endpoint routing). `lib.rs` delegates to both.

## Command Surface (Tauri)

Command registration is in `tauri::generate_handler![...]` in `src-tauri/src/lib.rs`.

Main categories:

- DHT/network commands
- File transfer/download commands
- Wallet/transaction commands (delegated to `wallet.rs`)
- Geth/mining (CPU and GPU) commands
- Drive CRUD/seeding/share commands
- Hosting/marketplace commands
- Encryption commands
- Diagnostics and lifecycle commands

## Development Commands

```bash
# frontend deps
npm install

# desktop dev
npm run tauri:dev

# frontend build
npm run build

# frontend tests
npm test

# rust tests
cargo test --manifest-path src-tauri/Cargo.toml

# rust compile check
cargo check --manifest-path src-tauri/Cargo.toml
```

## Headless Mode

```bash
# Direct daemon start
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral_daemon -- --port 9419 --auto-start-dht

# Via CLI
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon start --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon status --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon stop --port 9419
```

Headless daemon API endpoints (port 9419 by default):

- **Health**: GET `/api/health`, GET `/api/ready`
- **Wallet**: POST `wallet/create`, `wallet/import`, `wallet/balance`, `wallet/send`, `wallet/receipt`, `wallet/history`, `wallet/faucet`; GET `wallet`, `wallet/chain-id`
- **DHT**: POST `dht/start`, `dht/stop`, `dht/put`, `dht/get`, `dht/ping`, `dht/echo`; GET `dht/health`, `dht/peers`, `dht/peer-id`, `dht/listening-addresses`
- **Files**: POST `dht/register-shared-file`, `dht/unregister-shared-file`, `dht/request-file`, `dht/send-file`, `file/search`
- **ChiralDrop**: GET `drop/inbox`, `drop/outgoing`; POST `drop/accept`, `drop/decline`
- **Geth**: POST `geth/install`, `geth/start`, `geth/stop`; GET `geth/status`, `geth/logs`
- **Mining**: POST `mining/start`, `mining/stop`, `mining/miner-address`; GET `mining/status`, `mining/blocks`
- **Hosting**: POST `hosting/publish-ad`; GET `hosting/registry`
- **CDN**: POST `cdn/upload`; GET `cdn/files`, `cdn/pricing`, `cdn/status`; DELETE `cdn/files/:hash`; PUT `cdn/files/:hash` (update price)
- **Drive**: Full CRUD via `/api/drive/*` routes (requires both `X-Owner` and `X-Owner-Sig: <unix_ts>:<hex_sig>` headers — see Owner-proof auth below)
- **Diagnostics**: GET `bootstrap-health`

All headless paths are prefixed with `/api/headless/` except health, ready, and drive routes.

## Docker Testing

```bash
# 30 production-connected nodes (host networking)
docker compose -f docker-compose.production-net.yml up -d

# Run stress test against 30 nodes
bash scripts/stress-test-30-nodes.sh

# Local isolated testing with relay
docker compose -f docker-compose.local-test.yml up -d --scale node=10
```

## Geth / Blockchain

- Chain ID: `98765` (Chiral Network)
- Consensus: Ethash (PoW)
- Block reward: 5 CHI
- Sync mode: `full` (default, configurable via `CHIRAL_GETH_SYNCMODE`)
- GC mode: `archive` (keeps all state, prevents block regression on restart)
- Genesis difficulty: `0x400000`
- Bootstrap enode: `130.245.173.73:30303`
- RPC: local at `127.0.0.1:8545` when running, remote fallback `130.245.173.73:8545`

## Reputation System

Elo scores are computed from transfer outcomes only (no user ratings). The formula uses:
- Transfer outcome: completed (1.0) or failed (0.0)
- Time weighting: recent events (within 180-day lookback) weighted more heavily
- Amount weighting: logarithmic scaling based on CHI transferred
- Bounded Elo updates clamped to 0-100

REST endpoints on the relay server (`http://130.245.173.73:8080`):
- `POST /api/ratings/transfer` — record transfer outcome
- `GET /api/ratings/:wallet` — get Elo + event history
- `POST /api/ratings/batch` — batch Elo lookup

## Toast Notifications

The toast store (`src/lib/toastStore.ts`) provides:
- `toasts.show(message, type, duration?)` — always show
- `toasts.detail(message, description, type, duration?)` — always show with title + description
- `toasts.notify(key, message, type, duration?)` — respects notification settings
- `toasts.notifyDetail(key, message, description, type, duration?)` — respects notification settings

Use `notify`/`notifyDetail` for event-driven notifications (downloads, network, mining, payments) so users can toggle them in Settings. Use `show`/`detail` for user-initiated action feedback.

## Relay Server

The canonical box `130.245.173.73` runs **two** systemd services. Both must be updated together when shipping FM-Agent / trust-contract changes — clients reject unsigned records published by either side, so a stale binary on one of them silently breaks the network.

| Service | Binary | Listens on | Role |
|---|---|---|---|
| `relay-server.service` | `/usr/local/bin/relay_server` | tcp/4001 (libp2p), tcp/8080 (HTTP gateway) | libp2p circuit relay v2 + same-origin HTTP API: reputation, drive proxy, wallet backup, site hosting, `/api/chain/rpc` proxy. |
| `cdn-freshnet.service` | `/opt/chiral/chiral_daemon` | tcp/9420 (HTTP), tcp/4002 (libp2p), spawns geth on 8545/30303 | The always-on CDN node. Same `chiral_daemon` binary as desktop, just headless and pre-configured with `--auto-start-dht --auto-start-geth --auto-mine`. |

Both services should be restarted together after any update to the trust contract:
```bash
ssh root@130.245.173.73 'systemctl restart relay-server.service cdn-freshnet.service'
```

Other notes:
- The relay filters private IPs from the Kademlia routing table (only stores public + relay-circuit addresses). Max 256 circuit reservations, 16 per peer.
- Geth is spawned by `chiral_daemon` (the `cdn-freshnet` unit). Its `--http.api` deliberately omits `admin` since the namespace is reachable over the public HTTP RPC and `admin_stopRPC` was used in the wild to take the RPC offline; admin remains available over the IPC socket (`<datadir>/geth/geth.ipc`).
- The CDN's `chiral_daemon` is started with `CHIRAL_WALLET_KEY_FILE=/etc/chiral-cdn-wallet.key` (mode 0600 hex private key). Without it, the daemon can populate the CDN's `wallet_address` from `--miner-address` but has no signing key, so every CDN-served file ends up with empty `chiral_seeder_*` / `chiral_file_*` signatures — clients reject those records and the upload is unreachable. Generate a key with `chiral wallet create` (or any 32-byte hex), drop it at the path, set perms 0600.

SMTP env vars: `CHIRAL_WALLET_EMAIL_SMTP_HOST`, `CHIRAL_WALLET_EMAIL_FROM` (required); `CHIRAL_WALLET_EMAIL_SMTP_USERNAME`, `CHIRAL_WALLET_EMAIL_SMTP_PASSWORD` (optional for local postfix).

## k3s Test Cluster

Cluster lives at `130.245.173.231` (login `debian`, sudo allowed). 30 chiral test daemons in namespace `chiral-test` as bare `Pod` objects (no Deployment / StatefulSet wrapping them). Image `docker.io/library/chiral-network-node:latest`, `imagePullPolicy: Never` — pulls aren't possible, so the image must be loaded into containerd manually:

```bash
# On the dev box, after `cargo build --release` is current:
docker build -f Dockerfile.local -t chiral-network-node:latest .
docker save chiral-network-node:latest -o /tmp/cnn.tar
scp /tmp/cnn.tar debian@130.245.173.231:/tmp/
ssh debian@130.245.173.231 'sudo k3s ctr images import /tmp/cnn.tar'
ssh debian@130.245.173.231 'kubectl delete pods --all -n chiral-test'
# Re-apply the manifest (it's not auto-recreated since pods are bare):
ssh debian@130.245.173.231 'kubectl apply -f /tmp/chiral-nodes.yaml'
```

The manifest at `/tmp/chiral-nodes.yaml` on `.231` covers the 30 pods (each with default container networking, an `emptyDir` `/data` volume, livenessProbe on `/api/health`, args `--port 9419 --auto-start-dht`). Pods reach the canonical `.73` relay through libp2p circuit relay since they use container networking, not host networking.

## Implementation Notes

- Prefer Tauri `invoke()` paths for app runtime behavior.
- All wallet logic lives in `wallet.rs` — `lib.rs` contains thin wrappers only.
- All RPC calls use the shared `rpc_client.rs` (connection-pooled, 5s timeout).
- Balance queries use a 5-second `RpcCache` to avoid duplicate RPC calls.
- Transaction sending batches nonce + balance + gasPrice in a single HTTP request.
- Transaction metadata persisted to `~/.local/share/chiral-network/tx_metadata.json`.
- Drive seeding state is restored from backend at DHT startup.
- DHT auto-restarts on login (tied to `isAuthenticated` store).
- Kademlia re-bootstraps every 30 seconds to discover new peers.
- Event listeners registered before DHT start to avoid missed events.
- Stale peers removed after 5 minutes without Kademlia/connection activity.
- Hosting: accepted agreements auto-seed files via `seed_hosted_file` command.
- Hosting: cancellation removes seeder from DHT and cleans up Drive.
- App shows a close confirmation dialog before quitting (wired in `src/App.svelte`).
- Wallet backup email step is optional (skip button) during wallet creation.
- Logout has 5s timeout on DHT stop + loading state to prevent hanging.
- Download cost: 0.01 CHI per MB. Platform fee: 0.5% on all transactions.
- Platform fee split: 99.5% to seller/burn, 0.5% to platform wallet.
- File metadata, seeder entries, folder manifests, site-directory entries, and the chunked-transfer FileInfo envelope are all ECDSA-signed by the publisher / seeder wallet. Readers verify before consuming and drop unsigned/invalid records — every Tauri publisher refuses to write a record unless `private_key` is provided.
- Payment verification: on-chain tx receipt checked before serving file chunks.
- CDN server at `130.245.173.73:9420` — always-on file hosting with market-based pricing.
- CDN pricing: `max(floor, median_peer_price × 1.2)` — adapts to marketplace.
- CDN upload requires on-chain payment verification (5% tolerance for rounding).
- CDN files re-seed to DHT on startup (15s delay after bootstrap).
- CDN expiration cleanup runs every 60s — removes expired files from disk + DHT.
- Download page queries CDN servers as fallback when DHT search times out.
- Stop seeding removes peer from DHT seeder list (not just local shared files).
- Wallet RPC reads (`get_wallet_balance`, `verify_tx_details`) walk an ordered fallback list via `rpc_client::call_with_fallbacks`: direct canonical Geth (port 8545) first, then the relay's `/api/chain/rpc` same-origin proxy on 8080. Either path can be down (firewall, crash, etc.) without taking the wallet offline. Write paths still pin to a single endpoint to avoid double-broadcast.
- Wallet UI surfaces RPC failures explicitly: `walletService.getBalance` populates a `walletBalanceError` Svelte store on failure (cleared on success), and the Account page renders a yellow "Balance may be stale — canonical RPC unreachable" banner with the underlying reason instead of silently rendering `0.00`. Mining page does the same with `get_mining_balance_diagnostic` — compares local-Geth balance against canonical-RPC balance for the miner address and renders an inline warning when the canonical RPC errors or the two diverge by more than ~0.001 CHI (private-fork diagnostic).
- Folder bundles: a folder is published under one content-addressed hash (SHA-256 of owner_wallet + sorted (rel_path, file_hash) list). DHT key `chiral_folder_<hash>`; seller also registers as Kademlia provider for the folder hash. Buyers' `search_folder` returns common-seeders intersection across the bundle.
- Version enforcement (`src-tauri/src/version.rs`): bundled `VersionPolicy` embedded at compile time; effective policy held in a global `OnceCell<RwLock<VersionPolicy>>` so libp2p Identify, HTTP middleware, and Tauri commands all read the same value. `/api/version-policy` is mounted by the gateway router (relay, daemon, desktop hosting). On startup the desktop fetches the relay's policy and promotes it via `update_effective_policy` if `is_acceptable_remote_policy` accepts (rollback by `issuedAt`, signed accept against `POLICY_PUBLIC_KEY`, or unsigned-not-tightening if no signed policy is in effect).
- Version enforcement layers: `UpdateGate.svelte` UI (soft banner / hard modal), `ensure_version_supported` Tauri gate, `X-Chiral-Client-Version` HTTP middleware (426 Upgrade Required below `minRequired`), libp2p Identify `agent_version="chiral/<v>"` with peer disconnect on mismatch.
- `chiral-policy-sign` operator binary: keygen / sign / verify policies with the project's offline Ed25519 key. The compile-time `POLICY_PUBLIC_KEY` is 32 zero bytes (placeholder); operators activate signed policies at deploy time by setting the `CHIRAL_POLICY_PUBLIC_KEY` env var (32-byte hex, with or without `0x` prefix). All three binaries (desktop, daemon, relay) print a one-shot `[VERSION]` line at startup confirming whether signed policies are enabled or warning that the placeholder is still in use.

## Owner-proof HTTP auth

Every authenticated HTTP route uses a stateless signed-challenge scheme (replaces the previously-trusted bare `X-Owner` header):

- Headers: `X-Owner: 0x<40 hex>` + `X-Owner-Sig: <unix_ts>:<hex_signature>`.
- Signed bytes: `auth::owner_proof_payload(wallet_lowercased, ts, method, path_and_query)` — length-prefixed canonical, tagged `chiral-owner-proof-v1`. Binds wallet ↔ exact request, so a captured proof can't be replayed against a different endpoint within its ±5-minute validity window.
- Server: `auth::owner_proof_middleware` recovers the secp256k1 signer and rejects with 401 on mismatch / expired / missing.
- Applied to: `/api/drive/*` (every route), `POST /api/ratings/transfer`, and the unregister DELETEs on relay register routes. Public visitor routes (`/drive/:token`, `/sites/:site_id`) stay unauthenticated.
- Tauri command: `compute_owner_proof(method, path, walletAddress, privateKey)` — frontend calls it before each authenticated fetch; private keys never leave the process.

## Trust boundary specifics

- **Folder bundles**: `FolderManifest` is signed by `owner_wallet`. `publish_drive_folder` skips the publish if the wallet is locked. `search_folder` drops unsigned/invalid manifests.
- **Chunked transfer**: `ChunkResponse::FileInfo` is signed by the seeder's wallet over `dht::file_info_sign_payload`. The downloader verifies before consuming `wallet_address` / `price_wei` and fails over to other seeders on signature failure. `SharedFileInfo` carries the seeder's `private_key`; if absent the seeder responder refuses to serve.
- **Drive shares**: `verify_share_access` binds each `tx_hash` to the first share token it unlocks via a persisted ledger at `<data_dir>/drive_share_spent_tx.json`. One tx ↔ one share forever; the buyer can reload their page, but the URL doesn't unlock unrelated shares.
- **Seeder spent-tx**: `claim_spent_tx(tx_hash, file_hash)` keys on the *(tx, file)* pair, so one payment to a wallet that seeds many priced files redeems exactly one delivery.
- **CDN payment math**: exact `u128` ceil-rounded `required_upload_wei`. No `f64`, no 5% tolerance — `min_accepted_wei == required_wei`.

## Relay register signing

Relay `register_share` / `register_site` POSTs require an ECDSA `signature` field over `relay_share_proxy::register_payload(operation, id, owner_wallet, origin_url)` (length-prefixed, tagged `chiral-relay-register-v1`). First-claim-wins is enforced cryptographically: an existing record can only be overwritten by the wallet that originally signed it. The relay also runs `is_safe_origin_url` to reject:

- non-http(s) schemes
- RFC1918 (10/8, 172.16/12, 192.168/16), CGNAT (100.64/10)
- link-local (169.254/16, fe80::/10) — blocks AWS / GCP cloud metadata IPs
- unique-local IPv6 (fc00::/7), multicast, broadcast, unspecified
- IPv4-mapped IPv6 variants of any of the above

Loopback (127.0.0.0/8, ::1) stays accepted because `fix_origin_url` substitutes the registrant's public IP at request time. The Tauri commands `publish_drive_share` / `publish_site_to_relay` (and unpublish counterparts) take `private_key`, sign locally, and refuse if the wallet is locked. New `compute_relay_register_signature` Tauri command for external callers.

## Other defense-in-depth

- `dht_put` headless route refuses keys in reserved namespaces (`chiral_file_*`, `chiral_seeder_*`, `chiral_folder_*`, `chiral_drive_share_*`, `chiral_host_ad_*`) with 403 — those records are written through their dedicated signed-publication commands.
- Local-daemon CORS allowlists Tauri webview origins only (relay-mode keeps `Any`); blocks CSRF from any visited webpage. `/api/drive/*` has `DefaultBodyLimit::max(500 MiB)`. `is_item_under_shared_root` tracks visited IDs to short-circuit parent cycles.
- `wallet::verify_tx_details` checks `tx.chainId == geth::chain_id()` so cross-chain replays of signed txs are rejected. `wallet::recover_signer` enforces low-`s` (EIP-2) so no caller using sig hex as a dedup key gets fooled by malleable variants.
- Chunked downloader rejects out-of-sequence chunks (`chunk_index != current_chunk_index`) before bandwidth waste; the full-file hash still backstops integrity.
- Seeder `PaymentProof` handler distinguishes "tx not yet mined" (retryable, the buyer is told to retry in 30s) from "wrong amount/recipient" (permanent). The spent-tx ledger only records on success, so the same `tx_hash` can be presented again safely.
