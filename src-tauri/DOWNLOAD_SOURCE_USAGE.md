# Download Source Abstraction Usage Guide

## Overview

The unified download source abstraction provides a consistent interface for handling multiple download sources (P2P, HTTP, FTP) in the Chiral Network application.

## Architecture

### Core Components

1. **`download_source.rs`** - Core abstraction module
   - `DownloadSource` enum with variants for P2P, HTTP, and FTP
   - Source info structures: `P2pSourceInfo`, `HttpSourceInfo`, `FtpSourceInfo`
   - Helper methods for source selection and display

2. **`download_scheduler.rs`** - Example integration
   - Demonstrates how to use `DownloadSource` in scheduling logic
   - Shows logging patterns and source selection
   - Placeholder handlers for each source type

## DownloadSource Enum

```rust
pub enum DownloadSource {
    P2p(P2pSourceInfo),
    Http(HttpSourceInfo),
    Ftp(FtpSourceInfo),
}
```

### P2P Source

Used for peer-to-peer downloads via libp2p/WebRTC:

```rust
DownloadSource::P2p(P2pSourceInfo {
    peer_id: "12D3KooWABC123".to_string(),
    multiaddr: Some("/ip4/127.0.0.1/tcp/4001".to_string()),
    reputation: Some(85),
    supports_encryption: true,
    protocol: Some("webrtc".to_string()),
})
```

**Fields:**
- `peer_id`: Peer identifier in the P2P network
- `multiaddr`: Optional multiaddress for connection
- `reputation`: Optional reputation score (0-100)
- `supports_encryption`: Whether peer supports encrypted transfers
- `protocol`: Protocol used (webrtc, tcp, etc.)

### HTTP Source

Used for HTTP/HTTPS downloads:

```rust
DownloadSource::Http(HttpSourceInfo {
    url: "https://cdn.example.com/file.zip".to_string(),
    auth_header: Some("Bearer token123".to_string()),
    verify_ssl: true,
    headers: Some(vec![
        ("User-Agent".to_string(), "ChiralNetwork/1.0".to_string())
    ]),
    timeout_secs: Some(30),
})
```

**Fields:**
- `url`: Full HTTP/HTTPS URL
- `auth_header`: Optional authentication header
- `verify_ssl`: Whether to verify SSL certificates (default: true)
- `headers`: Optional custom headers
- `timeout_secs`: Connection timeout

### FTP Source

Used for FTP/FTPS downloads:

```rust
DownloadSource::Ftp(FtpSourceInfo {
    url: "ftp://ftp.example.com/pub/file.tar.gz".to_string(),
    username: Some("anonymous".to_string()),
    encrypted_password: Some("base64_encrypted_pass".to_string()),
    passive_mode: true,
    use_ftps: true,
    timeout_secs: Some(60),
})
```

**Fields:**
- `url`: FTP URL (e.g., `ftp://host/path`)
- `username`: Optional username (for non-anonymous FTP)
- `encrypted_password`: Optional encrypted password (Base64-encoded AES-GCM-SIV)
- `passive_mode`: Use passive FTP mode (default: true)
- `use_ftps`: Use FTPS (FTP over TLS) (default: false)
- `timeout_secs`: Connection timeout

## Common Patterns

### 1. Source Selection

```rust
use crate::download_source::DownloadSource;

fn select_source(sources: &[DownloadSource]) -> Option<DownloadSource> {
    // Sort by priority score (higher is better)
    let mut sorted: Vec<_> = sources
        .iter()
        .map(|s| (s.clone(), s.priority_score()))
        .collect();

    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    sorted.first().map(|(source, _)| source.clone())
}
```

**Priority Scores:**
- P2P: 100 + reputation score (150-190 range)
- HTTP: 50
- FTP: 25

### 2. Logging

```rust
use tracing::info;

fn log_source_info(source: &DownloadSource) {
    info!(
        source_type = source.source_type(),
        source = %source,
        supports_encryption = source.supports_encryption(),
        priority = source.priority_score(),
        "Download source selected"
    );
}
```

### 3. Match Pattern Handling

```rust
match source {
    DownloadSource::P2p(info) => {
        println!("Connecting to peer: {}", info.peer_id);
        // Handle P2P download
    }
    DownloadSource::Http(info) => {
        println!("Downloading from: {}", info.url);
        // Handle HTTP download
    }
    DownloadSource::Ftp(info) => {
        println!("FTP download from: {}", info.url);
        println!("Passive mode: {}", info.passive_mode);
        println!("FTPS enabled: {}", info.use_ftps);
        // Handle FTP download
    }
}
```

### 4. Status Display

```rust
fn display_sources(sources: &[DownloadSource]) {
    for (idx, source) in sources.iter().enumerate() {
        println!(
            "{}. {} - {}",
            idx + 1,
            source.source_type(),
            source.display_name()
        );
    }
}
```

**Example Output:**
```
1. P2P - P2P peer: 12D3KooW
2. HTTP - HTTP: cdn.example.com
3. FTP - FTP: ftp.example.com
```

## Integration Points

### Multi-Source Download Service

The `download_source.rs` can be integrated with existing `multi_source_download.rs`:

```rust
use crate::download_source::DownloadSource;

pub struct MultiSourceDownload {
    sources: Vec<DownloadSource>,
    // ... existing fields
}

impl MultiSourceDownload {
    pub fn add_source(&mut self, source: DownloadSource) {
        info!(
            source_type = source.source_type(),
            "Adding download source"
        );
        self.sources.push(source);
    }
}
```

### DHT Metadata

FTP source info can be stored in DHT metadata:

```rust
use serde_json::json;

let metadata = json!({
    "file_hash": "QmHash123",
    "file_name": "file.zip",
    "sources": [
        {
            "type": "ftp",
            "url": "ftp://ftp.example.com/file.zip",
            "username": "user",
            "passiveMode": true,
            "useFtps": true
        }
    ]
});
```

### Frontend Integration

The `DownloadSource` enum serializes to JSON with tagged format:

```json
{
  "type": "ftp",
  "url": "ftp://ftp.example.com/file.tar.gz",
  "username": "anonymous",
  "passiveMode": true,
  "useFtps": false,
  "timeoutSecs": 60
}
```

## Security Considerations

### Password Encryption

FTP passwords should be encrypted before storage:

```rust
use crate::encryption;

// Encrypt password with file's AES key
let encrypted_password = encryption::encrypt_data(
    password.as_bytes(),
    &file_aes_key
)?;

let ftp_source = FtpSourceInfo {
    url: "ftp://example.com/file".to_string(),
    username: Some("user".to_string()),
    encrypted_password: Some(base64::encode(&encrypted_password)),
    // ...
};
```

### SSL/TLS Verification

- HTTP sources default to SSL verification enabled
- FTP sources can enable FTPS for encrypted connections
- Consider security implications when disabling verification

## Testing

Run tests with:

```bash
# Test core download source module
cargo test download_source --lib

# Test scheduler integration
cargo test download_scheduler --lib

# Run all tests
cargo test --lib
```

## Future Enhancements

### Planned Features

1. **Torrent Support**: Add `Torrent(TorrentSourceInfo)` variant
2. **IPFS Integration**: Add `Ipfs(IpfsSourceInfo)` variant
3. **Source Health Monitoring**: Track success/failure rates
4. **Automatic Fallback**: Switch sources on failure
5. **Bandwidth Scheduling**: Source-aware scheduling
6. **Mirror Management**: Automatic mirror discovery

### Implementation Notes

To add a new source type:

1. Define source info struct in `download_source.rs`
2. Add variant to `DownloadSource` enum
3. Update `source_type()`, `display_name()`, and `priority_score()` methods
4. Add match arms in scheduler and handlers
5. Write tests for new source type
6. Update this documentation

## Examples

### Example 1: Multi-Source Task

```rust
use crate::download_source::*;
use crate::download_scheduler::*;

let task = DownloadTask {
    task_id: "download_123".to_string(),
    file_hash: "QmFileHash".to_string(),
    file_name: "ubuntu-20.04.iso".to_string(),
    sources: vec![
        // Primary: P2P from trusted peer
        DownloadSource::P2p(P2pSourceInfo {
            peer_id: "12D3KooWHighRep".to_string(),
            multiaddr: None,
            reputation: Some(95),
            supports_encryption: true,
            protocol: Some("webrtc".to_string()),
        }),
        // Backup: Official HTTP mirror
        DownloadSource::Http(HttpSourceInfo {
            url: "https://releases.ubuntu.com/20.04/ubuntu-20.04.iso".to_string(),
            auth_header: None,
            verify_ssl: true,
            headers: None,
            timeout_secs: Some(60),
        }),
        // Fallback: FTP mirror
        DownloadSource::Ftp(FtpSourceInfo {
            url: "ftp://ftp.ubuntu.com/ubuntu/20.04/ubuntu-20.04.iso".to_string(),
            username: None, // Anonymous
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: Some(120),
        }),
    ],
    status: DownloadTaskStatus::Pending,
    priority: 100,
};

let mut scheduler = DownloadScheduler::new();
scheduler.add_task(task);

// Will select P2P source (highest priority)
let best_source = scheduler.select_best_source("download_123").unwrap();
```

### Example 2: FTP-Only Download

```rust
let ftp_task = DownloadTask {
    task_id: "ftp_download".to_string(),
    file_hash: "QmFtpFile".to_string(),
    file_name: "data.tar.gz".to_string(),
    sources: vec![
        DownloadSource::Ftp(FtpSourceInfo {
            url: "ftp://files.example.org/datasets/data.tar.gz".to_string(),
            username: Some("researcher".to_string()),
            encrypted_password: Some("encrypted_base64_pass".to_string()),
            passive_mode: true,
            use_ftps: true, // Use FTPS for security
            timeout_secs: Some(300),
        }),
    ],
    status: DownloadTaskStatus::Pending,
    priority: 50,
};
```

## Troubleshooting

### FTP Connection Issues

1. **Passive Mode**: If behind NAT, ensure `passive_mode: true`
2. **FTPS**: Some servers may not support FTPS
3. **Anonymous FTP**: Leave `username` and `encrypted_password` as `None`
4. **Timeout**: Increase `timeout_secs` for slow connections

### Logging

Enable debug logging to see source selection:

```bash
RUST_LOG=chiral_network::download_scheduler=debug cargo run
```

## References

- [RFC 959 - FTP Protocol](https://tools.ietf.org/html/rfc959)
- [RFC 4217 - FTP over TLS](https://tools.ietf.org/html/rfc4217)
- [libp2p Specs](https://github.com/libp2p/specs)
- [HTTP/1.1 RFC](https://tools.ietf.org/html/rfc7230)
