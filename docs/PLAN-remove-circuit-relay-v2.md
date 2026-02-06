# Plan: Remove Circuit Relay v2, AutoRelay, and DCUtR

## Context

The project wants to completely remove Circuit Relay v2, AutoRelay, and DCUtR (Direct Connection Upgrade through Relay) functionality from the v1 chiral-network codebase. Chapter 8.3 of the chiral-network-book.md documents this feature. The removal spans the Rust backend, TypeScript/Svelte frontend, locale files, documentation, tests, and a standalone relay daemon module.

## Scope Summary

- **Rust backend**: ~6 files to modify (dht.rs, models.rs, main.rs, headless.rs, tui.rs, repl.rs), Cargo.toml, proxy_server.rs
- **Frontend**: ~8 files to modify/delete (dht.ts, stores.ts, Network.svelte, Settings.svelte, Upload.svelte, NetworkQuickActions.svelte, Reputation.svelte)
- **Delete entirely**: relay/ directory, relayErrorService.ts, RelayErrorMonitor.svelte, RelayReputationLeaderboard.svelte, 2 docs
- **Documentation**: ~6 docs to modify/delete
- **Tests**: 3 test files to modify
- **Locale files**: ~10 locale JSON files to clean relay/dcutr keys

## What Gets Removed

- Circuit Relay v2 client and server behaviors in libp2p swarm
- DCUtR hole-punching protocol
- AutoRelay automatic relay discovery
- Relay server mode (user's node acting as relay)
- Relay reputation tracking and leaderboard
- Relay error monitoring service
- All relay/dcutr UI sections, settings, and metrics
- Standalone relay daemon binary (relay/ directory)

## What Gets Preserved

- AutoNAT v2 (reachability detection - independent of relay)
- UPnP port mapping
- SOCKS5 proxy support
- mDNS local peer discovery
- Kademlia DHT
- All file transfer protocols
- Core P2P networking (noise, yamux, tcp, identify, ping)

## Implementation Steps

### Step 1: Cargo.toml - Remove libp2p features
**File**: `src-tauri/Cargo.toml`
- Remove `"relay"` and `"dcutr"` from libp2p features (line 57)

### Step 2: Rust Backend - dht/models.rs
**File**: `src-tauri/src/dht/models.rs`
- Remove autorelay fields from DhtMetrics struct (~3 fields)
- Remove DCUtR fields from DhtMetrics struct (~6 fields)
- Remove matching fields from DhtMetricsSnapshot struct
- Remove serialization code for these fields

### Step 3: Rust Backend - dht.rs (largest change)
**File**: `src-tauri/src/dht.rs`
- Remove relay/dcutr imports
- Remove relay_client, relay_server, dcutr from DhtBehaviour struct
- Remove DiscoverRelays from SwarmCommand enum
- Remove RelayState struct
- Remove relay fields from ProxyManager
- Remove relay/dcutr metric fields from DhtMetrics initialization
- Remove RelayClient event handler (~60 lines)
- Remove RelayServer event handler (~150 lines)
- Remove DCUtR event handler and handle_dcutr_event() function
- Remove periodic relay discovery in event loop
- Remove relay config fields from DhtConfig
- Remove relay/dcutr behavior creation in swarm setup
- Remove autorelay_history() method
- Remove is_relay_candidate() helper
- Remove relay_capable_peers tracking
- Remove create_circuit_relay_address functions

### Step 4: Rust Backend - main.rs
**File**: `src-tauri/src/main.rs`
- Remove relay-related AppState fields (relay_reputation, relay_aliases, autorelay_last_enabled/disabled)
- Remove enable_autorelay, enable_relay_server, preferred_relays parameters from start_dht command
- Remove relay DhtConfig builder calls
- Remove autorelay history tracking code
- Remove relay reputation event handling
- Remove Tauri commands: get_relay_reputation_stats, set_relay_alias, get_relay_alias
- Remove RelayNodeStats and RelayReputationStats structs
- Remove field initializations in AppState constructor

### Step 5: Rust Backend - headless.rs
**File**: `src-tauri/src/headless.rs`
- Remove CLI args: enable_relay, show_dcutr, disable_autorelay, relay
- Remove autorelay DhtConfig builder calls
- Remove CHIRAL_DISABLE_AUTORELAY environment variable handling
- Remove DCUtR display blocks
- Remove log_dcutr_snapshot() function

### Step 6: Rust Backend - tui.rs and repl.rs
**Files**: `src-tauri/src/tui.rs`, `src-tauri/src/repl.rs`
- Remove relay/DCUtR display formatting in tui.rs
- Remove relay/dcutr display, JSON output, and CSV columns in repl.rs

### Step 7: Rust Backend - net/proxy_server.rs
**File**: `src-tauri/src/net/proxy_server.rs`
- Remove RelayClientBehaviour import and usage
- Simplify swarm setup without relay client

### Step 8: Frontend - dht.ts
**File**: `src/lib/dht.ts`
- Remove from DhtConfig: enableAutorelay, preferredRelays, enableRelayServer, relayServerAlias
- Remove from DhtHealth: all autorelay fields (lines 103-122), all dcutr fields (lines 124-130)
- Update start_dht_node invoke call to not pass relay params

### Step 9: Frontend - stores.ts
**File**: `src/lib/stores.ts`
- Remove from AppSettings: enableAutorelay, preferredRelays, enableRelayServer, relayServerAlias
- Remove from default settings values

### Step 10: Frontend - Delete relay service and components
**Delete entirely**:
- `src/lib/services/relayErrorService.ts`
- `src/lib/components/RelayErrorMonitor.svelte`
- `src/lib/components/RelayReputationLeaderboard.svelte`

### Step 11: Frontend - Network.svelte
**File**: `src/pages/Network.svelte`
- Remove relayErrorService import and usage
- Remove RelayErrorMonitor import and component
- Remove autorelayToggling, relayServer* state variables
- Remove setAutorelay(), toggleRelayServer(), saveRelayServerAlias() functions
- Remove relay health check initialization
- Remove relayErrorService.syncFromHealthSnapshot() calls
- Remove DCUtR Hole-Punching card UI section
- Remove Relay Server configuration section
- Remove AutoRelay Client section
- Remove autorelayEnabled prop from NetworkQuickActions
- Remove on:toggleAutorelay handler

### Step 12: Frontend - NetworkQuickActions.svelte
**File**: `src/lib/components/network/NetworkQuickActions.svelte`
- Remove autorelayEnabled prop
- Remove handleToggleAutorelay function
- Remove AutoRelay toggle button

### Step 13: Frontend - Settings.svelte
**File**: `src/pages/Settings.svelte`
- Remove enableAutorelay from privacy snapshot type and usage
- Remove relay server config from settings payload
- Remove autorelay toggle from privacy mode logic

### Step 14: Frontend - Upload.svelte
**File**: `src/pages/Upload.svelte`
- Remove NAT relay info banner

### Step 15: Frontend - Reputation.svelte
**File**: `src/pages/Reputation.svelte`
- Remove RelayReputationLeaderboard import and component
- Remove showRelayLeaderboard toggle and persistence
- Remove myRelayStats state and loadMyRelayStats() function
- Remove "My Relay Status" UI section
- Remove relay leaderboard section

### Step 16: Locale Files
**Files**: All files in `src/locales/*.json` (en, es, ru, zh, ko, fr, pt, hi, ar, bn)
- Remove all `autorelay.*` keys
- Remove all `relay.*` keys
- Remove all `dcutr.*` / `network.dht.dcutr.*` keys
- Remove `network.quickActions.toggleAutorelay.*` keys

### Step 17: Delete relay/ Directory
**Delete entirely**: `relay/` directory (standalone relay daemon)

### Step 18: Tests
- `src-tauri/tests/nat_traversal_e2e_test.rs`: Remove test_dcutr_enabled()
- `src-tauri/tests/nat_traversal_test.rs`: Remove test_autorelay_can_be_disabled(), update settings tests
- `tests/dht.test.ts`: Remove relay/autorelay/dcutr test cases

### Step 19: Documentation
- **Delete**: `docs/RELAY_SERVER_TOGGLE.md`, `docs/RELAY_ERROR_PROTOCOL.md`
- **Modify**: `docs/nat-traversal.md` - Remove relay sections, keep AutoNAT/UPnP
- **Modify**: `docs/chiral-network-book.md` - Remove chapter 8.3 and relay references
- **Modify**: `CLAUDE.md` - Remove relay references
- **Modify**: Other docs (api-documentation.md, code-reading-guide.md, etc.) - Remove relay mentions

## Verification

1. `cd src-tauri && cargo build` - Verify Rust compilation
2. `cd .. && npm run build` - Verify frontend build (if applicable)
3. `cd src-tauri && cargo test` - Run Rust tests
4. Search for orphaned relay/dcutr references: `grep -r "relay\|dcutr\|autorelay" --include="*.rs" --include="*.ts" --include="*.svelte" src-tauri/src/ src/`
