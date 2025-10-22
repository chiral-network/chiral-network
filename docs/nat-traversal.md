
# IMPORTANT: This document needs full revision. If we decided go through only public protocols (http, ftp, webtorrent, etc), there might be no needs to do NAT traversal ourselves. 

# NAT Traversal & Network Reachability

Chiral Network implements comprehensive NAT traversal solutions to ensure connectivity between peers regardless of network configuration.

## Current Implementation Status

### ✅ Implemented Features

#### 1. AutoNAT v2 Reachability Detection
- Automatic 30-second probe cycles
- Real-time reachability status (Public/Private/Unknown)
- Confidence scoring for reachability state
- Reachability history tracking
- Headless CLI support: `--disable-autonat`, `--autonat-probe-interval`, `--autonat-server`

#### 2. Circuit Relay v2 with AutoRelay
- Automatic relay candidate detection from bootstrap nodes
- Dynamic relay reservation for NAT'd peers
- Relay health monitoring and connection tracking
- Headless CLI support: `--enable-autorelay`, `--disable-autorelay`, `--relay <multiaddr>`
- Configurable preferred relay nodes (GUI + CLI)

#### 3. Observed Address Tracking
- libp2p identify protocol integration
- Persistent tracking of externally observed addresses
- Address change detection and logging

#### 4. SOCKS5 Proxy Integration
- P2P traffic routing through SOCKS5 proxies
- CLI flag: `--socks5-proxy <address>`

### ✅ GUI Configuration (Recently Implemented)

#### 1. Settings UI for NAT Traversal
- AutoNAT toggle with configurable probe interval (10-300s)
- Custom AutoNAT servers textarea (multiaddr format)
- AutoRelay toggle for Circuit Relay v2
- Preferred relay nodes textarea (multiaddr format)
- **Relay Server toggle** - Enable your node to act as a relay server for other peers
- All settings persist to localStorage

#### 2. Real-Time Reachability Display
- Live NAT status badge (Public/Private/Unknown)
- Confidence scoring display (High/Medium/Low)
- Observed addresses from libp2p identify
- Reachability history table with timestamps
- Last probe time and state change tracking
- AutoNAT enabled/disabled indicator

#### 3. Relay Server Mode (User-Configurable)
- Simple checkbox toggle in Settings → Network Settings
- Enable your node to accept relay reservations from NAT'd peers
- Help establish circuits for peers behind restrictive NATs
- Earn reputation points for your node
- Uses bandwidth when actively relaying connections
- Can be disabled at any time without affecting relay client functionality
- Disabled by default (users must opt-in)

### ✅ Public Relay Infrastructure (Recently Implemented)

#### 1. Dedicated Circuit Relay v2 Daemon
- Standalone relay node binary (`chiral-relay`)
- Configurable reservation/circuit limits
- Persistent peer identity across restarts
- JSON metrics export for monitoring
- Production-ready with systemd/Docker support
- Location: `relay/`

#### 2. Deployment Scripts
- `start-relay.sh` - Bootstrap script with auto IP detection
- `stop-relay.sh` - Graceful shutdown with fallback force kill
- `status-relay.sh` - Comprehensive status and metrics display
- Environment variable configuration
- PID file management

#### 3. Documentation
- `relay/README.md` - Quick start guide
- `relay/DEPLOYMENT.md` - Production deployment
- systemd service examples
- Docker/docker-compose configs
- Cloud deployment guides (AWS, GCP, DigitalOcean)
- Prometheus metrics integration

## ❌ Not Yet Implemented

1. **Advanced Security**
   - Relay reservation authentication
   - Rate limiting for AutoNAT probes
   - Anti-amplification safeguards

2. **Resilience Testing**
   - End-to-end NAT traversal scenarios
   - Private↔Public connection tests
   - Private↔Private relay/hole-punch tests
   - Failure fallback validation

## Headless Mode NAT Configuration

### Command-Line Options

```bash
# Enable AutoNAT with custom probe interval
./chiral-network --autonat-probe-interval 60

# Disable AutoNAT
./chiral-network --disable-autonat

# Add custom AutoNAT servers
./chiral-network --autonat-server /ip4/1.2.3.4/tcp/4001/p2p/QmPeerId

# Enable AutoRelay with custom relay nodes
./chiral-network --relay /ip4/relay.example.com/tcp/4001/p2p/QmRelayId

# Route P2P through SOCKS5 proxy
./chiral-network --socks5-proxy 127.0.0.1:9050
```

## NAT Traversal Architecture

The network uses a multi-layered approach to ensure connectivity:

### 1. Direct Connection (fastest)
For publicly reachable peers with no NAT or firewall restrictions.

### 2. Hole Punching (DCUtR)
For symmetric NAT traversal using Direct Connection Upgrade through Relay protocol.

### 3. Circuit Relay (fallback)
For restrictive NATs where hole punching fails. Connections are relayed through trusted relay nodes.

### 4. SOCKS5 Proxy (privacy)
For anonymous routing when privacy is the primary concern.

## Configuration Best Practices

### For Public Nodes
- Enable Relay Server mode to help the network
- Keep AutoNAT enabled for reachability monitoring
- Configure port forwarding if possible for better performance

### For NAT'd Nodes
- Enable AutoRelay for automatic relay discovery
- Add trusted relay nodes to preferred relays list
- Monitor reachability status regularly

### For Privacy-Focused Nodes
- Use SOCKS5 proxy (e.g., Tor)
- Enable anonymous mode in settings
- Use Circuit Relay v2 to hide IP addresses
- Disable AutoNAT or use custom AutoNAT servers

## Troubleshooting

### Peer Connection Issues
1. Check reachability status in Network page
2. Verify AutoNAT is enabled and probing
3. Ensure relay reservations are active
4. Check firewall and router settings

### Relay Server Not Working
1. Verify DHT is running
2. Check relay server toggle is enabled
3. Restart DHT after enabling relay mode
4. Monitor relay reputation in Reputation page

### SOCKS5 Proxy Issues
1. Verify proxy address and port
2. Test proxy connectivity separately
3. Check proxy supports P2P traffic
4. Review proxy authentication settings

## See Also

- [Network Protocol](network-protocol.md) - P2P networking details
- [Security & Privacy](security-privacy.md) - Privacy features
- [Deployment Guide](deployment-guide.md) - Production setup
