# File Sharing Guide

Chiral Network implements a BitTorrent-like file sharing model with instant seeding and DHT-based discovery.

## Overview

Files shared on Chiral Network are:
- Instantly available when added (no upload process)
- Identified by cryptographic hashes (SHA-256)
- Discoverable through the DHT network
- Optionally encrypted with AES-256-GCM
- Versioned for update tracking

## Sharing Files (Upload)

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

### File Encryption

Files can be encrypted before sharing:

1. **Enable encryption** in file upload dialog
2. **Choose encryption method**:
   - Password-based (PBKDF2 + AES-256-GCM)
   - Public key encryption (for specific recipients)
3. **Share the file hash** with authorized users
4. **Share the encryption key** securely (out-of-band)

**Encryption Features**:
- AES-256-GCM encryption with PBKDF2 key derivation
- Key fingerprinting for verification
- Recipient public key support
- Manifest-based chunk tracking
- Encrypted chunk transfers via WebRTC

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
   - File metadata (name, size, type)
   - List of seeders
   - Encryption status
   - Version information

### Search History

The application maintains search history:
- **Recent searches** saved locally
- **Quick re-download** from history
- **Seeder count** updated in real-time
- **Filter by status** (available, unavailable)

## File Metadata

Each file has associated metadata published to DHT:

```typescript
{
  fileHash: string          // SHA-256 content hash
  fileName: string          // Original filename
  fileSize: number          // Size in bytes
  seeders: string[]         // List of seeder peer IDs
  createdAt: number         // Unix timestamp
  merkleRoot?: string       // Merkle tree root for chunks
  mimeType?: string         // File MIME type
  isEncrypted: boolean      // Encryption flag
  encryptionMethod?: string // Encryption algorithm
  keyFingerprint?: string   // Key verification
  version?: number          // File version number
  cids?: string[]           // Content IDs for chunks
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

1. **Navigate to Settings** → Bandwidth Scheduling
2. **Create schedules** with:
   - Time ranges (HH:MM format)
   - Days of week
   - Upload/download limits
3. **Enable scheduling**
4. **System applies limits** automatically

### File Storage

Files are stored locally:

- **Default location**: `~/ChiralNetwork/Storage`
- **Configurable path**: Change in Settings
- **Storage limits**: Set maximum storage size
- **Auto-cleanup**: Remove old files automatically

## Best Practices

### For Uploaders

1. **Use encryption** for sensitive files
2. **Verify file hashes** before sharing
3. **Share complete files** (no partial uploads)
4. **Keep seeding** to ensure availability
5. **Monitor bandwidth** usage

### For Downloaders

1. **Verify file hashes** with sender
2. **Check seeder count** before downloading
3. **Select multiple sources** for faster downloads
4. **Verify encryption** status
5. **Scan files** before opening

### Privacy Considerations

1. **File hashes reveal content**: Use encryption for privacy
2. **Seeding reveals IP**: Use proxy/relay for anonymity
3. **Metadata is public**: DHT metadata visible to all peers
4. **Consider file names**: Avoid revealing information

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

## See Also

- [Network Protocol](network-protocol.md) - P2P transfer details
- [Security & Privacy](security-privacy.md) - Encryption features
- [User Guide](user-guide.md) - Step-by-step instructions
