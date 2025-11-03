# BitTorrent Implementation Guideline

This document provides a detailed guide for implementing BitTorrent support within the Chiral Network. It expands on the concepts outlined in the main `architecture.md` document and provides a descriptive overview of the implementation process.

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

*   **`src-tauri/src/protocols/mod.rs`**: This file defines the `ProtocolHandler` trait, which provides a generic interface for all data transfer protocols. This ensures that the `ProtocolManager` can interact with different protocols in a consistent way. The `ProtocolHandler` trait should include methods for downloading, seeding, and managing transfers.

*   **`src-tauri/src/protocols/bittorrent.rs`**: This file will contain the `BitTorrentHandler` struct, which implements the `ProtocolHandler` trait. This struct will encapsulate all BitTorrent-specific logic, including:
    *   Parsing magnet links and `.torrent` files.
    *   Managing connections to trackers and the public BitTorrent DHT.
    *   Handling the peer-wire protocol.
    *   Downloading and verifying torrent pieces against their SHA-1 hashes.

*   **`src-tauri/src/protocols/bittorrent/parser.rs`**: This file will contain the logic for parsing `.torrent` files and magnet URIs. It will extract information such as tracker URLs, file lists, and the info hash.

## Libraries and Implementation Strategy

The current implementation uses a set of focused libraries to handle specific parts of the BitTorrent protocol:

*   **`bendy`**: For Bencode serialization and deserialization, used for parsing `.torrent` files.
*   **`bip_handshake`**: For handling the initial BitTorrent protocol handshake between peers.
*   **`bip_metainfo`**: For parsing and manipulating the metainfo of torrents.
*   **`sha1`**: For calculating the SHA-1 hashes of torrent pieces for verification.
*   **`url`**: For parsing magnet URIs.

This approach provides a high degree of control over the implementation, but it also means that much of the BitTorrent peer-wire protocol, tracker communication, and piece management needs to be implemented manually. Implementing custom extensions like BEP 10 would also require significant effort.

### Alternative: High-Level BitTorrent Library

An alternative approach is to use a full-featured BitTorrent library like `rqbit`. `rqbit` is a BitTorrent client written in Rust that can be used as a library. 

**Advantages of using `rqbit`:**

*   **Complete Solution:** It handles all the low-level details of the BitTorrent protocol, including the peer-wire protocol, tracker communication, and DHT integration.
*   **BEP 10 Support:** It has built-in support for the BitTorrent Extension Protocol (BEP 10), which would greatly simplify the implementation of the Chiral client identification feature.
*   **Faster Development:** Using a high-level library would significantly speed up the development process and reduce the amount of boilerplate code.

**Trade-offs:**

The choice between these two approaches is a trade-off between control and speed of development. Building the client from lower-level components offers maximum flexibility, while using a high-level library like `rqbit` provides a faster path to a robust and feature-complete implementation.

## Code Implementation

This section provides an overview of the Rust code for the BitTorrent implementation.

### `ProtocolHandler` Trait in `src-tauri/src/protocols/mod.rs`

This file defines the `ProtocolHandler` trait, which is the core abstraction for all protocol implementations. The trait is defined as follows:

```rust
pub trait ProtocolHandler {
    async fn download(&self, identifier: &str) -> Result<(), String>;
    async fn seed(&self, file_path: &str) -> Result<String, String>;
}
```
 
*   `download`: This function takes a generic `identifier` string, which can be a magnet link, an HTTP URL, an FTP URL, or a Chiral-specific content ID. Each protocol handler will be responsible for determining if it can handle the given identifier and then initiating the download.
*   `seed`: This function takes a file path, begins seeding it, and returns a protocol-specific identifier (e.g., a magnet URI for BitTorrent, or a Chiral content ID for the native protocol).

### `BitTorrentHandler` Implementation

The `BitTorrentHandler` will be the primary interface for all BitTorrent operations. If using a library like `rqbit`, the handler would be structured as follows:

```rust
pub struct BitTorrentHandler {
    rqbit_session: Arc<Mutex<rqbit::session::Session>>,
    // Add other necessary fields, like a channel for sending events to the rest of the application
}

impl ProtocolHandler for BitTorrentHandler {
    async fn download(&self, identifier: &str) -> Result<(), String> {
        if identifier.starts_with("magnet:") || identifier.ends_with(".torrent") {
            let session = self.rqbit_session.lock().await;
            session.add_torrent(identifier).await.map_err(|e| e.to_string())?;
            Ok(())
        }else{
            Err("Identifier not supported by BitTorrent handler".to_string())
        }
    }

    async fn seed(&self, file_path: &str) -> Result<String, String> {
        let session = self.rqbit_session.lock().await;
        // Use rqbit's API to create and seed a new torrent
        // It should return a magnet link that can be shared
        let magnet_link = session.create_and_seed_torrent(file_path).await.map_err(|e| e.to_string())?;
        Ok(magnet_link)
    }
}
```

The handler would also be responsible for subscribing to events from the `rqbit` session, such as progress updates and peer-related events, and translating them into Chiral-specific events that can be consumed by the UI and other services.

### BitTorrent Parser in `src-tauri/src/protocols/bittorrent/parser.rs`

This file contains the data structures and parsing logic for BitTorrent files. The key components are:

*   **`Torrent` enum**: Represents either a `.torrent` file (`Torrent::File`) or a magnet link (`Torrent::Magnet`).
*   **`TorrentMetadata` struct**: Holds the data from a `.torrent` file, including the tracker URL and file information.
*   **`Magnet` struct**: Holds the data from a magnet link, including the info hash and tracker URLs.
*   **`parse_torrent_file` function**: Parses a `.torrent` file from a given path and returns a `TorrentMetadata` struct.
*   **`parse_magnet_uri` function**: Parses a magnet URI and returns a `Magnet` struct.

## Implementation Details

### Core Backend (Rust Architecture & Cryptography)

The backend implementation will involve creating a new module for the BitTorrent protocol. This includes scaffolding the module, defining a generic `ProtocolHandler` trait, and implementing a `BitTorrentHandler` that encapsulates all BitTorrent-specific logic. This handler will be responsible for parsing torrent files and magnet links, managing peer connections, and handling the piece exchange.

Cryptographic verification is a critical part of this process. The implementation must include SHA-1 hash validation for torrent pieces to ensure data integrity. A shared utility for verifying chunks, compatible with both BitTorrent and Chiral's native file transfer, should be developed.

### Networking / DHT Integration

To integrate BitTorrent with the Chiral network, the DHT is extended to support torrent info hashes. This enables a dual lookup mechanism, allowing peers to be discovered via both Chiral's content IDs and BitTorrent info hashes. Peers can announce their ability to seed torrents, and this activity is integrated with the reputation system to reward reliable seeders.

#### DHT Integration Features

The DHT includes several key features to support BitTorrent integration:

*   **Dual Lookup for `info_hash`**: The DHT supports a dual lookup mechanism, allowing files to be discovered using either the Chiral Merkle root or a BitTorrent `info_hash`. When a file is published with an `info_hash`, a secondary index is created in the DHT that maps the `info_hash` to the Merkle root. This allows users to download files using standard magnet links.

*   **Protocol-Aware Peer Selection**: The peer selection process is protocol-aware. The `PeerMetrics` have been updated to track the protocols supported by each peer. This allows the client to filter peers based on the required transfer protocol, ensuring that it only connects to peers that can handle the desired transfer method (e.g., WebRTC, Bitswap).

*   **DHT Key Namespacing**: To prevent conflicts between different types of records in the DHT, key namespacing is used. Prefixes such as `info_hash_idx::` and `keyword_idx::` are used to ensure that `info_hash` lookups, keyword searches, and other record types do not collide.

### Frontend / UI (Svelte)

The user interface will be updated to provide a seamless experience for BitTorrent users. A new "Torrents" section will be added to the application, with UI elements for adding downloads via magnet links or `.torrent` files. The UI will also display download progress, including metrics like ETA and peer count, and provide controls for filtering sources.

### System Integration (Tauri + ProtocolManager)

The `BitTorrentHandler` will be registered with the `ProtocolManager` to enable it to handle torrent-related tasks. New Tauri commands will be created to expose the BitTorrent functionality to the frontend, allowing users to initiate downloads and seeding from the UI.

The multi-source download engine will be updated to treat the BitTorrent swarm as a valid source, and to handle torrent pieces in addition to byte ranges. Fallback logic will be implemented to ensure that downloads can continue from other sources if the BitTorrent swarm is unavailable.

### Testing & Validation

Thorough testing is essential to ensure the stability and performance of the BitTorrent implementation. This includes unit tests for the backend logic, integration tests to validate the interaction between the different components, and end-to-end tests using real torrents. Performance benchmarks will be conducted to compare download speeds with and without BitTorrent, and to evaluate the efficiency of the multi-source download engine.

## Additional Future Implementations

Beyond the core implementation, several other aspects are crucial for a robust and user-friendly BitTorrent integration.

### Payment Integration

Given the decoupled payment architecture, the `BitTorrentHandler` must be able to track data transfer on a per-peer basis. This includes:

*   **Data Transfer Accounting:** Accurately measure the amount of data uploaded and downloaded from each peer in the BitTorrent swarm.
*   **Transaction Triggering:** Trigger payment transactions on the Chiral blockchain at appropriate intervals (e.g., after a certain amount of data has been transferred, or at the end of the download).

### Multi-Source Download Strategy

The `multi_source_download.rs` engine should be enhanced with a sophisticated strategy for handling downloads from multiple sources (e.g., a BitTorrent swarm and a Chiral peer) simultaneously. This includes:

*   **Parallel Downloading:** Download different pieces of the file from multiple sources in parallel to maximize bandwidth.
*   **Piece Selection Logic:** Implement intelligent piece selection to avoid downloading the same piece from multiple sources and to prioritize rare pieces.

### Security Considerations

In addition to the Chiral client identification, the following security measures should be considered:

*   **Peer Message Validation:** Validate all messages received from peers in the BitTorrent swarm to prevent potential exploits.
*   **Privacy (Optional):** Take measures to protect user privacy, such as using a VPN or a proxy for BitTorrent traffic.

### Seeding and File Lifecycle

The process of creating and seeding new torrents should be clearly defined:

*   **Torrent Creation:** Implement a user-friendly way for users to create a `.torrent` file for a local file or directory.
*   **Magnet Link Generation:** Automatically generate a magnet link for newly created torrents.
*   **Seeding Management:** Provide a clear interface for users to manage their seeded files, including the ability to start, stop, and remove them.

## Chiral Client Identification in BitTorrent Swarms

To enable Chiral clients to identify and prioritize each other within a public BitTorrent swarm, a custom extension to the BitTorrent protocol will be implemented, based on **BEP 10 (BitTorrent Extension Protocol)**.

*   **Handshake Extension:** Chiral clients will set a reserved bit in the BitTorrent handshake to indicate support for protocol extensions.
*   **Extended Handshake:** Chiral clients will include a "chiral" identifier in the extended handshake message.
*   **Identification Message:** If two peers both support the "chiral" extension, they can exchange custom messages containing their Chiral peer ID and reputation score.
*   **Prioritization:** Once a peer is verified as a Chiral client, the application can prioritize it for piece requests, creating a "fast lane" for transfers between Chiral users.

Standard BitTorrent clients will ignore this extension, ensuring full compatibility with the public swarm.

### BEP 10 Custom Extension

To implement the Chiral client identification, a custom BEP 10 extension will be used. This will allow Chiral clients to identify each other and create a prioritized "fast lane" for transfers.

**Extended Handshake:**

The extended handshake message should include a `chiral` identifier. The `m` dictionary in the extended handshake message would look something like this:

```json
{
    "m": {
        "chiral": 1
    }
}
```

**Custom Chiral Message:**

Once two peers have identified each other as Chiral clients, they can exchange a custom message. This message should be Bencoded and could have the following structure:

```json
{
    "msg_type": 1, // Custom message type for Chiral info
    "chiral_peer_id": "<Chiral peer ID>",
    "reputation_score": 4.5,
    "wallet_address": "<Chiral wallet address>"
}
```

The `BitTorrentHandler` will be responsible for sending this message after a successful extended handshake and for parsing the message received from other Chiral peers. This information can then be used by the peer selection logic to prioritize connections to other Chiral clients.

## Payment Integration with `BitTorrentHandler`

The `BitTorrentHandler` needs to be integrated with the payment system to reward peers for seeding. This can be achieved by:

1.  **Monitoring Data Transfer:** The handler should subscribe to events from the BitTorrent library that provide information about the amount of data uploaded and downloaded to/from each peer.
2.  **Accumulating Data:** The handler should maintain a data structure (e.g., a `HashMap`) to track the total data transferred for each peer.
3.  **Triggering Payments:** When the amount of data transferred to a peer reaches a certain threshold (e.g., 1 MB), or after a certain time interval, the handler should emit a `PaymentRequired` event. This event should contain the peer's wallet address and the amount of data transferred.
4.  **Handling the Event:** A separate payment service will listen for `PaymentRequired` events and handle the process of creating and sending the transaction on the Chiral blockchain.

## `multi_source_download.rs` Strategy with BitTorrent

The multi-source download engine will treat the BitTorrent swarm as a single, powerful source of pieces. The strategy will be as follows:

1.  **Unified Piece View:** When a download starts, the engine will get a list of available pieces from the `BitTorrentHandler` (representing the entire swarm) and from any other sources (e.g., Chiral peers).
2.  **Prioritized Piece Selection:** The engine will use a unified piece selection algorithm that prioritizes pieces based on:
    *   **Rarity:** Request the rarest pieces first to improve the health of the swarm.
    *   **Source Reputation:** Prioritize pieces from Chiral peers with high reputation scores.
    *   **Source Speed:** Prioritize sources that are providing data at a higher speed.
3.  **Parallel Downloads:** The engine will download pieces from multiple sources in parallel, constantly re-evaluating the best source for each piece based on the factors above.