# Chiral Network - High-Level Architecture Overview

**For Project Managers and Non-Technical Stakeholders**

---

## What is Chiral Network?

Chiral Network is a decentralized peer-to-peer file sharing application. Think of it as BitTorrent combined with a payment system - users share files directly with each other without central servers, and seeders (people who share files) can earn cryptocurrency from downloaders.

---

## How It Works (Simple Explanation)

1. **Alice wants to share a file**: She adds the file to Chiral Network. The file is split into small pieces and advertised to the network.

2. **Bob wants to download the file**: He searches for the file by its unique identifier (hash). The network tells him which peers have the file.

3. **Bob downloads from Alice**: He connects directly to Alice (and potentially other seeders) and downloads the file pieces. Multiple pieces can be downloaded simultaneously from different sources.

4. **Bob pays Alice**: After the download completes, Bob's wallet automatically sends cryptocurrency to Alice for the data transferred.

---

## Key Components

### 1. DHT (Distributed Hash Table)

**What it is**: A decentralized lookup system that tracks who has what files.

**How it works**:
- When Alice shares a file, she announces it to the DHT
- When Bob searches for a file, he queries the DHT
- No central server needed - all peers collectively maintain the lookup table
- Uses the Kademlia protocol (same as BitTorrent)

**Why it matters**: Eliminates single points of failure and censorship.

---

### 2. Bootstrap Nodes

**What they are**: Well-known entry points into the network.

**How they work**:
- When a new user starts Chiral Network, they connect to bootstrap nodes first
- Bootstrap nodes introduce the new user to other peers
- After initial connection, the user no longer depends on bootstrap nodes

**Why they matter**: New users need a starting point to join the network.

---

### 3. File Transfer Protocols

Chiral Network supports multiple ways to transfer files:

| Protocol | Best For | How It Works |
|----------|----------|--------------|
| **HTTP** | Users with public IP addresses | Standard web protocol, very compatible |
| **WebTorrent** | Users behind firewalls/NAT | Uses WebRTC for NAT traversal |
| **BitTorrent** | Large files with many seeders | Efficient swarming from multiple peers |
| **ed2k** | Legacy files on eDonkey network | Multi-source downloads |

The system automatically selects the best protocol based on network conditions.

---

### 4. NAT Traversal

**The problem**: Most home users are behind routers (NAT) and cannot accept incoming connections.

**The solutions**:

| Technology | What It Does |
|------------|--------------|
| **AutoNAT** | Detects if you can be reached from the internet |
| **UPnP** | Automatically opens ports on your router |
| **Circuit Relay** | Routes traffic through public nodes when direct connection fails |
| **Hole Punching** | Establishes direct connections between NAT'd users |

---

### 5. Blockchain / Payments

**Network**: Ethereum-compatible private blockchain

**How payments work**:
1. Seeders set a price per megabyte
2. Downloaders pay after receiving verified data
3. Payments happen on the blockchain layer, separate from file transfers
4. Same payment logic regardless of which protocol transferred the data

**Mining**: Users can run mining software to earn cryptocurrency by validating transactions.

---

### 6. Wallet System

**HD Wallets**: Hierarchical Deterministic wallets using industry-standard BIP32/BIP39

**Features**:
- Generate wallets from a 12/24 word recovery phrase
- Create multiple accounts from one seed
- Secure key storage
- Transaction signing

---

### 7. Reputation System

**Purpose**: Help users identify trustworthy peers.

**How it works**:
- Peers earn reputation by successfully completing transfers
- Higher reputation = more likely to be selected for downloads
- Bad actors (slow speeds, corrupt data) lose reputation
- Reputation is tracked across the network

**Trust Levels**: Unknown, Low, Medium, High, Trusted

---

## Architecture Diagram

```
                                    ┌──────────────────┐
                                    │   User Interface │
                                    │   (Desktop App)  │
                                    └────────┬─────────┘
                                             │
                    ┌────────────────────────┴────────────────────────┐
                    │                                                  │
           ┌────────[v]────────┐                              ┌─────────[v]─────────┐
           │  File Transfer  │                              │     Payments      │
           │     Layer       │                              │      Layer        │
           │                 │                              │                   │
           │  - HTTP         │                              │  - Wallet         │
           │  - WebTorrent   │[<]────── Decoupled ──────────[>]│  - Blockchain     │
           │  - BitTorrent   │                              │  - Mining         │
           │  - ed2k         │                              │                   │
           └────────┬────────┘                              └───────────────────┘
                    │
           ┌────────[v]────────┐
           │   P2P Network   │
           │                 │
           │  - DHT          │
           │  - Bootstrap    │
           │  - NAT Traversal│
           │  - Relay        │
           └─────────────────┘
```

---

## Application Pages

| Page | Purpose |
|------|---------|
| **Download** | Search for and download files |
| **Upload** | Share files to the network (instant seeding) |
| **Network** | View connected peers, DHT status |
| **Relay** | Configure relay server mode |
| **Mining** | CPU mining controls and statistics |
| **Proxy** | SOCKS5 proxy configuration |
| **Analytics** | Bandwidth and usage statistics |
| **Reputation** | Peer trust scores and leaderboards |
| **Account** | Wallet management |
| **Settings** | Application configuration |

---

## Data Flow Summary

### Uploading (Seeding) a File

```
1. User selects file
2. File is chunked into 256KB pieces
3. SHA-256 hash generated (file identifier)
4. Optionally encrypted
5. Metadata published to DHT
6. File available for download
7. User earns payments when others download
```

### Downloading a File

```
1. User enters file hash
2. DHT queried for seeders
3. Best seeders selected (reputation, speed, protocol)
4. Chunks downloaded (possibly from multiple sources)
5. Chunks verified via hash
6. File assembled
7. Payment sent to seeders
```

---

## Security Features

| Feature | Purpose |
|---------|---------|
| **AES-256 Encryption** | Optional file encryption |
| **SHA-256 Hashing** | Data integrity verification |
| **Noise Protocol** | Encrypted network communications |
| **Signed Transactions** | Cryptographic payment verification |
| **Merkle Trees** | Efficient chunk verification |

---

## What Makes Chiral Network Different

1. **Decoupled Architecture**: Payments are separate from file transfers, allowing multiple protocols

2. **No Central Servers**: Fully peer-to-peer with DHT discovery

3. **Economic Incentives**: Seeders earn cryptocurrency, solving BitTorrent's "leech" problem

4. **Multi-Protocol**: Supports HTTP, WebTorrent, BitTorrent, and ed2k

5. **Privacy Features**: Relay support, proxy integration, optional encryption

6. **Reputation System**: Trust-based peer selection

---

## Technology Summary

| Component | Technology |
|-----------|------------|
| Frontend | Svelte 5 + TypeScript |
| Desktop Runtime | Tauri 2 (Rust) |
| P2P Network | libp2p v0.54 |
| DHT | Kademlia |
| Blockchain | Geth (Ethereum-compatible) |
| Mining | Proof-of-Work (Ethash) |
| Encryption | AES-256-GCM |
| Wallets | HD Wallets (BIP32/BIP39) |

---

## Glossary

| Term | Definition |
|------|------------|
| **Seeder** | A peer sharing a file |
| **Leecher** | A peer downloading a file |
| **DHT** | Distributed Hash Table - decentralized lookup system |
| **Bootstrap** | Initial nodes for joining the network |
| **NAT** | Network Address Translation - router firewall |
| **Relay** | A public node that forwards traffic for NAT'd users |
| **Chunk** | A 256KB piece of a file |
| **Hash** | Unique identifier derived from file content |
| **Merkle Root** | Root hash of a tree of chunk hashes |
| **CID** | Content Identifier - file's unique hash |

---

## Project Status

| Phase | Status |
|-------|--------|
| Phase 1: Core Infrastructure | Completed |
| Phase 2: P2P Network | Completed |
| Phase 3: File Sharing & Protocols | In Progress |
| Phase 4: Advanced Features | Planned |

---

*Last Updated: January 2026*
