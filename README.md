# Chiral Network

> **Decentralized P2P File Sharing Platform**

Chiral Network is a BitTorrent-like peer-to-peer file storage and sharing system that combines blockchain technology with DHT-based distributed storage. Built with privacy, security, and legitimate use in mind.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Build Status](https://github.com/Aery1e/chiral-network/workflows/test/badge.svg)](https://github.com/Aery1e/chiral-network/actions)

## ✨ Features

- 🔄 **Instant File Sharing** - BitTorrent-style continuous seeding
- 🌐 **Fully Decentralized** - No central servers, DHT-based discovery
- 🔒 **End-to-End Encryption** - AES-256-GCM with PBKDF2 key derivation
- 🛡️ **Privacy-First** - Circuit Relay v2, AutoNAT v2, SOCKS5 proxy support
- ⚡ **Multi-Source Downloads** - Parallel downloads from multiple peers
- 🎯 **Reputation System** - Intelligent peer selection based on reliability
- 💼 **HD Wallets** - BIP32/BIP39 secure wallet management
- ⛏️ **CPU Mining** - Secure the network and earn rewards
- 🌍 **Multi-Language** - English, Spanish, Russian, Chinese, Korean
- 🚀 **NAT Traversal** - Works behind firewalls with automatic relay discovery

## 🚀 Quick Start

### Installation

```bash
# Clone the repository
git clone https://github.com/Aery1e/chiral-network.git
cd chiral-network

# Install dependencies
npm install

# Run in development mode
npm run tauri:dev

# Build for production
npm run tauri:build
```

### First Steps

1. **Launch** the application
2. **Create wallet** (optional) - Secure HD wallet with mnemonic backup
3. **Configure settings** - Set storage location and network preferences
4. **Share files** - Drag & drop to Upload page, copy hash to share
5. **Download files** - Enter file hash in Download page

## 📚 Documentation

### Getting Started
- **[User Guide](docs/user-guide.md)** - Complete guide for end users
- **[Developer Setup](docs/developer-setup.md)** - Development environment setup
- **[System Overview](docs/system-overview.md)** - Introduction and core concepts

### Features & Guides
- **[File Sharing](docs/file-sharing.md)** - Upload/download workflows and encryption
- **[NAT Traversal](docs/nat-traversal.md)** - Network connectivity and relay configuration
- **[Reputation System](docs/reputation.md)** - Peer reputation and trust levels
- **[Wallet & Blockchain](docs/wallet-blockchain.md)** - HD wallets and mining
- **[Internationalization](docs/i18n.md)** - Multi-language support

### Technical Documentation
- **[Architecture](docs/architecture.md)** - System design and components
- **[Technical Specifications](docs/technical-specifications.md)** - Detailed technical specs
- **[Network Protocol](docs/network-protocol.md)** - P2P networking and DHT
- **[API Documentation](docs/api-documentation.md)** - Service APIs and integration
- **[Security & Privacy](docs/security-privacy.md)** - Security features and threat model

### Development
- **[Implementation Guide](docs/implementation-guide.md)** - Development workflows
- **[Contributing Guide](docs/contributing.md)** - How to contribute
- **[Deployment Guide](docs/deployment-guide.md)** - Production deployment

📖 **[Full Documentation Index](docs/index.md)**

## 🛠️ Technology Stack

### Frontend
- **Svelte 5** - Modern reactive UI framework
- **TypeScript** - Type-safe development
- **Tailwind CSS** - Utility-first styling
- **Tauri 2** - Rust-based desktop runtime

### P2P Network
- **libp2p v0.54** - Full P2P networking stack (Rust)
- **Kademlia DHT** - Distributed hash table
- **WebRTC** - Peer-to-peer data channels
- **Circuit Relay v2** - NAT traversal
- **AutoNAT v2** - Reachability detection

### Blockchain & Security
- **Geth** - Ethereum node integration
- **HD Wallets** - BIP32/BIP39 implementation
- **AES-256-GCM** - File encryption
- **HMAC** - Stream authentication

## 🎯 Use Cases

### Personal
- Backup & sync across devices
- Share photos/videos with family
- Cross-device file access

### Educational
- Share research datasets
- Distribute course materials
- Collaborative projects

### Organizational
- Internal file distribution
- Distributed backup solution
- Branch office file sharing

## 🔒 Security & Privacy

- **End-to-end encryption** for sensitive files
- **Anonymous routing** via SOCKS5/Circuit Relay
- **No centralized tracking** - fully decentralized
- **HD wallet security** with mnemonic backup
- **Peer reputation** system for trust

See [Security & Privacy](docs/security-privacy.md) for details.

## 🤝 Contributing

We welcome contributions! Please read our [Contributing Guide](docs/contributing.md) for:
- Code of conduct
- Development workflow
- Code style guidelines
- Pull request process

**What we're looking for:**
- Bug fixes and improvements
- Documentation enhancements
- Translation contributions
- Test coverage

**What we don't accept:**
- Centralized market features
- Commercial tracking systems
- Features enabling piracy

## 📋 Roadmap

### Completed ✅
- Full libp2p P2P networking
- DHT-based file discovery
- WebRTC file transfers
- NAT traversal (AutoNAT v2, Circuit Relay v2)
- Multi-source parallel downloads
- Reputation system
- HD wallet implementation
- CPU mining with Geth
- Multi-language support (5 languages)

### In Progress 🚧
- Advanced relay discovery
- Enhanced file versioning UI
- Performance optimizations

### Planned 📅
- Mobile app version
- Hardware wallet support
- IPFS compatibility layer
- Advanced analytics dashboard

See [Project Roadmap](https://github.com/Aery1e/chiral-network/projects) for details.

## 🐛 Troubleshooting

### Common Issues

**Can't connect to network?**
- Check firewall settings
- Verify DHT status in Network page
- Try restarting the application

**Files not downloading?**
- Verify file hash is correct
- Check if seeders are online
- Review bandwidth limits in Settings

**Mining not starting?**
- Ensure Geth is initialized
- Check system resources
- Verify mining address is set

See [User Guide - Troubleshooting](docs/user-guide.md#troubleshooting) for more.

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

Special thanks to all contributors and design teams whose concepts shaped this implementation:
- Focus on legitimate P2P use cases
- BitTorrent-inspired continuous seeding model
- Privacy-first architecture
- Open source community

## 📞 Support

- **Documentation**: [docs/](docs/index.md)
- **Issues**: [GitHub Issues](https://github.com/Aery1e/chiral-network/issues)
- **Discussions**: [GitHub Discussions](https://github.com/Aery1e/chiral-network/discussions)
- **Community**: Join us on Zulip

## ⚠️ Disclaimer

Chiral Network is designed for legitimate file storage and sharing. Users are responsible for ensuring they have the rights to share any content they upload. The platform uses a fully decentralized architecture to ensure true peer-to-peer operation and prevent commercial misuse or piracy.

---

**Built with ❤️ for a decentralized, privacy-focused future**

[Documentation](docs/index.md) • [Contributing](docs/contributing.md) • [License](LICENSE) • [GitHub](https://github.com/Aery1e/chiral-network)
