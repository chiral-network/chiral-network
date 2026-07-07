//! Storage API — the Axum HTTP surface for the storage data plane. It puts the
//! [`crate::storage_provider::StorageProvider`] object store behind contract
//! auth and metering: a request authenticates with its contract's session
//! bearer, the bucket is bound to the contract, egress is charged to the
//! contract's balance via the ledger, and the S3 metadata headers (`ETag`,
//! `x-amz-checksum-sha256`) plus the `X-Chiral-*` metering headers are returned.
//!
//! The state is two shared handles — `Arc<Mutex<ProviderState>>` (the ledger +
//! sessions, **shared with the handshake router** so a contract opened via
//! `/v1/contracts/open` is visible here) and `Arc<Mutex<StorageProvider>>` (the
//! object store). This is what lets one provider process serve the handshake and
//! the data plane over one `ProviderState`.
//!
//! This is a working object HTTP interface (`PUT`/`GET`/`HEAD`/`DELETE`) with
//! bearer auth; full AWS SigV4 + XML listing/multipart is the tool-compat layer
//! that wraps the same store. Design: `docs/chiral-book.md` → "Data-Plane API:
//! Storage (S3)" and "Provider Implementation" → Storage.

use std::sync::{Arc, Mutex};

use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::put,
    Router,
};

use crate::contract_service::{ChainVerifier, ProviderState};
use crate::storage_provider::StorageProvider;

/// Shared state: the provider's ledger/sessions (shared with the handshake) plus
/// its object store.
pub struct StorageState<V: ChainVerifier> {
    pub provider: Arc<Mutex<ProviderState<V>>>,
    pub store: Arc<Mutex<StorageProvider>>,
}

impl<V: ChainVerifier> StorageState<V> {
    pub fn new(provider: Arc<Mutex<ProviderState<V>>>, store: StorageProvider) -> Self {
        StorageState {
            provider,
            store: Arc::new(Mutex::new(store)),
        }
    }
}

// Manual Clone (Arc clones regardless of whether V is Clone).
impl<V: ChainVerifier> Clone for StorageState<V> {
    fn clone(&self) -> Self {
        StorageState {
            provider: Arc::clone(&self.provider),
            store: Arc::clone(&self.store),
        }
    }
}

/// Build the object router (`/{bucket}/{key...}`).
pub fn router<V: ChainVerifier + Send + Sync + 'static>(state: StorageState<V>) -> Router {
    Router::new()
        .route(
            "/:bucket/*key",
            put(put_object::<V>)
                .get(get_object::<V>)
                .head(head_object::<V>)
                .delete(delete_object::<V>),
        )
        .layer(DefaultBodyLimit::max(512 * 1024 * 1024))
        .with_state(state)
}

fn bucket_for(contract: &str) -> String {
    let h = contract.trim_start_matches("0x");
    format!("c-{}", &h[..h.len().min(16)])
}

/// Resolve the bearer to a contract and check it owns the addressed bucket.
fn authed<V: ChainVerifier>(
    st: &StorageState<V>,
    headers: &HeaderMap,
    bucket: &str,
    now: u64,
) -> Result<String, StatusCode> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let contract = st
        .provider
        .lock()
        .unwrap()
        .resolve_bearer(bearer, now)
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if bucket != bucket_for(&contract) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(contract)
}

async fn put_object<V: ChainVerifier + Send + Sync + 'static>(
    State(st): State<StorageState<V>>,
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let now = now_unix();
    if let Err(s) = authed(&st, &headers, &bucket, now) {
        return s.into_response();
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let result = st
        .store
        .lock()
        .unwrap()
        .put(&bucket, &key, body.to_vec(), &content_type, now);
    match result {
        Ok(r) => Response::builder()
            .status(StatusCode::OK)
            .header(header::ETAG, format!("\"{}\"", r.etag))
            .header("x-amz-checksum-sha256", r.checksum_sha256)
            .body(Body::empty())
            .unwrap(),
        Err(e) => s3_error(&e),
    }
}

async fn get_object<V: ChainVerifier + Send + Sync + 'static>(
    State(st): State<StorageState<V>>,
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let now = now_unix();
    let contract = match authed(&st, &headers, &bucket, now) {
        Ok(c) => c,
        Err(s) => return s.into_response(),
    };
    let g = match st.store.lock().unwrap().get(&bucket, &key) {
        Ok(g) => g,
        Err(e) => return s3_error(&e),
    };
    // Charge egress before serving; refuse if the balance can't cover it.
    let balance = match st.provider.lock().unwrap().draw_down(&contract, g.egress_cost_wei) {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::PAYMENT_REQUIRED,
                "<Error><Code>InsufficientBalance</Code></Error>",
            )
                .into_response()
        }
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, g.content_type)
        .header(header::ETAG, format!("\"{}\"", g.etag))
        .header("x-amz-checksum-sha256", g.checksum_sha256)
        .header("X-Chiral-Contract-Id", contract)
        .header("X-Chiral-Cost-Wei", g.egress_cost_wei.to_string())
        .header("X-Chiral-Balance-Wei", balance.to_string())
        .body(Body::from(g.data))
        .unwrap()
}

async fn head_object<V: ChainVerifier + Send + Sync + 'static>(
    State(st): State<StorageState<V>>,
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let now = now_unix();
    if let Err(s) = authed(&st, &headers, &bucket, now) {
        return s.into_response();
    }
    let m = st.store.lock().unwrap().head(&bucket, &key);
    match m {
        Ok(m) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, m.content_type)
            .header(header::CONTENT_LENGTH, m.size.to_string())
            .header(header::ETAG, format!("\"{}\"", m.etag))
            .header("x-amz-checksum-sha256", m.checksum_sha256)
            .body(Body::empty())
            .unwrap(),
        Err(e) => (StatusCode::from_u16(e.http_status()).unwrap(), Body::empty()).into_response(),
    }
}

async fn delete_object<V: ChainVerifier + Send + Sync + 'static>(
    State(st): State<StorageState<V>>,
    Path((bucket, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    let now = now_unix();
    if let Err(s) = authed(&st, &headers, &bucket, now) {
        return s.into_response();
    }
    st.store.lock().unwrap().delete(&bucket, &key);
    StatusCode::NO_CONTENT.into_response()
}

fn s3_error(e: &crate::storage_provider::StorageError) -> Response {
    let body = format!("<Error><Code>{}</Code></Error>", e.s3_code());
    (StatusCode::from_u16(e.http_status()).unwrap(), body).into_response()
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract_service::VerifiedFunding;
    use crate::resource_offer::{ResourceClass, ResourceOffer};
    use crate::session_credential::SessionCredential;
    use tower::ServiceExt;

    struct NoChain;
    impl ChainVerifier for NoChain {
        fn verify_funding(&self, _t: &str, _to: &str) -> Result<VerifiedFunding, String> {
            Err("no".into())
        }
    }

    const CONTRACT: &str = "0xfeed00000000000000000000000000000000000000000000000000000000beef";
    const ONE_CHI: u128 = 1_000_000_000_000_000_000;

    fn offer() -> ResourceOffer {
        ResourceOffer {
            provider_wallet: "0xp".into(),
            resource_class: ResourceClass::Storage,
            capacity: serde_json::json!({}),
            price_schedule: serde_json::json!({}),
            endpoint: "https://p".into(),
            region: "".into(),
            min_funding_wei: "0".into(),
            offer_nonce: 1,
            valid_until: 0,
            signature: String::new(),
        }
    }

    fn app() -> (Router, String, String) {
        let mut provider = ProviderState::new("0xp".into(), "0x".to_string() + &"33".repeat(32), offer(), NoChain);
        provider
            .ledger
            .open(CONTRACT, "0xconsumer", ResourceClass::Storage, ONE_CHI)
            .unwrap();
        let cred = SessionCredential::derive(CONTRACT, u64::MAX, &[1u8; 64]).unwrap();
        let (bearer, bucket) = (cred.bearer.clone(), cred.bucket.clone());
        provider.sessions.insert(cred);
        let store = StorageProvider::new(1_000_000_000_000_000, 10_000_000_000_000_000, 5_000_000);
        let state = StorageState::new(Arc::new(Mutex::new(provider)), store);
        (router(state), bearer, bucket)
    }

    fn req(method: &str, uri: &str, bearer: Option<&str>, body: Vec<u8>) -> axum::http::Request<Body> {
        let mut b = axum::http::Request::builder().method(method).uri(uri);
        if let Some(t) = bearer {
            b = b.header(header::AUTHORIZATION, format!("Bearer {}", t));
        }
        b.body(Body::from(body)).unwrap()
    }

    #[tokio::test]
    async fn put_get_roundtrip_charges_egress() {
        let (app, bearer, bucket) = app();
        let uri = format!("/{}/hello.txt", bucket);
        let resp = app.clone().oneshot(req("PUT", &uri, Some(&bearer), b"hello world".to_vec())).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(header::ETAG).is_some());

        let resp = app.clone().oneshot(req("GET", &uri, Some(&bearer), vec![])).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let balance: u128 = resp.headers().get("X-Chiral-Balance-Wei").unwrap().to_str().unwrap().parse().unwrap();
        assert!(balance < 995_000_000_000_000_000);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[..], b"hello world");
    }

    #[tokio::test]
    async fn missing_auth_is_401_and_wrong_bucket_403() {
        let (app, bearer, bucket) = app();
        let uri = format!("/{}/k", bucket);
        let r = app.clone().oneshot(req("GET", &uri, None, vec![])).await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        let r = app.clone().oneshot(req("GET", "/c-someoneelse00000/k", Some(&bearer), vec![])).await.unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn missing_key_is_404_and_delete_204() {
        let (app, bearer, bucket) = app();
        let get = app.clone().oneshot(req("GET", &format!("/{}/nope", bucket), Some(&bearer), vec![])).await.unwrap();
        assert_eq!(get.status(), StatusCode::NOT_FOUND);
        let del = app.clone().oneshot(req("DELETE", &format!("/{}/nope", bucket), Some(&bearer), vec![])).await.unwrap();
        assert_eq!(del.status(), StatusCode::NO_CONTENT);
    }
}
