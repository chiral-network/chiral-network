# Chiral Network

> **Decentralized P2P File Sharing Platform**

Chiral Network is a BitTorrent-like peer-to-peer file storage and sharing system that combines blockchain technology with DHT-based distributed storage. Built with privacy, security, and legitimate use in mind.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Build Status](https://github.com/chiral-network/chiral-network/workflows/test/badge.svg)](https://github.com/chiral-network/chiral-network/actions)

## [NEW] Features

- [NET] **Fully Decentralized** - No central servers, DHT-based discovery
- [FAST] **Multi-Source Downloads** - Parallel downloads from multiple peers
- [TARGET] **Reputation System** - Intelligent peer selection based on reliability

## [START] Quick Start

### Installation

```bash
# Clone the repository
git clone https://github.com/chiral-network/chiral-network.git
cd chiral-network

# Install dependencies
npm install

# Run in development mode
npm run tauri:dev

# Build for production
npm run tauri:build
```

## [DOCS] Documentation

### Getting Started

- **[User Guide](docs/user-guide.md)** - Complete guide for end users
- **[Developer Setup](docs/developer-setup.md)** - Development environment setup
- **[System Overview](docs/system-overview.md)** - Introduction and core concepts

### Development

- **[Implementation Guide](docs/implementation-guide.md)** - Development workflows
- **[Contributing Guide](docs/contributing.md)** - How to contribute
- **[Deployment Guide](docs/deployment-guide.md)** - Production deployment

### More

[GUIDE] **[Full Documentation Index](docs/index.md)**

## [HANDSHAKE] Contributing

We welcome contributions! Please read our [Contributing Guide](docs/contributing.md) for:

- Code of conduct
- Development workflow
- Code style guidelines
- Pull request process

See our [Roadmap](docs/roadmap.md) for planned features and development phases.

## [BUG] Troubleshooting

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

## [FILE] License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## [SUPPORT] Support

- **Documentation**: [docs/](docs/index.md)
- **Issues**: [GitHub Issues](https://github.com/chiral-network/chiral-network/issues)
- **Discussions**: [Zulip](https://brooknet.zulipchat.com/join/f3jj4k2okvlfpu5vykz5kkk5/)

## [WARN] Disclaimer

Chiral Network is designed for legitimate file storage and sharing. Users are responsible for ensuring they have the rights to share any content they upload. The platform uses a fully decentralized architecture to ensure true peer-to-peer operation.

---

**Built with  for a decentralized, privacy-focused future**

[Documentation](docs/index.md) • [Contributing](docs/contributing.md) • [License](LICENSE) • [GitHub](https://github.com/chiral-network/chiral-network)
