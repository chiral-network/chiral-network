# ADR-001: Blockchain-Based Peer Reputation System

## Status: Accepted

## Context

The Chiral Network requires a tamper-proof, globally verifiable peer reputation system to ensure network security and enable intelligent peer selection. The existing DHT-based infrastructure provides local reputation metrics, but lacks the immutability and global consensus needed for secure reputation management.

## Decision

Implement a hybrid blockchain + DHT reputation system with blockchain as the primary source of truth for reputation data.

### Architecture Overview

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│   Local DHT     │    │  Blockchain      │    │   Frontend      │
│   Metrics       │◄──►│  Reputation      │◄──►│   Display       │
│                 │    │  System          │    │                 │
└─────────────────┘    └──────────────────┘    └─────────────────┘
         │                       │                       │
         ▼                       ▼                       ▼
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│ Real-time       │    │ Tamper-proof     │    │ User Interface  │
│ Performance     │    │ Global Consensus │    │ & Analytics     │
│ Metrics         │    │ Verification     │    │                 │
└─────────────────┘    └──────────────────┘    └─────────────────┘
```

### Key Components

1. **Reputation Events**: File transfers, connections, payments
2. **Reputation Epochs**: Batched events with Merkle tree proofs
3. **Blockchain Integration**: Ethereum-compatible transactions
4. **Hybrid Scoring**: Combines local metrics with blockchain reputation
5. **Secure Key Management**: Encrypted private key storage with zeroization

## Rationale

### Why Blockchain over DHT?

**Advantages of Blockchain:**
- **Immutable Records**: Cryptographic immutability prevents reputation manipulation
- **Global Consensus**: Single source of truth across the network
- **Economic Security**: Reputation tied to economic stakes prevents Sybil attacks
- **Cross-Network Portability**: Reputation can be portable across different networks
- **Verification**: Cryptographic proofs enable reputation verification

**Limitations of DHT-only Approach:**
- **Mutable Data**: DHT entries can be modified or deleted by peers
- **Local Views**: Each peer has its own view of reputation data
- **Manipulation Vulnerable**: No protection against reputation manipulation attacks
- **No Economic Stakes**: Easy to create fake identities (Sybil attacks)

### Why Hybrid Approach?

The implementation uses a hybrid approach that combines the benefits of both systems:

```rust
// Hybrid scoring algorithm
let local_score = metrics.get_quality_score(false) * 1000.0;
let blockchain_score = get_blockchain_reputation_score(peer_id).await?;
let reputation_weight = 0.7; // 70% blockchain, 30% local
let final_score = (blockchain_score * 1000.0 * reputation_weight) + 
                  (local_score * (1.0 - reputation_weight));
```

**Benefits:**
- **Fallback Capability**: Works when blockchain is unavailable
- **Real-time Performance**: Local metrics for immediate decisions
- **Long-term Security**: Blockchain reputation for persistent trust
- **Configurable Balance**: Adjustable weight between local and blockchain reputation

## Consequences

### Positive

- **Tamper-Proof Reputation**: Cryptographic immutability prevents manipulation
- **Global Consensus**: Single source of truth for reputation across the network
- **Economic Security**: Reputation tied to economic stakes prevents Sybil attacks
- **Verification**: Cryptographic proofs enable reputation verification
- **Scalability**: Blockchain reputation scales with network size
- **Integration**: Can integrate with other blockchain-based features (payments, governance)

### Negative

- **Added Complexity**: Additional infrastructure requirement (Geth dependency)
- **Higher Resource Usage**: Blockchain operations require more computational resources
- **Network Dependency**: Requires blockchain network availability
- **Latency**: Blockchain operations may introduce latency compared to local DHT operations

### Mitigation Strategies

1. **Fallback Mode**: DHT-only reputation when blockchain unavailable
2. **Caching**: Blockchain reputation caching to reduce network calls
3. **Async Operations**: Non-blocking blockchain operations to prevent UI blocking
4. **Configurable Weights**: Adjustable balance between local and blockchain reputation
5. **Secure Key Management**: Encrypted private key storage with automatic zeroization

## Implementation Details

### Security Considerations

- **Private Key Encryption**: AES-256-GCM encryption with PBKDF2 key derivation
- **Memory Zeroization**: Automatic clearing of sensitive data from memory
- **Secure Storage**: Encrypted private key storage with salt-based encryption
- **Error Handling**: Comprehensive error handling for blockchain connectivity issues

### Performance Optimizations

- **Reputation Caching**: 5-minute cache for blockchain reputation scores
- **Async Operations**: Non-blocking blockchain reputation fetching
- **Batch Processing**: Epoch-based reputation event batching
- **Fallback Mechanisms**: Graceful degradation when blockchain unavailable

### Integration Points

- **DHT Service**: Integrated with existing libp2p DHT infrastructure
- **Peer Selection**: Enhanced existing peer selection with blockchain reputation
- **Frontend**: Seamless integration with existing reputation UI
- **Blockchain**: Compatible with Ethereum-compatible networks (Chiral Network ID: 98765)

## Alternatives Considered

### 1. DHT-Only Reputation
- **Pros**: Simple, fast, no external dependencies
- **Cons**: Mutable, local views, vulnerable to manipulation
- **Decision**: Rejected due to security vulnerabilities

### 2. Centralized Reputation Server
- **Pros**: Simple implementation, fast queries
- **Cons**: Single point of failure, centralization violates project principles
- **Decision**: Rejected due to centralization concerns

### 3. Pure Blockchain Reputation
- **Pros**: Maximum security and immutability
- **Cons**: High latency, resource intensive, no fallback
- **Decision**: Rejected in favor of hybrid approach

## Monitoring and Metrics

- **Blockchain Connectivity**: Monitor Geth node status and connectivity
- **Reputation Cache Performance**: Track cache hit rates and performance
- **Fallback Usage**: Monitor when DHT-only mode is used
- **Security Events**: Track private key usage and security-related events

## Future Considerations

- **Smart Contract Integration**: Deploy dedicated reputation smart contracts
- **Reputation Rewards**: Integrate reputation with mining rewards
- **Cross-Network Portability**: Enable reputation portability across networks
- **Advanced Verification**: Implement more sophisticated reputation verification algorithms

## References

- [Ethereum Documentation](https://ethereum.org/developers/)
- [libp2p Documentation](https://docs.libp2p.io/)
- [Ring Cryptography Library](https://github.com/briansmith/ring)
- [Zeroize Memory Clearing](https://github.com/RustCrypto/utils/tree/master/zeroize)

---

**Date**: 2024-01-XX  
**Authors**: Chiral Network Team  
**Reviewers**: [To be filled]  
**Status**: Accepted
