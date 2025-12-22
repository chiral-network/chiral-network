// Webhook notification system for REPL events
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use colored::Colorize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Webhook {
    pub id: String,
    pub event: String,
    pub url: String,
    pub enabled: bool,
    pub created_at: u64,
    pub last_triggered: Option<u64>,
    pub trigger_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookPayload {
    pub event: String,
    pub timestamp: String,
    pub peer_id: String,
    pub data: serde_json::Value,
}

pub struct WebhookManager {
    webhooks: Arc<RwLock<HashMap<String, Webhook>>>,
    config_path: std::path::PathBuf,
}

impl WebhookManager {
    pub fn new(config_path: std::path::PathBuf) -> Self {
        let webhooks = Self::load_webhooks(&config_path).unwrap_or_default();

        Self {
            webhooks: Arc::new(RwLock::new(webhooks)),
            config_path,
        }
    }

    fn load_webhooks(path: &std::path::Path) -> Result<HashMap<String, Webhook>, Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(HashMap::new());
        }

        let data = std::fs::read_to_string(path)?;
        let webhooks: HashMap<String, Webhook> = serde_json::from_str(&data)?;
        Ok(webhooks)
    }

    fn save_webhooks(&self) -> Result<(), Box<dyn std::error::Error>> {
        let webhooks = self.webhooks.blocking_read();
        let data = serde_json::to_string_pretty(&*webhooks)?;

        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&self.config_path, data)?;
        Ok(())
    }

    pub async fn add_webhook(&self, event: String, url: String) -> Result<String, String> {
        let id = format!("webhook_{}", chrono::Utc::now().timestamp_millis());

        let webhook = Webhook {
            id: id.clone(),
            event,
            url,
            enabled: true,
            created_at: chrono::Utc::now().timestamp() as u64,
            last_triggered: None,
            trigger_count: 0,
        };

        let mut webhooks = self.webhooks.write().await;
        webhooks.insert(id.clone(), webhook);
        drop(webhooks);

        self.save_webhooks()
            .map_err(|e| format!("Failed to save webhooks: {}", e))?;

        Ok(id)
    }

    pub async fn remove_webhook(&self, id: &str) -> Result<(), String> {
        let mut webhooks = self.webhooks.write().await;

        if webhooks.remove(id).is_none() {
            return Err(format!("Webhook not found: {}", id));
        }

        drop(webhooks);

        self.save_webhooks()
            .map_err(|e| format!("Failed to save webhooks: {}", e))?;

        Ok(())
    }

    pub async fn list_webhooks(&self) -> Vec<Webhook> {
        let webhooks = self.webhooks.read().await;
        webhooks.values().cloned().collect()
    }

    pub async fn trigger_webhook(&self, event: &str, peer_id: &str, data: serde_json::Value) {
        let webhooks = self.webhooks.read().await;

        let matching_webhooks: Vec<Webhook> = webhooks
            .values()
            .filter(|w| w.enabled && w.event == event)
            .cloned()
            .collect();

        drop(webhooks);

        for webhook in matching_webhooks {
            let payload = WebhookPayload {
                event: event.to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                peer_id: peer_id.to_string(),
                data: data.clone(),
            };

            // Spawn background task to send webhook
            let webhook_clone = webhook.clone();
            let webhook_manager = self.clone();

            tokio::spawn(async move {
                match send_webhook(&webhook_clone, &payload).await {
                    Ok(_) => {
                        // Update trigger count
                        webhook_manager.update_webhook_stats(&webhook_clone.id).await;
                    }
                    Err(e) => {
                        eprintln!("Failed to send webhook {}: {}", webhook_clone.id, e);
                    }
                }
            });
        }
    }

    async fn update_webhook_stats(&self, id: &str) {
        let mut webhooks = self.webhooks.write().await;

        if let Some(webhook) = webhooks.get_mut(id) {
            webhook.trigger_count += 1;
            webhook.last_triggered = Some(chrono::Utc::now().timestamp() as u64);
        }

        drop(webhooks);

        let _ = self.save_webhooks();
    }

    pub async fn test_webhook(&self, id: &str, peer_id: &str) -> Result<(), String> {
        let webhooks = self.webhooks.read().await;

        let webhook = webhooks.get(id)
            .ok_or_else(|| format!("Webhook not found: {}", id))?
            .clone();

        drop(webhooks);

        let payload = WebhookPayload {
            event: format!("{}_test", webhook.event),
            timestamp: chrono::Utc::now().to_rfc3339(),
            peer_id: peer_id.to_string(),
            data: serde_json::json!({
                "test": true,
                "message": "This is a test webhook payload"
            }),
        };

        send_webhook(&webhook, &payload).await
            .map_err(|e| format!("Failed to send test webhook: {}", e))
    }

    fn clone(&self) -> Self {
        Self {
            webhooks: self.webhooks.clone(),
            config_path: self.config_path.clone(),
        }
    }
}

async fn send_webhook(webhook: &Webhook, payload: &WebhookPayload) -> Result<(), String> {
    let client = reqwest::Client::new();

    let response = client
        .post(&webhook.url)
        .json(payload)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Failed to send webhook: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Webhook returned status: {}", response.status()));
    }

    Ok(())
}

// Supported webhook events
pub const WEBHOOK_EVENTS: &[&str] = &[
    "peer_connected",
    "peer_disconnected",
    "download_started",
    "download_completed",
    "download_failed",
    "file_added",
    "mining_started",
    "mining_stopped",
    "block_found",
];

pub fn is_valid_event(event: &str) -> bool {
    WEBHOOK_EVENTS.contains(&event)
}

// Print available webhook events
pub fn print_webhook_events() {
    println!("\n  Available webhook events:");
    for event in WEBHOOK_EVENTS {
        println!("    - {}", event.green());
    }
}
