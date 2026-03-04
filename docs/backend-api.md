# Backend API

All frontend-to-backend communication goes through Tauri `invoke()` calls. Each command is an async Rust function in `src-tauri/src/lib.rs` unless noted otherwise. Return types are `Result<T, String>` unless noted.

---

## DHT and Network

| Command | Parameters | Returns |
|---------|-----------|---------|
| `start_dht` | app, state | `String` (peer ID) |
| `stop_dht` | state | `()` |
| `get_dht_peers` | state | `Vec<PeerInfo>` |
| `get_network_stats` | state | `NetworkStats` |
| `get_dht_health` | state | `DhtHealthInfo` |
| `get_peer_id` | state | `Option<String>` |
| `get_bootstrap_peer_ids` | none | `Vec<String>` (sync) |
| `ping_peer` | state, peer_id | `String` (latency) |
| `echo_peer` | state, peer_id, payload | `Vec<u8>` |
| `store_dht_value` | state, key, value | `()` |
| `get_dht_value` | state, key | `Option<String>` |

## File Publishing and Search

| Command | Parameters | Returns |
|---------|-----------|---------|
| `publish_file` | state, file_path, file_name, protocol?, price_chi?, wallet_address? | `PublishResult` |
| `publish_file_data` | state, file_name, file_data, price_chi?, wallet_address? | `PublishResult` |
| `search_file` | state, file_hash | `Option<SearchResult>` |
| `register_shared_file` | state, file_hash, file_path, file_name, file_size, price_chi?, wallet_address? | `()` |
| `republish_shared_file` | state, file_hash, file_path, file_name, file_size, price_chi?, wallet_address? | `()` |
| `unpublish_all_shared_files` | state | `u32` (count removed) |

`PublishResult` contains `hash`, `file_name`, `file_size`, and `magnet_link`. `SearchResult` contains `hash`, `file_name`, `file_size`, `protocol`, `seeders` (list of `SeederInfo` with peer_id, price_wei, wallet_address, multiaddrs).

## Downloads

| Command | Parameters | Returns |
|---------|-----------|---------|
| `start_download` | app, state, file_hash, file_name, seeders, speed_tier, file_size, wallet_address?, private_key?, seeder_price_wei?, seeder_wallet_address? | `DownloadStartResult` |
| `calculate_download_cost` | speed_tier, file_size | `DownloadCostResult` |

`start_download` orchestrates the full download: resolves seeder multiaddresses from DHT, sends speed tier payment if applicable, sends seeder payment if priced, then initiates chunked transfer. Emits `download-started`, `file-download-progress`, `file-download-complete`, or `file-download-failed` events.

## P2P File Transfer (ChiralDrop)

| Command | Parameters | Returns |
|---------|-----------|---------|
| `send_file` | state, peer_id, file_name, file_data, transfer_id, price_wei?, sender_wallet?, file_hash?, file_size? | `()` |
| `send_file_by_path` | state, peer_id, file_path, transfer_id, price_wei?, sender_wallet?, file_hash? | `()` |
| `accept_file_transfer` | app, state, transfer_id | `String` (saved path) |
| `decline_file_transfer` | state, transfer_id | `()` |
| `send_encrypted_file` | state, peer_id, file_name, file_data, recipient_public_key, transfer_id | `()` |

## Wallet and Blockchain

| Command | Parameters | Returns |
|---------|-----------|---------|
| `get_wallet_balance` | address | `WalletBalanceResult` |
| `send_transaction` | from_address, to_address, amount, private_key | `SendTransactionResult` |
| `get_transaction_receipt` | tx_hash | `Option<Value>` |
| `get_transaction_history` | state, address | `TransactionHistoryResult` |
| `record_transaction_meta` | state, tx_hash, tx_type, description, recipient_label?, balance_before?, balance_after? | `()` |
| `request_faucet` | address | `SendTransactionResult` |
| `get_chain_id` | none | `u64` (sync) |

Transaction metadata types: `send`, `receive`, `speed_tier_payment`, `faucet`. Metadata enriches the transaction history display with human-readable labels and file context.

## Geth and Mining

| Command | Parameters | Returns |
|---------|-----------|---------|
| `is_geth_installed` | none | `bool` |
| `download_geth` | app | `()` |
| `start_geth` | state, miner_address? | `()` |
| `stop_geth` | state | `()` |
| `get_geth_status` | state | `GethStatus` |
| `start_mining` | state, threads? | `()` |
| `stop_mining` | state | `()` |
| `get_mining_status` | state | `MiningStatus` |
| `get_mined_blocks` | state, max_blocks? | `Vec<MinedBlock>` |
| `set_miner_address` | state, address | `()` |
| `read_geth_log` | lines? | `String` |
| `check_bootstrap_health` | none | `BootstrapHealthReport` |
| `get_bootstrap_health` | none | `Option<BootstrapHealthReport>` |

## Encryption

| Command | Parameters | Returns |
|---------|-----------|---------|
| `init_encryption_keypair` | state, wallet_private_key | `String` (public key hex) |
| `get_encryption_public_key` | state | `Option<String>` |
| `encrypt_file_for_recipient` | recipient_public_key, file_data | `EncryptedFileBundle` |
| `decrypt_file_data` | state, encrypted_bundle | `Vec<u8>` |
| `publish_encryption_key` | state | `()` |
| `lookup_encryption_key` | state, peer_id | `Option<String>` |

`EncryptedFileBundle` contains `ephemeral_public_key`, `ciphertext`, and `nonce`, all hex-encoded. Keys are derived deterministically from the wallet private key via SHA-256 into X25519.

## Drive Storage

| Command | Parameters | Returns |
|---------|-----------|---------|
| `drive_list_items` | state, owner, parent_id? | `Vec<DriveItem>` |
| `drive_list_all_items` | state, owner | `Vec<DriveItem>` |
| `drive_create_folder` | state, owner, name, parent_id? | `DriveItem` |
| `drive_upload_file` | state, owner, file_path, parent_id?, merkle_root? | `DriveItem` |
| `drive_update_item` | state, owner, item_id, name?, parent_id?, starred? | `DriveItem` |
| `drive_delete_item` | state, owner, item_id | `()` |
| `drive_toggle_visibility` | state, owner, item_id, is_public | `DriveItem` |
| `drive_create_share` | state, owner, item_id, password?, is_public? | `Value` |
| `drive_revoke_share` | state, token | `()` |
| `drive_list_shares` | state | `Vec<Value>` |
| `publish_drive_file` | state, owner, item_id, protocol?, price_chi?, wallet_address? | `DriveItem` |
| `drive_stop_seeding` | state, owner, item_id | `DriveItem` |
| `drive_export_torrent` | state, owner, item_id | `String` (path) |
| `get_drive_server_url` | state | `Option<String>` |
| `publish_drive_share` | state, share_token, relay_url, owner_wallet | `()` |
| `unpublish_drive_share` | state, share_token, relay_url | `()` |

`DriveItem` fields: id, name, item_type ("file"/"folder"), parent_id, size, mime_type, created_at, modified_at, starred, storage_path, owner, is_public, merkle_root, protocol, price_chi, seeding.

## Hosting

| Command | Parameters | Returns |
|---------|-----------|---------|
| `publish_host_advertisement` | state, advertisement_json | `()` |
| `unpublish_host_advertisement` | state | `()` |
| `get_host_registry` | state | `String` (JSON) |
| `get_host_advertisement` | state, peer_id | `Option<String>` |
| `store_hosting_agreement` | state, agreement_id, agreement_json | `()` |
| `get_hosting_agreement` | state, agreement_id | `Option<String>` |
| `list_hosting_agreements` | none | `Vec<String>` |
| `get_active_hosted_files` | state | `Vec<Value>` |
| `cleanup_agreement_files` | state, agreement_id | `()` |

Hosting agreements are stored both locally at `~/.local/share/chiral-network/agreements/` and in DHT under key `chiral_agreement_{id}`. Local storage is checked first on read, with DHT fallback.

## Hosted Sites and Relay

| Command | Parameters | Returns |
|---------|-----------|---------|
| `create_hosted_site` | state, name, file_paths | `HostedSite` |
| `list_hosted_sites` | state | `Vec<HostedSite>` |
| `delete_hosted_site` | state, site_id | `()` |
| `start_hosting_server` | state, port | `String` (URL) |
| `stop_hosting_server` | state | `()` |
| `get_hosting_server_status` | state | `HostingServerStatus` |
| `publish_site_to_relay` | state, site_id, relay_url | `String` (public URL) |
| `unpublish_site_from_relay` | state, site_id | `()` |

## File System and UI

| Command | Parameters | Returns |
|---------|-----------|---------|
| `open_file` | path | `()` |
| `show_in_folder` | path | `()` |
| `show_drive_item_in_folder` | state, owner, item_id | `()` |
| `open_file_dialog` | multiple | `Vec<String>` |
| `pick_download_directory` | none | `Option<String>` |
| `set_download_directory` | state, path? | `()` |
| `get_download_directory` | state | `String` |
| `get_available_storage` | none | `u64` (bytes) |
| `get_file_size` | file_path | `u64` |
| `exit_app` | app | void |

## Torrent

| Command | Parameters | Returns |
|---------|-----------|---------|
| `parse_torrent_file` | file_path | `TorrentInfo` |
| `export_torrent_file` | file_hash, file_name, file_size, file_path | `ExportTorrentResult` |

---

## DhtService Public Methods

The `DhtService` struct in `dht.rs` is the bridge between Tauri command handlers and the libp2p swarm. Commands are sent via an MPSC channel; responses come back through oneshot channels.

| Method | Purpose |
|--------|---------|
| `new(file_transfer_service, download_tiers, download_directory, download_credentials)` | Constructor |
| `start(app)` | Spawn swarm task, connect to bootstrap nodes |
| `stop()` | Shut down swarm |
| `is_running()` | Check swarm status |
| `get_peers()` | List connected peers |
| `get_stats()` | Network statistics |
| `get_peer_id()` | Local peer ID |
| `get_health()` | DHT health info |
| `ping_peer(peer_id, app)` | Ping a specific peer |
| `send_file(peer_id, transfer_id, file_name, file_data, ...)` | Direct file transfer (ChiralDrop) |
| `request_file(peer_id, file_hash, request_id, multiaddrs)` | Initiate chunked download |
| `put_dht_value(key, value)` | Store record in DHT |
| `get_dht_value(key)` | Retrieve record from DHT |
| `is_peer_connected(peer_id)` | Check connection status |
| `echo(peer_id, payload)` | Echo test |
| `register_shared_file(hash, path, name, size, price_wei, wallet)` | Register file for seeding |
| `unregister_shared_file(hash)` | Stop seeding a file |
| `get_shared_files()` | List registered shared files |
| `get_listening_addresses()` | Get swarm listener multiaddresses |

## DHT Key Conventions

| Key Pattern | Content |
|-------------|---------|
| `chiral_file_{hash}` | `FileMetadata` JSON with seeders list |
| `chiral_host_{peer_id}` | Host advertisement JSON |
| `chiral_host_registry` | `Vec<HostRegistryEntry>` JSON |
| `chiral_agreement_{id}` | Hosting agreement JSON |
| `chiral_encryption_key_{peer_id}` | X25519 public key hex |

## Tauri Events (Backend to Frontend)

| Event | Payload | Source |
|-------|---------|--------|
| `peer-discovered` | peer info | mDNS/Kademlia discovery |
| `peer-expired` | peer_id | mDNS expiry |
| `connection-established` | peer_id, address | swarm event |
| `connection-closed` | peer_id | swarm event |
| `download-started` | request_id, file_hash | start_download |
| `file-download-progress` | request_id, file_hash, bytes_written, total_bytes, progress | chunk handler |
| `file-download-complete` | request_id, file_hash, file_path | download completion |
| `file-download-failed` | request_id, file_hash, error | download failure |
| `file-transfer-request` | transfer_id, peer_id, file_name, file_size, price_wei | incoming transfer |
| `file-transfer-complete` | transfer_id | transfer saved |
| `file-received` | transfer_id, file_name, file_path | file written to disk |
| `speed-tier-payment-complete` | tx_hash, tier | payment confirmed |
| `geth-download-progress` | progress, total | Geth binary download |
