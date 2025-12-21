# Chiral Network Interactive Shell Guide

## Table of Contents

- [Overview](#overview)
- [Implementation Roadmap](#implementation-roadmap)
- [Mode Comparison](#mode-comparison)
- [Getting Started](#getting-started)
- [REPL Mode](#repl-mode)
- [TUI Mode (Future)](#tui-mode-future)
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

### Phase 3: TUI Mode 🚧 **IN PLANNING**

**Target:** v0.3.0

Full-screen terminal dashboard with live updates.

**Planned Features:**

- ⏳ Live dashboard with multiple panels
- ⏳ Real-time network metrics visualization
- ⏳ Progress bars for active downloads
- ⏳ Panel switching (Network, Downloads, Peers, Mining)
- ⏳ Mouse support (optional)
- ⏳ Charts and graphs (bandwidth, peers over time)
- ⏳ Keyboard shortcuts (hjkl navigation)
- ⏳ Customizable layout and themes

**Technology Stack:**

- `ratatui` - Modern Rust TUI framework
- `crossterm` - Cross-platform terminal handling
- Event-driven architecture
- 1-second refresh rate

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

### Phase 5: Mining Integration 📅 **PLANNED**

**Target:** v0.5.0

**Goal:** Fully integrate mining capabilities into the interactive shell with real-time monitoring and control.

**Current Status:**

- Mining commands exist in REPL but show placeholders only
- Backend functions fully implemented in `ethereum.rs`:
  - `start_mining(miner_address, threads)`
  - `stop_mining()`
  - `get_mining_status()`
  - `get_mining_performance(data_dir)`
  - `get_mining_logs(data_dir, lines)`
  - `get_total_mining_rewards(miner_address)`
- Ready for integration

**Planned Features:**

#### 5.1: Core Mining Integration (High Priority)

Connect REPL mining commands to actual Geth mining functions.

- ⏳ Update `cmd_mining()` to call real mining functions
- ⏳ Display real mining status (hash rate, blocks found)
- ⏳ Implement mining start/stop with thread control
- ⏳ Add error handling for mining operations
- ⏳ Wallet/miner address management

**Code Example:**
```rust
// mining start 4
crate::ethereum::start_mining(&miner_address, 4).await?;
println!("✓ Mining started with 4 thread(s)");

// mining status
let is_mining = crate::ethereum::get_mining_status().await?;
let (hash_rate, blocks) = crate::ethereum::get_mining_performance(&data_dir).await?;
println!("Hash Rate: {:.2} MH/s | Blocks: {}", hash_rate, blocks);
```

#### 5.2: Mining Dashboard (Medium Priority)

Real-time mining statistics and monitoring.

- ⏳ Live updating mining dashboard
- ⏳ Hash rate trends and history
- ⏳ Block discovery notifications
- ⏳ Mining rewards accumulator
- ⏳ Thread utilization display
- ⏳ Mining uptime tracking

**New Commands:**
- `mining dashboard` - Real-time mining view with auto-refresh
- `mining stats [--live]` - Detailed mining statistics
- `mining logs [--tail 50]` - View recent mining logs

#### 5.3: Mining History & Analytics (Medium Priority)

Track and analyze mining performance over time.

- ⏳ Session mining history with timestamps
- ⏳ Total rewards calculation per address
- ⏳ Performance trends and charts
- ⏳ Export mining data to JSON/CSV (integrate with Phase 4)
- ⏳ Mining efficiency metrics

**New Commands:**
- `mining history [--limit 10]` - Recent mining sessions
- `mining rewards [--address]` - Total rewards earned
- `export mining --format json` - Export mining data (Phase 4 integration)

#### 5.4: Advanced Mining Configuration (Low Priority)

Persistent mining configuration and optimization.

- ⏳ Thread configuration with persistence
- ⏳ Mining intensity presets (high/medium/low)
- ⏳ Etherbase (coinbase) address management
- ⏳ Hardware auto-tuning based on CPU/GPU capabilities
- ⏳ Configuration validation and testing

**New Commands:**
- `mining config list` - Show all mining settings
- `mining config set threads <n>` - Set mining threads
- `mining config set intensity <high|medium|low>` - Set mining intensity
- `mining config set etherbase <address>` - Set mining reward address
- `mining autotune` - Auto-optimize settings for hardware

#### 5.5: Smart Mining Features (Low Priority)

Intelligent mining with scheduling and conditions.

- ⏳ Time-based mining schedules (mine during off-peak hours)
- ⏳ Conditional mining (only mine when peers > threshold)
- ⏳ Profitability calculator (estimate vs electricity cost)
- ⏳ Power consumption tracking and estimates
- ⏳ Temperature monitoring (if sensors available)
- ⏳ Automatic shutdown on overheating

**New Commands:**
- `mining schedule add --time "02:00-06:00" --days "Mon,Tue,Wed"` - Add schedule
- `mining schedule list` - List all schedules
- `mining schedule remove <id>` - Remove schedule
- `mining threshold set --min-peers 5` - Set minimum peers requirement
- `mining profitability --electricity-cost 0.12` - Calculate profitability

#### 5.6: TUI Mining Panel (Low Priority, depends on Phase 3)

Dedicated mining panel in TUI mode with live visualization.

- ⏳ Real-time hash rate graph (line chart)
- ⏳ Block discovery timeline
- ⏳ Thread utilization bars
- ⏳ Temperature/power monitoring gauges
- ⏳ Live earnings counter
- ⏳ Mining event log (blocks found, errors)

**TUI Layout Example:**
```
┌─────────────────────────────────────────────────────┐
│ Mining Status                   [Active: Yes]       │
├─────────────────────────────────────────────────────┤
│ Hash Rate: 45.2 MH/s            Blocks: 127         │
│ Rewards: 2,540.00 CHR          Uptime: 2h 34m      │
├─────────────────────────────────────────────────────┤
│ Hash Rate History (Last Hour)                       │
│  50 │     ╭──╮                                      │
│  40 │  ╭──╯  ╰─╮  ╭──                               │
│  30 │──╯       ╰──╯                                 │
│     └────────────────────────────────────           │
├─────────────────────────────────────────────────────┤
│ Recent Blocks                                       │
│  #1234 - 2 min ago - 20.0 CHR                       │
│  #1233 - 8 min ago - 20.0 CHR                       │
└─────────────────────────────────────────────────────┘
```

#### 5.7: Mining Webhook Integration

Integrate mining events with Phase 4 webhook system.

- ⏳ `mining_started` webhook event
- ⏳ `mining_stopped` webhook event
- ⏳ `block_found` webhook event (already in Phase 4)
- ⏳ `mining_error` webhook event
- ⏳ Mining performance alerts via webhooks

**Dependencies:**

- Geth process running with `--enable-geth` flag
- Wallet with miner address configured
- Network connection for blockchain sync
- (Optional) Power monitoring for consumption tracking
- (Optional) Temperature sensors for overheating protection

**Implementation Order:**

1. Phase 5.1 - Core mining integration (Week 1)
2. Phase 5.2 - Mining dashboard (Week 2)
3. Phase 5.3 - History & analytics (Week 2)
4. Phase 5.4 - Advanced configuration (Week 3)
5. Phase 5.5 - Smart mining features (Week 4)
6. Phase 5.6 - TUI panel (After Phase 3 completion)
7. Phase 5.7 - Webhook integration (After Phase 5.1)

**Security Considerations:**

- Never log miner private keys
- Validate addresses before use
- CPU throttling to prevent overheating
- Memory limits for mining operations
- Automatic shutdown on critical errors
- Rate limiting for RPC calls

**Testing Requirements:**

- Unit tests for command parsing and validation
- Integration tests for mining start/stop cycles
- Manual tests on different hardware configurations
- Performance benchmarking
- Power consumption validation

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
./target/release/chiral-network --tui          # TUI mode (future)
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
--disable-autorelay            # Disable AutoRelay client
--enable-relay                 # Run as relay server
--relay <MULTIADDR>            # Preferred relay nodes

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
  │ Circuit Relay: None                                    │
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

## TUI Mode (Future)

> **Status:** Planned for future release
>
> TUI (Terminal User Interface) mode will provide a full-screen dashboard with live updates, similar to `htop` or `btop`.

### Planned Features

- 📊 **Live Dashboard** - Real-time network stats
- 🎨 **Multiple Panels** - Network, downloads, peers, mining
- ⌨️ **Keyboard Navigation** - Switch between panels
- 🖱️ **Mouse Support** - Optional click interactions
- 📈 **Charts & Graphs** - Visual representation of metrics
- 🎯 **Panel Focus** - Zoom into specific sections

### Planned Interface Layout

```
┌─────────────────────────────────────────────────────────────┐
│ Chiral Network v0.1.0          [Q]uit [H]elp              │
├─────────────────────────┬────────────────────────────────────┤
│ 📡 Network [1]          │ 📥 Active Downloads [2]            │
│ Peers: 42 ████████░░    │ ┌──────────────────────────────────┐ │
│ DHT: 1,234 entries      │ │ file.pdf [████████░░] 75%       │ │
│ NAT: Public             │ │   8 peers, 4.2 MB/s, ETA 2m     │ │
│ Relay: Connected        │ │                                  │ │
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

### Planned Keybindings

| Key         | Action               |
| ----------- | -------------------- |
| `1-5`       | Switch to panel      |
| `q`         | Quit                 |
| `h` or `F1` | Help                 |
| `r`         | Refresh              |
| `↑↓←→`      | Navigate             |
| `Enter`     | Select/Activate      |
| `Tab`       | Command autocomplete |

### Starting TUI Mode (Future)

```bash
# Basic usage
./target/release/chiral-network --tui

# With options
./target/release/chiral-network --tui --dht-port 5001 --enable-geth
```

### Implementation Timeline

TUI mode is planned for a future release after REPL mode is stable. Implementation will use:

- **ratatui** - Modern Rust TUI framework
- **crossterm** - Cross-platform terminal manipulation
- **Live updates** - 1-second refresh rate
- **Panel system** - Modular layout design

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
./chiral-network --interactive --is-bootstrap --enable-relay

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
**TUI Status:** 📋 Planned
