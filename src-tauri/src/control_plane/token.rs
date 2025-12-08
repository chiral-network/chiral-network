use super::handshake::{HandshakeAck, HandshakeRequest, LeaseWindow};
use super::jwks::{JwksCache, JwksError, JwksFetcher};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, SigningKey};
#[cfg(test)]
use ed25519_dalek::VerifyingKey;
use ed25519_dalek::Signer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

const MIN_LEASE_SECS: i64 = 300; // 5 minutes
const MAX_LEASE_SECS: i64 = 86_400; // 24 hours
const DEFAULT_LEASE_SECS: i64 = 14_400; // 4 hours
const DEFAULT_SCOPE: &str = "resume";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResumeTokenClaims {
    pub sub: String,
    pub aud: String,
    pub download_id: String,
    pub etag: String,
    pub epoch: u64,
    pub iat: i64,
    pub nbf: i64,
    pub exp: i64,
    pub scp: String,
    pub kid: String,
}

#[derive(Debug, Serialize)]
struct TokenHeader<'a> {
    alg: &'a str,
    typ: &'a str,
    kid: &'a str,
}

#[derive(Debug, Deserialize)]
struct TokenHeaderOwned {
    alg: String,
    #[allow(dead_code)]
    typ: Option<String>,
    kid: String,
}

#[derive(Debug, Error)]
pub enum ResumeTokenError {
    #[error("invalid token: {0}")]
    Invalid(&'static str),
    #[error("token signature invalid")]
    Signature,
    #[error("token expired")]
    Expired,
    #[error("token not yet valid")]
    NotYetValid,
    #[error("clock skew exceeded tolerance")]
    ClockSkew,
    #[error("weak etag rejected")]
    WeakEtag,
    #[error("jwks error: {0}")]
    Jwks(#[from] JwksError),
}

pub fn ensure_strong_etag(etag: &str) -> Result<String, ResumeTokenError> {
    let trimmed = etag.trim();
    if trimmed.is_empty() {
        return Err(ResumeTokenError::Invalid("etag missing"));
    }
    if trimmed.starts_with("W/") {
        return Err(ResumeTokenError::WeakEtag);
    }
    Ok(trimmed.to_string())
}

pub struct ResumeTokenSigner {
    signing_key: SigningKey,
    key_id: String,
    seeder_peer_id: String,
    default_duration: Duration,
}

impl ResumeTokenSigner {
    pub fn new(signing_key: SigningKey, key_id: impl Into<String>, seeder_peer_id: impl Into<String>) -> Self {
        Self {
            signing_key,
            key_id: key_id.into(),
            seeder_peer_id: seeder_peer_id.into(),
            default_duration: Duration::seconds(DEFAULT_LEASE_SECS),
        }
    }

    pub fn with_default_duration(mut self, duration: Duration) -> Self {
        self.default_duration = duration;
        self
    }

    pub fn issue_ack(
        &self,
        request: &HandshakeRequest,
        etag: &str,
        size: u64,
        epoch: u64,
        now: DateTime<Utc>,
        lease_override: Option<Duration>,
    ) -> Result<HandshakeAck, ResumeTokenError> {
        let lease = lease_override.unwrap_or(self.default_duration);
        let clamped = clamp_duration(lease)?;
        let issued_at = now;
        let expires_at = now + clamped;
        let etag = ensure_strong_etag(etag)?;

        let claims = ResumeTokenClaims {
            sub: request.file_id.clone(),
            aud: self.seeder_peer_id.clone(),
            download_id: request.download_id.clone(),
            etag: etag.clone(),
            epoch,
            iat: issued_at.timestamp(),
            nbf: issued_at.timestamp(),
            exp: expires_at.timestamp(),
            scp: DEFAULT_SCOPE.to_string(),
            kid: self.key_id.clone(),
        };

        let token = self.encode_token(&claims)?;
        Ok(HandshakeAck {
            file_id: request.file_id.clone(),
            download_id: request.download_id.clone(),
            epoch,
            etag,
            size,
            lease_exp: expires_at,
            lease_issued_at: issued_at,
            resume_token: token,
        })
    }

    fn encode_token(&self, claims: &ResumeTokenClaims) -> Result<String, ResumeTokenError> {
        let header = TokenHeader {
            alg: "EdDSA",
            typ: "JWT",
            kid: &self.key_id,
        };
        let header_json = serde_json::to_vec(&header).map_err(|_| ResumeTokenError::Invalid("header"))?;
        let payload_json = serde_json::to_vec(claims).map_err(|_| ResumeTokenError::Invalid("claims"))?;
        let header_b64 = URL_SAFE_NO_PAD.encode(header_json);
        let payload_b64 = URL_SAFE_NO_PAD.encode(payload_json);
        let signing_input = format!("{}.{}", header_b64, payload_b64);
        let signature = self.signing_key.sign(signing_input.as_bytes());
        let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());
        Ok(format!("{}.{}", signing_input, signature_b64))
    }
}

fn clamp_duration(duration: Duration) -> Result<Duration, ResumeTokenError> {
    let secs = duration.num_seconds();
    if secs < MIN_LEASE_SECS {
        return Err(ResumeTokenError::Invalid("lease too short"));
    }
    if secs > MAX_LEASE_SECS {
        return Err(ResumeTokenError::Invalid("lease too long"));
    }
    Ok(duration)
}

pub struct ResumeTokenVerifier<F: JwksFetcher> {
    cache: Arc<JwksCache<F>>,
    expected_audience: String,
    expected_scope: String,
    max_clock_skew: Duration,
}

impl<F: JwksFetcher> ResumeTokenVerifier<F> {
    pub fn new(cache: Arc<JwksCache<F>>, expected_audience: impl Into<String>) -> Self {
        Self {
            cache,
            expected_audience: expected_audience.into(),
            expected_scope: DEFAULT_SCOPE.to_string(),
            max_clock_skew: Duration::minutes(5),
        }
    }

    pub fn with_clock_skew(mut self, skew: Duration) -> Self {
        self.max_clock_skew = skew;
        self
    }

    pub async fn verify_ack(
        &self,
        ack: &HandshakeAck,
        expected_file_id: &str,
        expected_download_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(ResumeTokenClaims, LeaseWindow), ResumeTokenError> {
        let claims = self
            .verify_token(
                &ack.resume_token,
                expected_file_id,
                expected_download_id,
                ack.epoch,
                now,
            )
            .await?;
        let etag = ensure_strong_etag(&ack.etag)?;
        if etag != claims.etag {
            return Err(ResumeTokenError::Invalid("etag mismatch"));
        }
        if ack.lease_exp.timestamp() != claims.exp {
            return Err(ResumeTokenError::Invalid("lease exp mismatch"));
        }
        if ack.lease_issued_at.timestamp() != claims.iat {
            return Err(ResumeTokenError::Invalid("lease issued mismatch"));
        }
        Ok((claims, ack.lease_window()))
    }

    async fn verify_token(
        &self,
        token: &str,
        expected_file_id: &str,
        expected_download_id: &str,
        expected_epoch: u64,
        now: DateTime<Utc>,
    ) -> Result<ResumeTokenClaims, ResumeTokenError> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(ResumeTokenError::Invalid("token format"));
        }
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let header: TokenHeaderOwned = decode_json_part(parts[0])?;
        if header.alg != "EdDSA" {
            return Err(ResumeTokenError::Invalid("alg"));
        }
        let signature_bytes = URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|_| ResumeTokenError::Signature)?;
        let signature = Signature::from_bytes(
            signature_bytes
                .as_slice()
                .try_into()
                .map_err(|_| ResumeTokenError::Signature)?,
        );
        let key = self.cache.get_key(&header.kid).await?;
        key.verify_strict(signing_input.as_bytes(), &signature)
            .map_err(|_| ResumeTokenError::Signature)?;
        let claims: ResumeTokenClaims = decode_json_part(parts[1])?;
        if claims.kid != header.kid {
            return Err(ResumeTokenError::Invalid("kid mismatch"));
        }
        self.validate_claims(
            &claims,
            expected_file_id,
            expected_download_id,
            expected_epoch,
            now,
        )?;
        Ok(claims)
    }

    fn validate_claims(
        &self,
        claims: &ResumeTokenClaims,
        expected_file_id: &str,
        expected_download_id: &str,
        expected_epoch: u64,
        now: DateTime<Utc>,
    ) -> Result<(), ResumeTokenError> {
        if claims.sub != expected_file_id {
            return Err(ResumeTokenError::Invalid("file id"));
        }
        if claims.download_id != expected_download_id {
            return Err(ResumeTokenError::Invalid("download id"));
        }
        if claims.epoch != expected_epoch {
            return Err(ResumeTokenError::Invalid("epoch"));
        }
        if claims.aud != self.expected_audience {
            return Err(ResumeTokenError::Invalid("aud"));
        }
        if claims.scp != self.expected_scope {
            return Err(ResumeTokenError::Invalid("scope"));
        }
        ensure_strong_etag(&claims.etag)?;
        let lease_len = claims.exp - claims.iat;
        if !(MIN_LEASE_SECS..=MAX_LEASE_SECS).contains(&lease_len) {
            return Err(ResumeTokenError::Invalid("lease bounds"));
        }
        let skew = self.max_clock_skew.num_seconds();
        let now_ts = now.timestamp();
        if now_ts - skew > claims.exp {
            return Err(ResumeTokenError::Expired);
        }
        if now_ts + skew < claims.nbf {
            return Err(ResumeTokenError::NotYetValid);
        }
        if (claims.iat - now_ts).abs() > skew {
            return Err(ResumeTokenError::ClockSkew);
        }
        Ok(())
    }
}

fn decode_json_part<T: for<'de> Deserialize<'de>>(part: &str) -> Result<T, ResumeTokenError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| ResumeTokenError::Invalid("b64"))?;
    serde_json::from_slice(&bytes).map_err(|_| ResumeTokenError::Invalid("json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_plane::jwks::{Jwk, JwkDocument, JwksError, JwksFetchResult, JwksFetcher};
    use async_trait::async_trait;
    use rand::rngs::OsRng;

    struct MockFetcher {
        doc: JwkDocument,
    }

    #[async_trait]
    impl JwksFetcher for MockFetcher {
        async fn fetch(
            &self,
            _url: &url::Url,
            _etag: Option<&str>,
        ) -> Result<JwksFetchResult, JwksError> {
            Ok(JwksFetchResult {
                document: Some(self.doc.clone()),
                etag: Some("kid".into()),
                max_age: Some(std::time::Duration::from_secs(60)),
                not_modified: false,
            })
        }
    }

    fn cache_with_key(key: &VerifyingKey, kid: &str) -> Arc<JwksCache<MockFetcher>> {
        let jwk = Jwk {
            kty: "OKP".into(),
            usage: None,
            alg: Some("EdDSA".into()),
            crv: Some("Ed25519".into()),
            kid: Some(kid.to_string()),
            x: Some(URL_SAFE_NO_PAD.encode(key.to_bytes())),
        };
        Arc::new(JwksCache::new(
            url::Url::parse("https://example.com/jwks").unwrap(),
            MockFetcher {
                doc: JwkDocument { keys: vec![jwk] },
            },
        ))
    }

    #[tokio::test]
    async fn round_trip() {
        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();
        let request = HandshakeRequest::new("file", "download", 7, "peerA");
        let signer = ResumeTokenSigner::new(signing, "kid1", "peerB");
        let ack = signer
            .issue_ack(&request, "\"strong\"", 1337, request.epoch, Utc::now(), None)
            .unwrap();
        let cache = cache_with_key(&verifying, "kid1");
        let verifier = ResumeTokenVerifier::new(cache, "peerB");
        let (claims, window) = verifier
            .verify_ack(&ack, "file", "download", Utc::now())
            .await
            .unwrap();
        assert_eq!(claims.sub, "file");
        assert_eq!(window.duration().num_seconds(), DEFAULT_LEASE_SECS);
    }

    #[tokio::test]
    async fn rejects_wrong_audience() {
        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();
        let request = HandshakeRequest::new("file", "download", 1, "peerA");
        let signer = ResumeTokenSigner::new(signing, "kid1", "peerB");
        let ack = signer
            .issue_ack(&request, "\"strong\"", 10, request.epoch, Utc::now(), None)
            .unwrap();
        let cache = cache_with_key(&verifying, "kid1");
        let verifier = ResumeTokenVerifier::new(cache, "peerC");
        let err = verifier
            .verify_ack(&ack, "file", "download", Utc::now())
            .await
            .unwrap_err();
        assert!(matches!(err, ResumeTokenError::Invalid("aud")));
    }

    #[tokio::test]
    async fn rejects_missing_kid() {
        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();
        let request = HandshakeRequest::new("file", "download", 1, "peerA");
        let signer = ResumeTokenSigner::new(signing, "kid_missing", "peerB");
        let ack = signer
            .issue_ack(&request, "\"strong\"", 10, request.epoch, Utc::now(), None)
            .unwrap();
        let cache = cache_with_key(&verifying, "kid1");
        let verifier = ResumeTokenVerifier::new(cache, "peerB");
        let err = verifier
            .verify_ack(&ack, "file", "download", Utc::now())
            .await
            .unwrap_err();
        assert!(matches!(err, ResumeTokenError::Jwks(JwksError::KeyNotFound(_))));
    }

    #[tokio::test]
    async fn rejects_expired_token() {
        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();
        let request = HandshakeRequest::new("file", "download", 1, "peerA");
        let signer = ResumeTokenSigner::new(signing, "kid1", "peerB");
        let ack = signer
            .issue_ack(&request, "\"strong\"", 10, request.epoch, Utc::now(), None)
            .unwrap();
        let cache = cache_with_key(&verifying, "kid1");
        let verifier = ResumeTokenVerifier::new(cache, "peerB");
        let future = ack.lease_exp + Duration::minutes(10);
        let err = verifier
            .verify_ack(&ack, "file", "download", future)
            .await
            .unwrap_err();
        assert!(matches!(err, ResumeTokenError::Expired));
    }

    #[tokio::test]
    async fn rejects_not_yet_valid_token() {
        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();
        let request = HandshakeRequest::new("file", "download", 1, "peerA");
        let signer = ResumeTokenSigner::new(signing, "kid1", "peerB");
        let ack = signer
            .issue_ack(&request, "\"strong\"", 10, request.epoch, Utc::now(), None)
            .unwrap();
        let cache = cache_with_key(&verifying, "kid1");
        let verifier = ResumeTokenVerifier::new(cache, "peerB");
        let past = ack.lease_issued_at - Duration::minutes(10);
        let err = verifier
            .verify_ack(&ack, "file", "download", past)
            .await
            .unwrap_err();
        assert!(matches!(err, ResumeTokenError::NotYetValid));
    }

    #[tokio::test]
    async fn rejects_clock_skew() {
        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();
        let request = HandshakeRequest::new("file", "download", 1, "peerA");
        let signer = ResumeTokenSigner::new(signing, "kid1", "peerB");
        let ack = signer
            .issue_ack(&request, "\"strong\"", 10, request.epoch, Utc::now(), None)
            .unwrap();
        let cache = cache_with_key(&verifying, "kid1");
        let verifier = ResumeTokenVerifier::new(cache, "peerB");
        let skewed = ack.lease_issued_at + Duration::minutes(10);
        let err = verifier
            .verify_ack(&ack, "file", "download", skewed)
            .await
            .unwrap_err();
        assert!(matches!(err, ResumeTokenError::ClockSkew));
    }
}
