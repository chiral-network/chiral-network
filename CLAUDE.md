# Chiral Network Development Guide

## Project Overview

Chiral Network is a decentralized peer-to-peer file sharing application that combines blockchain technology with distributed hash table (DHT) based file storage. It implements a BitTorrent-like continuous seeding model where files are instantly available to the network, with a strong focus on privacy, security, and legitimate use cases.

## Current Architecture

### Core Design Principles

1. **Fully Decentralized P2P**: No centralized servers - all peer discovery via DHT
2. **BitTorrent-Style Sharing**: Files immediately start seeding when added (no "upload" step)
3. **Non-Commercial**: No marketplace, pricing, or trading features to prevent misuse
4. **Privacy-First**: Circuit Relay v2, AutoNAT v2, SOCKS5 proxy support, anonymous mode
5. **Legitimate Use Only**: Designed for personal, educational, and organizational file sharing
6. **Blockchain Integration**: Separate Ethereum-compatible network with Geth integration

### Technology Stack

#### Frontend
- **Svelte 5**: Latest reactive UI framework (upgraded from Svelte 4)
- **TypeScript**: Type-safe development
- **Tailwind CSS**: Utility-first styling
- **@mateothegreat/svelte5-router**: Client-side routing for Svelte 5
- **svelte-i18n**: Internationalization support (EN, ES, RU, ZH, KO)
- **Lucide Svelte**: Icon library
- **Bits UI**: Accessible component primitives
- **mode-watcher**: Dark/light mode support

#### Desktop Integration
- **Tauri 2**: Rust-based desktop runtime
- **Native File System**: OS-level file operations
- **System Tray**: Background operation support

#### P2P Network Infrastructure
- **libp2p v0.54**: Full P2P networking stack (Rust backend)
- **Kademlia DHT**: Distributed file metadata storage and peer discovery
- **WebRTC**: Direct peer-to-peer data channels for file transfers
- **NAT Traversal**:
  - AutoNAT v2 for reachability detection
  - Circuit Relay v2 with AutoRelay for NAT'd peers
  - DCUtR (Direct Connection Upgrade through Relay) for hole punching
  - mDNS for local peer discovery
- **Noise Protocol**: Cryptographic transport security
- **Bitswap Protocol**: Efficient block exchange
- **SOCKS5 Proxy**: Privacy-focused traffic routing

#### Blockchain & Security
- **Geth Integration**: Ethereum node for blockchain operations
- **HD Wallets**: BIP32, BIP39, secp256k1 implementations
- **AES-256-GCM**: File encryption with PBKDF2 key derivation
- **HMAC Authentication**: Stream integrity verification
- **Proof of Storage**: Solidity smart contract for storage verification

## Page Structure

The application uses client-side routing with the following pages:

1. **Download** (default page) - File download management with search, history, and peer selection
2. **Upload** - "Shared Files" page - instant seeding interface with drag & drop
3. **Network** - Peer discovery, DHT status, NAT traversal monitoring
4. **Relay** - Circuit Relay v2 configuration (server mode + client mode)
5. **Mining** - CPU mining for network security (proof-of-work)
6. **Proxy** - SOCKS5 proxy configuration and privacy routing
7. **Analytics** - Usage statistics, bandwidth tracking, performance metrics
8. **Reputation** - Peer reputation system with analytics and relay leaderboard
9. **Account** - Wallet management, HD wallet support, transaction history
10. **Settings** - Comprehensive configuration (storage, network, privacy, bandwidth scheduling, i18n)

### Removed Pages (Anti-Piracy Measures)
- ❌ Search page (could enable finding copyrighted content)
- ❌ Market page (no commercial transactions)
- ❌ Bundles page (no selling file packages)

## State Management (`src/lib/stores.ts`)

### Core Stores

```typescript
// File management
files: FileItem[]                    // All files (downloading, seeding, completed)
downloadQueue: FileItem[]            // Files waiting to download
activeDownloads: number              // Current download count

// Network
peers: PeerInfo[]                    // Connected peers with metrics
networkStats: NetworkStats           // Global network statistics
networkStatus: NetworkStatus         // Connection status from networkService
peerGeoDistribution: Derived         // Geographic distribution of peers

// Wallet & Transactions
wallet: WalletInfo                   // Wallet address and balance
etcAccount: ETCAccount | null        // Ethereum Classic account
transactions: Transaction[]          // Transaction history

// Mining
miningState: MiningState             // Mining status, hash rate, rewards, history
miningProgress: MiningProgress       // Block progress tracking

// Privacy & Proxy
userLocation: string                 // User's region
blacklist: BlacklistEntry[]          // Blacklisted peer addresses
suspiciousActivity: ActivityLog[]    // Security monitoring

// Settings
settings: AppSettings                // Comprehensive app configuration
activeTransfers: Map<>               // P2P/WebRTC transfer tracking
```

### Key Interfaces

**FileItem**: Supports multiple states (downloading, paused, completed, seeding, queued, canceled, failed), priority levels, encryption status, file versioning, multi-CID support, and visual ordering.

**AppSettings**: Includes storage management, bandwidth limits (including bandwidth scheduling), network configuration (port, UPnP, NAT), proxy settings (SOCKS5), privacy modes, NAT traversal settings (AutoNAT v2, Circuit Relay v2, relay server mode), DHT configuration, notification preferences, and i18n.

**MiningState**: Tracks hash rate, rewards, blocks found, thread allocation, mining history for charts, recent blocks, and session persistence.

**PeerInfo**: Includes reputation score, reliability metrics, geographic location, connection status, shared files, and last seen timestamp.

## New Features Since Original CLAUDE.md

### 1. Internationalization (i18n)
- **Implementation**: svelte-i18n library
- **Location**: `src/i18n/i18n.ts`, `src/locales/*.json`
- **Languages**: English, Spanish, Russian, Chinese, Korean
- **Usage**: All UI text uses `$t()` translation function
- **Auto-detection**: Geolocation-based language detection on startup

### 2. Reputation System
- **Page**: `src/pages/Reputation.svelte`
- **Backend**: `src/lib/services/peerSelectionService.ts`
- **Store**: `src/lib/reputationStore.ts`
- **Types**: `src/lib/types/reputation.ts`
- **Features**:
  - Trust levels (Trusted, High, Medium, Low, Unknown)
  - Composite scoring based on latency, bandwidth, uptime, transfers
  - Analytics dashboard with trust level distribution
  - Filtering by trust level, encryption support, uptime
  - Sorting by score, interactions, last seen
  - Relay reputation leaderboard
  - Personal relay stats (if running as relay server)

### 3. Circuit Relay Infrastructure
- **Page**: `src/pages/Relay.svelte`
- **Configuration**:
  - Relay Server Mode: Enable node to act as relay for NAT'd peers
  - AutoRelay Client: Automatically find and use relay nodes
  - Preferred Relays: Manual relay node configuration
- **Features**:
  - Circuit Relay v2 support
  - Reservation management
  - Relay health monitoring
  - Reputation earning for relay operators
  - Headless CLI support (`--enable-autorelay`, `--relay`, etc.)

### 4. HD Wallet Implementation
- **Location**: `src/lib/wallet/`
- **Components**:
  - `bip32.ts` - Hierarchical Deterministic wallet derivation
  - `bip39.ts` - Mnemonic phrase generation and validation
  - `secp256k1.ts` - Elliptic curve cryptography
  - `hd.ts` - HD wallet utilities
  - `wordlist-en.ts` - BIP39 English wordlist
- **UI Components**:
  - `MnemonicWizard.svelte` - Mnemonic generation/import
  - `AccountList.svelte` - Account management
- **Features**: QR code support, multiple accounts, secure key management

### 5. DHT & libp2p Integration
- **Location**: `src/lib/dht.ts`
- **Backend**: Rust-based libp2p v0.54
- **Features**:
  - Kademlia DHT for file metadata
  - Automatic peer discovery
  - Bootstrap node support
  - File metadata publishing/retrieval
  - NAT reachability detection (AutoNAT v2)
  - Circuit relay support
  - Observed address tracking
  - Health monitoring

### 6. WebRTC File Transfers
- **Service**: `src/lib/services/webrtcService.ts`
- **Signaling**: `src/lib/services/signalingService.ts`
- **P2P Transfer**: `src/lib/services/p2pFileTransfer.ts`
- **Features**:
  - Direct peer-to-peer file transfer
  - Chunk-based transfer with Bitswap
  - Multi-source downloads
  - Transfer encryption
  - Progress tracking
  - Connection state management

### 7. Multi-Source Downloads
- **Service**: `src/lib/services/multiSourceDownloadService.ts`
- **Features**:
  - Parallel chunk downloads from multiple peers
  - Intelligent peer selection based on reputation
  - Bandwidth aggregation
  - Automatic failover
  - Chunk verification

### 8. Bandwidth Scheduling
- **Service**: `src/lib/services/bandwidthScheduler.ts`
- **Interface**: `BandwidthScheduleEntry` in stores
- **Features**:
  - Time-based bandwidth limits (HH:MM format)
  - Day-of-week rules (0-6, 0 = Sunday)
  - Separate upload/download limits
  - Multiple schedule support
  - Enable/disable toggle
  - Automatic schedule application

### 9. File Encryption
- **Service**: `src/lib/services/encryption.ts`
- **Algorithm**: AES-256-GCM with PBKDF2 key derivation
- **Features**:
  - Upload encryption with recipient public key
  - Download decryption with key management
  - WebRTC chunk encryption
  - Key fingerprinting
  - Manifest-based chunk tracking

### 10. Download Search & History
- **Components**:
  - `DownloadSearchSection.svelte`
  - `SearchHistoryPanel.svelte`
  - `SearchResultCard.svelte`
  - `PeerSelectionModal.svelte`
- **Store**: `src/lib/stores/searchHistory.ts`
- **Features**:
  - File hash search
  - Search history tracking
  - Peer selection modal
  - Seeder/leecher display
  - Quick re-download

### 11. Geth Integration
- **Service**: `src/lib/services/gethService.ts`
- **Component**: `GethDownloader.svelte`, `GethStatusCard.svelte`
- **Features**: Ethereum node integration, transaction handling, blockchain operations

### 12. Advanced Services

#### Analytics Service (`analyticsService.ts`)
- Bandwidth tracking (upload/download)
- Storage metrics
- Network activity monitoring
- Historical data persistence

#### Peer Services
- `peerService.ts` - Peer connection management
- `peerEventService.ts` - Peer event handling
- `peerSelectionService.ts` - Intelligent peer selection with reputation

#### File Services
- `fileService.ts` - File operation management
- `seedPersistence.ts` - Seed state persistence
- `p2pFileTransfer.ts` - P2P transfer protocol

#### Network Services
- `networkService.ts` - Network monitoring and status
- `geolocation.ts` - User region detection
- `proxyLoadBalancer.ts` - Proxy load balancing
- `proxyLatencyOptimization.ts` - Proxy performance optimization

#### Privacy & Security
- `privacyService.ts` - Privacy mode management
- `proxyAuth.ts` - Proxy authentication
- `encryption.ts` - File encryption utilities

### 13. Smart Contracts
- **Location**: `src/lib/services/ProofOfStorage.sol`
- **Purpose**: Proof of Storage consensus mechanism
- **Language**: Solidity

### 14. UI Components

#### Core Components
- `Modal.svelte` - Reusable modal dialog
- `SimpleToast.svelte` - Toast notifications
- `WindowControls.svelte` - Desktop window controls
- `TransactionList.svelte` - Transaction history display
- `TransactionReceipt.svelte` - Transaction details
- `PeerMetrics.svelte` - Peer performance metrics

#### Reputation Components
- `ReputationCard.svelte` - Individual peer reputation display
- `ReputationAnalytics.svelte` - System-wide reputation analytics
- `RelayReputationLeaderboard.svelte` - Top relay nodes

#### Geo/Network Components
- `GeoDistributionCard.svelte` - Geographic distribution visualization

#### UI Primitives (`src/lib/components/ui/`)
- `button.svelte`, `card.svelte`, `input.svelte`, `label.svelte`
- `badge.svelte`, `progress.svelte`, `dropDown.svelte`
- `Expandable.svelte`

## Key Implementation Details

### File Sharing Model
- Files are **instantly seeded** when added (no pending/uploaded distinction)
- Each file gets a cryptographic hash (SHA-256)
- Files can be encrypted with AES-256-GCM
- Metadata published to Kademlia DHT
- Files show real-time seeder/leecher counts
- Continuous seeding until manually removed
- Support for file versioning
- Multi-CID support for chunked files

### NAT Traversal Architecture
1. **AutoNAT v2**: Automatic reachability detection with confidence scoring
2. **Circuit Relay v2**: Relay reservation for NAT'd peers
3. **DCUtR**: Hole punching for direct connections
4. **mDNS**: Local network peer discovery
5. **SOCKS5 Proxy**: Privacy-focused routing

### Routing System
- **Router**: `@mateothegreat/svelte5-router`
- **Route Config**: Defined in `App.svelte`
- **Navigation**: Context-based navigation with `goto()` function
- **404 Handling**: NotFound page for invalid routes
- **Browser History**: Synced with browser back/forward
- **Scroll Management**: Auto-scroll to top on page change

### Internationalization Flow
1. **Initialization**: `setupI18n()` called in `App.svelte` onMount
2. **Auto-detection**: Geolocation-based language detection
3. **Persistence**: User preference stored in localStorage
4. **Usage**: `$t('key.path')` for all UI text
5. **Locale Files**: JSON files in `src/locales/`

### Mining Implementation
- **Backend**: Geth integration for real blockchain mining
- **UI**: Mining page with thread control, intensity adjustment
- **History**: Mining history tracking with hash rate charts
- **Rewards**: Block rewards tracked (some values are mock data)
- **Persistence**: Session state saved to localStorage

## Development Guidelines

### When Adding Features

1. **No Commercial Elements**: Never add pricing, trading, or marketplace features
2. **Privacy First**: Always consider user privacy and anonymity
3. **Legitimate Use**: Design for legal file sharing use cases only
4. **Decentralized**: No centralized servers or intermediaries
5. **BitTorrent Model**: Files should seed continuously, not "upload once"
6. **i18n Support**: Add translation keys for all new UI text
7. **Type Safety**: Use TypeScript interfaces for all data structures

### Code Style

- Use TypeScript for type safety
- Follow existing Svelte 5 patterns (runes, reactive statements)
- Keep components small and focused
- Use Tailwind classes for styling
- Add translation keys to locale files
- Document complex services with JSDoc comments
- Use consistent naming conventions

### Testing Approach

- Test with mock data first
- Ensure UI works without backend
- Verify drag-and-drop functionality
- Test responsive design
- Test i18n with multiple languages
- Run vitest tests: `npm test`

### Adding Translations

1. Add keys to all locale files in `src/locales/`
2. Use descriptive key paths (e.g., `reputation.filters.trustLevel`)
3. Test with all languages
4. Maintain consistency across translations

## Common Tasks

### Adding a New Page

1. Create component in `src/pages/`
2. Import in `App.svelte`
3. Add to `routes` array in `App.svelte`
4. Add to `menuItems` array with icon
5. Add translation keys for nav label
6. Update route handling

### Modifying Stores

1. Update interfaces in `stores.ts`
2. Add TypeScript types
3. Update dependent components
4. Test state reactivity
5. Ensure localStorage persistence if needed

### Adding a New Service

1. Create service file in `src/lib/services/`
2. Export service as singleton or factory
3. Add TypeScript interfaces for all types
4. Integrate with Tauri backend if needed
5. Update relevant stores
6. Add error handling

### Working with DHT/libp2p

1. DHT operations are backend (Rust) via Tauri `invoke()`
2. File metadata: `FileMetadata` interface
3. Use `dhtService` from `src/lib/dht.ts`
4. Monitor DHT health with `DhtHealth` interface
5. Handle NAT traversal states

### Adding UI Components

1. Create in `src/lib/components/` or subdirectory
2. Use TypeScript for props
3. Follow accessibility best practices (Bits UI)
4. Add i18n support with `$t()`
5. Use existing UI primitives when possible
6. Make responsive with Tailwind

## Architecture Decisions

### Why Svelte 5?
- Modern reactive system with runes
- Better performance than Svelte 4
- Improved TypeScript support
- Simpler state management

### Why @mateothegreat/svelte5-router?
- Svelte 5 compatible
- Simple, declarative routing
- Type-safe route configuration
- Supports 404 handling

### Why libp2p?
- Industry-standard P2P networking
- Built-in NAT traversal
- Modular protocol stack
- Production-ready DHT implementation
- Strong privacy features

### Why Separate Blockchain?
- Custom parameters for file sharing use case
- No reliance on external networks
- Proof of Storage integration
- Mining rewards for network participation

### Why HD Wallets?
- Secure key management
- Multiple account support
- Industry-standard (BIP32/BIP39)
- Recovery phrase backup

## Security Considerations

### Implemented Security
- Input validation on all forms
- XSS protection in user content
- Secure random for IDs
- AES-256-GCM file encryption
- PBKDF2 key derivation
- HMAC stream authentication
- No centralized servers to compromise
- HD wallet security
- Private key protection

### Privacy Features
- Anonymous mode with mandatory relay/proxy routing
- SOCKS5 proxy support
- Circuit Relay v2 for IP hiding
- Encrypted file transfers
- No analytics tracking in anonymous mode

### Best Practices
- Never log private keys
- Sanitize all user inputs
- Validate file sizes and types
- Use secure WebRTC connections
- Verify peer identities
- Rate limiting on network operations

## Performance Optimizations

### Current Optimizations
- Virtual scrolling for large lists
- Lazy loading of components
- Efficient state management with Svelte 5
- Debounced search inputs
- Progressive file streaming
- Chunk-based file transfers
- Multi-source parallel downloads
- Bandwidth scheduling

### Monitoring
- Analytics service tracks performance
- DHT health monitoring
- Peer latency tracking
- Bandwidth usage metrics
- Mining hash rate monitoring

## Future Enhancements (Allowed)

### Phase 3+ Priorities
- [ ] WebAssembly for crypto operations
- [ ] Service workers for offline support
- [ ] Advanced compression for network traffic
- [ ] Database indexing for faster searches
- [ ] Enhanced file versioning UI
- [ ] Advanced relay discovery mechanisms
- [ ] Improved geolocation accuracy
- [ ] Mobile app version
- [ ] Hardware wallet support

### Phase 4+ Possibilities
- [ ] IPFS compatibility layer
- [ ] Advanced analytics dashboard
- [ ] Network visualization tools
- [ ] Automated testing framework
- [ ] Performance benchmarking suite

## What NOT to Implement

### Commercial & Piracy-Enabling Features

**Never add:**
- Global file search/discovery (could enable piracy)
- Price fields or payment systems
- File marketplace or trading
- Content recommendations
- Social features (comments, likes, reviews)
- Advertising systems
- Analytics that could track users
- Centralized market servers

### VPN & General Anonymity Network Features

**We are NOT building a VPN or anonymity network:**
- ❌ VPN service functionality
- ❌ General internet traffic routing (only P2P file transfer traffic)
- ❌ Exit node functionality (no routing of non-P2P traffic)
- ❌ Anonymous browsing capabilities
- ❌ Full anonymity network features
- ❌ Traffic mixing/onion routing

**What we DO support** (limited to file sharing):
- ✅ SOCKS5 proxy support (use existing proxies like Tor)
- ✅ Circuit Relay v2 (for NAT traversal, not anonymity)
- ✅ File encryption (protect file content)
- ✅ Anonymous mode (hide IP during P2P file transfers only)

**Why?**
1. Legal compliance - VPN/anonymity networks attract regulatory scrutiny
2. Resource focus - Different expertise and infrastructure required
3. Liability concerns - General traffic routing creates legal risks
4. Mission clarity - We're a file sharing platform, not a privacy network

## Deployment

```bash
# Development
npm run dev              # Web dev server
npm run tauri:dev        # Desktop app with hot reload

# Production build
npm run build            # Web production build
npm run tauri:build      # Desktop production build

# Testing
npm test                 # Run vitest tests
npm run test:watch       # Watch mode

# Type checking
npm run check            # TypeScript type check
```

## Troubleshooting

### Common Issues

1. **Extra `</script>` tags**: Check Svelte files end correctly
2. **Import errors**: Ensure all pages are properly imported in `App.svelte`
3. **Drag-drop failing**: Verify event handlers are attached to correct elements
4. **i18n not loading**: Check `setupI18n()` is called in `App.svelte`
5. **DHT not connecting**: Verify bootstrap nodes are reachable
6. **Mining not starting**: Check Geth service is initialized
7. **Tauri invoke errors**: Ensure backend commands are registered

### Debug Commands

```bash
# Check for syntax errors
npm run check

# Clean and rebuild
rm -rf node_modules dist
npm install
npm run build

# View DHT logs (in dev mode)
# Check browser console for DHT events

# Test vitest
npm test -- --reporter=verbose
```

## File Organization

```
src/
├── i18n/                    # Internationalization
│   └── i18n.ts
├── lib/
│   ├── components/          # Reusable components
│   │   ├── download/        # Download-specific components
│   │   ├── ui/              # UI primitives
│   │   └── wallet/          # Wallet components
│   ├── services/            # Backend services
│   │   ├── analyticsService.ts
│   │   ├── bandwidthScheduler.ts
│   │   ├── fileService.ts
│   │   ├── gethService.ts
│   │   ├── multiSourceDownloadService.ts
│   │   ├── networkService.ts
│   │   ├── p2pFileTransfer.ts
│   │   ├── peerSelectionService.ts
│   │   ├── privacyService.ts
│   │   ├── webrtcService.ts
│   │   └── ...
│   ├── stores/              # Additional stores
│   │   └── searchHistory.ts
│   ├── types/               # TypeScript type definitions
│   │   └── reputation.ts
│   ├── utils/               # Utility functions
│   │   └── validation.ts
│   ├── wallet/              # HD wallet implementation
│   │   ├── bip32.ts
│   │   ├── bip39.ts
│   │   ├── secp256k1.ts
│   │   └── ...
│   ├── dht.ts               # DHT configuration and utilities
│   ├── stores.ts            # Main state stores
│   ├── reputationStore.ts   # Reputation state
│   └── ...
├── locales/                 # Translation files
│   ├── en.json
│   ├── es.json
│   ├── ru.json
│   ├── zh.json
│   └── ko.json
├── pages/                   # Application pages
│   ├── Account.svelte
│   ├── Analytics.svelte
│   ├── Download.svelte
│   ├── Mining.svelte
│   ├── Network.svelte
│   ├── NotFound.svelte
│   ├── Proxy.svelte
│   ├── Relay.svelte
│   ├── Reputation.svelte
│   ├── Settings.svelte
│   └── Upload.svelte
├── routes/                  # Special routes
│   └── proxy-self-test.svelte
├── App.svelte               # Main app component
└── main.ts                  # Entry point
```

## Contact & Support

For questions about design decisions or implementation details, refer to:

1. This CLAUDE.md file
2. README.md for user-facing documentation
3. Git history for decision context
4. Community support via Zulip

---

_Last Updated: October 2024_
_Current Version: v0.1.0_
_Focus: Fully decentralized BitTorrent-like P2P sharing with blockchain integration_
