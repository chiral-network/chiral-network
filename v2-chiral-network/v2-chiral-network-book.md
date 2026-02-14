# The Chiral Network V2 Implementation Book

**A V2-specific technical manual based on the original Chiral Network book**

Version 2.0 (Codebase Snapshot)  
As of February 11, 2026

---

## Table of Contents

- [Part I: Scope and Current Snapshot](#part-i-scope-and-current-snapshot)
  - [Chapter 1: What V2 Is](#chapter-1-what-v2-is)
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
  - [Chapter 10: Upload](#chapter-10-upload)
  - [Chapter 11: Download](#chapter-11-download)
  - [Chapter 12: ChiralDrop](#chapter-12-chiraldrop)
- [Part V: Blockchain, Wallet, and Payments](#part-v-blockchain-wallet-and-payments)
  - [Chapter 13: Chain and Node Model](#chapter-13-chain-and-node-model)
  - [Chapter 14: Wallet UX and Account](#chapter-14-wallet-ux-and-account)
  - [Chapter 15: Payment Flows](#chapter-15-payment-flows)
- [Part VI: Security, Privacy, and Integrity](#part-vi-security-privacy-and-integrity)
  - [Chapter 16: Cryptographic and Signing Capabilities](#chapter-16-cryptographic-and-signing-capabilities)
  - [Chapter 17: Data Protection Status](#chapter-17-data-protection-status)
  - [Chapter 18: Security Gaps to Address](#chapter-18-security-gaps-to-address)
- [Part VII: UI and Application Surfaces](#part-vii-ui-and-application-surfaces)
  - [Chapter 19: Page-by-Page Status](#chapter-19-page-by-page-status)
  - [Chapter 20: Stores, Services, and Reactivity](#chapter-20-stores-services-and-reactivity)
- [Part VIII: Operations and Testing](#part-viii-operations-and-testing)
  - [Chapter 21: Build and Runtime Operations](#chapter-21-build-and-runtime-operations)
  - [Chapter 22: Test Coverage Status](#chapter-22-test-coverage-status)
  - [Chapter 23: Known Gaps and Next Work](#chapter-23-known-gaps-and-next-work)
- [Appendix A: Tauri Command Inventory](#appendix-a-tauri-command-inventory)
- [Appendix B: Major Differences from V1 Book](#appendix-b-major-differences-from-v1-book)

---

# Part I: Scope and Current Snapshot

## Chapter 1: What V2 Is

Chiral Network V2 is a rebuild of the original app with a tighter frontend/backend split and simpler feature paths.

Current implementation basis:

- Frontend: Svelte 5 + TypeScript (`v2-chiral-network/src`)
- Desktop/runtime: Tauri 2 (`v2-chiral-network/src-tauri`)
- Networking: libp2p Kademlia + mDNS + request/response protocols (`v2-chiral-network/src-tauri/src/dht.rs`)
- Chain integration: Core-Geth lifecycle + remote RPC balance/tx queries (`v2-chiral-network/src-tauri/src/geth.rs`, `v2-chiral-network/src-tauri/src/lib.rs`)

This document replaces v1 assumptions with what is actually present in the current v2 code.

## Chapter 2: Delivery Status Summary

Status legend:

- **Implemented**: present and wired end-to-end
- **Partial**: present, but with known functional gaps or stub behavior
- **Not Implemented**: missing from current code

High-level summary:

| Domain | Status | Notes |
|---|---|---|
| Wallet onboarding (create/import/verify) | Implemented | `Wallet.svelte`, `WalletCreation.svelte`, `WalletLogin.svelte` |
| Navbar, routing, auth gating | Implemented | `App.svelte`, `Navbar.svelte` |
| DHT connect/disconnect + peer visibility | Implemented | `dhtService.ts`, `Network.svelte`, Rust DHT commands |
| ChiralDrop map + transfer flow | Implemented (core), Partial (advanced) | interactive wave map and transfer flows exist |
| Upload protocols (WebRTC/BitTorrent selection) | Partial | selector exists; backend transport path does not switch by protocol |
| Download by hash/magnet/torrent + tracker UI | Implemented (core), Partial (controls) | pause/resume/cancel are UI-local only |
| In-app file preview for downloaded files | Implemented | `Download.svelte` + `assetProtocol` enabled |
| Account balance/send/history | Implemented | `Account.svelte`, wallet RPC + tx metadata |
| Default 1 CHR on wallet creation | Not Implemented | no wallet-creation credit path in UI/backend |
| Mining controls/history | Implemented | `Mining.svelte`, geth mining commands |
| Settings toggles (theme/storage) | Implemented | download directory and theme are active |
| Settings toggles (notification filtering, compact, reduced motion) | Partial | persisted in store; not enforced across feature code |
| Encryption workflow in UI | Partial | backend + service exist; not integrated into user pages |
| Reputation/dispute system | Not Implemented | no peer scoring/dispute flows in v2 |

## Chapter 3: Project Outline Compliance Matrix

Source requirement file: `v2-chiral-network/project-outline.md`

| Project-outline item | Status | Current implementation |
|---|---|---|
| Wallet create with 12-word phrase, copy/regenerate/download, verification quiz | Implemented | `src/lib/components/WalletCreation.svelte` |
| Existing wallet login by private key or mnemonic | Implemented | `src/lib/components/WalletLogin.svelte` |
| Navbar with Download/Upload/Account/Network/Settings + logout + connection indicator | Implemented (plus extra pages) | `src/lib/components/Navbar.svelte` |
| Network tab with peer visibility and connect/disconnect | Implemented | `src/pages/Network.svelte`, `src/lib/dhtService.ts` |
| ChiralDrop alias + map + click peer + transfer + accept/decline + persisted history | Implemented (core), Partial (location realism) | `src/pages/ChiralDrop.svelte`, `src/lib/chiralDropStore.ts`, `src/lib/encryptedHistoryService.ts` |
| Upload with WebRTC/BitTorrent options, file picker/drag-drop, upload history, remove | Partial | options and history exist, protocol selection not transport-enforced |
| Download by hash/magnet/torrent with status tracking + history | Implemented (core), Partial (pause/cancel controls) | `src/pages/Download.svelte` |
| Account page with balance/address/private key, tx history, send CHR | Implemented | `src/pages/Account.svelte` |

---

# Part II: Architecture

## Chapter 4: Runtime Architecture

V2 architecture is practical and page-driven:

- `App.svelte` handles auth gating and route selection.
- Frontend pages call Tauri commands through `invoke(...)`.
- Rust backend (`src-tauri/src/lib.rs`) is the orchestration layer for DHT, transfer, wallet RPC, Geth, mining, diagnostics, and encryption.

Primary state containers:

- App stores: `src/lib/stores.ts`
- DHT runtime wrapper: `src/lib/dhtService.ts`
- Wallet RPC wrapper: `src/lib/services/walletService.ts`
- ChiralDrop state/history: `src/lib/chiralDropStore.ts`

## Chapter 5: Data and Control Flows

Main flows in v2:

1. **Wallet/Auth flow**
   - wallet create/import in frontend only
   - auth state in Svelte stores

2. **Upload flow**
   - frontend picks file path -> `publish_file`
   - Rust computes SHA-256 hash, caches file data, writes metadata record in DHT

3. **Download flow**
   - frontend `search_file` -> receives metadata (hash/name/seeders/price)
   - `start_download` handles tier payment, seeder payment handshakes, chunked transfer, emits progress/complete events

4. **ChiralDrop flow**
   - peer-discovery events drive peer-map entries
   - free transfer uses direct file transfer request/accept
   - paid transfer uses metadata request + paid `start_download`

5. **Chain flow**
   - balances and transaction history queried from RPC endpoint
   - transaction metadata enrichment kept locally in app state (`tx_metadata`)

## Chapter 6: Protocol Reality vs V1 Design

Compared to the original book, v2 currently runs on these effective transfer paths:

- DHT + request/response custom protocols
- direct file transfer protocol (`/chiral/file-transfer/1.0.0`)
- chunked file request protocol (`/chiral/file-request/3.0.0`)

What is not present as true runtime transports in v2:

- standalone HTTP transfer path
- FTP/ed2k implementation
- explicit WebRTC transfer stack in backend
- true BitTorrent peer-wire/session engine in backend

The upload protocol selector (`WebRTC`/`BitTorrent`) is stored and displayed, but does not currently switch the backend transport implementation.

---

# Part III: Network and P2P Infrastructure

## Chapter 7: DHT and Peer Discovery

Implemented:

- Kademlia DHT record operations
- mDNS local discovery
- bootstrap dial + health checks
- peer list/event emission to frontend
- ping request/response protocol

Key files:

- `src-tauri/src/dht.rs`
- `src/lib/dhtService.ts`
- `src/pages/Network.svelte`

Current limitation:

- bootstrap node list is currently minimal (`get_bootstrap_nodes()` uses one node).

## Chapter 8: Connectivity and Traversal

Implemented:

- mDNS for local-peer discovery
- TCP + Noise + Yamux transport stack

Not implemented in v2 runtime code:

- AutoNAT v2
- UPnP/NAT-PMP automation
- dedicated relay strategy

These were stronger themes in v1 documentation but are not currently wired in v2 code.

## Chapter 9: Transfer Protocols

Implemented:

- direct request/response file transfer for free ChiralDrop flows
- chunked file download protocol with per-chunk hash validation and retry
- paid download path with payment proof + verification

Partial:

- multi-seeder behavior exists as retry/fallback by seeder list; no swarm-style concurrent multi-source piece download.

---

# Part IV: File Sharing Features

## Chapter 10: Upload

Implemented:

- desktop file picker and drag-drop
- SHA-256 hash publication
- DHT metadata registration
- upload history persistence (localStorage)
- magnet link generation and torrent export
- optional pricing metadata

Partial/gaps:

- removing from upload history does not expose an explicit backend unpublish/unregister command to retract DHT metadata immediately
- protocol selector is currently metadata/UI-level

Code:

- `src/pages/Upload.svelte`
- `src-tauri/src/lib.rs` (`publish_file`, `register_shared_file`, `export_torrent_file`)

## Chapter 11: Download

Implemented:

- search by hash
- magnet parsing
- `.torrent` parsing for Chiral-formatted torrents
- active/history tracking in UI
- speed-tier payment integration
- seeder payment workflow
- open file/show folder actions
- in-app preview modal for video/audio/image/pdf

Partial/gaps:

- pause/resume/cancel are UI state operations only (no backend cancel/pause commands)
- `queued` status exists in type model but is not a full backend queue implementation

Code:

- `src/pages/Download.svelte`
- `src-tauri/src/lib.rs` (`search_file`, `start_download`, `parse_torrent_file`)

## Chapter 12: ChiralDrop

Implemented:

- peer aliasing (color+animal)
- wave-map peer surface with clickable peers
- incoming transfer requests (accept/decline)
- paid transfer handshake path
- persisted transfer history with local cache + encrypted DHT sync

Partial/gaps:

- map coordinates are synthetic/randomized visualization, not geographic positions
- encryption service exists but is not integrated in ChiralDrop UI flow yet

Code:

- `src/pages/ChiralDrop.svelte`
- `src/lib/chiralDropStore.ts`
- `src/lib/encryptedHistoryService.ts`

---

# Part V: Blockchain, Wallet, and Payments

## Chapter 13: Chain and Node Model

Implemented:

- Geth install/download lifecycle
- start/stop node controls
- mining controls and status
- bootstrap-health diagnostics

Important v2 design detail:

- balance and transaction RPC use a shared endpoint (`rpc_endpoint()` defaults to `http://130.245.173.73:8545`) for canonical state visibility.
- local Geth is still used for local node operation and mining workflow.

Code:

- `src-tauri/src/geth.rs`
- `src/pages/Network.svelte`
- `src/pages/Mining.svelte`

## Chapter 14: Wallet UX and Account

Implemented:

- create wallet with mnemonic verification challenge
- login by private key or mnemonic
- address/private-key display with copy/hide controls
- CHR send modal with confirmation
- transaction history rendering with enriched metadata

Not implemented:

- automatic 1 CHR grant on wallet creation in UI flow
- faucet action is not surfaced in UI despite backend command availability

Code:

- `src/pages/Wallet.svelte`
- `src/lib/components/WalletCreation.svelte`
- `src/lib/components/WalletLogin.svelte`
- `src/pages/Account.svelte`
- `src-tauri/src/lib.rs` (`request_faucet`, `send_transaction`, `get_transaction_history`)

## Chapter 15: Payment Flows

Implemented:

- speed-tier payment (burn address)
- per-file seeder payments for paid downloads
- ChiralDrop paid transfer metadata and event handling
- metadata recording for account history context

Code:

- `src-tauri/src/lib.rs` (`start_download`, `send_transaction`, `record_transaction_meta`)
- `src/pages/Download.svelte`
- `src/pages/ChiralDrop.svelte`

---

# Part VI: Security, Privacy, and Integrity

## Chapter 16: Cryptographic and Signing Capabilities

Implemented:

- local transaction signing using secp256k1
- Keccak/RLP transaction serialization pipeline
- per-chunk SHA-256 integrity checks for chunked downloads
- encryption module and Tauri commands for E2E file encryption service

Code:

- `src-tauri/src/lib.rs`
- `src-tauri/src/encryption.rs`
- `src-tauri/src/dht.rs`
- `src/lib/services/encryptionService.ts`

## Chapter 17: Data Protection Status

Implemented:

- encrypted ChiralDrop history at-rest in DHT via AES-GCM (key derived from wallet private key)
- optional custom download directory controls

Partial:

- if wallet is unavailable, history falls back to plaintext localStorage (`chiraldrop_history_plain`)
- app notification preference toggles are not applied as global policy gates yet

## Chapter 18: Security Gaps to Address

Current high-priority gaps:

1. **Pause/cancel control coupling**
   - download cancellation is not propagated to backend transfer state.

2. **Upload unpublish semantics**
   - no explicit frontend command path to retract shared file metadata immediately when removed from history.

3. **Feature-toggle enforcement**
   - notification/reduced-motion/compact settings are stored but not globally enforced.

4. **Encryption UX integration**
   - encryption service exists but is not exposed in the primary send/download UX.

5. **Sensitive data handling hardening**
   - wallet private key is intentionally available in UI for export/use, but this increases exposure risk and should be paired with additional operational safeguards.

---

# Part VII: UI and Application Surfaces

## Chapter 19: Page-by-Page Status

| Page | Status | Notes |
|---|---|---|
| `Wallet.svelte` | Implemented | mode switch to create/login components |
| `Download.svelte` | Implemented (core), Partial (controls) | search + transfer + history + in-app preview; pause/cancel backend gap |
| `Upload.svelte` | Partial | robust UI flow; protocol switch not transport-enforced |
| `ChiralDrop.svelte` | Implemented (core), Partial | strong transfer flow; non-geographic map |
| `Account.svelte` | Implemented | send CHR, tx history enrichment, key controls |
| `Network.svelte` | Implemented (core), Partial | DHT + Geth controls/health; no geo distribution map |
| `Mining.svelte` | Implemented | thread controls, mining status, mined-block history |
| `Settings.svelte` | Partial | theme/storage active; several toggles not globally applied |
| `Diagnostics.svelte` | Implemented | DHT/bootstrap/geth/mining/log observability |

## Chapter 20: Stores, Services, and Reactivity

Implemented store/service layout:

- stores: auth, wallet, network, settings (`src/lib/stores.ts`)
- DHT abstraction (`src/lib/dhtService.ts`)
- wallet abstraction (`src/lib/services/walletService.ts`)
- Geth abstraction (`src/lib/services/gethService.ts`)
- encrypted history sync (`src/lib/encryptedHistoryService.ts`)

Partial behavior to track:

- settings flags for notifications/compact/reduced-motion currently have limited downstream enforcement.

---

# Part VIII: Operations and Testing

## Chapter 21: Build and Runtime Operations

Primary commands:

- `cd v2-chiral-network && npm run tauri:dev`
- `cd v2-chiral-network && npm run tauri:build`
- `cd v2-chiral-network && npm run build`
- `cd v2-chiral-network && npm test`

Tauri security/runtime note:

- local preview of downloaded files is enabled through asset protocol scope in `src-tauri/tauri.conf.json`.

## Chapter 22: Test Coverage Status

Frontend:

- active Vitest suite for stores/utilities/alias/bootstrap/encryption-service wrappers
- existing tests pass in current branch context

Backend:

- Rust module tests exist in multiple modules (`dht`, `encryption`, `geth`, `geth_bootstrap`, `file_transfer`)
- maintainers should run `cargo test` in `src-tauri` as part of release verification

Reference: `v2-chiral-network/tests/README.md`

## Chapter 23: Known Gaps and Next Work

Recommended next priorities:

1. Implement backend-backed download pause/resume/cancel semantics.
2. Add explicit unpublish/unregister command path from Upload UI.
3. Either implement true protocol differentiation (WebRTC vs BitTorrent) or relabel selector to reflect current runtime behavior.
4. Integrate encryption workflow in ChiralDrop send/receive UX.
5. Wire notification/compact/reduced-motion settings into actual app behavior.
6. Add network geographic visualization only if reliable peer location source and privacy policy are defined.
7. Expose faucet action in Account UI if dev workflow requires quick funding.

---

## Appendix A: Tauri Command Inventory

Domains currently exposed in `src-tauri/src/lib.rs`:

- DHT: `start_dht`, `stop_dht`, `get_dht_peers`, `get_network_stats`, `get_peer_id`, `get_dht_health`, `ping_peer`, `store_dht_value`, `get_dht_value`
- Transfer: `send_file`, `send_file_by_path`, `accept_file_transfer`, `decline_file_transfer`
- File/share/download: `publish_file`, `publish_file_data`, `search_file`, `start_download`, `calculate_download_cost`, `register_shared_file`, `parse_torrent_file`, `export_torrent_file`, `open_file`, `show_in_folder`
- File system + settings: `open_file_dialog`, `pick_download_directory`, `set_download_directory`, `get_download_directory`, `get_available_storage`, `get_file_size`
- Wallet/transactions: `get_wallet_balance`, `send_transaction`, `get_transaction_receipt`, `get_transaction_history`, `record_transaction_meta`, `request_faucet`, `get_chain_id`
- Geth/mining: `is_geth_installed`, `download_geth`, `start_geth`, `stop_geth`, `get_geth_status`, `start_mining`, `stop_mining`, `get_mining_status`, `get_mined_blocks`, `set_miner_address`
- Diagnostics/bootstrap: `read_geth_log`, `check_bootstrap_health`, `get_bootstrap_health`
- Encryption: `init_encryption_keypair`, `get_encryption_public_key`, `encrypt_file_for_recipient`, `decrypt_file_data`, `send_encrypted_file`, `publish_encryption_key`, `lookup_encryption_key`

## Appendix B: Major Differences from V1 Book

1. V1 documentation emphasized protocol plurality (HTTP/WebRTC/BitTorrent/ed2k/FTP).  
   V2 currently ships a simpler custom-libp2p request/response model.

2. V1 documentation emphasized broader NAT/privacy feature sets.  
   V2 currently ships mDNS + bootstrap + Kademlia without full AutoNAT/UPnP pipeline.

3. V1 had broader conceptual coverage of reputation/dispute architecture.  
   V2 does not currently implement a live reputation/dispute subsystem.

4. V2 adds practical, operator-focused pages not central in the original manual tone:
   - Mining control page
   - Diagnostics page
   - richer per-event logging surfaces

5. V2 includes direct in-app preview of downloaded files (video/audio/image/pdf) in the download experience.

