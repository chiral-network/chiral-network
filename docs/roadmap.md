# Project Roadmap

This roadmap outlines the development milestones for Chiral Network, from initial concept to future enhancements.

## Vision

Build a fully decentralized peer-to-peer file sharing platform that prioritizes legitimate use cases, secure file transfers, and network resilience. We focus on file sharing excellence, not general-purpose anonymity or VPN services.

## Development Phases

### Phase 1: Core Infrastructure ✅ COMPLETED

**Goal**: Establish basic UI and application structure

**Completed Features**:
- ✅ Modern desktop interface (Svelte 5 + Tauri 2)
- ✅ Real-time file management dashboard
- ✅ Network monitoring & peer discovery
- ✅ Analytics dashboard with real metrics
- ✅ Comprehensive settings management

**Timeline**: W1-W4
---

### Phase 2: P2P Network Infrastructure ✅ COMPLETED

**Goal**: Implement production-ready P2P networking

**Completed Features**:
- ✅ Full libp2p v0.54 integration
- ✅ Kademlia DHT integration
- ✅ CPU mining with Geth integration

**Timeline**: W5-W8

---

### Phase 3: Core File Sharing Features 🚧 IN PROGRESS

**Goal**: Complete essential file sharing functionality

**In Progress**:
- Chunk transfer protocol
- Support for multiple protocols, such as http, ftp, bittorrent, ed2k, etc.
- Reputation system with trust levels
- GUI integration

**Timeline**: W9-W12

---

### Phase 4: Advanced Features 📅 PLANNED

**Goal**: Add sophisticated functionality

**Planned Features**:

- Proxy?
- A CDN service?
- Chrome extension?

**Timeline**: W13-W16

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

### What We Won't Add For Now (Before core features are working and stable)

**Commercial & Piracy-Enabling Features**:
- Centralized market servers
- Global file search/discovery (piracy risk)
- Price/payment systems
- Commercial tracking
- Social features (likes, comments, reviews)
- Advertising systems
- Marketplace or trading platforms

**The Proxy Feature**:
- Proxy service provider for peers to support for web traffic
- Traffic mixing/onion routing

**Privacy in File Sharing**
- File encryption (protect file content)
- Anonymous mode (hide IP during P2P file transfers only)

## Contributing to the Roadmap

Want to help shape the future of Chiral Network?

1. **Read** [Contributing Guide](contributing.md)
2. **Join** discussions on Zulip
3. **Submit** feature requests with detailed proposals
4. **Vote** on features that matter to you
5. **Contribute** code for planned features

## Roadmap Updates

**Last Updated**: October 2024
---

[Back to Documentation Index](index.md)
