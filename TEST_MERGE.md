# Thorough Testing Guide for NAT AutoRelay Merge

This guide provides comprehensive testing steps for the merged AutoRelay + DCUtR features.

## ✅ Pre-flight Checks (COMPLETED)

- [x] Rust unit tests: 21/24 passed (3 upstream stream_auth tests failing - not related to merge)
- [x] Build succeeds: `cargo build` ✓
- [x] No merge conflicts
- [x] Code compiles

## 🧪 Manual Testing Steps

### 1. Backend Compilation Tests

```bash
cd ~/cse416/chiral-network/src-tauri

# Test debug build
cargo build
# Expected: Build succeeds with warnings only

# Test release build (takes ~3-5 min)
cargo build --release
# Expected: Build succeeds

# Run specific tests
cargo test dht:: --lib
# Expected: DHT tests pass

cargo test file_transfer:: --lib
# Expected: File transfer tests pass
```

### 2. DHT Service Initialization Test

**Test AutoRelay parameters are correctly passed:**

```bash
# Start in headless mode with verbose logging
cd ~/cse416/chiral-network/src-tauri
RUST_LOG=info cargo run -- --headless --dht-port 4001

# Check logs for:
# - "DHT service started"
# - No errors about missing parameters
# - AutoNAT initialization messages
```

**Expected Output:**
```
✓ DHT service started on port 4001
✓ AutoNAT: disabled (default in headless)
✓ AutoRelay: disabled (default in headless)
✓ Listening on addresses: [...]
```

Press Ctrl+C to stop.

### 3. GUI Application Test

**Test that the app launches and UI renders properly:**

```bash
cd ~/cse416/chiral-network

# Install frontend dependencies (if needed)
npm ci || npm install

# Start the app in dev mode
npm run tauri dev
```

**Manual UI Checks:**

#### Settings Page:
1. Navigate to **Settings** page
2. Scroll to **NAT Traversal** section
3. Verify you see:
   - ✓ "Enable AutoNAT" toggle
   - ✓ "AutoNAT Probe Interval" field
   - ✓ "AutoNAT Servers" textarea
   - ✓ "Enable AutoRelay (Circuit Relay v2)" toggle ← **YOUR FEATURE**
   - ✓ "Preferred Relay Nodes" textarea ← **YOUR FEATURE**

#### Test AutoRelay Settings:
1. Enable "Enable AutoRelay" toggle
2. Add a relay address (e.g., `/ip4/127.0.0.1/tcp/4002/p2p/12D3K...`)
3. Click "Save Settings"
4. Verify no errors in console
5. **Screenshot this page** for documentation

#### Network → DHT Page:
1. Click **Network** in sidebar
2. Click **DHT** tab
3. Click "Start DHT"
4. Wait for DHT to initialize (~5-10 seconds)
5. Verify you see cards for:
   - ✓ NAT Reachability Status
   - ✓ **Relay Status** ← **YOUR FEATURE** (should show "AutoRelay Disabled" if not configured)
   - ✓ **DCUtR Hole-Punching** ← **UPSTREAM FEATURE**

6. **Screenshot the DHT page** showing all metric cards

### 4. Test AutoRelay Functionality (If you have a relay server)

**Only if you have access to a relay server:**

1. In Settings, enable AutoRelay and add relay address
2. Start DHT
3. Wait 30 seconds
4. Check Network → DHT page
5. Relay Status card should show:
   - "AutoRelay Enabled"
   - Active relay peer ID (if connected)
   - Reservation status

### 5. Test Settings Persistence

```bash
# Test 1: Save settings
1. Go to Settings
2. Enable AutoRelay
3. Add relay: /ip4/127.0.0.1/tcp/4002/p2p/test123
4. Click Save
5. Close app

# Test 2: Reload and verify
1. Reopen app
2. Go to Settings
3. Verify:
   - ✓ AutoRelay is still enabled
   - ✓ Relay address is still there
```

### 6. Test Frontend-Backend Integration

**Verify data flows correctly:**

```bash
# In browser console (when app is running):
# Check stores have correct structure

# Should show AutoRelay fields:
window.electronAPI.invoke('get_dht_health').then(console.log)

# Expected output should include:
{
  autorelayEnabled: false,
  activeRelayPeerId: null,
  relayReservationStatus: null,
  lastReservationSuccess: null,
  lastReservationFailure: null,
  reservationRenewals: 0,
  reservationEvictions: 0,
  dcutrEnabled: false,
  dcutrHolePunchAttempts: 0,
  dcutrHolePunchSuccesses: 0,
  // ... other fields
}
```

### 7. Test Headless Mode with All Parameters

```bash
cd ~/cse416/chiral-network/src-tauri

# Test with AutoNAT and verbose output
cargo run -- \
  --headless \
  --dht-port 4001 \
  --show-reachability \
  --show-dcutr

# Let it run for 60 seconds, then Ctrl+C

# Check logs for:
# - ✓ Reachability status printed
# - ✓ DCUtR metrics printed (should be 0s)
# - ✓ No panics or errors
```

### 8. Build Production Binary

```bash
cd ~/cse416/chiral-network

# Full production build
npm run tauri build

# Expected output:
# - Frontend builds successfully
# - Rust compiles in release mode
# - Binary created in src-tauri/target/release/
```

## 🐛 Known Issues (From Upstream)

The following test failures are from upstream `stream_auth` module (NOT your changes):
- `stream_auth::tests::test_authenticated_chunk` ❌
- `stream_auth::tests::test_sequence_verification` ❌
- `stream_auth::tests::test_sign_and_verify` ❌

These failures exist in upstream/main and are not caused by your merge.

## ✅ Success Criteria

Your merge is successful if:

- [x] `cargo build` completes without errors
- [ ] App launches via `npm run tauri dev`
- [ ] Settings page shows AutoRelay configuration options
- [ ] Network → DHT page shows Relay Status card
- [ ] Network → DHT page shows DCUtR card (from upstream)
- [ ] Settings can be saved and persisted
- [ ] DHT starts without errors when AutoRelay is enabled/disabled
- [ ] No runtime panics or crashes
- [ ] Translation keys are present for all UI elements

## 📸 Screenshots to Capture

For PR documentation, take screenshots of:

1. **Settings Page** - NAT Traversal section showing:
   - AutoNAT settings
   - AutoRelay toggle and relay input ← YOUR FEATURE

2. **Network → DHT Page** showing:
   - Reachability card
   - Relay Status card ← YOUR FEATURE
   - DCUtR card ← UPSTREAM FEATURE

3. **Console Output** - No errors when:
   - Starting DHT
   - Saving settings
   - Stopping DHT

## 🔍 What to Look For

### ✅ GOOD Signs:
- DHT starts successfully
- All UI cards render
- Settings save/load correctly
- No console errors
- Warnings only (no hard errors)

### ❌ BAD Signs:
- Runtime panics
- "cannot find value" errors
- Missing UI elements
- Settings not persisting
- DHT fails to start

## 🚀 Quick Smoke Test (5 minutes)

If you want a quick validation:

```bash
# 1. Build
cd ~/cse416/chiral-network/src-tauri && cargo build

# 2. Launch app
cd ~/cse416/chiral-network && npm run tauri dev

# 3. In the app:
# - Go to Settings → Enable AutoRelay → Save
# - Go to Network → Start DHT
# - Verify no errors in console
# - Check that Relay Status card appears

# 4. If steps 1-3 work ✓ → Merge is good!
```

## 📊 Test Results Template

Copy this and fill in your results:

```markdown
## Test Results - NAT AutoRelay Merge

**Date:** [DATE]
**Branch:** feat/nat-autorelay-behavior
**Commit:** 46b482c

### Backend Tests
- [ ] cargo build: PASS/FAIL
- [ ] cargo test --lib: 21/24 tests pass
- [ ] DHT initialization: PASS/FAIL

### Frontend Tests
- [ ] App launches: PASS/FAIL
- [ ] Settings page renders: PASS/FAIL
- [ ] AutoRelay toggle visible: PASS/FAIL
- [ ] Relay input visible: PASS/FAIL
- [ ] Network DHT page renders: PASS/FAIL
- [ ] Relay Status card visible: PASS/FAIL
- [ ] DCUtR card visible: PASS/FAIL

### Integration Tests
- [ ] Settings persistence: PASS/FAIL
- [ ] DHT starts with AutoRelay enabled: PASS/FAIL
- [ ] DHT starts with AutoRelay disabled: PASS/FAIL
- [ ] No runtime errors: PASS/FAIL

### Issues Found
[List any issues here]

### Screenshots
[Attach screenshots here]
```

---

**Need Help?**
- Check browser console: F12 → Console tab
- Check Rust logs: Look for errors in terminal
- Verify settings file: `~/.config/chiral-network/` (or equivalent on your OS)
