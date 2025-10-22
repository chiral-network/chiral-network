# Chiral Network Deployment Guide

## Deployment Overview

This guide provides comprehensive instructions for deploying the Chiral Network in various environments, from local development to production-scale deployments. It covers node setup, network configuration, monitoring, and maintenance procedures.

## Prerequisites

### System Requirements

#### Minimum Requirements

| Component | Specification                              |
| --------- | ------------------------------------------ |
| CPU       | 2 cores @ 2.0 GHz                          |
| RAM       | 4 GB                                       |
| Storage   | 100 GB SSD                                 |
| Network   | 10 Mbps symmetric                          |
| OS        | Ubuntu 20.04+ / Windows 10+ / macOS 10.15+ |

#### Recommended Production

| Component | Specification     |
| --------- | ----------------- |
| CPU       | 8 cores @ 3.0 GHz |
| RAM       | 16 GB             |
| Storage   | 1 TB NVMe SSD     |
| Network   | 1 Gbps symmetric  |
| OS        | Ubuntu 22.04 LTS  |

### Software Dependencies

```bash
# Core Dependencies
- Node.js 18.x or higher
- Rust 1.70 or higher
- Docker 24.x (optional)

# Build Tools
- Git
- Make
- GCC/Clang
- Python 3.8+
```

## Installation Methods

### 1. Binary Installation (Recommended)

#### Download Pre-built Binaries

```bash
# Linux/macOS
curl -L https://github.com/chiral-network/releases/latest/download/chiral-linux-amd64.tar.gz | tar xz
cd chiral-network
sudo ./install.sh

# Windows
Invoke-WebRequest -Uri https://github.com/chiral-network/releases/latest/download/chiral-windows-amd64.zip -OutFile chiral.zip
Expand-Archive chiral.zip
cd chiral-network
./install.bat
```

### 2. Docker Installation

#### Using Docker Compose

```yaml
# docker-compose.yml
version: "3.8"

services:
  blockchain:
    image: chiralnetwork/node:latest
    container_name: chiral-blockchain
    ports:
      - "30304:30304" # P2P
      - "8546:8546" # RPC
      - "8547:8547" # WebSocket
    volumes:
      - blockchain-data:/data
      - ./config/blockchain.toml:/config/blockchain.toml
    command: ["--config", "/config/blockchain.toml"]
    restart: unless-stopped

  peer:
    image: chiralnetwork/peer:latest
    container_name: chiral-peer
    ports:
      - "8080:8080" # File transfer
      - "4001:4001" # DHT
    volumes:
      - peer-data:/data
      - ./config/peer.toml:/config/peer.toml
    environment:
      - MAX_STORAGE=100GB
    restart: unless-stopped

volumes:
  blockchain-data:
  peer-data:
```

#### Start Services

```bash
# Start all services
docker-compose up -d

# View logs
docker-compose logs -f

# Stop services
docker-compose down
```

### 3. Building from Source

#### Clone Repository

```bash
git clone https://github.com/chiral-network/chiral-network.git
cd chiral-network
```

#### Build Blockchain Node

```bash
cd blockchain
cargo build --release
sudo cp target/release/chiral-node /usr/local/bin/
```

[TODO: Revise this. I think we only have one binary.]

#### Build Peer Node

```bash
cd ../peer
cargo build --release
sudo cp target/release/chiral-peer /usr/local/bin/
```

#### Build Client Application

```bash
cd ../chiral-app
npm install
npm run build
npm run tauri build
```

## Configuration

### 1. Blockchain Configuration

#### blockchain.toml

```toml
[network]
chain_id = 9001
network_id = 9001
port = 30304
bootnodes = [
  "enode://node1@seed1.chiral.network:30304",
  "enode://node2@seed2.chiral.network:30304"
]

[mining]
engine_signer = "0x0000000000000000000000000000000000000000"
reseal_on_txs = "all"
reseal_min_period = 4000
work_queue_size = 20

[rpc]
port = 8546
apis = ["web3", "eth", "net", "personal", "rpc"]
cors = ["all"]
hosts = ["all"]

[websockets]
port = 8547
apis = ["web3", "eth", "net", "rpc", "secretstore"]
origins = ["all"]

[storage]
data_dir = "/var/lib/chiral/blockchain"
cache_size = 1024
pruning = "archive"

[mining]
enabled = false
coinbase = "0x0000000000000000000000000000000000000000"
threads = 4
```

### 2. Peer Node Configuration

**Note**: All nodes are equal peers. Any node can seed files, download files, and participate in DHT. There are no dedicated "storage nodes".

#### peer.toml

```toml
[node]
id = "auto"
# Max storage capacity this peer will dedicate to seeding files
capacity = "100GB"

[network]
listen_addresses = [
  "/ip4/0.0.0.0/tcp/8080",
  "/ip6/::/tcp/8080"
]
announce_addresses = []
bootstrap_peers = [
  "/ip4/bootstrap.chiral.network/tcp/4001/p2p/QmBootstrap"
]

[file_sharing]
# Local path for seeding files
path = "/var/lib/chiral/files"
chunk_size = 262144

[api]
enabled = true
port = 8081
max_upload_size = "100MB"

[dht]
protocol = "/chiral/kad/1.0.0"
bucket_size = 20
```

### 3. DHT Configuration (Decentralized)

No centralized servers required - peer discovery handled via DHT

# API

API_PORT=3000
API_RATE_LIMIT=1000
API_CORS_ORIGIN=\*

# Security

JWT_SECRET=your-secret-key-here
ENCRYPTION_KEY=your-encryption-key-here

# Monitoring

METRICS_ENABLED=true
METRICS_PORT=9090

````

## Network Setup

### 1. Genesis Configuration

#### Create Genesis Block

```bash
# Generate genesis configuration for Ethereum network
chiral-node tools genesis \
  --chain-id 9001 \
  --network-id 9001 \
  --timestamp $(date +%s) \
  --difficulty 0x20000 \
  --gas-limit 0x7A1200 \
  --alloc accounts.json \
  --output genesis.json
````

#### accounts.json

```json
{
  "0x1000000000000000000000000000000000000001": {
    "balance": "0x6c6b935b8bbd400000",
    "nonce": "0x0"
  },
  "0x1000000000000000000000000000000000000002": {
    "balance": "0x6c6b935b8bbd400000",
    "nonce": "0x0"
  }
}
```

### 2. Bootstrap Nodes

#### Deploy Bootstrap Node

```bash
# Start bootstrap node with Geth
geth --datadir ./bootnode-data \
  --networkid 98765 \
  --port 30304 \
  --http --http.port 8546 \
  --ws --ws.port 8547 \
  --nodiscover \
  --verbosity 3

# Get enode address for other nodes
geth attach ./bootnode-data/geth.ipc --exec admin.nodeInfo.enode
```

#### Configure DNS Seeds

```
# Add to DNS records
_chiral._tcp.seed.example.com. IN SRV 0 0 30304 seed1.example.com.
_chiral._tcp.seed.example.com. IN SRV 0 0 30304 seed2.example.com.
```

### 3. Firewall Configuration

#### Required Ports

```bash
# Blockchain
sudo ufw allow 30304/tcp comment 'Chiral P2P'
sudo ufw allow 8546/tcp comment 'Chiral RPC'
sudo ufw allow 8547/tcp comment 'Chiral WebSocket'

# Storage
sudo ufw allow 8080/tcp comment 'File Transfer'
sudo ufw allow 4001/udp comment 'DHT'

# DHT P2P (no central server needed)

# Enable firewall
sudo ufw enable
```

## Deployment Scenarios

### 1. Single Node Development

```bash
# Start dev node with Geth
geth --datadir ./dev-data \
  --networkid 98765 \
  --mine --miner.threads 1 \
  --http --http.port 8546 \
  --ws --ws.port 8547 \
  --dev --dev.period 14 \
  console
```

### 2. Multi-Node Test Network

#### Node 1 (Full Node + Mining)

```bash
geth --datadir ./node1 \
  --networkid 98765 \
  --port 30304 \
  --mine --miner.threads 2 \
  --miner.etherbase 0xYourAddress \
  --http --http.port 8546 \
  --ws --ws.port 8547
```

#### Node 2 (Seeding Peer)

**Note**: This is a regular peer that happens to seed files. All nodes are equal and can seed files.

```bash
# Start Geth node
geth --datadir ./node2 \
  --networkid 98765 \
  --port 30305 \
  --bootnodes "enode://node1-id@node1-ip:30304" \
  --http --http.port 8547 \
  --ws --ws.port 8548

# Run peer service (for file sharing/seeding)
chiral-peer \
  --ethereum-rpc http://localhost:8547 \
  --max-capacity 500GB \
  --data-dir ./node2/files
```

#### Node 3 (Light Client)

```bash
geth --datadir ./node3 \
  --networkid 98765 \
  --port 30306 \
  --syncmode "light" \
  --bootnodes "enode://node1-id@node1-ip:30304" \
  --http --http.port 8548 \
  --ws --ws.port 8549
```

### 3. Production Cluster

#### Kubernetes Deployment

```yaml
# chiral-deployment.yaml
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: chiral-node
spec:
  serviceName: chiral
  replicas: 3
  selector:
    matchLabels:
      app: chiral
  template:
    metadata:
      labels:
        app: chiral
    spec:
      containers:
        - name: chiral
          image: chiralnetwork/node:latest
          ports:
            - containerPort: 30304
              name: p2p
            - containerPort: 8546
              name: rpc
          volumeMounts:
            - name: data
              mountPath: /data
          resources:
            requests:
              memory: "4Gi"
              cpu: "2"
            limits:
              memory: "8Gi"
              cpu: "4"
  volumeClaimTemplates:
    - metadata:
        name: data
      spec:
        accessModes: ["ReadWriteOnce"]
        resources:
          requests:
            storage: 100Gi
```

#### Deploy to Kubernetes

```bash
kubectl apply -f chiral-deployment.yaml
kubectl apply -f chiral-service.yaml
kubectl apply -f chiral-ingress.yaml
```

## Monitoring & Logging

### 1. Prometheus Metrics

#### prometheus.yml

```yaml
global:
  scrape_interval: 15s

scrape_configs:
  - job_name: "chiral-blockchain"
    static_configs:
      - targets: ["localhost:9090"]
        labels:
          service: "blockchain"

  - job_name: "chiral-storage"
    static_configs:
      - targets: ["localhost:9091"]
        labels:
          service: "storage"
```

### 2. Grafana Dashboard

#### Key Metrics to Monitor

```json
{
  "dashboard": {
    "title": "Chiral Network Dashboard",
    "panels": [
      {
        "title": "Block Height",
        "target": "chiral_blockchain_height"
      },
      {
        "title": "Peer Count",
        "target": "chiral_p2p_peers"
      },
      {
        "title": "Storage Used",
        "target": "chiral_storage_bytes_used"
      },
      {
        "title": "Network Bandwidth",
        "target": "rate(chiral_network_bytes_total[5m])"
      }
    ]
  }
}
```

### 3. Log Aggregation

#### Filebeat Configuration

```yaml
filebeat.inputs:
  - type: log
    paths:
      - /var/log/chiral/*.log
    multiline.pattern: '^\d{4}-\d{2}-\d{2}'
    multiline.negate: true
    multiline.match: after

output.elasticsearch:
  hosts: ["elasticsearch:9200"]
  index: "chiral-%{+yyyy.MM.dd}"
```

### 4. Alerting Rules

#### alerts.yml

```yaml
groups:
  - name: chiral_alerts
    rules:
      - alert: NodeDown
        expr: up{job="chiral"} == 0
        for: 5m
        annotations:
          summary: "Node {{ $labels.instance }} is down"

      - alert: LowDiskSpace
        expr: chiral_storage_bytes_free < 10737418240
        for: 10m
        annotations:
          summary: "Low disk space on {{ $labels.instance }}"

      - alert: HighMemoryUsage
        expr: chiral_memory_usage_percent > 90
        for: 5m
        annotations:
          summary: "High memory usage on {{ $labels.instance }}"
```

## Maintenance

### 1. Backup Procedures

#### Automated Backup Script

```bash
#!/bin/bash
# backup.sh

BACKUP_DIR="/backup/chiral"
DATE=$(date +%Y%m%d_%H%M%S)

# Backup blockchain data
tar -czf $BACKUP_DIR/blockchain_$DATE.tar.gz /var/lib/chiral/blockchain

# Backup seeding files (if desired)
tar -czf $BACKUP_DIR/files_$DATE.tar.gz /var/lib/chiral/files

# Backup configuration
tar -czf $BACKUP_DIR/config_$DATE.tar.gz /etc/chiral

# Upload to S3
aws s3 cp $BACKUP_DIR/ s3://backup-bucket/chiral/$DATE/ --recursive

# Clean old backups
find $BACKUP_DIR -mtime +30 -delete
```

#### Restore Process

```bash
# Stop services
systemctl stop chiral-node

# Restore blockchain
tar -xzf blockchain_backup.tar.gz -C /

# Restore seeding files (if backed up)
tar -xzf files_backup.tar.gz -C /

# Start services
systemctl start chiral-node
```

### 2. Updates & Upgrades

#### Rolling Update Process

```bash
# 1. Update one node at a time
kubectl set image statefulset/chiral-node chiral=chiralnetwork/node:v2.0.0

# 2. Wait for sync
kubectl wait --for=condition=ready pod/chiral-node-0

# 3. Continue with next node
kubectl rollout status statefulset/chiral-node
```

#### DHT Network Initialization

```bash
# Initialize DHT routing table
chiral-cli dht bootstrap

# Check DHT connectivity
chiral-cli dht status
```

### 3. Performance Tuning

#### System Optimization

```bash
# Increase file descriptors
echo "* soft nofile 65536" >> /etc/security/limits.conf
echo "* hard nofile 65536" >> /etc/security/limits.conf

# TCP tuning
sysctl -w net.core.rmem_max=134217728
sysctl -w net.core.wmem_max=134217728
sysctl -w net.ipv4.tcp_rmem="4096 87380 134217728"
sysctl -w net.ipv4.tcp_wmem="4096 65536 134217728"

# Disk I/O optimization
echo noop > /sys/block/sda/queue/scheduler
echo 0 > /sys/block/sda/queue/rotational
```

#### Application Tuning

```toml
# Optimize blockchain performance
[performance]
cache_size = 4096
max_peers = 100
transaction_pool_size = 10000
parallel_processing = true

# Optimize storage
[storage_performance]
concurrent_transfers = 50
chunk_cache_size = 1000
compression = true
```

## Troubleshooting

### Common Issues

#### 1. Node Won't Sync

```bash
# Check peer connections
chiral-cli peers list

# Reset peer connections
rm -rf /var/lib/chiral/blockchain/network/
systemctl restart chiral-node

# Force sync from specific peer
chiral-cli admin addPeer "enode://..."
```

#### 2. File Seeding Issues

```bash
# Check seeding status
chiral-cli files status

# Repair corrupted chunks
chiral-cli files repair --verify

# Reindex seeding files
chiral-cli files reindex
```

#### 3. High Resource Usage

```bash
# Profile CPU usage
perf record -F 99 -p $(pgrep chiral-node) -g -- sleep 30
perf report

# Analyze memory
pmap -x $(pgrep chiral-node)

# Check open files
lsof -p $(pgrep chiral-node) | wc -l
```

### Debug Commands

```bash
# Enable debug logging
chiral-node --log-level debug

# Trace specific module
chiral-node --trace p2p,storage

# Export metrics
curl http://localhost:9090/metrics

# Generate heap dump
kill -USR1 $(pgrep chiral-node)
```

## Security Hardening

### 1. Node Security

```bash
# Run as non-root user
useradd -m -s /bin/bash chiral
chown -R chiral:chiral /var/lib/chiral

# Use systemd security features
[Service]
User=chiral
Group=chiral
PrivateTmp=true
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/chiral
```

### 2. Network Security

```bash
# Enable fail2ban
apt-get install fail2ban
cp /etc/fail2ban/jail.conf /etc/fail2ban/jail.local

# Configure SSL/TLS
chiral-node \
  --rpc-tls \
  --rpc-tls-cert /etc/ssl/certs/chiral.crt \
  --rpc-tls-key /etc/ssl/private/chiral.key
```

### 3. Access Control

```nginx
# Nginx reverse proxy with auth
server {
    listen 443 ssl;
    server_name rpc.chiral.network;

    ssl_certificate /etc/ssl/certs/chiral.crt;
    ssl_certificate_key /etc/ssl/private/chiral.key;

    location / {
        auth_basic "Restricted";
        auth_basic_user_file /etc/nginx/.htpasswd;

        proxy_pass http://localhost:8546;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

## Disaster Recovery

### 1. Backup Strategy

- **Daily:** Configuration files, DHT state
- **Weekly:** Full blockchain snapshot
- **Monthly:** Complete system backup

### 2. Recovery Time Objectives

- **Critical Services:** < 1 hour
- **Full Network:** < 4 hours
- **Complete Data:** < 24 hours

### 3. Emergency Procedures

```bash
# Emergency shutdown
ansible all -m shell -a "systemctl stop chiral-node"

# Data verification
chiral-cli verify --check-integrity

# Emergency fork
chiral-node --fork-block 12345 --override
```

## Support & Resources

### Documentation

- Deployment Docs: https://github.com/chiral-network/chiral-network/blob/main/docs/08-deployment-guide.md
- API Reference:   https://github.com/chiral-network/chiral-network/blob/main/docs/05-api-documentation.md
- Troubleshooting: N/A

### Community Support

TBD

### Professional Support

TBD
