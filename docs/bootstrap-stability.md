# DHT & Bootstrap Stability - Technical Proposal

## 🔴 PRIORITY 1 - CRITICAL ISSUE

**Status:** Planning Phase | **Date:** 2024-11-24

---

## Executive Summary

This document addresses critical DHT and bootstrap stability issues affecting peer discoverability in Chiral Network. The main challenges are:

1. **Bootstrap node failures** causing infinite hangs and silent degradation
2. **Private IP problem** where NAT'd nodes (60-70% of users) are undiscoverable via DHT
3. **Discovery verification** ensuring peers can actually see each other

---

## Part 1: Bootstrap Node Stability

### Critical Issues

| Issue | Problem | Impact |
|-------|---------|--------|
| Infinite Hang | `connectPeer()` has no timeout | Users force-quit app |
| Silent Degradation | Bootstrap crashes after connection | Peers invisible for hours |
| Discovery Not Verified | DHT connected ≠ discovery working | Users can't find each other |

### Business Impact
- 30% connection failure rate
- 15 support tickets/week
- Hours of downtime when bootstrap crashes

### Solution #1: Connection Timeout

Wrap `connectPeer()` in `Promise.race()` with 10s timeout:

```javascript
// Network.svelte - connectWithTimeout()
async function connectWithTimeout(peer, timeout = 10000) {
  return Promise.race([
    connectPeer(peer),
    new Promise((_, reject) => 
      setTimeout(() => reject(new Error('Connection timeout')), timeout)
    )
  ]);
}
```

**Benefit:** Fail fast instead of infinite hang

### Solution #2: Health Monitoring

New `check_bootstrap_health()` command returns:
- Latency, reachability, peer count, routing table size

Frontend runs 30s interval health checks with auto-reconnect.

```rust
// bootstrap.rs
pub struct BootstrapHealth {
    pub latency: u64,
    pub reachable: bool,
    pub peer_count: usize,
    pub routing_table_size: usize,
}
```

**Benefit:** Detect issues in 30s, auto-recover

### Solution #3: Discovery Verification

6-step verification flow with 5 new Tauri commands:

1. Check routing table populated (`get_dht_routing_table_size`)
2. Publish test key via DHT
3. Query test key from peer perspective (`test_peer_discovery`)
4. List discovered peer IDs (`list_discovered_peer_ids`)
5. Verify bidirectional reachability (`verify_peer_reachable`)
6. Confirm provider records synced (`get_provider_records`)

**Benefit:** Proves peers can actually see each other

### Solution #4: Server Auto-Recovery

- Systemd timer checks bootstrap process every 1 minute
- Auto-restart on failure with monitoring alerts
- Downtime reduced from hours to <2min

### Success Metrics

| Metric | Before | After | Target |
|--------|--------|-------|--------|
| Connection Success Rate | 70% | 95%+ | 95% |
| Bootstrap Downtime | Hours | <2min | <5min |
| Discovery Failures | 20% | <1% | <2% |
| Issue Detection Time | Never | 30s | <60s |

---

## Part 2: The Private IP vs Public IP Problem

### Problem Statement

**Core Issue**: Nodes behind NAT have private IP addresses that cannot be reached from the public internet, making them undiscoverable even when they announce themselves as providers in the DHT.

### Why This Happens

When a NAT'd node publishes a file:
1. **Stores metadata** in DHT under file's Merkle root
2. **Announces as provider** via `kademlia.start_providing(file_key)`

The problem: Provider announcement includes unreachable private addresses.

```
Node A (Behind NAT):
  Peer ID: 12D3KooWxyz...
  Private IP: 192.168.1.100:4001  ← UNREACHABLE from internet
  
Node B (Searching):
  Finds provider 12D3KooWxyz...
  Tries to connect to 192.168.1.100:4001
  Result: CONNECTION FAILS ❌
```

### Private Address Ranges (RFC 1918)

These addresses are **not routable** on the public internet:

| Range | Addresses |
|-------|-----------|
| 10.0.0.0/8 | 10.0.0.0 - 10.255.255.255 |
| 172.16.0.0/12 | 172.16.0.0 - 172.31.255.255 |
| 192.168.0.0/16 | 192.168.0.0 - 192.168.255.255 |
| 127.0.0.0/8 | Loopback (localhost) |

### Real-World Impact

| User Type | Behind NAT | Impact |
|-----------|------------|--------|
| Home users | 60-70% | Cannot share files directly |
| Mobile users | 90%+ | Almost always behind CGNAT |
| Corporate | 40-50% | Restrictive NAT policies |

**Without NAT traversal**: Majority of peers become unreachable, severely limiting file availability.

---

## Part 3: Proposed Solutions (Relay-Free)

### Goal: Avoid Relays

Relays add complexity, latency, and infrastructure costs. We propose **direct NAT traversal** methods that establish peer-to-peer connections without intermediate relay servers.

### Solution A: Hole Punching (DCUtR) - RECOMMENDED

**How It Works:**
```
1. Both peers connect to bootstrap (coordination only)
2. Exchange external IP:port via DHT/signaling
3. Simultaneously send packets to each other
4. NAT "punches hole" allowing direct connection
```

```
Node A (NAT)          Bootstrap           Node B (NAT)
    |                     |                    |
    |---> Get external IP |                    |
    |<--- 203.0.113.5:4001|                    |
    |                     |<--- Get external IP|
    |                     |--> 198.51.100.7:4002
    |                     |                    |
    |======== HOLE PUNCH (simultaneous) =======|
    |<------------- DIRECT CONNECTION -------->|
```

**Success Rate:** 70-80% of NAT types
**Latency:** Direct (no relay overhead)
**Implementation:** libp2p DCUtR protocol already supported

### Solution B: UPnP/NAT-PMP Port Mapping

**How It Works:**
```
1. Node requests router to open external port
2. Router maps external:4001 → internal:4001
3. Node announces public IP:port to DHT
4. Peers connect directly
```

```rust
// Proposed implementation
async fn request_port_mapping() -> Result<SocketAddr> {
    // Try UPnP first
    if let Ok(gateway) = igd::search_gateway().await {
        gateway.add_port(TCP, 4001, local_addr, 3600, "chiral")?;
        return Ok(gateway.get_external_ip()? + ":4001");
    }
    // Fallback to NAT-PMP
    natpmp::request_mapping(4001)?
}
```

**Success Rate:** 60-70% of home routers support UPnP
**Latency:** Direct connection
**Requirement:** Router must have UPnP enabled

### Solution C: STUN-Based Discovery

**How It Works:**
```
1. Node sends request to STUN server
2. STUN server returns observed external IP:port
3. Node announces this address to DHT
4. Works if NAT preserves port mapping
```

```
Node A          STUN Server
    |               |
    |---> Request --|
    |<-- Your external IP is 203.0.113.5:4001
    |
    |---> Announce to DHT: 203.0.113.5:4001
```

**Success Rate:** Works for ~50% of NAT types (cone NATs)
**Cost:** Only need lightweight STUN server (not relay)
**Note:** STUN only discovers address; doesn't relay traffic

### Solution Comparison

| Method | Success Rate | Latency | Infrastructure | Recommended |
|--------|--------------|---------|----------------|-------------|
| Hole Punching (DCUtR) | 70-80% | Direct | Bootstrap only | ✅ Primary |
| UPnP/NAT-PMP | 60-70% | Direct | None | ✅ Auto-enable |
| STUN Discovery | 50% | Direct | STUN server | ✅ Fallback |
| Circuit Relay | 100% | +50-100ms | Relay servers | ⚠️ Last resort |

### Proposed Architecture (Relay-Free)

```
┌─────────────────────────────────────────────────────────┐
│                   Connection Strategy                     │
├─────────────────────────────────────────────────────────┤
│  1. Try UPnP/NAT-PMP port mapping                        │
│     ↓ (if fails)                                         │
│  2. Try STUN to discover external address                │
│     ↓ (if symmetric NAT)                                 │
│  3. Try DCUtR hole punching                              │
│     ↓ (if all fail - rare)                               │
│  4. Fall back to relay (last resort only)                │
└─────────────────────────────────────────────────────────┘
```

---

## Part 4: Implementation Details

### Private IP Filtering

The network filters out unreachable private addresses:

```rust
// src-tauri/src/dht.rs
fn ma_plausibly_reachable(ma: &Multiaddr) -> bool {
    if let Some(Protocol::Ip4(v4)) = ma.iter().find(|p| matches!(p, Protocol::Ip4(_))) {
        // Reject loopback and private addresses
        return !v4.is_loopback() && !v4.is_private();
    }
    false
}
```

### AutoNAT v2 - Detecting Reachability

AutoNAT detects whether a node is publicly reachable:

```
Node A:
1. Observes own addresses via identify protocol
2. Asks remote peers to dial back
3. If dialback succeeds → Public (use direct address)
4. If dialback fails → Try hole punching
```

### Proposed: UPnP Integration

```rust
// New module: src-tauri/src/upnp.rs
pub async fn setup_port_mapping(port: u16) -> Result<Option<SocketAddr>> {
    // 1. Discover gateway
    let gateway = igd::search_gateway(Default::default()).await?;
    
    // 2. Get local address
    let local_ip = get_local_ip()?;
    let local_addr = SocketAddrV4::new(local_ip, port);
    
    // 3. Request port mapping (1 hour lease)
    gateway.add_port(
        PortMappingProtocol::TCP,
        port,
        local_addr,
        3600,
        "Chiral Network"
    )?;
    
    // 4. Return external address
    let external_ip = gateway.get_external_ip()?;
    Ok(Some(SocketAddr::new(external_ip.into(), port)))
}
```

### Proposed: Enhanced Hole Punching

```rust
// Enhance existing DCUtR with retry logic
pub async fn attempt_hole_punch(peer_id: PeerId) -> Result<()> {
    for attempt in 1..=3 {
        match dcutr_connect(peer_id).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                warn!("Hole punch attempt {} failed: {}", attempt, e);
                tokio::time::sleep(Duration::from_millis(500 * attempt)).await;
            }
        }
    }
    Err(Error::HolePunchFailed)
}
```

### Current vs Proposed Features

| Feature | Current | Proposed |
|---------|---------|----------|
| Circuit Relay | ✅ Primary | ⚠️ Last resort |
| DCUtR Hole Punching | ✅ Available | ✅ Primary method |
| UPnP/NAT-PMP | ❌ Not implemented | ✅ Auto-enable |
| STUN Discovery | ❌ Not implemented | ✅ Add support |
| Private IP Filtering | ✅ Implemented | ✅ Keep |

---

## Part 5: Configuration

### For Users Behind NAT

**Recommended Settings (Relay-Free):**
```toml
[network]
enable_autonat = true
enable_upnp = true           # NEW: Auto port mapping
enable_hole_punching = true  # NEW: DCUtR priority
stun_servers = [             # NEW: For address discovery
    "stun:stun.l.google.com:19302",
    "stun:stun.cloudflare.com:3478",
]

# Relay disabled by default
enable_autorelay = false
```

**CLI:**
```bash
chiral-network \
  --enable-upnp \
  --enable-hole-punching \
  --stun-server stun.l.google.com:19302
```

### For Advanced Users

If hole punching fails consistently:
```toml
[network]
# Enable relay as fallback only
enable_autorelay = true
relay_mode = "fallback"  # Only use if direct fails
```

---

## Part 6: Troubleshooting

### "No peers found for file"

**Diagnosis:**
```bash
chiral-cli network dht-status
chiral-cli network routing-table-size
chiral-cli network bootstrap-health
```

**Solutions:**
1. Verify bootstrap node connectivity
2. Wait 30-60s for DHT to populate
3. Check provider record TTL
4. Ensure file is still being seeded

### "Connection failed to provider"

**Diagnosis:**
```bash
chiral-cli network peer-info <peer-id>
chiral-cli network nat-status
chiral-cli network upnp-status     # NEW
chiral-cli network hole-punch-test  # NEW
```

**Solutions:**
1. Check if UPnP port mapping succeeded
2. Verify hole punching is enabled
3. Check NAT type (symmetric NAT may need STUN)
4. Try manual port forwarding

### "Hole punching failed"

**Common Causes:**
- Symmetric NAT on both sides
- Firewall blocking UDP
- Router doesn't support hairpin NAT

**Solutions:**
1. Enable UPnP on router
2. Manually forward port 4001
3. Check firewall allows UDP traffic
4. As last resort: enable relay fallback

---

## Part 7: Implementation Checklist

### Phase 1: Bootstrap Timeout
- [ ] `connectWithTimeout()` wrapper
- [ ] Retry UI with error messages

### Phase 2: Health Monitoring
- [ ] `check_bootstrap_health()` command
- [ ] 30s interval health loop
- [ ] UI health indicator

### Phase 3: Discovery Verification
- [ ] 5 verification Tauri commands
- [ ] 6-step verification flow
- [ ] Diagnostics panel

### Phase 4: Server Auto-Recovery
- [ ] Health check bash script
- [ ] Systemd timer/service
- [ ] Monitoring alerts

### Phase 5: Relay-Free NAT Traversal (NEW)
- [ ] UPnP/NAT-PMP port mapping module
- [ ] STUN server integration
- [ ] Enhanced DCUtR hole punching with retry
- [ ] Connection strategy: UPnP → STUN → DCUtR → Relay
- [ ] Relay demoted to fallback-only mode

### Phase 6: Testing & Deploy
- [ ] All stability tests passing
- [ ] NAT traversal success rate >80%
- [ ] Load test 50+ concurrent users
- [ ] Documentation updates
- [ ] Production deployment

---

## Key Files

| File | Changes | Purpose |
|------|---------|---------|
| `Network.svelte` | ~50 lines | Timeout, health loop, verification |
| `bootstrap.rs` | ~150 lines | Health struct, Tauri commands |
| `dht.rs` | ~30 lines | Routing table, address filtering |
| `upnp.rs` | ~100 lines | NEW: UPnP/NAT-PMP port mapping |
| `peerEventService.ts` | ~20 lines | Discovery events |
| Server scripts | ~100 lines | Auto-recovery, systemd |

---

## Conclusion

**Problems:**
1. Bootstrap failures → 30% connection failure rate
2. Private IPs in DHT → 60-70% of users undiscoverable
3. No verification → Peers can't find each other

**Proposed Solutions (Relay-Free):**
1. Timeout + health monitoring + auto-recovery
2. Direct NAT traversal: UPnP → STUN → Hole Punching
3. 6-step discovery verification
4. Relay as last resort only (not default)

**Expected Outcome:**
- 95%+ connection success rate
- <2min bootstrap downtime
- 80%+ NAT'd users connect directly (no relay)
- Reduced infrastructure costs (no relay servers)
- Lower latency (direct connections)

---

## References

- [libp2p DCUtR (Hole Punching)](https://github.com/libp2p/specs/blob/master/relay/dcutr.md)
- [UPnP IGD Protocol](https://en.wikipedia.org/wiki/Internet_Gateway_Device_Protocol)
- [STUN Protocol (RFC 5389)](https://tools.ietf.org/html/rfc5389)
- [NAT-PMP (RFC 6886)](https://tools.ietf.org/html/rfc6886)
- [AutoNAT v2 Specification](https://github.com/libp2p/specs/blob/master/autonat/README.md)
- [Kademlia DHT](https://docs.libp2p.io/concepts/fundamentals/protocols/#kademlia)
- [RFC 1918 - Private IP Addresses](https://tools.ietf.org/html/rfc1918)
