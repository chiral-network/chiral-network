# **File Sharing & Payment System Guide**

Chiral Network implements a BitTorrent-like file sharing model with instant seeding and DHT-based discovery.

## **Purpose**

- Facilitate peer-to-peer file sharing incentivized by **Chiral token** exchange.
- Automatically price files using a **global Chiral-to-MB value** or a **hash-power-based dynamic rate**.
- Provide a consistent protocol for **upload, download, and micropayment flows**.
- Ensure seamless integration between:
  - **Frontend**: pricing display, validation, and payment UX,
  - **Backend**: payment services, wallet handling, and transaction logging,
  - **Network layers**: DHT discovery, seeding, and transport protocols.
- Maintain modular separation between **data transmission** and **payment verification**.
- Support **tiered download speeds** — rate-limited free tier with paid Chiral token tiers for faster downloads.

---

## **Download Speed Tiers**

Downloads are rate-limited by default. Users can pay Chiral tokens for faster download speeds:

| Tier     | Speed        | Cost                  |
| -------- | ------------ | --------------------- |
| Free     | Baseline     | None                  |
| Standard | 2x baseline  | Chiral tokens per MB  |
| Premium  | 5x baseline  | Chiral tokens per MB  |
| Ultra    | Uncapped     | Chiral tokens per MB  |

- **All payments use Chiral tokens** via the existing wallet/blockchain infrastructure
- **Decentralized** — tokens transfer peer-to-peer between downloader and seeder, no central payment server
- **Seeders earn more** for serving higher-tier downloads, incentivizing network participation
- **Tier selection is per-download** — users choose their speed tier when starting each download
- **Rate limiting is enforced by the seeder** — the seeder throttles upload bandwidth based on the tier paid

---

## **Overview**

- The Chiral Network implements a **BitTorrent-like file sharing model** with DHT-based peer discovery and instant seeding.
- Each uploaded file has an **associated price**, automatically computed using a **global Chiral-to-MB exchange rate**.
- The default conversion is **1 MB = 0.001 Chiral**.
- No manual price entry is required per upload — prices derive automatically from:
  - The **current global Chiral value**, or
  - A **dynamic rate** calculated from **network hash power**, reflecting mining difficulty.
- Files shared on Chiral Network are:
  - Instantly available when added (no upload process)
  - Identified by cryptographic hashes (SHA-256)
  - Discoverable through the DHT network

### Basic File Sharing

1. **Navigate to Upload Page** (Shared Files)
2. **Add Files** using one of these methods:
   - Click "Add Files" button
   - Drag & drop files onto the card
   - Drag files anywhere on the page
3. **Files are processed immediately**:
   - Content hash generated (SHA-256)
   - Metadata published to DHT
   - File immediately available for download by peers

### File Versioning

Chiral Network supports file versioning:

1. **Upload a file** normally
2. **Upload updated version** with same base name
3. **System tracks versions** automatically
4. **Users can choose** which version to download

## Downloading Files

### Basic Download

1. **Navigate to Download Page**
2. **Enter file hash** received from peer
3. **Click "Search & Download"**
4. **System will**:
   - Query DHT for file metadata
   - Discover seeders
   - Display peer selection modal
5. **Select peers** to download from
6. **Monitor progress** in download queue

### Multi-Source Downloads

Chiral Network supports downloading from multiple peers simultaneously:

- **Parallel chunk downloads** from different peers
- **Intelligent peer selection** based on reputation
- **Bandwidth aggregation** for faster transfers
- **Automatic failover** if peers disconnect
- **Chunk verification** using merkle tree

#### Multi-Peer Download Details:

- Architecture:
  - Files are split into pieces of equal length (except for the last piece). If peer acceleration is enabled, then WebRTC or Bitswap will be used for piece exchange. When one peer downloads a piece of the file they need, it informs the rest of the peers that “I have this piece” so that the rest of the peers can start downloading from this peer who now currently has this piece. In the same way, every other peer does the same thing
  - We will support downloading a file from its original single source, either HTTP, FTP, BitTorrent or ed2k -- and optionally accelerate the download using Chiral Network's Private Peer Protocols WebRTC or Bitswap.
  - It is important to note that multiple conventional sources are NOT combined (i.e. using more than different protocol simultaneously to download a file). Instead, the main download source is simply the original URL and peers simply assist with acceleration.

- Problems:
  - How do we manage multi downloads and ensure its speed?
  - How do the pieces get transferred and what happens when a seeder suddenly dips after a piece?
- Solution:
  - Give the highest priority to the rarest piece of the file. We distribute the rarest piece of the file with a selected number of peers and then make sure to transfer the rest of the pieces with as many peers as possible so that it doesn’t matter whether the seeder is active or not
  - As a result, we minimize the dependency on the original seeder for downloading the file and encouraging more P2P connections

#### Strategies

- Rarest-First Piece Selection: prioritize the download of the rarest piece among all peers
  - Maxmize piece distribution across peers
  - Ensure no piece becomes unavailable if the original source disconnects
  - Improve speed - rare pieces are more sought out which results in more reciprocation and higher unchoke rates.
    How to compute the rarest piece? There are two ways (borrowed from BitTorrent Protocols)
  - **Have** messages: When a peer downloads a piece, it broadcasts a dynamic message saying,
    I have piece X now."
- **Bitfield** messages: during the initial handshake, a peer sends a Bitfield message that contains the pieces that it holds
  - When 2 peers handshake, we send an array of bits with 0s or 1s where 1 represents the pieces that it has and 0 representing the pieces that it doesn’t have. After exchanging the bitfield messages, the peer computes the rarest piece and prioritizes downloading of that piece first.
  - Also prioritize downloading of all the blocks in the piece before moving onto the next piece.Every peer maintains the piece availability across its peer set and uses it to compute the rarest piece.

Download Flow Summary:

1. Download begins from the original single protocol URL (HTTP, FTP, BT, or ed2k)
2. Peers can also use WebRTC or BitSwap for acceleration
3. Peers can exchange bitfield messages and compute the rarest piece OR peers can download pieces and send have message announcements

### Download Queue Management

**Priority Levels**:

- High: Download immediately
- Normal: Queue normally
- Low: Download when bandwidth available

**Queue Controls**:

- Pause/Resume individual downloads
- Cancel downloads
- Reorder queue
- Set concurrent download limit (1-10)

## File Discovery

### Hash-Based Discovery

Files are discovered using their content hash:

1. **Obtain file hash** from peer (out-of-band)
2. **Search using hash** in Download page
3. **DHT returns**:
   - File metadata (name, size, type, price)
   - List of seeders
   - Version information

### Search History

The application maintains search history:

- **Recent searches** saved locally
- **Quick re-download** from history
- **Seeder count** updated in real-time
- **Filter by status** (available, unavailable)

## **System Overview Diagram**

```mermaid
sequenceDiagram
    participant A as Downloader
    participant DHT
    participant B as Seeder
    participant ETH

    A->>DHT: Query file hash (metadata request)
    DHT-->>A: Return seeders + price info
    A->>B: Initiate file handshake
    A->>ETH: Verify wallet balance
    ETH-->>A: Balance confirmation
    A->>B: Begin download

    loop Pay-per-MB transfer
        A->>ETH: Send microtransaction
        ETH-->>B: Credit Chiral tokens
        B-->>A: Serve next MB segment
    end

    A-->>DHT: Log download transaction
    B-->>DHT: Log payment receipt
```

- Price and uploader address fields are automatically populated on upload. The backend references the global pricing configuration to maintain consistent pricing across sessions and peers.

### File Metadata:

```typescript
{
  fileHash: string          // SHA-256 content hash
  fileName: string          // Original filename
  fileSize: number          // Size in bytes
  seeders: string[]         // List of seeder peer IDs
  createdAt: number         // Unix timestamp
  merkleRoot?: string       // Merkle tree root for chunks
  mimeType?: string         // File MIME type
  keyFingerprint?: string   // Key verification
  version?: number          // File version number
  cids?: string[]           // Content IDs for chunks
  price: number             // price of file in Chiral
}
```

## Seeding Behavior

### Continuous Seeding

Files remain seeded as long as they're in your "Shared Files" list:

- **No upload step**: Files immediately available
- **Real-time seeder count**: Shows how many peers have the file
- **Automatic DHT updates**: Metadata refreshed periodically
- **Bandwidth control**: Configurable upload limits

### Seed Management

**Stop Seeding**:

1. Go to Upload page
2. Find file in list
3. Click Remove/Delete
4. File removed from DHT

**Seed Statistics**:

- Total upload bandwidth contributed
- Number of peers served
- Reputation earned from seeding

## Advanced Features

### Chunk-Based Transfers

Large files are split into chunks:

- **Configurable chunk size** (default 256 KB)
- **Bitswap protocol** for efficient exchange
- **Parallel chunk transfer** from multiple sources
- **Integrity verification** per chunk
- **Resume support** for interrupted transfers

### Bandwidth Scheduling

Control when files are seeded:

1. **Navigate to Settings** -> Bandwidth Scheduling
2. **Create schedules** with:
   - Time ranges (HH:MM format)
   - Days of week
   - Upload/download limits
3. **Enable scheduling**
4. **System applies limits** automatically

### File Storage

Files are stored locally:

- **Default location**: ~/Downloads/Chiral-Network-Storage
- **Configurable path**: Change in Settings
- **Storage limits**: Set maximum storage size
- **Auto-cleanup**: Remove old files automatically

## Best Practices

### For Uploaders

1. **Verify file hashes** before sharing
2. **Share complete files** (no partial uploads)
3. **Keep seeding** to ensure availability
4. **Monitor bandwidth** usage

### For Downloaders

1. **Verify file hashes** with sender
2. **Check seeder count** before downloading
3. **Select multiple sources** for faster downloads
4. **Scan files** before opening

### Privacy Considerations

1. **Seeding reveals IP**: Use proxy for anonymity
2. **Metadata is public**: DHT metadata visible to all peers
3. **Consider file names**: Avoid revealing information

## Troubleshooting

### File Not Found

- Verify file hash is correct
- Check if seeders are online
- Wait for DHT propagation (~30 seconds)
- Try searching again later

### Slow Downloads

- Select more seeders
- Check your download bandwidth limit
- Verify seeders have good reputation
- Enable multi-source downloads

### Upload Not Working

- Check DHT is connected
- Verify file is not corrupted
- Ensure storage path is writable
- Check firewall settings

# Global Pricing System

Objective: Maintain a single global configuration for determining the Chiral-to-MB conversion, avoiding manual price setting for every file.

### Mechanisms

1. Static Global Rate (Default)
   - Conversion: 1 MB = 0.001 Chiral.
   - Stored globally in the DHT or config service.
   - Provides stability and simplicity.

2. Dynamic Network-Based Rate: Adjusts pricing based on network conditions with bounded limits.
   `pricePerMB = basePricePerMB * minerFactor * efficiencyFactor * normalizationFactor`

##### Dynamic Network-Based Pricing Explanation:

| Variable              | What it means                        | Range / Units           |
| --------------------- | ------------------------------------ | ----------------------- |
| `basePricePerMB`      | Base price per megabyte              | 0.001 Chiral/MB         |
| `minerFactor`         | Demand factor based on active miners | 0.5x to 2.0x            |
| `efficiencyFactor`    | Network efficiency adjustment        | 0.8x to 1.5x            |
| `normalizationFactor` | Tunable scaling factor               | Default: 1.2            |
| `pricePerMB`          | Final Chiral price per MB (bounded)  | 0.0001 to 1.0 Chiral/MB |

**Formula Components:**

- **Base Price**: 0.001 Chiral per MB provides a stable foundation
- **Miner Factor** = clamp(active_miners / 10, 0.5, 2.0)
  - More miners indicates higher network demand, slightly increasing price
  - Clamped to prevent extreme fluctuations
- **Efficiency Factor** = clamp(actualEfficiency / baselineEfficiency, 0.8, 1.5)
  - Based on power usage relative to hashrate (baseline: 100W per MH/s)
  - Higher power consumption per hash = slightly higher price
- **Final Price Bounds**: Always between 0.0001 and 1.0 Chiral per MB
  - Prevents astronomical prices from edge cases
  - Ensures minimum viable payment for seeders

# Download Control & Payment Enforcement Protocol

### Mechanism

##### During downloads:

- Node B (seeder) serves file chunks through HTTP/WebRTC.
- The protocol enforces payment checkpoints after specific data amounts are transmitted.

##### Example Control Logic:

- Serve first 10 MB as initial handshake segment.
- If no payment received after 10 MB -> stop serving.
- If payment received -> continue serving.
- Download resumes with incremental payments:
  - 1 MB --> pay Chiral
  - 2 MB --> pay Chiral
  - 4 MB --> pay Chiral
  - etc

##### Payment Options

| Mode                     | Description                                 | Recommended For                                 |
| ------------------------ | ------------------------------------------- | ----------------------------------------------- |
| **Exponential Scaling**  | Payments grow as trust builds (1->2->4->8 MB). | Typical use case, balances risk and efficiency. |
| **Full Upfront Payment** | Pay entire file price at once.              | Trusted seeders or verified peers.              |

Potential Protocol Message Specification:
| Message Type | Direction | Description |
| ------------------- | ----------------------- | ---------------------------------------------- |
| `PRICE_QUERY` | Downloader -> DHT | Request global Chiral price and file rate |
| `PRICE_RESPONSE` | DHT -> Downloader | Return computed price per MB |
| `PAYMENT_INIT` | Downloader -> Seeder | Initiate payment session |
| `PAYMENT_ACK` | Seeder -> Downloader | Confirm receipt of Chiral payment |
| `CHUNK_REQUEST` | Downloader -> Seeder | Request data chunk (N MB) |
| `CHUNK_DELIVER` | Seeder -> Downloader | Serve chunk upon payment confirmation |
| `PAYMENT_FAILURE` | Seeder -> Downloader | Stop serving due to missing or invalid payment |
| `REPUTATION_UPDATE` | Seeder/Downloader -> DHT | Submit peer trust rating |

##### Payment Flow Protocol

```mermaid
sequenceDiagram
  participant A as Downloader
  participant B as Seeder
  participant E as Ethereum

  A->>B: PAYMENT_INIT (fileHash, MB_count)
  B->>E: Create/verify payment channel
  E-->>B: Channel ready
  loop For each MB segment
    A->>E: Send microtransaction
    E-->>B: Credit Chiral balance
    B-->>A: Send MB data
    alt Payment timeout
      B-->>A: Stop serving
    end
  end
  A-->>E: Close payment channel
  E-->>A: Transaction summary
```

#### Ethereum/Chiral Integration:

##### Wallet Capabilities:

Each peer node runs a local Ethereum-compatible wallet supporting:

- getBalance()
- sendPayment(address, amount)
- verifyTransaction(txHash)
- logTransaction(metadata)

##### Transaction Model

Transactions include:

- Recipient address (seeder)
- File hash reference
- Payment amount (in Chiral)
- Metadata (timestamp, MB segment)

#### Security

- Transactions are cryptographically signed with user’s private key.
- DHT stores public receipts only, never private keys.
- Local key storage handled securely within native environment (e.g., Tauri).

#### Gas and Fees

- Each transaction includes a small gas fee.
- Estimated cost = base gas + data fee.
- Option to dynamically add a “gas surcharge” to file price for accurate costing.

### Frontend Integration

#### Features

- Automatic price computation after file upload.
- Price badges display file cost in Chiral.
- Balance check prior to download.
- Payment confirmation modals for user validation.
- Transaction logs shown in account dashboard.
- Live peer-to-peer toasts for payment success/failure notifications.

#### Payment UI Flow

```mermaid
flowchart TD
  A[User selects file to download] --> B[Fetch metadata & price]
  B --> C[Check user balance]
  C -->|Sufficient| D[Show payment modal]
  C -->|Insufficient| E[Show balance error]
  D --> F[Execute payment]
  F --> G[Start incremental download]
  G --> H[Record transaction in history]
```

### Areas for Improvement & TODOs

- Define rounding rules for price display (e.g., 4 decimal places).
- Add automated tests for global pricing updates.
- Handle empty or zero-byte file uploads gracefully.
- Include uploader/audit logs for all uploads.
- Implement transaction timeouts and retry strategies.
- Add multi-seeder payment splitting support.
- Include gas fee estimation in pricing UI.
- Enable dynamic rate propagation through DHT.
- Build reputation registry for trust-weighted pricing.
- Develop control parameters for HTTP server payment enforcement.

### Summary

##### The Chiral File Sharing & Payment System integrates peer-to-peer file distribution with an adaptive Chiral-based payment mechanism. This system ensures:

- Automated and fair file pricing through global or hash-based valuation.
- Incremental, flexible payment protocols tied to network trust.
- Ethereum-backed settlement for verifiable transactions.
- Scalable architecture for decentralized, incentive-aligned file exchange.

## See Also

- [Network Protocol](network-protocol.md) - P2P transfer details
- [Security & Privacy](security-privacy.md) - Encryption features
- [User Guide](user-guide.md) - Step-by-step instructions
