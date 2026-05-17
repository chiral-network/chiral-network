use axum::{
    body::Bytes,
    http::{header::CONTENT_TYPE, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};

/// Read-only method namespaces that the canonical chain proxy will
/// forward. Anything outside this list is rejected with 403 — most
/// importantly that means `miner_*` (e.g. `miner_setEtherbase`,
/// `miner_start`), `personal_*` (keystore management), `debug_*` and
/// `admin_*` cannot be invoked through the proxy.
///
/// Why this matters: the canonical relay binds geth's HTTP RPC to
/// localhost only, so the proxy is the *only* externally reachable
/// path to the chain. Without a method allowlist here, anyone on the
/// internet could call `miner_setEtherbase("0x<attacker>")` and
/// redirect mining rewards. That happened (2026-05 incident — ~7 168
/// CHI of freshnet block rewards diverted before this filter
/// existed).
///
/// A namespace prefix (e.g. `eth_`) is allowlisted as a whole because
/// the eth/net/web3/txpool surface is large but uniformly read-only
/// (or write-only-to-the-chain-via-signed-tx, which is fine — the
/// relay can't forge signatures). Per-method enumeration would be
/// more conservative but harder to keep in sync with new geth
/// releases.
const ALLOWED_METHOD_PREFIXES: &[&str] = &["eth_", "net_", "web3_", "txpool_"];

/// A method passes the allowlist iff it starts with one of the four
/// read-only namespace prefixes.
///
/// The example-based tests in this file enumerate known dangerous and
/// known safe method names; the proof at
/// `verified/method_allowlist.rs` (checked by Verus, not cargo)
/// upgrades them to the universal statement that EVERY string
/// starting with `miner_`, `personal_`, `debug_`, or `admin_` is
/// rejected — including method names geth hasn't shipped yet.
fn is_allowed_method(method: &str) -> bool {
    ALLOWED_METHOD_PREFIXES
        .iter()
        .any(|p| method.starts_with(p))
}

/// Inspect the JSON-RPC request body and return Err with the offending
/// method name on the first method outside the allowlist. Handles both
/// single requests and batch arrays. Empty / unparseable bodies are
/// permitted to fall through (geth itself will return a parse error).
fn check_methods(body: &[u8]) -> Result<(), String> {
    let value: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return Ok(()), // let geth surface the parse error to the client
    };
    let check_one = |req: &serde_json::Value| -> Result<(), String> {
        let m = req
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if m.is_empty() || is_allowed_method(m) {
            Ok(())
        } else {
            Err(m.to_string())
        }
    };
    if let Some(arr) = value.as_array() {
        for req in arr {
            check_one(req)?;
        }
        Ok(())
    } else {
        check_one(&value)
    }
}

/// Same-origin JSON-RPC proxy with a read-only method allowlist.
/// Browser share pages, the desktop wallet's RPC fallback, and any
/// other external client all hit this. Geth itself is bound to
/// 127.0.0.1 on the canonical relay, so this is the only externally
/// reachable path to the chain — the allowlist is the only thing
/// keeping `miner_setEtherbase` from being a public RPC.
async fn proxy_chain_rpc(body: Bytes) -> Response {
    if let Err(method) = check_methods(&body) {
        return (
            StatusCode::FORBIDDEN,
            format!(
                "JSON-RPC method `{}` not exposed via this proxy. Allowed namespaces: {}",
                method,
                ALLOWED_METHOD_PREFIXES.join(", ")
            ),
        )
            .into_response();
    }

    let rpc = crate::geth::rpc_endpoint();
    let client = reqwest::Client::new();

    let upstream = match client
        .post(&rpc)
        .header("content-type", "application/json")
        .body(body)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Chain RPC unavailable: {}", e),
            )
                .into_response();
        }
    };

    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let bytes = match upstream.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Failed to read RPC response: {}", e),
            )
                .into_response();
        }
    };

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    (status, headers, bytes).into_response()
}

/// Routes for chain JSON-RPC proxying.
pub fn chain_rpc_routes() -> Router {
    Router::new().route("/api/chain/rpc", post(proxy_chain_rpc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_read_only_namespaces() {
        for m in [
            "eth_blockNumber",
            "eth_getBalance",
            "eth_sendRawTransaction",
            "net_version",
            "web3_clientVersion",
            "txpool_status",
        ] {
            assert!(is_allowed_method(m), "expected {m} allowed");
        }
    }

    #[test]
    fn blocks_dangerous_namespaces() {
        for m in [
            "miner_setEtherbase",
            "miner_start",
            "miner_stop",
            "personal_unlockAccount",
            "personal_sendTransaction",
            "debug_traceTransaction",
            "admin_addPeer",
            "admin_stopRPC",
        ] {
            assert!(!is_allowed_method(m), "expected {m} blocked");
        }
    }

    #[test]
    fn check_methods_rejects_dangerous_single() {
        let body = br#"{"jsonrpc":"2.0","method":"miner_setEtherbase","params":["0x00"],"id":1}"#;
        assert_eq!(check_methods(body).unwrap_err(), "miner_setEtherbase");
    }

    #[test]
    fn check_methods_accepts_safe_single() {
        let body = br#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#;
        assert!(check_methods(body).is_ok());
    }

    #[test]
    fn check_methods_rejects_dangerous_in_batch() {
        // A batch with one dangerous method must be rejected as a whole.
        // Otherwise an attacker hides miner_setEtherbase in a batch with
        // benign reads to slip past per-request inspection.
        let body = br#"[
            {"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1},
            {"jsonrpc":"2.0","method":"miner_setEtherbase","params":["0x00"],"id":2}
        ]"#;
        assert_eq!(check_methods(body).unwrap_err(), "miner_setEtherbase");
    }

    #[test]
    fn check_methods_accepts_safe_batch() {
        let body = br#"[
            {"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1},
            {"jsonrpc":"2.0","method":"net_peerCount","params":[],"id":2}
        ]"#;
        assert!(check_methods(body).is_ok());
    }

    #[test]
    fn check_methods_passes_unparseable_body_through() {
        // geth itself returns a clearer parse-error than we could; don't
        // pre-empt it with our own 403.
        assert!(check_methods(b"{not json").is_ok());
    }
}
