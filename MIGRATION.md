# Migration Guide: Svelte 5 & Tauri 2.0 Stable + DHT Improvements

## Overview
This document outlines the migration from:
- Svelte 4.2.19 → Svelte 5.0
- Tauri 2.0 beta → Tauri 2.1 stable
- DHT networking improvements for stable P2P connections

## Changes Made

### Package.json Updates
```json
// Updated dependencies:
"svelte": "^5.0.0"                    // was ^4.2.19
"@tauri-apps/api": "^2.1.1"           // was ^2.2.0
"@tauri-apps/plugin-shell": "^2.0.0"  // was 2.0.1

// Updated devDependencies:
"@sveltejs/vite-plugin-svelte": "^4.0.0"  // was ^3.0.0
"@tauri-apps/cli": "^2.1.0"               // was 2.2.4
"svelte-check": "^4.0.0"                  // was ^3.6.0
"typescript": "^5.7.0"                    // was ^5.0.0
"vite": "^5.4.0"                         // was ^5.0.0
```

### Cargo.toml Updates
```toml
# Updated to stable versions:
tauri-build = { version = "2.0", features = [] }  # was 2.0.0-beta.11
tauri = { version = "2.1", features = ["macos-private-api"] }  # was 2.0.0-beta.14
tauri-plugin-process = "2.0"  # was tauri-plugin-app 2.0.0-alpha.2
tauri-plugin-os = "2.0"       # was 2.0.0-beta.3
tauri-plugin-shell = "2.0"    # was 2.0.0-beta.3
```

### Code Changes
- Updated `src-tauri/src/main.rs`: Changed `tauri_plugin_app` to `tauri_plugin_process`

## Installation Steps

1. **Clean install dependencies:**
```bash
# Remove old dependencies
rm -rf node_modules package-lock.json
rm -rf src-tauri/target

# Install new dependencies
npm install

# Build Rust dependencies
cd src-tauri
cargo build
cd ..
```

2. **Run Svelte 5 migration (optional but recommended):**
```bash
npx sv migrate svelte-5
```

This will automatically update your Svelte components to use the new Svelte 5 syntax where beneficial.

## Svelte 5 Key Changes

### Runes (New Reactivity System)
Svelte 5 introduces runes for explicit reactivity:

**Old (Svelte 4):**
```javascript
let count = 0;
$: doubled = count * 2;
```

**New (Svelte 5 - optional):**
```javascript
let count = $state(0);
let doubled = $derived(count * 2);
```

### Important Notes:
- **Backwards Compatibility**: Svelte 5 is mostly backwards compatible with Svelte 4 syntax
- **Gradual Migration**: You can migrate components one at a time
- **Performance**: Svelte 5 offers better performance and smaller bundle sizes

## Tauri 2.0 Stable Benefits

### What's New:
1. **Mobile Support**: Full iOS and Android support (if needed in future)
2. **Improved Security**: Better isolation and permission system
3. **Stable APIs**: No more breaking changes from beta versions
4. **Better Performance**: Optimized runtime and smaller binaries
5. **Plugin Ecosystem**: Mature plugin system with stable APIs

### Plugin Changes:
- `tauri-plugin-app` → `tauri-plugin-process` (handles app lifecycle)
- All plugins now at stable 2.0 versions

## DHT Networking Improvements

### What Was Fixed:

1. **Connection Stability Issues:**
   - **Problem**: Clients were disconnecting immediately after connecting ("Error(Right(Closed))")
   - **Solution**: Added libp2p identify protocol for proper protocol negotiation

2. **Keep-Alive Timeout Issues:**
   - **Problem**: Connections timing out after 60 seconds of inactivity
   - **Solutions**:
     - Extended idle connection timeout from 60 to 300 seconds (5 minutes)
     - Added periodic Kademlia bootstrap every 30 seconds
     - Implemented automatic bootstrap on initial connection

### Technical Changes to DHT (`src-tauri/src/dht.rs`):

```rust
// Added identify protocol
use libp2p::identify;

// Updated NetworkBehaviour to include identify
#[derive(NetworkBehaviour)]
pub struct DhtBehaviour {
    kademlia: Kademlia<MemoryStore>,
    identify: identify::Behaviour,
}

// Set Kademlia to server mode
kademlia.set_mode(Some(Mode::Server));

// Extended idle timeout
.with_swarm_config(|c| c
    .with_idle_connection_timeout(Duration::from_secs(300))
)

// Added periodic bootstrap
let mut bootstrap_interval = tokio::time::interval(Duration::from_secs(30));
```

### Benefits:
- **Stable Connections**: Peers maintain long-lived connections without dropouts
- **Protocol Negotiation**: Nodes properly identify and communicate capabilities
- **Automatic Recovery**: Periodic bootstrap maintains network connectivity
- **Better Routing**: Peers are properly added to Kademlia routing tables

## Testing Checklist

After migration, test these critical areas:

- [ ] Application starts without errors
- [ ] File upload/download functionality works
- [ ] DHT network connection establishes
- [ ] DHT connections remain stable (no disconnections)
- [ ] Peers can discover each other via bootstrap nodes
- [ ] Mining interface functions correctly
- [ ] Settings save and load properly
- [ ] Theme switching works
- [ ] All pages load without errors
- [ ] Tauri APIs work (file system, shell, etc.)

## Troubleshooting

### Common Issues:

1. **Build errors after update:**
```bash
# Clear all caches
npm run clean
cargo clean
npm install
cargo build
```

2. **Svelte compilation errors:**
- Check for deprecated features in the console
- Run `npx sv migrate svelte-5` for automatic fixes
- Review https://svelte.dev/docs/svelte/v5-migration-guide

3. **Tauri plugin errors:**
- Ensure all plugins are at 2.0+ versions
- Check import statements match new plugin names
- Rebuild Rust code: `cd src-tauri && cargo build`

## Rollback Instructions

If you need to rollback:

```bash
# Restore from git
git checkout -- package.json
git checkout -- src-tauri/Cargo.toml
git checkout -- src-tauri/src/main.rs

# Reinstall old dependencies
rm -rf node_modules package-lock.json
npm install
```

## Resources

- [Svelte 5 Migration Guide](https://svelte.dev/docs/svelte/v5-migration-guide)
- [Svelte 5 Documentation](https://svelte.dev/docs)
- [Tauri 2.0 Release Notes](https://v2.tauri.app/blog/tauri-20/)
- [Tauri 2.0 Documentation](https://v2.tauri.app/)

## Next Steps

1. Run `npm install` to install updated dependencies
2. Run `npm run tauri dev` to test the application
3. Consider running `npx sv migrate svelte-5` for automatic migration
4. Test all functionality thoroughly
5. Update any custom components to use Svelte 5 features as needed