# Migration: File Sharing → Resource Exchange

This repo is pivoting from a decentralized **file-sharing** app to a general
**resource exchange** (storage / container / LLM). The target design is
`docs/chiral-book.md`. This document tracks the migration: which legacy modules
are superseded, by what, and the safe order of deprecation.

## Guiding principle

**Do not remove legacy code until the new design replaces its function and works
end to end.** The file-sharing backend is live and actively hardened; deleting
it before the resource-exchange path is wired would break the running network.
Deprecation is therefore *sequenced after* the new modules are wired into the
DHT / HTTP layers — not done up front. Each row below has a **removal gate**.

## New-design engine modules (implemented + unit-tested)

Landed on branch `feat/resource-exchange-v1` (PR #1188), all covered by tests
(`cargo test --manifest-path src-tauri/Cargo.toml --lib`):

| Module | Role |
|---|---|
| `resource_offer.rs` | Signed multi-class offer record (`chiral-offer-v1`), `offer_ref`, DHT key |
| `codec.rs` | Shared canonical encoding (keccak256, LE length prefix, canonical JSON) |
| `service_contract.rs` | Contract-tx `data` codec (`CHR1`/`CHR2`) + `ContractTerms` / `terms_hash` |
| `contract_ledger.rs` | Prepaid-balance accounting (open / top-up / drawdown, fee cut, replay guard) |
| `usage_receipt.rs` | Wallet-signed usage receipts (`chiral-receipt-v1`) |

**Not yet built (wiring):** session-credential store; offer publish/search over
the DHT; handshake HTTP handlers (`/v1/contracts/*`); the three data-plane
servers (S3 / container / LLM); the payment-gated reputation `feedback` endpoint.
Until these exist, nothing below is removable.

## Legacy → replacement map

| Legacy (file-sharing) | Superseded by | Removal gate |
|---|---|---|
| Host advertisement (`host_advertisement_payload`, `hosting/publish-ad`, host registry) | `resource_offer` (typed, multi-class) | offer publish/search wired |
| Chunked file transfer (`file_transfer.rs`, `dht` chunk protocol, `ChunkResponse::FileInfo`) | S3 storage data-plane | storage provider serves objects |
| BitTorrent / magnet / `.torrent` UI scaffolding (Download / Drive Svelte) | hash-addressed S3 objects | safe now (inert per book) — UX change, coordinate |
| CDN service (`cdn_server.rs`, `cdn/*`) | Provider offers + contracts | providers replace always-on CDN |
| Drive HTTP shares (`relay_share_proxy.rs`, drive share links) | S3 presigned / public-read | storage data-plane ships |
| Per-download payment (per-file price path in `dht.rs`) | Contract prepaid balance (`contract_ledger`) | settlement handlers wired |
| Reputation transfer-outcome events | Payment-gated feedback (contract-keyed) | `feedback` endpoint ships |

## Reused as-is (not deprecated)

Wallet + on-chain tx verification, `rpc_client`, Geth/mining, DHT transport +
Kademlia, version enforcement, the Elo reputation engine, the headless
daemon / CLI / relay scaffolding, and `speed_tiers::split_payment` (the platform
fee — single source of truth, reused by `contract_ledger`).
