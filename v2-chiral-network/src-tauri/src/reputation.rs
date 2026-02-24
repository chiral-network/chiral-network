use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

// ============================================================================
// VERDICT TYPES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum VerdictOutcome {
    Good,
    Disputed,
    Bad,
}

/// A signed reputation verdict issued by one peer about another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionVerdict {
    /// Peer being rated
    pub target_id: String,
    /// Peer who filed the rating
    pub issuer_id: String,
    pub outcome: VerdictOutcome,
    pub details: Option<String>,
    pub issued_at: u64,
    /// hex-encoded ed25519 signature over the canonical payload
    pub issuer_sig: String,
}

impl TransactionVerdict {
    /// DHT key for a list of verdicts about a specific target peer.
    pub fn dht_key_for_target(target_id: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(target_id.as_bytes());
        hasher.update(b"||tx-rep-v2");
        format!("rep:agg:{}", hex::encode(hasher.finalize()))
    }

    /// Compute the canonical signable payload (excludes issuer_sig).
    fn signable_payload(&self) -> Result<Vec<u8>, String> {
        let payload = serde_json::json!({
            "target_id": self.target_id,
            "issuer_id": self.issuer_id,
            "outcome": self.outcome,
            "details": self.details,
            "issued_at": self.issued_at,
        });
        serde_json::to_vec(&payload).map_err(|e| e.to_string())
    }

    /// Sign this verdict with the given signing key. Sets issuer_id and issuer_sig.
    pub fn sign(&mut self, signing_key: &SigningKey, issuer_id: &str) -> Result<(), String> {
        self.issuer_id = issuer_id.to_string();
        let payload_bytes = self.signable_payload()?;
        let signature = signing_key.sign(&payload_bytes);
        self.issuer_sig = hex::encode(signature.to_bytes());
        Ok(())
    }

    /// Verify the signature using the issuer's verifying key.
    pub fn verify_signature(&self, verifying_key: &VerifyingKey) -> bool {
        let Ok(payload_bytes) = self.signable_payload() else {
            return false;
        };
        let Ok(sig_bytes) = hex::decode(&self.issuer_sig) else {
            return false;
        };
        if sig_bytes.len() != 64 {
            return false;
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&sig_bytes);
        let signature = Signature::from_bytes(&arr);
        verifying_key.verify(&payload_bytes, &signature).is_ok()
    }
}

// ============================================================================
// PEER KEY RECORD — published to DHT so others can verify verdict signatures
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerKeyRecord {
    pub peer_id: String,
    /// hex-encoded ed25519 verifying key (32 bytes → 64 hex chars)
    pub ed25519_public_key: String,
    pub registered_at: u64,
    /// Self-signature proving key ownership
    pub self_sig: String,
}

impl PeerKeyRecord {
    pub fn dht_key(peer_id: &str) -> String {
        format!("rep:pubkey:{}", peer_id)
    }

    pub fn create(peer_id: &str, signing_key: &SigningKey) -> Result<Self, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let public_key_hex = hex::encode(signing_key.verifying_key().to_bytes());

        let mut record = Self {
            peer_id: peer_id.to_string(),
            ed25519_public_key: public_key_hex,
            registered_at: now,
            self_sig: String::new(),
        };

        let payload = serde_json::json!({
            "peer_id": record.peer_id,
            "ed25519_public_key": record.ed25519_public_key,
            "registered_at": record.registered_at,
        });
        let payload_bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
        let signature = signing_key.sign(&payload_bytes);
        record.self_sig = hex::encode(signature.to_bytes());

        Ok(record)
    }

    pub fn to_verifying_key(&self) -> Option<VerifyingKey> {
        ReputationKeyStore::verifying_key_from_hex(&self.ed25519_public_key)
    }
}

// ============================================================================
// KEY STORE — persists the node's ed25519 reputation signing key
// ============================================================================

pub struct ReputationKeyStore {
    signing_key: SigningKey,
}

impl ReputationKeyStore {
    const KEY_FILE_NAME: &'static str = "reputation_signing_key.bin";

    /// Load the key from disk, or generate and persist a new one.
    pub fn generate_or_load() -> Self {
        let key_path = Self::key_path();

        if let Ok(bytes) = std::fs::read(&key_path) {
            if bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                return Self {
                    signing_key: SigningKey::from_bytes(&arr),
                };
            }
        }

        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);

        if let Some(parent) = key_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&key_path, signing_key.to_bytes());

        Self { signing_key }
    }

    fn key_path() -> std::path::PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("chiral-network")
            .join(Self::KEY_FILE_NAME)
    }

    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(self.signing_key.verifying_key().to_bytes())
    }

    pub fn verifying_key_from_hex(hex_str: &str) -> Option<VerifyingKey> {
        let bytes = hex::decode(hex_str).ok()?;
        if bytes.len() != 32 {
            return None;
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        VerifyingKey::from_bytes(&arr).ok()
    }
}

// ============================================================================
// VERIFIED REPUTATION — aggregated score returned to the frontend
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedReputation {
    pub score: f64,
    pub trust_level: String,
    pub total_verdicts: usize,
    pub good_count: usize,
    pub disputed_count: usize,
    pub bad_count: usize,
    /// Verdicts whose ed25519 signature was verified against the issuer's DHT-published key
    pub signature_verified_count: usize,
    /// 0.0–1.0: scales with evidence quantity and signature verification rate
    pub confidence: f64,
}

impl VerifiedReputation {
    pub fn compute(verdicts: &[TransactionVerdict], signature_verified: usize) -> Self {
        let total = verdicts.len();
        let good = verdicts
            .iter()
            .filter(|v| v.outcome == VerdictOutcome::Good)
            .count();
        let bad = verdicts
            .iter()
            .filter(|v| v.outcome == VerdictOutcome::Bad)
            .count();
        let disputed = verdicts
            .iter()
            .filter(|v| v.outcome == VerdictOutcome::Disputed)
            .count();

        let score = if total == 0 {
            0.5
        } else {
            let weighted: f64 = verdicts
                .iter()
                .map(|v| match v.outcome {
                    VerdictOutcome::Good => 1.0_f64,
                    VerdictOutcome::Disputed => 0.5_f64,
                    VerdictOutcome::Bad => 0.0_f64,
                })
                .sum();
            weighted / total as f64
        };

        let trust_level = if total == 0 {
            "unknown"
        } else if score >= 0.8 {
            "trusted"
        } else if score >= 0.6 {
            "high"
        } else if score >= 0.4 {
            "medium"
        } else if score >= 0.2 {
            "low"
        } else {
            "unknown"
        }
        .to_string();

        // Confidence grows with evidence quantity and signature verification rate.
        // Reaches ~0.5 at 5 verdicts, ~0.9 at 20 verdicts.
        let verdict_confidence = (total as f64 / (total as f64 + 5.0)).min(1.0);
        let sig_rate = if total == 0 {
            0.0
        } else {
            signature_verified as f64 / total as f64
        };
        let confidence = verdict_confidence * (0.4 + 0.6 * sig_rate);

        Self {
            score,
            trust_level,
            total_verdicts: total,
            good_count: good,
            disputed_count: disputed,
            bad_count: bad,
            signature_verified_count: signature_verified,
            confidence,
        }
    }

    pub fn unknown() -> Self {
        Self {
            score: 0.5,
            trust_level: "unknown".to_string(),
            total_verdicts: 0,
            good_count: 0,
            disputed_count: 0,
            bad_count: 0,
            signature_verified_count: 0,
            confidence: 0.0,
        }
    }
}

// ============================================================================
// DHT HELPERS — serialize/deserialize verdict lists
// ============================================================================

/// Deserialize a verdict list from a DHT string value (JSON array).
pub fn verdicts_from_dht(value: &str) -> Vec<TransactionVerdict> {
    serde_json::from_str(value).unwrap_or_default()
}

/// Serialize a verdict list to a JSON string for DHT storage.
pub fn verdicts_to_dht(verdicts: &[TransactionVerdict]) -> Result<String, String> {
    serde_json::to_string(verdicts).map_err(|e| e.to_string())
}

// ============================================================================
// REPUTATION DETAILS — combined score + verdicts in one response
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReputationDetails {
    pub score: VerifiedReputation,
    pub verdicts: Vec<TransactionVerdict>,
}
