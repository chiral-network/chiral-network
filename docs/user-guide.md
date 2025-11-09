# User Guide

Complete guide for using Chiral Network for file sharing, mining, and network participation.

## Getting Started

### Installation

1. **Download** the latest release for your platform
2. **Install** the application
3. **Launch** Chiral Network
4. **Initial setup** wizard will guide you through:
   - Language selection
   - Storage location
   - Network configuration
   - Wallet creation (optional)

### First Launch

On first launch, the application will:

- Create default storage directory
- Connect to DHT network
- Detect your region automatically
- Apply default settings

## Main Interface

### Navigation

The sidebar contains navigation to all pages:

- **Download** - Manage file downloads
- **Upload** - Share files with the network
- **Network** - View peers and network status
- **Relay** - Configure relay server/client
- **Mining** - Mine blocks for rewards
- **Proxy** - Configure privacy routing
- **Analytics** - View statistics and metrics
- **Reputation** - View peer reputation
- **Account** - Manage wallet and transactions
- **Settings** - Configure application

### Status Indicators

**Network Status** (top right):

- 🟢 **Connected** - DHT network active
- 🔴 **Disconnected** - No network connection

## Sharing Files

### Upload Files

1. **Navigate to Upload page**
2. **Add files** using one of these methods:
   - Click "Add Files" button
   - Drag & drop files onto the upload area
3. **Files are processed immediately**:
   - Content hash generated
   - Metadata published to DHT
   - File becomes available to network
4. **Copy file hash** to share with others

### Upload Options

**Encryption**:

- Enable to encrypt files before sharing
- Choose password-based or public key encryption
- Share decryption key securely with recipients

**Priority**:

- Set upload bandwidth priority
- Manage seeding resources

### Managing Uploads

**View uploaded files**:

- File name and size
- Content hash
- Number of seeders
- Upload status

**Actions**:

- Copy hash to clipboard
- Remove from sharing (stop seeding)
- View file details

## Downloading Files

### Download a File

1. **Navigate to Download page**
2. **Enter file hash** in search box (received from sender)
3. **Click "Search"**
4. **Review file details**:
   - File name and size
   - Number of seeders
   - Encryption status
5. **Select download sources** (peers)
6. **Click "Download"**
7. **Monitor progress** in download list

### Download Queue

**Queue Management**:

- View all active, queued, and completed downloads
- Set priority (High/Normal/Low)
- Pause/resume individual downloads
- Cancel downloads
- Retry failed downloads

**Filters**:

- Active - Currently downloading
- Queued - Waiting to start
- Completed - Finished downloads
- All - Show everything

### Download Settings

**Concurrent Downloads**:

- Set maximum simultaneous downloads (1-10)
- Higher = faster overall, but more resource intensive

**Download Location**:

- Default: `~/ChiralNetwork/Downloads`
- Change in Settings → Storage

## Network & Peers

### Viewing Network Status

**Network page shows**:

- Connected peers count
- DHT status
- Your peer ID
- Listen addresses
- NAT reachability status

### Connecting to Peers

**Automatic**:

- DHT discovers peers automatically
- Connects based on file queries

**Manual**:

1. Click "Connect to Peer"
2. Enter peer multiaddress or ID
3. Click "Connect"

### NAT Traversal

**Check reachability**:

- View NAT status in Network page
- Public/Private/Unknown indicator
- Confidence level

**If behind NAT**:

- AutoRelay will automatically find relay nodes
- Or manually add relays in Relay page

## Using Relay Nodes

### As a Client (Using Relays)

If you're behind NAT:

1. **Navigate to Relay page**
2. **Enable AutoRelay** (disabled by default)
3. **Optionally add preferred relays**:
   - Enter multiaddress of trusted relay
   - One per line
4. **System will automatically**:
   - Discover available relays
   - Reserve relay slots
   - Route connections through relays

### As a Server (Running Relay)

Help other NAT'd peers:

1. **Navigate to Relay page**
2. **Click "Enable Relay Server"**
3. **DHT restarts** with relay mode enabled
4. **Monitor relay reputation** in Reputation page
5. **Earn reputation** by helping peers

**Requirements**:

- Good uptime
- Stable connection
- Sufficient bandwidth
- DHT must be running

## Mining

### Start Mining

1. **Navigate to Mining page**
2. **Configure settings**:
   - **Threads**: Number of CPU cores to use (1-16)
   - **Intensity**: CPU usage percentage (1-100%)
   - **Pool**: Solo or pool mining (pool not yet implemented)
3. **Click "Start Mining"**
4. **Monitor performance**:
   - Hash rate (H/s, KH/s, MH/s)
   - Blocks found
   - Total rewards

### Mining Tips

**Optimization**:

- Use all available CPU cores for maximum hashrate
- Start with 50% intensity and adjust based on system performance
- Close unnecessary applications
- Monitor CPU temperature

**Economics**:

- Calculate electricity costs vs. rewards
- Solo mining may have irregular rewards
- Pool mining (coming soon) offers consistent payouts

### Mining History

View mining statistics:

- Hash rate over time (chart)
- Recent blocks found
- Total earnings
- Session duration

## Wallet Management

### Creating a Wallet

1. **Navigate to Account page**
2. **Click "Create Wallet"**
3. **Write down mnemonic phrase** (12 or 24 words)
   - ⚠️ **CRITICAL**: This is your ONLY backup
   - Store securely offline
   - Never share with anyone
4. **Verify phrase** by re-entering
5. **Wallet created**

### Importing a Wallet

1. **Navigate to Account page**
2. **Click "Import Wallet"**
3. **Enter mnemonic phrase**
4. **Click "Import"**
5. **Wallet restored** with all accounts

### Managing Accounts

**Create additional accounts**:

1. Click "Add Account"
2. New account derived from mnemonic
3. Each has unique address

**Account operations**:

- View balance
- Copy address
- Generate QR code
- Set as default

### Sending Transactions

1. **Click "Send"** in Account page
2. **Enter**:
   - Recipient address
   - Amount
3. **Review details**
4. **Click "Confirm"**
5. **Transaction sent** to blockchain

## Analytics

### Viewing Statistics

The Analytics page shows:

**Storage**:

- Total space used
- Number of files shared
- Files by type

**Bandwidth**:

- Upload/download over time
- Peak usage times
- Total transferred

**Network**:

- Peers connected
- Connection history
- Network contribution

**Mining** (if enabled):

- Hash rate history
- Blocks found
- Rewards earned

## Settings

### General Settings

**Language**:

- Select preferred language
- Changes apply immediately

**Theme**:

- Light/Dark mode
- Auto (system preference)

### Storage Settings

**Location**:

- Set download/upload directories
- Change storage path

**Limits**:

- Maximum storage size
- Auto-cleanup threshold

### Network Settings

**Connection**:

- Port number (default: 30303)
- Max connections
- Enable UPnP/NAT

**Bandwidth**:

- Upload/download limits (KB/s)
- 0 = unlimited

**Bandwidth Scheduling**:

- Create time-based bandwidth rules
- Different limits for different hours/days

### Privacy Settings

**Proxy**:

- Enable SOCKS5 proxy
- Enter proxy address (e.g., 127.0.0.1:9050 for Tor)

**Anonymous Mode**:

- Routes all traffic through relay/proxy
- Changes peer ID periodically
- Disables direct connections

**Encryption**:

- Force encryption for all transfers
- Disable unencrypted connections

### NAT Traversal Settings

**AutoNAT**:

- Enable/disable reachability detection
- Set probe interval (seconds)
- Add custom AutoNAT servers

**AutoRelay**:

- Enable/disable automatic relay usage
- Add preferred relay nodes

**Relay Server**:

- Enable to help NAT'd peers
- Contributes bandwidth to network

### Advanced Settings

**DHT**:

- Auto-start DHT on launch
- Bootstrap nodes
- Kademlia parameters

**File Transfer**:

- Chunk size (KB)
- Cache size (MB)
- Concurrent transfers

**Logging**:

- Log level (debug/info/warn/error)
- Log location

## Reputation System

### Viewing Peer Reputation

1. **Navigate to Reputation page**
2. **View all known peers** with their:
   - Trust level (Trusted/High/Medium/Low/Unknown)
   - Reputation score (0.0 - 1.0)
   - Recent interactions
   - Performance metrics

### Filtering Peers

**Filter by**:

- Trust level
- Encryption support
- Minimum uptime percentage

**Sort by**:

- Reputation score
- Number of interactions
- Last seen time

### Relay Leaderboard

View top relay servers:

- Ranked by relay reputation
- Shows circuit success rate
- Displays reservations accepted
- Your rank (if running relay)

## Troubleshooting

### Can't Connect to Network

1. Check internet connection
2. Verify firewall allows application
3. Try different port in Settings
4. Check DHT status in Network page
5. Restart application

### Files Not Downloading

1. Verify file hash is correct
2. Check if seeders are online
3. View seeder list in download modal
4. Try selecting different peers
5. Check bandwidth limits in Settings

### Mining Not Starting

1. Ensure Geth is initialized
2. Check mining address is set
3. Verify system resources available
4. Review console for errors
5. Restart application

### Wallet Issues

1. Verify mnemonic phrase is correct
2. Check Geth is synced
3. Wait for network connection
4. Restart application
5. Check transaction history for pending txs

## Best Practices

### Security

- ✅ Backup mnemonic phrase immediately
- ✅ Use encryption for sensitive files
- ✅ Verify file hashes before downloading
- ✅ Keep application updated
- ✅ Use strong device passwords
- ❌ Never share mnemonic phrase
- ❌ Never store keys digitally
- ❌ Don't download from untrusted peers

### Performance

- Keep application running for better reputation
- Contribute storage and bandwidth to network
- Use multiple download sources
- Enable relay server if you have stable connection
- Monitor resource usage

### Privacy

- Use SOCKS5 proxy (Tor) for anonymity
- Enable anonymous mode for sensitive activities
- Use encryption for all file transfers
- Regularly change anonymous peer ID
- Don't reveal personal info in file names

## Keyboard Shortcuts

- `Ctrl/Cmd + ,` - Open Settings
- `Ctrl/Cmd + R` - Refresh current page
- `Ctrl/Cmd + Q` - Quit application
- `F5` - Reload application
- `F11` - Toggle fullscreen (desktop)

## Getting Help

If you need assistance:

1. Check this guide and documentation
2. Review troubleshooting section
3. Check GitHub issues for similar problems
4. Join community on Zulip
5. Open new issue on GitHub

## See Also

- [File Sharing](file-sharing.md) - Detailed file sharing guide
- [NAT Traversal](nat-traversal.md) - Network connectivity guide
- [Wallet & Blockchain](wallet-blockchain.md) - Wallet management
- [Reputation](reputation.md) - Reputation system details
