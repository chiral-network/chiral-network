
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

## Deploying Your Own Relay on Google Cloud (Free)

Google Cloud offers $300 in free credits for new accounts, and Compute Engine has a free tier that's perfect for testing.

### Step 1: Create Google Cloud VM

1. Go to https://console.cloud.google.com
2. Navigate to **Compute Engine** → **VM instances**
3. Click **Create Instance**
4. **Configure your VM:**
   - **Name:** `chiral-relay-1`
   - **Region:** Choose closest to your users (e.g., `us-central1`)
   - **Machine type:** `e2-micro` (free tier eligible, 0.25-1 vCPU, 1 GB RAM)
   - **Boot disk:** Ubuntu 22.04 LTS or Debian 12
   - **Firewall:** Check both "Allow HTTP traffic" and "Allow HTTPS traffic"
5. Click **Create**

### Step 2: Configure Firewall for libp2p

1. Note your VM's **External IP** (shown in VM instances list)
2. Go to **VPC network** → **Firewall**
3. Click **Create Firewall Rule**
   - **Name:** `allow-libp2p`
   - **Targets:** All instances in the network
   - **Source IPv4 ranges:** `0.0.0.0/0`
   - **Protocols and ports:** `tcp:4001`
4. Click **Create**

### Step 3: SSH into VM and Install Dependencies

```bash
# SSH into your VM (click "SSH" button in Google Cloud Console)

# Update system
sudo apt update && sudo apt upgrade -y

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Install build dependencies
sudo apt install -y build-essential pkg-config libssl-dev git

# Verify installation
rustc --version
cargo --version
```

### Step 4: Clone and Build Relay

```bash
# Clone repository
git clone https://github.com/YOUR_USERNAME/chiral-network.git
cd chiral-network/relay

# Build relay in release mode (optimized)
cargo build --release

# This will take 5-10 minutes on e2-micro instance
# Binary will be at: target/release/chiral-relay
```

### Step 5: Run Relay with External Address

```bash
# Get your external IP (if you didn't note it earlier)
EXTERNAL_IP=$(curl -s ifconfig.me)
echo "External IP: $EXTERNAL_IP"

# Run relay with external address
./target/release/chiral-relay \
  --port 4001 \
  --external-address /ip4/$EXTERNAL_IP/tcp/4001

# Keep this terminal open - you'll see logs
```

**Expected output:**
```
🚀 Starting Chiral Network Relay Daemon
🔑 Generating ephemeral identity (use --identity-path to persist)
📋 Peer ID: 12D3KooW...
👂 Listening on /ip4/0.0.0.0/tcp/4001
🌐 External address: /ip4/YOUR_IP/tcp/4001
📋 Full multiaddr: /ip4/YOUR_IP/tcp/4001/p2p/12D3KooW...
✅ Relay daemon is running
```

**Copy the full multiaddr** - you'll need it for your main app configuration!

### Step 6: Run Relay as Background Service (Optional)

To keep relay running after closing SSH:

```bash
# Install screen or tmux
sudo apt install -y screen

# Run in screen session
screen -S relay
./target/release/chiral-relay --port 4001 --external-address /ip4/$EXTERNAL_IP/tcp/4001

# Detach: Press Ctrl+A, then D
# Reattach later: screen -r relay
```

**Or use systemd service:**

```bash
# Create systemd service file
sudo tee /etc/systemd/system/chiral-relay.service > /dev/null <<EOF
[Unit]
Description=Chiral Network Relay Daemon
After=network.target

[Service]
Type=simple
User=$USER
WorkingDirectory=$HOME/chiral-network/relay
ExecStart=$HOME/chiral-network/relay/target/release/chiral-relay --port 4001 --external-address /ip4/$EXTERNAL_IP/tcp/4001
Restart=on-failure
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

# Enable and start service
sudo systemctl enable chiral-relay
sudo systemctl start chiral-relay

# Check status
sudo systemctl status chiral-relay

# View logs
sudo journalctl -u chiral-relay -f
```

### Step 7: Configure Your Main App

**Important**: The Google Cloud relay is NOT hardcoded - you must configure it manually through Settings to use it.

1. Copy the full multiaddr from relay logs (format: `/ip4/YOUR_IP/tcp/4001/p2p/12D3KooW...`)
   - Example: `/ip4/34.41.241.133/tcp/4001/p2p/12D3KooWEqLehCCY28NPieRjj2bbovqai1LW5bp19ZeMMa3DLLNG`

2. In your main Chiral Network app:
   - Go to **Settings** → **Network Settings**
   - Scroll to **NAT Traversal Configuration**
   - In **AutoNAT Servers** textarea, paste the multiaddr
   - Enable **AutoNAT** toggle
   - In **Preferred Relay Nodes** textarea, paste the same multiaddr
   - Enable **AutoRelay** toggle
   - Click **Save Settings**

3. Restart DHT in **Network** page

**Note**: The app uses a default bootstrap node by default. Your Google Cloud relay is an optional enhancement that you can configure for better NAT traversal and testing.

### Step 8: Test Connection

1. In your main app, go to **Network** page
2. Monitor **DHT Events Log**
3. Look for:
   - `Dialing AutoNAT server: /ip4/YOUR_IP/tcp/4001/p2p/...`
   - `Connection established with peer: 12D3KooW...`
   - `AutoNAT v2 event received`

4. On your relay server, check logs:
   - `Connection established with peer: ... (total: 1)`
   - AutoNAT probe requests

### Troubleshooting

**Connection refused:**
- Check firewall rule allows tcp:4001
- Verify external IP in multiaddr is correct
- Ensure relay is running: `ps aux | grep chiral-relay`

**Relay not probing:**
- Verify AutoNAT is enabled in Settings
- Check DHT is running (Network page)
- Restart DHT after configuration changes

**VM stops after SSH closes:**
- Use `screen`, `tmux`, or systemd service
- Or use `nohup ./target/release/chiral-relay ... &`

### Cost Optimization

**Google Cloud Free Tier:**
- e2-micro instance: 1 free instance per month (US regions)
- 30 GB standard persistent disk
- 1 GB network egress (after $300 credits expire)

**Staying within free tier:**
- Use e2-micro instance
- Choose US region
- Monitor bandwidth usage in Google Cloud Console
- Relay traffic is minimal unless actively relaying many circuits

### Testing with Two Machines on Different Networks

Once your relay is deployed:

1. **Machine A (your local machine):**
   - Configure AutoNAT server and relay (as above)
   - Start app, verify connection to relay

2. **Machine B (different network - friend, another location):**
   - Clone repository
   - Configure same relay multiaddr in Settings
   - Start app

3. **Test peer discovery:**
   - Both machines should connect to relay
   - Check Network page for peer connections
   - Try file sharing between machines

## See Also

- [Network Protocol](network-protocol.md) - P2P networking details
- [Security & Privacy](security-privacy.md) - Privacy features
- [Deployment Guide](deployment-guide.md) - Production setup
- [relay/DEPLOYMENT.md](../relay/DEPLOYMENT.md) - Advanced relay deployment
