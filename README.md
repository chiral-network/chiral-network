# Chiral Network - Decentralized P2P File Sharing Platform

## Overview

Chiral Network is a decentralized peer-to-peer file storage and sharing system that combines blockchain technology with distributed hash table (DHT) based file storage. The system creates a separate Ethereum-compatible blockchain network with custom parameters for handling transactions while using a DHT-based approach similar to IPFS for completely decentralized file storage and retrieval without any centralized market servers.

## Design Philosophy

This implementation synthesizes concepts from multiple design teams, focusing on legitimate use cases for distributed storage:

### Core Architecture Choices

- **DHT-based P2P Network**: Utilizing distributed hash tables for decentralized file discovery and routing
- **Content-Addressed Storage**: Files identified by cryptographic hashes ensuring integrity
- **libp2p Protocol Stack**: Industry-standard P2P networking with NAT traversal and peer discovery
- **Hybrid Node Architecture**: Supporting provider, consumer, proxy, and relay node types
- **Proof-of-Storage Consensus**: Nodes validate storage claims through periodic challenges

### Network Model

- **Non-Commercial Focus**: No marketplace or trading features to prevent misuse
- **Community-Driven**: Focus on collaborative storage and sharing
- **Privacy-First**: Anonymous routing through proxy nodes
- **Resource Sharing**: Contribute storage and bandwidth to the network

## Key Features

### 1. File Sharing & Seeding

- ✅ **Instant Sharing**: Files immediately processed and metadata published to DHT
- ✅ **Drag & Drop Interface**: Simple, compact file addition with real-time feedback
- ✅ **Content Hashing**: SHA-256 hash generation for unique file identifiers
- ✅ **DHT Metadata Distribution**: File information shared via distributed hash table
- ✅ **Network Integration**: Files registered with P2P network for discovery
- ✅ **No Size Limits**: Share files of any size efficiently

### 2. Intelligent Download Management

- ✅ **Unified Download List**: Single interface for all download states
- ✅ **Smart Filtering**: View active, queued, or completed downloads
- ✅ **Priority Queue System**: High/Normal/Low priority settings
- ✅ **Concurrent Control**: Configurable max simultaneous downloads (1-10)
- ✅ **Auto-Start Queue**: Automatic processing of queued downloads
- ✅ **Pause/Resume Support**: Full control over individual downloads
- ✅ **Progress Tracking**: Real-time download progress with ETA
- ✅ **P2P File Transfer**: Real peer-to-peer file transfer via WebRTC data channels

### 3. Network Monitoring & Peer Discovery

- ✅ **Real-Time Network Stats**: Monitor peers, bandwidth, and network health with real analytics
- ✅ **Automatic Peer Discovery**: DHT-based peer finding with manual connect option
- ✅ **Reputation-Based Peer Selection**: Track peer reliability, latency, and bandwidth for intelligent routing
- ✅ **Multi-Source Downloads**: Parallel downloads from multiple peers for faster transfers
- ❌ **Geographic Distribution**: View real peer locations and regional statistics (geolocation not implemented)
- ✅ **Connection Management**: Direct control over peer connections with disconnect functionality
- ✅ **Network Health Indicators**: Visual status of network connectivity
- ✅ **NAT Traversal**: AutoNAT v2 reachability detection and Circuit Relay v2 for NAT'd peers

### 4. Comprehensive Analytics Dashboard

- ✅ **Storage Metrics**: Track used space and file distribution
- ✅ **Bandwidth Usage**: Real-time upload/download statistics with persistent tracking
- ✅ **Performance Analytics**: Monitor network efficiency with transfer speed metrics
- ✅ **Network Activity**: Connection history and network-wide statistics
- ✅ **Resource Contribution**: Track your contribution to the network with real bandwidth/storage metrics
- ✅ **Historical Data**: View bandwidth and contribution trends over time (mining earnings use mock data)

### 5. Proxy & NAT Traversal Support

- ✅ **SOCKS5 Proxy Support**: Route P2P traffic through SOCKS5 proxies for privacy
- ✅ **Circuit Relay v2**: Automatic relay reservation for NAT traversal
- ✅ **AutoNAT v2**: Automatic reachability detection (Public/Private/Unknown)
- ✅ **Relay Health Monitoring**: Track relay connection status and performance
- ✅ **Custom Relay Nodes**: Add trusted relay nodes manually
- ✅ **Headless Relay Configuration**: CLI flags for --enable-autorelay, --relay, --autonat-server
- ✅ **Privacy Protection**: Route traffic through proxy nodes with enforced SOCKS5 routing
- ❌ **Load Balancing**: Automatic distribution across multiple proxies (no parallel downloads or file segmentation)
- ❌ **Latency Optimization**: Choose proxies based on performance (no download process uses latency framework)
- ✅ **Custom Node Addition**: Add trusted proxy nodes manually
- ❌ **Bandwidth Aggregation**: Combine multiple proxy connections (no actual combining of multiple proxy connections)
- ✅ **Real Proxy Management**: Backend proxy connection and management
- ❌ **Public Relay Infrastructure**: Dedicated relay daemon deployment (in progress)

### 6. Security & Privacy

- ✅ **End-to-End Encryption**: AES-256-GCM encryption with PBKDF2 key derivation (can be enabled in Settings)
- ✅ **Wallet Security**: Secure credential management with HD wallets
- ✅ **Stream Authentication**: HMAC-based cryptographic verification of data integrity during file transfers
- ✅ **Anonymous Routing**: Hide your IP from other peers with proxy-enforced anonymous mode
- ✅ **No Commercial Tracking**: No marketplace means no transaction tracking

### 7. Mining & Network Security

- ✅ **CPU Mining**: Real blockchain mining with Geth integration
- ❌ **Mining Pool Support**: Pool selection UI with mock data (actual pool mining not implemented)
- ❌ **Real-Time Statistics**: Monitor hash rate, power usage, and efficiency (power and efficiency are mock data)
- ❌ **Reward Tracking**: Block counting works but rewards use hardcoded values (not actual earnings)
- ✅ **Adjustable Intensity**: Control CPU usage and thread allocation
- ✅ **Temperature Monitoring**: Keep track of system thermals

### 8. Comprehensive Settings

- ✅ **Storage Management**: Configure storage location and limits
- ✅ **Network Configuration**: Set bandwidth limits and connection parameters
- ✅ **Advanced Bandwidth Scheduling**: Set different bandwidth limits for specific times and days
- ✅ **Privacy Controls**: Mandatory encryption, proxy support, and anonymous mode
- ✅ **Notification Preferences**: Customize alerts and notifications
- ✅ **Advanced Options**: Fine-tune DHT, chunk size, and cache settings (configurable through UI)
- ✅ **Import/Export**: Backup and restore settings
- ✅ **Multi-language Support**: English, Spanish, Chinese, Korean

## NAT Traversal & Network Reachability

### Current Implementation Status

#### ✅ Implemented Features

1. **AutoNAT v2 Reachability Detection**
   - Automatic 30-second probe cycles
   - Real-time reachability status (Public/Private/Unknown)
   - Confidence scoring for reachability state
   - Reachability history tracking
   - Headless CLI support: `--disable-autonat`, `--autonat-probe-interval`, `--autonat-server`

2. **Circuit Relay v2 with AutoRelay**
   - Automatic relay candidate detection from bootstrap nodes
   - Dynamic relay reservation for NAT'd peers
   - Relay health monitoring and connection tracking
   - Headless CLI support: `--enable-autorelay`, `--disable-autorelay`, `--relay <multiaddr>`
   - Configurable preferred relay nodes (GUI + CLI)

3. **Observed Address Tracking**
   - libp2p identify protocol integration
   - Persistent tracking of externally observed addresses
   - Address change detection and logging

4. **SOCKS5 Proxy Integration**
   - P2P traffic routing through SOCKS5 proxies
   - CLI flag: `--socks5-proxy <address>`

#### ✅ GUI Configuration (Recently Implemented)

1. **Settings UI for NAT Traversal**
   - AutoNAT toggle with configurable probe interval (10-300s)
   - Custom AutoNAT servers textarea (multiaddr format)
   - AutoRelay toggle for Circuit Relay v2
   - Preferred relay nodes textarea (multiaddr format)
   - All settings persist to localStorage

2. **Real-Time Reachability Display**
   - Live NAT status badge (Public/Private/Unknown)
   - Confidence scoring display (High/Medium/Low)
   - Observed addresses from libp2p identify
   - Reachability history table with timestamps
   - Last probe time and state change tracking
   - AutoNAT enabled/disabled indicator

#### ✅ Public Relay Infrastructure (Recently Implemented)

1. **Dedicated Circuit Relay v2 Daemon**
   - Standalone relay node binary (`chiral-relay`)
   - Configurable reservation/circuit limits
   - Persistent peer identity across restarts
   - JSON metrics export for monitoring
   - Production-ready with systemd/Docker support
   - Location: `relay-infrastructure/`

2. **Deployment Scripts**
   - `start-relay.sh` - Bootstrap script with auto IP detection
   - `stop-relay.sh` - Graceful shutdown with fallback force kill
   - `status-relay.sh` - Comprehensive status and metrics display
   - Environment variable configuration
   - PID file management

3. **Documentation**
   - `relay-infrastructure/README.md` - Quick start guide
   - `relay-infrastructure/DEPLOYMENT.md` - Production deployment
   - systemd service examples
   - Docker/docker-compose configs
   - Cloud deployment guides (AWS, GCP, DigitalOcean)
   - Prometheus metrics integration

#### ❌ Not Yet Implemented

1. **Advanced Security**
   - Relay reservation authentication
   - Rate limiting for AutoNAT probes
   - Anti-amplification safeguards

3. **Resilience Testing**
   - End-to-end NAT traversal scenarios
   - Private↔Public connection tests
   - Private↔Private relay/hole-punch tests
   - Failure fallback validation

### Headless Mode NAT Configuration

```bash
# Enable AutoNAT with custom probe interval
./chiral-network --autonat-probe-interval 60

# Disable AutoNAT
./chiral-network --disable-autonat

# Add custom AutoNAT servers
./chiral-network --autonat-server /ip4/1.2.3.4/tcp/4001/p2p/QmPeerId

# Enable AutoRelay with custom relay nodes
./chiral-network --relay /ip4/relay.example.com/tcp/4001/p2p/QmRelayId

# Route P2P through SOCKS5 proxy
./chiral-network --socks5-proxy 127.0.0.1:9050
```

### NAT Traversal Architecture

The network uses a multi-layered approach to ensure connectivity:

1. **Direct Connection** (fastest): For publicly reachable peers
2. **Hole Punching** (DCUtR): For symmetric NAT traversal
3. **Circuit Relay** (fallback): For restrictive NATs
4. **SOCKS5 Proxy** (privacy): For anonymous routing

## Technical Implementation

### Frontend Stack

- **Svelte 5**: Reactive UI framework for efficient updates
- **TypeScript**: Type-safe development with better IDE support
- **Tailwind CSS**: Utility-first styling
- **Lucide Icons**: Consistent, customizable icon system
- **Bits UI**: Accessible component primitives

### Desktop Integration

- **Tauri 2**: Rust-based desktop runtime for native performance
- **Native File System**: Direct OS integration for file operations
- **System Tray**: Background operation support
- **Multi-language Support**: English, Spanish, Chinese, Korean

### P2P Network Infrastructure

- **libp2p v0.54**: Full peer-to-peer networking stack with production features
- **Kademlia DHT**: Distributed hash table for file metadata storage and peer discovery
- **WebRTC**: Direct peer-to-peer data channels for file transfers
- **NAT Traversal**:
  - AutoNAT v2 for reachability detection
  - Circuit Relay v2 with AutoRelay for NAT'd peers
  - DCUtR (Direct Connection Upgrade through Relay) for hole punching
  - mDNS for local peer discovery
- **Noise Protocol**: Modern cryptographic transport security
- **Bitswap Protocol**: Efficient block exchange for chunked file transfers
- **SOCKS5 Proxy**: Privacy-focused P2P traffic routing
- **Multi-Source Downloads**: Parallel chunk downloading from multiple peers
- **Reputation System**: Track peer reliability, bandwidth, and latency for intelligent peer selection

## Architecture Decisions

### Why These Design Choices?

1. **Decentralized BitTorrent Model**
   - No centralized marketplace or intermediaries
   - Pure P2P file sharing for legitimate use
   - Continuous seeding model like BitTorrent
   - Fully decentralized peer discovery via DHT
   - Suitable for personal, educational, and organizational use

2. **Privacy-Focused Architecture**
   - No centralized servers to track users
   - Anonymous routing options through proxy nodes
   - Local-first data storage
   - Decentralized peer discovery prevents tracking

3. **Community Resource Sharing**
   - Contribute storage space to help others
   - Share bandwidth for network resilience
   - Mine blocks to secure the network
   - Build reputation through reliable service

4. **Proof-of-Work Security**
   - CPU-friendly mining algorithm
   - Decentralized consensus mechanism
   - Fair reward distribution
   - Energy-efficient compared to ASIC mining

5. **Progressive Decentralization**
   - Start with mock data for immediate usability
   - Gradually integrate real P2P networking
   - Maintain backwards compatibility

## Setup and Testing

```bash
# Clone the repository
git clone https://github.com/yourusername/chiral-network.git
cd chiral-network

# Install dependencies
npm install

# Run in development mode
npm run dev       # Web development server
npm run tauri dev # Desktop app

# Build for production
npm run build       # Web production build
npm run tauri build # Desktop production build

# Run tests
npm test

# Run tests in watch mode
npm run test:watch
```

### Getting Started

1. **Launch the application** - Opens to the Download page
2. **Configure settings** - Adjust storage, network, and privacy preferences
3. **Connect to network** - Automatic peer discovery starts
4. **Add files to share** - Drag & drop or click to add files (instant seeding)
5. **Download files** - Enter file hash to download from peers
6. **Start mining** (optional) - Earn rewards by securing the network
7. **Monitor activity** - Track your contributions and network stats

### File Sharing (BitTorrent-Style UI)

1. Navigate to Upload page (renamed "Shared Files")
2. Click "Add Files" or drag & drop anywhere on the card
3. Files are processed and metadata published to DHT network
4. View connected peers and network statistics
5. Copy file hash to share with others
6. Files remain available as long as application is running

### File Discovery & Network

1. Go to Download page
2. Enter file hash received from peer
3. Click Search & Download
4. Monitor progress in queue
5. Access completed files locally

### Network Participation

1. Keep application running to support network
2. Configure proxy nodes for privacy
3. Enable mining to earn rewards
4. Monitor your contributions in Analytics
5. Maintain good peer reputation

### Mining for Rewards

1. Navigate to Mining page
2. Select mining pool or solo mining
3. Choose number of CPU threads
4. Set mining intensity
5. Click Start Mining
6. Monitor hash rate and rewards
7. Track found blocks in history

### Bandwidth Scheduling

1. Navigate to Settings page
2. Scroll to Bandwidth Scheduling section
3. Enable "Enable Bandwidth Scheduling" toggle
4. Click "Add Schedule" to create a new schedule
5. Configure schedule:
   - Set schedule name
   - Select start and end times (24-hour format)
   - Choose days of week when schedule applies
   - Set upload and download limits (KB/s, 0 = unlimited)
6. Toggle schedule on/off with checkbox
7. Create multiple schedules for different time periods
8. Scheduler automatically applies appropriate limits based on current time

## Legitimate Use Cases

### Personal Use

- **Backup & Sync**: Distributed backup of personal files
- **Family Sharing**: Share photos and videos with family
- **Cross-Device Access**: Access your files from any device

### Educational

- **Research Data**: Share research datasets with colleagues
- **Course Materials**: Distribute educational content
- **Collaborative Projects**: Share project files with team members

### Organizational

- **Internal Distribution**: Share company documents securely
- **Backup Solution**: Distributed backup for critical data
- **Branch Offices**: Efficient file distribution across locations

## Roadmap & Future Enhancements

### Phase 1: Core UI & Infrastructure (Completed)

- ✅ Modern desktop interface (Svelte + Tauri)
- ✅ Real-time file management dashboard
- ✅ Network monitoring & peer discovery
- ✅ Proxy support for anonymity
- ✅ Analytics dashboard with real metrics
- ✅ CPU mining with pool support
- ✅ Comprehensive settings management
- ✅ Multi-language support (EN, ES, ZH, KO)

### Phase 2: P2P Network Infrastructure (Completed)

- ✅ Full libp2p v0.54 integration with all production features
- ✅ Production-ready Kademlia DHT implementation
- ✅ Real peer discovery with mDNS and libp2p
- ✅ Complete WebRTC data channel support for P2P transfers
- ✅ NAT traversal (AutoNAT v2, Circuit Relay v2, DCUtR, mDNS)
- ✅ Advanced peer selection and reputation system
- ✅ Multi-source downloads with parallel chunk transfers
- ✅ SOCKS5 proxy support for privacy
- ✅ Bitswap protocol for efficient block exchange
- ✅ Comprehensive analytics with real-time metrics tracking

### Phase 3: Core P2P Features (In Progress)

- ✅ **File Upload Encryption**: AES-256-GCM encryption with PBKDF2 key derivation for uploaded files
- ✅ **File Download Decryption**: Key management and decryption for downloaded files
- ✅ **WebRTC Encryption**: Encrypted P2P chunk transfers
- ✅ **Key Exchange UI**: Recipient public key input for encrypted sharing
- ✅ **Real P2P File Transfer**: Production-ready WebRTC-based transfer protocol
- ✅ **File Versioning System**: Track and manage multiple versions of files
- ✅ **Advanced Bandwidth Scheduling**: Time-based bandwidth limits with day-of-week rules
- ✅ **GUI NAT Configuration**: Settings UI for AutoNAT, AutoRelay, and relay preferences
- ✅ **Public Relay Infrastructure**: Dedicated relay daemon with deployment scripts
- [ ] **Selective Sync Capabilities**: Choose which files to download
- [ ] **Mobile Applications**: iOS and Android support

### Phase 4: Enterprise Features

- [ ] Access control lists
- [ ] Organization management
- [ ] Audit logging
- [ ] Compliance tools
- [ ] API for integrations

## Performance Optimizations

### Current Optimizations

- Virtual scrolling for large lists
- Lazy loading of components
- Efficient state management
- Debounced search inputs
- Progressive file streaming

### Planned Optimizations

- WebAssembly for crypto operations
- Service workers for offline support
- Compression for network traffic
- Database indexing for searches
- Parallel download streams

## Security Considerations

### Implemented Security

- Input validation on all forms
- XSS protection in user content
- CORS configuration for API calls
- Secure random for IDs
- AES-256-GCM file encryption for uploads
- PBKDF2 key derivation for encryption
- ECIES key exchange infrastructure
- File download decryption with key management
- WebRTC encrypted chunk transfers
- Key exchange UI for recipient-specific encryption
- No centralized servers to compromise
- Fully decentralized architecture prevents single points of failure

### Planned Security

- ✅ Key exchange UI for encrypted sharing
- File encryption at rest
- Signed software updates
- Two-factor authentication
- Hardware security module support
- Audit logging

## Contributing

We welcome contributions that align with our non-commercial, privacy-focused vision:

- Code improvements and bug fixes
- Security enhancements
- Performance optimizations
- Documentation improvements
- Translation support

Please avoid:

- Adding centralized market servers
- Commercial tracking systems
- Features that could enable piracy
- Centralized intermediaries that compromise decentralization

## License

MIT License - See LICENSE file for details

## Key Technical Decisions

### BitTorrent-Like Architecture

- **Instant Seeding**: Files immediately available when added
- **Continuous Availability**: Files remain accessible while in your list
- **Peer Statistics**: Track seeders and leechers for each file
- **No Pending State**: Eliminates confusion between "uploading" and "shared"

### UI/UX Improvements

- **Unified Lists**: Single view for downloads and uploads
- **Compact Design**: Removed large drop zones for cleaner interface
- **Smart Filtering**: Contextual filters for better organization
- **Drag Anywhere**: Entire cards accept drag-and-drop
- **Fully Decentralized**: No market servers, pure P2P file discovery via DHT

## Acknowledgments

Special thanks to all design teams whose concepts shaped this implementation:

- Focus on legitimate P2P use cases
- BitTorrent-inspired continuous seeding model
- Privacy-first architecture

## Support

For issues, questions, or suggestions:

- GitHub Issues: [Report bugs or request features]
- Documentation: [Comprehensive guides]
- Community: Using Zulip

## Disclaimer

Chiral Network is designed for legitimate file storage and sharing. Users are responsible for ensuring they have the rights to share any content they upload. The platform uses a fully decentralized architecture without centralized market servers to ensure true peer-to-peer operation and prevent commercial misuse or piracy.

---

Built for a decentralized, privacy-focused future 🛡️
