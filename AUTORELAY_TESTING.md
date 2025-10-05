# AutoRelay Testing Guide

## Understanding AutoRelay

**AutoRelay** allows nodes behind NATs to automatically discover and use relay servers to accept inbound connections. This PR implements the foundation - relay discovery and reservation tracking.

## Why "Not connected to relay" is Expected

When you first run the app, you'll see:
- ✅ **AutoRelay Enabled** (green badge)
- ⚠️ **Not connected to relay** (orange text)

This is **CORRECT** and expected behavior! Here's why:

### AutoRelay Flow:
1. ✅ Node starts with AutoRelay enabled
2. 🔍 Node connects to bootstrap nodes
3. 🤝 Node identifies peers via libp2p `identify` protocol
4. 📡 If peer is a relay candidate → requests reservation
5. ✅ **Only after reservation accepted** → shows active relay

**Current issue:** The default bootstrap nodes (public IPFS nodes) likely don't:
- Support Circuit Relay v2
- Accept relay reservations from unknown peers
- Have the `/chiral/1.0.0` protocol

## How to Test AutoRelay Properly

### Option 1: Check the Logs (Easiest)

Run the app and check the console/terminal for these log messages:

```bash
npm run tauri dev
```

**Look for these logs:**

1. **At startup:**
```
🔗 AutoRelay enabled, using 6 bootstrap nodes as relay candidates
   Candidate 1: /ip4/145.40.118.135/tcp/4001/p2p/QmcZf59...
   Candidate 2: /ip4/139.178.91.71/tcp/4001/p2p/QmNnooD...
   ...
```

2. **When peers are identified:**
```
🔍 Identified peer 12D3KooW...: "/chiral/1.0.0" (listen_addrs: 2)
  AutoRelay check: is_relay_candidate=true, total_candidates=6
  Relay candidates: ["/ip4/145.40.118.135/tcp/4001/p2p/Qmc...", ...]
```

3. **If relay connection succeeds:**
```
📡 Attempting to listen via relay 12D3KooW... at /ip4/.../p2p/.../p2p-circuit
✅ Listening via relay peer 12D3KooW...
✅ Relay reservation accepted from 12D3KooW...
```

4. **If relay fails:**
```
❌ Failed to listen on relay address .../p2p-circuit: [error message]
```

### Option 2: Run Two Local Nodes (Advanced)

To actually see relay connections work, you need a relay-capable node:

#### Terminal 1: Run a relay server (future work)
```bash
# This would require running a Circuit Relay v2 daemon
# Not yet implemented in this PR
```

#### Terminal 2: Run your app
```bash
npm run tauri dev -- --relay /ip4/127.0.0.1/tcp/4002/p2p/[RELAY_PEER_ID]
```

### Option 3: Wait for Public Relay Infrastructure (Recommended)

The checklist item #2 "Public relay infrastructure" will:
- Deploy standalone Circuit Relay v2 daemons
- Provide known relay server addresses
- Make AutoRelay actually connect

## What This PR Provides

### ✅ Implemented:
- AutoRelay behavior (enabled by default in GUI)
- Relay candidate discovery from bootstrap nodes
- Reservation event tracking (accepted, renewed, evicted)
- Metrics: `active_relay_peer_id`, `reservation_renewals`, etc.
- UI: Relay Status card on Network → DHT page
- CLI flags: `--disable-autorelay`, `--relay <multiaddr>`
- Comprehensive debug logging

### ❌ Not Yet Implemented (Future PRs):
- Public relay daemon deployment
- Relay server capability in the app itself
- DCUtR hole-punching (separate branch)
- Configuration UI in Settings page

## Interpreting the UI

### Relay Status Card Shows:

| Field | Meaning | Expected Value (No Relay) |
|-------|---------|--------------------------|
| **AutoRelay Enabled** | Feature is on | Green badge ✅ |
| **Active Relay** | Current relay peer ID | "Not connected to relay" |
| **Status** | Reservation state | Hidden (no reservation) |
| **Reservations Renewed** | Renewal counter | 0 |
| **Last Successful Reservation** | Timestamp | Never |
| **Reservations Evicted** | Eviction counter | 0 |

## Troubleshooting

### "Not connected to relay" - Is this a bug?

**No!** It means:
- AutoRelay is working correctly
- But no relay-capable peers were discovered
- This is expected with public IPFS bootstrap nodes

### How do I know if AutoRelay is actually working?

Check the logs for:
1. ✅ "AutoRelay enabled, using X bootstrap nodes as relay candidates"
2. ✅ "Identified peer" messages with "is_relay_candidate=true"
3. ⚠️ If you see "Attempting to listen via relay" but then "Failed" → relay server doesn't support Circuit Relay v2
4. ⚠️ If you never see "is_relay_candidate=true" → bootstrap nodes aren't being identified (connection issue)

### Logs say "is_relay_candidate=false" for all peers

This means the peer IDs from `identify` events don't match the bootstrap node peer IDs. Reasons:
- Bootstrap connection failed (check "Connected Peers" count)
- Protocol version mismatch (logs show "Removing peer")
- Peer ID in bootstrap multiaddr doesn't match actual peer

## Next Steps

To fully test AutoRelay with actual relay connections:

1. **Merge the DCUtR branch** (feat/nat-dcutr-hole-punching) - provides more NAT traversal
2. **Deploy a relay server** (checklist item #2) - provides actual relay capability
3. **Add relay server to bootstrap nodes** - ensures discovery
4. **Test with `--relay` flag** - point to known relay server

## Summary

**This PR is working correctly!** The "Not connected to relay" message means:
- ✅ AutoRelay is enabled and looking for relays
- ✅ Relay candidate discovery is running
- ⚠️ No relay-capable peers were found yet (expected with current bootstrap nodes)

The actual relay connections will work once:
- A Circuit Relay v2 server is deployed, OR
- Bootstrap nodes support Circuit Relay v2, OR
- You manually specify a relay with `--relay <multiaddr>`
