use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::protocols::ProtocolHandler;

// Preferences for auto-detection.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DetectionPreferences {
    pub prefer_fastest: bool,
    pub prefer_most_reliable: bool,
    pub preferred_protocols: Vec<String>,
    pub banned_protocols: Vec<String>,
}

// Core detector used by ProtocolManager.
pub struct ProtocolDetector {
    preferences: DetectionPreferences,
}

impl ProtocolDetector {
    pub fn new() -> Self {
        Self {
            preferences: DetectionPreferences::default(),
        }
    }

    // Update preferences (Task 6.1: set_priority)
    pub fn set_priority(&mut self, prefs: DetectionPreferences) {
        self.preferences = prefs;
    }

    // Task 6.1: detect_all
    // For now, "detection" means:
    // - Include any handler whose `supports(identifier)` is true
    // - Exclude protocols in `banned_protocols`
    pub async fn detect_all(
        &self,
        identifier: &str,
        handlers: &HashMap<String, &dyn ProtocolHandler>,
    ) -> Vec<String> {
        let mut available = Vec::new();

        for (name, handler) in handlers {
            if handler.supports(identifier)
                && !self.preferences.banned_protocols.contains(name)
            {
                available.push(name.clone());
            }
        }

        available
    }

    // Task 6.1: detect_best
    // Strategy:
    // 1. Get all supported protocols via detect_all
    // 2. If any preferred_protocols are available, pick the first one
    // 3. Otherwise, pick the first available protocol
    pub async fn detect_best(
        &self,
        identifier: &str,
        handlers: &HashMap<String, &dyn ProtocolHandler>,
    ) -> Option<String> {
        let available = self.detect_all(identifier, handlers).await;

        if available.is_empty() {
            return None;
        }

        // Preferred protocols override
        for pref in &self.preferences.preferred_protocols {
            if available.contains(pref) {
                return Some(pref.clone());
            }
        }

        // Fallback: first available
        Some(available[0].clone())
    }
}
