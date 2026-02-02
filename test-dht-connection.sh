#!/bin/bash

echo "Testing DHT connection to bootstrap node..."
echo "=========================================="
echo ""

# Start a second DHT node on a different port to test connectivity
./src-tauri/target/release/chiral-network \
  --headless \
  --dht-port 4002 \
  --log-level debug \
  --show-multiaddr \
  --bootstrap "/ip4/127.0.0.1/tcp/4001"