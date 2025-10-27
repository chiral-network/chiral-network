//! # Protocol Handler Abstraction
//!
//! This module defines the generic interface for all file transfer protocols
//! supported by the Chiral Network. It ensures that different protocols
//! like BitTorrent, HTTP, or WebTorrent can be used interchangeably
//! by the core application logic.

use self::bittorrent::Torrent;

pub mod bittorrent;
// pub mod http; // Placeholder for future HTTP handler
// pub mod webtorrent; // Placeholder for future WebTorrent handler

/// A generic trait for handling file transfers across different protocols.
///
/// This trait abstracts the core functionalities of downloading and seeding files,
/// allowing the network to interact with various protocols through a unified interface. It
/// operates on a `Torrent` enum, which can represent data from a `.torrent` file or a magnet link.
pub trait ProtocolHandler {
    /// Adds a torrent to be downloaded or seeded.
    async fn add_torrent(&self, torrent: Torrent) -> Result<(), String>;

    /// Begins seeding a file from a local path and returns a protocol-specific identifier.
    /// This identifier (e.g., magnet URI) is what other peers will use to find and download the file.
    async fn seed(&self, file_path: &str) -> Result<String, String>;
}
