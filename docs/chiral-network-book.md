# The Chiral Network Implementation Book

**Technical manual for the Chiral Network application**

Version 3.0 (Codebase Snapshot)
As of March 3, 2026

---

## Table of Contents

- [Part I: Scope and Current Snapshot](#part-i-scope-and-current-snapshot)
  - [Chapter 1: What Chiral Network Is](#chapter-1-what-chiral-network-is)
  - [Chapter 2: Delivery Status Summary](#chapter-2-delivery-status-summary)
  - [Chapter 3: Project Outline Compliance Matrix](#chapter-3-project-outline-compliance-matrix)
- [Part II: Architecture](#part-ii-architecture)
  - [Chapter 4: Runtime Architecture](#chapter-4-runtime-architecture)
  - [Chapter 5: Data and Control Flows](#chapter-5-data-and-control-flows)
  - [Chapter 6: Protocol Reality vs V1 Design](#chapter-6-protocol-reality-vs-v1-design)
- [Part III: Network and P2P Infrastructure](#part-iii-network-and-p2p-infrastructure)
  - [Chapter 7: DHT and Peer Discovery](#chapter-7-dht-and-peer-discovery)
  - [Chapter 8: Connectivity and Traversal](#chapter-8-connectivity-and-traversal)
  - [Chapter 9: Transfer Protocols](#chapter-9-transfer-protocols)
- [Part IV: File Sharing Features](#part-iv-file-sharing-features)
  - [Chapter 10: Drive](#chapter-10-drive)
  - [Chapter 11: Download](#chapter-11-download)
  - [Chapter 12: ChiralDrop](#chapter-12-chiraldrop)
- [Part V: Blockchain, Wallet, and Payments](#part-v-blockchain-wallet-and-payments)
  - [Chapter 13: Chain and Node Model](#chapter-13-chain-and-node-model)
  - [Chapter 14: Wallet UX and Account](#chapter-14-wallet-ux-and-account)
  - [Chapter 15: Payment Flows](#chapter-15-payment-flows)
- [Part VI: Hosting and Sharing](#part-vi-hosting-and-sharing)
  - [Chapter 16: Hosting Marketplace](#chapter-16-hosting-marketplace)
  - [Chapter 17: Drive Sharing and Relay Proxy](#chapter-17-drive-sharing-and-relay-proxy)
  - [Chapter 18: Local Gateway Server](#chapter-18-local-gateway-server)
- [Part VII: Security, Privacy, and Integrity](#part-vii-security-privacy-and-integrity)
  - [Chapter 19: Cryptographic and Signing Capabilities](#chapter-19-cryptographic-and-signing-capabilities)
  - [Chapter 20: Data Protection Status](#chapter-20-data-protection-status)
  - [Chapter 21: Security Gaps to Address](#chapter-21-security-gaps-to-address)
- [Part VIII: UI and Application Surfaces](#part-viii-ui-and-application-surfaces)
  - [Chapter 22: Page-by-Page Status](#chapter-22-page-by-page-status)
  - [Chapter 23: Stores, Services, and Reactivity](#chapter-23-stores-services-and-reactivity)
- [Part IX: Operations and Testing](#part-ix-operations-and-testing)
  - [Chapter 24: Build and Runtime Operations](#chapter-24-build-and-runtime-operations)
  - [Chapter 25: Test Coverage Status](#chapter-25-test-coverage-status)
  - [Chapter 26: Known Gaps and Future Work](#chapter-26-known-gaps-and-future-work)
- [Appendix A: Tauri Command Inventory](#appendix-a-tauri-command-inventory)
- [Appendix B: Major Differences from V1](#appendix-b-major-differences-from-v1)

---

# Part I: Scope and Current Snapshot

## Chapter 1: What Chiral Network Is

Chiral Network is a decentralized peer-to-peer file sharing desktop application. The v1 codebase was fully removed (commit `e0f8579c`). The current codebase lives at the repository root.

Current implementation basis:

- Frontend: Svelte 5 + TypeScript (`src/`)
- Desktop/runtime: Tauri 2 (`src-tauri/`)
- Networking: libp2p 0.53 with Kademlia, mDNS, Circuit Relay v2, DCUtR, custom request/response protocols (`src-tauri/src/dht.rs`)
- Chain integration: Core-Geth lifecycle + remote RPC balance/tx queries (`src-tauri/src/geth.rs`, `src-tauri/src/lib.rs`)
- Local storage: Drive file management with Axum gateway server (`src-tauri/src/drive_storage.rs`, `src-tauri/src/drive_api.rs`)

## Chapter 2: Delivery Status Summary

Status legend:

- **Implemented**: present and wired end-to-end
- **Partial**: present, but with known functional gaps or stub behavior
- **Not Implemented**: missing from current code

High-level summary:

| Domain | Status | Notes |
|---|---|---|
| Wallet onboarding (create/import/verify) | Implemented | `Wallet.svelte`, `WalletCreation.svelte`, `WalletLogin.svelte` |
| Navbar/Sidebar, routing, auth gating | Implemented | `App.svelte`, `Navbar.svelte`, `Sidebar.svelte` |
| DHT connect/disconnect + peer visibility | Implemented | `dhtService.ts`, `Network.svelte`, Rust DHT commands |
| ChiralDrop map + transfer flow | Implemented | interactive wave map, free/paid transfers, encrypted history |
| Drive (local file storage + folder hierarchy) | Implemented | `Drive.svelte`, `drive_storage.rs`, `drive_api.rs` |
| Download by hash/magnet/torrent + tracker UI | Implemented (core), Partial (controls) | pause/resume/cancel are UI-local only |
| In-app file preview for downloaded files | Implemented | `Download.svelte` + `assetProtocol` enabled |
| Account balance/send/history + peer blacklist | Implemented | `Account.svelte`, wallet RPC + tx metadata |
| Hosting marketplace (advertise/browse/agree) | Implemented | `Hosting.svelte`, `Hosts.svelte`, DHT-based agreements |
| Mining controls/history | Implemented | `Mining.svelte`, geth mining commands |
| Settings (theme/storage/notifications/nav style) | Implemented | `Settings.svelte`, persisted to localStorage |
| Diagnostics | Implemented | `Diagnostics.svelte`, 5-category health checks |
| Encryption workflow in UI | Partial | backend + service exist; not integrated into user pages |
| Relay proxy for Drive shares | Implemented | `relay_share_proxy.rs`, WebSocket tunnel support |
| Circuit Relay v2 + DCUtR | Implemented | relay reservations, direct connection upgrade |
| Seeder multiaddress publishing | Implemented | seeders store listener addresses in DHT for direct dialing |
| Default 1 CHR on wallet creation | Not Implemented | no wallet-creation credit path in UI/backend |

## Chapter 3: Project Outline Compliance Matrix

Source requirement file: `docs/project-outline.md`

| Project-outline item | Status | Current implementation |
|---|---|---|
| Wallet create with 12-word phrase, copy/regenerate/download, verification quiz | Implemented | `src/lib/components/WalletCreation.svelte` |
| Existing wallet login by private key or mnemonic | Implemented | `src/lib/components/WalletLogin.svelte` |
| Navbar with pages + logout + connection indicator | Implemented (plus extra pages) | `src/lib/components/Navbar.svelte`, `Sidebar.svelte` |
| Network tab with peer visibility and connect/disconnect | Implemented | `src/pages/Network.svelte`, `src/lib/dhtService.ts` |
| ChiralDrop alias + map + click peer + transfer + accept/decline + persisted history | Implemented | `src/pages/ChiralDrop.svelte`, `src/lib/chiralDropStore.ts` |
| File sharing with file picker/drag-drop, history, remove | Implemented | `src/pages/Drive.svelte`, publish/unpublish commands |
| Download by hash/magnet/torrent with status tracking + history | Implemented (core), Partial (pause/cancel controls) | `src/pages/Download.svelte` |
| Account page with balance/address/private key, tx history, send CHR | Implemented | `src/pages/Account.svelte` |

---

# Part II: Architecture

## Chapter 4: Runtime Architecture

Architecture is page-driven with a clear frontend/backend split:

- `App.svelte` handles auth gating (unauthenticated routes redirect to `/wallet`) and route selection. Default authenticated route is `/network`.
- Frontend pages call Tauri commands through `invoke(...)`.
- Rust backend (`src-tauri/src/lib.rs`) is the orchestration layer for DHT, transfer, wallet RPC, Geth, mining, diagnostics, encryption, Drive storage, and hosting.
- An Axum HTTP server starts automatically on port 9419 for Drive API and hosted site serving.

Primary state containers:

- App stores: `src/lib/stores.ts` (auth, wallet, peers, network, settings, blacklist)
- DHT runtime wrapper: `src/lib/dhtService.ts`
- ChiralDrop state/history: `src/lib/chiralDropStore.ts`, `src/lib/encryptedHistoryService.ts`

## Chapter 5: Data and Control Flows

Main flows:

1. **Wallet/Auth flow** -- wallet create/import in frontend only; auth state in Svelte stores; wallet persisted to sessionStorage.

2. **Drive/Publish flow** -- files stored locally at `~/.local/share/chiral-network/chiral-drive/`; `publish_drive_file` computes SHA-256 hash, registers as shared file, writes metadata to DHT with seeder multiaddresses.

3. **Download flow** -- frontend `search_file` retrieves metadata (hash/name/seeders/price/multiaddrs); `start_download` resolves seeder addresses from DHT, handles tier payment, seeder payment, then initiates chunked transfer with progress events.

4. **ChiralDrop flow** -- peer-discovery events drive peer-map entries; free transfer uses direct file transfer request/accept; paid transfer publishes metadata to DHT and recipient initiates paid `start_download`.

5. **Chain flow** -- balances and transaction history queried from shared RPC endpoint (`130.245.173.73:8545`); transaction metadata enrichment kept locally.

6. **Hosting flow** -- hosts advertise capacity/pricing to DHT; proposers select files from Drive and create agreements stored in DHT + locally; hosts seed agreed files via chunked transfer protocol.

## Chapter 6: Protocol Reality vs V1 Design

Current effective transfer paths:

- DHT + request/response custom protocols over libp2p
- Direct file transfer protocol (`/chiral/file-transfer/1.0.0`) for ChiralDrop
- Chunked file request protocol (`/chiral/file-request/3.0.0`) for downloads
- Circuit Relay v2 for NAT traversal, DCUtR for direct connection upgrade

What is not present as true runtime transports:

- Standalone HTTP transfer path
- FTP/ed2k implementation
- Explicit WebRTC transfer stack in backend
- True BitTorrent peer-wire/session engine in backend

The upload protocol selector (`WebRTC`/`BitTorrent`) is stored and displayed, but does not currently switch the backend transport implementation.

---

# Part III: Network and P2P Infrastructure

## Chapter 7: DHT and Peer Discovery

Implemented:

- Kademlia DHT record operations (put/get) with JSON-serialized values
- mDNS local discovery (automatic connection on LAN)
- Bootstrap dial to 3 nodes + health checks
- Peer list/event emission to frontend (`peer-discovered`, `peer-expired`, `connection-established`, `connection-closed`)
- Custom ping/echo request/response protocols
- 10 sub-behaviours composed into `DhtBehaviour` via `#[derive(NetworkBehaviour)]`

Key files: `src-tauri/src/dht.rs`, `src/lib/dhtService.ts`, `src/pages/Network.svelte`

DHT key conventions:

| Key Pattern | Content |
|---|---|
| `chiral_file_{hash}` | File metadata with multi-seeder list |
| `chiral_host_{peer_id}` | Host advertisement |
| `chiral_host_registry` | List of hosting registry entries |
| `chiral_agreement_{id}` | Hosting agreement |
| `chiral_encryption_key_{peer_id}` | X25519 public key |

## Chapter 8: Connectivity and Traversal

Implemented:

- TCP + Noise (XX handshake) + Yamux transport stack
- mDNS for local-peer discovery
- Circuit Relay v2 client with relay reservations on bootstrap nodes
- DCUtR for direct connection upgrade through relay
- Seeder multiaddress storage in DHT for direct dialing
- Multi-strategy connection: direct dial from stored addresses, then relay fallback
- Dual-stack listening (IPv4 + IPv6) on random ports

Not implemented:

- AutoNAT v2
- UPnP/NAT-PMP automation

## Chapter 9: Transfer Protocols

Implemented:

- Direct request/response file transfer for free ChiralDrop flows (`/chiral/file-transfer/1.0.0`)
- Chunked file download protocol (`/chiral/file-request/3.0.0`) with:
  - 256 KB chunks
  - Per-chunk SHA-256 verification
  - Full-file hash verification on completion
  - Max 3 retries per chunk
  - Custom CBOR codec (1 MB request limit, 32 MB response limit)
- Paid download path with payment proof + payment acknowledgment
- Speed tier rate limiting (Standard: 1 MB/s, Premium: 5 MB/s, Ultra: unlimited)

Partial:

- Multi-seeder behavior exists as sequential retry/fallback by seeder list; no concurrent multi-source piece download.

---

# Part IV: File Sharing Features

## Chapter 10: Drive

Implemented:

- Local file storage with folder hierarchy at `~/.local/share/chiral-network/chiral-drive/`
- File upload via drag-and-drop or file picker (500 MB max per file)
- Folder creation, rename, delete, star
- Grid and list view modes with search filtering
- Publish files to DHT for seeding (registers shared file with multiaddresses)
- Export as `.torrent` file
- Token-based share links served by local gateway on port 9419
- Share link with optional password protection
- Relay proxying for NAT traversal of shared content

Code: `src/pages/Drive.svelte`, `src-tauri/src/drive_storage.rs`, `src-tauri/src/drive_api.rs`

## Chapter 11: Download

Implemented:

- Search by SHA-256 hash, magnet link, or `.torrent` file
- Peer selection modal showing available seeders with pricing
- Speed tier selection (Standard/Premium/Ultra) with cost calculation
- Active download tracking with real-time progress, speed, and ETA
- Download history
- In-app preview for images, video, audio, and PDF
- Open file / show in folder actions
- Seeder multiaddress resolution from DHT for direct peer dialing

Partial/gaps:

- Pause/resume/cancel are UI state operations only (no backend cancel/pause commands)
- `queued` status exists in type model but is not a full backend queue implementation

Code: `src/pages/Download.svelte`, `src-tauri/src/lib.rs` (`search_file`, `start_download`, `parse_torrent_file`)

## Chapter 12: ChiralDrop

Implemented:

- Peer aliasing (color+animal combination, changes per session)
- Animated wave-map peer visualization with clickable peers
- Free transfers via direct file transfer protocol
- Paid transfers via DHT metadata publish + paid download
- Incoming transfer requests with accept/decline modal
- Persisted transfer history with AES-GCM encryption (key derived from wallet private key)
- DHT-synced backup of encrypted history

Partial/gaps:

- Map coordinates are synthetic/randomized visualization, not geographic positions
- Encryption service exists but is not integrated in ChiralDrop UI send flow

Code: `src/pages/ChiralDrop.svelte`, `src/lib/chiralDropStore.ts`, `src/lib/encryptedHistoryService.ts`

---

# Part V: Blockchain, Wallet, and Payments

## Chapter 13: Chain and Node Model

Implemented:

- Geth install/download lifecycle (auto-download if not present)
- Start/stop node controls
- Mining controls with configurable thread count
- Hash rate, blocks found, and accumulated rewards display
- Bootstrap-health diagnostics (bootstrap node reachability checks)
- Geth log viewer

Important design detail:

- Balance and transaction RPC use a shared endpoint (`http://130.245.173.73:8545`) for canonical state visibility.
- Local Geth is still used for local node operation and mining workflow.

Code: `src-tauri/src/geth.rs`, `src-tauri/src/geth_bootstrap.rs`, `src/pages/Network.svelte`, `src/pages/Mining.svelte`

## Chapter 14: Wallet UX and Account

Implemented:

- Create wallet with BIP39 12-word mnemonic and verification quiz (2 randomly selected words)
- Login by private key or mnemonic phrase
- Address/private-key display with copy/hide controls
- CHI send modal with recipient address and confirmation
- Transaction history with enriched metadata (type labels: "Speed Tier Payment", "Seeder Payment", file names)
- Peer blacklist management (block specific peers by address)

Not implemented:

- Automatic 1 CHI grant on wallet creation in UI flow
- Faucet action is not surfaced in UI despite backend command availability

Code: `src/pages/Wallet.svelte`, `src/pages/Account.svelte`, `src/lib/components/WalletCreation.svelte`, `src/lib/components/WalletLogin.svelte`

## Chapter 15: Payment Flows

Implemented:

- Speed-tier payment (burn address) before download begins
- Per-file seeder payments for priced downloads (paid to seeder's wallet)
- ChiralDrop paid transfer metadata and event handling
- Transaction metadata recording for account history context

Speed tier costs:

| Tier | Cost per MB | Speed |
|---|---|---|
| Standard | 0.001 CHI | 1 MB/s |
| Premium | 0.005 CHI | 5 MB/s |
| Ultra | 0.01 CHI | Unlimited |

Code: `src-tauri/src/lib.rs` (`start_download`, `send_transaction`, `record_transaction_meta`), `src-tauri/src/speed_tiers.rs`

---

# Part VI: Hosting and Sharing

## Chapter 16: Hosting Marketplace

Implemented:

- Host advertisement publishing to DHT (capacity, pricing per MB per day, minimum deposit, accepted file types)
- Host registry discovery via `chiral_host_registry` DHT key
- Browse available hosts sortable by reputation, price, or storage
- Proposal creation: select files from Drive, specify duration, submit to DHT
- Host accept/decline of incoming proposals
- Active agreement tracking with status updates (proposed, accepted, active, cancelled)
- Automatic file seeding when agreement is active
- Agreement cleanup (unregister files, remove from DHT)

Agreements are stored both locally at `~/.local/share/chiral-network/agreements/` and in DHT under `chiral_agreement_{id}`. Local storage is checked first on read, with DHT fallback.

Code: `src/pages/Hosting.svelte`, `src/pages/Hosts.svelte`, `src-tauri/src/lib.rs`

## Chapter 17: Drive Sharing and Relay Proxy

Drive shares work through two mechanisms:

1. **Local gateway** -- Share links served directly from port 9419 using token-based URLs. Works when the sharer is directly reachable.

2. **Relay proxy** -- For NAT traversal, share metadata (token + origin URL) is published to a relay server. The relay either forwards requests through a WebSocket tunnel or reverse-proxies to the origin. URL rewriting replaces localhost/0.0.0.0 with the client's real IP.

Relay proxy flow:

- `publish_drive_share` POSTs token and local origin URL to relay
- Relay maintains `RelayShareRegistry` (persisted to disk)
- Access attempts: WebSocket tunnel first, then direct HTTP proxy, then offline error
- WebSocket tunnel carries `TunnelRequest`/`TunnelResponse` with base64 bodies, 30-second timeout

NAT limitation: Direct HTTP proxying requires the sharer's port 9419 to be reachable from the relay. WebSocket tunnel mode handles NAT'd hosts.

Code: `src-tauri/src/relay_share_proxy.rs`, `src-tauri/src/drive_api.rs`

## Chapter 18: Local Gateway Server

An Axum HTTP server starts on port 9419 during app setup (`lib.rs` Tauri `.setup()`).

Routes:

| Route | Purpose |
|---|---|
| `GET /health` | Health check |
| `GET /sites/{id}/*` | Serve hosted site files |
| `GET /api/drive/items` | List Drive items (X-Owner header required) |
| `POST /api/drive/folders` | Create folder |
| `POST /api/drive/upload` | Upload file (multipart, 500 MB max) |
| `PUT /api/drive/items/:id` | Update item |
| `POST /api/drive/shares` | Create share link |
| `GET /api/drive/shares/:token` | Access shared file |

Security: directory traversal protection rejects `..`, null bytes, leading `/` or `\`, and paths that escape the site directory after canonicalization. Returns 403 or 404 on violation.

Code: `src-tauri/src/hosting_server.rs`, `src-tauri/src/drive_api.rs`, `src-tauri/src/hosting.rs`

---

# Part VII: Security, Privacy, and Integrity

## Chapter 19: Cryptographic and Signing Capabilities

Implemented:

- Local transaction signing using secp256k1 with Keccak hashing and RLP encoding
- Per-chunk SHA-256 integrity checks for chunked downloads
- Full-file SHA-256 verification on download completion
- E2E file encryption: X25519 ECDH key agreement + AES-256-GCM, HKDF-SHA256 key derivation with `"chiral-network-v2-e2ee"` info string
- Encryption keypair derived deterministically from wallet private key via SHA-256
- Encryption public keys published to DHT for lookup by peer ID
- Noise protocol (XX handshake) for all libp2p transport connections

Code: `src-tauri/src/lib.rs`, `src-tauri/src/encryption.rs`, `src-tauri/src/dht.rs`

## Chapter 20: Data Protection Status

Implemented:

- Encrypted ChiralDrop history at-rest via AES-GCM (key derived from wallet private key)
- DHT-synced backup of encrypted history
- Custom download directory controls
- Password-protected Drive share links (SHA-256 hash of password)

Partial:

- If wallet is unavailable, history falls back to plaintext localStorage (`chiraldrop_history_plain`)
- App notification preference toggles are not applied as global policy gates yet

## Chapter 21: Security Gaps to Address

Current gaps (potential future work):

1. **Pause/cancel control coupling** -- download cancellation is not propagated to backend transfer state.

2. **Feature-toggle enforcement** -- notification/reduced-motion/compact settings are stored but not globally enforced.

3. **Encryption UX integration** -- encryption service exists but is not exposed in the primary send/download UX. Future work could integrate encrypted transfers in ChiralDrop.

4. **Sensitive data handling hardening** -- wallet private key is intentionally available in UI for export/use, increasing exposure risk.

5. **DHT record validation** -- no cryptographic proof that DHT records were published by the claimed peer. Future work could add signed DHT records.

6. **Relay trust model** -- relay proxy forwards requests without authentication. Future work could add relay authentication tokens.

---

# Part VIII: UI and Application Surfaces

## Chapter 22: Page-by-Page Status

| Page | Path | Status | Notes |
|---|---|---|---|
| `Wallet.svelte` | `/wallet` | Implemented | create/login with mnemonic verification |
| `Download.svelte` | `/download` | Implemented (core), Partial (controls) | search + transfer + history + preview; pause/cancel UI-only |
| `Drive.svelte` | `/drive` | Implemented | local storage, folders, publish, share, torrent export |
| `ChiralDrop.svelte` | `/chiraldrop` | Implemented | wave map, free/paid transfers, encrypted history |
| `Account.svelte` | `/account` | Implemented | send CHI, tx history, key controls, peer blacklist |
| `Network.svelte` | `/network` | Implemented | DHT + Geth controls, peer list, health, bootstrap status |
| `Hosting.svelte` | `/hosting` | Implemented | host advertisement, agreement management |
| `Hosts.svelte` | `/hosts` | Implemented | browse hosts, create proposals |
| `Mining.svelte` | `/mining` | Implemented | thread controls, mining status, mined-block history |
| `Settings.svelte` | `/settings` | Implemented | theme, nav style, storage, notifications |
| `Diagnostics.svelte` | `/diagnostics` | Implemented | 5-category health checks, Geth log viewer |

Navigation is rendered by either `Navbar.svelte` (top bar) or `Sidebar.svelte` (left panel), configurable in Settings.

## Chapter 23: Stores, Services, and Reactivity

Implemented store/service layout:

- Core stores: auth, wallet, peers, network status, settings, blacklist (`src/lib/stores.ts`)
- DHT abstraction (`src/lib/dhtService.ts`)
- ChiralDrop state/encrypted history (`src/lib/chiralDropStore.ts`, `src/lib/encryptedHistoryService.ts`)

Settings persist to localStorage. Wallet persists to sessionStorage. Peer lists and network stats are ephemeral from backend events.

---

# Part IX: Operations and Testing

## Chapter 24: Build and Runtime Operations

Primary commands:

- `npm run tauri:dev` -- desktop app with hot reload
- `npm run tauri:build` -- production desktop build
- `npm run build` -- web production build
- `npm test` -- run Vitest frontend tests
- `cargo build --lib` (in `src-tauri/`) -- compile Rust backend only
- `cargo test` (in `src-tauri/`) -- run Rust tests

Tauri security/runtime note: local preview of downloaded files is enabled through asset protocol scope in `src-tauri/tauri.conf.json`.

## Chapter 25: Test Coverage Status

Frontend:

- Active Vitest suite for stores, utilities, alias generation, bootstrap, encryption-service wrappers
- 313 tests passing as of March 2026

Backend:

- Rust module tests in `dht`, `encryption`, `geth`, `geth_bootstrap`, `file_transfer`, `speed_tiers`, `drive_storage`
- 153 tests passing as of March 2026
- Some tests require network (DHT node spawn, bootstrap health) and may fail in offline environments

## Chapter 26: Known Gaps and Future Work

Recommended next priorities:

1. Implement backend-backed download pause/resume/cancel semantics.
2. Either implement true protocol differentiation (WebRTC vs BitTorrent transport) or relabel selector to reflect current runtime behavior.
3. Integrate encryption workflow in ChiralDrop send/receive UX.
4. Wire notification/compact/reduced-motion settings into actual app behavior.
5. Add network geographic visualization only if reliable peer location source and privacy policy are defined.
6. Expose faucet action in Account UI if dev workflow requires quick funding.
7. Add signed DHT records for verifiable metadata provenance.
8. Implement relay authentication to prevent unauthorized proxying.
9. Add concurrent multi-source downloads (parallel chunk fetching from multiple seeders).
10. Implement reputation/trust scoring for peers based on transfer history.

---

## Appendix A: Tauri Command Inventory

95 commands exposed in `src-tauri/src/lib.rs`:

- DHT/Network: `start_dht`, `stop_dht`, `get_dht_peers`, `get_network_stats`, `get_peer_id`, `get_dht_health`, `get_bootstrap_peer_ids`, `ping_peer`, `echo_peer`, `store_dht_value`, `get_dht_value`
- P2P Transfer: `send_file`, `send_file_by_path`, `accept_file_transfer`, `decline_file_transfer`, `send_encrypted_file`
- File Publishing: `publish_file`, `publish_file_data`, `search_file`, `register_shared_file`, `republish_shared_file`, `unpublish_all_shared_files`
- Downloads: `start_download`, `calculate_download_cost`
- Torrent: `parse_torrent_file`, `export_torrent_file`
- File System: `open_file`, `show_in_folder`, `show_drive_item_in_folder`, `open_file_dialog`, `pick_download_directory`, `set_download_directory`, `get_download_directory`, `get_available_storage`, `get_file_size`, `exit_app`
- Wallet/Blockchain: `get_wallet_balance`, `send_transaction`, `get_transaction_receipt`, `get_transaction_history`, `record_transaction_meta`, `request_faucet`, `get_chain_id`
- Geth/Mining: `is_geth_installed`, `download_geth`, `start_geth`, `stop_geth`, `get_geth_status`, `start_mining`, `stop_mining`, `get_mining_status`, `get_mined_blocks`, `set_miner_address`, `read_geth_log`, `check_bootstrap_health`, `get_bootstrap_health`
- Encryption: `init_encryption_keypair`, `get_encryption_public_key`, `encrypt_file_for_recipient`, `decrypt_file_data`, `publish_encryption_key`, `lookup_encryption_key`
- Hosting Marketplace: `publish_host_advertisement`, `unpublish_host_advertisement`, `get_host_registry`, `get_host_advertisement`, `store_hosting_agreement`, `get_hosting_agreement`, `list_hosting_agreements`, `get_active_hosted_files`, `cleanup_agreement_files`
- Hosted Sites: `create_hosted_site`, `list_hosted_sites`, `delete_hosted_site`, `start_hosting_server`, `stop_hosting_server`, `get_hosting_server_status`, `publish_site_to_relay`, `unpublish_site_from_relay`
- Drive Storage: `drive_list_items`, `drive_list_all_items`, `drive_create_folder`, `drive_upload_file`, `drive_update_item`, `drive_delete_item`, `drive_toggle_visibility`, `drive_create_share`, `drive_revoke_share`, `drive_list_shares`, `publish_drive_file`, `drive_stop_seeding`, `drive_export_torrent`, `get_drive_server_url`, `publish_drive_share`, `unpublish_drive_share`

See `docs/backend-api.md` for full signatures and return types.

## Appendix B: Major Differences from V1

1. V1 emphasized protocol plurality (HTTP/WebRTC/BitTorrent/ed2k/FTP). Current codebase ships custom libp2p request/response protocols only.

2. V1 emphasized broader NAT/privacy features (AutoNAT v2, UPnP, SOCKS5). Current codebase implements Circuit Relay v2 + DCUtR + seeder multiaddress publishing for NAT traversal.

3. V1 had broader conceptual coverage of reputation/dispute architecture. Not currently implemented; listed as future work.

4. V1 had no Drive, Hosting, or Hosts features. These are entirely new in the current codebase.

5. V1 codebase has been fully removed (commit `e0f8579c`). All paths reference the repository root, not `v2-chiral-network/`.

6. Current codebase adds: relay share proxying with WebSocket tunnels, encrypted ChiralDrop history with DHT sync, local Axum gateway server on port 9419, Drive file management with folder hierarchy, hosting marketplace with DHT-based agreements.
