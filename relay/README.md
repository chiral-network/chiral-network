# Chiral Network Relay Infrastructure

Dedicated Circuit Relay v2 daemon for NAT traversal in the Chiral Network.

## Quick Start

```bash
# Build the relay daemon
cargo build --release

# Start relay (auto-detects external IP)
./scripts/start-relay.sh

# Check status
./scripts/status-relay.sh

# Stop relay
./scripts/stop-relay.sh
```

## What is This?

This is a standalone relay node that helps Chiral Network peers behind NATs communicate with each other. It implements libp2p's Circuit Relay v2 protocol.

### Features

- ✅ Circuit Relay v2 with configurable limits
- ✅ AutoNAT v2 server for reachability detection
- ✅ Persistent identity across restarts
- ✅ JSON metrics export
- ✅ Graceful shutdown handling
- ✅ Health monitoring scripts

## Architecture

```
┌─────────────┐      ┌──────────────┐      ┌─────────────┐
│   Peer A    │      │    Relay     │      │   Peer B    │
│  (Private)  │─────▶│   (Public)   │◀─────│  (Private)  │
└─────────────┘      └──────────────┘      └─────────────┘
                            │
                     Relay Circuit
                            │
        ┌───────────────────┴───────────────────┐
        │                                       │
        ▼                                       ▼
  Reservation                              Data Relay
  (Reserve slot)                          (Forward traffic)
```

## Configuration

Environment variables for scripts:

```bash
export RELAY_PORT=4001                              # TCP port
export EXTERNAL_ADDRESS=/ip4/1.2.3.4/tcp/4001     # External multiaddr
export MAX_RESERVATIONS=128                         # Max concurrent reservations
export MAX_CIRCUITS=16                              # Max concurrent circuits
export RELAY_DIR=$HOME/.chiral-relay               # Data directory
export VERBOSE=true                                 # Enable debug logs
```

## Usage

### Basic Usage

```bash
# Start with defaults
./scripts/start-relay.sh

# View logs
tail -f ~/.chiral-relay/relay.log

# Check status
./scripts/status-relay.sh
```

### Custom Configuration

```bash
# High-capacity relay
RELAY_PORT=4001 \
EXTERNAL_ADDRESS=/ip4/203.0.113.42/tcp/4001 \
MAX_RESERVATIONS=256 \
MAX_CIRCUITS=32 \
./scripts/start-relay.sh
```

### Manual Execution

```bash
./target/release/chiral-relay \
  --port 4001 \
  --external-address /ip4/YOUR_IP/tcp/4001 \
  --identity-path ~/.chiral-relay/identity.key \
  --max-reservations 128 \
  --max-circuits 16 \
  --verbose
```

## Scripts

| Script | Description |
|--------|-------------|
| `start-relay.sh` | Bootstrap and start the relay daemon |
| `stop-relay.sh` | Gracefully stop the relay daemon |
| `status-relay.sh` | Check status and display metrics |

### Script Options

**start-relay.sh**: Uses environment variables (see Configuration)

**stop-relay.sh**:
```bash
./scripts/stop-relay.sh              # Graceful shutdown (30s timeout)
./scripts/stop-relay.sh --force      # Immediate force kill
./scripts/stop-relay.sh --timeout 60 # Custom timeout
```

**status-relay.sh**: No options, displays comprehensive status

## Deployment

### Development

```bash
# Local testing
./scripts/start-relay.sh
```

### Production (systemd)

```bash
# Copy binary
sudo cp target/release/chiral-relay /usr/local/bin/

# Create service file
sudo nano /etc/systemd/system/chiral-relay.service
# (See DEPLOYMENT.md for full config)

# Enable and start
sudo systemctl enable chiral-relay
sudo systemctl start chiral-relay
```

### Docker

```bash
# Build image
docker build -t chiral-relay:latest .

# Run container
docker run -d \
  --name chiral-relay \
  -p 4001:4001 \
  -v relay-data:/data \
  chiral-relay:latest
```

See [DEPLOYMENT.md](DEPLOYMENT.md) for comprehensive deployment guides including:
- systemd service setup
- Docker/docker-compose
- Cloud deployment (AWS, GCP, DigitalOcean)
- Network configuration
- Monitoring and troubleshooting

## Metrics

The daemon exports metrics to `~/.chiral-relay/metrics.json`:

```json
{
  "peer_id": "12D3KooW...",
  "listen_addresses": ["/ip4/0.0.0.0/tcp/4001"],
  "connected_peers": 42,
  "uptime_seconds": 3600,
  "relay_reservations": 15,
  "relay_circuits": 3
}
```

## Network Requirements

- **Port**: TCP port (default 4001) open for inbound connections
- **IP**: Public IP address (or port forwarding)
- **Bandwidth**: Varies by load (typical: 1-10 Mbps)
- **Resources**: ~100 MB RAM, minimal CPU

## Firewall Configuration

```bash
# Linux (iptables)
sudo iptables -A INPUT -p tcp --dport 4001 -j ACCEPT

# Linux (ufw)
sudo ufw allow 4001/tcp

# Linux (firewalld)
sudo firewall-cmd --permanent --add-port=4001/tcp
sudo firewall-cmd --reload
```

## Adding Relay to Chiral Network

Once running, share your relay's multiaddr with the network:

```
/ip4/YOUR_PUBLIC_IP/tcp/4001/p2p/YOUR_PEER_ID
```

Find your peer ID in:
- Logs: `~/.chiral-relay/relay.log`
- Status: `./scripts/status-relay.sh`
- Metrics: `~/.chiral-relay/metrics.json`

Users can add your relay in Chiral Network Settings:
1. Open Settings → NAT Traversal
2. Add to "Preferred Relay Nodes"
3. Save settings

## Troubleshooting

### Relay won't start

```bash
# Check if port is in use
lsof -i :4001

# Check logs
tail -f ~/.chiral-relay/relay.log

# Clean up stale PID
rm ~/.chiral-relay/relay.pid
```

### No incoming connections

```bash
# Test connectivity from another machine
telnet YOUR_PUBLIC_IP 4001

# Check firewall
sudo iptables -L -n | grep 4001

# Verify external address
./scripts/status-relay.sh
```

### High resource usage

```bash
# Reduce limits
MAX_RESERVATIONS=64 \
MAX_CIRCUITS=8 \
./scripts/start-relay.sh

# Monitor resources
top -p $(cat ~/.chiral-relay/relay.pid)
```

## Security

- Run as non-root user
- Set reasonable reservation/circuit limits
- Keep software updated
- Protect identity key file permissions
- Monitor for abuse

## Performance Tuning

Adjust limits based on your resources:

```bash
# Low-end VPS (1 vCPU, 512 MB RAM)
MAX_RESERVATIONS=32 MAX_CIRCUITS=4

# Medium VPS (2 vCPU, 2 GB RAM)
MAX_RESERVATIONS=128 MAX_CIRCUITS=16

# Dedicated server (4+ vCPU, 8+ GB RAM)
MAX_RESERVATIONS=512 MAX_CIRCUITS=64
```

## Documentation

- [DEPLOYMENT.md](DEPLOYMENT.md) - Comprehensive deployment guide
- [Cargo.toml](Cargo.toml) - Rust project configuration
- [src/main.rs](src/main.rs) - Source code with inline docs

## Support

- **Issues**: https://github.com/yourusername/chiral-network/issues
- **Logs**: `~/.chiral-relay/relay.log`
- **Metrics**: `./scripts/status-relay.sh`

## License

MIT License - See LICENSE file for details
