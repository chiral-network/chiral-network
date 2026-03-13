use axum::{
    body::Bytes,
    http::{header::CONTENT_TYPE, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};

/// Minimal same-origin JSON-RPC proxy for browser share pages.
/// This allows browser wallet payment flows without exposing private keys to the server.
async fn proxy_chain_rpc(body: Bytes) -> Response {
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
