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
