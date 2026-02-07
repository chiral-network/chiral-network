# Chiral Network - Student Development Guide

**Last Updated:** November 29, 2025  
**Target Audience:** Future student developers, TAs, and contributors

---

## Table of Contents

1. [Project Overview](#project-overview)
2. [Understanding the Architecture](#understanding-the-architecture)
3. [Technology Stack](#technology-stack)
4. [Getting Started](#getting-started)
5. [Code Navigation Guide](#code-navigation-guide)
6. [Key Development Workflows](#key-development-workflows)
7. [Testing Strategy](#testing-strategy)
8. [Outstanding Tasks & Implementation Priorities](#outstanding-tasks--implementation-priorities)
9. [Common Pitfalls & Debugging Tips](#common-pitfalls--debugging-tips)
10. [Resources & Documentation](#resources--documentation)

---

## Project Overview

### What is Chiral Network?

Chiral Network is a **decentralized peer-to-peer file sharing platform**, with integrated blockchain-based payment system and reputation-aware peer selection.

### Core Concepts

- **Decentralized Storage**: No central servers—files exist only while peers seed them
- **Multi-Protocol Support**: HTTP, BitTorrent, WebTorrent, ed2k, and FTP protocols for flexibility
- **Payment Layer Separation**: Blockchain payments are completely decoupled from data transfer protocols
- **DHT-Based Discovery**: Kademlia DHT for peer and file discovery
- **Continuous Seeding Model**: Files are available immediately when added, not "uploaded" to a server

### Key Features Status

[OK] **Completed:**
- Desktop UI (Svelte 5 + Tauri 2)
- Basic P2P networking (libp2p integration)
- Kademlia DHT integration
- File chunking and encryption
- Wallet/blockchain integration (Ethereum-compatible)
- Mining functionality (Geth integration)
- Multi-protocol download framework
- Unified protocol management system (ProtocolManager)
- Protocol detection and automatic selection
- Seeding registry for multi-protocol tracking
- HTTP and FTP protocol handlers

[WIP] **In Progress:**
- BitTorrent protocol completion
- ed2k protocol refinement
- Payment automation and verification
- Enhanced protocol selection strategies
- Reputation system maturity

[DATE] **Planned:**
- Enhanced proxy services
- CDN-like distribution
- Browser extension support

---

## Understanding the Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────┐
│                  Frontend (Svelte)                      │
│  Pages: Download, Upload, Network, Wallet, Settings    │
└─────────────────┬───────────────────────────────────────┘
                  │
                  [v] Tauri Commands
┌─────────────────────────────────────────────────────────┐
│              Backend (Rust/Tauri)                       │
│  - DHT Service (peer discovery)                         │
│  - File Transfer (chunking, multi-source)               │
│  - Protocol Handlers (HTTP, FTP, BitTorrent, ed2k)     │
│  - Encryption/Keystore                                  │
│  - WebRTC Service (NAT traversal)                       │
└─────────────────┬───────────────────────────────────────┘
                  │
        ┌─────────┴─────────┐
        [v]                   [v]
┌──────────────┐    ┌──────────────┐
│  libp2p      │    │  Blockchain  │
│  Network     │    │  (Geth)      │
│  (DHT/Relay) │    │  Payments    │
└──────────────┘    └──────────────┘
```

### Decoupled Design Philosophy

**Critical Concept:** The payment layer is completely separate from data transfer protocols.

- **Data Transfer Layer**: Handles file chunks via HTTP/BitTorrent/ed2k/FTP
- **Payment Layer**: Blockchain transactions for seeding rewards
- **Why?**: Add new protocols without changing payment logic; use legacy protocols unchanged

### Component Breakdown

#### Frontend (`src/`)
- **Pages**: User-facing views (Download, Upload, Network, Wallet)
- **Services**: TypeScript wrappers around Tauri commands
  - `fileService.ts`: File operations and initialization
  - `dht.ts`: DHT operations and peer discovery
  - `walletService.ts`: Blockchain interactions
  - `webrtcService.ts`: WebRTC session management
- **Stores**: Svelte stores for reactive state management
- **Types**: TypeScript type definitions

#### Backend (`src-tauri/src/`)
- **Core Modules**:
  - `dht.rs`: Kademlia DHT, peer discovery, libp2p swarm
  - `file_transfer.rs`: File chunking and transfer coordination
  - `multi_source_download.rs`: Parallel downloads from multiple peers
  - `protocols/`: Unified protocol management system
  - `encryption.rs`: AES encryption and key management
  - `ethereum.rs`: Blockchain integration
  
- **Protocol System** (`src-tauri/src/protocols/`):
  - `mod.rs`: ProtocolManager - unified protocol routing and management
  - `traits.rs`: Core traits (ProtocolHandler, DownloadOptions, SeedOptions)
  - `detection.rs`: ProtocolDetector - automatic protocol selection
  - `seeding.rs`: SeedingRegistry - multi-protocol seeding tracking
  - `http.rs`: HTTP/HTTPS protocol handler ([OK] Complete)
  - `ftp.rs`: FTP/FTPS protocol handler ([OK] Complete)
  - `bittorrent.rs`: BitTorrent/WebTorrent handler ([WIP] In Progress)
  - `ed2k.rs`: eDonkey2000 protocol handler ([WIP] In Progress)

- **Legacy Protocol Files** (being migrated):
  - `http_download.rs`, `ftp_client.rs`, `ftp_downloader.rs`
  - `bittorrent_handler.rs`, `ed2k_client.rs`

- **Support Services**:
  - `peer_selection.rs`: Reputation-based peer ranking
  - `reputation.rs`: Peer reputation tracking
  - `webrtc_service.rs`: WebRTC for NAT traversal
  - `bandwidth.rs`: Bandwidth scheduling and throttling

#### Blockchain (`relay/` + Geth integration)
- Custom Ethereum-compatible chain (chainId: 98765)
- Relay server for signaling and coordination
- Mining rewards for seeders

---

## Technology Stack

### Frontend
- **Framework**: Svelte 5 (reactive UI framework)
- **Build Tool**: Vite (fast dev server and bundler)
- **Desktop Framework**: Tauri 2 (Rust-based Electron alternative)
- **Styling**: TailwindCSS + PostCSS
- **Blockchain**: ethers.js v6 (Ethereum interactions)
- **i18n**: svelte-i18n (internationalization)

### Backend
- **Language**: Rust (systems programming, memory safety)
- **Networking**: libp2p (P2P networking stack)
  - Kademlia DHT
  - Relay/AutoRelay for NAT traversal
  - DCUtR (Direct Connection Upgrade through Relay)
- **Async Runtime**: Tokio (async I/O)
- **Blockchain**: Geth (Go-Ethereum) integration
- **WebRTC**: WebRTC-rs for browser peer support

### Testing
- **Unit Tests**: Vitest (JavaScript/TypeScript)
- **Integration Tests**: Cargo test (Rust)
- **E2E Tests**: Playwright (browser automation)

---

## Getting Started

### Prerequisites
```bash
# Required
- Node.js 18+
- Rust 1.70+
- Git
- C++ compiler (native modules)

# Optional (for blockchain features)
- Geth (Go-Ethereum) 1.13+
- Python 3.8+ (build scripts)
```

### Initial Setup

```bash
# 1. Clone repository
git clone https://github.com/chiral-network/chiral-network.git
cd chiral-network

# 2. Install dependencies
npm install

# 3. Run in development mode
npm run tauri:dev

# 4. Run tests
npm test                  # Frontend tests
cd src-tauri && cargo test # Backend tests
```

### Development Workflow

```bash
# Frontend only (faster iteration)
npm run dev

# Full app with hot reload
npm run tauri:dev

# Type checking
npm run check

# Build production
npm run tauri:build
```

---

## Code Navigation Guide

### Where to Start Reading Code

**New to the project? Read in this order:**

1. **`docs/system-overview.md`** - Understand core concepts and architecture
2. **`docs/code-reading-guide.md`** - Detailed function-level walkthrough
3. **`README.md`** - Quick start and feature overview
4. **`src/lib/services/fileService.ts`** - Entry point for file operations
5. **`src-tauri/src/dht.rs`** - Core networking implementation
6. **`src/pages/Download.svelte`** - Example of frontend-backend integration

### Critical Data Flows

#### Publishing a File (Upload/Seed)
```
User selects file -> FileService.uploadFile()
  -> encryptionService.encryptFile() (chunks + encrypts)
  -> dhtService.publishFileToNetwork()
  -> Rust: upload_file_to_network command
  -> ChunkManager creates manifest
  -> DHT advertises availability
  -> Event: 'published_file' -> UI updates
```

#### Downloading a File
```
User initiates download -> dhtService.downloadFile()
  -> dhtService.searchFileMetadata() (find seeders)
  -> Rust: download_blocks_from_network command
  -> MultiSourceDownload coordinates parallel fetches
  -> Protocol handlers (HTTP/FTP/BitTorrent) fetch chunks
  -> Reassembly writes to disk
  -> Event: 'file_content' -> UI updates progress
```

#### Peer Discovery
```
DHT node starts -> connect to bootstrap nodes
  -> Periodic DHT queries for file hashes
  -> Kademlia routing table updates
  -> Relay/AutoRelay for NAT traversal
  -> WebRTC for direct browser connections
```

### Key Files by Feature

| Feature | Frontend | Backend |
|---------|----------|---------|
| File Upload | `pages/Upload.svelte` | `file_transfer.rs`, `encryption.rs` |
| File Download | `pages/Download.svelte` | `multi_source_download.rs` |
| DHT/Networking | `lib/dht.ts` | `dht.rs`, `dht/` modules |
| Wallet | `pages/Wallet.svelte` | `ethereum.rs`, `keystore.rs` |
| Protocol Management | `lib/services/multiSourceDownloadService.ts` | `protocols/mod.rs` (ProtocolManager) |
| Protocol Detection | - | `protocols/detection.rs` |
| Seeding | `pages/Upload.svelte` | `protocols/seeding.rs` (SeedingRegistry) |
| HTTP Protocol | - | `protocols/http.rs` |
| FTP Protocol | - | `protocols/ftp.rs` |
| BitTorrent Protocol | - | `protocols/bittorrent.rs` ([WIP]) |
| ed2k Protocol | - | `protocols/ed2k.rs` ([WIP]) |
| Reputation | `lib/reputationStore.ts` | `reputation.rs`, `peer_selection.rs` |
| Settings | `pages/Settings.svelte` | `config/` modules |

---

## Key Development Workflows

### Adding a New Protocol

1. **Implement ProtocolHandler trait** in `src-tauri/src/protocols/`
   ```rust
   // Example: protocols/custom_protocol.rs
   use async_trait::async_trait;
   use super::traits::*;
   
   pub struct CustomProtocolHandler {
       // connection pool, config, etc.
   }
   
   #[async_trait]
   impl ProtocolHandler for CustomProtocolHandler {
       fn name(&self) -> &'static str {
           "custom"
       }
       
       fn supports(&self, identifier: &str) -> bool {
           identifier.starts_with("custom://")
       }
       
       async fn download(&self, identifier: &str, options: DownloadOptions) 
           -> Result<DownloadHandle, ProtocolError> {
           // Implementation
       }
       
       async fn seed(&self, file_path: PathBuf, options: SeedOptions) 
           -> Result<SeedingInfo, ProtocolError> {
           // Implementation
       }
       
       fn capabilities(&self) -> ProtocolCapabilities {
           ProtocolCapabilities {
               supports_seeding: true,
               supports_pause_resume: true,
               // ... other capabilities
           }
       }
   }
   ```
2. **Export in `protocols/mod.rs`**
   ```rust
   pub mod custom_protocol;
   pub use custom_protocol::CustomProtocolHandler;
   ```
3. **Register with ProtocolManager** in initialization code
   ```rust
   manager.register(Box::new(CustomProtocolHandler::new()?));
   ```
4. **Add Frontend Support** in `lib/services/`
5. **Add Tests** in `tests/` directory
6. **Document** in `docs/` (create `CUSTOM_PROTOCOL_IMPLEMENTATION.md`)

### Adding a Tauri Command

**Backend** (`src-tauri/src/commands/`):
```rust
#[tauri::command]
pub async fn my_new_command(param: String) -> Result<String, String> {
    // Implementation
    Ok("result".to_string())
}

// Register in main.rs:
// .invoke_handler(tauri::generate_handler![my_new_command, ...])
```

**Frontend** (`src/lib/services/`):
```typescript
import { invoke } from "@tauri-apps/api/core";

export async function myNewFunction(param: string): Promise<string> {
    return await invoke<string>("my_new_command", { param });
}
```

### Debugging Network Issues

1. **Check DHT Health**: Network page -> view connected peers
2. **Inspect Logs**: 
   - Frontend: Browser DevTools console
   - Backend: Terminal output or `chiral.log` file
3. **Verify Bootstrap Nodes**: Settings -> ensure bootstrap nodes are reachable
4. **Test Protocols Individually**: Use curl/ftp client to verify endpoints
5. **Monitor Relay Status**: Check if relay nodes are accepting connections

---

## Testing Strategy

### Unit Tests

**Frontend** (Vitest):
```typescript
// tests/fileService.test.ts
import { describe, it, expect } from 'vitest';
import { FileService } from '$lib/services/fileService';

describe('FileService', () => {
    it('should validate file paths', () => {
        // Test implementation
    });
});
```

**Backend** (Cargo):
```rust
// src-tauri/src/file_transfer.rs
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_chunk_creation() {
        // Test implementation
    }
}
```

### Integration Tests

Located in `tests/` directory:
- `dht.test.ts`: DHT operations
- `multi-source-download.test.ts`: Multi-protocol downloads
- `encryption.test.ts`: Encryption/decryption flows
- `reputation-persistence.test.ts`: Reputation system

### Running Tests

```bash
# All frontend tests
npm test

# Watch mode (auto-rerun on changes)
npm run test:watch

# Specific test file
npx vitest tests/dht.test.ts

# All backend tests
cd src-tauri && cargo test

# Specific test
cargo test test_chunk_creation

# With output
cargo test -- --nocapture
```

---

## Outstanding Tasks & Implementation Priorities

### High Priority (Core Functionality)

#### 1. Complete Multi-Source Download Coordination
**Status**: Partial implementation, needs refinement  
**Files**: `src-tauri/src/multi_source_download.rs`

**TODOs**:
- [ ] Implement chunk validation after download
- [ ] Add retry logic for failed chunks
- [ ] Optimize source selection algorithm
- [ ] Integrate with new ProtocolManager for protocol-agnostic downloads
- [ ] Add per-protocol performance metrics

**Complexity**: Medium  
**Est. Time**: 2-3 weeks

#### 2. Implement P2P Download Logic in Scheduler
**Status**: Placeholder implementation  
**Files**: `src-tauri/src/download_scheduler.rs` (line 153)

**TODOs**:
- [ ] Replace placeholder with actual P2P coordination
- [ ] Integrate with ProtocolManager for protocol detection
- [ ] Add bandwidth allocation per download
- [ ] Implement priority queue for downloads
- [ ] Handle concurrent download limits
- [ ] Support dynamic protocol switching during download

**Complexity**: High  
**Est. Time**: 3-4 weeks

#### 3. BitTorrent Protocol Completion
**Status**: ProtocolHandler implemented, needs core functionality  
**Files**: `src-tauri/src/protocols/bittorrent.rs`

**TODOs**:
- [ ] Migrate from legacy `bittorrent_handler.rs` to new protocol system
- [ ] Complete peer exchange protocol
- [ ] Implement piece selection strategy (rarest-first, sequential)
- [ ] Add DHT integration for trackerless torrents
- [ ] Support magnet links parsing and resolution
- [ ] Implement WebTorrent WebRTC transport
- [ ] Add torrent file parsing (.torrent metadata)
- [ ] Integrate with SeedingRegistry for multi-protocol seeding
- [ ] Implement ProtocolHandler trait methods fully

**Complexity**: High  
**Est. Time**: 4-6 weeks  
**Reference**: `docs/bittorrent-implementation-guide.md`

#### 4. ed2k Protocol Refinement
**Status**: ProtocolHandler implemented, needs core functionality  
**Files**: `src-tauri/src/protocols/ed2k.rs`

**TODOs**:
- [ ] Migrate from legacy `ed2k_client.rs` to new protocol system
- [ ] Complete ed2k link parsing validation (`ed2k://|file|...`)
- [ ] Implement multi-server connections
- [ ] Add part hash verification (AICH)
- [ ] Support ed2k search protocol
- [ ] Implement lowID/high ID handling
- [ ] Integrate with ProtocolManager for auto-detection
- [ ] Add SeedingRegistry integration

**Complexity**: Medium  
**Est. Time**: 2-3 weeks  
**Reference**: `docs/ED2K_PROTOCOL_IMPLEMENTATION.md`

### Medium Priority (Enhancement)

#### 5. Enhanced Protocol Detection and Selection
**Status**: Basic framework implemented  
**Files**: `src-tauri/src/protocols/detection.rs`, `protocols/mod.rs`

**TODOs**:
- [ ] Implement speed-based protocol ranking
- [ ] Add reliability metrics per protocol
- [ ] Implement dynamic protocol switching during downloads
- [ ] Add user preference persistence
- [ ] Create bandwidth estimation per protocol
- [ ] Implement fallback strategies when preferred protocol fails
- [ ] Add protocol blacklisting/whitelisting UI
- [ ] Track success rates per protocol per peer

**Complexity**: Medium  
**Est. Time**: 2-3 weeks

#### 6. Reputation System Maturity
**Status**: Basic framework in place  
**Files**: `src-tauri/src/reputation.rs`, `peer_selection.rs`

**TODOs**:
- [ ] Add time-decay for old reputation data
- [ ] Implement reputation recovery mechanisms
- [ ] Add peer reporting/flagging system
- [ ] Create reputation-based access tiers
- [ ] Persist reputation across restarts (partially done)
- [ ] Add reputation visualization in UI

**Complexity**: Medium  
**Est. Time**: 2-3 weeks

#### 7. Payment Automation
**Status**: Manual payment flows exist  
**Files**: `src/pages/Download.svelte`, `src-tauri/src/ethereum.rs`

**TODOs**:
- [ ] Auto-calculate payments based on chunk downloads
- [ ] Implement micropayment batching
- [ ] Add payment verification before chunk delivery
- [ ] Create seeder reward distribution system
- [ ] Add payment history tracking
- [ ] Implement dispute resolution protocol

**Complexity**: High  
**Est. Time**: 4-5 weeks

#### 8. NAT Traversal Optimization
**Status**: Basic relay support implemented  
**Files**: `src-tauri/src/webrtc_service.rs`, `dht.rs`

**TODOs**:
- [ ] Improve relay node discovery
- [ ] Add STUN/TURN server fallbacks
- [ ] Implement hole punching success rate tracking
- [ ] Add UPnP/NAT-PMP support
- [ ] Optimize relay bandwidth usage
- [ ] Add relay node reputation system

**Complexity**: High  
**Est. Time**: 3-4 weeks  
**Reference**: `docs/nat-traversal.md`

### Low Priority (Nice-to-Have)

#### 9. Download Resume/Pause Enhancement
**Status**: Basic pause/resume exists  
**Files**: `src-tauri/src/download_restart.rs`

**TODOs**:
- [ ] Add progress persistence to disk
- [ ] Support resume after app restart
- [ ] Implement smart resume (find new sources)
- [ ] Add partial file validation
- [ ] Create download queue management UI

**Complexity**: Medium  
**Est. Time**: 2 weeks  
**Reference**: `docs/DOWNLOAD_RESTART_USAGE.md`

#### 10. Proxy Service Implementation
**Status**: Basic proxy routing exists  
**Files**: `src/lib/proxy.ts`, `src-tauri/src/proxy_latency.rs`

**TODOs**:
- [ ] Complete proxy self-test mechanism
- [ ] Add proxy latency monitoring
- [ ] Implement load balancing across proxies
- [ ] Add proxy reputation tracking
- [ ] Create proxy provider incentive system

**Complexity**: High  
**Est. Time**: 4-5 weeks  
**Reference**: `docs/PROXY_SELF_TEST.md`

#### 11. Internationalization Expansion
**Status**: Framework in place, limited languages  
**Files**: `src/locales/`, `docs/i18n.md`

**TODOs**:
- [ ] Add more language translations
- [ ] Create translation contribution guide
- [ ] Implement context-aware translations
- [ ] Add RTL language support
- [ ] Create translation testing tools

**Complexity**: Low  
**Est. Time**: 1-2 weeks per language  
**Reference**: `docs/i18n.md`, `docs/add-*.plan.md`

### Future Considerations

#### 12. Browser Extension
**Status**: Not started  
**Complexity**: High | **Est. Time**: 6-8 weeks

**Goals**:
- Intercept magnet links and ed2k links
- Quick-add files to Chiral Network
- Show seeding status in browser

#### 13. Mobile Support
**Status**: Not planned  
**Complexity**: Very High | **Est. Time**: 3-6 months

**Challenges**:
- Background seeding on mobile
- Battery optimization
- Platform-specific P2P restrictions

---

## Common Pitfalls & Debugging Tips

### Frontend Issues

**Issue**: Tauri commands fail with "command not found"  
**Solution**: Ensure command is registered in `src-tauri/src/main.rs` `invoke_handler![]`

**Issue**: State not updating reactively  
**Solution**: Use Svelte stores correctly; wrap Tauri event listeners in `$effect()` for Svelte 5

**Issue**: File paths with spaces cause errors  
**Solution**: Always use proper path joining (`@tauri-apps/api/path`) and escape user input

### Backend Issues

**Issue**: DHT not discovering peers  
**Solution**: 
1. Check firewall settings
2. Verify bootstrap nodes are reachable
3. Ensure correct multiaddr format
4. Check if port is already in use

**Issue**: File chunks not reassembling correctly  
**Solution**:
1. Verify chunk hashes match manifest
2. Check for missing chunks in download coordinator
3. Ensure write permissions on output directory
4. Debug chunk ordering in reassembly logic

**Issue**: WebRTC connections fail  
**Solution**:
1. Verify STUN servers are accessible
2. Check browser console for ICE errors
3. Test with relay fallback enabled
4. Ensure firewall allows UDP traffic

### Protocol Issues

**Issue**: HTTP downloads timeout  
**Solution**: Increase timeout values, check if remote server supports range requests

**Issue**: FTP downloads fail with authentication  
**Solution**: Verify credentials, check if server allows anonymous connections

**Issue**: BitTorrent peers not connecting  
**Solution**: Ensure proper peer exchange protocol, verify tracker responses

### Build Issues

**Issue**: Rust compilation fails  
**Solution**:
```bash
# Clean build cache
cargo clean
cd src-tauri && cargo clean

# Update dependencies
cargo update

# Check for platform-specific issues
rustc --version  # Ensure 1.70+
```

**Issue**: Node modules conflicts  
**Solution**:
```bash
# Clean install
rm -rf node_modules package-lock.json
npm install
```

---

## Resources & Documentation

### Essential Documentation
- **System Overview**: `docs/system-overview.md` - Architecture and concepts
- **Code Reading Guide**: `docs/code-reading-guide.md` - Detailed code walkthrough
- **Implementation Guide**: `docs/implementation-guide.md` - Development workflows
- **API Documentation**: `docs/api-documentation.md` - Tauri commands reference

### Protocol Documentation
- **HTTP**: `docs/HTTP_PROTOCOL_IMPLEMENTATION.md`
- **FTP**: `docs/FTP_SOURCE_IMPLEMENTATION.md`
- **BitTorrent**: `docs/bittorrent-implementation-guide.md`
- **ed2k**: `docs/ED2K_PROTOCOL_IMPLEMENTATION.md`

### Feature Documentation
- **File Sharing**: `docs/file-sharing.md`
- **DHT & Network**: `docs/network-protocol.md`
- **NAT Traversal**: `docs/nat-traversal.md`
- **Reputation System**: `docs/reputation.md`
- **Wallet & Blockchain**: `docs/wallet-blockchain.md`
- **Download Management**: `docs/DOWNLOAD_RESTART_USAGE.md`

### Development Guides
- **Developer Setup**: `docs/developer-setup.md`
- **Contributing**: `docs/contributing.md`
- **Deployment**: `docs/deployment-guide.md`
- **Testing**: Test files in `tests/` directory

### External Resources
- **Tauri Docs**: https://tauri.app/v2/guides/
- **Svelte 5 Docs**: https://svelte-5-preview.vercel.app/docs/
- **libp2p Specs**: https://docs.libp2p.io/
- **Ethereum Docs**: https://ethereum.org/en/developers/docs/
- **BitTorrent Spec**: https://www.bittorrent.org/beps/bep_0003.html

### Community & Support
- **GitHub Issues**: Report bugs and feature requests
- **Zulip Chat**: https://brooknet.zulipchat.com/join/f3jj4k2okvlfpu5vykz5kkk5/
- **GitHub Discussions**: Long-form discussions and Q&A

---

## Development Tips for Students

### Best Practices

1. **Read Existing Code First**: Don't dive into writing new features without understanding the current architecture
2. **Test Incrementally**: Write tests as you go, not after the fact
3. **Document Your Changes**: Update relevant `.md` files in `docs/`
4. **Small Commits**: Commit often with clear, descriptive messages
5. **Ask Questions**: Use GitHub Discussions or Zulip when stuck

### Time Management

- **Allocate 20% of time to reading/understanding** existing code
- **Spend 60% on implementation**, testing as you go
- **Reserve 20% for documentation and cleanup**

### Code Review Checklist

Before submitting a PR:
- [ ] Code compiles without warnings
- [ ] All tests pass (`npm test` and `cargo test`)
- [ ] Added tests for new functionality
- [ ] Updated relevant documentation
- [ ] Checked for TypeScript/Rust linting errors
- [ ] Tested on your local machine end-to-end
- [ ] Followed existing code style conventions

### Recommended Development Path for New Students

**Week 1-2**: Environment setup and codebase exploration
- Set up development environment
- Run the application successfully
- Read core documentation
- Trace one complete file upload/download flow

**Week 3-4**: Small contributions
- Fix minor bugs or TODOs
- Add tests for existing functionality
- Improve documentation clarity

**Week 5+**: Feature development
- Pick a medium-priority task from the list above
- Design and implement with feedback
- Write comprehensive tests
- Document your implementation

---

## Quick Reference

### Important Commands
```bash
# Development
npm run tauri:dev           # Run app with hot reload
npm run check               # TypeScript type checking
npm test                    # Run frontend tests
cargo test                  # Run backend tests

# Building
npm run tauri:build         # Production build
cargo build --release       # Backend only

# Debugging
npm run dev                 # Frontend only (faster)
cargo run                   # Backend only
```

### Key File Locations
```
Frontend Entry:     src/main.ts
Backend Entry:      src-tauri/src/main.rs
Tauri Config:       src-tauri/tauri.conf.json
Blockchain Config:  genesis.json
DHT Implementation: src-tauri/src/dht.rs
File Service:       src/lib/services/fileService.ts
```

### Environment Variables
```bash
# Backend (Rust)
RUST_LOG=debug              # Enable debug logging
RUST_BACKTRACE=1            # Enable backtraces

# Frontend (Vite)
VITE_DEV_MODE=true          # Development mode flag
```

---

## Conclusion

Chiral Network is an ambitious decentralized file sharing platform with a solid foundation and many opportunities for enhancement. The codebase is well-structured with clear separation of concerns, making it approachable for new developers.

Focus on understanding the core file transfer flow first, then branch out into specific protocols or features based on your interests and project needs. Don't hesitate to ask questions and contribute back to the documentation as you learn.

**Happy coding!** [START]

---

*For questions or clarifications, create a GitHub Discussion or post in the Zulip channel.*