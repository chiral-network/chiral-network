
# IMPORTANT: This document needs full revision. If we decided go through only public protocols (http, ftp, webtorrent, etc), there might be no needs to do NAT traversal ourselves. 

# NAT Traversal & Network Reachability

Chiral Network implements comprehensive NAT traversal solutions to ensure connectivity between peers regardless of network configuration.

## Understanding NAT Traversal Protocols

### AutoNAT v2

**AutoNAT v2** (Reachability Detection):
- **Purpose**: Detects if your node is behind NAT (Public/Private/Unknown status)
- **How it works**: Other peers try to dial you back directly on your observed addresses
- **Security**: Cannot use relay connections for dial-back (libp2p security requirement)
- **Built-in libp2p protocol**: Enable by configuring AutoNAT behavior in NetworkBehaviour

**Key Takeaway**: AutoNAT v2 is a core libp2p feature - no third-party services needed.

## Implementation Progress

### ✅ Phase 1: NAT Traversal Infrastructure (Completed)
- AutoNAT v2 for reachability detection
- UPnP automatic port forwarding
- mDNS for local peer discovery

### 🔄 Phase 2: Content-Based Discovery (Next Step)
- **Goal**: Automatic peer discovery by file hash using DHT
- **Implementation Needed**:
  - When sharing a file: `put_record("file:SHA256_HASH", peer_address)`
  - When searching for a file: `get_record("file:SHA256_HASH")` returns peer addresses
- **Result**: Fully decentralized P2P file sharing - no manual address exchange needed

### 📋 Phase 3: Optimization (Future)
- WebRTC direct connections for browser-compatible transfers
- SOCKS5 proxy integration for privacy

## Current Implementation Status

### ✅ Implemented Features

#### 1. AutoNAT v2 Reachability Detection
- Automatic 30-second probe cycles
- Real-time reachability status (Public/Private/Unknown)
- Confidence scoring for reachability state
- Reachability history tracking
- Headless CLI support: `--disable-autonat`, `--autonat-probe-interval`, `--autonat-server`

#### 2. Observed Address Tracking
- libp2p identify protocol integration
- Persistent tracking of externally observed addresses
- Address change detection and logging


### ✅ GUI Configuration 

#### 1. Settings UI for NAT Traversal
- AutoNAT toggle with configurable probe interval (10-300s)
- Custom AutoNAT servers textarea (multiaddr format)
- All settings persist to localStorage

#### 2. Real-Time Reachability Display
- Live NAT status badge (Public/Private/Unknown)
- Confidence scoring display (High/Medium/Low)
- Observed addresses from libp2p identify
- Reachability history table with timestamps
- Last probe time and state change tracking
- AutoNAT enabled/disabled indicator

## Headless Mode NAT Configuration

### Command-Line Options

```bash
# Enable AutoNAT with custom probe interval
./chiral-network --autonat-probe-interval 60

# Disable AutoNAT
./chiral-network --disable-autonat

# Add custom AutoNAT servers
./chiral-network --autonat-server /ip4/1.2.3.4/tcp/4001/p2p/QmPeerId

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
- Peers behind restrictive NATs can use SOCKS5 proxy for connectivity

**Connection Priority**:
```
1. Try UPnP → Direct connection if successful
2. If failed → SOCKS5 proxy (if configured)
```

## See Also

- [Network Protocol](network-protocol.md) - P2P networking details
- [Security & Privacy](security-privacy.md) - Privacy features
- [Deployment Guide](deployment-guide.md) - Production setup
- [AutoNAT v2 Package](https://www.npmjs.com/package/@libp2p/autonat-v2/v/1.0.0-5ed83dd69) - libp2p AutoNAT v2 specification
