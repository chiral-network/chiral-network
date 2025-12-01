//! Protocol Auto-Detection
//!
//! This module provides smart protocol detection based on identifiers, with
//! intelligent fallback and priority handling. It analyzes file identifiers
//! (URLs, magnet links, ed2k links, etc.) and determines which protocols can
//! handle them, then selects the best option based on user preferences.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

use crate::protocols::ProtocolHandler;

/// Default priority order for protocol selection (highest to lowest):
/// 1. BitTorrent (best for large files, P2P)
/// 2. ED2K (good for rare files, distributed)
/// 3. HTTP (reliable, widely supported)
/// 4. FTP (basic file transfer)
const DEFAULT_PRIORITY: &[&str] = &["bittorrent", "ed2k", "http", "ftp"];

/// User preferences for protocol detection
///
/// These preferences are used to filter and prioritize protocols
/// based on required capabilities and user choices.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DetectionPreferences {
    /// Prefer fastest protocol (not yet implemented)
    pub prefer_fastest: bool,
    /// Prefer most reliable protocol (not yet implemented)
    pub prefer_most_reliable: bool,
    /// List of preferred protocols in priority order
    pub preferred_protocols: Vec<String>,
    /// List of protocols to exclude from detection
    pub banned_protocols: Vec<String>,
    /// Require protocol to support seeding/uploading
    pub require_seeding: bool,
    /// Require protocol to support encryption
    pub require_encryption: bool,
    /// Require protocol to support pause and resume
    pub require_pause_resume: bool,
}

/// Core detector used by ProtocolManager.
///
/// Provides intelligent protocol routing based on identifier analysis
/// and user preferences.
pub struct ProtocolDetector {
    preferences: DetectionPreferences,
}

impl ProtocolDetector {
    /// Create a new protocol detector with default preferences
    pub fn new() -> Self {
        Self {
            preferences: DetectionPreferences::default(),
        }
    }

    /// Update detection preferences
    ///
    /// # Arguments
    ///
    /// * `prefs` - New preferences to use for protocol detection
    pub fn set_priority(&mut self, prefs: DetectionPreferences) {
        info!("Updating protocol detection preferences");
        self.preferences = prefs;
    }

    /// Get current preferences
    pub fn get_preferences(&self) -> &DetectionPreferences {
        &self.preferences
    }

    /// Detect all protocols that can handle the given identifier
    ///
    /// Returns a list of protocol names that:
    /// - Support the given identifier format
    /// - Are not in the banned_protocols list
    /// - Meet capability requirements (if specified)
    ///
    /// # Arguments
    ///
    /// * `identifier` - The file identifier (URL, magnet link, ed2k link, etc.)
    /// * `handlers` - Map of protocol names to their handlers
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let protocols = detector.detect_all("magnet:?xt=urn:btih:...", &handlers).await;
    /// // Returns: ["bittorrent"]
    /// ```
    pub async fn detect_all(
        &self,
        identifier: &str,
        handlers: &HashMap<String, &dyn ProtocolHandler>,
    ) -> Vec<String> {
        debug!("Detecting protocols for identifier: {}", identifier);

        let mut available = Vec::new();

        for (name, handler) in handlers {
            // Check if handler supports this identifier
            if !handler.supports(identifier) {
                continue;
            }

            // Check if protocol is banned
            if self.preferences.banned_protocols.contains(name) {
                debug!("Protocol {} is banned, skipping", name);
                continue;
            }

            // Check capability requirements
            let caps = handler.capabilities();

            if self.preferences.require_seeding && !caps.supports_seeding {
                debug!("Protocol {} doesn't support seeding, skipping", name);
                continue;
            }

            if self.preferences.require_encryption && !caps.supports_encryption {
                debug!("Protocol {} doesn't support encryption, skipping", name);
                continue;
            }

            if self.preferences.require_pause_resume && !caps.supports_pause_resume {
                debug!("Protocol {} doesn't support pause/resume, skipping", name);
                continue;
            }

            available.push(name.clone());
        }

        info!(
            "Found {} protocol(s) supporting identifier: {:?}",
            available.len(),
            available
        );

        available
    }

    /// Detect the best protocol for the given identifier
    ///
    /// Selection strategy:
    /// 1. Get all supported protocols via detect_all
    /// 2. If preferred_protocols are set, pick the first matching one
    /// 3. Otherwise, use default priority order (BitTorrent > ED2K > HTTP > FTP)
    /// 4. If no priority match, return the first available protocol
    ///
    /// # Arguments
    ///
    /// * `identifier` - The file identifier (URL, magnet link, ed2k link, etc.)
    /// * `handlers` - Map of protocol names to their handlers
    ///
    /// # Returns
    ///
    /// The name of the best matching protocol, or `None` if no protocol matches.
    pub async fn detect_best(
        &self,
        identifier: &str,
        handlers: &HashMap<String, &dyn ProtocolHandler>,
    ) -> Option<String> {
        debug!("Detecting best protocol for identifier");

        let available = self.detect_all(identifier, handlers).await;

        if available.is_empty() {
            debug!("No protocols support this identifier");
            return None;
        }

        // Check user-specified preferred protocols first
        for pref in &self.preferences.preferred_protocols {
            if available.contains(pref) {
                info!("Selected protocol: {} (user preferred)", pref);
                return Some(pref.clone());
            }
        }

        // Fall back to default priority order
        for protocol in DEFAULT_PRIORITY {
            let protocol_str = protocol.to_string();
            if available.contains(&protocol_str) {
                info!("Selected protocol: {} (default priority)", protocol);
                return Some(protocol_str);
            }
        }

        // If nothing in priority list matches, return first available
        let fallback = available.into_iter().next();
        if let Some(ref protocol) = fallback {
            info!("Selected protocol: {} (fallback)", protocol);
        }
        fallback
    }
}

impl Default for ProtocolDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocols::traits::{
        DownloadHandle, DownloadOptions, DownloadProgress, ProtocolCapabilities,
        ProtocolError, SeedOptions, SeedingInfo,
    };
    use async_trait::async_trait;
    use std::path::PathBuf;

    // Mock protocol handler for testing
    struct MockHandler {
        name: &'static str,
        prefix: &'static str,
        caps: ProtocolCapabilities,
    }

    #[async_trait]
    impl ProtocolHandler for MockHandler {
        fn name(&self) -> &'static str {
            self.name
        }

        fn supports(&self, identifier: &str) -> bool {
            identifier.starts_with(self.prefix)
        }

        async fn download(
            &self,
            _identifier: &str,
            _options: DownloadOptions,
        ) -> Result<DownloadHandle, ProtocolError> {
            unimplemented!()
        }

        async fn seed(
            &self,
            _file_path: PathBuf,
            _options: SeedOptions,
        ) -> Result<SeedingInfo, ProtocolError> {
            unimplemented!()
        }

        async fn stop_seeding(&self, _identifier: &str) -> Result<(), ProtocolError> {
            unimplemented!()
        }

        async fn pause_download(&self, _identifier: &str) -> Result<(), ProtocolError> {
            unimplemented!()
        }

        async fn resume_download(&self, _identifier: &str) -> Result<(), ProtocolError> {
            unimplemented!()
        }

        async fn cancel_download(&self, _identifier: &str) -> Result<(), ProtocolError> {
            unimplemented!()
        }

        async fn get_download_progress(
            &self,
            _identifier: &str,
        ) -> Result<DownloadProgress, ProtocolError> {
            unimplemented!()
        }

        async fn list_seeding(&self) -> Result<Vec<SeedingInfo>, ProtocolError> {
            unimplemented!()
        }

        fn capabilities(&self) -> ProtocolCapabilities {
            self.caps.clone()
        }
    }

    fn create_test_handlers() -> (MockHandler, MockHandler, MockHandler) {
        let http_handler = MockHandler {
            name: "http",
            prefix: "http",
            caps: ProtocolCapabilities {
                supports_seeding: false,
                supports_pause_resume: true,
                supports_encryption: true,
                supports_multi_source: false,
                supports_dht: false,
            },
        };

        let bittorrent_handler = MockHandler {
            name: "bittorrent",
            prefix: "magnet:",
            caps: ProtocolCapabilities {
                supports_seeding: true,
                supports_pause_resume: true,
                supports_encryption: true,
                supports_multi_source: true,
                supports_dht: true,
            },
        };

        let ed2k_handler = MockHandler {
            name: "ed2k",
            prefix: "ed2k://",
            caps: ProtocolCapabilities {
                supports_seeding: true,
                supports_pause_resume: true,
                supports_encryption: false,
                supports_multi_source: true,
                supports_dht: false,
            },
        };

        (http_handler, bittorrent_handler, ed2k_handler)
    }

    #[tokio::test]
    async fn test_detect_all_http() {
        let detector = ProtocolDetector::new();
        let (http, bt, ed2k) = create_test_handlers();

        let mut handlers: HashMap<String, &dyn ProtocolHandler> = HashMap::new();
        handlers.insert("http".to_string(), &http);
        handlers.insert("bittorrent".to_string(), &bt);
        handlers.insert("ed2k".to_string(), &ed2k);

        let protocols = detector.detect_all("https://example.com/file.zip", &handlers).await;
        assert_eq!(protocols, vec!["http"]);
    }

    #[tokio::test]
    async fn test_detect_all_magnet() {
        let detector = ProtocolDetector::new();
        let (http, bt, ed2k) = create_test_handlers();

        let mut handlers: HashMap<String, &dyn ProtocolHandler> = HashMap::new();
        handlers.insert("http".to_string(), &http);
        handlers.insert("bittorrent".to_string(), &bt);
        handlers.insert("ed2k".to_string(), &ed2k);

        let protocols = detector.detect_all("magnet:?xt=urn:btih:abc123", &handlers).await;
        assert_eq!(protocols, vec!["bittorrent"]);
    }

    #[tokio::test]
    async fn test_detect_all_ed2k() {
        let detector = ProtocolDetector::new();
        let (http, bt, ed2k) = create_test_handlers();

        let mut handlers: HashMap<String, &dyn ProtocolHandler> = HashMap::new();
        handlers.insert("http".to_string(), &http);
        handlers.insert("bittorrent".to_string(), &bt);
        handlers.insert("ed2k".to_string(), &ed2k);

        let protocols = detector.detect_all("ed2k://|file|test.iso|12345|hash|/", &handlers).await;
        assert_eq!(protocols, vec!["ed2k"]);
    }

    #[tokio::test]
    async fn test_detect_all_no_match() {
        let detector = ProtocolDetector::new();
        let (http, bt, ed2k) = create_test_handlers();

        let mut handlers: HashMap<String, &dyn ProtocolHandler> = HashMap::new();
        handlers.insert("http".to_string(), &http);
        handlers.insert("bittorrent".to_string(), &bt);
        handlers.insert("ed2k".to_string(), &ed2k);

        let protocols = detector.detect_all("unknown://invalid", &handlers).await;
        assert!(protocols.is_empty());
    }

    #[tokio::test]
    async fn test_detect_best_default_priority() {
        let detector = ProtocolDetector::new();
        let (http, bt, ed2k) = create_test_handlers();

        let mut handlers: HashMap<String, &dyn ProtocolHandler> = HashMap::new();
        handlers.insert("http".to_string(), &http);
        handlers.insert("bittorrent".to_string(), &bt);
        handlers.insert("ed2k".to_string(), &ed2k);

        let best = detector.detect_best("magnet:?xt=urn:btih:abc", &handlers).await;
        assert_eq!(best, Some("bittorrent".to_string()));
    }

    #[tokio::test]
    async fn test_detect_with_banned_protocol() {
        let mut detector = ProtocolDetector::new();
        detector.set_priority(DetectionPreferences {
            banned_protocols: vec!["http".to_string()],
            ..Default::default()
        });

        let (http, bt, ed2k) = create_test_handlers();

        let mut handlers: HashMap<String, &dyn ProtocolHandler> = HashMap::new();
        handlers.insert("http".to_string(), &http);
        handlers.insert("bittorrent".to_string(), &bt);
        handlers.insert("ed2k".to_string(), &ed2k);

        // HTTP should be banned
        let protocols = detector.detect_all("https://example.com/file.zip", &handlers).await;
        assert!(protocols.is_empty());
    }

    #[tokio::test]
    async fn test_detect_require_seeding() {
        let mut detector = ProtocolDetector::new();
        detector.set_priority(DetectionPreferences {
            require_seeding: true,
            ..Default::default()
        });

        let (http, bt, ed2k) = create_test_handlers();

        let mut handlers: HashMap<String, &dyn ProtocolHandler> = HashMap::new();
        handlers.insert("http".to_string(), &http);
        handlers.insert("bittorrent".to_string(), &bt);
        handlers.insert("ed2k".to_string(), &ed2k);

        // HTTP doesn't support seeding, should be filtered out
        let protocols = detector.detect_all("https://example.com/file.zip", &handlers).await;
        assert!(protocols.is_empty());

        // BitTorrent supports seeding
        let protocols = detector.detect_all("magnet:?xt=urn:btih:abc", &handlers).await;
        assert_eq!(protocols, vec!["bittorrent"]);
    }

    #[tokio::test]
    async fn test_detect_require_encryption() {
        let mut detector = ProtocolDetector::new();
        detector.set_priority(DetectionPreferences {
            require_encryption: true,
            ..Default::default()
        });

        let (http, bt, ed2k) = create_test_handlers();

        let mut handlers: HashMap<String, &dyn ProtocolHandler> = HashMap::new();
        handlers.insert("http".to_string(), &http);
        handlers.insert("bittorrent".to_string(), &bt);
        handlers.insert("ed2k".to_string(), &ed2k);

        // ED2K doesn't support encryption (in our mock)
        let protocols = detector.detect_all("ed2k://|file|test|123|hash|/", &handlers).await;
        assert!(protocols.is_empty());

        // BitTorrent supports encryption
        let protocols = detector.detect_all("magnet:?xt=urn:btih:abc", &handlers).await;
        assert_eq!(protocols, vec!["bittorrent"]);
    }

    #[tokio::test]
    async fn test_detect_best_with_preferred() {
        let mut detector = ProtocolDetector::new();
        detector.set_priority(DetectionPreferences {
            preferred_protocols: vec!["http".to_string()],
            ..Default::default()
        });

        let (http, bt, ed2k) = create_test_handlers();

        let mut handlers: HashMap<String, &dyn ProtocolHandler> = HashMap::new();
        handlers.insert("http".to_string(), &http);
        handlers.insert("bittorrent".to_string(), &bt);
        handlers.insert("ed2k".to_string(), &ed2k);

        // User prefers HTTP
        let best = detector.detect_best("https://example.com/file.zip", &handlers).await;
        assert_eq!(best, Some("http".to_string()));
    }

    #[tokio::test]
    async fn test_get_preferences() {
        let mut detector = ProtocolDetector::new();
        let prefs = DetectionPreferences {
            require_seeding: true,
            require_encryption: true,
            ..Default::default()
        };
        detector.set_priority(prefs.clone());

        let retrieved = detector.get_preferences();
        assert!(retrieved.require_seeding);
        assert!(retrieved.require_encryption);
    }
}
