# Application Pages

## Routing

The application uses `@mateothegreat/svelte5-router` for client-side routing. `App.svelte` defines two route sets: unauthenticated routes (wallet login) and authenticated routes (all other pages). When not authenticated, all paths redirect to `/wallet`. The default authenticated route is `/network`.

Navigation is rendered by either `Navbar.svelte` (top bar) or `Sidebar.svelte` (left panel), configurable in Settings.

---

## Wallet

**Path:** `/wallet`
**Source:** `src/pages/Wallet.svelte`
**Auth required:** No

Entry point for the application. Presents two options: create a new wallet or log in with an existing one. Wallet creation generates a BIP39 12-word mnemonic phrase with options to copy, regenerate, or download. A verification quiz requires the user to confirm two randomly selected words before proceeding. Login accepts either a private key or mnemonic phrase. Wallet state is stored in the `walletAccount` Svelte store and persisted to sessionStorage.

**Components:** `WalletCreation.svelte`, `WalletLogin.svelte`

---

## Download

**Path:** `/download`
**Source:** `src/pages/Download.svelte`

File download management. Users search for files by SHA-256 hash, magnet link, or `.torrent` file. Search results show file metadata and available seeders with their pricing. A peer selection modal lets users choose which seeder to download from and select a speed tier (Free, Standard, Premium).

Active downloads show real-time progress with speed and ETA. Completed downloads support in-app preview for images, video, audio, and PDF. Downloads can be opened in the system file manager. A history section tracks all past downloads.

**Backend commands:** `search_file`, `start_download`, `calculate_download_cost`, `parse_torrent_file`, `open_file`, `show_in_folder`
**Events listened:** `download-started`, `file-download-progress`, `file-download-complete`, `file-download-failed`, `speed-tier-payment-complete`

---

## Drive

**Path:** `/drive`
**Source:** `src/pages/Drive.svelte`

Local file storage with a folder hierarchy. Files are stored at `~/.local/share/chiral-network/chiral-drive/`. Users can create folders, upload files via drag-and-drop or file picker, rename items, and delete items. Supports grid and list view modes with search filtering.

Files can be published to the DHT for seeding, exported as `.torrent` files, or shared via token-based links. Sharing works through the local gateway server on port 9419, with optional relay proxying for NAT traversal. Each shared file gets a unique token URL.

**Backend commands:** `drive_list_items`, `drive_create_folder`, `drive_upload_file`, `drive_update_item`, `drive_delete_item`, `drive_create_share`, `drive_revoke_share`, `drive_list_shares`, `publish_drive_file`, `drive_stop_seeding`, `drive_export_torrent`, `get_drive_server_url`, `publish_drive_share`

---

## ChiralDrop

**Path:** `/chiraldrop`
**Source:** `src/pages/ChiralDrop.svelte`

Direct peer-to-peer file transfer between locally discovered peers. Each peer gets a randomly generated alias (color + animal combination) that changes per session. Peers appear on an animated wave visualization. Clicking a peer opens a transfer dialog.

Transfers can be free or paid. For free transfers, the file is sent directly via the file transfer protocol. For paid transfers, file metadata is published to DHT and the recipient initiates a paid download. Incoming transfer requests show a modal with accept/decline options. Transfer history persists using AES-GCM encryption keyed to the wallet private key, with DHT-synced backup.

**Backend commands:** `send_file`, `send_file_by_path`, `accept_file_transfer`, `decline_file_transfer`, `publish_file_data`
**Events listened:** `file-transfer-request`, `file-transfer-complete`, `file-received`

---

## Account

**Path:** `/account`
**Source:** `src/pages/Account.svelte`

Wallet and account management. Displays the wallet address (copyable, hideable) and private key (copyable, hideable) with current CHI balance. A send dialog allows transferring CHI to another address with confirmation.

Transaction history shows all transactions with enriched metadata (type labels like "Speed Tier Payment", "Seeder Payment", file names). The page also includes a peer reputation section showing connected peers with trust indicators and a blacklist manager for blocking specific peers by address.

**Backend commands:** `get_wallet_balance`, `send_transaction`, `get_transaction_history`, `get_transaction_receipt`, `record_transaction_meta`

---

## Network

**Path:** `/network`
**Source:** `src/pages/Network.svelte`

DHT network monitoring and control. Shows connection status with connect/disconnect controls. Displays the local peer ID, listening port, and multiaddresses. Lists all connected peers with their peer IDs, addresses, and last-seen timestamps.

Network statistics show total peers, bandwidth usage, and DHT health (routing table size, bootstrap node reachability). The Geth node status panel shows blockchain sync state, block height, and peer count. Provides controls for starting/stopping both DHT and Geth services.

**Backend commands:** `start_dht`, `stop_dht`, `get_dht_peers`, `get_network_stats`, `get_dht_health`, `get_peer_id`, `ping_peer`, `start_geth`, `stop_geth`, `get_geth_status`, `check_bootstrap_health`

---

## Hosting

**Path:** `/hosting`
**Source:** `src/pages/Hosting.svelte`

Host advertisement and management for the hosting marketplace. Hosts can advertise their storage capacity, set pricing (CHI per MB per day), configure minimum deposit requirements, and specify accepted file types. Advertisements are published to DHT for discovery by other peers.

The page shows active hosting agreements with their status (proposed, accepted, active, cancelled), file lists, and payment tracking. Hosts can accept or decline incoming proposals. When an agreement is active, the host automatically seeds the agreed files.

**Backend commands:** `publish_host_advertisement`, `unpublish_host_advertisement`, `store_hosting_agreement`, `get_hosting_agreement`, `list_hosting_agreements`, `get_active_hosted_files`

---

## Hosts

**Path:** `/hosts`
**Source:** `src/pages/Hosts.svelte`

Browse available hosts and create hosting agreements. Queries the DHT host registry to find peers advertising storage. Hosts are sortable by reputation, price, or available storage.

To propose an agreement, users select files from their Drive, specify a duration, and submit a proposal. The proposal includes file hashes, size, duration, and deposit amount. Proposals are stored in DHT and the target host is notified. The page tracks all outgoing agreements with status updates.

**Backend commands:** `get_host_registry`, `get_host_advertisement`, `store_hosting_agreement`, `get_hosting_agreement`, `list_hosting_agreements`, `drive_list_items`

---

## Mining

**Path:** `/mining`
**Source:** `src/pages/Mining.svelte`

CPU mining interface using the integrated Geth node. Controls for starting/stopping mining with configurable thread count. Real-time display of hash rate, blocks found, and accumulated rewards.

Shows mining history with block numbers, timestamps, and rewards. Requires Geth to be installed and running. The miner address is set from the active wallet. Geth installation and download are handled automatically if not present.

**Backend commands:** `start_mining`, `stop_mining`, `get_mining_status`, `get_mined_blocks`, `set_miner_address`, `is_geth_installed`, `download_geth`, `start_geth`, `get_geth_status`

---

## Settings

**Path:** `/settings`
**Source:** `src/pages/Settings.svelte`

Application configuration organized into sections:

**Appearance:** Theme mode (light/dark/system), color theme selection, navigation style (navbar/sidebar).

**Storage:** Custom download directory selection, available disk space display.

**Notifications:** Toggle notifications by category (downloads, transfers, network events).

**Hosting:** Enable/disable hosting marketplace participation, configure max storage allocation, pricing, and minimum deposit.

All settings persist to localStorage via the `settings` store. Changes apply immediately without restart.

**Backend commands:** `pick_download_directory`, `set_download_directory`, `get_download_directory`, `get_available_storage`

---

## Diagnostics

**Path:** `/diagnostics`
**Source:** `src/pages/Diagnostics.svelte`

System health diagnostics with tests across five categories: environment, network, storage, security, and system. Each test returns pass, warning, fail, or info status. Tests include Tauri environment detection, DHT connectivity, bootstrap node reachability, storage path validation, disk space checks, encryption capability, and WebRTC support.

Results are displayed with color-coded status indicators. A full report can be exported as text for troubleshooting. Includes a Geth log viewer for blockchain node debugging.

**Backend commands:** `get_dht_health`, `check_bootstrap_health`, `get_geth_status`, `get_available_storage`, `read_geth_log`
