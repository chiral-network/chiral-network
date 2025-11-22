//! Protocol Handlers Module
//!
//! This module provides a unified interface for different file transfer protocols
//! used in the Chiral Network. Each protocol implements the `ProtocolHandler` trait,
//! allowing for consistent handling of downloads and seeding operations.
//!
//! ## Supported Protocols
//!
//! - **BitTorrent**: Magnet links and .torrent files, with DHT support
//! - **HTTP/HTTPS**: Direct file downloads with range request support
//! - **FTP/FTPS**: FTP server downloads with resume capability
//! - **ED2K**: eDonkey2000 protocol with chunk-based downloads
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::protocols::{ProtocolManager, DownloadOptions};
//!
//! // Create a protocol manager
//! let mut manager = ProtocolManager::new();
//!
//! // Register handlers
//! manager.register(Box::new(HttpProtocolHandler::new()?));
//! manager.register(Box::new(BitTorrentProtocolHandler::with_download_directory(dir).await?));
//!
//! // Download a file
//! let handle = manager.download(
//!     "https://example.com/file.zip",
//!     DownloadOptions::default(),
//! ).await?;
//! ```

pub mod traits;
pub mod bittorrent;
pub mod http;
pub mod ftp;
pub mod ed2k;

// Re-export commonly used types
pub use traits::{
    ProtocolHandler,
    ProtocolManager,
    ProtocolCapabilities,
    ProtocolError,
    DownloadHandle,
    DownloadOptions,
    DownloadProgress,
    DownloadStatus,
    SeedOptions,
    SeedingInfo,
    // Legacy exports for backward compatibility
    SimpleProtocolHandler,
    SimpleProtocolManager,
};

// Re-export legacy trait with the old name for backward compatibility
// This allows existing code like bittorrent_handler.rs to continue working
#[doc(hidden)]
#[deprecated(note = "Use SimpleProtocolHandler or ProtocolHandler instead")]
pub use traits::SimpleProtocolHandler as LegacyProtocolHandler;

pub use bittorrent::BitTorrentProtocolHandler;
pub use http::HttpProtocolHandler;
pub use ftp::FtpProtocolHandler;
pub use ed2k::Ed2kProtocolHandler;

/// Creates a protocol manager with all available handlers registered
///
/// This is a convenience function for setting up a fully-configured protocol manager.
///
/// # Arguments
///
/// * `download_dir` - Directory for BitTorrent downloads
/// * `ed2k_server` - ED2K server URL (optional, if None ED2K is not registered)
///
/// # Example
///
/// ```rust,ignore
/// let manager = create_protocol_manager(
///     PathBuf::from("./downloads"),
///     Some("ed2k://|server|192.168.1.1|4661|/".to_string()),
/// ).await?;
/// ```
pub async fn create_protocol_manager(
    download_dir: std::path::PathBuf,
    ed2k_server: Option<String>,
) -> Result<ProtocolManager, ProtocolError> {
    let mut manager = ProtocolManager::new();

    // Register HTTP handler
    manager.register(Box::new(HttpProtocolHandler::new()?));

    // Register FTP handler
    manager.register(Box::new(FtpProtocolHandler::new()));

    // Register BitTorrent handler
    let bt_handler = BitTorrentProtocolHandler::with_download_directory(download_dir).await?;
    manager.register(Box::new(bt_handler));

    // Register ED2K handler if server is provided
    if let Some(server) = ed2k_server {
        manager.register(Box::new(Ed2kProtocolHandler::new(server)));
    }

    Ok(manager)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_identification() {
        let http = HttpProtocolHandler::new().unwrap();
        let ftp = FtpProtocolHandler::new();

        // HTTP
        assert!(http.supports("http://example.com/file.zip"));
        assert!(http.supports("https://example.com/file.zip"));
        assert!(!http.supports("ftp://example.com/file.zip"));

        // FTP
        assert!(ftp.supports("ftp://example.com/file.zip"));
        assert!(ftp.supports("ftps://example.com/file.zip"));
        assert!(!ftp.supports("http://example.com/file.zip"));
    }

    #[test]
    fn test_protocol_names() {
        let http = HttpProtocolHandler::new().unwrap();
        let ftp = FtpProtocolHandler::new();

        assert_eq!(http.name(), "http");
        assert_eq!(ftp.name(), "ftp");
    }

    #[test]
    fn test_protocol_capabilities() {
        let http = HttpProtocolHandler::new().unwrap();
        let ftp = FtpProtocolHandler::new();

        let http_caps = http.capabilities();
        assert!(!http_caps.supports_seeding);
        assert!(http_caps.supports_encryption); // HTTPS

        let ftp_caps = ftp.capabilities();
        assert!(ftp_caps.supports_seeding);
        assert!(ftp_caps.supports_pause_resume);
    }
}
