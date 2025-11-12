# BitTorrent Implementation Guideline

This document provides a detailed guide for implementing BitTorrent support within the Chiral Network using the `rqbit` library. It expands on the concepts outlined in the main `architecture.md` document and provides a descriptive overview of the implementation process.

## Core Concepts

### Dual-Network Approach

The Chiral client operates as a bridge between two separate P2P networks:

*   **Chiral P2P Network (libp2p):** Used for discovering other Chiral peers, managing reputation, and coordinating transfers and payments within the Chiral ecosystem.
*   **Public BitTorrent Network:** The global, public swarm of all BitTorrent clients. The Chiral client connects to this network to download and seed files like a standard BitTorrent client.

This dual-network approach allows the `ProtocolManager` to query both networks simultaneously, combining the speed and trust of the private Chiral network with the vast reach of the public one for faster, more resilient downloads.

### ProtocolManager

The `ProtocolManager` is the central orchestrator for all data transfer protocols. It identifies the source type (e.g., a `magnet:` link) and delegates the download or seeding task to the appropriate handler, in this case, the `BitTorrentHandler`.

## File Descriptions

This section describes the purpose of the key files involved in the BitTorrent implementation.

*   **`src-tauri/src/protocols.rs`**: This file defines the `ProtocolHandler` trait, which provides a generic interface for all data transfer protocols. This ensures that the `ProtocolManager` can interact with different protocols in a consistent way.

*   **`src-tauri/src/bittorrent_handler.rs`**: This file contains the `BitTorrentHandler` struct, which implements the `ProtocolHandler` trait. This struct encapsulates all BitTorrent-specific logic, using the `rqbit` library.

## Libraries and Implementation Strategy

To ensure a robust and feature-complete BitTorrent implementation, the Chiral Network will use the `rqbit` library. `rqbit` is a high-level, asynchronous BitTorrent client library for Rust.

**Advantages of using `rqbit`:**

*   **Complete Solution:** It handles all the low-level details of the BitTorrent protocol, including the peer-wire protocol, tracker communication, and DHT integration.
*   **BEP 10 Support:** It has built-in support for the BitTorrent Extension Protocol (BEP 10), which will greatly simplify the implementation of the Chiral client identification feature.
*   **Faster Development:** Using a high-level library will significantly speed up the development process and reduce the amount of boilerplate code.
*   **Asynchronous API:** `rqbit`'s async API integrates well with the existing Tokio-based runtime in the Chiral Network.

The previous approach of using low-level libraries like `bendy`, `bip_handshake`, and `bip_metainfo` will be abandoned in favor of `rqbit`.

## Code Implementation

This section provides an overview of the Rust code for the BitTorrent implementation using `rqbit`.

### `ProtocolHandler` Trait in `src-tauri/src/protocols.rs`

This file defines the `ProtocolHandler` trait, which is the core abstraction for all protocol implementations. The trait is defined as follows:

```rust
use async_trait::async_trait;
use std::sync::Arc;

/// A trait for handling a specific download/upload protocol like BitTorrent or HTTP.
#[async_trait]
pub trait ProtocolHandler: Send + Sync {
    /// Returns the name of the protocol (e.g., "bittorrent", "http").
    fn name(&self) -> &'static str;

    /// Determines if this handler can process the given identifier (e.g., a URL or magnet link).
    fn supports(&self, identifier: &str) -> bool;

    /// Initiates a download for the given identifier.
    async fn download(&self, identifier: &str) -> Result<(), String>;

    /// Starts seeding a file and returns an identifier (e.g., magnet link) for others to use.
    async fn seed(&self, file_path: &str) -> Result<String, String>;
}
```

### `BitTorrentHandler` Implementation with `rqbit`

The `BitTorrentHandler` will be the primary interface for all BitTorrent operations. It will be implemented in `src-tauri/src/bittorrent_handler.rs` and will use `rqbit` to handle the core BitTorrent logic.

```rust
use crate::protocols::ProtocolHandler;
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, error, instrument};
use rqbit::session::Session;
use rqbit::torrent::Torrent;
use rqbit::AddTorrent;

/// BitTorrent protocol handler implementing the ProtocolHandler trait.
/// This handler manages BitTorrent downloads and seeding operations using rqbit.
pub struct BitTorrentHandler {
    rqbit_session: Arc<Session>,
    download_directory: std::path::PathBuf,
}

impl BitTorrentHandler {
    /// Creates a new BitTorrentHandler with the specified download directory.
    pub async fn new(download_directory: std::path::PathBuf) -> Result<Self, String> {
        let session = Session::new(download_directory.clone()).await.map_err(|e| e.to_string())?;
        info!("Initializing BitTorrentHandler with download directory: {:?}", download_directory);
        Ok(Self {
            rqbit_session: Arc::new(session),
            download_directory,
        })
    }
}

#[async_trait]
impl ProtocolHandler for BitTorrentHandler {
    fn name(&self) -> &'static str {
        "bittorrent"
    }

    fn supports(&self, identifier: &str) -> bool {
        identifier.starts_with("magnet:") || identifier.ends_with(".torrent")
    }

    #[instrument(skip(self), fields(protocol = "bittorrent"))]
    async fn download(&self, identifier: &str) -> Result<(), String> {
        info!("Starting BitTorrent download for: {}", identifier);
        let add_torrent = if identifier.starts_with("magnet:") {
            AddTorrent::from_url(identifier)
        } else {
            AddTorrent::from_file(identifier).map_err(|e| e.to_string())?
        };

        let handle = self.rqbit_session.add_torrent(add_torrent, None).await.map_err(|e| e.to_string())?;

        // TODO: Add logic to monitor the download progress and handle completion.
        // This can be done by periodically checking the status of the torrent
        // using the handle, or by using an event stream from rqbit if available.

        info!("BitTorrent download started for: {}", identifier);
        Ok(())
    }

    #[instrument(skip(self), fields(protocol = "bittorrent"))]
    async fn seed(&self, file_path: &str) -> Result<String, String> {
        info!("Starting to seed file: {}", file_path);

        let path = Path::new(file_path);
        if !path.exists() {
            return Err(format!("File does not exist: {}", file_path));
        }

        let add_torrent = AddTorrent::from_file(file_path).map_err(|e| e.to_string())?;
        let handle = self.rqbit_session.add_torrent(add_torrent, None).await.map_err(|e| e.to_string())?;
        
        let magnet_link = handle.magnet_link().await.ok_or("Couldn't get magnet link")?;

        info!("Seeding started for file: {} with magnet link: {}", file_path, magnet_link);
        
        Ok(magnet_link)
    }
}
```

## Refactoring the Existing Implementation

The current `bittorrent_handler.rs` contains a simulated implementation. This will be replaced with the `rqbit`-based implementation above. The refactoring process will involve the following steps:

1.  **Add `rqbit` to `Cargo.toml`:** Add `rqbit = "0.1"` to the `[dependencies]` section of `src-tauri/Cargo.toml`.
2.  **Remove Old Helper Functions:** The helper functions `is_magnet_link`, `is_torrent_file`, `extract_info_hash`, `create_magnet_link`, and `verify_assembled_file_integrity` in `bittorrent_handler.rs` will be removed, as `rqbit` will handle this functionality.
3.  **Update `BitTorrentHandler::new()`:** The constructor will be updated to initialize the `rqbit::Session`.
4.  **Update `download()` and `seed()`:** The `download` and `seed` methods will be updated to use the `rqbit` session to add and manage torrents.
5.  **Update Tests:** The existing tests in `bittorrent_handler.rs` will need to be updated to work with the new `rqbit`-based implementation. This will involve creating a real `rqbit` session for testing and asserting that torrents are added and managed correctly. The old tests for the helper functions can be removed.

## Implementation Details

### Core Backend (Rust Architecture & Cryptography)

The backend implementation will be centered around the `BitTorrentHandler` in `src-tauri/src/bittorrent_handler.rs`. This handler will be responsible for all BitTorrent-related operations, delegating the complex protocol logic to `rqbit`.

Cryptographic verification of torrent pieces is handled automatically by `rqbit`, which validates the SHA-1 hash of each piece as it is downloaded.

### Networking / DHT Integration

`rqbit` has its own DHT implementation, which will be used for peer discovery on the public BitTorrent network. To integrate with the Chiral network, the Chiral DHT will be used to find other Chiral peers who are seeding the same torrent.

#### DHT Integration Features

The Chiral DHT will be used to enhance, not replace, the public BitTorrent DHT.

*   **Dual Lookup for `info_hash`**: The Chiral DHT will store mappings from a torrent's `info_hash` to the Chiral peer IDs of seeders. This allows Chiral clients to find each other directly.
*   **Protocol-Aware Peer Selection**: The `PeerMetrics` in the Chiral network will be used to prioritize Chiral peers found through the Chiral DHT.

### Frontend / UI (Svelte)

The user interface will be updated to provide a seamless experience for BitTorrent users. A new "Torrents" section will be added to the application, with UI elements for adding downloads via magnet links or `.torrent` files. The UI will also display download progress, including metrics like ETA and peer count, and provide controls for filtering sources. This will be powered by events sent from the `BitTorrentHandler` to the frontend.

### System Integration (Tauri + ProtocolManager)

The `BitTorrentHandler` will be registered with the `ProtocolManager` to enable it to handle torrent-related tasks. New Tauri commands will be created to expose the BitTorrent functionality to the frontend, allowing users to initiate downloads and seeding from the UI.

The multi-source download engine will be updated to treat the BitTorrent swarm as a valid source, and to handle torrent pieces in addition to byte ranges. Fallback logic will be implemented to ensure that downloads can continue from other sources if the BitTorrent swarm is unavailable.

### Event Handling and Progress Reporting

**User Expectation:** Standard. A user expects to see real-time feedback on their downloads.

For a good user experience, the UI must display real-time download progress.

*   **Creating a Monitoring Task:** Spawn a separate asynchronous task for each download that periodically polls the `TorrentHandle` provided by `rqbit` for statistics (e.g., download speed, downloaded bytes, peer count).
*   **Sending Events to the Frontend:** Use Tauri's event system to send progress updates from the monitoring task to the Svelte frontend.
*   **Handling Different Torrent States:** Handle different states of a torrent, such as "downloading", "seeding", "paused", and "error", and communicate these states to the UI.

### Configuration

**User Expectation:** Advanced. Power users may expect to configure client behavior.

A robust implementation should allow users to configure the BitTorrent client's behavior.

*   **Exposing `rqbit` Options:** Expose `rqbit`'s session settings (e.g., listen port, download/upload rate limits) to the user through the Chiral Network's settings page.
*   **Storing and Loading Configuration:** Use `tauri-plugin-store` to persist the user's BitTorrent settings and load them when the application starts.
*   **Applying Configuration:** Apply the saved settings when initializing the `rqbit::Session`.

### Advanced Error Handling

**User Expectation:** Standard for basic error handling, Advanced for a more robust custom error handling system.

*   **Custom Error Types:** Create custom error types for the `BitTorrentHandler` instead of just returning strings. This will make error handling more precise and easier to debug.
*   **User-Friendly Error Messages:** Map different types of errors (e.g., invalid magnet link, file not found, tracker error) to user-friendly messages that can be displayed in the UI.

### Testing & Validation

Thorough testing is essential to ensure the stability and performance of the `rqbit`-based implementation.

*   **Unit Tests:** For the logic within `BitTorrentHandler` that is not directly part of `rqbit`.
*   **Integration Tests:** To validate the interaction between `BitTorrentHandler`, `ProtocolManager`, and the Tauri API. This should include using a real `rqbit` session with a test torrent.
*   **End-to-End Tests:** Using real torrents to test the full download and seed lifecycle.
*   **Advanced Testing:** Write integration tests that mock the Tauri event system to verify that progress events are being sent correctly.

## Additional Future Implementations

### Payment Integration

**User Expectation:** Advanced. This is a core feature of the Chiral ecosystem, but it is an advanced implementation detail.

Given the decoupled payment architecture, the `BitTorrentHandler` must be able to track data transfer on a per-peer basis.

*   **Using `rqbit` Statistics:** Use the peer statistics from `rqbit` to track the amount of data transferred to and from each peer. The `TorrentHandle` provides methods to get peer information.
*   **Triggering Payments:** The `BitTorrentHandler` should emit a `PaymentRequired` event with the peer's ID and the amount of data transferred. A separate payment service will handle the blockchain transaction.

### Multi-Source Download Strategy

The `multi_source_download.rs` engine should be enhanced with a sophisticated strategy for handling downloads from multiple sources (e.g., a BitTorrent swarm and a Chiral peer) simultaneously. This includes:

*   **Parallel Downloading:** Download different pieces of the file from multiple sources in parallel to maximize bandwidth.
*   **Piece Selection Logic:** Implement intelligent piece selection to avoid downloading the same piece from multiple sources and to prioritize rare pieces.

### Chiral Client Identification in BitTorrent Swarms

**User Expectation:** Advanced. This is a unique feature of the Chiral Network.

`rqbit`'s support for BEP 10 makes it possible to implement a custom extension for Chiral client identification.

*   **Extended Handshake:** Chiral clients will include a "chiral" identifier in the extended handshake message.
*   **Identification Message:** If two peers both support the "chiral" extension, they can exchange custom messages containing their Chiral peer ID and reputation score.
*   **Prioritization:** Once a peer is verified as a Chiral client, the application can prioritize it for piece requests.


### Seeding and File Lifecycle

The process of creating and seeding new torrents will be handled by `rqbit`. The `BitTorrentHandler` will expose this functionality through the `seed` method, which will:

*   **Torrent Creation:** Use `rqbit` to create a `.torrent` file for a local file or directory.
*   **Magnet Link Generation:** Use `rqbit` to generate a magnet link for the new torrent.
*   **Seeding Management:** Provide a clear interface for users to manage their seeded files, including the ability to start, stop, and remove them.