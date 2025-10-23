# Blockchain-Based Peer Reputation System

## Overview

The Chiral Network implements a hybrid blockchain + DHT reputation system that provides tamper-proof, globally verifiable peer reputation while maintaining real-time performance through local metrics.

## Architecture

### Core Components

1. **Reputation Events**: Individual actions that affect peer reputation
2. **Reputation Epochs**: Batched events with cryptographic proofs
3. **Blockchain Integration**: Ethereum-compatible transaction submission
4. **Hybrid Scoring**: Combines local DHT metrics with blockchain reputation
5. **Secure Key Management**: Encrypted private key storage with zeroization

### Data Flow

```
Peer Action → Reputation Event → Local Metrics → Epoch Batching → Blockchain Submission → Global Verification
     ↓              ↓                ↓              ↓                    ↓                      ↓
  Real-time      Immediate        DHT Storage    Merkle Tree         Ethereum            Cross-Network
  Response       Processing       & Caching      Proofs              Transactions        Verification
```

## Security Features

### Private Key Management

- **AES-256-GCM Encryption**: Private keys encrypted with industry-standard encryption
- **PBKDF2 Key Derivation**: 100,000 iterations for key derivation
- **Memory Zeroization**: Automatic clearing of sensitive data from memory
- **Salt-based Encryption**: Unique salt for each private key encryption

### Cryptographic Verification

- **Merkle Tree Proofs**: Cryptographic proofs for reputation event integrity
- **ED25519 Signatures**: Digital signatures for reputation event authentication
- **SHA-256 Hashing**: Cryptographic hashing for data integrity
- **Blockchain Immutability**: Tamper-proof reputation records

## Implementation Details

### Reputation Events

```rust
pub struct ReputationEvent {
    pub id: String,
    pub peer_id: String,
    pub rater_peer_id: String,
    pub event_type: EventType,
    pub timestamp: u64,
    pub data: serde_json::Value,
    pub impact: f64,
    pub signature: String,
    pub epoch: Option<u64>,
}
```

**Event Types:**
- `FileTransferSuccess`: Successful file transfer
- `FileTransferFailure`: Failed file transfer
- `ConnectionEstablished`: Peer connection established
- `ConnectionLost`: Peer connection lost
- `PaymentSuccess`: Successful payment
- `PaymentFailure`: Failed payment

### Reputation Epochs

```rust
pub struct ReputationEpoch {
    pub epoch_id: u64,
    pub merkle_root: String,
    pub timestamp: u64,
    pub block_number: Option<u64>,
    pub event_count: usize,
    pub submitter: String,
}
```

**Epoch Management:**
- **Automatic Finalization**: Epochs finalized based on time or event count
- **Merkle Tree Proofs**: Cryptographic proofs for epoch integrity
- **Blockchain Submission**: Epochs submitted as Ethereum transactions
- **Verification**: Blockchain-based epoch verification

### Hybrid Scoring Algorithm

```rust
// Combine blockchain reputation with local metrics
let local_score = metrics.get_quality_score(false) * 1000.0;
let blockchain_score = get_blockchain_reputation_score(peer_id).await?;
let reputation_weight = 0.7; // 70% blockchain, 30% local
let final_score = (blockchain_score * 1000.0 * reputation_weight) + 
                  (local_score * (1.0 - reputation_weight));
```

**Scoring Factors:**
- **Blockchain Reputation**: 70% weight - tamper-proof, global consensus
- **Local Metrics**: 30% weight - real-time performance data
- **Configurable Weights**: Adjustable balance between local and blockchain reputation

## Usage

### Setting Up Reputation System

```rust
// Create reputation system
let mut reputation_system = ReputationSystem::new(98765); // Chiral Network chain ID

// Set DHT service
reputation_system.set_dht_service(dht_service);

// Set contract address (if using smart contracts)
reputation_system.set_contract_address("0x...".to_string());
```

### Adding Reputation Events

```rust
// Create reputation event
let event = ReputationEvent::new(
    "event_123".to_string(),
    "peer_456".to_string(),
    "rater_789".to_string(),
    EventType::FileTransferSuccess,
    serde_json::json!({"bytes": 1024, "duration_ms": 100}),
    0.8,
);

// Add event to system
reputation_system.add_reputation_event(event).await?;
```

### Setting Private Key

```rust
// Set private key securely
peer_selection_service.set_active_private_key(Some("0x...".to_string()))?;
```

### Getting Reputation Scores

```rust
// Get blockchain reputation score
let score = reputation_system.get_peer_reputation_score("peer_456", None).await?;

// Verify reputation consistency
let is_consistent = reputation_system.verify_peer_reputation_consistency("peer_456", &events).await?;
```

## Configuration

### Environment Variables

```bash
# Blockchain configuration
CHIRAL_NETWORK_CHAIN_ID=98765
GETH_RPC_URL=http://127.0.0.1:8545

# Reputation system configuration
REPUTATION_EPOCH_DURATION=3600  # 1 hour
REPUTATION_MAX_EVENTS_PER_EPOCH=100
REPUTATION_CACHE_DURATION=300   # 5 minutes
```

### Settings

- **Epoch Duration**: How long to collect events before finalizing
- **Max Events per Epoch**: Maximum number of events per epoch
- **Cache Duration**: How long to cache blockchain reputation scores
- **Reputation Weight**: Balance between local and blockchain reputation

## Performance Considerations

### Caching

- **Blockchain Reputation Cache**: 5-minute cache for blockchain reputation scores
- **Local Metrics Cache**: Real-time local metrics with immediate updates
- **Epoch Batching**: Efficient batch processing of reputation events

### Async Operations

- **Non-blocking Blockchain Calls**: Async blockchain operations prevent UI blocking
- **Background Processing**: Reputation events processed in background tasks
- **Fallback Mechanisms**: Graceful degradation when blockchain unavailable

### Resource Usage

- **Memory Management**: Automatic cleanup of expired cache entries
- **CPU Usage**: Optimized cryptographic operations
- **Network Usage**: Efficient blockchain transaction submission

## Security Best Practices

### Private Key Security

1. **Never store private keys in plain text**
2. **Use encrypted storage with zeroization**
3. **Implement secure key derivation**
4. **Clear sensitive data from memory**

### Reputation Security

1. **Verify all reputation events cryptographically**
2. **Use Merkle tree proofs for epoch integrity**
3. **Implement fallback mechanisms for blockchain failures**
4. **Monitor for reputation manipulation attempts**

### Network Security

1. **Use secure communication channels**
2. **Implement rate limiting for reputation submissions**
3. **Monitor for Sybil attacks**
4. **Validate all incoming reputation data**

## Troubleshooting

### Common Issues

**Blockchain Connection Failed**
- Check Geth node status
- Verify network connectivity
- Check chain ID configuration

**Private Key Decryption Failed**
- Verify private key format
- Check encryption/decryption implementation
- Ensure proper key derivation

**Reputation Score Inconsistent**
- Check blockchain synchronization
- Verify Merkle tree proofs
- Validate event signatures

### Debug Mode

Enable debug logging to troubleshoot issues:

```rust
// Enable debug logging
tracing::subscriber::set_global_default(
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init()
)?;
```

## Future Enhancements

### Planned Features

1. **Smart Contract Integration**: Deploy dedicated reputation smart contracts
2. **Reputation Rewards**: Integrate reputation with mining rewards
3. **Cross-Network Portability**: Enable reputation portability across networks
4. **Advanced Verification**: Implement more sophisticated reputation verification algorithms

### Research Areas

1. **Reputation Decay**: Implement time-based reputation decay
2. **Reputation Aggregation**: Advanced algorithms for reputation aggregation
3. **Reputation Prediction**: Machine learning for reputation prediction
4. **Reputation Governance**: Decentralized reputation governance mechanisms

## Contributing

We welcome contributions to the reputation system:

- **Security Improvements**: Enhanced cryptographic implementations
- **Performance Optimizations**: Better caching and async operations
- **Feature Additions**: New reputation event types and scoring algorithms
- **Documentation**: Improved documentation and examples

## License

MIT License - See LICENSE file for details

---

**Last Updated**: 2024-01-XX  
**Version**: 1.0.0  
**Maintainers**: Chiral Network Team
