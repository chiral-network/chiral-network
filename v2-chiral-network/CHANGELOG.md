# `apas-dev` Branch Summary

**62 commits** | **152 files changed** | **+31,342 / -5,280 lines**

---

## Work Items

### 1. Project Restructure & Cleanup
- Cleared ChiralDrop page and backend for rebuild
- Removed outdated documentation files
- Added high-level architecture overview
- Created rebuild template

### 2. Wallet System
- Wallet creation/login page
- Wallet balance tracking via blockchain RPC
- Send CHR transactions with local signing (EIP-155)
- Transaction history
- Logout with navigation back to wallet page

### 3. DHT & P2P Networking
- DHT foundations with libp2p (Kademlia, mDNS, Noise)
- Peer discovery and ping/pong protocol
- Added libp2p bootstrap nodes from v1 (4 nodes)
- Bootstrap node health checking with UI
- DHT file metadata publishing and lookup

### 4. ChiralDrop File Sharing
- Full ChiralDrop implementation with peer-to-peer file transfer
- Alias service for peer naming
- Accept/decline file transfers
- Pulsing wave animation for user icon
- Toast notifications with stacking and auto-dismiss

### 5. Upload & Download System
- Upload page with drag-and-drop, protocol selection, DHT integration
- Download page with hash search, magnet links, torrent file support
- Remote file download via P2P network
- Shareable magnet links and torrent export
- Re-register shared files on app startup
- Multi-peer fallback for DHT file lookup
- Encrypted history service (AES-256-GCM, DHT sync)

### 6. Blockchain / Geth Integration
- Geth download, install, start/stop management
- Genesis block initialization (Chain ID 98765)
- Mining page with thread control
- Geth status on Network page
- Bootstrap node health checking for Geth enodes

### 7. Account Page
- Comprehensive wallet management UI
- Balance display, send CHR modal
- Transaction history with receipts
- Security tips and account details

### 8. Dark Mode
- Settings page with theme selection (light/dark/system)
- Dark mode applied to all 8 pages + navbar

### 9. End-to-End Encryption
- X25519 key exchange + AES-256-GCM (ECIES pattern)
- Deterministic keypair from wallet private key
- Encrypt/decrypt file transfers
- Publish/lookup encryption keys via DHT
- Frontend encryption service (TypeScript)

### 10. Bug Fixes
- Ping/pong protocol fixes (multiple iterations)
- Toast stacking and reactivity fixes
- Transaction amount type errors
- Torrent file dialog and magnet link parsing
- Infinite loop in Upload page effect
- Non-Tauri environment handling
