# Chiral Network System Overview

## Executive Summary

Chiral Network is a **BitTorrent-inspired decentralized peer-to-peer file sharing system with economic incentives**. It combines modern P2P networking (HTTP, WebTorrent, BitTorrent, ed2k) with blockchain-based payments using a **decoupled architecture** where the payment mechanism is completely separate from data transfer protocols.

Files are shared through **continuous seeding** - when you add a file, it instantly becomes available to the network, but only remains available as long as at least one peer continues to seed it. There are **no permanent storage guarantees**. Seeders earn cryptocurrency for sharing files, with payments handled on the blockchain layer regardless of which protocol is used for data transfer.

All nodes are equal and can simultaneously seed files (earning cryptocurrency), download files (making payments), relay traffic for NAT-traversed peers, and participate in mining. 

## Core Components

### 1. Blockchain Layer (Payment Layer)

- **Purpose**: Handle payments to providers who seed the files
- **Decoupled Design**: Payment mechanism is completely separate from data transfer protocols
- **Protocol Agnostic**: Payments work regardless of whether files are transferred via HTTP, WebTorrent, BitTorrent, or ed2k
- **Implementation**: Ethereum-compatible network with custom genesis block and network parameters
- **Currency**: Native cryptocurrency for all network transactions
- **Consensus**: Ethash proof-of-work consensus 

### 2. File Sharing Layer (BitTorrent-like Model)

- **Continuous Seeding**: Files are instantly available when added - no "upload" step to a specific server is required
- **Content Addressing**: Files identified by SHA-256 hashes (CIDs)
- **DHT Integration**: Kademlia DHT for peer discovery and file metadata
- **No Permanent Storage**: Files only available while peers are actively seeding them
- **Equal Peers**: All nodes can seed, leech, and participate equally

### 3. Network Layer (Data Transfer Layer)

- **Multiple Protocol Support**: HTTP, WebTorrent, BitTorrent, and ed2k protocols for data transfer
- **Decoupled from Payments**: All protocols handle only data transfer - payments happen separately on blockchain
- **Protocol Independence**: Choose any protocol; payment mechanism is protocol-agnostic
- **Node Discovery**: DHT-based peer discovery mechanism (Kademlia)
- **P2P Communication**: libp2p protocol stack for node communication
- **NAT Traversal**: Support for nodes behind NATs

## System Architecture

```text
┌─────────────────────────────────────────────────────────┐
│                     Client Applications                   │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐   │
│  │   GUI   │  │   CLI   │  │   API   │  │  Wallet │   │
│  └─────────┘  └─────────┘  └─────────┘  └─────────┘   │
└─────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────┐
│                     Service Layer                        │
│  ┌──────────────┐                      ┌──────────────┐ │
│  │ File Service │                      │Wallet Service│ │
│  │ (Seed/Leech) │                      │              │ │
│  └──────────────┘                      └──────────────┘ │
└─────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────┐
│                     Network Layer                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐ │
│  │ Kademlia DHT │  │  Protocols   │  │  Blockchain  │ │
│  │ (Discovery)  │  │HTTP/WebTorr. │  │   (Mining)   │ │
│  │              │  │BitTorr./ed2k │  │              │ │
│  └──────────────┘  └──────────────┘  └──────────────┘ │
└─────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────────────────────────────────┐
│               Local Storage (Per-Node Only)              │
│  ┌──────────────────────────────────────────────────┐   │
│  │  Files you're seeding + Downloaded file cache    │   │
│  │  (No network-wide storage guarantee)             │   │
│  └──────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

## Key Design Principles

### 1. True Decentralization

- No centralized storage or servers
- All nodes are equal - everyone can seed, leech, relay, and mine
- Files only available while peers actively seed them (no permanent storage)
- Distributed consensus through blockchain (for mining only)
- Multi-protocol P2P file distribution (HTTP, WebTorrent, BitTorrent, ed2k)

### 2. Security

- Cryptographic signatures for blockchain transactions and payments
- Anonymous transactions through cryptocurrency
- Content verification through hashes (CIDs)
- Payment verification on blockchain

### 3. Transfer Efficiency

- **Multiple Protocol Support**: HTTP, WebTorrent, BitTorrent, and ed2k
- Chunked file transfers for parallel downloads from multiple peers
- Multi-source downloads (BitSwap protocol, BitTorrent swarming)
- Local caching of downloaded files
- Efficient peer discovery through Kademlia DHT
- WebTorrent support with WebRTC direct connections for browser compatibility
- Traditional BitTorrent and ed2k protocol implementations

## Node Roles (All Nodes Are Equal)

**Important**: There are no dedicated "storage nodes" or node types. Every node is equal and can simultaneously perform multiple roles. The network operates like BitTorrent - files are only available when someone is actively seeding them. There is **no permanent storage guarantee**.

### All Nodes Can:

**1. Seed Files (Upload)**
- Add files to share - they instantly become available to the network
- Files remain available only while you keep them seeded
- Publish file metadata to DHT for peer discovery
- Serve file chunks via multiple protocols (HTTP, WebTorrent, BitTorrent, ed2k)
- Support WebRTC connections through WebTorrent protocol

**2. Leech Files (Download)**
- Search for files by hash (CID)
- Discover seeders via DHT
- Download chunks from multiple peers simultaneously
- Verify chunk integrity with cryptographic hashes
- Files only available if at least one seeder is online

**3. Mine Blocks (Optional)**
- Validate blockchain transactions
- Secure the network with proof-of-work
- Earn mining rewards and transaction fees
- Requires computational resources

**4. Relay Traffic (Optional)**
- Run as Circuit Relay v2 server to help NAT'd peers
- Facilitate hole punching (DCUtR)
- Improve network connectivity
- Earn reputation points for reliability
- No commercial relay services

## Decoupled Architecture: Payments vs. Data Transfer

**Key Design Decision**: The payment mechanism is completely decoupled from the data transfer mechanism. This architectural separation provides several advantages:

### Why Decoupling Matters

1. **Protocol Flexibility**: Users can transfer files via HTTP, WebTorrent, BitTorrent, or ed2k - payment works identically for all protocols
2. **Independent Evolution**: Data transfer protocols and payment mechanisms can be upgraded independently
3. **Broader Compatibility**: Support legacy P2P protocols without modifying their core specifications
4. **Payment Certainty**: Blockchain-based payments are verifiable regardless of which protocol transferred the data
5. **Choice**: Clients choose best protocol for their network conditions while payments remain consistent

### How It Works

- **Data Transfer**: Handled by HTTP, WebTorrent, BitTorrent, or ed2k protocols
- **Payment Settlement**: Handled separately on the Ethereum-compatible blockchain
- **Verification**: File integrity verified via cryptographic hashes (CIDs)
- **Payment Triggers**: Completed transfers trigger blockchain transactions to reward seeders
- **No Protocol Lock-in**: Switch protocols mid-transfer without affecting payment logic

## Supported Transfer Protocols

Chiral Network supports multiple protocols for **data transfer only** (payments are separate):

### 1. HTTP
- Simple file retrieval over HTTP/HTTPS
- Fallback option for restricted networks
- Direct file downloads from seeders
- No special client software required

### 2. WebTorrent
- WebRTC-based P2P transfers
- Browser compatibility without plugins
- Direct peer-to-peer connections using WebRTC data channels
- Ideal for web-based clients and cross-platform sharing

### 3. BitTorrent
- Native BitTorrent protocol implementation
- Full compatibility with standard BitTorrent clients
- Efficient swarming and piece exchange
- Proven P2P technology with wide adoption

### 4. ed2k (eDonkey2000)
- ed2k network protocol support
- Hash-based file identification
- Multi-source downloading
- Compatibility with ed2k network

**Payment happens on the blockchain layer regardless of which protocol(s) were used.**

## Node Incentives (Payment-Based Model)

The network incentivizes file sharing through blockchain-based payments to seeders, completely decoupled from the data transfer mechanism.

### Payment Incentives (Blockchain Layer)

1. **Seeding Rewards**: Earn cryptocurrency by seeding files that others download
2. **Payment Decoupling**: Earn rewards regardless of which protocol (HTTP, WebTorrent, BitTorrent, ed2k) is used for transfer
3. **Protocol Agnostic**: Same payment structure across all supported protocols
4. **Blockchain Settlement**: All payments settled on Ethereum-compatible blockchain

### Mining Rewards (Blockchain Layer)

1. **Block Rewards**: New coins created through Ethash proof-of-work mining
2. **Gas Fees**: Transaction fees from all blockchain operations including payment transactions
3. **Mining Participation**: Anyone can mine to earn cryptocurrency and secure the network

### Why Nodes Participate

1. **Earn Cryptocurrency**: Get paid for seeding files others download
2. **Protocol Freedom**: Earn rewards regardless of which transfer protocol is used
3. **Mining Rewards**: Optional - earn additional cryptocurrency through mining
4. **Access Network**: Download files from other seeders

## Comparison with Existing Systems

### vs. BitTorrent

- **Core Model**: Very similar - continuous seeding, no permanent storage
- **Similarities**: P2P file sharing, chunked distribution, DHT-based discovery
- **Native BitTorrent Support**: Full compatibility with BitTorrent protocol
- **Key Differences**:
  - **Payment Incentives**: Seeders earn cryptocurrency (BitTorrent relies on altruism)

### vs. IPFS

- **Similarities**: Content addressing (CID/hash), Kademlia DHT, P2P distribution
- **Key Differences**:
  - **No Pinning Services**: Files disappear when all seeders go offline (like BitTorrent)
  - **No Permanent Storage**: IPFS encourages pinning; we're explicit about non-permanence
  - **Payment for Seeding**: We pay seeders; IPFS requires separate incentive layer (Filecoin)
  - **Separate Blockchain**: Our own chain for payments and mining
  - **Protocol Flexibility**: Multiple protocols (HTTP, WebTorrent, BitTorrent, ed2k) with unified payments

### vs. Filecoin

- **Similarities**: Both pay for storage/seeding using blockchain
- **Fundamental Differences**:
  - **Storage Model**: Filecoin provides **paid, guaranteed storage** with proof-of-storage contracts. We provide **paid seeding** with no storage guarantees (BitTorrent-style)
  - **No Storage Deals**: We don't have contracts or storage commitments - files available only while seeders choose to seed
  - **Protocol Decoupling**: Our payments work with any protocol (HTTP, WebTorrent, BitTorrent, ed2k); Filecoin tied to IPFS
  - **Different Use Case**: Filecoin is for persistent archival; we're for active file sharing with economic incentives

### vs. Traditional Cloud Storage

- **Advantages**:
  - Decentralized and censorship-resistant
  - No monthly subscription fees
  - Full control over your data
  - Privacy-focused with encryption
- **Disadvantages**:
  - No storage guarantee - files gone when seeders leave
  - Requires technical setup
  - Need to keep app running to seed
  - File availability depends on peer participation

## Development Roadmap

### Phase 1: Foundation (Current)

- Basic blockchain implementation
- Simple file storage and retrieval
- Decentralized peer discovery via DHT
- Desktop GUI application

### Phase 2: Enhancement

- Improved P2P networking
- Advanced encryption options
- Reputation system

### Phase 3: Optimization

- Performance improvements
- Advanced caching strategies
- Cross-chain compatibility
- Enterprise features

### Phase 4: Scale

- Global network deployment
- Third-party integrations
- Developer ecosystem
- Governance mechanisms

## Success Metrics

### Technical Metrics

- Network uptime > 99.9%
- Average file retrieval time < 5 seconds
- Transaction confirmation time < 1 minute

### User Metrics

- Active node count > 10,000
- Active seeding sessions > 50,000
- Average seeders per popular file > 10
- Daily active users > 1,000
- File retrieval success rate > 95% (when seeders available)

## Challenges and Solutions (for future)

### Challenge: Network Bootstrapping

**Solution**: Initial seed nodes, incentivized early adoption

### Challenge: Scalability

**Solution**: Sharding, layer-2 solutions, efficient routing

### Challenge: User Experience

**Solution**: Intuitive GUI, automated processes, good defaults

### Challenge: File Availability

**Solution**: Pay seeders with cryptocurrency to incentivize availability, reputation system for reliable seeders, show seeder counts, support multi-source downloads across multiple protocols

### Challenge: Preventing Misuse

**Solution**: Focus on legitimate use cases, content addressing (no global search), privacy features for legal sharing, transparent blockchain transactions, reputation system to discourage bad actors

## Conclusion

Chiral Network is a **BitTorrent-inspired P2P file sharing system with economic incentives** that combines the best aspects of modern P2P technology with blockchain-based payments. Unlike storage networks like Filecoin or IPFS with pinning services, we provide **paid, temporary seeding** where files are only available when peers actively seed them (BitTorrent model) but seeders are compensated with cryptocurrency.

**Key Takeaways:**

- **Payment-Incentivized Seeding**: Seeders earn cryptocurrency for sharing files
- **Decoupled Architecture**: Payments are separate from data transfer - works with any protocol
- **Multiple Protocols**: Support for HTTP, WebTorrent (WebRTC), BitTorrent, and ed2k protocols
- **Protocol-Agnostic Payments**: Same payment mechanism regardless of which protocol is used
- **No Permanent Storage**: Files available only while seeders are online (BitTorrent-style)
- **All Nodes Are Equal**: Everyone can seed, leech, relay, and mine simultaneously
- **Blockchain Integration**: Ethereum-compatible chain for payments and optional mining
