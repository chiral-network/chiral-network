
# IMPORTANT: This document needs full revision. If we decided go through only public protocols (http, ftp, webtorrent, etc), there might be no needs to do NAT traversal ourselves. 

# NAT Traversal & Network Reachability

Chiral Network implements comprehensive NAT traversal solutions to ensure connectivity between peers regardless of network configuration.

## Understanding NAT Traversal Protocols

### AutoNAT v2 vs Circuit Relay v2

These are two **independent** libp2p protocols serving different purposes:

**AutoNAT v2** (Reachability Detection):
- **Does NOT require relay** - completely independent protocol
- **Purpose**: Detects if your node is behind NAT (Public/Private/Unknown status)
- **How it works**: Other peers try to dial you back directly on your observed addresses
- **Security**: Cannot use relay connections for dial-back (libp2p security requirement)
- **Built-in libp2p protocol**: Enable by configuring AutoNAT behavior in NetworkBehaviour

**Circuit Relay v2** (NAT Traversal):
- **Requires publicly reachable relay nodes**
- **Purpose**: Forwards traffic between NAT'd peers who cannot connect directly
- **How it works**:
  - NAT'd peer (A) requests reservation with relay (R)
  - Relay R listens for incoming connections on A's behalf
  - When peer B wants to connect to A, B connects to relay R, which relays traffic
- **End-to-end encrypted**: Relay cannot read or tamper with traffic (inspired by TURN protocol)
- **Built-in libp2p protocol**: Enable by configuring relay behavior in NetworkBehaviour
- **Decentralized**: Any public node can opt-in to relay mode

**Key Takeaway**: Both are core libp2p features - no third-party services needed. The relay is simply running libp2p relay protocol, not a centralized service.

## Implementation Progress

### ✅ Phase 1: NAT Traversal Infrastructure (Completed)
- Circuit Relay v2 implemented and tested across different networks
- AutoNAT v2 for reachability detection
- Relay server mode (any public node can act as relay)
- Cross-network connectivity verified through relay circuits

**Current State**: Peers can communicate through relay, but requires **manual multiaddress sharing** (peer-ID discovery).

### 🔄 Phase 2: Content-Based Discovery (Next Step)
- **Goal**: Automatic peer discovery by file hash using DHT
- **Implementation Needed**:
  - When sharing a file: `put_record("file:SHA256_HASH", peer_relay_circuit_address)`
  - When searching for a file: `get_record("file:SHA256_HASH")` returns peer addresses
  - NAT'd peers publish their relay circuit addresses to DHT
- **Result**: Fully decentralized P2P file sharing - no manual address exchange needed

### 📋 Phase 3: Optimization (Future)
- WebRTC direct connections after initial relay handshake
- DCUtR (Direct Connection Upgrade through Relay) for hole punching
- Relay as fallback only, not primary transfer method

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


### ✅ GUI Configuration 

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
- Uses bandwidth when actively relaying connections
- Can be disabled at any time without affecting relay client functionality
- Disabled by default (users must opt-in)

### ✅ Public Relay Infrastructure

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

#### Automatic Port Forwarding (UPnP)
Modern routers support automatic port forwarding protocols that enable NAT'd peers to become publicly reachable without manual configuration:

- **UPnP (Universal Plug and Play)**: Industry-standard protocol for automatic port mapping
  - **Built-in libp2p feature**: libp2p provides core UPnP functionality
  - Discovers IGD (Internet Gateway Device) on the local network via SSDP multicast
  - Requests external port mappings through SOAP/XML API
  - Router exposes internal service on its public IP address
  - Widely supported on consumer routers (check router settings: "UPnP" or "UPnP IGD")

**Benefits**:
- Transforms NAT'd nodes into publicly reachable peers automatically
- Eliminates need for manual port forwarding configuration
- Improves network performance by enabling direct P2P connections
- Reduces relay bandwidth usage

**Fallback Strategy**:
- If UPnP fails (unsupported router, disabled, or restrictive firewall)
- Automatically proceeds to Hole Punching (DCUtR)
- Circuit Relay used as final fallback

**Connection Priority**:
```
1. Try UPnP → Direct connection if successful
2. If failed → Hole Punching (DCUtR)
3. If failed → Circuit Relay
```

### 2. Hole Punching (DCUtR)
For symmetric NAT traversal using Direct Connection Upgrade through Relay protocol.

### 3. Circuit Relay (fallback)
For restrictive NATs where hole punching fails. Connections are relayed through trusted relay nodes.



## Deploying Your Own Relay Node

For testing NAT traversal or contributing relay infrastructure to the network, you can deploy your own relay node:

### Quick Start

The relay daemon (`chiral-relay`) is located in `relay/` directory. Basic usage:

```bash
cd relay
cargo build --release
./target/release/chiral-relay --port 4001 --external-address /ip4/YOUR_PUBLIC_IP/tcp/4001
```

### Cloud Deployment

For production relay deployment on cloud providers (Google Cloud, AWS, DigitalOcean, etc.), see:

**[relay/DEPLOYMENT.md](../relay/DEPLOYMENT.md)** - Complete deployment guide including:
- Cloud provider setup (GCP, AWS, DigitalOcean)
- Firewall configuration
- systemd service setup
- Docker/docker-compose configs
- Monitoring and metrics

### Configure Relay in Main App

After deploying a relay, configure it in your Chiral Network app:

1. Copy the relay's full multiaddr: `/ip4/YOUR_IP/tcp/4001/p2p/12D3KooW...`
2. Go to **Settings** → **Network Settings**
3. Paste multiaddr into **Preferred Relay Nodes**
4. Enable **AutoRelay** toggle
5. Save and restart DHT

## See Also

- [Network Protocol](network-protocol.md) - P2P networking details
- [Security & Privacy](security-privacy.md) - Privacy features
- [Deployment Guide](deployment-guide.md) - Production setup
- [relay/DEPLOYMENT.md](../relay/DEPLOYMENT.md) - Advanced relay deployment
- [AutoNAT v2 Package](https://www.npmjs.com/package/@libp2p/autonat-v2/v/1.0.0-5ed83dd69) - libp2p AutoNAT v2 specification
- [Circuit Relay Documentation](https://docs.libp2p.io/concepts/nat/circuit-relay/) - libp2p Circuit Relay protocol
