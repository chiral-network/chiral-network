//! Shared JSON-RPC client for Geth communication.
//!
//! Provides a connection-pooled reqwest client, batch request support,
//! and a response cache with TTL to reduce redundant RPC calls.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

// ============================================================================
// Shared HTTP client (connection-pooled, reused across all RPC calls)
// ============================================================================

static SHARED_CLIENT: Lazy<Result<reqwest::Client, String>> = Lazy::new(build_shared_client);

fn shared_client_default_headers() -> reqwest::header::HeaderMap {
    // Default headers — every outgoing HTTP call from Rust carries the
    // client's compile-time version. Phase 3 of version enforcement: the
    // server gateway middleware compares this against its bundled
    // `min_required` and 426s out-of-date clients.
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(v) = reqwest::header::HeaderValue::from_str(crate::version::CURRENT_VERSION) {
        headers.insert("X-Chiral-Client-Version", v);
    }
    headers
}

fn shared_client_startup_error(error: impl std::fmt::Display) -> String {
    format!(
        "Failed to create shared HTTP client with Chiral default headers: {}",
        error
    )
}

fn build_shared_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .default_headers(shared_client_default_headers())
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(shared_client_startup_error)
}

/// Auto-incrementing request ID for JSON-RPC calls.
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    REQUEST_ID.fetch_add(1, Ordering::Relaxed)
}

/// Returns a reference to the shared connection-pooled reqwest client.
pub fn client() -> Result<&'static reqwest::Client, String> {
    SHARED_CLIENT.as_ref().map_err(|err| err.clone())
}

// ============================================================================
// JSON-RPC types
// ============================================================================

#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct RpcRequest {
    jsonrpc: &'static str,
    method: String,
    params: serde_json::Value,
    id: u64,
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    id: u64,
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
}

// ============================================================================
// Single RPC call
// ============================================================================

/// Make a single JSON-RPC call to the given endpoint.
pub async fn call(
    endpoint: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let id = next_id();
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": id
    });

    let response = client()?
        .post(endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("RPC request to {} failed: {}", endpoint, e))?;

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse RPC response: {}", e))?;

    if let Some(error) = json.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    Ok(json.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

/// Read-only RPC call with an ordered fallback list. Tries each
/// endpoint in turn and returns the first success; aggregates errors
/// for the failure case. Use this for `eth_getBalance`,
/// `eth_getTransactionByHash`, etc. when the primary RPC may be
/// firewall-blocked or temporarily unreachable but a same-origin
/// proxy on a different port is also an option.
///
/// Do NOT use this for write paths (`eth_sendRawTransaction`) — those
/// should hit a single endpoint to avoid double-broadcast races.
pub async fn call_with_fallbacks(
    endpoints: &[String],
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if endpoints.is_empty() {
        return Err("call_with_fallbacks: no endpoints".to_string());
    }
    let mut errors: Vec<String> = Vec::with_capacity(endpoints.len());
    for ep in endpoints {
        match call(ep, method, params.clone()).await {
            Ok(v) => return Ok(v),
            Err(e) => errors.push(format!("{}: {}", ep, e)),
        }
    }
    Err(format!(
        "all RPC endpoints failed:\n  {}",
        errors.join("\n  ")
    ))
}

// ============================================================================
// Batch RPC call
// ============================================================================

/// A single request within a batch.
pub struct BatchEntry {
    pub method: String,
    pub params: serde_json::Value,
    id: u64,
}

/// Build a batch of RPC requests.
pub fn batch() -> BatchBuilder {
    BatchBuilder { entries: Vec::new() }
}

pub struct BatchBuilder {
    entries: Vec<BatchEntry>,
}

impl BatchBuilder {
    /// Add a method call to the batch.  Returns the index (0-based) for
    /// retrieving the result after execution.
    pub fn add(&mut self, method: &str, params: serde_json::Value) -> usize {
        let idx = self.entries.len();
        self.entries.push(BatchEntry {
            method: method.to_string(),
            params,
            id: next_id(),
        });
        idx
    }

    /// Execute all queued calls as a single HTTP request.
    /// Returns results indexed by the order they were added.
    pub async fn execute(self, endpoint: &str) -> Result<Vec<Result<serde_json::Value, String>>, String> {
        if self.entries.is_empty() {
            return Ok(Vec::new());
        }

        // Single call optimisation — avoid batch array overhead.
        if self.entries.len() == 1 {
            let e = &self.entries[0];
            let result = call(endpoint, &e.method, e.params.clone()).await;
            return Ok(vec![result]);
        }

        let payloads: Vec<serde_json::Value> = self
            .entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": e.method,
                    "params": e.params,
                    "id": e.id
                })
            })
            .collect();

        let response = client()?
            .post(endpoint)
            .json(&payloads)
            .send()
            .await
            .map_err(|e| format!("Batch RPC request failed: {}", e))?;

        let responses: Vec<RpcResponse> = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse batch RPC response: {}", e))?;

        // Map responses back by ID.
        let mut by_id: HashMap<u64, RpcResponse> = HashMap::new();
        for r in responses {
            by_id.insert(r.id, r);
        }

        let results: Vec<Result<serde_json::Value, String>> = self
            .entries
            .iter()
            .map(|e| {
                match by_id.remove(&e.id) {
                    Some(r) => {
                        if let Some(err) = r.error {
                            Err(format!("RPC error ({}): {}", e.method, err))
                        } else {
                            Ok(r.result.unwrap_or(serde_json::Value::Null))
                        }
                    }
                    None => Err(format!("No response for {} (id={})", e.method, e.id)),
                }
            })
            .collect();

        Ok(results)
    }
}

// ============================================================================
// Response cache with TTL
// ============================================================================

struct CacheEntry {
    value: serde_json::Value,
    expires: Instant,
}

/// A simple TTL cache for RPC responses.
pub struct RpcCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    ttl: Duration,
}

impl RpcCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Get a cached value if it exists and hasn't expired.
    pub async fn get(&self, key: &str) -> Option<serde_json::Value> {
        let entries = self.entries.read().await;
        if let Some(entry) = entries.get(key) {
            if Instant::now() < entry.expires {
                return Some(entry.value.clone());
            }
        }
        None
    }

    /// Store a value in the cache.
    pub async fn set(&self, key: String, value: serde_json::Value) {
        let mut entries = self.entries.write().await;
        entries.insert(
            key,
            CacheEntry {
                value,
                expires: Instant::now() + self.ttl,
            },
        );
    }

    /// Invalidate a specific key.
    pub async fn invalidate(&self, key: &str) {
        let mut entries = self.entries.write().await;
        entries.remove(key);
    }

    /// Invalidate all entries.
    pub async fn clear(&self) {
        let mut entries = self.entries.write().await;
        entries.clear();
    }
}

// ============================================================================
// Hex parsing helpers
// ============================================================================

/// Parse a hex string (with or without 0x prefix) to u64.
pub fn hex_to_u64(hex: &str) -> u64 {
    u64::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0)
}

/// Parse a hex string (with or without 0x prefix) to u128.
pub fn hex_to_u128(hex: &str) -> u128 {
    u128::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0)
}

/// Convert wei (u128) to CHI (f64) string with 6 decimal places.
pub fn wei_to_chi_string(wei: u128) -> String {
    let chi = wei as f64 / 1e18;
    format!("{:.6}", chi)
}

/// Convert wei (u128) to CHI (f64).
pub fn wei_to_chi(wei: u128) -> f64 {
    wei as f64 / 1e18
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_client_default_headers_include_client_version() {
        let headers = shared_client_default_headers();
        let version = headers
            .get("X-Chiral-Client-Version")
            .expect("client version header should be present");

        assert_eq!(
            version.to_str().expect("version header should be UTF-8"),
            crate::version::CURRENT_VERSION
        );
    }

    #[test]
    fn shared_client_startup_error_is_actionable() {
        let error = shared_client_startup_error("builder rejected configuration");

        assert!(error.contains("shared HTTP client"));
        assert!(error.contains("Chiral default headers"));
        assert!(error.contains("builder rejected configuration"));
    }
}
