//! Narrow Conflux eSpace RPC gateway. Operator/mining methods are never forwarded.
use crate::{
    chain_client::{ChainClient, ChainError, MAX_ESTIMATE_GAS, MAX_PAYMENT_DATA, MAX_RAW_BYTES},
    network::{normalize_address, validate_hash},
    rpc_client::{MAX_BLOCK_RESPONSE_BYTES, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES},
};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::Semaphore;
const MAX_BATCH: usize = 8;
/// The exact `eth_*` allowlist (book line 667). No Chiral client calls `net_version`;
/// clients identify the network with `eth_chainId` and the genesis block.
pub fn allowed_method(method: &str) -> bool {
    matches!(
        method,
        "eth_chainId"
            | "eth_blockNumber"
            | "eth_gasPrice"
            | "eth_getBalance"
            | "eth_getTransactionCount"
            | "eth_getTransactionReceipt"
            | "eth_sendRawTransaction"
            | "eth_syncing"
            | "eth_getBlockByNumber"
            | "eth_estimateGas"
    )
}
struct Bucket {
    tokens: u32,
    last: Instant,
    rate: u32,
    capacity: u32,
}
impl Bucket {
    fn new(rate: u32, capacity: u32) -> Self {
        Self {
            tokens: capacity,
            last: Instant::now(),
            rate,
            capacity,
        }
    }
    fn take(&mut self, count: u32) -> bool {
        let ticks = self
            .last
            .elapsed()
            .as_millis()
            .saturating_mul(u128::from(self.rate))
            / 1000;
        if ticks > 0 {
            self.tokens = self
                .tokens
                .saturating_add(ticks.min(u128::from(u32::MAX)) as u32)
                .min(self.capacity);
            self.last = Instant::now();
        }
        if self.tokens < count {
            false
        } else {
            self.tokens -= count;
            true
        }
    }
}
struct Proxy {
    chain: Arc<ChainClient>,
    reads: Semaphore,
    writes: Semaphore,
    read_rate: Mutex<Bucket>,
    write_rate: Mutex<Bucket>,
}
fn validate(request: &Value) -> Result<bool, &'static str> {
    let object = request
        .as_object()
        .ok_or("JSON-RPC envelope must be an object")?;
    if object
        .keys()
        .any(|k| !matches!(k.as_str(), "jsonrpc" | "id" | "method" | "params"))
        || request["jsonrpc"] != "2.0"
    {
        return Err("invalid JSON-RPC envelope");
    }
    let id = request.get("id").ok_or("notifications are not supported")?;
    if !(id.is_i64() || id.is_u64() || id.as_str().is_some_and(|s| s.len() <= 128)) {
        return Err("invalid JSON-RPC id");
    }
    let method = request["method"].as_str().ok_or("missing method")?;
    if !allowed_method(method) {
        return Err("RPC method is not exposed");
    }
    let params = request["params"]
        .as_array()
        .ok_or("params must be an array")?;
    match method {
        "eth_getBalance" | "eth_getTransactionCount" => {
            if params.len() != 2
                || !(params[1] == "latest"
                    || (method == "eth_getTransactionCount" && params[1] == "pending"))
                || params[0]
                    .as_str()
                    .and_then(|a| normalize_address(a).ok())
                    .is_none()
            {
                return Err("requires address and a supported account block tag");
            }
        }
        "eth_getBlockByNumber" => {
            if params.len() != 2 || params[1] != false
                || !params[0].as_str().is_some_and(|tag|
                    matches!(tag, "latest" | "safe" | "finalized")
                        || crate::rpc_client::hex_to_u64(tag).is_ok())
            {
                return Err("requires a canonical block height or latest/safe/finalized tag, and false");
            }
        }
        "eth_estimateGas" => {
            let transaction = params.first().and_then(Value::as_object)
                .ok_or("requires a bounded native payment estimate")?;
            if params.len() != 1 || transaction.len() != 6
                || transaction.keys().any(|key| !matches!(key.as_str(),
                    "from" | "to" | "value" | "data" | "gasPrice" | "gas"))
            {
                return Err("estimate requires from, to, value, data, gasPrice and bounded gas only");
            }
            for field in ["from", "to"] {
                if !transaction[field].as_str().is_some_and(|value| normalize_address(value).is_ok()) {
                    return Err("invalid estimate address");
                }
            }
            for field in ["value", "gasPrice", "gas"] {
                if !transaction[field].as_str().is_some_and(|value| crate::rpc_client::hex_to_u128(value).is_ok()) {
                    return Err("invalid estimate quantity");
                }
            }
            let gas = crate::rpc_client::hex_to_u128(transaction["gas"].as_str().unwrap()).unwrap();
            if gas < 21000 || gas > u128::from(MAX_ESTIMATE_GAS) {
                return Err("estimate gas exceeds native payment bounds");
            }
            if !transaction["data"].as_str().is_some_and(|data|
                data.starts_with("0x") && data.len() % 2 == 0
                    && data.len() <= MAX_PAYMENT_DATA * 2 + 2
                    && data[2..].bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return Err("invalid or oversized estimate data");
            }
        }
        "eth_getTransactionReceipt" => {
            if params.len() != 1
                || params[0]
                    .as_str()
                    .and_then(|h| validate_hash(h).ok())
                    .is_none()
            {
                return Err("requires one 32-byte transaction hash");
            }
        }
        "eth_sendRawTransaction" => {
            let raw = params
                .first()
                .and_then(Value::as_str)
                .ok_or("requires a signed transaction")?;
            if params.len() != 1
                || raw.len() < 4
                || raw.len() > MAX_RAW_BYTES * 2 + 2
                || !raw.starts_with("0x")
                || raw.len() % 2 != 0
                || !raw[2..].bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("invalid or oversized signed transaction");
            }
        }
        _ => {
            if !params.is_empty() {
                return Err("method requires empty params");
            }
        }
    }
    Ok(method == "eth_sendRawTransaction")
}
fn error(status: StatusCode, code: i64, message: &str) -> Response {
    (
        status,
        Json(json!({"jsonrpc":"2.0","id":null,"error":{"code":code,"message":message}})),
    )
        .into_response()
}
// Serialize each reply directly into one capped body instead of retaining up to
// eight parsed block results and then allocating a second aggregate JSON copy.
struct ResponseBody {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for ResponseBody {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit - self.bytes.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "RPC response limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
async fn proxy(State(state): State<Arc<Proxy>>, body: Bytes) -> Response {
    if body.len() > MAX_REQUEST_BYTES {
        return error(
            StatusCode::PAYLOAD_TOO_LARGE,
            -32600,
            "RPC request exceeds size limit",
        );
    }
    let value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return error(StatusCode::BAD_REQUEST, -32700, "invalid JSON"),
    };
    let batch = value.is_array();
    let requests = if let Some(array) = value.as_array() {
        array.as_slice()
    } else {
        std::slice::from_ref(&value)
    };
    if requests.is_empty() || requests.len() > MAX_BATCH {
        return error(
            StatusCode::BAD_REQUEST,
            -32600,
            "batch must contain 1..8 requests",
        );
    }
    let mut reads = 0;
    let mut writes = 0;
    for request in requests {
        match validate(request) {
            Ok(true) => writes += 1,
            Ok(false) => reads += 1,
            Err(message) => return error(StatusCode::BAD_REQUEST, -32600, message),
        }
    }
    // Reject the whole batch before any write when either class is saturated.
    if !state.read_rate.lock().is_ok_and(|mut b| b.take(reads))
        || !state.write_rate.lock().is_ok_and(|mut b| b.take(writes))
    {
        return error(StatusCode::TOO_MANY_REQUESTS, -32005, "RPC rate limit");
    }
    let _read = if reads > 0 {
        match state.reads.try_acquire_many(reads) {
            Ok(p) => Some(p),
            Err(_) => {
                return error(
                    StatusCode::TOO_MANY_REQUESTS,
                    -32005,
                    "read capacity exhausted",
                )
            }
        }
    } else {
        None
    };
    let _write = if writes > 0 {
        match state.writes.try_acquire_many(writes) {
            Ok(p) => Some(p),
            Err(_) => {
                return error(
                    StatusCode::TOO_MANY_REQUESTS,
                    -32005,
                    "submission capacity exhausted",
                )
            }
        }
    } else {
        None
    };
    // The aggregate cap is independent of batch length. Ordinary-only batches
    // retain the small bound; even eight large block reads share one block cap.
    let limit = if requests.iter().any(|r| r["method"] == "eth_getBlockByNumber") {
        MAX_BLOCK_RESPONSE_BYTES
    } else {
        MAX_RESPONSE_BYTES
    };
    let mut body = ResponseBody { bytes: Vec::new(), limit };
    if batch {
        body.bytes.push(b'[');
    }
    for request in requests {
        let result = state
            .chain
            .proxy_call(
                request["method"].as_str().expect("validated method"),
                request["params"].clone(),
            )
            .await;
        let mut response = json!({"jsonrpc":"2.0","id":request["id"]});
        match result {
            Ok(result) => response["result"] = result,
            Err(reason) => {
                let (code, message) = match reason {
                    ChainError::Rejected(_) => (-32602, "chain request rejected"),
                    ChainError::Pending(_) => (-32001, "chain request pending"),
                    ChainError::Unavailable(_) => (-32002, "eSpace chain RPC unavailable"),
                };
                response["error"] = json!({"code":code,"message":message});
            }
        };
        if batch && body.bytes.len() > 1 && body.write_all(b",").is_err() {
            return error(StatusCode::BAD_GATEWAY, -32005, "RPC batch response exceeds size limit");
        }
        if serde_json::to_writer(&mut body, &response).is_err() {
            return error(StatusCode::BAD_GATEWAY,-32005,"RPC response exceeds size limit; query individually (a submitted transaction may already be admitted)");
        }
    }
    if batch && body.write_all(b"]").is_err() {
        return error(StatusCode::BAD_GATEWAY, -32005, "RPC batch response exceeds size limit");
    }
    ([(CONTENT_TYPE, "application/json")], body.bytes).into_response()
}
pub fn router(chain: Arc<ChainClient>) -> Router {
    let state = Arc::new(Proxy {
        chain,
        reads: Semaphore::new(32),
        writes: Semaphore::new(4),
        read_rate: Mutex::new(Bucket::new(64, 128)),
        write_rate: Mutex::new(Bucket::new(2, 4)),
    });
    Router::new()
        .route("/api/chain/rpc", post(proxy))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_namespace_bypass() {
        for method in [
            "eth_sendTransaction",
            "eth_call",
            "eth_getTransactionByHash",
            "admin_stopRPC",
            "personal_sign",
            "miner_start",
            "eth_submitWork",
            "eth_submitHashrate",
            "debug_traceTransaction",
            "eth_futureOperatorMethod",
        ] {
            assert!(!allowed_method(method), "{method}");
        }
        for method in [
            "eth_syncing",
            "eth_getBlockByNumber",
            "eth_estimateGas",
            "eth_sendRawTransaction",
            "eth_getTransactionReceipt",
        ] {
            assert!(allowed_method(method));
        }
    }
    #[test]
    fn allowlist_is_exactly_ten_eth_methods() {
        for method in [
            "eth_chainId",
            "eth_blockNumber",
            "eth_gasPrice",
            "eth_getBalance",
            "eth_getTransactionCount",
            "eth_getTransactionReceipt",
            "eth_sendRawTransaction",
            "eth_syncing",
            "eth_getBlockByNumber",
            "eth_estimateGas",
        ] {
            assert!(allowed_method(method), "{method}");
        }
        // Not in the eth_* allowlist, and no case, prefix or padding variant of a listed name.
        for method in [
            "net_version",
            "net_listening",
            "net_peerCount",
            "web3_clientVersion",
            "web3_sha3",
            "eth_chainid",
            "ETH_CHAINID",
            "eth_chainId ",
            " eth_chainId",
            "eth_",
            "eth_*",
            "",
        ] {
            assert!(!allowed_method(method), "{method:?}");
            assert_eq!(
                validate(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":[]})),
                Err("RPC method is not exposed"),
                "{method:?}"
            );
        }
    }
    #[test]
    fn validates_supported_shapes_only() {
        let address = format!("0x{}", "11".repeat(20));
        assert!(validate(
            &json!({"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":[address,"latest"]})
        )
        .is_ok());
        for params in [
            json!([address, "pending"]),
            json!(["0x01", "latest"]),
            json!([address, "latest", true]),
        ] {
            assert!(validate(
                &json!({"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":params})
            )
            .is_err());
        }
        assert!(validate(
            &json!({"jsonrpc":"2.0","id":1,"method":"eth_getTransactionCount","params":[address,"pending"]})
        ).is_ok());
        for tag in ["0x0", "0xa", "latest", "safe", "finalized"] {
            assert!(validate(&json!({"jsonrpc":"2.0","id":1,"method":"eth_getBlockByNumber","params":[tag,false]})).is_ok());
        }
        for params in [json!(["latest",true]), json!(["pending",false]), json!(["0x00",false]), json!(["0x10000000000000000",false])] {
            assert!(validate(&json!({"jsonrpc":"2.0","id":1,"method":"eth_getBlockByNumber","params":params})).is_err());
        }
        assert!(validate(&json!({"jsonrpc":"2.0","method":"eth_chainId","params":[]})).is_err());
        assert!(validate(
            &json!({"jsonrpc":"2.0","id":1,"method":"eth_submitWork","params":[]})
        )
        .is_err());
    }
    #[test]
    fn native_estimation_is_independently_bounded() {
        let mut tx = json!({"from":format!("0x{}", "11".repeat(20)), "to":format!("0x{}", "22".repeat(20)),
            "value":"0x1", "data":"0x00", "gasPrice":"0x1", "gas":"0xf4240"});
        let valid = |tx: &Value| validate(&json!({"jsonrpc":"2.0","id":1,"method":"eth_estimateGas","params":[tx]})).is_ok();
        assert!(valid(&tx));
        tx["gas"] = json!("0xf4241");
        assert!(!valid(&tx));
        tx["gas"] = json!("0xf4240");
        tx["data"] = json!(format!("0x{}", "00".repeat(MAX_PAYMENT_DATA + 1)));
        assert!(!valid(&tx));
        tx["data"] = json!("0x");
        tx["stateOverrides"] = json!({});
        assert!(!valid(&tx));
    }
    /// Relay-path defensive bound, not the decoder: `eth_sendRawTransaction` text is refused
    /// beyond `0x` plus twice `MAX_RAW_BYTES` digits, and is inclusive at that length. The
    /// at-bound request fits the 64 KiB request cap, so that cap does not mask this check.
    /// `proxy_call` still applies the full decoder before forwarding anything.
    #[test]
    fn raw_transaction_text_bound_is_inclusive() {
        const REFUSED: &str = "invalid or oversized signed transaction";
        let request = |raw: &str| json!({"jsonrpc":"2.0","id":1,"method":"eth_sendRawTransaction","params":[raw]});
        let at_bound = format!("0x{}", "aB".repeat(MAX_RAW_BYTES));
        assert_eq!(at_bound.len(), MAX_RAW_BYTES * 2 + 2);
        assert!(serde_json::to_vec(&request(&at_bound)).unwrap().len() <= MAX_REQUEST_BYTES);
        assert_eq!(validate(&request(&at_bound)), Ok(true));
        assert_eq!(validate(&request("0x00")), Ok(true));
        for raw in [
            format!("0x{}", "ab".repeat(MAX_RAW_BYTES + 1)),
            format!("{at_bound}a"),
            format!("0x{}", "ab".repeat(4 * MAX_RAW_BYTES)),
            "0x".into(),
            "0x0".into(),
            format!("0x{}a", "ab".repeat(MAX_RAW_BYTES - 1)),
            format!("0X{}", "ab".repeat(MAX_RAW_BYTES)),
            format!("0x{}zz", "ab".repeat(MAX_RAW_BYTES - 1)),
        ] {
            assert_eq!(validate(&request(&raw)), Err(REFUSED), "{} characters", raw.len());
        }
        assert_eq!(
            validate(&json!({"jsonrpc":"2.0","id":1,"method":"eth_sendRawTransaction","params":[at_bound, at_bound]})),
            Err(REFUSED)
        );
    }
    #[test]
    fn submission_budget_is_independent() {
        let mut reads = Bucket::new(64, 128);
        let mut writes = Bucket::new(2, 4);
        assert!(reads.take(128));
        assert!(!reads.take(1));
        assert!(writes.take(4));
        assert!(!writes.take(1));
    }
}
