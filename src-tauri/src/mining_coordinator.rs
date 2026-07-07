//! Solo mining coordinator — lets **thin miners** (no local chain) mine to their
//! own address. A thin miner runs `ethminer` (the same external Ethash engine the
//! GPU path already wraps) pointed at this coordinator instead of a local geth;
//! the coordinator hands out per-address work off a full node and relays the
//! miner's submissions back to it.
//!
//! ## The per-address problem (and the v1 workaround)
//! Stock geth bakes the node's global `--miner.etherbase` into the header that
//! `eth_getWork` returns, so an external miner cannot choose the reward address.
//! To pay each thin miner to *their own* address, the coordinator serializes a
//! **lease**: while miner X holds the lease, geth's etherbase is set to X and X's
//! work is served; a different miner is told to retry until the lease frees or
//! expires. This is correct but **single-miner-at-a-time** — the real fix is a
//! fork-level per-address `getWork` (tracked as post-v1), and a share-based pool
//! is the smooth-reward alternative. See the book's Node model.
//!
//! What is testable here (pure): the [`LeaseManager`] state machine and
//! [`extract_miner_address`]. The RPC proxying needs a live geth, so it is
//! compiled + wired but exercised only against a running node.

use std::sync::Mutex;

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};

/// A held lease on the coordinator's etherbase.
#[derive(Debug, Clone)]
pub struct Lease {
    pub address: String,
    pub expires_at: u64,
}

/// Outcome of a lease-acquire attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseDecision {
    /// This miner holds the lease; serve its work.
    Granted,
    /// Another miner holds it; tell this one to retry.
    Busy { holder: String, retry_after_s: u64 },
}

/// Serializes the shared etherbase across thin miners. Pure + sync, so the whole
/// scheduling policy is unit-testable without a chain.
pub struct LeaseManager {
    lease_ttl_secs: u64,
    current: Mutex<Option<Lease>>,
}

impl LeaseManager {
    pub fn new(lease_ttl_secs: u64) -> Self {
        LeaseManager {
            lease_ttl_secs,
            current: Mutex::new(None),
        }
    }

    /// Try to (re)acquire the lease for `address` at `now` (unix secs). Grants if
    /// the lease is free, expired, or already held by this address (renewing it);
    /// otherwise reports the current holder and how long until it expires.
    pub fn acquire(&self, address: &str, now: u64) -> LeaseDecision {
        let mut cur = self.current.lock().unwrap();
        if let Some(l) = cur.as_ref() {
            if l.expires_at > now && !l.address.eq_ignore_ascii_case(address) {
                return LeaseDecision::Busy {
                    holder: l.address.clone(),
                    retry_after_s: l.expires_at - now,
                };
            }
        }
        *cur = Some(Lease {
            address: address.to_lowercase(),
            expires_at: now + self.lease_ttl_secs,
        });
        LeaseDecision::Granted
    }

    /// The current (unexpired) holder, if any.
    pub fn holder(&self, now: u64) -> Option<String> {
        self.current
            .lock()
            .unwrap()
            .as_ref()
            .filter(|l| l.expires_at > now)
            .map(|l| l.address.clone())
    }
}

/// Extract the miner's reward address from a coordinator request. Miners point
/// `ethminer` at `http://<addr>@host:port/<addr>`; we accept the address from the
/// URL path segment first, then the HTTP Basic-Auth username (`<addr>:`), which is
/// the pool convention `ethminer` sends. Returns a lowercased `0x…` address.
pub fn extract_miner_address(path_addr: Option<&str>, auth_header: Option<&str>) -> Option<String> {
    if let Some(p) = path_addr {
        if let Some(a) = normalize_address(p) {
            return Some(a);
        }
    }
    // `Authorization: Basic base64("<user>:<pass>")` — user is the address.
    let b64 = auth_header?.strip_prefix("Basic ")?;
    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64.trim()).ok()?;
    let s = String::from_utf8(decoded).ok()?;
    let user = s.split(':').next().unwrap_or("");
    normalize_address(user)
}

/// Accept a 0x-prefixed 20-byte hex address, lowercased. `None` otherwise.
fn normalize_address(raw: &str) -> Option<String> {
    let s = raw.trim();
    let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    if hex.len() == 40 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("0x{}", hex.to_lowercase()))
    } else {
        None
    }
}

/// Shared state for the coordinator HTTP service.
#[derive(Clone)]
pub struct CoordinatorState {
    /// RPC endpoint of the full geth node that assembles + broadcasts blocks.
    pub geth_endpoint: String,
    pub leases: std::sync::Arc<LeaseManager>,
}

/// Build the coordinator router. `ethminer` speaks standard eth JSON-RPC to it:
/// `eth_getWork` is intercepted (lease → set etherbase → forward), everything
/// else (`eth_submitWork`, `eth_submitHashrate`, `eth_chainId`, …) is proxied to
/// geth unchanged.
pub fn router(state: CoordinatorState) -> Router {
    Router::new()
        .route("/", post(handle_root))
        .route("/:address", post(handle_with_address))
        .with_state(state)
}

async fn handle_root(
    State(state): State<CoordinatorState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    dispatch(&state, None, &headers, body).await
}

async fn handle_with_address(
    State(state): State<CoordinatorState>,
    Path(address): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    dispatch(&state, Some(address.as_str()), &headers, body).await
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn rpc_ok(id: Value, result: Value) -> Json<Value> {
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn rpc_err(id: Value, message: String) -> Json<Value> {
    Json(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": message } }))
}

async fn dispatch(
    state: &CoordinatorState,
    path_addr: Option<&str>,
    headers: &HeaderMap,
    body: Value,
) -> Json<Value> {
    let id = body.get("id").cloned().unwrap_or(json!(0));
    let method = body.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = body.get("params").cloned().unwrap_or(json!([]));

    if method == "eth_getWork" {
        let auth = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        let address = match extract_miner_address(path_addr, auth) {
            Some(a) => a,
            None => return rpc_err(id, "miner address required (point ethminer at http://<addr>@host:port/<addr>)".into()),
        };
        match state.leases.acquire(&address, now_unix()) {
            LeaseDecision::Busy { holder, retry_after_s } => {
                return rpc_err(id, format!("coordinator busy (leased to {holder}); retry in {retry_after_s}s"));
            }
            LeaseDecision::Granted => {
                if let Err(e) = crate::rpc_client::call(
                    &state.geth_endpoint,
                    "miner_setEtherbase",
                    json!([address]),
                )
                .await
                {
                    return rpc_err(id, format!("set etherbase: {e}"));
                }
            }
        }
    }

    // Proxy the method (eth_getWork after the lease/etherbase step, or anything
    // else — submitWork, submitHashrate, chainId, …) to the full node.
    match crate::rpc_client::call(&state.geth_endpoint, method, params).await {
        Ok(result) => rpc_ok(id, result),
        Err(e) => rpc_err(id, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0x00000000000000000000000000000000000000aa";
    const B: &str = "0x00000000000000000000000000000000000000bb";

    #[test]
    fn lease_grants_when_free_and_renews_same_address() {
        let m = LeaseManager::new(30);
        assert_eq!(m.acquire(A, 1000), LeaseDecision::Granted);
        // Same miner renews while holding.
        assert_eq!(m.acquire(A, 1005), LeaseDecision::Granted);
        assert_eq!(m.holder(1005).as_deref(), Some(A));
    }

    #[test]
    fn lease_busy_for_other_then_free_after_expiry() {
        let m = LeaseManager::new(30);
        assert_eq!(m.acquire(A, 1000), LeaseDecision::Granted);
        // Another miner is refused while A's lease is live.
        match m.acquire(B, 1010) {
            LeaseDecision::Busy { holder, retry_after_s } => {
                assert_eq!(holder, A);
                assert_eq!(retry_after_s, 20); // expires at 1030, now 1010
            }
            _ => panic!("expected Busy"),
        }
        // After A's lease expires, B can take it.
        assert_eq!(m.acquire(B, 1031), LeaseDecision::Granted);
        assert_eq!(m.holder(1031).as_deref(), Some(B));
    }

    #[test]
    fn holder_none_after_expiry() {
        let m = LeaseManager::new(30);
        m.acquire(A, 1000);
        assert_eq!(m.holder(2000), None);
    }

    #[test]
    fn extract_address_from_path_then_auth() {
        // Path segment wins.
        assert_eq!(extract_miner_address(Some(A), None).as_deref(), Some(A));
        // Case-insensitive + normalized to lowercase 0x.
        assert_eq!(
            extract_miner_address(Some("0xAABB00000000000000000000000000000000AABB"), None).as_deref(),
            Some("0xaabb00000000000000000000000000000000aabb")
        );
        // Falls back to Basic-Auth username.
        let auth = format!(
            "Basic {}",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, format!("{A}:x"))
        );
        assert_eq!(extract_miner_address(None, Some(&auth)).as_deref(), Some(A));
        // Junk is rejected.
        assert_eq!(extract_miner_address(Some("not-an-address"), None), None);
        assert_eq!(extract_miner_address(Some("0x1234"), None), None);
        assert_eq!(extract_miner_address(None, None), None);
    }
}
