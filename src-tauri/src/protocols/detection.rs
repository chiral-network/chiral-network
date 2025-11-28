//! Protocol Auto-Detection
//!
//! This module provides smart protocol detection based on identifiers, with
//! intelligent fallback and priority handling. It analyzes file identifiers
//! (URLs, magnet links, ed2k links, etc.) and determines which protocols can
//! handle them, then selects the best option based on user preferences.

use super::traits::*;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

/// Protocol detector that intelligently routes downloads to appropriate handlers
pub struct ProtocolDetector {
    /// Map of protocol name to handler
    handlers: HashMap<String, Arc<dyn ProtocolHandler>>,
    /// Priority order for protocol selection (higher index = higher priority)
    priority: Vec<String>,
}

impl ProtocolDetector {
    /// Create a new protocol detector with default priority order
    ///
    /// Default priority (highest to lowest):
    /// 1. BitTorrent (best for large files, P2P)
    /// 2. ED2K (good for rare files, distributed)
    /// 3. HTTP (reliable, widely supported)
    /// 4. FTP (basic file transfer)
    pub fn new(handlers: HashMap<String, Arc<dyn ProtocolHandler>>) -> Self {
        info!("Initializing ProtocolDetector with {} handlers", handlers.len());
        Self {
            handlers,
            priority: vec![
                "bittorrent".to_string(),
                "ed2k".to_string(),
                "http".to_string(),
                "ftp".to_string(),
            ],
        }
    }

    /// Detect all protocols that support this identifier
    ///
    /// Returns a list of protocol names that can handle the given identifier.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let protocols = detector.detect_all("magnet:?xt=urn:btih:...");
    /// // Returns: ["bittorrent"]
    ///
    /// let protocols = detector.detect_all("https://example.com/file.zip");
    /// // Returns: ["http"]
    /// ```
    pub fn detect_all(&self, identifier: &str) -> Vec<String> {
        debug!("Detecting protocols for identifier: {}", identifier);

        let supported: Vec<String> = self.handlers
            .iter()
            .filter(|(_, handler)| handler.supports(identifier))
            .map(|(name, _)| name.clone())
            .collect();

        info!(
            "Found {} protocol(s) supporting identifier: {:?}",
            supported.len(),
            supported
        );

        supported
    }

    /// Get best protocol based on priority and capabilities
    ///
    /// This method filters protocols by user preferences (encryption, seeding, etc.)
    /// and then selects the highest priority protocol that matches.
    ///
    /// # Arguments
    ///
    /// * `identifier` - The file identifier (URL, magnet link, etc.)
    /// * `preferences` - User preferences for filtering protocols
    ///
    /// # Returns
    ///
    /// The name of the best matching protocol, or `None` if no protocol matches.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let prefs = DetectionPreferences {
    ///     require_encryption: true,
    ///     ..Default::default()
    /// };
    /// let best = detector.detect_best("ed2k://|file|...", prefs);
    /// ```
    pub fn detect_best(
        &self,
        identifier: &str,
        preferences: DetectionPreferences,
    ) -> Option<String> {
        debug!("Detecting best protocol for identifier with preferences: {:?}", preferences);

        let supported = self.detect_all(identifier);

        if supported.is_empty() {
            debug!("No protocols support this identifier");
            return None;
        }

        // Filter by preferences
        let mut candidates: Vec<&String> = supported.iter().collect();

        if preferences.require_seeding {
            debug!("Filtering for seeding support");
            candidates.retain(|protocol| {
                if let Some(handler) = self.handlers.get(*protocol) {
                    handler.capabilities().supports_seeding
                } else {
                    false
                }
            });
        }

        if preferences.require_encryption {
            debug!("Filtering for encryption support");
            candidates.retain(|protocol| {
                if let Some(handler) = self.handlers.get(*protocol) {
                    handler.capabilities().supports_encryption
                } else {
                    false
                }
            });
        }

        if preferences.require_pause_resume {
            debug!("Filtering for pause/resume support");
            candidates.retain(|protocol| {
                if let Some(handler) = self.handlers.get(*protocol) {
                    handler.capabilities().supports_pause_resume
                } else {
                    false
                }
            });
        }

        if candidates.is_empty() {
            debug!("No protocols match the specified preferences");
            return None;
        }

        // Return highest priority candidate
        for protocol in &self.priority {
            if candidates.contains(&protocol) {
                info!("Selected best protocol: {} (by priority)", protocol);
                return Some(protocol.clone());
            }
        }

        // If no priority match, return first candidate
        let fallback = candidates.first().map(|s| s.to_string());
        if let Some(ref protocol) = fallback {
            info!("Selected best protocol: {} (fallback)", protocol);
        }
        fallback
    }

    /// Set custom priority order
    ///
    /// Override the default priority order with a custom list.
    /// Protocols earlier in the list have higher priority.
    ///
    /// # Arguments
    ///
    /// * `priority` - List of protocol names in descending priority order
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Prefer HTTP over BitTorrent
    /// detector.set_priority(vec![
    ///     "http".to_string(),
    ///     "bittorrent".to_string(),
    ///     "ed2k".to_string(),
    /// ]);
    /// ```
    pub fn set_priority(&mut self, priority: Vec<String>) {
        info!("Updating protocol priority to: {:?}", priority);
        self.priority = priority;
    }

    /// Get current priority order
    pub fn get_priority(&self) -> &[String] {
        &self.priority
    }

    /// Get all registered protocol names
    pub fn list_protocols(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }
}

/// User preferences for protocol detection
///
/// These preferences are used to filter protocols based on required capabilities.
#[derive(Debug, Clone, Default)]
pub struct DetectionPreferences {
    /// Require protocol to support seeding/uploading
    pub require_seeding: bool,
    /// Require protocol to support encryption
    pub require_encryption: bool,
    /// Require protocol to support pause and resume
    pub require_pause_resume: bool,
    /// Prefer P2P protocols over centralized (not yet implemented)
    pub prefer_p2p: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn create_test_detector() -> ProtocolDetector {
        let mut handlers: HashMap<String, Arc<dyn ProtocolHandler>> = HashMap::new();

        handlers.insert(
            "http".to_string(),
            Arc::new(MockHandler {
                name: "http",
                prefix: "http",
                caps: ProtocolCapabilities {
                    supports_seeding: false,
                    supports_pause_resume: true,
                    supports_encryption: true,
                    supports_multi_source: false,
                    supports_dht: false,
                },
            }),
        );

        handlers.insert(
            "bittorrent".to_string(),
            Arc::new(MockHandler {
                name: "bittorrent",
                prefix: "magnet:",
                caps: ProtocolCapabilities {
                    supports_seeding: true,
                    supports_pause_resume: true,
                    supports_encryption: true,
                    supports_multi_source: true,
                    supports_dht: true,
                },
            }),
        );

        handlers.insert(
            "ed2k".to_string(),
            Arc::new(MockHandler {
                name: "ed2k",
                prefix: "ed2k://",
                caps: ProtocolCapabilities {
                    supports_seeding: true,
                    supports_pause_resume: true,
                    supports_encryption: false,
                    supports_multi_source: true,
                    supports_dht: false,
                },
            }),
        );

        ProtocolDetector::new(handlers)
    }

    #[test]
    fn test_detect_all_http() {
        let detector = create_test_detector();
        let protocols = detector.detect_all("https://example.com/file.zip");
        assert_eq!(protocols, vec!["http"]);
    }

    #[test]
    fn test_detect_all_magnet() {
        let detector = create_test_detector();
        let protocols = detector.detect_all("magnet:?xt=urn:btih:abc123");
        assert_eq!(protocols, vec!["bittorrent"]);
    }

    #[test]
    fn test_detect_all_ed2k() {
        let detector = create_test_detector();
        let protocols = detector.detect_all("ed2k://|file|test.iso|12345|hash|/");
        assert_eq!(protocols, vec!["ed2k"]);
    }

    #[test]
    fn test_detect_all_no_match() {
        let detector = create_test_detector();
        let protocols = detector.detect_all("unknown://invalid");
        assert!(protocols.is_empty());
    }

    #[test]
    fn test_detect_best_default() {
        let detector = create_test_detector();
        let best = detector.detect_best("magnet:?xt=urn:btih:abc", DetectionPreferences::default());
        assert_eq!(best, Some("bittorrent".to_string()));
    }

    #[test]
    fn test_detect_best_require_seeding() {
        let detector = create_test_detector();
        let prefs = DetectionPreferences {
            require_seeding: true,
            ..Default::default()
        };

        // HTTP doesn't support seeding, should fail
        let best = detector.detect_best("https://example.com/file", prefs.clone());
        assert_eq!(best, None);

        // BitTorrent supports seeding
        let best = detector.detect_best("magnet:?xt=urn:btih:abc", prefs);
        assert_eq!(best, Some("bittorrent".to_string()));
    }

    #[test]
    fn test_detect_best_require_encryption() {
        let detector = create_test_detector();
        let prefs = DetectionPreferences {
            require_encryption: true,
            ..Default::default()
        };

        // ED2K doesn't support encryption (in our mock)
        let best = detector.detect_best("ed2k://|file|test|123|hash|/", prefs.clone());
        assert_eq!(best, None);

        // BitTorrent supports encryption
        let best = detector.detect_best("magnet:?xt=urn:btih:abc", prefs);
        assert_eq!(best, Some("bittorrent".to_string()));
    }

    #[test]
    fn test_set_priority() {
        let mut detector = create_test_detector();

        // Default priority should prefer BitTorrent
        let best = detector.detect_best("ed2k://|file|test|123|hash|/", DetectionPreferences::default());
        assert_eq!(best, Some("ed2k".to_string()));

        // Change priority to prefer HTTP
        detector.set_priority(vec!["http".to_string(), "ed2k".to_string(), "bittorrent".to_string()]);

        // Now if multiple protocols match, HTTP should win
        // (but ed2k link only matches ed2k, so result stays same)
        let best = detector.detect_best("ed2k://|file|test|123|hash|/", DetectionPreferences::default());
        assert_eq!(best, Some("ed2k".to_string()));
    }

    #[test]
    fn test_multiple_preferences() {
        let detector = create_test_detector();
        let prefs = DetectionPreferences {
            require_seeding: true,
            require_encryption: true,
            require_pause_resume: true,
            prefer_p2p: false,
        };

        // Only BitTorrent matches all requirements
        let best = detector.detect_best("magnet:?xt=urn:btih:abc", prefs);
        assert_eq!(best, Some("bittorrent".to_string()));
    }

    #[test]
    fn test_list_protocols() {
        let detector = create_test_detector();
        let mut protocols = detector.list_protocols();
        protocols.sort();

        assert_eq!(protocols.len(), 3);
        assert!(protocols.contains(&"http".to_string()));
        assert!(protocols.contains(&"bittorrent".to_string()));
        assert!(protocols.contains(&"ed2k".to_string()));
    }

    #[test]
    fn test_get_priority() {
        let detector = create_test_detector();
        let priority = detector.get_priority();

        assert_eq!(priority[0], "bittorrent");
        assert_eq!(priority[1], "ed2k");
        assert_eq!(priority[2], "http");
        assert_eq!(priority[3], "ftp");
    }
}
