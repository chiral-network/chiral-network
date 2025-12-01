use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ed25519_dalek::VerifyingKey;
use reqwest::header::{CACHE_CONTROL, ETAG, IF_NONE_MATCH};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use url::Url;

#[derive(Debug, thiserror::Error)]
pub enum JwksError {
    #[error("jwks fetch failed: {0}")]
    Network(String),
    #[error("unexpected http status: {0}")]
    UnexpectedStatus(u16),
    #[error("jwks parse error: {0}")]
    InvalidDocument(String),
    #[error("jwks missing key: {0}")]
    KeyNotFound(String),
    #[error("jwks cache empty")]
    EmptyCache,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JwkDocument {
    pub keys: Vec<Jwk>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Jwk {
    pub kty: String,
    #[serde(rename = "use", default)]
    pub usage: Option<String>,
    #[serde(default)]
    pub alg: Option<String>,
    #[serde(default)]
    pub crv: Option<String>,
    #[serde(default)]
    pub kid: Option<String>,
    #[serde(default)]
    pub x: Option<String>,
}

#[derive(Debug, Clone)]
pub struct JwksFetchResult {
    pub document: Option<JwkDocument>,
    pub etag: Option<String>,
    pub max_age: Option<Duration>,
    pub not_modified: bool,
}

#[async_trait]
pub trait JwksFetcher: Send + Sync + 'static {
    async fn fetch(
        &self,
        url: &Url,
        etag: Option<&str>,
    ) -> Result<JwksFetchResult, JwksError>;
}

#[derive(Clone)]
pub struct HttpJwksFetcher {
    client: Client,
}

impl HttpJwksFetcher {
    pub fn new() -> Result<Self, JwksError> {
        let client = Client::builder()
            .user_agent("chiral-control-plane/1.0")
            .build()
            .map_err(|e| JwksError::Network(e.to_string()))?;
        Ok(Self { client })
    }

    pub fn with_client(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl JwksFetcher for HttpJwksFetcher {
    async fn fetch(
        &self,
        url: &Url,
        etag: Option<&str>,
    ) -> Result<JwksFetchResult, JwksError> {
        let mut req = self.client.get(url.clone());
        if let Some(tag) = etag {
            req = req.header(IF_NONE_MATCH, tag);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| JwksError::Network(e.to_string()))?;

        if resp.status() == StatusCode::NOT_MODIFIED {
            let cache_control = resp.headers().get(CACHE_CONTROL).cloned();
            return Ok(JwksFetchResult {
                document: None,
                etag: etag.map(|s| s.to_string()),
                max_age: cache_control.and_then(parse_max_age),
                not_modified: true,
            });
        }

        if !resp.status().is_success() {
            return Err(JwksError::UnexpectedStatus(resp.status().as_u16()));
        }

        let cache_control = resp.headers().get(CACHE_CONTROL).cloned();
        let etag = resp
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let document = resp
            .json::<JwkDocument>()
            .await
            .map_err(|e| JwksError::InvalidDocument(e.to_string()))?;

        Ok(JwksFetchResult {
            document: Some(document),
            etag,
            max_age: cache_control.and_then(parse_max_age),
            not_modified: false,
        })
    }
}

fn parse_max_age(value: reqwest::header::HeaderValue) -> Option<Duration> {
    let value_str = value.to_str().ok()?;
    value_str
        .split(',')
        .find_map(|directive| {
            let directive = directive.trim();
            if let Some(rest) = directive.strip_prefix("max-age=") {
                rest.parse::<u64>().ok()
            } else {
                None
            }
        })
        .map(Duration::from_secs)
}

struct CachedJwks {
    keys: HashMap<String, VerifyingKey>,
    etag: Option<String>,
    expires_at: Instant,
    max_age: Duration,
}

pub struct JwksCache<F: JwksFetcher> {
    fetcher: F,
    jwks_url: Url,
    default_ttl: Duration,
    state: Arc<Mutex<Option<CachedJwks>>>,
}

impl<F: JwksFetcher> JwksCache<F> {
    pub fn new(jwks_url: Url, fetcher: F) -> Self {
        Self {
            fetcher,
            jwks_url,
            default_ttl: Duration::from_secs(300),
            state: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn get_key(&self, kid: &str) -> Result<VerifyingKey, JwksError> {
        {
            let guard = self.state.lock().await;
            if let Some(cache) = guard.as_ref() {
                if cache.expires_at > Instant::now() {
                    if let Some(key) = cache.keys.get(kid) {
                        return Ok(key.clone());
                    }
                }
            }
        }

        self.refresh().await?;

        let guard = self.state.lock().await;
        let cache = guard.as_ref().ok_or(JwksError::EmptyCache)?;
        cache
            .keys
            .get(kid)
            .cloned()
            .ok_or_else(|| JwksError::KeyNotFound(kid.to_string()))
    }

    async fn refresh(&self) -> Result<(), JwksError> {
        let current_etag = {
            let guard = self.state.lock().await;
            guard.as_ref().and_then(|c| c.etag.clone())
        };

        let response = self
            .fetcher
            .fetch(&self.jwks_url, current_etag.as_deref())
            .await?;

        if response.not_modified {
            let mut guard = self.state.lock().await;
            if let Some(cache) = guard.as_mut() {
                let ttl = response.max_age.unwrap_or(cache.max_age);
                cache.max_age = ttl;
                cache.expires_at = Instant::now() + ttl;
            } else {
                return Err(JwksError::EmptyCache);
            }
            return Ok(());
        }

        let document = response
            .document
            .ok_or_else(|| JwksError::InvalidDocument("missing body".into()))?;
        let keys = build_key_map(&document)?;
        let ttl = response.max_age.unwrap_or(self.default_ttl);
        let new_cache = CachedJwks {
            keys,
            etag: response.etag,
            expires_at: Instant::now() + ttl,
            max_age: ttl,
        };
        let mut guard = self.state.lock().await;
        *guard = Some(new_cache);

        Ok(())
    }
}

fn build_key_map(document: &JwkDocument) -> Result<HashMap<String, VerifyingKey>, JwksError> {
    let mut map = HashMap::new();
    for jwk in &document.keys {
        if jwk.kty != "OKP" {
            continue;
        }
        if jwk.crv.as_deref() != Some("Ed25519") {
            continue;
        }
        let kid = match &jwk.kid {
            Some(kid) => kid.clone(),
            None => continue,
        };
        let x = jwk
            .x
            .as_ref()
            .ok_or_else(|| JwksError::InvalidDocument("missing x".into()))?;
        let bytes = URL_SAFE_NO_PAD
            .decode(x)
            .map_err(|e| JwksError::InvalidDocument(e.to_string()))?;
        let array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| JwksError::InvalidDocument("invalid key length".into()))?;
        let key = VerifyingKey::from_bytes(&array)
            .map_err(|e| JwksError::InvalidDocument(e.to_string()))?;
        map.insert(kid, key);
    }
    if map.is_empty() {
        return Err(JwksError::InvalidDocument(
            "no usable keys in JWKS".into(),
        ));
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StaticFetcher {
        doc: Option<JwkDocument>,
        etag: Option<String>,
    }

    #[async_trait]
    impl JwksFetcher for StaticFetcher {
        async fn fetch(
            &self,
            _url: &Url,
            _etag: Option<&str>,
        ) -> Result<JwksFetchResult, JwksError> {
            Ok(JwksFetchResult {
                document: self.doc.clone(),
                etag: self.etag.clone(),
                max_age: Some(Duration::from_secs(60)),
                not_modified: false,
            })
        }
    }

    #[tokio::test]
    async fn caches_key() {
        let keypair = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        let jwk = Jwk {
            kty: "OKP".into(),
            usage: None,
            alg: Some("EdDSA".into()),
            crv: Some("Ed25519".into()),
            kid: Some("kid1".into()),
            x: Some(URL_SAFE_NO_PAD.encode(keypair.verifying_key().to_bytes())),
        };
        let fetcher = StaticFetcher {
            doc: Some(JwkDocument { keys: vec![jwk] }),
            etag: Some("etag".into()),
        };
        let cache = JwksCache::new(Url::parse("https://example.com/jwks").unwrap(), fetcher);
        let key = cache.get_key("kid1").await.unwrap();
        assert_eq!(key.as_bytes(), keypair.verifying_key().as_bytes());
    }
}
