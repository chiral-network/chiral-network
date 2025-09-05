# Chiral Network - Decentralized P2P File Sharing Platform

## Overview

Chiral Network is a BitTorrent-like decentralized file sharing application designed for legitimate personal and organizational use. Built with Svelte, TypeScript, and Tauri, it provides a modern desktop experience for peer-to-peer file sharing focused on privacy, security, and efficient data distribution. The platform operates on a continuous seeding model where files are instantly available to the network once added, similar to BitTorrent's architecture.

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

- **Instant Sharing**: Files immediately start seeding when added (BitTorrent-style)
- **Drag & Drop Interface**: Simple, compact file addition
- **Content Hashing**: Automatic generation of unique file identifiers
- **Continuous Seeding**: Files remain available as long as they're in your list
- **Peer Statistics**: Real-time display of seeders and leechers
- **No Size Limits**: Share files of any size efficiently

### 2. Intelligent Download Management

- **Unified Download List**: Single interface for all download states
- **Smart Filtering**: View active, queued, or completed downloads
- **Priority Queue System**: High/Normal/Low priority settings
- **Concurrent Control**: Configurable max simultaneous downloads (1-10)
- **Auto-Start Queue**: Automatic processing of queued downloads
- **Pause/Resume Support**: Full control over individual downloads
- **Progress Tracking**: Real-time download progress with ETA

### 3. Network Monitoring & Peer Discovery

- **Real-Time Network Stats**: Monitor peers, bandwidth, and network health
- **Automatic Peer Discovery**: DHT-based peer finding with manual connect option
- **Peer Reputation**: Track and display peer reliability scores
- **Geographic Distribution**: View peer locations and regional statistics
- **Connection Management**: Direct control over peer connections
- **Network Health Indicators**: Visual status of network connectivity

### 4. Comprehensive Analytics Dashboard

- **Storage Metrics**: Track used space and file distribution
- **Bandwidth Usage**: Real-time upload/download statistics
- **Performance Analytics**: Monitor network efficiency
- **Network Activity**: Connection history and network-wide statistics
- **Resource Contribution**: Track your contribution to the network
- **Historical Data**: View trends over time

### 5. Proxy Network Support

- **Privacy Protection**: Route traffic through proxy nodes
- **Load Balancing**: Automatic distribution across multiple proxies
- **Latency Optimization**: Choose proxies based on performance
- **Custom Node Addition**: Add trusted proxy nodes manually
- **Bandwidth Aggregation**: Combine multiple proxy connections
- **Anonymous Browsing**: Hide your IP from other peers

### 6. Security & Privacy

- **End-to-End Encryption**: Optional file encryption (planned)
- **Wallet Security**: Secure credential management
- **Stream Authentication**: Verify data integrity during transfer
- **Anonymous Routing**: Proxy-based identity protection
- **No Commercial Tracking**: No marketplace means no transaction tracking

### 7. Mining & Network Security

- **CPU Mining**: Contribute computing power to secure the network
- **Mining Pool Support**: Solo or pool mining options
- **Real-Time Statistics**: Monitor hash rate, power usage, and efficiency
- **Reward Tracking**: Track blocks found and earnings
- **Adjustable Intensity**: Control CPU usage and thread allocation
- **Temperature Monitoring**: Keep track of system thermals

### 8. Comprehensive Settings

- **Storage Management**: Configure storage location and limits
- **Network Configuration**: Set bandwidth limits and connection parameters
- **Privacy Controls**: Enable encryption, proxy, and anonymous mode
- **Notification Preferences**: Customize alerts and notifications
- **Advanced Options**: Fine-tune DHT, chunk size, and cache settings
- **Import/Export**: Backup and restore settings

### 9. User Experience Enhancements

- **Dark/Light Theme**: Toggle between themes for comfort
- **Drag & Drop Upload**: Intuitive file upload interface
- **Real-Time Notifications**: Status updates via toast messages
- **Responsive Design**: Adaptive layout for different screen sizes
- **Clean Interface**: Focus on functionality without marketplace clutter

## Technical Implementation

### Frontend Stack

- **Svelte 5**: Reactive UI framework for efficient updates
- **TypeScript**: Type-safe development with better IDE support
- **Tailwind CSS**: Utility-first styling with dark mode support
- **Lucide Icons**: Consistent, customizable icon system
- **Bits UI**: Accessible component primitives

### Desktop Integration

- **Tauri 2**: Rust-based desktop runtime for native performance
- **Native File System**: Direct OS integration for file operations
- **System Tray**: Background operation support (planned)
- **Auto-Updates**: Seamless application updates (planned)

### Data Management

- **Svelte Stores**: Reactive state management
- **Local Storage**: Persistent user preferences
- **IndexedDB**: Large file metadata caching (planned)
- **WebSocket**: Real-time peer communication (planned)

## Architecture Decisions

### Why These Design Choices?

1. **Non-Commercial BitTorrent Model**

   - No marketplace or monetary transactions
   - Pure P2P file sharing for legitimate use
   - Continuous seeding model like BitTorrent
   - Suitable for personal, educational, and organizational use

2. **Privacy-Focused Architecture**

   - No transaction tracking or marketplace analytics
   - Anonymous routing options through proxy nodes
   - Local-first data storage

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

### File Sharing (BitTorrent-Style)

1. Navigate to Upload page (renamed "Shared Files")
2. Click "Add Files" or drag & drop anywhere on the card
3. Files instantly start seeding (no upload button needed)
4. View seeders/leechers count for each file
5. Copy file hash to share with others
6. Files continue seeding until manually removed

### Downloading Files

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

### Phase 1: Core Functionality (Completed)

- ✅ BitTorrent-style instant seeding
- ✅ Unified download management
- ✅ Network monitoring & peer discovery
- ✅ Proxy support for anonymity
- ✅ Analytics dashboard
- ✅ CPU mining with pool support
- ✅ Comprehensive settings management

### Phase 2: P2P Integration (Next)

- [ ] libp2p integration
- [ ] DHT implementation
- [ ] Real peer discovery
- [ ] WebRTC data channels
- [ ] NAT traversal

### Phase 3: Advanced Features

- [ ] End-to-end encryption
- [ ] File versioning
- [ ] Selective sync
- [ ] Bandwidth scheduling
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
- No commercial features to prevent misuse

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

- Adding marketplace features
- Commercial tracking systems
- Features that could enable piracy

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

## Acknowledgments

Special thanks to all design teams whose concepts shaped this implementation:

- Teams: Beluga, Dolphin, Otter, Seal, Seawolf, Squid, Amoeba, Tuna, Walrus, Whale
- Focus on legitimate P2P use cases
- BitTorrent-inspired continuous seeding model
- Privacy-first architecture

## Support

For issues, questions, or suggestions:

- GitHub Issues: [Report bugs or request features]
- Documentation: [Comprehensive guides]
- Community: [Join our Discord]

## Disclaimer

Chiral Network is designed for legitimate file storage and sharing. Users are responsible for ensuring they have the rights to share any content they upload. The platform does not include marketplace features to prevent commercial misuse or piracy.

---

Built for a decentralized, privacy-focused future 🛡️
