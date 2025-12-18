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

| Mode | Interface Type | Use Case |
|------|---------------|----------|
| **GUI** (default) | Graphical window | Desktop users, visual monitoring |
| **Headless** | Daemon (no interaction) | Bootstrap nodes, background services |
| **REPL** | Interactive shell | Testing, debugging, server management |
| **TUI** | Full-screen terminal | Live monitoring, server dashboards |

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

### Phase 2: Enhanced REPL Features 📋 **PLANNED**

**Target:** v0.2.0

Advanced REPL capabilities and improved UX.

**Planned Features:**
- ⏳ Tab completion for commands and file paths
- ⏳ Syntax highlighting for hashes and addresses
- ⏳ Real-time download progress display
- ⏳ Configuration management commands (`config get/set`)
- ⏳ Advanced peer filtering and search
- ⏳ File versioning commands
- ⏳ Reputation management commands
- ⏳ Enhanced error messages with suggestions

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

### Phase 4: Advanced Features 💡 **FUTURE**

**Target:** v0.4.0+

Advanced monitoring and management capabilities.

**Ideas Under Consideration:**
- Custom REPL scripts and macros
- Plugin system for custom commands
- Remote REPL access (secure RPC)
- Multi-node management from single shell
- Advanced analytics and reporting
- Export metrics to files (JSON, CSV)
- Integration with monitoring tools (Prometheus, Grafana)
- Webhook notifications for events

---

## Mode Comparison

### Detailed Comparison Table

| Feature | GUI | Headless | REPL | TUI (Future) |
|---------|-----|----------|------|--------------|
| **Display Required** | ✅ Yes (X11/Wayland) | ❌ No | ❌ No | ❌ No |
| **Works over SSH** | ❌ No | ✅ Yes | ✅ Yes | ✅ Yes |
| **Runtime Interaction** | ✅ Full | ❌ None | ✅ Commands | ✅ Full |
| **Resource Usage** | 🔴 High | 🟢 Low | 🟢 Low | 🟡 Medium |
| **Visual Feedback** | 🟢 Best | ⚫ Logs only | 🟡 Text output | 🟢 Live dashboard |
| **Learning Curve** | 🟢 Easy | - | 🟡 Medium | 🟡 Medium |
| **Automation** | ❌ No | ⚠️ Limited | ✅ Yes | ⚠️ Limited |
| **Monitoring** | 🟢 Real-time | ⚫ Logs | 🟡 On-demand | 🟢 Real-time |

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

| Key | Action |
|-----|--------|
| `1-5` | Switch to panel |
| `q` | Quit |
| `h` or `F1` | Help |
| `r` | Refresh |
| `↑↓←→` | Navigate |
| `Enter` | Select/Activate |
| `Tab` | Command autocomplete |

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

| Command | Aliases | Description | Example |
|---------|---------|-------------|---------|
| `help` | `h`, `?` | Show command list | `help` |
| `status` | `s` | Network status overview | `status` |
| `clear` | `cls` | Clear screen | `clear` |
| `quit` | `exit`, `q` | Exit shell | `quit` |

### Network Commands

| Command | Description | Example |
|---------|-------------|---------|
| `peers count` | Show peer count | `peers count` |
| `peers list` | List all peers | `peers list` |
| `dht status` | DHT reachability info | `dht status` |
| `dht get <hash>` | Search DHT for file | `dht get QmHash...` |

### File Commands

| Command | Description | Example |
|---------|-------------|---------|
| `list files` | List seeding files | `list files` |
| `list downloads` | Show download history | `list downloads` |
| `add <path>` | Add file to share | `add /path/file.pdf` |
| `download <hash>` | Download by hash | `download QmHash...` |

### Mining Commands

> **Note:** Requires `--enable-geth` flag

| Command | Description | Example |
|---------|-------------|---------|
| `mining status` | Show mining info | `mining status` |
| `mining start [threads]` | Start mining | `mining start 4` |
| `mining stop` | Stop mining | `mining stop` |

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
