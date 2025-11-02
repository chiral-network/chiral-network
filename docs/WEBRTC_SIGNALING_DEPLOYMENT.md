# WebRTC P2P File Transfer with Signaling Server Deployment

## Overview

WebRTC P2P infrastructure is **already implemented** in the codebase. The only missing piece is deploying the **signaling server** to Google Cloud VM to enable **real peer-to-peer connections across different networks**.

---

## What is Signaling in WebRTC?

WebRTC enables **direct peer-to-peer connections** between browsers, but there's a chicken-and-egg problem: **how do two peers find each other to establish a direct connection?**

### The Paradox

```
Peer A wants to connect to Peer B
  ↓
But Peer A doesn't know Peer B's IP address
  ↓
And Peer B doesn't know Peer A's IP address
  ↓
How do they establish a direct connection?
```

### The Solution: Signaling

**Signaling** is the process of coordinating communication between peers **before** the P2P connection exists. Think of it as an "introduction service."

### How Signaling Works

```
1. Discovery Phase (via Signaling Server)
   Peer A ──► [Signaling Server] ◄── Peer B
              "I want to connect!"

2. Exchange Connection Info (via Signaling Server)
   Peer A ──► SDP Offer ──► [Server] ──► Peer B
   Peer A ◄── SDP Answer ◄─ [Server] ◄── Peer B

   Peer A ──► ICE Candidates ──► [Server] ──► Peer B
   (These contain IP addresses discovered via STUN)

3. Direct P2P Connection Established
   Peer A ◄═══════════════════════════════► Peer B
   (Signaling server no longer involved)
```

### What Gets Exchanged via Signaling

**SDP (Session Description Protocol):**
- Media capabilities (audio, video, data channels)
- Codecs supported
- Connection preferences
- "Offer" from initiator, "Answer" from receiver

**ICE Candidates:**
- Possible network paths to reach each peer
- Public IP addresses (discovered via STUN servers)
- Local IP addresses
- Relay addresses (if using TURN servers)

### Key Points

1. **Signaling is NOT standardized by WebRTC**
   - WebRTC handles the P2P connection
   - Developers choose how to implement signaling (WebSocket, HTTP, DHT, etc.)
   - Our choice: WebSocket server

2. **Signaling only needed for setup**
   - Used only to exchange SDP/ICE candidates
   - Once P2P connection established, signaling server is no longer used
   - Actual file data transfers directly peer-to-peer

3. **Signaling server sees no file data**
   - Only sees connection metadata (SDP/ICE)
   - Cannot read, modify, or intercept file transfers
   - Very low bandwidth requirements

## Current State Analysis

### ✅ Already Fully Implemented

#### 1. **SignalingService** (`src/lib/services/signalingService.ts`)
- **Status:** Complete, production-ready
- WebSocket client implementation
- DHT/WebSocket hybrid backend (tries DHT first, falls back to WebSocket)
- Automatic reconnection with exponential backoff
- Heartbeat/ping-pong for connection health
- Peer list management with persistence
- SDP/ICE message routing
- **Current configuration:** `ws://localhost:9000` (line 62)
- **Problem:** localhost only works for same-machine testing

```typescript
// Current code (line 60-62)
constructor(opts: SignalingOptions = {}) {
  this.clientId = createClientId();
  this.wsUrl = opts.url ?? "ws://localhost:9000";  // ← localhost only!
}
```

#### 2. **WebRTC Infrastructure** (`src/lib/services/webrtcService.ts`)
- **Status:** Complete, signaling-ready
- RTCPeerConnection setup
- DataChannel creation
- SDP offer/answer handling (lines 180-207)
- ICE candidate handling (lines 159-170)
- STUN servers configured (Google, Twilio, Cloudflare)
- Signaling integration prepared (lines 162-168, 186-188)

```typescript
// WebRTC already supports signaling (line 98)
signaling?: SignalingService;

// Automatically sends offer via signaling (line 186-188)
if (signaling && peerId) {
  signaling.send({ type: "offer", sdp, to: peerId });
}
```

#### 3. **UI Integration** (`src/pages/Network.svelte`)
- **Status:** Complete, UI ready
- SignalingService instantiation (line 102)
- WebRTC session management (line 103)
- Peer discovery display (lines 104-107)
- Connection state tracking (line 109)

```typescript
// Already integrated in Network page (line 102-103)
let signaling: SignalingService;
let webrtcSession: ReturnType<typeof createWebRTCSession> | null = null;
```

#### 4. **File Transfer** (`src/lib/services/p2pFileTransfer.ts`)
- Chunk-based transfer protocol
- Progress tracking
- Currently uses libp2p backend

#### 5. **libp2p Infrastructure** (Rust backend)
- Circuit Relay v2 with AutoRelay
- Kademlia DHT for peer discovery
- AutoNAT v2 for reachability detection
- **Status:** Production-ready, deployed relay nodes available

---

### ❌ Current Problem: Local-Only Configuration

#### **Existing Signaling Server Options**

**Option A: Node.js Server** (`src/lib/services/server/signalingServer.js`)
- **Port:** 3000
- **Purpose:** Local development testing
- **Status:** Works for localhost only
- **Problem:**
  - Not production-ready
  - Cannot handle cross-network connections
  - Node.js dependency (different from Rust backend stack)

```javascript
// signalingServer.js:3
const wss = new WebSocketServer({ port: 3000 });  // localhost only
```

**Option B: Rust Server** (`src-tauri/src/signaling_server.rs`)
- **Port:** 9000
- **Purpose:** Production deployment
- **Status:** Code complete, **not deployed**
- **Advantage:**
  - Production-ready
  - Matches backend stack (Rust)
  - Better performance
  - **Ready to deploy to VM**

---

### 🚫 Why Current Setup Doesn't Work for Real P2P

**Current Scenario (Fails):**
```
Peer A (home WiFi, 192.168.1.100)
  ↓
  Tries to connect: ws://localhost:9000
  ↓
  ✅ Connects to LOCAL signaling server on Peer A's machine

Peer B (school WiFi, 10.0.0.50)
  ↓
  Tries to connect: ws://localhost:9000
  ↓
  ✅ Connects to LOCAL signaling server on Peer B's machine

❌ Problem: They're connected to DIFFERENT servers!
❌ Cannot exchange SDP/ICE candidates
❌ No P2P connection possible
```

**Required Scenario (Works):**
```
Peer A (home WiFi)
  ↓
  ws://VM_IP:9000
  ↓
      [Google Cloud VM]
      Signaling Server
  ↑
  ws://VM_IP:9000
  ↑
Peer B (school WiFi)

✅ Both connect to SAME server
✅ Can exchange SDP/ICE candidates
✅ WebRTC establishes direct P2P connection
✅ File transfer works!
```

---

## Solution: Deploy Rust Signaling Server to Google Cloud VM

### What Needs to Change

**Before (current code):**
```typescript
// signalingService.ts:62
this.wsUrl = opts.url ?? "ws://localhost:9000";  // Only works locally
```

**After (with VM deployment):**
```typescript
// In application code
const signaling = new SignalingService({
  url: 'ws://ACTUAL_VM_IP:9000'  // Accessible from anywhere
});
```

**That's literally the only code change needed!**

---

## Implementation Plan

### Step 1: Build Rust Signaling Server

The Rust server is already implemented and ready. Just build it:

```bash
cd src-tauri
cargo build --bin signaling-server --release

# Binary location
ls -lh target/release/signaling-server
```

**Why Rust server instead of Node.js:**
- ✅ Already implemented in repo
- ✅ Production-ready
- ✅ Matches backend stack (all Rust)
- ✅ Better performance and reliability
- ✅ Same codebase as main app

---

### Step 2: Deploy to Google Cloud VM

#### 2.1 Create VM Instance
```bash
gcloud compute instances create chiral-signaling \
  --zone=us-central1-a \
  --machine-type=e2-micro \
  --image-family=ubuntu-2204-lts \
  --image-project=ubuntu-os-cloud \
  --boot-disk-size=10GB
```

#### 2.2 Configure Firewall
```bash
# Allow WebSocket traffic on port 9000
gcloud compute firewall-rules create allow-webrtc-signaling \
  --allow=tcp:9000 \
  --description="WebRTC Signaling Server" \
  --target-tags=signaling

# Tag the instance
gcloud compute instances add-tags chiral-signaling \
  --zone=us-central1-a \
  --tags=signaling
```

#### 2.3 Upload Binary
```bash
gcloud compute scp target/release/signaling-server \
  chiral-signaling:~/ \
  --zone=us-central1-a
```

#### 2.4 Run on VM
```bash
# SSH into VM
gcloud compute ssh chiral-signaling --zone=us-central1-a

# Make executable and run
chmod +x ~/signaling-server
./signaling-server
```

#### 2.5 Setup Systemd Service (Optional but Recommended)
```bash
# Create service file
sudo nano /etc/systemd/system/chiral-signaling.service
```

```ini
[Unit]
Description=Chiral WebRTC Signaling Server
After=network.target

[Service]
Type=simple
User=your-username
ExecStart=/home/your-username/signaling-server
Restart=always
RestartSec=10
Environment="PORT=9000"
Environment="RUST_LOG=info"

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable chiral-signaling
sudo systemctl start chiral-signaling

# Check status
sudo systemctl status chiral-signaling
```

---

### Step 3: Update Frontend Configuration

#### Option A: Environment Variable (Recommended)
```typescript
// Create .env file
VITE_SIGNALING_URL=ws://YOUR_VM_IP:9000

// Update code to use env variable
const signaling = new SignalingService({
  url: import.meta.env.VITE_SIGNALING_URL || 'ws://localhost:9000'
});
```

#### Option B: Settings UI
Add signaling server URL to Settings page:
```typescript
// In Settings.svelte
<input
  type="text"
  bind:value={$settings.signalingServerUrl}
  placeholder="ws://YOUR_VM_IP:9000"
/>

// In app initialization
const signaling = new SignalingService({
  url: $settings.signalingServerUrl || 'ws://localhost:9000'
});
```

#### Option C: Direct Change (Quick Test)
```typescript
// signalingService.ts:62
this.wsUrl = opts.url ?? "ws://YOUR_ACTUAL_VM_IP:9000";
```

---

### Step 4: Test Cross-Network P2P

#### Local Development Test
```bash
# Terminal 1: Run signaling server
cargo run --bin signaling-server

# Terminal 2: Run app
npm run dev

# Open two browser tabs, verify connection
```

#### Production Test (After VM Deployment)
```bash
# Update signaling URL to VM IP
# Test from two different networks:

Machine A (home WiFi):
  - Open app
  - Should connect to ws://VM_IP:9000
  - See peer B in peer list

Machine B (mobile hotspot or different location):
  - Open app
  - Should connect to ws://VM_IP:9000
  - See peer A in peer list
  - Attempt P2P connection
  - Transfer file via WebRTC DataChannel

# Verify:
# 1. Both peers see each other
# 2. WebRTC connection establishes (check console)
# 3. File transfer works
```

---

## Architecture Flow

### Complete P2P File Transfer Flow

```
┌─────────────────────────────────────────────────────────────┐
│  1. Peer Discovery (libp2p DHT - Already Working)           │
└─────────────────────────────────────────────────────────────┘

User A: "Who has file-abc123?"
  ↓
[libp2p Kademlia DHT]
  ↓
Result: "Peer B (12D3KooW...) has it"


┌─────────────────────────────────────────────────────────────┐
│  2. WebRTC Signaling (Deploy to Google Cloud)               │
└─────────────────────────────────────────────────────────────┘

Peer A                 [Signaling Server]              Peer B
  │                   ws://VM_IP:9000                     │
  │                                                       │
  │─── connect() ────────►│                              │
  │                        │◄──── connect() ─────────────│
  │                        │                              │
  │─── Register ─────────►│                              │
  │   (clientId: A)        │                              │
  │                        │◄──── Register ──────────────│
  │                        │   (clientId: B)              │
  │                        │                              │
  │◄─── Peers: [B] ───────│                              │
  │                        │──── Peers: [A] ─────────────►│
  │                        │                              │
  │─── SDP Offer ────────►│                              │
  │   (to: B)              │──── SDP Offer ──────────────►│
  │                        │   (from: A)                  │
  │                        │                              │
  │                        │◄──── SDP Answer ────────────│
  │◄─── SDP Answer ───────│   (to: A)                    │
  │   (from: B)            │                              │
  │                        │                              │
  │─── ICE Candidates ───►│                              │
  │                        │──── ICE Candidates ─────────►│
  │                        │                              │
  │                        │◄──── ICE Candidates ─────────│
  │◄─── ICE Candidates ───│                              │


┌─────────────────────────────────────────────────────────────┐
│  3. STUN Hole Punching (Already Configured)                 │
└─────────────────────────────────────────────────────────────┘

Peer A                  [STUN Server]                   Peer B
  │                   (stun.l.google.com)                 │
  │                                                       │
  │─── Binding Request ───►│                             │
  │◄── "203.0.113.45" ─────│                             │
  │                                                       │
  │                        │◄─── Binding Request ────────│
  │                        │──── "198.51.100.23" ────────►│
  │                                                       │
  Both peers discover their public IPs via STUN          │


┌─────────────────────────────────────────────────────────────┐
│  4. Direct P2P Connection (WebRTC DataChannel)              │
└─────────────────────────────────────────────────────────────┘

Peer A ◄═══════ Direct WebRTC DataChannel ═══════► Peer B
         (Signaling server no longer involved)

  │──── File Chunk 1 ────────────────────────────────►│
  │──── File Chunk 2 ────────────────────────────────►│
  │──── File Chunk N ────────────────────────────────►│
```

**Key Point:** Signaling server only used in Phase 2 to exchange SDP/ICE. Actual file transfer is direct P2P.

---

## Why Both libp2p and WebRTC?

### libp2p Circuit Relay v2
- **Purpose:** Peer discovery via DHT, metadata exchange, fallback relay
- **Status:** Already deployed, production-ready
- **Protocol:** libp2p multiaddrs, Rust backend
- **Speed:** Slower (relays all data through server)
- **Success Rate:** 100% (always works)

### WebRTC with STUN
- **Purpose:** Direct peer-to-peer file transfer
- **Status:** Code complete, needs VM deployment
- **Protocol:** WebRTC DataChannel, browser native
- **Speed:** Fast (direct connection)
- **Success Rate:** ~85% (fails on symmetric NAT)

### Combined Strategy
```
1. Find peers with libp2p DHT
2. Try WebRTC direct connection (fast)
3. If WebRTC fails, use libp2p relay (fallback)
4. Result: Fast when possible, reliable always
```

---

## Testing Checklist

### Local Testing
- [ ] Build signaling server binary
- [ ] Run `cargo run --bin signaling-server`
- [ ] Open two browser tabs
- [ ] Verify both connect to localhost:9000
- [ ] Check peer list shows both clients
- [ ] Test SDP/ICE exchange in console

### VM Deployment Testing
- [ ] Deploy binary to Google Cloud VM
- [ ] Configure firewall for port 9000
- [ ] Update frontend config with VM IP
- [ ] Test from same network (verify it works)
- [ ] Test from different networks (home WiFi vs mobile hotspot)
- [ ] Verify peers discover each other
- [ ] Verify WebRTC connection establishes
- [ ] Transfer test file via WebRTC DataChannel
- [ ] Monitor VM logs for errors

### Cross-Network P2P Testing
- [ ] Peer A on home WiFi
- [ ] Peer B on mobile hotspot
- [ ] Both connect to signaling server
- [ ] Both appear in each other's peer list
- [ ] WebRTC connection state: "connected"
- [ ] File transfer completes successfully
- [ ] Check transfer speed (should be fast, direct)

---

## What We're NOT Doing

- ❌ Rewriting SignalingService (already complete)
- ❌ Rewriting WebRTC infrastructure (already complete)
- ❌ Building new file transfer protocol (already exists)
- ❌ Replacing libp2p (keep as complementary system)
- ❌ Building TURN server (STUN sufficient for 85% cases)

## What We ARE Doing

- ✅ Deploy existing Rust signaling server to Google Cloud VM
- ✅ Update one line of config (signaling URL)
- ✅ Test cross-network P2P connections
- ✅ Integrate with existing file transfer UI

---

## Next Steps

1. **Deploy Signaling Server**
   - Build Rust binary
   - Upload to Google Cloud VM
   - Configure firewall and systemd

2. **Update Configuration**
   - Add VM IP to environment variable or settings
   - Test connection from local machine

3. **Cross-Network Testing**
   - Test from two different networks
   - Verify P2P connection works
   - Measure transfer speed

4. **UI Integration**
   - Add signaling server status to Network page
   - Show WebRTC connection state
   - Display transfer method (direct vs relay)

5. **Documentation**
   - Update deployment docs with VM IP
   - Document signaling server URL configuration
   - Add troubleshooting guide

---

## Summary

**Current State:**
- ✅ SignalingService fully implemented
- ✅ WebRTC infrastructure complete
- ✅ UI integration done
- ❌ Configured for localhost only (not usable for real P2P)

**Required Action:**
- Deploy Rust signaling server to Google Cloud VM
- Update config: `ws://localhost:9000` → `ws://VM_IP:9000`

**Expected Result:**
- Direct P2P file transfers across different networks
- 85%+ connection success with STUN alone
- ~$7/month operating cost

---

## References

- Existing signaling service: `src/lib/services/signalingService.ts`
- WebRTC service: `src/lib/services/webrtcService.ts`
- Signaling server (Rust): `src-tauri/src/signaling_server.rs`
- Signaling server (Node.js, dev only): `src/lib/services/server/signalingServer.js`
- Network UI integration: `src/pages/Network.svelte`
- File transfer: `src/lib/services/p2pFileTransfer.ts`
- libp2p infrastructure: `docs/nat-traversal.md`
- Relay deployment: `relay/DEPLOYMENT.md`
