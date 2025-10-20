# Project Roadmap

This roadmap outlines the development milestones for Chiral Network, from initial concept to future enhancements.

## Vision

Build a fully decentralized, privacy-focused peer-to-peer file sharing platform that prioritizes legitimate use cases, user privacy, and network resilience.

## Development Phases

### Phase 1: Core Infrastructure ✅ COMPLETED

**Goal**: Establish basic UI and application structure

**Completed Features**:
- ✅ Modern desktop interface (Svelte 5 + Tauri 2)
- ✅ Real-time file management dashboard
- ✅ Network monitoring & peer discovery
- ✅ Proxy support for anonymity
- ✅ Analytics dashboard with real metrics
- ✅ CPU mining with Geth integration
- ✅ Comprehensive settings management
- ✅ Multi-language support (EN, ES, ZH, KO, RU)

**Timeline**: Q1-Q2 2024

---

### Phase 2: P2P Network Infrastructure ✅ COMPLETED

**Goal**: Implement production-ready P2P networking

**Completed Features**:
- ✅ Full libp2p v0.54 integration
- ✅ Production-ready Kademlia DHT implementation
- ✅ Real peer discovery with mDNS and libp2p
- ✅ Complete WebRTC data channel support for P2P transfers
- ✅ NAT traversal (AutoNAT v2, Circuit Relay v2, DCUtR, mDNS)
- ✅ Advanced peer selection and reputation system
- ✅ Multi-source downloads with parallel chunk transfers
- ✅ SOCKS5 proxy support for privacy
- ✅ Bitswap protocol for efficient block exchange
- ✅ Comprehensive analytics with real-time metrics tracking

**Timeline**: Q3-Q4 2024

---

### Phase 3: Core P2P Features 🚧 IN PROGRESS

**Goal**: Complete essential P2P functionality

**Completed**:
- ✅ File Upload Encryption (AES-256-GCM with PBKDF2)
- ✅ File Download Decryption with key management
- ✅ WebRTC Encryption for P2P chunk transfers
- ✅ Key Exchange UI for encrypted sharing
- ✅ Real P2P File Transfer protocol (WebRTC-based)
- ✅ File Versioning System
- ✅ Advanced Bandwidth Scheduling
- ✅ GUI NAT Configuration
- ✅ Relay Server Toggle (user-configurable)
- ✅ Public Relay Infrastructure with deployment scripts
- ✅ HD Wallet implementation (BIP32/BIP39)
- ✅ Reputation system with trust levels

**In Progress**:
- 🚧 Enhanced file versioning UI
- 🚧 Advanced relay discovery mechanisms
- 🚧 Performance optimizations

**Planned**:
- 📅 Selective sync capabilities
- 📅 Advanced chunk verification
- 📅 Improved geolocation accuracy

**Timeline**: Q1 2025

---

### Phase 4: Security & Privacy Enhancements 📅 PLANNED

**Goal**: Strengthen security and privacy features

**Planned Features**:
- 📅 Relay reservation authentication
- 📅 Rate limiting for AutoNAT probes
- 📅 Anti-amplification safeguards
- 📅 Enhanced encryption key management
- 📅 Tor integration improvements
- 📅 Advanced privacy modes
- 📅 Hardware wallet support (Ledger, Trezor)
- 📅 Encrypted metadata storage
- 📅 Perfect forward secrecy for transfers

**Timeline**: Q2-Q3 2025

---

### Phase 5: Performance & Scalability 📅 PLANNED

**Goal**: Optimize for large-scale usage

**Planned Features**:
- 📅 WebAssembly for cryptographic operations
- 📅 Service workers for offline support
- 📅 Advanced compression for network traffic
- 📅 Database indexing for faster searches
- 📅 Improved caching mechanisms
- 📅 Connection pooling optimization
- 📅 Memory usage optimization
- 📅 Reduced startup time
- 📅 Better resource management

**Timeline**: Q3-Q4 2025

---

### Phase 6: Cross-Platform Support 📅 PLANNED

**Goal**: Expand platform availability

**Planned Features**:
- 📅 Mobile app (iOS & Android)
- 📅 Web-based interface
- 📅 Browser extension
- 📅 Headless server mode (production)
- 📅 ARM architecture support
- 📅 Docker container optimization
- 📅 Kubernetes deployment guides

**Timeline**: Q4 2025 - Q1 2026

---

### Phase 7: Advanced Features 📅 PLANNED

**Goal**: Add sophisticated functionality

**Planned Features**:
- 📅 IPFS compatibility layer
- 📅 Advanced analytics dashboard
- 📅 Network visualization tools
- 📅 Smart contract improvements
- 📅 Automated testing framework
- 📅 Performance benchmarking suite
- 📅 Advanced file metadata (tags, categories)
- 📅 Collaborative editing features
- 📅 Integration APIs for third-party apps

**Timeline**: Q2-Q4 2026

---

## Feature Requests & Community Input

We welcome community input on our roadmap! Here's how to contribute:

### Suggesting New Features

1. **Check existing issues** to avoid duplicates
2. **Create feature request** on GitHub Issues
3. **Provide context**: Use case, benefits, implementation ideas
4. **Engage in discussion** with maintainers and community

### Voting on Features

- 👍 React to issues with thumbs up for features you want
- Comment with your use case to help prioritize
- Star the repository to show general support

### What We Won't Add

To maintain focus on legitimate, privacy-focused use:

❌ **Never**:
- Centralized market servers
- Global file search/discovery (piracy risk)
- Price/payment systems
- Commercial tracking
- Social features (likes, comments, reviews)
- Advertising systems

## Current Sprint Focus

**Q1 2025 Priorities**:

1. **Enhanced File Versioning UI**
   - Better version comparison
   - Rollback functionality
   - Visual diff for text files

2. **Performance Optimizations**
   - Reduce memory footprint
   - Faster DHT queries
   - Improved chunk scheduling

3. **Advanced Relay Discovery**
   - Better relay selection algorithm
   - Relay reputation integration
   - Automatic relay fallback

4. **Bug Fixes & Stability**
   - Address reported issues
   - Improve error handling
   - Better logging

## Milestone Tracking

Track detailed progress:
- **GitHub Projects**: [Project Board](https://github.com/chiral-network/chiral-network/projects)
- **GitHub Milestones**: [Milestones](https://github.com/chiral-network/chiral-network/milestones)
- **Release Notes**: [Releases](https://github.com/chiral-network/chiral-network/releases)

## Version History

### v0.1.0 (Current) - October 2024
- Initial release with core P2P functionality
- Full DHT and libp2p integration
- NAT traversal support
- Reputation system
- HD wallets and mining

### v0.2.0 (Planned) - Q1 2025
- Enhanced file versioning
- Performance improvements
- Advanced relay discovery
- Bug fixes and stability

### v0.3.0 (Planned) - Q2 2025
- Security enhancements
- Hardware wallet support
- Improved privacy features

### v1.0.0 (Target) - Q4 2025
- Production-ready release
- Full feature set
- Comprehensive documentation
- Stable API

## Contributing to the Roadmap

Want to help shape the future of Chiral Network?

1. **Read** [Contributing Guide](contributing.md)
2. **Join** discussions on GitHub
3. **Submit** feature requests with detailed proposals
4. **Vote** on features that matter to you
5. **Contribute** code for planned features

## Roadmap Updates

This roadmap is updated quarterly to reflect:
- Completed features
- Shifted priorities
- New community requests
- Technical discoveries
- Resource availability

**Last Updated**: October 2024
**Next Review**: January 2025

---

## Questions?

- **About planned features**: Open a GitHub Discussion
- **About timelines**: Check project milestones
- **About priorities**: Review current sprint in Projects

[Back to Documentation Index](index.md)
