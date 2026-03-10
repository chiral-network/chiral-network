# Networking

## libp2p Surface

Networking is implemented in `src-tauri/src/dht.rs`.

Core behaviours:

- Kademlia DHT
- mDNS discovery
- Relay client + DCUtR
- Identify + ping
- Custom request/response protocols:
  - `/chiral/file-transfer/1.0.0`
  - `/chiral/file-request/3.0.0`
  - `/chiral/ping/1.0.0`
  - `/chiral/echo/1.0.0`

Transport uses TCP + Noise + Yamux.

## Bootstrap / Relay

Bootstrap peer IDs and multiaddrs are configured in backend code and exposed through `get_bootstrap_peer_ids`.

The node can:

- dial bootstrap peers directly,
- maintain relay reservations,
- attempt direct upgrade paths through DCUtR.

## File Discovery

File records are stored in DHT keys named `chiral_file_<hash>`.

Metadata includes:

- `hash`, `fileName`, `fileSize`
- per-seeder entries (`peerId`, `priceWei`, `walletAddress`, `multiaddrs`)

Downloaders use this metadata to select seeders and dial addresses.

## Download Flow (High-Level)

1. Query metadata with `search_file`.
2. Start transfer with `start_download`.
3. Request file info/chunks over `/chiral/file-request/3.0.0`.
4. Verify chunk hashes and final file hash.
5. Emit progress/completion events to frontend.

The app also includes local short-circuit paths for same-node shared files to avoid unnecessary relay retries.

## Drive Seeding and Startup Recovery

Drive-published files are registered as shared files in the DHT service. On DHT startup, backend reseeds persisted Drive files marked as seeding (including nested-folder files), restoring discoverability after app restart.

## Key DHT Namespaces

- `chiral_file_<hash>`: file metadata + seeders
- `chiral_host_<peer_id>`: host advertisement
- `chiral_host_registry`: discovered host index
- `chiral_agreement_<id>`: hosting agreements
- `chiral_encryption_key_<peer_id>`: encryption public key
