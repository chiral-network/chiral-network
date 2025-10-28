# NAT Traversal & Hole Punching Guide

Complete guide for configuring and using NAT traversal features in Chiral Network.

## Table of Contents

1. [Overview](#overview)
2. [NAT Traversal Technologies](#nat-traversal-technologies)
3. [Configuration](#configuration)
4. [Public IP Detection](#public-ip-detection)
5. [Port Forwarding Setup](#port-forwarding-setup)
6. [DCUtR Validation](#dcutr-validation)
7. [Testing](#testing)
8. [Troubleshooting](#troubleshooting)

## Overview

Chiral Network implements a comprehensive NAT traversal stack to enable peer-to-peer connections across different network topologies:

- **AutoNAT v2**: Automatic detection of network reachability
- **Circuit Relay v2**: NAT-to-NAT communication via relay nodes
- **DCUtR**: Direct Connection Upgrade through Relay (hole-punching)
- **mDNS**: Local network peer discovery

### Network Scenarios Supported

1. ✅ **Public ↔ Public**: Direct connections (optimal)
2. ✅ **Public ↔ Private**: Direct with AutoNAT assistance
3. ✅ **Private ↔ Private via Relay**: Circuit Relay for guaranteed connectivity
4. ✅ **Private ↔ Private via DCUtR**: Hole-punching for direct upgrade

## NAT Traversal Technologies

### 1. AutoNAT v2

AutoNAT automatically detects whether your node is publicly reachable or behind NAT.

**Features:**
- Periodic reachability probes
- Confidence scoring
- Multiple probe servers
- Adaptive probe intervals

**Metrics Tracked:**
- Reachability state (Public, Private, Unknown)
- Confidence level (High, Medium, Low)
- Probe success/failure rate
- Last probe timestamp

**CLI Options:**
```bash
# Enable AutoNAT (enabled by default)
cargo run -- --headless --dht-port 4001

# Disable AutoNAT
cargo run -- --headless --disable-autonat

# Custom probe interval (seconds)
cargo run -- --headless --autonat-probe-interval 60

# Specify AutoNAT servers
cargo run -- --headless --autonat-server /ip4/1.2.3.4/tcp/4001/p2p/QmBootstrap1
```

### 2. Circuit Relay v2

Circuit Relay enables NAT'd nodes to communicate via relay servers.

**How it Works:**
1. Private node connects to relay server
2. Creates reservation for incoming connections
3. Other nodes can reach it via relay address
4. Optionally upgrade to direct connection via DCUtR

**Relay Server Mode:**
```bash
# Run as relay server (public node)
cargo run -- --headless --enable-relay-server

# With custom alias
cargo run -- --headless --enable-relay-server --relay-alias "my-relay"
```

**Relay Client Mode:**
```bash
# Enable AutoRelay (automatic relay discovery)
cargo run -- --headless

# Disable AutoRelay
cargo run -- --headless --disable-autorelay

# Use specific relay nodes
cargo run -- --headless --relay /ip4/1.2.3.4/tcp/4001/p2p/QmRelay1
```

**Metrics Tracked:**
- Active relay peer ID
- Reservation status
- Reservation renewals/evictions
- Relay connection attempts/successes
- Relay health score

### 3. DCUtR (Hole-Punching)

DCUtR attempts to upgrade relayed connections to direct connections via hole-punching.

**How it Works:**
1. Two nodes communicate via relay
2. DCUtR coordinates simultaneous connection attempts
3. NAT devices create temporary port mappings
4. Direct connection established (if successful)

**Automatic Activation:**
- DCUtR is automatically enabled when AutoNAT is enabled
- Requires relay connection for coordination
- Attempts upgrade transparently

**Metrics Tracked:**
- Total hole-punch attempts
- Successes and failures
- Success rate
- Last success/failure timestamp

### 4. mDNS (Local Discovery)

mDNS enables automatic discovery of peers on the local network.

**Features:**
- Zero-configuration peer discovery
- Works without internet connectivity
- Automatic for local peers
- No configuration needed

## Configuration

### Environment Variables

#### CHIRAL_PUBLIC_IP

Set your public IP address for external advertisement:

```bash
# Detect and use your public IP
export CHIRAL_PUBLIC_IP=$(curl -s https://api.ipify.org)

# Or set manually
export CHIRAL_PUBLIC_IP="203.0.113.42"

# Run Chiral Network
cargo run
```

This is especially important for:
- Running relay servers
- Nodes with static public IPs
- Port-forwarded setups

### Programmatic Configuration

```rust
use chiral_network::dht::DhtService;
use std::time::Duration;

let service = DhtService::new(
    4001,                           // DHT port
    vec![],                         // Bootstrap nodes
    None,                           // Identity secret
    false,                          // Is bootstrap node
    true,                           // Enable AutoNAT
    Some(Duration::from_secs(30)),  // AutoNAT probe interval
    vec![],                         // AutoNAT servers
    None,                           // SOCKS5 proxy
    None,                           // File transfer service
    Some(256),                      // Chunk size KB
    Some(1024),                     // Cache size MB
    true,                           // Enable AutoRelay
    vec![],                         // Preferred relays
    false,                          // Enable relay server
    None,                           // Relay server alias
    None,                           // Blockstore path
).await?;
```

## Public IP Detection

Chiral Network can automatically detect your public IP address.

### Automatic Detection

```rust
use chiral_network::dht::detect_public_ip;

match detect_public_ip().await {
    Ok(ip) => println!("Public IP: {}", ip),
    Err(e) => println!("Detection failed: {}", e),
}
```

### Detection Services Used

The function tries multiple services in order:
1. https://api.ipify.org
2. https://icanhazip.com
3. https://ifconfig.me/ip
4. https://checkip.amazonaws.com

### Manual vs Automatic

**When to use automatic detection:**
- Dynamic IP addresses (residential/mobile)
- Testing and development
- Nodes without static configuration

**When to use manual configuration (CHIRAL_PUBLIC_IP):**
- Static IP addresses (servers/VPS)
- Port-forwarded setups
- Production relay servers
- Privacy concerns with external IP detection services

## Port Forwarding Setup

### Why Port Forward?

Port forwarding improves connectivity by:
- Making your node publicly reachable
- Eliminating dependency on relay servers
- Improving connection performance
- Enabling you to run a relay server

### Getting Configuration

```rust
let config = service.get_port_forwarding_config().await;

println!("Public IP: {:?}", config.public_ip);
println!("Local IP: {:?}", config.local_ip);
println!("Port: {:?}", config.primary_port);
println!("NAT Status: {}", config.nat_status);

for instruction in config.instructions {
    println!("{}", instruction);
}
```

### Manual Port Forwarding Steps

1. **Find your router's IP** (usually 192.168.1.1 or 192.168.0.1)

2. **Log into router admin panel**
   - Open browser to router IP
   - Enter admin credentials

3. **Navigate to Port Forwarding section**
   - May be called "Virtual Server", "Port Forwarding", or "NAT"

4. **Create port forwarding rule:**
   - **Service Name**: Chiral Network
   - **External Port**: 4001 (or your DHT port)
   - **Internal Port**: 4001 (same as external)
   - **Internal IP**: Your local machine IP
   - **Protocol**: TCP
   - **Enable**: Yes

5. **Save and apply settings**
   - Router may need restart

6. **Configure Chiral Network:**
   ```bash
   export CHIRAL_PUBLIC_IP="your.public.ip"
   cargo run
   ```

7. **Verify setup:**
   ```bash
   # Check from external network
   nc -zv your.public.ip 4001
   ```

### UPnP Alternative

Some routers support UPnP (Universal Plug and Play) for automatic port forwarding.

**Note**: UPnP is not currently implemented in Chiral Network but is planned for future releases.

## DCUtR Validation

### Checking DCUtR Status

```rust
let validation = service.validate_dcutr().await;

println!("Status: {}", validation.status);
println!("Success Rate: {:.2}%", validation.success_rate);
println!("Total Attempts: {}", validation.total_attempts);

for rec in validation.recommendations {
    println!("Recommendation: {}", rec);
}
```

### Status Values

- **`disabled`**: DCUtR is not enabled (enable AutoNAT)
- **`not_tested`**: No hole-punch attempts yet
- **`excellent`**: ≥70% success rate
- **`good`**: 40-70% success rate
- **`poor`**: 20-40% success rate
- **`failing`**: <20% success rate

### Improving DCUtR Success Rate

If you see poor DCUtR performance:

1. **Check Firewall Settings**
   - Allow UDP/TCP traffic on DHT port
   - Disable strict firewall rules

2. **Enable UPnP on Router**
   - Check router settings
   - Enable UPnP/NAT-PMP if available

3. **Set Up Port Forwarding**
   - See [Port Forwarding Setup](#port-forwarding-setup)
   - Eliminates need for hole-punching

4. **Check Network Type**
   - Some cellular/mobile networks use Carrier-Grade NAT
   - These may block hole-punching
   - Use relay servers in these cases

5. **Verify Relay Connectivity**
   - Ensure you're connected to at least one relay
   - Check `active_relay_peer_id` in metrics

## Testing

### Running Tests

```bash
# All NAT traversal tests
cargo test nat_traversal

# Specific test suites
cargo test test_autonat_detection
cargo test test_topology_a_all_public_nodes
cargo test test_topology_b_mixed_public_private
cargo test test_topology_c_all_private_nodes
cargo test test_dcutr_validation
cargo test test_public_ip_detection

# Run with logging
RUST_LOG=debug cargo test nat_traversal -- --nocapture
```

### Test Topologies

**Topology A: All Public Nodes**
- Tests direct peer-to-peer connections
- No NAT, relay, or hole-punching needed
- Optimal performance baseline

**Topology B: Mixed Public/Private**
- Public relay server + private clients
- Tests relay functionality
- Tests DCUtR hole-punching

**Topology C: All Private Nodes**
- Worst-case scenario
- All nodes behind NAT
- Tests relay-only communication

**Topology D: Multi-Hop Relay**
- Chain of relay servers
- Tests relay mesh network
- Tests long-path connectivity

### Manual Testing

#### Test AutoNAT Detection

```bash
# Start node and check reachability
cargo run -- --headless --show-reachability

# Expected output:
# Reachability: Public/Private/Unknown
# Confidence: High/Medium/Low
# Observed addresses: [...]
```

#### Test Relay Connectivity

```bash
# Terminal 1: Start relay server
cargo run -- --headless --enable-relay-server --dht-port 4001

# Terminal 2: Start client
cargo run -- --headless --dht-port 4002 --bootstrap /ip4/127.0.0.1/tcp/4001/p2p/QmXXX

# Check metrics for relay connection
```

#### Test DCUtR

```bash
# Start two private nodes with relay
# They should attempt DCUtR upgrade
# Check --show-dcutr flag for metrics
```

## Troubleshooting

### Problem: Node always shows "Private" reachability

**Possible Causes:**
- Behind NAT without port forwarding
- Firewall blocking external connections
- Router doesn't support NAT traversal

**Solutions:**
1. Set up port forwarding (see above)
2. Enable relay mode (automatic if private)
3. Check firewall rules
4. Verify router supports NAT-PMP or UPnP

### Problem: DCUtR always fails

**Possible Causes:**
- Symmetric NAT (common in cellular networks)
- Strict firewall rules
- No relay connection established

**Solutions:**
1. Check relay connectivity first
2. Verify firewall allows UDP
3. Use port forwarding instead
4. Accept relay-only communication

### Problem: No relay connection

**Possible Causes:**
- AutoRelay disabled
- No relay servers available
- Bootstrap nodes unreachable

**Solutions:**
1. Enable AutoRelay (default)
2. Specify relay nodes manually with `--relay`
3. Check bootstrap node connectivity
4. Run your own relay server

### Problem: Can't detect public IP

**Possible Causes:**
- No internet connection
- Firewall blocking HTTPS
- All detection services down

**Solutions:**
1. Check internet connectivity
2. Set CHIRAL_PUBLIC_IP manually
3. Check firewall/proxy settings
4. Verify DNS resolution works

### Problem: Port forwarding not working

**Possible Causes:**
- Wrong internal IP configured
- Router configuration not saved
- ISP blocks inbound connections
- Double NAT (router behind router)

**Solutions:**
1. Verify internal IP is correct
2. Restart router after configuration
3. Contact ISP about port blocking
4. Check for double NAT scenario
5. Test from external network

## Metrics and Monitoring

### Key Metrics

Access via `metrics_snapshot()`:

```rust
let metrics = service.metrics_snapshot().await;

// AutoNAT
println!("Reachability: {:?}", metrics.reachability);
println!("Confidence: {:?}", metrics.reachability_confidence);
println!("Observed Addrs: {:?}", metrics.observed_addrs);

// Circuit Relay
println!("Active Relay: {:?}", metrics.active_relay_peer_id);
println!("Reservation Status: {:?}", metrics.relay_reservation_status);
println!("Relay Health: {}", metrics.relay_health_score);

// DCUtR
println!("DCUtR Attempts: {}", metrics.dcutr_hole_punch_attempts);
println!("DCUtR Success Rate: {:.2}%",
    (metrics.dcutr_hole_punch_successes as f64 /
     metrics.dcutr_hole_punch_attempts as f64) * 100.0
);
```

### Logging

Enable detailed logging:

```bash
# All debug logs
RUST_LOG=debug cargo run

# Specific subsystems
RUST_LOG=libp2p_dcutr=debug,libp2p_relay=debug cargo run

# NAT-related logs only
RUST_LOG=chiral_network::dht=debug cargo run
```

## Best Practices

### For Home Users

1. **Enable AutoNAT and AutoRelay** (default)
2. **Set up port forwarding** if possible
3. **Use default bootstrap nodes**
4. **Monitor DCUtR success rate**

### For Server/VPS Deployments

1. **Set CHIRAL_PUBLIC_IP** environment variable
2. **Enable relay server mode** to help network
3. **Use static port** for consistency
4. **Monitor relay metrics** for health

### For Relay Operators

1. **Use public IP with port forwarding**
2. **Set descriptive relay alias**
3. **Monitor resource usage** (bandwidth, connections)
4. **Publish relay address** for manual configuration
5. **Keep node running** for network stability

### For Privacy-Conscious Users

1. **Use SOCKS5 proxy** (e.g., Tor)
2. **Disable public IP detection** (manual config only)
3. **Use relay-only mode** (no DCUtR)
4. **Monitor connection metadata**

## Advanced Configuration

### Custom Bootstrap Nodes

```bash
cargo run -- --headless \
  --bootstrap /ip4/203.0.113.1/tcp/4001/p2p/QmBootstrap1 \
  --bootstrap /ip4/203.0.113.2/tcp/4001/p2p/QmBootstrap2
```

### Preferred Relay Nodes

```bash
cargo run -- --headless \
  --relay /ip4/203.0.113.10/tcp/4001/p2p/QmRelay1 \
  --relay /ip4/203.0.113.11/tcp/4001/p2p/QmRelay2
```

### Custom AutoNAT Servers

```bash
cargo run -- --headless \
  --autonat-server /ip4/203.0.113.20/tcp/4001/p2p/QmAutoNAT1
```

### Combined Configuration

```bash
export CHIRAL_PUBLIC_IP="203.0.113.42"

cargo run -- --headless \
  --dht-port 4001 \
  --enable-relay-server \
  --autonat-probe-interval 60 \
  --bootstrap /ip4/203.0.113.1/tcp/4001/p2p/QmBootstrap1 \
  --log-level debug
```

## Performance Considerations

### Connection Latency

- **Direct**: Lowest latency (optimal)
- **DCUtR**: Low latency after upgrade
- **Relay**: Higher latency (1-2 hops)
- **Multi-hop relay**: Highest latency

### Bandwidth Usage

- **Direct**: Maximum bandwidth
- **DCUtR**: Near-maximum after upgrade
- **Relay**: Limited by relay capacity
- **Multi-hop**: Limited by slowest relay

### Recommendations

1. **Prefer direct connections** when possible
2. **Enable DCUtR** for automatic upgrades
3. **Use relays as fallback** only
4. **Monitor connection types** in production
5. **Optimize relay selection** for proximity

## Future Enhancements

Planned improvements:

- [ ] UPnP/NAT-PMP support for automatic port forwarding
- [ ] IPv6 support
- [ ] STUN/TURN integration
- [ ] Relay pool management
- [ ] Bandwidth-aware relay selection
- [ ] Connection type preferences
- [ ] Advanced hole-punching techniques
- [ ] Better symmetric NAT handling

## References

- [libp2p AutoNAT Specification](https://github.com/libp2p/specs/tree/master/autonat)
- [libp2p Circuit Relay v2](https://github.com/libp2p/specs/blob/master/relay/circuit-v2.md)
- [DCUtR Specification](https://github.com/libp2p/specs/blob/master/relay/DCUtR.md)
- [NAT Traversal Techniques](https://en.wikipedia.org/wiki/NAT_traversal)

---

**Last Updated**: 2025-10-28
**Version**: 1.0.0
