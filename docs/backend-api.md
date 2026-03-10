# Backend API (Tauri Commands)

Frontend calls into Rust through Tauri `invoke()` handlers in `src-tauri/src/lib.rs`.

## Command Groups

### DHT / Network

- `start_dht`
- `stop_dht`
- `get_dht_peers`
- `get_network_stats`
- `get_peer_id`
- `get_dht_health`
- `get_bootstrap_peer_ids`
- `ping_peer`
- `store_dht_value`
- `get_dht_value`
- `echo_peer`

### Transfer / Download / Torrent

- `send_file`
- `send_file_by_path`
- `accept_file_transfer`
- `decline_file_transfer`
- `publish_file`
- `publish_file_data`
- `search_file`
- `start_download`
- `calculate_download_cost`
- `register_shared_file`
- `republish_shared_file`
- `unpublish_all_shared_files`
- `parse_torrent_file`
- `export_torrent_file`

### File System Helpers

- `get_available_storage`
- `get_file_size`
- `open_file_dialog`
- `pick_download_directory`
- `set_download_directory`
- `get_download_directory`
- `open_file`
- `show_in_folder`
- `show_drive_item_in_folder`

### Wallet / Chain

- `get_wallet_balance`
- `send_transaction`
- `get_transaction_receipt`
- `get_transaction_history`
- `record_transaction_meta`
- `request_faucet`
- `get_chain_id`

### Geth / Mining

- `is_geth_installed`
- `download_geth`
- `start_geth`
- `stop_geth`
- `get_geth_status`
- `start_mining`
- `stop_mining`
- `get_mining_status`
- `get_gpu_mining_capabilities`
- `list_gpu_devices`
- `start_gpu_mining`
- `stop_gpu_mining`
- `get_gpu_mining_status`
- `get_mined_blocks`
- `set_miner_address`
- `read_geth_log`
- `check_bootstrap_health`
- `get_bootstrap_health`

### Encryption

- `init_encryption_keypair`
- `get_encryption_public_key`
- `encrypt_file_for_recipient`
- `decrypt_file_data`
- `send_encrypted_file`
- `publish_encryption_key`
- `lookup_encryption_key`

### Drive

- `get_drive_server_url`
- `publish_drive_share`
- `unpublish_drive_share`
- `drive_list_items`
- `drive_list_all_items`
- `drive_create_folder`
- `drive_upload_file`
- `drive_update_item`
- `drive_delete_item`
- `drive_create_share`
- `drive_revoke_share`
- `drive_list_shares`
- `drive_toggle_visibility`
- `publish_drive_file`
- `drive_stop_seeding`
- `drive_export_torrent`

### Hosting / Marketplace

- `create_hosted_site`
- `list_hosted_sites`
- `delete_hosted_site`
- `start_hosting_server`
- `stop_hosting_server`
- `get_hosting_server_status`
- `publish_site_to_relay`
- `unpublish_site_from_relay`
- `publish_host_advertisement`
- `unpublish_host_advertisement`
- `get_host_registry`
- `get_host_advertisement`
- `store_hosting_agreement`
- `get_hosting_agreement`
- `list_hosting_agreements`
- `get_active_hosted_files`
- `cleanup_agreement_files`

### App Lifecycle

- `exit_app`

## Notes

- `publish_file` / `publish_file_data` return `PublishResult { merkleRoot }`.
- `search_file` returns `SearchResult` with `seeders[]` entries including `peerId`, `priceWei`, `walletAddress`, and `multiaddrs`.
- `publish_drive_file` requires DHT to be running; otherwise it returns an error instead of setting a false seeding state.
- `drive_delete_item` removes files from disk and returns an error if physical deletion fails.

## Source of Truth

The authoritative command list is the `tauri::generate_handler![ ... ]` block in [`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs).
