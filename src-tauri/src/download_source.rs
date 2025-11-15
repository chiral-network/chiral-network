// download_source.rs
// Unified Source Abstraction for multi-source downloads
//
// This module defines a unified interface for different download sources
// (P2P, HTTP, FTP, etc.) that can be used throughout the application.

use serde::{Deserialize, Serialize};

/// Represents different types of download sources
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DownloadSource {
    /// Peer-to-peer download via libp2p/WebRTC
    P2p(P2pSourceInfo),

    /// HTTP/HTTPS download
    Http(HttpSourceInfo),

    /// FTP/FTPS download
    Ftp(FtpSourceInfo),

    /// ed2k (eDonkey2000) download
    Ed2k(Ed2kSourceInfo),

    /// BitTorrent download
    BitTorrent(BitTorrentSourceInfo),
}

/// Information about a P2P download source
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct P2pSourceInfo {
    /// Peer ID in the P2P network
    pub peer_id: String,

    /// Multiaddress for connecting to the peer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiaddr: Option<String>,

    /// Peer reputation score (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reputation: Option<u8>,

    /// Whether this peer supports encryption
    #[serde(default)]
    pub supports_encryption: bool,

    /// Protocol used (webrtc, tcp, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
}

/// Information about an HTTP download source
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpSourceInfo {
    /// Full HTTP/HTTPS URL
    pub url: String,

    /// Optional authentication headers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_header: Option<String>,

    /// Whether to verify SSL certificates
    #[serde(default = "default_verify_ssl")]
    pub verify_ssl: bool,

    /// Custom headers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<(String, String)>>,

    /// Timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

/// Information about an FTP download source
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FtpSourceInfo {
    /// FTP URL (e.g., "ftp://ftp.example.com/path/to/file")
    pub url: String,

    /// FTP username (optional for anonymous FTP)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// Encrypted password (Base64-encoded AES-GCM-SIV encrypted)
    /// Encryption key should be derived from the file's AES key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_password: Option<String>,

    /// Whether to use passive mode
    #[serde(default = "default_passive_mode")]
    pub passive_mode: bool,

    /// Whether to use FTPS (FTP over TLS/SSL)
    #[serde(default)]
    pub use_ftps: bool,

    /// Connection timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

/// Information about an ed2k (eDonkey2000) download source
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ed2kSourceInfo {
    /// ed2k server URL (e.g., "ed2k://|server|176.103.48.36|4661|/")
    pub server_url: String,

    /// ed2k file hash (MD4 hash in hex format)
    pub file_hash: String,

    /// File size in bytes
    pub file_size: u64,

    /// Optional file name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,

    /// List of known sources (IP:Port pairs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<String>>,

    /// Connection timeout in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

/// Information about a BitTorrent download source
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitTorrentSourceInfo {
    /// Magnet URI for the torrent
    pub magnet_uri: String,
}


// Default value functions
fn default_verify_ssl() -> bool {
    true
}

fn default_passive_mode() -> bool {
    true
}

impl DownloadSource {
    /// Returns a human-readable name for the source type
    pub fn source_type(&self) -> &'static str {
        match self {
            DownloadSource::P2p(_) => "P2P",
            DownloadSource::Http(_) => "HTTP",
            DownloadSource::Ftp(_) => "FTP",
            DownloadSource::Ed2k(_) => "ED2K",
            DownloadSource::BitTorrent(_) => "BitTorrent",
        }
    }

    /// Returns a display string for logging and UI
    pub fn display_name(&self) -> String {
        match self {
            DownloadSource::P2p(info) => {
                format!("P2P peer: {}", &info.peer_id[..8.min(info.peer_id.len())])
            }
            DownloadSource::Http(info) => {
                // Extract domain from URL for display
                if let Some(domain) = extract_domain(&info.url) {
                    format!("HTTP: {}", domain)
                } else {
                    format!("HTTP: {}", info.url)
                }
            }
            DownloadSource::Ftp(info) => {
                // Extract host from FTP URL
                if let Some(host) = extract_domain(&info.url) {
                    format!("FTP: {}", host)
                } else {
                    format!("FTP: {}", info.url)
                }
            }
            DownloadSource::Ed2k(info) => {
                // Display file name if available, otherwise show hash prefix
                if let Some(name) = &info.file_name {
                    format!("ED2K: {}", name)
                } else {
                    format!("ED2K: {}", &info.file_hash[..8.min(info.file_hash.len())])
                }
            }
            DownloadSource::BitTorrent(info) => {
                // Extract info hash from magnet link for display
                if let Some(xt) = info.magnet_uri.split('&').find(|s| s.starts_with("xt=urn:btih:")) {
                    let info_hash = &xt[11..];
                    format!("BitTorrent: {}", &info_hash[..8.min(info_hash.len())])
                } else {
                    "BitTorrent".to_string()
                }
            }
        }
    }

    /// Returns the source identifier (peer ID, URL, etc.)
    pub fn identifier(&self) -> String {
        match self {
            DownloadSource::P2p(info) => info.peer_id.clone(),
            DownloadSource::Http(info) => info.url.clone(),
            DownloadSource::Ftp(info) => info.url.clone(),
            DownloadSource::Ed2k(info) => {
                // Use file hash as identifier
                info.file_hash.clone()
            }
            DownloadSource::BitTorrent(info) => info.magnet_uri.clone(),
        }
    }

    /// Checks if this source supports encryption
    pub fn supports_encryption(&self) -> bool {
        match self {
            DownloadSource::P2p(info) => info.supports_encryption,
            DownloadSource::Http(info) => info.url.starts_with("https://"),
            DownloadSource::Ftp(info) => info.use_ftps,
            DownloadSource::Ed2k(_) => false, // ed2k protocol does not natively support encryption
            DownloadSource::BitTorrent(_) => true, // BitTorrent protocol has its own encryption
        }
    }

    /// Returns priority score for source selection (higher is better)
    pub fn priority_score(&self) -> u32 {
        match self {
            DownloadSource::P2p(info) => {
                // P2P is preferred, bonus for high reputation
                100 + info.reputation.unwrap_or(50) as u32
            }
            DownloadSource::BitTorrent(_) => {
                // BitTorrent is a high-priority source
                90
            }
            DownloadSource::Http(_) => {
                // HTTP is secondary
                50
            }
            DownloadSource::Ed2k(_) => {
                // ed2k is between FTP and HTTP (P2P nature but legacy protocol)
                30
            }
            DownloadSource::Ftp(_) => {
                // FTP is fallback
                25
            }
        }
    }
}

impl std::fmt::Display for DownloadSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// Helper functions

/// Extract domain/host from URL
fn extract_domain(url: &str) -> Option<String> {
    // Simple domain extraction (not using full URL parser to avoid dependencies)
    if let Some(after_protocol) = url.split("://").nth(1) {
        if let Some(domain) = after_protocol.split('/').next() {
            // Remove port if present
            if let Some(host) = domain.split(':').next() {
                return Some(host.to_string());
            }
            return Some(domain.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_p2p_source_creation() {
        let source = DownloadSource::P2p(P2pSourceInfo {
            peer_id: "12D3KooWExample".to_string(),
            multiaddr: Some("/ip4/127.0.0.1/tcp/4001".to_string()),
            reputation: Some(85),
            supports_encryption: true,
            protocol: Some("webrtc".to_string()),
        });

        assert_eq!(source.source_type(), "P2P");
        assert!(source.supports_encryption());
        assert!(source.priority_score() > 100);
    }

    #[test]
    fn test_http_source_creation() {
        let source = DownloadSource::Http(HttpSourceInfo {
            url: "https://example.com/file.zip".to_string(),
            auth_header: None,
            verify_ssl: true,
            headers: None,
            timeout_secs: Some(30),
        });

        assert_eq!(source.source_type(), "HTTP");
        assert!(source.supports_encryption());
        assert_eq!(source.priority_score(), 50);
    }

    #[test]
    fn test_ftp_source_creation() {
        let source = DownloadSource::Ftp(FtpSourceInfo {
            url: "ftp://ftp.example.com/pub/file.tar.gz".to_string(),
            username: Some("anonymous".to_string()),
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: Some(60),
        });

        assert_eq!(source.source_type(), "FTP");
        assert!(!source.supports_encryption());
        assert_eq!(source.priority_score(), 25);
        assert_eq!(source.display_name(), "FTP: ftp.example.com");
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            extract_domain("https://example.com/path/to/file"),
            Some("example.com".to_string())
        );
        assert_eq!(
            extract_domain("ftp://ftp.example.org:21/file"),
            Some("ftp.example.org".to_string())
        );
        assert_eq!(extract_domain("invalid"), None);
    }

    #[test]
    fn test_display_name() {
        let p2p = DownloadSource::P2p(P2pSourceInfo {
            peer_id: "12D3KooWABC123".to_string(),
            multiaddr: None,
            reputation: None,
            supports_encryption: false,
            protocol: None,
        });
        assert_eq!(p2p.display_name(), "P2P peer: 12D3KooW");

        let http = DownloadSource::Http(HttpSourceInfo {
            url: "https://cdn.example.com/files/data.zip".to_string(),
            auth_header: None,
            verify_ssl: true,
            headers: None,
            timeout_secs: None,
        });
        assert_eq!(http.display_name(), "HTTP: cdn.example.com");
    }

    #[test]
    fn test_ed2k_source_creation() {
        let source = DownloadSource::Ed2k(Ed2kSourceInfo {
            server_url: "ed2k://|server|176.103.48.36|4661|/".to_string(),
            file_hash: "31D6CFE0D16AE931B73C59D7E0C089C0".to_string(),
            file_size: 9728000,
            file_name: Some("ubuntu.iso".to_string()),
            sources: Some(vec!["192.168.1.1:4662".to_string()]),
            timeout_secs: Some(30),
        });

        assert_eq!(source.source_type(), "ED2K");
        assert!(!source.supports_encryption());
        assert_eq!(source.priority_score(), 30);
        assert_eq!(source.display_name(), "ED2K: ubuntu.iso");
    }

    #[test]
    fn test_ed2k_source_without_filename() {
        let source = DownloadSource::Ed2k(Ed2kSourceInfo {
            server_url: "ed2k://|server|176.103.48.36|4661|/".to_string(),
            file_hash: "31D6CFE0D16AE931B73C59D7E0C089C0".to_string(),
            file_size: 9728000,
            file_name: None,
            sources: None,
            timeout_secs: Some(30),
        });

        assert_eq!(source.display_name(), "ED2K: 31D6CFE0");
        assert_eq!(source.identifier(), "31D6CFE0D16AE931B73C59D7E0C089C0");
    }

    #[test]
    fn test_priority_scores() {
        let ed2k = DownloadSource::Ed2k(Ed2kSourceInfo {
            server_url: "ed2k://|server|176.103.48.36|4661|/".to_string(),
            file_hash: "31D6CFE0D16AE931B73C59D7E0C089C0".to_string(),
            file_size: 1024,
            file_name: None,
            sources: None,
            timeout_secs: None,
        });

        let ftp = DownloadSource::Ftp(FtpSourceInfo {
            url: "ftp://ftp.example.com/file.zip".to_string(),
            username: None,
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: None,
        });

        let http = DownloadSource::Http(HttpSourceInfo {
            url: "https://example.com/file.zip".to_string(),
            auth_header: None,
            verify_ssl: true,
            headers: None,
            timeout_secs: None,
        });

        let p2p = DownloadSource::P2p(P2pSourceInfo {
            peer_id: "12D3KooW".to_string(),
            multiaddr: None,
            reputation: Some(80),
            supports_encryption: true,
            protocol: None,
        });

        let bittorrent = DownloadSource::BitTorrent(BitTorrentSourceInfo {
            magnet_uri: "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10".to_string(),
        });

        // Verify priority order: P2P (180) > BitTorrent (90) > HTTP (50) > ED2K (30) > FTP (25)
        assert!(p2p.priority_score() > bittorrent.priority_score());
        assert!(bittorrent.priority_score() > http.priority_score());
        assert!(http.priority_score() > ed2k.priority_score());
        assert!(ed2k.priority_score() > ftp.priority_score());

        assert_eq!(p2p.priority_score(), 180);
        assert_eq!(bittorrent.priority_score(), 90);
        assert_eq!(http.priority_score(), 50);
        assert_eq!(ed2k.priority_score(), 30);
        assert_eq!(ftp.priority_score(), 25);
    }

    #[test]
    fn test_bittorrent_source_creation() {
        let source = DownloadSource::BitTorrent(BitTorrentSourceInfo {
            magnet_uri: "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel".to_string(),
        });

        assert_eq!(source.source_type(), "BitTorrent");
        assert!(source.supports_encryption());
        assert_eq!(source.priority_score(), 90);
        assert_eq!(source.display_name(), "BitTorrent: 08ada5a7");
        assert_eq!(source.identifier(), "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel");
    }

    #[test]
    fn test_ed2k_serialization() {
        let ed2k_info = Ed2kSourceInfo {
            server_url: "ed2k://|server|176.103.48.36|4661|/".to_string(),
            file_hash: "31D6CFE0D16AE931B73C59D7E0C089C0".to_string(),
            file_size: 1024000,
            file_name: Some("test.iso".to_string()),
            sources: Some(vec!["192.168.1.1:4662".to_string()]),
            timeout_secs: Some(60),
        };

        let source = DownloadSource::Ed2k(ed2k_info);
        let json = serde_json::to_string(&source).unwrap();

        // Verify JSON contains expected fields
        assert!(json.contains("\"type\":\"ed2k\""));
        assert!(json.contains("\"serverUrl\""));
        assert!(json.contains("\"fileHash\""));
        assert!(json.contains("\"fileSize\":1024000"));
        assert!(json.contains("\"fileName\":\"test.iso\""));

        // Deserialize and verify
        let deserialized: DownloadSource = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.source_type(), "ED2K");
    }
}
