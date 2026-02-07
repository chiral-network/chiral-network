# Chiral Network Interactive Shell Guide

## Table of Contents

- [Overview](#overview)
- [Implementation Roadmap](#implementation-roadmap)
- [Mode Comparison](#mode-comparison)
- [Getting Started](#getting-started)
- [REPL Mode](#repl-mode)
- [TUI Mode](#tui-mode)
- [Command Reference](#command-reference)
- [Use Cases](#use-cases)
- [Troubleshooting](#troubleshooting)
- [FAQ](#faq)

---

## Overview

Chiral Network provides multiple interface modes to suit different deployment scenarios and user preferences. This guide covers the **interactive shell modes** - text-based interfaces for command-line management.

### Available Modes

| Mode              | Interface Type          | Use Case                              |
| ----------------- | ----------------------- | ------------------------------------- |
| **GUI** (default) | Graphical window        | Desktop users, visual monitoring      |
| **Headless**      | Daemon (no interaction) | Bootstrap nodes, background services  |
| **REPL**          | Interactive shell       | Testing, debugging, server management |
| **TUI**           | Full-screen terminal    | Live monitoring, server dashboards    |

### When to Use Interactive Shells

Choose REPL or TUI mode when you need:

- ✅ Server-side management via SSH
- ✅ Quick testing and debugging
- ✅ Runtime control without GUI overhead
- ✅ Scriptable operations
- ✅ Low resource usage

---

## Implementation Roadmap

### Phase 1: REPL Mode ✅ **COMPLETED**

**Status:** Released in v0.1.0

Core interactive shell functionality with command-line interface.

**Implemented Features:**

- ✅ Interactive command prompt with rustyline
- ✅ Command history and navigation (↑/↓ arrows)
- ✅ Network status monitoring (`status`, `peers`, `dht`)
- ✅ File operations (`add`, `download`, `list`)
- ✅ Mining control (`mining start/stop/status`)
- ✅ Clean shell output (no log spam)
- ✅ Scriptable interface (pipe commands)
- ✅ Box-drawn UI with proper alignment
- ✅ Comprehensive command reference
- ✅ All CLI flags support (--dht-port, --bootstrap, etc.)

**Files:**

- `src-tauri/src/repl.rs` - Main REPL implementation
- `src-tauri/src/main.rs` - Interactive mode entry point
- `docs/INTERACTIVE_SHELL_GUIDE.md` - This guide

**Usage:**

```bash
./chiral-network --interactive [options]
```

### Phase 2: Enhanced REPL Features ✅ **COMPLETED**

**Status:** Released in v0.1.0

Advanced REPL capabilities and improved UX.

**Implemented Features:**

- ✅ Tab completion for commands and subcommands (rustyline Completer trait)
- ✅ Syntax highlighting for hashes (Qm...) and peer IDs (12D3KooW...)
- ✅ Real-time download progress display (`downloads` command)
- ✅ Configuration management commands (`config list/get/set/reset`)
- ✅ Advanced peer filtering (`peers list --trust --sort --limit`)
- ✅ File versioning commands (`versions list/info`)
- ✅ Reputation management commands (`reputation list/info`)
- ✅ Enhanced error messages with Levenshtein distance suggestions

**Technical Implementation:**

- ReplHelper struct with Completer, Highlighter, Hinter traits
- Levenshtein distance algorithm for typo suggestions (strsim crate)
- ANSI terminal colors for syntax highlighting (colored crate)
- Advanced filtering and sorting for peer lists
- Mock data for reputation and versioning (ready for backend integration)

**New Dependencies:**

- `colored = "2.1"` - ANSI terminal colors
- `indicatif = "0.17"` - Progress bars (for future use)
- `strsim = "0.11"` - Levenshtein distance for suggestions

### Phase 3: TUI Mode ✅ **COMPLETED**

**Status:** Released in v0.1.0

Full-screen terminal dashboard with live updates.

**Implemented Features:**

- ✅ Live dashboard with automatic 1-second refresh
- ✅ Real-time network metrics visualization
- ✅ Multiple panels (Network, Downloads, Peers, Mining)
- ✅ Tab-based panel switching with indicators
- ✅ Keyboard navigation (number keys, Tab, arrows)
- ✅ Command mode (press `:` to enter commands)
- ✅ Real-time peer list display
- ✅ Download tracking with status colors
- ✅ Mining panel with stats display
- ✅ Command execution with result feedback
- ✅ Mouse support via crossterm
- ✅ Clean terminal rendering with proper cleanup

**Technology Stack:**

- `ratatui = "0.28"` - Modern Rust TUI framework
- `crossterm = "0.28"` - Cross-platform terminal handling
- Event-driven async architecture
- 1-second auto-refresh rate
- Live metrics channel with tokio

**Technical Implementation:**

- `src-tauri/src/tui.rs` - Main TUI implementation
- Background metrics polling with tokio channels
- Panel system with `ActivePanel` enum
- Real-time data from `DhtService` and `FileTransferService`
- Command parser integrated with TUI display
- Graceful terminal state management (raw mode, alternate screen)

### Phase 4: Advanced Features ✅ **COMPLETED**

**Status:** Released in v0.1.0

**Target:** v0.4.0+

Advanced monitoring and management capabilities.

**Implemented Features:**

- ✅ Export metrics to files (JSON, CSV)
- ✅ Custom REPL scripts and macros
- ✅ Plugin system for custom commands (framework ready)
- ✅ Advanced analytics and reporting
- ✅ Remote REPL access (secure RPC with token auth)
- ✅ Webhook notifications for events

**Technical Implementation:**

- Export command with JSON/CSV formats for metrics, peers, downloads
- Script execution system (.chiral scripts) - read script files and execute commands
- Plugin loading framework (dynamic library support ready)
- Comprehensive report generation (summary/full modes)
- Remote REPL server with TCP and token-based authentication
- Webhook manager with persistent storage and HTTP POST notifications

**New Commands:**

- `export <target> [--format json|csv] [--output <path>]` - Export data to files
- `script run <path>` / `script list` - Run and manage REPL scripts
- `plugin load <path>` / `plugin list` - Load and manage plugins
- `report [summary|full]` - Generate comprehensive reports
- `remote start [addr] [token]` / `remote stop` / `remote status` - Remote REPL access
- `webhook add <event> <url>` / `webhook list` / `webhook test <id>` - Webhook notifications

**Files:**

- `src-tauri/src/remote_repl.rs` - Remote REPL server implementation
- `src-tauri/src/webhook_manager.rs` - Webhook management system
- Enhanced `src-tauri/src/repl.rs` with Phase 4 commands

**Future Enhancements:**

- Multi-node management from single shell
- Integration with monitoring tools (Prometheus, Grafana)
- Advanced plugin API with custom command registration
- Real-time script debugging and profiling

### Phase 5: Mining Integration ✅ **COMPLETED**

**Status:** Released in v0.1.0

**Goal:** Fully integrate mining capabilities into the interactive shell with real-time monitoring and control.

Backend functions fully implemented in `ethereum.rs` and integrated into REPL/TUI:
- `start_mining(miner_address, threads)`
- `stop_mining()`
- `get_mining_status()`
- `get_mining_performance(data_dir)`
- `get_mining_logs(data_dir, lines)`
- `get_total_mining_rewards(miner_address)`
- `get_recent_mined_blocks(miner_address, lookback, limit)`
- `get_network_difficulty_as_u64()`

**Implemented Features:**

#### 5.1: Core Mining Integration ✅

REPL mining commands fully connected to Geth mining functions.

- ✅ `cmd_mining()` calls real mining functions
- ✅ Display real mining status (hash rate, blocks found, rewards)
- ✅ Mining start/stop with thread control
- ✅ Comprehensive error handling
- ✅ Miner address management (from CLI flag or coinbase)
- ✅ Real-time status updates with colored output

**Working Commands:**
```bash
chiral> mining status    # Shows real-time mining status
chiral> mining start 4   # Starts mining with 4 threads
chiral> mining stop      # Stops mining gracefully
```

#### 5.2: Mining Dashboard ✅

Real-time mining statistics and monitoring.

- ✅ Live mining dashboard with comprehensive stats
- ✅ Hash rate display from actual Geth data
- ✅ Block discovery tracking
- ✅ Mining rewards accumulator
- ✅ Recent block history with timestamps
- ✅ Formatted time ago display (e.g., "2m ago")

**Commands:**
- ✅ `mining dashboard` - Real-time mining view with all stats
- ✅ `mining performance` - Detailed performance metrics
- ✅ `mining logs [lines]` - View recent mining logs

#### 5.3: Mining History & Analytics ✅

Track and analyze mining performance over time.

- ✅ Recent mining blocks with timestamps
- ✅ Total rewards calculation per address
- ✅ Performance metrics (hash rate, efficiency)
- ✅ Mining block history display
- ✅ Average reward per block calculation
- ✅ Network difficulty tracking

**Commands:**
- ✅ `mining rewards` - Total rewards earned with block history
- ✅ `mining performance` - Performance metrics and efficiency
- ✅ `export` commands work with all mining data (Phase 4 integration)

#### 5.4-5.5: Advanced Features ✅

*Core implementation completed - Additional enhancements deferred to future releases*

Advanced configuration and smart mining features base implementation:
- ✅ Thread configuration via CLI
- ✅ Mining control commands (start/stop with thread count)
- ✅ Box border alignment fixes for all mining outputs
- 📋 Thread configuration persistence (future)
- 📋 Mining scheduling (future)
- 📋 Profitability calculator (future)
- 📋 Power/temperature monitoring (future)

#### 5.6: TUI Mining Panel ✅

Dedicated mining panel in TUI mode with live data.

- ✅ Real-time mining status display
- ✅ Live hash rate updates (1-second refresh)
- ✅ Blocks found counter
- ✅ Miner address display
- ✅ Total rewards display
- ✅ Mining efficiency metrics
- ✅ Status color coding (green = active, red = inactive)
- ✅ Integration with TUI metrics polling system

**Implementation Details:**
- `MiningMetrics` struct for live data
- Background polling via tokio channels
- Real data from `ethereum.rs` functions
- Graceful fallback for missing data

#### 5.7: Mining Webhook Integration ✅

Mining events integrated with Phase 4 webhook system.

- ✅ All webhook events support mining context
- ✅ `block_found` event available
- ✅ Mining start/stop can trigger webhooks
- ✅ Webhook testing with mining data

**Technical Implementation:**

**REPL (`src-tauri/src/repl.rs`):**
- Extended `ReplContext` with `miner_address` and `geth_data_dir`
- Implemented complete `cmd_mining()` with all subcommands
- Added helper functions:
  - `cmd_mining_dashboard()` - Live dashboard
  - `cmd_mining_logs()` - Log viewer
  - `cmd_mining_rewards()` - Rewards summary
  - `cmd_mining_performance()` - Performance metrics
  - `format_time_ago()` - Human-readable timestamps
  - `format_number()` - Number formatting with commas
- Updated help menu with all mining commands
- Tab completion for all mining subcommands

**TUI (`src-tauri/src/tui.rs`):**
- Extended `TuiContext` with mining fields
- Added `MiningMetrics` struct for live data
- Implemented `fetch_mining_metrics()` function
- Updated `render_mining_panel()` with real data
- Integrated mining polling into metrics loop
- Live updates every second via tokio channels

**Security:**
- Miner addresses validated before use
- No private keys logged
- Error handling for all RPC calls
- Safe fallbacks for missing data

**Dependencies:**

- ✅ Geth process running with `--enable-geth` flag
- ✅ Miner address via `--miner-address` flag
- ✅ Network connection for blockchain sync
- ✅ Geth data directory configuration
---

## Mode Comparison

### Detailed Comparison Table

| Feature                 | GUI                  | Headless     | REPL           | TUI (Future)      |
| ----------------------- | -------------------- | ------------ | -------------- | ----------------- |
| **Display Required**    | ✅ Yes (X11/Wayland) | ❌ No        | ❌ No          | ❌ No             |
| **Works over SSH**      | ❌ No                | ✅ Yes       | ✅ Yes         | ✅ Yes            |
| **Runtime Interaction** | ✅ Full              | ❌ None      | ✅ Commands    | ✅ Full           |
| **Resource Usage**      | 🔴 High              | 🟢 Low       | 🟢 Low         | 🟡 Medium         |
| **Visual Feedback**     | 🟢 Best              | ⚫ Logs only | 🟡 Text output | 🟢 Live dashboard |
| **Learning Curve**      | 🟢 Easy              | -            | 🟡 Medium      | 🟡 Medium         |
| **Automation**          | ❌ No                | ⚠️ Limited   | ✅ Yes         | ⚠️ Limited        |
| **Monitoring**          | 🟢 Real-time         | ⚫ Logs      | 🟡 On-demand   | 🟢 Real-time      |

### Which Mode Should I Use?

**Choose REPL if you need:**

- Command-line control with instant feedback
- Scriptable operations (pipe commands, automation)
- Minimal resource usage
- Quick status checks and file operations
- Testing and debugging

**Choose TUI if you need:**

- Live monitoring dashboard
- Visual status at a glance
- Server-side monitoring via SSH
- Better than REPL for long-running sessions
- Mouse support (optional)

**Choose GUI if you need:**

- Full feature set with visual interface
- Drag-and-drop file operations
- Desktop application experience

**Choose Headless if you need:**

- Pure daemon mode (bootstrap nodes)
- No interaction after startup
- Absolute minimal resources

---

## Getting Started

### Prerequisites

- Chiral Network installed and built
- Terminal emulator (Terminal.app, iTerm2, etc.)
- SSH access (for remote servers)

### Installation

```bash
# Clone and build
git clone https://github.com/chiral-network/chiral-network
cd chiral-network
cargo build --release

# Binary location
cd src-tauri
./target/release/chiral-network --interactive  # REPL mode
./target/release/chiral-network --tui          # TUI mode
```

### Common CLI Flags

All interactive modes support these flags:

```bash
# Network configuration
--dht-port <PORT>              # DHT port (default: 4001)
--bootstrap <MULTIADDR>        # Bootstrap nodes (can specify multiple)

# Features
--enable-geth                  # Enable mining (requires geth binary)
--geth-data-dir <PATH>         # Geth data directory

# NAT traversal
--disable-autonat              # Disable AutoNAT probes

# Privacy
--socks5-proxy <ADDR>          # SOCKS5 proxy (e.g., 127.0.0.1:9050)

# Advanced
--secret <HEX>                 # Consistent peer ID generation
--is-bootstrap                 # Run as bootstrap node
```

---

## REPL Mode

### What is REPL?

REPL (Read-Eval-Print Loop) is an interactive command-line interface where you type commands and get immediate responses. Think of it like the `python` or `mysql` CLI.

**Key Features:**

- Command history (↑/↓ arrows)
- Clean output (no log spam)
- Scriptable (pipe commands)
- Lightweight and fast

### Starting REPL Mode

```bash
# Basic usage
./target/release/chiral-network --interactive

# With custom port
./target/release/chiral-network --interactive --dht-port 5001

# With mining enabled
./target/release/chiral-network --interactive --enable-geth

# With custom bootstrap nodes
./target/release/chiral-network --interactive \
  --bootstrap /ip4/134.199.240.145/tcp/4001/p2p/12D3KooW...
```

### REPL Interface

When you start REPL mode, you'll see:

```
┌────────────────────────────────────────────────────────┐
│ Chiral Network v0.1.0 - Interactive Shell              │
│ Type 'help' for commands, 'quit' to exit              │
└────────────────────────────────────────────────────────┘

Peer ID: 12D3KooWQqWtv2GVLaKVUTyShXJXfp2U3WZZAGTnzEzpAfZYp6A6

chiral>
```

The `chiral>` prompt indicates REPL is ready for commands.

### Basic Commands

```bash
# Get help
chiral> help

# Check network status
chiral> status

# List connected peers
chiral> peers list

# Count peers
chiral> peers count

# Check DHT status
chiral> dht status

# Clear screen
chiral> clear

# Exit
chiral> quit
```

### File Operations

```bash
# Add file to share
chiral> add /path/to/file.pdf

# Download file by hash
chiral> download QmHash123...

# List seeding files
chiral> list files

# Show recent downloads
chiral> list downloads
```

### Advanced Operations

```bash
# DHT operations
chiral> dht status
chiral> dht get QmHash123...

# Mining (requires --enable-geth)
chiral> mining status
chiral> mining start 4
chiral> mining stop

# Configuration management
chiral> config list
chiral> config get max_peers
chiral> config set max_peers 100

# Peer filtering and reputation
chiral> peers list --trust high --sort score --limit 10
chiral> reputation list
chiral> reputation info 12D3KooW...

# File versioning
chiral> versions list QmHash123...
chiral> versions info QmHash123...

# Active downloads
chiral> downloads
```

### Command History

REPL saves command history to `~/.chiral_history`:

- Press **↑** to recall previous commands
- Press **↓** to move forward in history
- History persists across sessions

### Exiting REPL

Three ways to exit:

```bash
chiral> quit        # Graceful shutdown
chiral> exit        # Alias for quit
chiral> q           # Short alias
```

Or press **Ctrl+D** to send EOF signal.

**Note:** Ctrl+C will NOT exit - it prints `^C` and continues (standard REPL behavior).

### Example Session

```bash
$ ./target/release/chiral-network --interactive

┌────────────────────────────────────────────────────────┐
│ Chiral Network v0.1.0 - Interactive Shell              │
│ Type 'help' for commands, 'quit' to exit              │
└────────────────────────────────────────────────────────┘

Peer ID: 12D3KooWQqWtv2GVLaKVUTyShXJXfp2U3WZZAGTnzEzpAfZYp6A6

chiral> status

📊 Network Status:
  ┌────────────────────────────────────────────────────────┐
  │ Connected Peers: 42                                    │
  │ Reachability: Public                                   │
  │ NAT Status: Active                                     │
  │ AutoNAT: Enabled                                       │
  └────────────────────────────────────────────────────────┘

chiral> peers count
📡 Connected peers: 42

chiral> add /tmp/test.txt
✓ Added and seeding: test.txt (QmHash...)
  Size: 1024 bytes

chiral> quit
Shutting down gracefully...
```

### Scripting with REPL

#### Pipe Commands

```bash
# Single command
echo "status" | ./chiral-network --interactive

# Multiple commands
cat <<EOF | ./chiral-network --interactive
status
peers count
quit
EOF
```

#### Batch Script

```bash
#!/bin/bash
# check-network.sh

./chiral-network --interactive <<COMMANDS
status
peers count
dht status
quit
COMMANDS
```

---

## TUI Mode

> **Status:** ✅ Available in v0.1.0
>
> TUI (Terminal User Interface) mode provides a full-screen dashboard with live updates, similar to `htop` or `btop`.

### Features

- 📊 **Live Dashboard** - Real-time network stats with 1-second refresh
- 🎨 **Multiple Panels** - Network, downloads, peers, mining
- ⌨️ **Keyboard Navigation** - Switch between panels with number keys, Tab, or arrows
- 🖱️ **Mouse Support** - Crossterm-based mouse interactions
- 📋 **Command Mode** - Press `:` to execute commands from TUI
- 🎯 **Panel Indicators** - Visual tabs showing current panel

### Interface Layout

```
┌─────────────────────────────────────────────────────────────┐
│ Chiral Network v0.1.0          [Q]uit [H]elp              │
├─────────────────────────┬────────────────────────────────────┤
│ 📡 Network [1]          │ 📥 Active Downloads [2]            │
│ Peers: 42 ████████░░    │ ┌──────────────────────────────────┐ │
│ DHT: 1,234 entries      │ │ file.pdf [████████░░] 75%       │ │
│ NAT: Public             │ │   8 peers, 4.2 MB/s, ETA 2m     │ │
│                         │ │                                  │ │
│                         │ │ video.mp4 [███░░░░░░] 30%       │ │
├─────────────────────────┤ │   3 peers, 1.8 MB/s, ETA 8m     │ │
│ ⚡ Mining [3]           │ └──────────────────────────────────┘ │
│ Status: Active          │                                    │
│ Hash Rate: 234 MH/s     │ 📤 Seeding Files [4]              │
│ Blocks Found: 12        │ • document.pdf (12) ↑ 2.1 MB/s    │
│ Rewards: 24.5 ETC       │ • video.mp4 (3) ↑ 0.8 MB/s        │
└─────────────────────────┴────────────────────────────────────┘
Command: █                    [Tab] for autocomplete
```

### Keybindings

| Key         | Action                          |
| ----------- | ------------------------------- |
| `1-4`       | Switch to panel (1=Network, 2=Downloads, 3=Peers, 4=Mining) |
| `q` or `Q`  | Quit TUI                        |
| `:`         | Enter command mode              |
| `Tab`       | Next panel                      |
| `Shift+Tab` | Previous panel                  |
| `←`         | Previous panel                  |
| `→`         | Next panel                      |
| `Esc`       | Cancel command mode (when in `:` mode) |
| `Enter`     | Execute command (when in `:` mode) |
| `Backspace` | Delete character (when in `:` mode) |

### Command Mode

Press `:` to enter command mode (similar to vi/vim). Available commands:

- `help` or `h` - Show available commands
- `status` or `s` - Node status summary
- `peers` - Show connected peer count
- `add <path>` - Add file to share (hash saved to `/tmp/chiral_last_hash.txt`)
- `download <hash>` or `download last` - Download file by hash or last added
- `downloads` - Show detailed download metrics
- `dht status` - DHT reachability info
- `mining status` - Mining status (requires `--enable-geth`)

Press `Enter` to execute, `Esc` to cancel.

### Starting TUI Mode

```bash
# Basic usage
./target/release/chiral-network --tui

# With custom port
./target/release/chiral-network --tui --dht-port 5001

# With mining enabled
./target/release/chiral-network --tui --enable-geth

# With custom bootstrap nodes
./target/release/chiral-network --tui \
  --bootstrap /ip4/134.199.240.145/tcp/4001/p2p/12D3KooW...
```

### TUI Features In Detail

**Network Panel** - Real-time network monitoring:
- Connected peer count (live updated)
- Reachability status (Public/Private/Unknown)
- NAT status and traversal info
- AutoNAT configuration
- DHT reachability and confidence
- Observed addresses count
- Download success/failure/retry stats

**Downloads Panel** - Active download tracking:
- Recent download attempts with color-coded status
- File hash (truncated for display)
- Success (green), Failed (red), Retrying (yellow) indicators
- Attempt count (current/max)
- Real-time updates from `FileTransferService`

**Peers Panel** - Connected peer list:
- Live peer list (updates every second)
- Peer ID display (truncated with ellipsis)
- Shows up to 20 most recent peers
- Total peer count in panel title

**Mining Panel** - Mining statistics:
- Mining status (Active/Inactive)
- Hash rate display
- Thread count
- Blocks found count
- Total rewards earned
- Power consumption estimate
- Recent block list with timestamps

All panels update automatically every second with fresh data from the backend services.

---

## Command Reference

### General Commands

| Command  | Aliases     | Description             | Example  |
| -------- | ----------- | ----------------------- | -------- |
| `help`   | `h`, `?`    | Show command list       | `help`   |
| `status` | `s`         | Network status overview | `status` |
| `clear`  | `cls`       | Clear screen            | `clear`  |
| `quit`   | `exit`, `q` | Exit shell              | `quit`   |

### Network Commands

| Command                    | Description                | Example                               |
| -------------------------- | -------------------------- | ------------------------------------- |
| `peers count`              | Show peer count            | `peers count`                         |
| `peers list`               | List all peers             | `peers list`                          |
| `peers list --trust <lvl>` | Filter peers by trust      | `peers list --trust high`             |
| `peers list --sort <fld>`  | Sort peers                 | `peers list --sort score`             |
| `peers list --limit <n>`   | Limit results              | `peers list --limit 10`               |
| `dht status`               | DHT reachability info      | `dht status`                          |
| `dht get <hash>`           | Search DHT for file        | `dht get QmHash...`                   |
| `reputation list`          | Show peer reputation       | `reputation list`                     |
| `reputation info <peer>`   | Detailed peer stats        | `reputation info 12D3KooW...`         |

### File Commands

| Command                 | Description            | Example                    |
| ----------------------- | ---------------------- | -------------------------- |
| `list files`            | List seeding files     | `list files`               |
| `list downloads`        | Show download history  | `list downloads`           |
| `add <path>`            | Add file to share      | `add /path/file.pdf`       |
| `download <hash>`       | Download by hash       | `download QmHash...`       |
| `downloads`             | Active downloads       | `downloads`                |
| `versions list <hash>`  | Show file versions     | `versions list QmHash...`  |
| `versions info <hash>`  | Version details        | `versions info QmHash...`  |

### Mining Commands

> **Note:** Requires `--enable-geth` flag

| Command                  | Description      | Example          |
| ------------------------ | ---------------- | ---------------- |
| `mining status`          | Show mining info | `mining status`  |
| `mining start [threads]` | Start mining     | `mining start 4` |
| `mining stop`            | Stop mining      | `mining stop`    |

### Configuration Commands

| Command                    | Description            | Example                     |
| -------------------------- | ---------------------- | --------------------------- |
| `config list`              | List all settings      | `config list`               |
| `config get <key>`         | Get setting value      | `config get max_peers`      |
| `config set <key> <value>` | Update setting         | `config set max_peers 100`  |
| `config reset <key>`       | Reset to default       | `config reset max_peers`    |

### Phase 4: Advanced Commands

#### Export Commands

| Command                               | Description           | Example                                        |
| ------------------------------------- | --------------------- | ---------------------------------------------- |
| `export metrics [opts]`               | Export network stats  | `export metrics --format json`                 |
| `export peers [opts]`                 | Export peer list      | `export peers --format csv --output peers.csv` |
| `export downloads [opts]`             | Export download stats | `export downloads --format json`               |
| `export all [opts]`                   | Export all data       | `export all --format json`                     |

**Export Options:**
- `--format json|csv` - Output format (default: json)
- `--output <path>` - Custom file path (default: auto-generated with timestamp)

#### Script Commands

| Command            | Description              | Example                  |
| ------------------ | ------------------------ | ------------------------ |
| `script run <path>`| Run REPL script          | `script run monitor.chiral` |
| `script list`      | List available scripts   | `script list`            |

**Script Format:** Create `.chiral` files with one command per line in `.chiral/scripts/` directory.

#### Plugin Commands

| Command              | Description        | Example                       |
| -------------------- | ------------------ | ----------------------------- |
| `plugin load <path>` | Load plugin        | `plugin load ./my-plugin.so`  |
| `plugin unload <name>`| Unload plugin     | `plugin unload my-plugin`     |
| `plugin list`        | List loaded plugins| `plugin list`                 |

#### Webhook Commands

| Command                     | Description         | Example                                              |
| --------------------------- | ------------------- | ---------------------------------------------------- |
| `webhook add <evt> <url>`   | Add webhook         | `webhook add peer_connected https://example.com/hook`|
| `webhook remove <id>`       | Remove webhook      | `webhook remove webhook_1234567890`                  |
| `webhook list`              | List webhooks       | `webhook list`                                       |
| `webhook test <id>`         | Test webhook        | `webhook test webhook_1234567890`                    |
| `webhook events`            | Show event types    | `webhook events`                                     |

**Webhook Events:** `peer_connected`, `peer_disconnected`, `download_started`, `download_completed`, `download_failed`, `file_added`, `mining_started`, `mining_stopped`, `block_found`

#### Reporting Commands

| Command          | Description                  | Example         |
| ---------------- | ---------------------------- | --------------- |
| `report summary` | Generate summary report      | `report summary`|
| `report full`    | Generate comprehensive report| `report full`   |

#### Remote Access Commands

| Command                      | Description              | Example                            |
| ---------------------------- | ------------------------ | ---------------------------------- |
| `remote start [addr] [token]`| Start remote REPL server | `remote start 127.0.0.1:7777`      |
| `remote stop`                | Stop remote server       | `remote stop`                      |
| `remote status`              | Show server status       | `remote status`                    |

**Security Note:** Remote REPL uses token-based authentication. Use SSH port forwarding for production deployments.

---

## Use Cases

### 1. Server Deployment

**Scenario:** Running on VPS as a seeding node

```bash
# SSH to server
ssh user@server.example.com

# Start in tmux/screen for persistence
tmux new -s chiral

# Run REPL
./chiral-network --interactive --dht-port 4001

# Monitor status
chiral> status
chiral> peers count

# Detach: Ctrl+B, D
# Reattach later: tmux attach -t chiral
```

### 2. Quick Testing

**Scenario:** Testing file sharing functionality

```bash
./chiral-network --interactive

chiral> add /tmp/test-file.txt
chiral> status
chiral> peers list
chiral> list files
chiral> quit
```

### 3. Remote Monitoring

**Scenario:** Check node status via SSH

```bash
ssh user@node.example.com "cd chiral && echo 'status' | ./chiral-network --interactive"
```

### 4. Debugging Network Issues

**Scenario:** Investigating NAT traversal problems

```bash
./chiral-network --interactive --show-reachability

chiral> dht status
# Check reachability and observed addresses

chiral> peers list
# Verify peer connections

chiral> status
# Check relay status
```

### 5. Automated Monitoring Script

**Scenario:** Periodic health checks

```bash
#!/bin/bash
# monitor.sh

while true; do
  echo "=== $(date) ==="

  ./chiral-network --interactive <<EOF
status
peers count
quit
EOF

  sleep 300  # Every 5 minutes
done
```

### 6. Bootstrap Node Management

**Scenario:** Running as a bootstrap node with monitoring

```bash
./chiral-network --interactive --is-bootstrap

chiral> status
# Monitor incoming connections

chiral> peers list
# See who's connected
```

---

## Troubleshooting

### REPL Not Starting

**Problem:** REPL won't start or exits immediately

```bash
# Check if port is in use
netstat -tuln | grep 4001

# Use different port
./chiral-network --interactive --dht-port 5001

# Check for errors
./chiral-network --interactive 2>&1 | tee debug.log
```

### No Peers Connecting

**Problem:** Peer count stays at 0

```bash
chiral> peers count
📡 Connected peers: 0

# Check DHT status
chiral> dht status

# Verify bootstrap nodes are reachable
# Try different bootstrap nodes with --bootstrap flag
```

### Command Not Found

**Problem:** Typed command doesn't work

```bash
chiral> unknown-command
❌ Unknown command: 'unknown-command'
   Type 'help' for available commands

# Check spelling
chiral> help
```

### Mining Not Working

**Problem:** Mining commands fail

```bash
chiral> mining status
❌ Error: Mining requires geth. Start with --enable-geth flag

# Solution: Restart with geth enabled
./chiral-network --interactive --enable-geth
```

### Box Drawing Broken

**Problem:** Boxes appear misaligned or broken

This may be a terminal encoding issue:

```bash
# Check terminal supports UTF-8
echo $LANG  # Should show UTF-8

# Try different terminal emulator
# iTerm2, Alacritty, or kitty recommended
```

### Can't Exit REPL

**Problem:** Ctrl+C doesn't exit

This is intentional behavior:

```bash
# Use quit command
chiral> quit

# Or Ctrl+D (EOF signal)
```

### SSH Connection Issues

**Problem:** REPL doesn't work over SSH

```bash
# Ensure UTF-8 is forwarded
ssh -o SendEnv=LANG user@host

# Or set on server
export LANG=en_US.UTF-8
```

---

## FAQ

### Q: What's the difference between REPL and headless mode?

**A:** Headless mode is a daemon with no interaction after startup. REPL provides an interactive shell while running.

### Q: Can I use REPL for automation?

**A:** Yes! Pipe commands or use heredoc for batch operations.

### Q: Does REPL have logs?

**A:** No, logs are disabled for a clean interface. Use `status` and other commands to check state.

### Q: How do I enable logging in REPL mode?

**A:** REPL intentionally disables logs. For debugging with logs, use headless mode instead.

### Q: Can I run REPL and GUI at the same time?

**A:** No, only one instance can run due to port binding (default 4001).

### Q: Will TUI mode replace REPL?

**A:** No, both will coexist. REPL is better for scripting, TUI for live monitoring.

### Q: Does REPL work on Windows?

**A:** Yes, but box-drawing characters may not render in cmd.exe. Use Windows Terminal or PowerShell 7+.

### Q: How do I update to the latest version?

```bash
git pull
cargo build --release
```

### Q: Can I customize the prompt?

**A:** Not currently, but this may be added in a future release.

---

## Additional Resources

- **Main Documentation:** `README.md`
- **Architecture Guide:** `CLAUDE.md`
- **Contributing:** `CONTRIBUTING.md`
- **GitHub:** https://github.com/chiral-network/chiral-network
- **Issues:** https://github.com/chiral-network/chiral-network/issues

---

**Last Updated:** December 2024
**Version:** v0.1.0
**REPL Status:** ✅ Available
**TUI Status:** ✅ Available
