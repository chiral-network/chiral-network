# IMPORTANT: This document needs full revision. We don't yet have a reputation system design, which should be a high-priority item.

# Reputation System

Chiral Network implements a comprehensive peer reputation system to ensure reliable file transfers and network quality.

## Overview

The reputation system tracks peer behavior and assigns trust scores based on:
- **Transfer success rate**: Successful vs. failed transfers
- **Latency**: Response time to requests
- **Bandwidth**: Upload/download speeds
- **Uptime**: Time peer has been online
- **Encryption support**: Whether peer supports secure transfers

## Trust Levels

Peers are classified into trust levels based on their composite score:

| Trust Level | Score Range | Description |
|-------------|-------------|-------------|
| **Trusted** | 0.8 - 1.0 | Highly reliable, consistently good performance |
| **High** | 0.6 - 0.8 | Very reliable, above-average performance |
| **Medium** | 0.4 - 0.6 | Moderately reliable, acceptable performance |
| **Low** | 0.2 - 0.4 | Less reliable, below-average performance |
| **Unknown** | 0.0 - 0.2 | New or unproven peers |

## Reputation Metrics

### Composite Score Calculation

The reputation score is calculated using multiple factors:

```typescript
compositeScore = (
  latencyScore * 0.25 +
  bandwidthScore * 0.25 +
  uptimeScore * 0.20 +
  successRateScore * 0.30
)
```

**Weight Distribution**:
- Success Rate: 30% (most important)
- Latency: 25%
- Bandwidth: 25%
- Uptime: 20%

### Individual Metrics

#### 1. Latency Score
- Based on average response time
- Lower latency = higher score
- Measured during peer interactions
- Updated with each transfer

#### 2. Bandwidth Score
- Based on upload/download speeds
- Higher bandwidth = higher score
- Measured in KB/s
- Averaged over multiple transfers

#### 3. Uptime Score
- Percentage of time peer is online
- Calculated from first seen to last seen
- Higher uptime = higher score
- Resets after extended offline periods

#### 4. Success Rate Score
- Successful transfers / total transfers
- Most heavily weighted metric
- Includes both uploads and downloads
- Recent transfers weighted more heavily

## Reputation Features

### Peer Analytics

The Reputation page displays:

- **Total Peers**: Number of known peers
- **Trusted Peers**: Count of highly-rated peers
- **Average Score**: Network-wide average reputation
- **Top Performers**: Leaderboard of best peers
- **Trust Distribution**: Breakdown by trust level

### Filtering & Sorting

**Filter Options**:
- Trust level (Trusted, High, Medium, Low, Unknown)
- Encryption support (Supported / Not Supported / Any)
- Minimum uptime percentage

**Sort Options**:
- By reputation score (highest first)
- By total interactions (most active)
- By last seen (most recent)

### Peer Selection

When downloading files, the system:

1. **Queries available seeders** from DHT
2. **Retrieves reputation scores** for each
3. **Ranks seeders** by composite score
4. **Presents top peers** in selection modal
5. **User can override** automatic selection

### Reputation History

Each peer maintains a history of:
- Reputation score over time
- Recent interactions (last 100)
- Trust level changes
- Performance trends

## Relay Reputation

Peers running as relay servers earn additional reputation:

### Relay Metrics

- **Circuits Successful**: Number of relay connections established
- **Reservations Accepted**: Number of relay reservations granted
- **Bytes Relayed**: Total data relayed for other peers
- **Uptime as Relay**: Time operating as relay server

### Relay Leaderboard

The Reputation page shows top relay nodes:
- Ranked by relay reputation score
- Displays relay-specific metrics
- Shows your node's rank (if running as relay)
- Updates in real-time

### Earning Relay Reputation

To earn relay reputation:

1. **Enable Relay Server** in Settings → Network
2. **Keep node online** with good uptime
3. **Accept reservations** from NAT'd peers
4. **Maintain reliable service** (don't drop circuits)
5. **Monitor your ranking** in Reputation page

## Blacklisting

Users can blacklist misbehaving peers:

### Blacklist Features

- **Manual blacklisting**: Add peer by address
- **Automatic blacklisting**: System flags suspicious behavior
- **Blacklist reasons**: Document why peer was blocked
- **Timestamp tracking**: When peer was blacklisted
- **Remove from blacklist**: Unblock peers

### Blacklist Criteria

Peers may be automatically blacklisted for:
- Repeated failed transfers
- Malformed data
- Protocol violations
- Excessive connection attempts
- Suspicious activity patterns

## Privacy Considerations

### What's Tracked

- Peer IDs (not real identities)
- Transfer statistics
- Connection metadata
- Performance metrics

### What's NOT Tracked

- File content
- User identities
- IP addresses (if using proxy/relay)
- Personal information

### Anonymous Mode

When anonymous mode is enabled:
- Your reputation is still tracked by others
- You can still view others' reputation
- Your peer ID changes periodically
- IP address hidden via relay/proxy

## Using Reputation Data

### For Downloads

1. **Check seeder reputation** before downloading
2. **Prefer Trusted peers** for important files
3. **Monitor transfer progress** from selected peers
4. **Report issues** if peer misbehaves

### For Uploads

1. **Build good reputation** by:
   - Maintaining high uptime
   - Completing transfers reliably
   - Supporting encryption
   - Running as relay server (optional)
2. **Monitor your reputation** in Analytics page
3. **Respond to requests** promptly

### For Network Health

1. **Avoid Low/Unknown peers** for critical transfers
2. **Contribute to network** to build reputation
3. **Report malicious peers** for blacklisting
4. **Help NAT'd peers** by running relay server

## API Access

Developers can access reputation data:

```typescript
import PeerSelectionService from '$lib/services/peerSelectionService';

// Get all peer metrics
const metrics = await PeerSelectionService.getPeerMetrics();

// Get composite score for a peer
const score = PeerSelectionService.compositeScoreFromMetrics(peerMetrics);

// Select best peers for download
const bestPeers = await PeerSelectionService.selectPeersForDownload(
  availableSeederIds,
  minRequiredPeers
);
```

## Troubleshooting

### Low Reputation Score

**Causes**:
- Unreliable connection
- Slow bandwidth
- Frequent disconnections
- Failed transfers

**Solutions**:
- Improve internet connection
- Keep application running
- Don't pause uploads mid-transfer
- Enable encryption support

### Peers Not Showing Reputation

**Causes**:
- New peers (no history)
- DHT not connected
- Reputation service not initialized

**Solutions**:
- Wait for peers to interact
- Check Network page for DHT status
- Restart application

### Reputation Not Updating

**Causes**:
- No recent transfers
- Application not running
- Backend service issue

**Solutions**:
- Perform some transfers
- Check console for errors
- Restart application

## See Also

- [Network Protocol](network-protocol.md) - Peer discovery details
- [File Sharing](file-sharing.md) - Transfer workflows
- [User Guide](user-guide.md) - Using the Reputation page

## Hybrid Reputation System with Economic Security (Staking + Challenge)

### Overview

A scalable reputation system combining off-chain event collection with on-chain anchoring. **Key decision**: Relay nodes act as aggregators and upload snapshots only for transfers they relay; regular nodes do not upload snapshots (they only sign events). **Anchoring is required** for uploaded snapshots: relays must stake tokens and accept a challenge period. Aggregators earn from transaction fees and block rewards.

```
┌─────────────────────────────────────────────────────────────────┐
│                     HYBRID REPUTATION SYSTEM                     │
└─────────────────────────────────────────────────────────────────┘
                                 │
        ┌────────────────────────┼────────────────────────┐
        │                        │                        │
┌───────▼────────┐    ┌──────────▼─────────┐    ┌────────▼────────┐
│  File Transfer │    │    Aggregators     │    │    Blockchain   │
│    (Users)     │    │ (Relay Nodes 24/7) │    │   (Anchoring)   │
└───────┬────────┘    └──────────┬─────────┘    └────────┬────────┘
        │                        │                        │
        │ 1. Sign events         │ 3. Collect & validate  │
        │    (secp256k1)         │    events per epoch    │
        │                        │                        │
        │ 2. Pay tx fee    ──────┼───> Transaction Pool   │
        │    (secp256k1)         │                        │
        │                        │ 4. Compute Merkle tree │
        ▼                        │    (SHA-256)           │
   DHT Metadata ◄─────────────────┤                        │
                                 │ 5. Publish to DHT      │
                                 │    {epoch, merkle_root}│
                                 │                        │
                                 │ 6. Anchor on-chain     │
                                 │    + Stake deposit     │
                                 └────────────────────────┤
                                                          │
                           ┌──────────────────────────────┤
                           │                              │
                    ┌──────▼──────┐              ┌────────▼────────┐
                    │   Honest    │              │   Dishonest     │
                    │ Aggregator  │              │  Aggregator     │
                    └──────┬──────┘              └────────┬────────┘
                           │                              │
                  Challenge Period                        │
                           │                      User Challenges
                           ▼                              │
                   ✅ No Challenges                       ▼
                   • Withdraw stake         ❌ Challenge Success
                   • Receive pool rewards   • Lose stake
                   • Continue earning       • Reward → Challenger
                                            • Reputation destroyed
```

### 1. Goals

- **Scalability**: Off-chain first (DHT), required on-chain anchors per epoch
- **Economic Security**: Aggregators stake tokens, users can challenge false events
- **Sustainability**: Transaction fees + block rewards fund aggregators
- **Decentralization**: Multiple independent aggregators, client-side weighting

### 2. Actors

- **Downloader/Seeder**: Sign transfer events and payments (secp256k1)
- **Relay/Aggregator**: Relay servers that facilitate NAT traversal and act as aggregators
  - Always-online assumption (24/7 uptime expected)
  - Collect only relayed transfer events they directly facilitated
  - Compute Merkle trees and publish snapshots
  - Must anchor snapshots on-chain with stake; subject to challenge window

### 3. Event Types

```
EVENT FLOW:

┌──────────────┐                           ┌──────────────┐
│  Downloader  │                           │    Seeder    │
└──────┬───────┘                           └──────┬───────┘
       │                                          │
       │ 1. Session Request (signed)              │
       │  {file_hash, chunks, nonce, sig_down}    │
       ├─────────────────────────────────────────>│
       │                                          │
       │            2. Transfer Data              │
       │<─────────────────────────────────────────┤
       │                                          │
       │                                          │
    SUCCESS PATH ✅              FAILURE PATH ❌
       │                                          │
       │ 3a. Success Receipt                      │ 3b. Nonpayment Event
       │  • dual-signed                           │  • seeder-signed only
       │  • sig_down + sig_seed                   │  • includes session_request_sig
       │  • +reputation                           │  • -reputation
       ▼                                          ▼
   
       Events observed by Relay during transfer
                              │
                              ▼
                       Relay/Aggregator
                              │
                    (Collects both types)
```

**Success Receipt** (dual-signed):
- Contains: file_hash, chunks, bytes, latency, timestamp, peer_ids
- Signatures: downloader + seeder (secp256k1)
- Purpose: Proof of successful transfer for positive reputation

**Nonpayment/Abort** (seeder-signed, binds to downloader's session_request):
- Contains: session_request_sig (from downloader), delivered vs requested bytes, reason
- Signatures: seeder (secp256k1)
- Purpose: Proof of payment failure for negative reputation
- **Anti-fabrication**: Seeder must reference downloader's signed session request

**Relay Co-signature** (optional): Relay adds third signature (secp256k1) for attestation

### 4. Snapshot Process

**Event Collection (Relay-only uploads):**
- Only relay servers upload snapshots
- Included events: transfers relayed by the server (success receipts, nonpayment/abort)
- Excluded events: direct transfers not relayed by the server

**Per Epoch** (default 6h):
1. Relay collects signed events from transfers it relayed
2. Validates signatures (secp256k1), deduplicates, filters malformed
3. Computes Merkle tree → `merkle_root`
4. Stores full payload off-chain or serves via HTTP
5. Publishes metadata to DHT: `{epoch_id, merkle_root, aggregator_sig}`
6. Anchors `merkle_root` on-chain with stake (required; subject to challenge period)

### 5. Aggregator Economics

**Revenue Model:**
- **Transaction fees**: Small percentage of file transfer payments auto-collected
- **Block rewards**: Portion of mining rewards allocated to aggregator pool
- Revenue distributed among active aggregators who anchor snapshots

**Cost Structure:**
- Stake deposit (refundable if honest)
- Gas fees for on-chain anchoring (L2 preferred for cost efficiency)
- Infrastructure (server, bandwidth, storage)

**Economic Incentive:**
- Revenue from fees and block rewards exceeds operational costs
- Makes aggregation profitable for relay nodes with existing infrastructure
- Relay nodes already incur most costs (24/7 server operation)

**Cold Start Consideration:**
- Early network may require bootstrapping mechanisms
- Initial relays may operate at subsidy until transaction volume grows
- Progressive transition to self-sustaining economics as network scales

**Who Uploads Snapshots?**
- **Relay nodes only** (acting as aggregators)
  - Upload snapshots for transfers they relayed
  - Must stake tokens to anchor on-chain and earn rewards
- **Regular nodes**
  - Do not upload snapshots
  - Only sign their own transfer events (used by relays in snapshots)

### 6. Staking and Challenge System

```
CHALLENGE FLOW:

Aggregator                     Blockchain                    Victim/Challenger
    │                              │                              │
    │ 1. Anchor snapshot           │                              │
    │    + Stake deposit           │                              │
    ├─────────────────────────────>│                              │
    │                              │                              │
    │        Challenge Period Starts                              │
    │                              │                              │
    │                              │  2. Monitor snapshots        │
    │                              │<─────────────────────────────┤
    │                              │                              │
    │                              │  3. Find false event!        │
    │                              │     (nonpayment claim)       │
    │                              │                              │
    │                              │  4. Submit challenge:        │
    │                              │     • Merkle proof           │
    │                              │     • Payment tx hash        │
    │                              │<─────────────────────────────┤
    │                              │                              │
    │                              │  5. Smart contract verifies: │
    │                              │     • Event in Merkle tree?  │
    │                              │     • Payment exists?        │
    │                              │     ✅ YES → Challenge valid │
    │                              │                              │
    │  ❌ SLASHED                  │                              │
    │  • Lose stake                │                              │
    │  • Reputation destroyed      │   6. Distribute stake:       │
    │                              │      • Majority burned       │
    │                              │      • Portion → Victim ────>│ ✅ Reward
    │                              │                              │
    │  Future revenue lost:        │                              │
    │  (permanent ban)             │                              │
    ▼                              ▼                              ▼

IF NO CHALLENGE after period:
    Aggregator withdraws stake + receives pool rewards
```

**Challenge Types:**
1. **INVALID_SIGNATURE**: Event signature fails verification
2. **FABRICATED_SESSION**: Nonpayment event without session request binding
3. **DUPLICATE_EVENT**: Same event included multiple times
4. **EXCLUDED_EVENT**: Valid event deliberately omitted

**Economic Security:**
- Attack cost: Stake loss + permanent revenue loss (banned from aggregating)
- Attack gain: Minimal (temporary reputation damage to competitor)
- Result: Economically irrational (cost >> gain)
- Honest behavior incentivized through continued revenue stream

### 7. Multi-Signature Quorum (Optional)

For higher trust, M-of-N aggregators co-sign snapshots off-chain before anchoring. Benefits:
- Multiple independent validators → higher confidence
- Progressive slashing (only dishonest signers slashed if challenged)
- Clients weight quorum snapshots higher

### 8. Client Consumption

1. Discover snapshots via DHT (require on-chain anchor)
2. Fetch payload by CID, verify signatures and Merkle root; confirm on-chain anchor matches
3. Sample-verify events (5-10%), full verification on disputes
4. Weight snapshots by: aggregator reputation, stake, quorum, historical accuracy
5. Fold events into local reputation: success → +rep, nonpayment → -rep
6. Conflict resolution: prefer later epoch, stronger quorum, higher stake

### 9. Design Considerations

**Privacy:**
- No IP addresses, only peer IDs and file hashes
- No raw chunk data, only indices/counts
- Optional session ID salting per epoch

**Scalability:**
- Off-chain storage (DHT + IPFS-like)
- ≤1 tx/aggregator/epoch, prefer L2
- Parallel aggregators, client-side weighting
- Sample verification (5-10%), full only on disputes

**Failure Handling:**
- Chain down → temporarily defer publishing (relay queues snapshot until L2 available)
- Aggregator Byzantine → downweight/ignore, use others
- CID unavailable → skip snapshot, prefer mirrors

**Defaults:**
- Epoch: 6h | Hash: SHA-256 | Sig: secp256k1
- Anchor: L2-first (required) | Challenge period: configurable

**Phase 0**: DHT + signed events only, no economics
**Phase 1**: Add Merkle snapshots + transaction fee mechanism
**Phase 2**: On-chain anchors + staking + challenges + block rewards
**Phase 3**: Multi-sig quorum, dynamic parameters, optimizations

### 11. Summary

**Key Advantages:**
- **Scalability**: Off-chain first (DHT), on-chain only for anchors (≤1 tx/epoch)
- **Security**: Staking + challenges make attacks costly (stake loss + permanent revenue loss)
- **Sustainability**: Transaction fees + block rewards fund aggregators
- **Decentralization**: Multiple aggregators, client-side weighting, no single authority
- **User Defense**: Victims can challenge false events with cryptographic proof and receive reward