# Chiral Network Code Reading Guide

This guide gives you a precise map of the project, how the parts fit together, how requests flow, and a function-level walkthrough of the core modules you will touch first.

## Project Introduction
- Decentralized, BitTorrent-inspired P2P file sharing with blockchain-based payments and reputation-aware peer selection.
- Tauri desktop app: Svelte/TypeScript front end + Rust backend services.
- Discovery and transfer run on Kademlia DHT + multi-protocol transport (HTTP, BitTorrent/WebTorrent, ed2k, WebRTC data channels); payments are chain-agnostic and separate from transport.
- Files are chunked, addressed by Merkle roots/CIDs, and transferred via Bitswap-like exchange; encryption is optional and handled by the backend.

## Main Components
- **Front end (Svelte + Vite)**: UI screens, Svelte stores, and service wrappers under `src/lib/services` that orchestrate backend commands, downloads, and WebRTC sessions.
- **Rust backend (Tauri)**: DHT node, file transfer, chunking/encryption, relay/auto-relay, DCUtR hole punching, WebRTC proxying, and blockchain plumbing exposed as Tauri commands (e.g., `start_dht_node`, `download_blocks_from_network`).
- **Networking**: libp2p stack (Kademlia, identify, ping, relay, dcutr), Bitswap-style block exchange, WebRTC as a data path for WebTorrent compatibility, STUN-based ICE setup.
- **Storage/encryption**: Chunking, Merkle tree construction, AES key bundling; manifests flow to the UI for tracking, downloads reassemble via Rust.
- **Wallet/blockchain**: ETH-compatible chain; UI signs with `ethers.js`, while settlement is decoupled from transfer protocols.

## Architecture & Relationships
1. **UI layer** (pages and components) triggers actions through service singletons (`fileService`, `dhtService`, `encryptionService`, `walletService`, `createWebRTCSession`), keeping views light.
2. **Service layer** validates inputs (paths, addresses), subscribes to Tauri events, and invokes Rust commands via `@tauri-apps/api`.
3. **Rust layer** runs the libp2p swarm (DHT, relay/auto-relay, Bitswap), chunk/encrypt/decrypt, and emits events such as `published_file`, `file_content`, `dht_metadata_found` back to the UI.
4. **Data flow**: UI → service → Tauri command → Rust subsystem → Tauri event → service → UI store/component.
5. **Payment flow**: UI prepares and signs transactions; on-chain submission and settlement happen separately so transport protocol choice does not matter.

## Critical Path: File Sharing Flow
Use this when debugging the main loop of publish/search/download.
- **Initialization**: UI calls `fileService.initializeServices()` → starts Rust file transfer + fetches bootstrap nodes → `start_dht_node` → `dhtService.setPeerId`.
- **Publish/seed**: UI selects file → Rust chunk/encrypt builds manifest → `dhtService.publishFileToNetwork` subscribes to `published_file` → triggers `upload_file_to_network` → event carries `fileHash`/`merkleRoot` → UI records metadata and advertises in DHT.
- **Search/metadata**: UI calls `dhtService.searchFileMetadata(hash)` → sends `search_file_metadata` → waits for `dht_metadata_found` → normalizes `fileHash`/`merkleRoot` → enriches with `get_file_seeders` → UI renders results.
- **Download**: UI resolves output path → `dhtService.downloadFile(metadata)` ensures directories → subscribes to `file_content` → prepares Bitswap metadata (`cids`, `isRoot`) → invokes `download_blocks_from_network` → receives chunks, writes file, emits final metadata.
- **WebRTC path** (when WebTorrent or browser peers): `createWebRTCSession` sets up STUN, offer/answer, ICE candidates via signaling; data channel used as an alternate transport for chunks.

## Recommended Reading Order
1. `docs/system-overview.md` for concepts and protocol separation.
2. `docs/architecture.md` and `docs/implementation-guide.md` for component boundaries and build/runtime expectations.
3. Front-end orchestration in `src/lib/services` (`fileService.ts`, `dht.ts`, `webrtcService.ts`, `walletService.ts`) plus Svelte stores.
4. Rust networking/transfer core in `src-tauri/src/dht.rs` with adjacent modules (`file_transfer.rs`, `manager.rs`, `webrtc_service.rs`) to see libp2p wiring and chunk pipeline.
5. UI entrypoints/pages to see how flows are invoked (search/upload/download).
6. Tests under `tests/` for behavior/regression cues.

## Function Walkthrough (key modules)

### src/lib/services/walletService.ts
- `signTransaction(txRequest)`: Builds and signs a legacy Ethereum transaction with stored private key + Chiral chain ID; throws if no wallet is loaded.
- `isValidAddress(address)`: Validates EIP-55-style address parsing.
- `formatEther(wei)`: Converts Wei to human-readable ETH for UI.
- `parseEther(eth)`: Converts ETH string to Wei for contract/tx inputs.

### src/lib/services/fileService.ts
- `initializeServices()`: Starts Rust file transfer, fetches bootstrap nodes, starts DHT, caches peer ID on the DHT singleton.
- `uploadFile(file, recipientPublicKey?)`: Reads dropped file, saves temp copy, delegates to encryption service so backend can chunk/encrypt before publishing.
- `getMerkleRoot(fileHash)`: Asks backend for Merkle root tied to a stored hash.
- `downloadFile(hash, fileName)`: Validates download path (settings), constructs output path, triggers network download via Rust, returns full path.
- `showInFolder(path)`: Requests OS to reveal file in native explorer.
- `getAvailableStorage()`: Returns available disk space (GB) or `null` on failure for UI retry.

### src/lib/dht.ts
- Types: `DhtConfig`, `FileMetadata`, `FileManifestForJs`, `DhtHealth`, NAT reachability enums.
- Helpers: `encryptionService.encryptFile/decryptFile` wrap Tauri encryption commands.
- `DhtService.start(config?)`: Boots DHT with optional NAT/relay/cache tuning; defaults to backend bootstrap nodes.
- `stop()`: Stops DHT node and clears cached peer ID.
- `publishFileToNetwork(filePath, price?)`: Subscribes to `published_file`, invokes `upload_file_to_network`, normalizes `fileHash`/`merkleRoot`, returns metadata.
- `downloadFile(fileMetadata)`: Resolves download path (metadata or settings), ensures directories exist, subscribes to `file_content`, prepares Bitswap metadata (`merkleRoot`, `cids`, `isRoot` fallback), then calls `download_blocks_from_network`.
- `searchFile(fileHash)`: Fire-and-forget searches via backend commands.
- `searchFileMetadata(fileHash, timeoutMs?)`: Waits for `dht_metadata_found`, enriches with live seeders, normalizes hashes, enforces timeout, returns metadata or throws.
- `connectPeer(peerAddress)`: Connects to peer multiaddr, updates reputation store on success/failure.
- Accessors: `getPeerId()`, `getPort()`, `getMultiaddr()`, `getSeedersForFile()`, `getPeerCount()`, `getHealth()`.

### src/lib/services/webrtcService.ts
- `createWebRTCSession(opts)`: Builds `RTCPeerConnection` with sanitized STUN list, optional initiator data channel, ICE/state wiring, signaling hooks (`createOffer`, `acceptOfferCreateAnswer`, `acceptAnswer`, `addRemoteIceCandidate`).
- Session API: `send(data)` throws if channel not open; `close()` disposes channel/connection; `connectionState`/`channelState` Svelte stores mirror live states.

### src-tauri/src/dht.rs (Rust, high level)
- Data Structures: `Ed2kSourceInfo` struct (in `dht/models.rs`) for holding ed2k link metadata.
- Protocol definitions: `KeyRequestProtocol`, `KeyRequestCodec`, `KeyRequest/KeyResponse` for exchanging encrypted key bundles tied to a Merkle root.
- Error handling: `Ed2kError` for validating ed2k links.
- Core services (further in file): libp2p swarm wiring (Kademlia DHT, relay/auto-relay, identify, dcutr, ping), Bitswap-style block exchange, chunking via `ChunkManager`, peer selection metrics, and command handlers that the Tauri layer exposes to the UI.

Use this as a map while you explore; it should help you jump to the right function, trace the critical file-sharing path, and understand how data and events move through the stack.
