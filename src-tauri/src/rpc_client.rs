//! Pooled, bounded JSON-RPC transport. No node lifecycle or implicit endpoints.
use futures_util::StreamExt;
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::{
    io::Read,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use zeroize::Zeroizing;
static CLIENT: Lazy<Result<reqwest::Client, String>> = Lazy::new(|| {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .pool_max_idle_per_host(4)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "cannot initialize HTTP transport".into())
});
static ID: AtomicU64 = AtomicU64::new(1);
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;
// Hash-only eSpace blocks can aggregate many thousands of transactions.
// Keep ordinary methods/errors small; only this bounded block shape gets more room.
pub const MAX_BLOCK_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub fn response_limit(method: &str, params: &Value) -> usize {
    if method == "eth_getBlockByNumber"
        && params.as_array().is_some_and(|p| p.len() == 2 && p[1] == false)
    {
        MAX_BLOCK_RESPONSE_BYTES
    } else {
        MAX_RESPONSE_BYTES
    }
}
pub fn client() -> Result<&'static reqwest::Client, String> {
    CLIENT.as_ref().map_err(Clone::clone)
}
#[derive(Debug)]
pub enum RpcError {
    Transport,
    Protocol,
    Remote(i64),
}
impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport => f.write_str("chain RPC transport unavailable"),
            Self::Protocol => f.write_str("invalid or oversized chain RPC response"),
            Self::Remote(code) => write!(f, "chain RPC rejected request (code {code})"),
        }
    }
}
impl std::error::Error for RpcError {}
pub async fn call(
    config: &crate::network::NetworkConfig,
    method: &str,
    params: Value,
) -> Result<Value, RpcError> {
    let id = ID.fetch_add(1, Ordering::Relaxed);
    let limit = response_limit(method, &params);
    let body = serde_json::to_vec(
        &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    ).map_err(|_| RpcError::Protocol)?;
    if body.len() > MAX_REQUEST_BYTES {
        return Err(RpcError::Protocol);
    }
    let mut request = client()
        .map_err(|_| RpcError::Transport)?
        .post(&config.rpc_url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body);
    if let Some(path) = &config.rpc_bearer_file {
        let mut bearer = Zeroizing::new(String::new());
        crate::wallet::protected_file(path)
            .map_err(|_| RpcError::Transport)?
            .take(4097)
            .read_to_string(&mut bearer)
            .map_err(|_| RpcError::Transport)?;
        let bearer = bearer.trim();
        if bearer.is_empty() || bearer.len() > 4096 || bearer.bytes().any(|b| b.is_ascii_control())
        {
            return Err(RpcError::Transport);
        }
        let mut header = reqwest::header::HeaderValue::from_str(&format!("Bearer {bearer}"))
            .map_err(|_| RpcError::Transport)?;
        header.set_sensitive(true);
        request = request.header(reqwest::header::AUTHORIZATION, header);
    }
    let response = request.send().await.map_err(|_| RpcError::Transport)?;
    if !response.status().is_success() {
        return Err(RpcError::Transport);
    }
    if response
        .content_length()
        .is_some_and(|n| n > limit as u64)
    {
        return Err(RpcError::Protocol);
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| RpcError::Transport)?;
        if chunk.len() > limit - bytes.len() {
            return Err(RpcError::Protocol);
        }
        bytes.extend_from_slice(&chunk);
    }
    let mut value: Value = serde_json::from_slice(&bytes).map_err(|_| RpcError::Protocol)?;
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || value.get("id").and_then(Value::as_u64) != Some(id)
    {
        return Err(RpcError::Protocol);
    }
    if let Some(error) = value.get("error") {
        if value.get("result").is_some() || bytes.len() > MAX_RESPONSE_BYTES {
            return Err(RpcError::Protocol);
        }
        return Err(RpcError::Remote(
            error
                .get("code")
                .and_then(Value::as_i64)
                .ok_or(RpcError::Protocol)?,
        ));
    }
    value.as_object_mut().and_then(|object| object.remove("result")).ok_or(RpcError::Protocol)
}
pub fn hex_to_u128(value: &str) -> Result<u128, String> {
    let hex = value
        .strip_prefix("0x")
        .ok_or("quantity must be 0x-prefixed")?;
    if hex.is_empty()
        || hex.len() > 32
        || (hex.len() > 1 && hex.starts_with('0'))
        || !hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("invalid canonical hexadecimal quantity".into());
    }
    u128::from_str_radix(hex, 16).map_err(|_| "quantity exceeds u128".into())
}
pub fn hex_to_u64(value: &str) -> Result<u64, String> {
    u64::try_from(hex_to_u128(value)?).map_err(|_| "quantity exceeds u64".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quantities_are_strict() {
        assert_eq!(hex_to_u128("0x0").unwrap(), 0);
        assert_eq!(
            hex_to_u128("0xffffffffffffffffffffffffffffffff").unwrap(),
            u128::MAX
        );
        for bad in [
            "0x",
            "1",
            "0x00",
            "0x01",
            "0x-1",
            "0x100000000000000000000000000000000",
        ] {
            assert!(hex_to_u128(bad).is_err());
        }
    }
}
