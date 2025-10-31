# WebRTC Signaling Server Deployment Guide

## Overview

This guide explains how to deploy the WebRTC Signaling Server to Google Cloud VM, working alongside the existing Circuit Relay v2 node for complete P2P file sharing functionality.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                Google Cloud VM                              │
│                                                             │
│  Port 4001: libp2p Circuit Relay v2                        │
│  └─ Role: DHT peer discovery + metadata relay              │
│  └─ Used for: Finding peers and file metadata              │
│                                                             │
│  Port 9000: WebRTC Signaling Server                        │
│  └─ Role: WebRTC connection setup (SDP/ICE exchange)       │
│  └─ Used for: Establishing P2P data channels               │
│                                                             │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                  File Sharing Flow                          │
└─────────────────────────────────────────────────────────────┘

1. Peer Discovery (Circuit Relay v2)
   Uploader → [Relay:4001] ← Downloader
   "I have file XYZ"    "Who has file XYZ?"

2. WebRTC Setup (Signaling Server)
   Uploader → [Signaling:9000] ← Downloader
   SDP Offer/Answer, ICE Candidates

3. File Transfer (WebRTC P2P)
   Uploader ←────────────────────→ Downloader
            Direct P2P transfer
```

## Why Both Servers?

### Circuit Relay v2 (Port 4001)
- **Purpose**: Enable NAT peers to participate in DHT
- **Traffic**: Metadata only (file hashes, peer IDs)
- **Bandwidth**: Low (KB/s)
- **Used by**: libp2p DHT operations

### Signaling Server (Port 9000)
- **Purpose**: Coordinate WebRTC connections
- **Traffic**: SDP offers/answers, ICE candidates
- **Bandwidth**: Very low (connection setup only)
- **Used by**: WebRTC connection establishment

**Note**: Neither server relays actual file data. File transfer happens directly peer-to-peer via WebRTC Data Channels.

## Building the Binary

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone repository
git clone https://github.com/your-org/chiral-network.git
cd chiral-network/src-tauri
```

### Build Release Binary

```bash
# Build optimized binary
cargo build --bin signaling-server --release

# Binary location
ls -lh target/release/signaling-server
```

The binary is optimized for size and performance with:
- LTO (Link Time Optimization)
- Strip symbols
- Minimal dependencies

## Deployment to Google Cloud

### Step 1: Create VM Instance

```bash
# Create VM (if not already created for relay)
gcloud compute instances create chiral-signaling \
  --zone=us-central1-a \
  --machine-type=e2-micro \
  --image-family=ubuntu-2204-lts \
  --image-project=ubuntu-os-cloud \
  --boot-disk-size=10GB
```

**Note**: If you already have a VM for Circuit Relay, you can reuse it.

### Step 2: Configure Firewall

```bash
# Allow WebSocket traffic on port 9000
gcloud compute firewall-rules create allow-signaling \
  --allow=tcp:9000 \
  --description="Allow WebRTC Signaling Server" \
  --target-tags=signaling

# Add tag to your VM
gcloud compute instances add-tags chiral-signaling \
  --zone=us-central1-a \
  --tags=signaling
```

### Step 3: Upload Binary

```bash
# Copy binary to VM
gcloud compute scp target/release/signaling-server \
  chiral-signaling:~/ \
  --zone=us-central1-a

# SSH into VM
gcloud compute ssh chiral-signaling --zone=us-central1-a
```

### Step 4: Run Server

#### Option A: Direct Execution (Testing)

```bash
# Make executable
chmod +x ~/signaling-server

# Run server
./signaling-server

# Run with custom port
PORT=9000 ./signaling-server
```

#### Option B: Systemd Service (Production)

```bash
# Create service file
sudo nano /etc/systemd/system/chiral-signaling.service
```

```ini
[Unit]
Description=Chiral Network WebRTC Signaling Server
After=network.target

[Service]
Type=simple
User=your-username
WorkingDirectory=/home/your-username
ExecStart=/home/your-username/signaling-server
Restart=always
RestartSec=10
Environment="PORT=9000"
Environment="RUST_LOG=info"

# Security
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

```bash
# Enable and start service
sudo systemctl daemon-reload
sudo systemctl enable chiral-signaling
sudo systemctl start chiral-signaling

# Check status
sudo systemctl status chiral-signaling

# View logs
sudo journalctl -u chiral-signaling -f
```

#### Option C: Docker (Alternative)

```dockerfile
# Dockerfile
FROM ubuntu:22.04
RUN apt-get update && apt-get install -y ca-certificates
COPY target/release/signaling-server /usr/local/bin/
EXPOSE 9000
CMD ["signaling-server"]
```

```bash
# Build and run
docker build -t chiral-signaling .
docker run -d -p 9000:9000 --name signaling chiral-signaling
```

## Running Both Servers Together

If using the same VM for both servers:

### Systemd Configuration

```bash
# Create service for relay (if not already created)
sudo nano /etc/systemd/system/chiral-relay.service
```

```ini
[Unit]
Description=Chiral Network Circuit Relay v2
After=network.target

[Service]
Type=simple
User=your-username
ExecStart=/home/your-username/relay-node
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

```bash
# Start both services
sudo systemctl start chiral-relay
sudo systemctl start chiral-signaling

# Check both are running
sudo systemctl status chiral-relay
sudo systemctl status chiral-signaling

# View logs
sudo journalctl -u chiral-relay -u chiral-signaling -f
```

### Port Summary

| Service | Port | Protocol | Purpose |
|---------|------|----------|---------|
| Circuit Relay v2 | 4001 | TCP | libp2p relay, DHT |
| Signaling Server | 9000 | TCP (WS) | WebRTC signaling |

## Client Configuration

Update your application to use the deployed server:

```typescript
// src/lib/services/signalingService.ts

const SIGNALING_SERVER_URL = 'ws://YOUR_VM_IP:9000';

class SignalingService {
  connect() {
    this.ws = new WebSocket(SIGNALING_SERVER_URL);
    // ... rest of implementation
  }
}
```

## Testing

### Test Signaling Server

```bash
# Install websocat (WebSocket client)
curl -L https://github.com/vi/websocat/releases/download/v1.12.0/websocat.x86_64-unknown-linux-musl \
  -o websocat && chmod +x websocat

# Connect to server
./websocat ws://localhost:9000

# Send test message
{"type":"register","clientId":"test-client"}
```

### Test from Application

```typescript
// Test WebSocket connection
const ws = new WebSocket('ws://YOUR_VM_IP:9000');

ws.onopen = () => {
  console.log('✅ Connected to signaling server');
  ws.send(JSON.stringify({
    type: 'register',
    clientId: 'test-peer-123'
  }));
};

ws.onmessage = (event) => {
  console.log('📥 Received:', event.data);
};

ws.onerror = (error) => {
  console.error('❌ Error:', error);
};
```

## Monitoring

### Check Server Status

```bash
# Check if server is running
ps aux | grep signaling-server

# Check port is listening
sudo netstat -tulpn | grep 9000

# Check logs
sudo journalctl -u chiral-signaling --since "10 minutes ago"
```

### Resource Usage

```bash
# Check CPU/Memory
top -p $(pgrep signaling-server)

# Check network connections
sudo ss -anp | grep :9000
```

### Logs

The server logs to stdout with these log levels:
- `INFO`: Normal operation (connection events)
- `WARN`: Non-critical issues
- `ERROR`: Critical errors

```bash
# View recent logs
sudo journalctl -u chiral-signaling -n 100

# Follow live logs
sudo journalctl -u chiral-signaling -f

# Filter by level
sudo journalctl -u chiral-signaling -p err
```

## Troubleshooting

### Server Won't Start

```bash
# Check port is not in use
sudo lsof -i :9000

# Check permissions
ls -l ~/signaling-server

# Check firewall
sudo iptables -L -n | grep 9000
```

### Connection Refused

```bash
# Verify firewall rules
gcloud compute firewall-rules list | grep signaling

# Test from VM
curl -v http://localhost:9000/health

# Test from external
telnet YOUR_VM_IP 9000
```

### High Memory Usage

```bash
# Check active connections
sudo ss -anp | grep :9000 | wc -l

# Restart service
sudo systemctl restart chiral-signaling
```

## Security Considerations

### Current Implementation

⚠️ **No authentication**: Anyone can connect to the signaling server

⚠️ **No encryption**: WebSocket traffic is unencrypted (ws://)

⚠️ **No rate limiting**: Open to abuse

### Recommended Improvements

1. **Add TLS (WSS)**:
   ```bash
   # Install certbot
   sudo apt install certbot

   # Get certificate
   sudo certbot certonly --standalone -d your-domain.com
   ```

2. **Add Authentication**:
   - JWT tokens
   - API keys
   - Client certificates

3. **Add Rate Limiting**:
   - Connection limits per IP
   - Message rate limits
   - Timeout disconnections

4. **Use Reverse Proxy**:
   ```nginx
   # nginx config
   server {
       listen 443 ssl;
       server_name signaling.your-domain.com;

       location / {
           proxy_pass http://localhost:9000;
           proxy_http_version 1.1;
           proxy_set_header Upgrade $http_upgrade;
           proxy_set_header Connection "upgrade";
       }
   }
   ```

## Cost Estimation

### Google Cloud e2-micro VM

- **VM**: ~$6-7/month (free tier eligible)
- **Bandwidth**: $0.12/GB egress (first 1GB free)
- **Storage**: $0.04/GB/month

**Estimated Total**: $7-10/month for small network

### Resource Requirements

| Metric | Signaling Server | Circuit Relay |
|--------|-----------------|---------------|
| CPU | Very low (<5%) | Low (5-10%) |
| Memory | 20-50 MB | 100-200 MB |
| Bandwidth | Minimal | Low-Moderate |

Both servers can run on a single e2-micro instance.

## Scaling

### When to Scale

Monitor these metrics:
- Connection count > 1000 simultaneous
- CPU usage > 80%
- Memory usage > 80%
- Response time > 100ms

### Horizontal Scaling

```bash
# Create load balancer
gcloud compute backend-services create signaling-backend \
  --protocol=TCP \
  --health-checks=signaling-health \
  --global

# Add multiple instances
gcloud compute instance-groups managed create signaling-group \
  --size=3 \
  --template=signaling-template
```

## Maintenance

### Updates

```bash
# Rebuild binary
cargo build --bin signaling-server --release

# Upload new binary
gcloud compute scp target/release/signaling-server \
  chiral-signaling:~/ \
  --zone=us-central1-a

# Restart service
gcloud compute ssh chiral-signaling --zone=us-central1-a \
  --command="sudo systemctl restart chiral-signaling"
```

### Backup

```bash
# Backup configuration
gcloud compute instances describe chiral-signaling \
  --zone=us-central1-a > backup-config.yaml

# Create snapshot
gcloud compute disks snapshot chiral-signaling-disk \
  --zone=us-central1-a
```

## Production Checklist

- [ ] Binary compiled with `--release`
- [ ] Systemd service configured
- [ ] Firewall rules applied
- [ ] SSL/TLS certificates installed (if using WSS)
- [ ] Monitoring setup
- [ ] Logs rotation configured
- [ ] Backup strategy in place
- [ ] Documentation updated with VM IP
- [ ] Client apps configured with server URL
- [ ] Load testing performed

## Support

For issues:
1. Check logs: `sudo journalctl -u chiral-signaling -f`
2. Verify connectivity: `telnet VM_IP 9000`
3. Test locally first: `cargo run --bin signaling-server`
4. Open GitHub issue with logs and error messages

## References

- [Circuit Relay v2 PR](link-to-pr)
- [WebRTC Signaling Protocol](https://webrtc.org/)
- [Google Cloud VM Documentation](https://cloud.google.com/compute/docs)
- [Systemd Service Management](https://www.freedesktop.org/software/systemd/man/systemd.service.html)
