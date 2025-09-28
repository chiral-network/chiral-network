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
- ❌ **Network Integration**: Files registered with P2P network for discovery (currently local-only storage)
- ✅ **No Size Limits**: Share files of any size efficiently

### 2. Intelligent Download Management

- ✅ **Unified Download List**: Single interface for all download states
- ✅ **Smart Filtering**: View active, queued, or completed downloads
- ✅ **Priority Queue System**: High/Normal/Low priority settings
- ✅ **Concurrent Control**: Configurable max simultaneous downloads (1-10)
- ✅ **Auto-Start Queue**: Automatic processing of queued downloads
- ✅ **Pause/Resume Support**: Full control over individual downloads
- ✅ **Progress Tracking**: Real-time download progress with ETA
- ❌ **P2P File Transfer**: Actual network downloads from peers (currently local-only storage)

### 3. Network Monitoring & Peer Discovery

- ❌ **Real-Time Network Stats**: Monitor peers, bandwidth, and network health (bandwidth and network health uses mock data)
- ✅ **Automatic Peer Discovery**: DHT-based peer finding with manual connect option
- ✅ **Peer Reputation**: Track and display peer reliability scores
- ❌ **Geographic Distribution**: View real peer locations and regional statistics (real geolocation not implemented)
- ❌ **Connection Management**: Direct control over peer connections (not implemented)
- ✅ **Network Health Indicators**: Visual status of network connectivity

### 4. Comprehensive Analytics Dashboard

- ✅ **Storage Metrics**: Track used space and file distribution
- ❌ **Bandwidth Usage**: Real-time upload/download statistics (uses mock data)
- ❌ **Performance Analytics**: Monitor network efficiency (no real network performance measurements)
- ❌ **Network Activity**: Connection history and network-wide statistics (uses mock data)
- ❌ **Resource Contribution**: Track your contribution to the network (rewards use mock data)
- ❌ **Historical Data**: View trends over time (earnings history uses mock data)

### 5. Proxy Network Support

- ❌ **Privacy Protection**: Route traffic through proxy nodes (no traffic routing implemented)
- ❌ **Load Balancing**: Automatic distribution across multiple proxies (no parallel downloads, file segmentation, or multi-source downloads)
- ❌ **Latency Optimization**: Choose proxies based on performance (no download process uses latency framework)
- ✅ **Custom Node Addition**: Add trusted proxy nodes manually
- ❌ **Bandwidth Aggregation**: Combine multiple proxy connections (no actual combining of multiple proxy connections)
- ✅ **Real Proxy Management**: Backend proxy connection and management

### 6. Security & Privacy

- ❌ **End-to-End Encryption**: AES-256-GCM encryption with PBKDF2 key derivation (there is encryption infrastructure, but it is not applied to uploads and downloads)
- ✅ **Wallet Security**: Secure credential management with HD wallets
- ❌ **Stream Authentication**: Cryptographic verification of data integrity (no actual stream authentication occurs during file transfers)
- ❌ **Anonymous Routing**: Hide your IP from other peers (no IP hiding or anonymization implemented)
- ✅ **No Commercial Tracking**: No marketplace means no transaction tracking

### 7. Mining & Network Security

- ✅ **CPU Mining**: Real blockchain mining with Geth integration
- ❌ **Mining Pool Support**: Solo or pool mining with real hashrate monitoring (no mining pool protocol implemented)
- ❌ **Real-Time Statistics**: Monitor hash rate, power usage, and efficiency (power and efficiency are mock data)
- ❌ **Reward Tracking**: Track blocks found and actual earnings (rewards not calculated from actual block data)
- ✅ **Adjustable Intensity**: Control CPU usage and thread allocation
- ✅ **Temperature Monitoring**: Keep track of system thermals

### 8. Comprehensive Settings

- ✅ **Storage Management**: Configure storage location and limits
- ✅ **Network Configuration**: Set bandwidth limits and connection parameters
- ❌ **Privacy Controls**: Enable encryption, proxy, and anonymous mode (anonymous mode not implemented)
- ✅ **Notification Preferences**: Customize alerts and notifications
- ❌ **Advanced Options**: Fine-tune DHT, chunk size, and cache settings (backend uses hardcoded values)
- ✅ **Import/Export**: Backup and restore settings
- ✅ **Multi-language Support**: English, Spanish, Chinese, Korean

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

- **libp2p v0.54**: Full peer-to-peer networking stack
- **Kademlia DHT**: Distributed hash table for metadata storage
- **WebRTC**: Direct peer-to-peer data channels
- **NAT Traversal**: STUN, relay, and mDNS support
- **Noise Protocol**: Modern cryptographic transport security

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

## Installation & Setup

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
```

## Usage Guide

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

- ✅ Full libp2p v0.54 integration
- ✅ Production-ready Kademlia DHT implementation
- ✅ Real peer discovery with mDNS and libp2p
- ✅ Complete WebRTC data channel support
- ✅ NAT traversal (STUN, libp2p relay, mDNS)
- ✅ Advanced peer selection and reputation system

### Phase 3: Core P2P Features (In Progress)

- ✅ End-to-end encryption (AES-256-GCM with PBKDF2)
- [ ] Real P2P file transfer protocol
- [ ] File versioning system
- [ ] Selective sync capabilities
- [ ] Advanced bandwidth scheduling
- [ ] Mobile applications

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
- No centralized servers to compromise
- Fully decentralized architecture prevents single points of failure

### Planned Security

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

- **Instant Seeding**: Files immediately available when added (no upload step)
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
