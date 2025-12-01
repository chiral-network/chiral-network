# Chiral Network Relay Infrastructure - Deployment Guide

## Overview

This directory contains a dedicated Circuit Relay v2 daemon for the Chiral Network. Relay nodes help NAT-traversal by allowing peers behind restrictive NATs to communicate with each other through relay circuits.

## Architecture

The relay daemon implements:

- **Circuit Relay v2**: libp2p's Circuit Relay protocol for NAT traversal
- **AutoNAT v2 Server**: Helps other peers detect their reachability
- **Identify Protocol**: Peer information exchange
- **Ping Protocol**: Connection health monitoring
- **Metrics Export**: JSON metrics for monitoring

## Requirements

### System Requirements

- **OS**: Linux, macOS, or BSD
- **CPU**: 1+ cores (2+ recommended for production)
- **RAM**: 512 MB minimum (1 GB+ recommended)
- **Network**: Public IP address with open TCP port
- **Disk**: 100 MB for binary + minimal storage for logs/metrics

### Software Requirements

- **Rust**: 1.70+ (for building from source)
- **curl**: For IP detection (optional)
- **jq**: For pretty metrics display (optional)

## Quick Start

### 1. Build the Relay Daemon

```bash
cd relay
cargo build --release
```

The binary will be located at `target/release/chiral-relay`.

### 2. Start the Relay

```bash
# Basic start with auto-detected external IP
./scripts/start-relay.sh

# Or with custom configuration
RELAY_PORT=4001 \
EXTERNAL_ADDRESS=/ip4/YOUR_PUBLIC_IP/tcp/4001 \
MAX_RESERVATIONS=256 \
MAX_CIRCUITS=32 \
./scripts/start-relay.sh
```

### 3. Check Status

```bash
./scripts/status-relay.sh
```

### 4. Stop the Relay

```bash
# Graceful shutdown
./scripts/stop-relay.sh

# Force shutdown after 30 seconds
./scripts/stop-relay.sh --timeout 30

# Force shutdown immediately
./scripts/stop-relay.sh --force
```

## Configuration

### Environment Variables

All scripts support these environment variables:

| Variable           | Default                   | Description                                        |
| ------------------ | ------------------------- | -------------------------------------------------- |
| `RELAY_PORT`       | `4001`                    | TCP port to listen on                              |
| `RELAY_DIR`        | `~/.chiral-relay`         | Data directory                                     |
| `EXTERNAL_ADDRESS` | Auto-detected             | External multiaddr (e.g., `/ip4/1.2.3.4/tcp/4001`) |
| `MAX_RESERVATIONS` | `128`                     | Maximum concurrent relay reservations              |
| `MAX_CIRCUITS`     | `16`                      | Maximum concurrent relay circuits                  |
| `IDENTITY_FILE`    | `$RELAY_DIR/identity.key` | Path to persistent identity                        |
| `PID_FILE`         | `$RELAY_DIR/relay.pid`    | PID file location                                  |
| `LOG_FILE`         | `$RELAY_DIR/relay.log`    | Log file location                                  |
| `METRICS_FILE`     | `$RELAY_DIR/metrics.json` | Metrics JSON file                                  |
| `VERBOSE`          | `false`                   | Enable verbose logging                             |
| `TIMEOUT`          | `30`                      | Shutdown timeout (seconds)                         |

### Manual Execution

You can also run the daemon manually:

```bash
./target/release/chiral-relay \
  --port 4001 \
  --external-address /ip4/YOUR_IP/tcp/4001 \
  --identity-path ~/.chiral-relay/identity.key \
  --pid-file ~/.chiral-relay/relay.pid \
  --metrics-file ~/.chiral-relay/metrics.json \
  --max-reservations 256 \
  --max-circuits 32 \
  --verbose
```

## Production Deployment

### 1. System Service (systemd)

Create `/etc/systemd/system/chiral-relay.service`:

```ini
[Unit]
Description=Chiral Network Circuit Relay v2 Daemon
After=network.target

[Service]
Type=simple
User=chiral-relay
Group=chiral-relay
Environment="RELAY_PORT=4001"
Environment="RELAY_DIR=/var/lib/chiral-relay"
Environment="EXTERNAL_ADDRESS=/ip4/YOUR_PUBLIC_IP/tcp/4001"
Environment="MAX_RESERVATIONS=256"
Environment="MAX_CIRCUITS=32"
ExecStart=/usr/local/bin/chiral-relay \
  --port 4001 \
  --external-address /ip4/YOUR_PUBLIC_IP/tcp/4001 \
  --identity-path /var/lib/chiral-relay/identity.key \
  --pid-file /var/lib/chiral-relay/relay.pid \
  --metrics-file /var/lib/chiral-relay/metrics.json \
  --max-reservations 256 \
  --max-circuits 32
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=chiral-relay

[Install]
WantedBy=multi-user.target
```

**Setup:**

```bash
# Create user
sudo useradd -r -s /bin/false chiral-relay

# Create data directory
sudo mkdir -p /var/lib/chiral-relay
sudo chown chiral-relay:chiral-relay /var/lib/chiral-relay

# Copy binary
sudo cp target/release/chiral-relay /usr/local/bin/
sudo chmod +x /usr/local/bin/chiral-relay

# Enable and start service
sudo systemctl enable chiral-relay
sudo systemctl start chiral-relay

# Check status
sudo systemctl status chiral-relay

# View logs
sudo journalctl -u chiral-relay -f
```

### 2. Docker Deployment

Create `Dockerfile`:

```dockerfile
FROM rust:1.70 as builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/chiral-relay /usr/local/bin/

RUN useradd -r -s /bin/false chiral-relay && \
    mkdir -p /data && \
    chown chiral-relay:chiral-relay /data

USER chiral-relay
VOLUME /data

EXPOSE 4001

ENTRYPOINT ["/usr/local/bin/chiral-relay"]
CMD ["--port", "4001", \
     "--identity-path", "/data/identity.key", \
     "--metrics-file", "/data/metrics.json"]
```

**Run with Docker:**

```bash
# Build image
docker build -t chiral-relay:latest .

# Run container
docker run -d \
  --name chiral-relay \
  -p 4001:4001 \
  -v chiral-relay-data:/data \
  -e EXTERNAL_ADDRESS=/ip4/YOUR_PUBLIC_IP/tcp/4001 \
  chiral-relay:latest \
  --external-address /ip4/YOUR_PUBLIC_IP/tcp/4001
```

**Docker Compose (`docker-compose.yml`):**

```yaml
version: "3.8"

services:
  chiral-relay:
    build: .
    container_name: chiral-relay
    restart: unless-stopped
    ports:
      - "4001:4001"
    volumes:
      - relay-data:/data
    environment:
      - EXTERNAL_ADDRESS=/ip4/YOUR_PUBLIC_IP/tcp/4001
    command: >
      --port 4001
      --external-address /ip4/YOUR_PUBLIC_IP/tcp/4001
      --identity-path /data/identity.key
      --metrics-file /data/metrics.json
      --max-reservations 256
      --max-circuits 32

volumes:
  relay-data:
```

### 3. Cloud Deployment

#### AWS EC2

1. Launch t3.micro or t3.small instance
2. Configure security group to allow TCP 4001
3. Assign Elastic IP
4. Install via systemd (see above)

#### Google Cloud Platform

```bash
# Create instance
gcloud compute instances create chiral-relay \
  --machine-type=e2-micro \
  --zone=us-central1-a \
  --tags=chiral-relay

# Configure firewall
gcloud compute firewall-rules create allow-chiral-relay \
  --allow=tcp:4001 \
  --target-tags=chiral-relay

# SSH and setup
gcloud compute ssh chiral-relay
# ... install and configure relay
```

#### DigitalOcean

```bash
# Use Droplet with Docker pre-installed
# Deploy via docker-compose (see above)
doctl compute droplet create chiral-relay \
  --image docker-20-04 \
  --size s-1vcpu-1gb \
  --region nyc1
```

## Network Configuration

### Firewall Rules

Ensure your firewall allows inbound TCP connections on the relay port:

```bash
# iptables
sudo iptables -A INPUT -p tcp --dport 4001 -j ACCEPT

# ufw
sudo ufw allow 4001/tcp

# firewalld
sudo firewall-cmd --permanent --add-port=4001/tcp
sudo firewall-cmd --reload
```

### Port Forwarding

If running behind NAT (e.g., home network):

1. Access your router's admin panel
2. Forward TCP port 4001 to your relay server's local IP
3. Use your public IP in `EXTERNAL_ADDRESS`

### DNS Configuration (Optional)

For easier discovery, create a DNS A record:

```
relay.your-domain.com.  IN  A  YOUR_PUBLIC_IP
```

Then use in multiaddr:

```
/dns4/relay.your-domain.com/tcp/4001/p2p/YOUR_PEER_ID
```

## Monitoring

### Metrics File

The daemon writes metrics to `metrics.json`:

```json
{
  "peer_id": "12D3KooWABC123...",
  "listen_addresses": ["/ip4/0.0.0.0/tcp/4001"],
  "connected_peers": 42,
  "uptime_seconds": 3600,
  "relay_reservations": 15,
  "relay_circuits": 3
}
```

### Health Check Script

Create a monitoring script:

```bash
#!/bin/bash
METRICS_FILE="$HOME/.chiral-relay/metrics.json"

if [ ! -f "$METRICS_FILE" ]; then
  echo "CRITICAL: Metrics file not found"
  exit 2
fi

# Check if metrics are fresh (updated within last 60 seconds)
last_modified=$(stat -f %m "$METRICS_FILE" 2>/dev/null || stat -c %Y "$METRICS_FILE" 2>/dev/null)
current_time=$(date +%s)
age=$((current_time - last_modified))

if [ $age -gt 60 ]; then
  echo "WARNING: Metrics are stale (${age}s old)"
  exit 1
fi

# Parse metrics
reservations=$(jq -r '.relay_reservations' "$METRICS_FILE")
circuits=$(jq -r '.relay_circuits' "$METRICS_FILE")
peers=$(jq -r '.connected_peers' "$METRICS_FILE")

echo "OK: $peers peers, $reservations reservations, $circuits circuits"
exit 0
```

### Prometheus Integration

Export metrics for Prometheus:

```bash
# Create metrics exporter script
cat > /usr/local/bin/chiral-relay-metrics.sh << 'EOF'
#!/bin/bash
METRICS_FILE="/var/lib/chiral-relay/metrics.json"

echo "# HELP chiral_relay_connected_peers Number of connected peers"
echo "# TYPE chiral_relay_connected_peers gauge"
jq -r '"chiral_relay_connected_peers " + (.connected_peers | tostring)' "$METRICS_FILE"

echo "# HELP chiral_relay_reservations Number of active relay reservations"
echo "# TYPE chiral_relay_reservations gauge"
jq -r '"chiral_relay_reservations " + (.relay_reservations | tostring)' "$METRICS_FILE"

echo "# HELP chiral_relay_circuits Number of active relay circuits"
echo "# TYPE chiral_relay_circuits gauge"
jq -r '"chiral_relay_circuits " + (.relay_circuits | tostring)' "$METRICS_FILE"

echo "# HELP chiral_relay_uptime_seconds Relay uptime in seconds"
echo "# TYPE chiral_relay_uptime_seconds counter"
jq -r '"chiral_relay_uptime_seconds " + (.uptime_seconds | tostring)' "$METRICS_FILE"
EOF

chmod +x /usr/local/bin/chiral-relay-metrics.sh
```

Add to Prometheus config:

```yaml
scrape_configs:
  - job_name: "chiral-relay"
    static_configs:
      - targets: ["localhost:9100"]
    metrics_path: "/metrics"
```

## Troubleshooting

### Relay Won't Start

**Check logs:**

```bash
tail -f ~/.chiral-relay/relay.log
```

**Common issues:**

- Port already in use: `lsof -i :4001`
- Permission denied: Run with appropriate user
- Invalid external address: Check multiaddr format

### No Connections

**Verify reachability:**

```bash
# From another machine
telnet YOUR_PUBLIC_IP 4001
```

**Check firewall:**

```bash
sudo iptables -L -n | grep 4001
```

### High Resource Usage

**Reduce limits:**

```bash
MAX_RESERVATIONS=64 \
MAX_CIRCUITS=8 \
./scripts/start-relay.sh
```

**Monitor with:**

```bash
# CPU and memory
top -p $(cat ~/.chiral-relay/relay.pid)

# Network connections
netstat -an | grep 4001 | wc -l
```

### Stale Metrics

Metrics should update every time an event occurs. If stale:

1. Check daemon is running: `./scripts/status-relay.sh`
2. Check for errors: `tail -f ~/.chiral-relay/relay.log`
3. Restart daemon: `./scripts/stop-relay.sh && ./scripts/start-relay.sh`

## Security Considerations

### Best Practices

1. **Run as dedicated user**: Don't run as root
2. **Limit reservations**: Set reasonable `MAX_RESERVATIONS`
3. **Monitor resource usage**: Prevent abuse
4. **Keep software updated**: Regularly rebuild with latest dependencies
5. **Secure identity key**: Protect `identity.key` file permissions

### Rate Limiting

The relay has built-in Circuit Relay v2 limits:

- `max_reservations`: Maximum concurrent reservations
- `max_circuits`: Maximum concurrent circuits
- `reservation_duration`: How long reservations last (1 hour default)

Adjust based on your resources:

```bash
# Conservative (low-end VPS)
MAX_RESERVATIONS=32 MAX_CIRCUITS=4

# Moderate (decent VPS)
MAX_RESERVATIONS=128 MAX_CIRCUITS=16

# High-capacity (dedicated server)
MAX_RESERVATIONS=512 MAX_CIRCUITS=64
```

## Performance Tuning

### OS Limits

Increase file descriptor limits:

```bash
# /etc/security/limits.conf
chiral-relay soft nofile 65536
chiral-relay hard nofile 65536
```

### Kernel Parameters

Optimize network stack:

```bash
# /etc/sysctl.conf
net.core.rmem_max = 16777216
net.core.wmem_max = 16777216
net.ipv4.tcp_rmem = 4096 87380 16777216
net.ipv4.tcp_wmem = 4096 65536 16777216
net.ipv4.tcp_max_syn_backlog = 8192
net.core.somaxconn = 1024
```

Apply with:

```bash
sudo sysctl -p
```

## Community Relay Network

To contribute your relay to the Chiral Network community:

1. Ensure high uptime (>99%)
2. Use a static IP or DNS name
3. Share your multiaddr: `/ip4/YOUR_IP/tcp/4001/p2p/YOUR_PEER_ID`
4. Submit via GitHub issue or community forum

## Support

For issues or questions:

- **GitHub Issues**: https://github.com/yourusername/chiral-network/issues
- **Logs**: Always include log output from `~/.chiral-relay/relay.log`
- **Metrics**: Include output from `./scripts/status-relay.sh`

## License

MIT License - See LICENSE file for details
