# Chiral Network Security & Privacy Documentation

## Security Overview

The Chiral Network implements multiple layers of security to protect data integrity, user privacy, and network resilience. This document outlines security measures, threat models, and best practices for maintaining a secure distributed file storage network.

## Cryptographic Foundations

### Hash Functions

| Algorithm  | Purpose                           | Security Level |
| ---------- | --------------------------------- | -------------- |
| SHA-256    | File hashing, chunk verification  | 256-bit        |
| SHA-3      | Alternative hashing (future)      | 256-bit        |
| BLAKE2b    | Fast hashing for non-critical ops | 256-bit        |
| Keccak-256 | Ethereum compatibility            | 256-bit        |

### Encryption Algorithms

| Algorithm         | Purpose                   | Key Size |
| ----------------- | ------------------------- | -------- |
| AES-256-GCM       | File encryption           | 256-bit  |
| ChaCha20-Poly1305 | Stream cipher alternative | 256-bit  |
| RSA-4096          | Key exchange (legacy)     | 4096-bit |
| Ed25519           | Digital signatures        | 256-bit  |
| X25519            | Key agreement             | 256-bit  |

### Key Derivation

```
Master Seed (BIP39 Mnemonic)
    │
    ├── PBKDF2 (100,000 iterations)
    │
    └── HD Wallet (BIP32/BIP44)
        │
        ├── m/44'/9001'/0'/0/* (Wallet Keys)
        ├── m/44'/9001'/1'/0/* (File Encryption Keys)
        └── m/44'/9001'/2'/0/* (Identity Keys)
```

## File Security

### Encryption and Chunking Process

The network ensures both confidentiality and availability using encryption and replication.

```
1. File Input
    ↓
2. Generate Random AES-256 Key for the file
    ↓
3. Chunk File (e.g., into 256KB pieces)
    ↓
4. For Each Original Chunk:
   a. Hash the original chunk to get its unique identifier (used for the Merkle tree)
   b. Encrypt and chunk file into 256 KB pieces
   c. For Each Chunk:
      - Encrypt the chunk with AES-256-GCM using a unique nonce
      - Hash the *encrypted* chunk to get its storage hash (used for retrieval)
    ↓
5. Encrypt the file's AES Key with the Recipient's Public Key
    ↓
6. Store the individually encrypted chunks across the network
```



### File Integrity and Retrieval

A Merkle Tree is constructed from the hashes of the **original, unencrypted** chunks. This tree's root hash serves as the file's primary identifier and ensures top-level integrity.

```
Merkle Tree Structure (built from original chunk hashes):
                Root Hash
               /         \
         Branch A       Branch B
         /     \        /      \
  Hash(Chunk 1) ... Hash(Chunk N)
```

**Verification and Decryption Steps:**

The process of retrieving and verifying a file is the reverse of the encryption process. Verification against the Merkle root can only happen _after_ the original chunk has been reconstructed.

1.  **Fetch Chunks**: Download encrypted chunks from the network. Each chunk is requested by the hash of its encrypted content.
2.  **Decrypt Chunks**: Decrypt the main file AES key, then use it to decrypt each downloaded chunk individually.
3.  **Verify Chunk Integrity**: Hash the fully decrypted (plaintext) chunk.
4.  **Verify Merkle Proof**: Compare the new hash against the original chunk hash stored in the file's manifest. Then, use the provided Merkle proof to verify this hash against the file's trusted root hash.
5.  If all verifications pass, the chunk is valid and can be written to the output file. If verification fails, the data is considered corrupt or tampered with.

### Access Control

#### Permission Levels

| Level | Description     | Capabilities         |
| ----- | --------------- | -------------------- |
| Owner | File creator    | Full control         |
| Admin | Delegated admin | Modify permissions   |
| Write | Can modify      | Update file content  |
| Read  | View only       | Download and decrypt |
| None  | No access       | Cannot access file   |

#### Access Control Implementation

```javascript
interface AccessControl {
  owner: Address;
  admins: Address[];
  writers: Address[];
  readers: Address[];
  public: boolean;
  expiry?: number;
  conditions?: {
    timelock?: number;
    multisig?: Address[];
    payment?: Amount;
  };
}
```

## Network Security

### Peer Authentication

```
Handshake Protocol:
1. Client → Server: Hello + Public Key
2. Server → Client: Challenge Nonce
3. Client → Server: Signed Challenge
4. Server → Client: Session Key (encrypted)
5. Both: Derive shared secret
```

### Transport Security

#### TLS Configuration

```
Minimum Version: TLS 1.3
Cipher Suites:
- TLS_AES_256_GCM_SHA384
- TLS_CHACHA20_POLY1305_SHA256
- TLS_AES_128_GCM_SHA256

Key Exchange:
- X25519
- P-256
- P-384
```

#### libp2p Security

```
Security Protocols:
- Noise Protocol Framework
- TLS 1.3
- QUIC (experimental)

Multiplexing:
- mplex
- yamux

Transport:
- TCP
- QUIC
- WebRTC
- WebSocket
```

### DDoS Protection

#### Rate Limiting

| Endpoint         | Limit     | Window  |
| ---------------- | --------- | ------- |
| File Upload      | 10/min    | Sliding |
| File Download    | 100/min   | Sliding |
| DHT Queries      | 50/min    | Fixed   |
| Peer Connections | 100/hour  | Fixed   |
| API Calls        | 1000/hour | Sliding |

#### Connection Management

```
Max Connections: 1000
Max Connections per IP: 10
Connection Timeout: 30 seconds
Idle Timeout: 5 minutes
Blacklist Duration: 1 hour
```

## Blockchain Security

### Transaction Security

#### Double-Spend Prevention

1. Account-based model tracks balances
2. Transaction ordering via blockchain
3. Confirmation requirements (12 blocks recommended)
4. Nonce-based replay protection

#### Transaction Validation

```
Validation Steps:
1. Verify ECDSA signature (secp256k1)
2. Check account balance sufficiency
3. Validate nonce sequence (prevent replay)
4. Verify gas price and gas limit
5. Execute transaction and deduct gas fees
6. Validate Ethereum address format (EIP-55)
```

### Mining Security

#### 51% Attack Mitigation

- Ethash ASIC resistance
- Dynamic difficulty adjustment per block
- Network monitoring for hash rate changes
- Community checkpointing for critical blocks

#### Selfish Mining Prevention

- Random block propagation delays
- Peer reputation scoring
- Multiple mining pools encouraged
- Block template rotation

## Privacy Features

### Anonymous Routing

#### Onion Routing

```
Client → Proxy 1 → Proxy 2 → Proxy 3 → Destination
  ↓         ↓         ↓         ↓
Encrypted Encrypted Encrypted Plain
  (3x)      (2x)      (1x)     text
```

#### Mix Networks

- Random delays (0-5 seconds)
- Packet padding to fixed size
- Traffic mixing at nodes
- Cover traffic generation

### Metadata Protection

#### What's Hidden

- Real IP addresses (via proxies)
- Download patterns (via caching)
- File associations (via encryption)
- Transaction linkability (via mixing)

#### What's Visible

- Encrypted file sizes (approximate)
- Connection times
- Total bandwidth usage
- Node participation

### Private Transactions

#### Confidential Transfers

```
Using Pedersen Commitments:
- Hide transaction amounts
- Prove range validity
- Maintain auditability
```

#### Ring Signatures (Future)

```
Sign transaction with group:
- Actual signer unknown
- Plausible deniability
- Unlinkable transactions
```

## Threat Models

### Threat Categories

#### External Threats

| Threat            | Impact             | Mitigation                |
| ----------------- | ------------------ | ------------------------- |
| DDoS Attack       | Service disruption | Rate limiting, CDN        |
| Man-in-the-Middle | Data theft         | TLS, certificate pinning  |
| Sybil Attack      | Network takeover   | Proof-of-work, reputation |
| Eclipse Attack    | Isolation          | Peer diversity            |
| Routing Attack    | Traffic hijacking  | Multiple paths            |

#### Internal Threats

| Threat          | Impact             | Mitigation                       |
| --------------- | ------------------ | -------------------------------- |
| Malicious Peer  | Data corruption    | Redundancy, chunk verification   |
| Free Riding     | Resource drain     | Payment incentive system         |
| Peer Collusion  | Network disruption | Reputation penalties             |
| Data Mining     | Privacy breach     | Encryption, anonymity            |

### Attack Scenarios

#### Scenario 1: Peer Compromise

**Attack:** Attacker gains control of a peer node
**Impact:** Access to encrypted chunks stored on that peer
**Mitigation:**

- Client-side encryption (chunks are encrypted)
- Chunk distribution across multiple peers
- Regular integrity checks
- Peer reputation system
- **Note**: Compromising one peer only exposes encrypted chunks, not original files

#### Scenario 2: Network Partition

**Attack:** Split network into isolated segments
**Impact:** Double-spending, data unavailability
**Mitigation:**

- Multiple bootstrap nodes
- Cross-region connections
- Consensus checkpoints
- Automatic reconnection

#### Scenario 3: Timing Analysis

**Attack:** Correlate traffic patterns
**Impact:** De-anonymization
**Mitigation:**

- Random delays
- Cover traffic
- Batch processing
- Proxy rotation

## Security Best Practices

### For Users

#### Key Management

1. **Use Hardware Wallets:** Store keys offline
2. **Backup Mnemonics:** Secure physical copies
3. **Password Strength:** Minimum 12 characters
4. **2FA Enable:** Time-based OTP preferred
5. **Regular Rotation:** Change keys periodically

#### Safe File Sharing

1. **Verify Recipients:** Check Ethereum addresses (0x format)
2. **Encrypt Sensitive Files:** Always use encryption
3. **Set Expiration:** Time-limit access
4. **Monitor Access:** Track file downloads
5. **Revoke When Needed:** Remove permissions

#### Network Safety

1. **Use VPN/Tor:** Additional privacy layer
2. **Verify Certificates:** Check TLS certs
3. **Update Software:** Latest security patches
4. **Monitor Activity:** Check for anomalies
5. **Report Issues:** Help improve security

### For Node Operators

#### System Security

```bash
# Firewall Configuration
iptables -A INPUT -p tcp --dport 30304 -j ACCEPT  # P2P
iptables -A INPUT -p tcp --dport 8546 -j ACCEPT   # RPC (local only)
iptables -A INPUT -p tcp --dport 8080 -j ACCEPT   # File transfer
iptables -A INPUT -j DROP  # Drop all other

# File Permissions
chmod 600 ~/.chiral/keystore/*
chmod 700 ~/.chiral/keystore
chmod 755 ~/.chiral/data

# Process Isolation
docker run --security-opt no-new-privileges \
          --read-only \
          --tmpfs /tmp \
          chiral-node
```

#### Monitoring

```yaml
# Prometheus Metrics
metrics_to_monitor:
  - peer_count
  - bandwidth_usage
  - disk_usage
  - memory_usage
  - failed_authentications
  - invalid_chunks
  - reputation_score
```

#### Backup Strategy

1. **Regular Backups:** Daily automated backups
2. **Offsite Storage:** Geographic redundancy
3. **Encryption:** Backup encryption
4. **Test Restoration:** Monthly restore tests
5. **Version Control:** Keep multiple versions

### For Developers

#### Secure Coding

```typescript
// Input Validation
function validateFileHash(hash: string): boolean {
  const regex = /^cn1[a-f0-9]{64}$/;
  return regex.test(hash) && hash.length === 67;
}

// Constant-Time Comparison
function secureCompare(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let result = 0;
  for (let i = 0; i < a.length; i++) {
    result |= a.charCodeAt(i) ^ b.charCodeAt(i);
  }
  return result === 0;
}

// Rate Limiting
const rateLimiter = new RateLimiter({
  windowMs: 60 * 1000,
  max: 100,
  message: "Too many requests",
});
```

#### Security Headers

```typescript
app.use((req, res, next) => {
  res.setHeader("X-Content-Type-Options", "nosniff");
  res.setHeader("X-Frame-Options", "DENY");
  res.setHeader("X-XSS-Protection", "1; mode=block");
  res.setHeader(
    "Content-Security-Policy",
    "default-src 'self'; script-src 'self' 'unsafe-inline'"
  );
  res.setHeader(
    "Strict-Transport-Security",
    "max-age=31536000; includeSubDomains"
  );
  next();
});
```

## Incident Response

### Response Plan

#### Phase 1: Detection

- Automated alerts
- User reports
- Monitoring anomalies
- Security scans

#### Phase 2: Containment

- Isolate affected systems
- Block malicious actors
- Preserve evidence
- Notify stakeholders

#### Phase 3: Eradication

- Remove malicious code
- Patch vulnerabilities
- Update signatures
- Reset credentials

#### Phase 4: Recovery

- Restore services
- Verify integrity
- Monitor closely
- Update documentation

#### Phase 5: Lessons Learned

- Post-mortem analysis
- Update procedures
- Improve monitoring
- Share knowledge

### Contact Information

```
Security Team Email: security@chiralnetwork.org
Bug Bounty Program: https://chiralnetwork.org/security/bounty
Emergency Hotline: +1-XXX-XXX-XXXX
PGP Key: https://chiralnetwork.org/security/pgp
```

## Compliance

### Data Protection

#### GDPR Compliance

- Right to erasure (file deletion)
- Data portability (export)
- Privacy by design
- Consent management
- Data minimization

#### Regional Requirements

| Region     | Requirement | Implementation    |
| ---------- | ----------- | ----------------- |
| EU         | GDPR        | Full compliance   |
| California | CCPA        | Privacy controls  |
| China      | PIPL        | Data localization |
| Russia     | 152-FZ      | Local storage     |

### Audit Logging

#### What to Log

```json
{
  "timestamp": "2024-01-01T00:00:00Z",
  "event_type": "file_access",
  "user_id": "hashed_user_id",
  "action": "download",
  "resource": "file_hash",
  "result": "success",
  "ip_address": "hashed_ip",
  "metadata": {
    "size": 1024,
    "duration_ms": 250
  }
}
```

#### Log Retention

- Access logs: 90 days
- Transaction logs: 1 year
- Security events: 2 years
- Audit trails: 7 years

## Security Roadmap

### Current Implementation

- ✅ AES-256 file encryption
- ✅ TLS 1.3 transport security
- ✅ Basic access control
- ✅ Rate limiting
- ✅ Input validation

### Phase 1: Enhanced Privacy (Q1 2024)

- [ ] Onion routing
- [ ] Mix networks
- [ ] Private transactions
- [ ] Metadata obfuscation

### Phase 2: Advanced Security (Q2 2024)

- [ ] Zero-knowledge proofs
- [ ] Homomorphic encryption
- [ ] Secure multi-party computation
- [ ] Threshold signatures

### Phase 3: Quantum Resistance (Q3 2024)

- [ ] Post-quantum algorithms
- [ ] Lattice-based crypto
- [ ] Hash-based signatures
- [ ] Quantum key distribution

## Security Resources

### Documentation

- Security Whitepaper: [Link]
- Threat Model Analysis: [Link]
- Penetration Test Reports: [Link]
- Audit Reports: [Link]

### Tools

- Security Scanner: `chiral-scan`
- Vulnerability Database: CVE tracking
- Security Updates: RSS feed
- Incident Reports: Public disclosure

### Community

- Security Working Group
- Bug Bounty Program
- Responsible Disclosure
- Security Newsletter

## Conclusion

Security and privacy are fundamental to the Chiral Network's design. Through multiple layers of encryption, anonymous routing, and careful protocol design, we provide users with a secure platform for distributed file storage while maintaining their privacy. Regular audits, community engagement, and continuous improvement ensure the network remains resistant to evolving threats.
