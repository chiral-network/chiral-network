# Reputation System

> **Status:** MVP design for blockchain-based transaction reputation with DHT caching. Penalty system includes proof-backed complaints with cryptographic evidence.

## Overview

Chiral Network tracks peer reputation through **on-chain transaction history** as the authoritative source of truth. The DHT serves as a volatile cache for quick lookups and recent feedback, but all reputation is ultimately derived from verifiable blockchain transactions between peers. This hybrid approach balances the immutability and reliability of on-chain data with the performance benefits of distributed caching.

### Core Principles

1. **Blockchain as Source of Truth**: All reputation stems from completed on-chain transactions
2. **DHT as Performance Cache**: Quick lookups without querying the full blockchain every time
3. **Transaction-Centric**: Reputation grows with successful transaction history (seeding or downloading)
4. **Proof-Backed Penalties**: Complaints require cryptographic evidence (signed handshakes, transaction data)
5. **Hybrid Verification**: Recent activity via DHT, historical data via blockchain

### Goals

- Build reputation on immutable, verifiable on-chain transaction history
- Use DHT for performance without relying on it for persistence
- Keep the system PoW-friendly: identities correspond to existing mining/transaction keys
- Support both reliable (on-chain) and unreliable (DHT gossip) penalties
- Enable future extensions without breaking compatibility

## Trust Levels

Peers are bucketed by their **transaction score**, calculated from their on-chain transaction history with adjustments for complaints and penalties.

| Trust Level | Score Range | Description |
|-------------|-------------|-------------|
| **Trusted** | 0.8 - 1.0 | Extensive successful transaction history, no unresolved complaints |
| **High** | 0.6 - 0.8 | Strong transaction history, reliable performance |
| **Medium** | 0.4 - 0.6 | Moderate transaction history, acceptable performance |
| **Low** | 0.2 - 0.4 | Limited history or some complaints, approach with caution |
| **Unknown** | 0.0 - 0.2 | New peers or those with insufficient transaction history |

## Reputation Architecture

### Two-Tier System

#### 1. On-Chain Layer (Authoritative)

The blockchain records all completed transactions. Each transaction inherently provides reputation data:

- **Successful completion** = positive reputation signal
- **Transaction count** = measure of experience and reliability
- **Role diversity** = reputation as both seeder and downloader
- **Complaint records** = negative signals with cryptographic proof

**On-chain data includes:**
- Transaction hash and block number
- Parties involved (seeder and downloader)
- File hash or content identifier
- Payment amount
- Timestamp
- Optional: Complaint flag with evidence pointer

#### 2. DHT Layer (Volatile Cache)

The DHT stores recent reputation updates for quick access:

- **Recent transaction summaries** (last 100 per peer)
- **Pending complaints** with attached cryptographic evidence
- **Score cache** to avoid repeated blockchain queries
- **Gossip signals** about suspicious behavior

**DHT cache characteristics:**
- Data expires and gets pruned regularly
- No guarantee of persistence
- Fast lookups without full blockchain scan
- Useful for real-time peer selection
- Must be verified against on-chain data when accuracy matters

### Reputation Calculation Flow

```
1. Query DHT for recent activity (last N transactions)
   ├─ If cache hit → Use cached score with timestamp
   └─ If cache miss or stale → Continue to step 2

2. Query blockchain for full transaction history
   ├─ Count successful transactions (seeding + downloading)
   ├─ Identify complaint records with proofs
   └─ Calculate base score from transaction count

3. Apply penalty adjustments
   ├─ Reliable penalties: On-chain complaint with proof
   └─ Unreliable penalties: DHT gossip (lower weight)

4. Cache result in DHT for future lookups
   └─ Store with TTL (default: 10 minutes)

5. Return final reputation score
```

## Transaction-Based Reputation

### Base Score Calculation

Reputation grows with successful transactions. The base formula:

```
base_score = min(1.0, successful_transactions / maturity_threshold)

where:
  successful_transactions = count of completed on-chain transactions
  maturity_threshold = 100 (configurable, number of transactions to reach max base score)
```

**Transaction types that count:**
- Successful file downloads (as downloader, payment confirmed)
- Successful file uploads (as seeder, payment received)
- Both roles contribute equally to reputation

**What doesn't count:**
- Incomplete transactions
- Disputed transactions (until resolved)
- Transactions from blacklisted peers
- Self-transactions (same peer both sides)

### Time Decay (Optional)

Clients may optionally apply time decay to emphasize recent activity:

```
weighted_score = Σ(tx_weight × decay_factor) / Σ(decay_factor)

decay_factor = e^(-age_in_days / half_life_days)
half_life_days = 90 (default, configurable)
```

This ensures inactive peers gradually lose reputation even if they had strong historical performance.

## Penalty System

### Two Types of Penalties

#### 1. Reliable Penalties (On-Chain)

**Proof-backed complaints recorded on the blockchain:**

```solidity
struct Complaint {
    address complainant;      // Who is filing the complaint
    address accused;          // Who is being accused
    bytes32 txHash;          // Related transaction
    bytes32 evidenceHash;    // Hash of supporting evidence
    string complaintType;    // "non-payment", "incomplete-transfer", "data-corruption"
    uint256 timestamp;
    ComplaintStatus status;  // pending, upheld, dismissed
}
```

**Complaint process:**
1. **Complainant submits** on-chain complaint with evidence hash
2. **Evidence stored** in DHT or other off-chain storage (IPFS, etc.)
3. **Community or automated system reviews** evidence
4. **Complaint marked** as upheld, dismissed, or remains pending
5. **Reputation adjusted** based on outcome

**Evidence requirements:**
- Signed handshake messages showing promised payment
- Transaction logs proving non-delivery
- Cryptographic proof of corrupted data
- Protocol violation records

**Penalty weights:**
```
upheld_complaint_penalty = -0.1 per complaint (adjusts final score)
pending_complaint_penalty = -0.02 per complaint (temporary, lighter)
dismissed_complaint_penalty = 0 (no effect)
```

#### 2. Unreliable Penalties (DHT Gossip)

**Lightweight complaints propagated through DHT:**

These provide faster feedback but carry less weight since they lack on-chain verification.

```typescript
interface GossipComplaint {
    target_id: string;           // Accused peer ID
    complainant_id: string;      // Who is complaining
    complaint_type: string;      // Type of misbehavior
    evidence: {
        signed_handshake?: string;  // Signed message promising payment
        protocol_logs?: string;      // Protocol violation evidence
        timestamps?: number[];       // Relevant timing data
    };
    issued_at: number;           // Unix timestamp
    ttl: number;                 // Time to live in DHT
    signature: string;           // Complainant's signature
}
```

**Gossip complaint process:**
1. **Peer publishes complaint** to DHT with cryptographic evidence
2. **DHT nodes propagate** complaint (limited TTL, e.g., 7 days)
3. **Other peers retrieve** and validate signature
4. **Apply lightweight penalty** in local scoring
5. **Evidence available** for manual review or escalation to on-chain complaint

**Gossip penalty:**
```
gossip_penalty = -0.01 per distinct complainant (max 5 gossip complaints count)
```

**Validation:**
- Signature must verify against complainant's known key
- Attached evidence (e.g., signed handshake) must be cryptographically valid
- Complainant must have sufficient reputation to file (prevents spam)
- Deduplicate by `(complainant_id, target_id, complaint_type)`

### Common Complaint Types

| Type | Description | Typical Evidence |
|------|-------------|------------------|
| **non-payment** | Downloader received file but didn't pay | Signed payment promise, delivery proof |
| **incomplete-transfer** | Seeder stopped mid-transfer | Protocol logs, chunk manifests |
| **data-corruption** | File chunks failed verification | Hash mismatches, chunk signatures |
| **protocol-violation** | Peer violated P2P protocol rules | Connection logs, malformed messages |
| **spam-connection** | Excessive connection attempts | Rate limit logs, timestamps |

### Complaint Resolution

**On-chain complaints can be resolved through:**

1. **Automated verification**: Smart contract checks cryptographic proofs
2. **Community voting**: Stake-weighted voting on ambiguous cases (future)
3. **Manual review**: Trusted arbitrators examine complex disputes (future)
4. **Time-based expiry**: Old pending complaints auto-dismiss after 30 days

**Resolution outcomes:**
- **Upheld**: Penalty applied, complainant may receive compensation
- **Dismissed**: No penalty, false accusation may penalize complainant
- **Settled**: Parties resolved privately, complaint withdrawn

## Reputation Score Formula

The final reputation score combines transaction history with penalties:

```
final_score = base_score
              - Σ(upheld_complaint_penalty)
              - Σ(pending_complaint_penalty)
              - Σ(gossip_penalty)
              + bonus_adjustments

Clamped to [0.0, 1.0]
```

**Bonus adjustments:**
- Long-term reliability: +0.05 if active >6 months with no complaints
- High volume: +0.05 if >500 successful transactions
- Role diversity: +0.02 if balanced seeding/downloading (40-60% ratio)

## DHT Storage Format

### Transaction Summary Cache

```typescript
interface TransactionSummary {
    peer_id: string;                    // Target peer
    total_transactions: number;         // Lifetime count from blockchain
    recent_transactions: Transaction[]; // Last 100, for quick display
    last_updated: number;               // Unix timestamp
    cached_score: number;               // Pre-calculated score
    complaints: {
        on_chain: number;               // Count of on-chain complaints
        gossip: number;                 // Count of active gossip complaints
    };
}
```

**DHT key:** `H(peer_id || "tx-summary")`

**Storage rules:**
- TTL: 10 minutes (configurable)
- Updated after each transaction completion
- Pruned when stale or DHT storage pressure increases
- No persistence guarantee

### Complaint Cache (Gossip)

```typescript
interface ComplaintCache {
    target_id: string;
    complaints: GossipComplaint[];      // Active gossip complaints
    last_updated: number;
}
```

**DHT key:** `H(target_id || "complaints")`

**Storage rules:**
- TTL: 7 days per complaint
- Deduplicated by complainant + type
- Validated signatures required
- Auto-pruned on expiry

## Reputation Features

### Publishing Reputation Updates

**After completing a transaction:**

1. **Transaction confirms on-chain** (12 blocks deep)
2. **Parties' reputation increments** automatically
3. **Update published to DHT** as cache entry:
   ```typescript
   await dhtService.cacheTransactionSummary({
       peer_id: seeder_id,
       total_transactions: count + 1,
       recent_transactions: [...recent, new_tx],
       cached_score: recalculated_score,
   });
   ```

**Filing a complaint (reliable):**

1. **Gather evidence** (signed messages, logs, etc.)
2. **Submit on-chain complaint** with evidence hash
3. **Store evidence** in DHT or IPFS
4. **Wait for resolution** (automated or manual review)

**Filing a complaint (unreliable/gossip):**

1. **Assemble gossip complaint** with attached evidence
2. **Sign complaint** with your transaction key
3. **Publish to DHT** with 7-day TTL
4. **Others retrieve and validate** your signature and evidence

### Retrieving Reputation

**Quick lookup (DHT-first):**

```typescript
const summary = await dhtService.getTransactionSummary(peer_id);
if (summary && !isStale(summary.last_updated)) {
    return summary.cached_score; // Fast path
}
// Otherwise fall back to blockchain query
```

**Authoritative lookup (blockchain):**

```typescript
const txCount = await blockchain.getTransactionCount(peer_id);
const complaints = await blockchain.getComplaints(peer_id);
const baseScore = Math.min(1.0, txCount / maturityThreshold);
const finalScore = applyPenalties(baseScore, complaints);
```

**Hybrid approach (recommended):**

```typescript
// 1. Check DHT for recent cache
const cached = await dhtService.getTransactionSummary(peer_id);

// 2. If stale or missing, query blockchain
if (!cached || isStale(cached.last_updated, 600_000)) {
    const onChainData = await blockchain.getReputationData(peer_id);
    const score = calculateScore(onChainData);

    // 3. Update DHT cache
    await dhtService.cacheTransactionSummary({
        peer_id,
        total_transactions: onChainData.txCount,
        cached_score: score,
        last_updated: Date.now(),
    });

    return score;
}

return cached.cached_score;
```

### Peer Selection for Downloads

When selecting a seeder:

1. **Query DHT** for available seeders
2. **Retrieve reputation scores** (DHT-first, blockchain fallback)
3. **Filter out blacklisted peers**
4. **Rank by reputation score**
5. **Apply additional heuristics**:
   - Geographic proximity
   - Available bandwidth
   - Current connection count
6. **Present top candidates** to user
7. **Allow manual override**

### Reputation Analytics

The Analytics page displays:

- **Personal reputation score** with trust level
- **Transaction history** (on-chain verified)
- **Active complaints** (both on-chain and gossip)
- **Score trend** over time
- **Role balance** (seeding vs. downloading ratio)
- **Peer comparisons** (percentile ranking)

## Blacklisting

Users can blacklist misbehaving peers based on reputation signals.

### Blacklist Features

- **Manual blacklisting**: Add any peer by ID
- **Automatic blacklisting**: Trigger on low score or complaint threshold
- **Reason documentation**: Track why each peer was blocked
- **Complaint review**: View associated complaints and evidence
- **Temporary blocks**: Auto-unban after configurable period
- **Permanent blocks**: Manually flagged peers stay blocked

### Blacklist Criteria

Peers may be automatically blacklisted for:

- **Score below threshold** (default: 0.2)
- **Multiple upheld complaints** (default: 3 or more)
- **Excessive gossip complaints** (default: 5+ from distinct peers)
- **Protocol violations** detected by local client
- **Connection abuse** (rate limiting violations)

### Blacklist Settings

| Setting | Description | Default |
|---------|-------------|---------|
| `blacklist_mode` | `manual`, `automatic`, or `hybrid` | `hybrid` |
| `auto_blacklist_enabled` | Enable automatic blacklisting | `true` |
| `score_threshold` | Score below which triggers auto-blacklist | `0.2` |
| `complaint_threshold` | Number of upheld complaints to trigger | `3` |
| `gossip_threshold` | Number of gossip complaints to trigger | `5` |
| `retention_period` | Days before auto-unban consideration | `30` |
| `notify_on_blacklist` | Show notification when peer auto-blacklisted | `true` |

## Privacy Considerations

### What's Tracked On-Chain

- Peer IDs (cryptographic identifiers, not real identities)
- Transaction hashes and block numbers
- Complaint records with evidence hashes
- Resolution outcomes

### What's Tracked in DHT

- Recent transaction summaries
- Gossip complaints with evidence
- Cached reputation scores
- Peer activity timestamps

### What's NOT Tracked

- File content or names
- Real-world identities
- IP addresses (hidden via relay/proxy)
- Personal information
- Private keys or wallet details

### Anonymous Mode

When anonymous mode is enabled:

- **On-chain identity persists** (transactions still build reputation)
- **IP hidden** via Circuit Relay v2 and SOCKS5 proxy
- **DHT access preserved** for score lookups
- **Rotating keys** reset reputation (discouraged for this reason)
- **Gossip complaints** still visible by peer ID

## Implementation Details

### Backend API (Rust/Tauri)

```rust
#[tauri::command]
async fn get_peer_reputation(peer_id: String) -> Result<ReputationScore, String> {
    // 1. Check DHT cache
    if let Some(cached) = dht::get_cached_summary(&peer_id).await? {
        if !cached.is_stale(Duration::from_secs(600)) {
            return Ok(cached.score);
        }
    }

    // 2. Query blockchain
    let tx_count = blockchain::get_transaction_count(&peer_id).await?;
    let complaints = blockchain::get_complaints(&peer_id).await?;

    // 3. Calculate score
    let base_score = (tx_count as f64 / MATURITY_THRESHOLD as f64).min(1.0);
    let final_score = apply_penalties(base_score, &complaints);

    // 4. Update DHT cache
    dht::cache_summary(&peer_id, tx_count, final_score).await?;

    Ok(final_score)
}

#[tauri::command]
async fn file_complaint(
    target_id: String,
    complaint_type: String,
    evidence: ComplaintEvidence,
    reliable: bool,
) -> Result<String, String> {
    if reliable {
        // On-chain complaint
        let evidence_hash = hash_evidence(&evidence);
        let tx_hash = blockchain::submit_complaint(
            &target_id,
            &complaint_type,
            evidence_hash,
        ).await?;

        // Store evidence off-chain
        ipfs::store_evidence(&evidence_hash, &evidence).await?;

        Ok(tx_hash)
    } else {
        // DHT gossip complaint
        let gossip = create_gossip_complaint(target_id, complaint_type, evidence);
        let signature = sign_complaint(&gossip)?;

        dht::publish_gossip_complaint(gossip, signature).await?;

        Ok(gossip.id)
    }
}
```

### Frontend Service (TypeScript)

```typescript
// src/lib/services/reputationService.ts

export class ReputationService {
    async getPeerScore(peerId: string): Promise<number> {
        return await invoke('get_peer_reputation', { peerId });
    }

    async fileComplaint(
        targetId: string,
        type: ComplaintType,
        evidence: ComplaintEvidence,
        reliable: boolean = false
    ): Promise<string> {
        return await invoke('file_complaint', {
            targetId,
            complaintType: type,
            evidence,
            reliable,
        });
    }

    async getComplaintsAgainst(peerId: string): Promise<Complaint[]> {
        const onChain = await invoke('get_on_chain_complaints', { peerId });
        const gossip = await invoke('get_gossip_complaints', { peerId });
        return [...onChain, ...gossip];
    }
}
```

### Configuration Defaults

| Parameter | Description | Default |
|-----------|-------------|---------|
| `confirmation_threshold` | Blocks required before transaction counts for reputation | 12 |
| `maturity_threshold` | Transactions needed to reach max base score (1.0) | 100 |
| `cache_ttl` | How long to cache DHT summaries (milliseconds) | 600,000 (10 min) |
| `gossip_ttl` | How long gossip complaints live in DHT (seconds) | 604,800 (7 days) |
| `decay_half_life` | Half-life for optional time decay (days) | 90 |
| `max_gossip_weight` | Maximum number of gossip complaints that count | 5 |
| `upheld_complaint_penalty` | Score reduction per upheld on-chain complaint | 0.1 |
| `pending_complaint_penalty` | Score reduction per pending on-chain complaint | 0.02 |
| `gossip_complaint_penalty` | Score reduction per gossip complaint | 0.01 |
| `complaint_timeout` | Days before pending complaint auto-dismisses | 30 |

## Using Reputation Data

### For Downloads

```typescript
import { reputationService } from '$lib/services/reputationService';

// Get available seeders
const seeders = await dhtService.findSeeders(fileHash);

// Score each seeder
const scoredSeeders = await Promise.all(
    seeders.map(async (seeder) => ({
        ...seeder,
        reputation: await reputationService.getPeerScore(seeder.id),
    }))
);

// Sort by reputation
const ranked = scoredSeeders.sort((a, b) => b.reputation - a.reputation);

// Present top candidates
showPeerSelectionModal(ranked.slice(0, 10));
```

### For Uploads

```typescript
// Complete transaction
await completeFileTransfer(downloaderId, fileHash);

// Reputation automatically updates on-chain
// Optionally cache update in DHT
await dhtService.cacheTransactionSummary(myPeerId, {
    total_transactions: myTxCount + 1,
    cached_score: await calculateMyScore(),
});
```

### Filing Complaints

```typescript
// Downloader didn't pay after receiving file
const evidence = {
    signed_handshake: downloadHandshake, // Their signed payment promise
    delivery_proof: chunkManifest,       // Proof we sent all chunks
    protocol_logs: transferLogs,         // Connection and transfer logs
};

// File on-chain complaint (more severe, requires gas)
await reputationService.fileComplaint(
    downloaderId,
    'non-payment',
    evidence,
    true // reliable = on-chain
);

// Or file gossip complaint (lighter, faster)
await reputationService.fileComplaint(
    downloaderId,
    'non-payment',
    evidence,
    false // unreliable = DHT gossip
);
```

## Troubleshooting

### Low Reputation Score

**Causes:**
- Few completed transactions (new user)
- Upheld complaints against you
- Many gossip complaints from other peers
- Long period of inactivity (if decay enabled)

**Solutions:**
- Complete more transactions successfully
- Resolve disputes and request complaint dismissal
- Investigate complaints and improve behavior
- Stay active to maintain score

### Reputation Not Updating

**Causes:**
- Transactions not yet confirmed (< 12 blocks)
- DHT cache stale but blockchain query failing
- Geth node not synced
- Network connectivity issues

**Solutions:**
- Wait for transaction confirmation
- Check Geth sync status in Network page
- Restart application
- Clear DHT cache manually

### False Complaints

**Causes:**
- Malicious peers filing fake complaints
- Misunderstanding of protocol
- Technical issues misinterpreted as misbehavior

**Solutions:**
- Provide counter-evidence to dispute on-chain complaints
- Gossip complaints expire after 7 days
- Build positive reputation to outweigh false signals
- Report abusive complainants

### DHT Cache Inconsistency

**Causes:**
- DHT nodes pruning data aggressively
- Network partitions
- Cache TTL too short

**Solutions:**
- Increase `cache_ttl` in settings
- Rely on blockchain for authoritative data
- DHT is best-effort, not guaranteed storage

## See Also

- [Network Protocol](network-protocol.md) — Peer discovery and connection details
- [File Sharing](file-sharing.md) — Transfer workflows and protocols
- [Wallet & Blockchain](wallet-blockchain.md) — Transaction and blockchain interaction
- [Smart Contracts](smart-contracts.md) — Complaint contract implementation
- [Roadmap](roadmap.md) — Future reputation features

## Related Systems

- **Bitcoin**: No built-in reputation; trust is transactional only
- **Ethereum**: On-chain interactions build implicit trust, no explicit reputation protocol
- **OpenBazaar**: Uses trust scores based on transaction history, similar approach
- **IPFS**: No built-in reputation; relies on Bitswap debt ledgers per peer pair
- **Filecoin**: Uses on-chain storage proofs and collateral, not reputation scoring

## Future Extensions

### Phase 2

- **Complaint review UI**: Visual interface for examining evidence
- **Automated complaint resolution**: Smart contract verification of common evidence types
- **Evidence encryption**: Privacy-preserving complaint evidence storage
- **Complainant reputation**: Weight complaints by filer's credibility

### Phase 3

- **Additional metrics**: Uptime, relay quality, storage proof participation
- **Community voting**: Stake-weighted dispute resolution
- **Reputation delegation**: Trust networks and transitive trust
- **Performance scoring**: Bandwidth, latency, reliability metrics
