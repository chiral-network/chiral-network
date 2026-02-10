use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use ethers::prelude::*;
use ethers::signers::Signer as EthSigner;
use rand::rngs::OsRng;
use rs_merkle::{Hasher, MerkleTree};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// Generate contract bindings for the ReputationEpoch contract
// The contract should have these functions:
// - submitEpoch(uint64 epochId, bytes32 merkleRoot, uint64 timestamp, uint256 eventCount)
// - getEpoch(uint64 epochId) returns (bytes32 merkleRoot, uint64 timestamp, uint256 eventCount, address submitter)
// - verifyEventProof(bytes32 eventHash, bytes32[] proof, uint64 epochId) returns (bool)
abigen!(
    ReputationEpochContract,
    r#"[
        function submitEpoch(uint64 epochId, bytes32 merkleRoot, uint64 timestamp, uint256 eventCount) external
        function getEpoch(uint64 epochId) external view returns (bytes32 merkleRoot, uint64 timestamp, uint256 eventCount, address submitter)
        function verifyEventProof(bytes32 eventHash, bytes32[] calldata proof, uint64 epochId) external view returns (bool)
        event EpochSubmitted(uint64 indexed epochId, bytes32 merkleRoot, address submitter)
    ]"#
);

// ============================================================================
// REPUTATION CONFIGURATION
// ============================================================================

/// Default deadline for signed transaction messages (seconds from handshake)
pub const PAYMENT_DEADLINE_DEFAULT: u64 = 3600; // 1 hour

/// Additional wait time after deadline before filing non-payment complaint (seconds)
pub const PAYMENT_GRACE_PERIOD: u64 = 1800; // 30 minutes

/// How long to track used nonces to prevent replay attacks (seconds)
pub const SIGNED_MESSAGE_NONCE_TTL: u64 = 86400; // 24 hours

/// Required balance as multiple of file price (e.g., 1.2 = 120% of price)
pub const MIN_BALANCE_MULTIPLIER: f64 = 1.2;

/// Cryptographic signature scheme for signed transaction messages
pub const SIGNATURE_ALGORITHM: &str = "ed25519";

// ============================================================================
// REPUTATION TYPES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationEvent {
    pub id: String,
    pub peer_id: String,
    pub rater_peer_id: String,
    pub event_type: EventType,
    pub timestamp: u64,
    pub data: serde_json::Value,
    pub impact: f64,
    pub signature: String,
    pub epoch: Option<u64>,
}

/// Transaction verdicts published to DHT to summarize an issuer's view of a
/// particular on-chain transaction involving `target_id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum VerdictOutcome {
    Good,
    Disputed,
    Bad,
}

/// Signed transaction message: downloader's off-chain payment promise
/// This serves as cryptographic proof of payment obligation before file transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTransactionMessage {
    pub from: String,                 // downloader's address
    pub to: String,                   // seeder's address
    pub amount: u64,                  // payment amount in smallest currency unit
    pub file_hash: String,            // target file identifier (merkle_root)
    pub nonce: String,                // unique identifier to prevent replay attacks
    pub deadline: u64,                // unix timestamp - maximum time for transfer completion
    pub downloader_signature: String, // hex-encoded ed25519 signature
}

impl SignedTransactionMessage {
    /// Create and sign a new transaction message
    pub fn new(
        from: String,
        to: String,
        amount: u64,
        file_hash: String,
        deadline: u64,
        signing_key: &SigningKey,
    ) -> Result<Self, String> {
        use uuid::Uuid;

        let nonce = Uuid::new_v4().to_string();

        let mut message = Self {
            from,
            to,
            amount,
            file_hash,
            nonce,
            deadline,
            downloader_signature: String::new(),
        };

        message.sign(signing_key)?;
        Ok(message)
    }

    /// Sign this message using the provided signing key
    pub fn sign(&mut self, signing_key: &SigningKey) -> Result<(), String> {
        let signable = serde_json::json!({
            "from": self.from,
            "to": self.to,
            "amount": self.amount,
            "file_hash": self.file_hash,
            "nonce": self.nonce,
            "deadline": self.deadline,
        });

        let serialized = serde_json::to_vec(&signable).map_err(|e| e.to_string())?;
        let signature = signing_key.sign(&serialized);
        self.downloader_signature = hex::encode(signature.to_bytes());
        Ok(())
    }

    /// Verify the signature on this message
    pub fn verify_signature(&self, verifying_key: &VerifyingKey) -> Result<bool, String> {
        let signable = serde_json::json!({
            "from": self.from,
            "to": self.to,
            "amount": self.amount,
            "file_hash": self.file_hash,
            "nonce": self.nonce,
            "deadline": self.deadline,
        });

        let serialized = serde_json::to_vec(&signable).map_err(|e| e.to_string())?;

        let signature_bytes = hex::decode(&self.downloader_signature).map_err(|e| e.to_string())?;
        if signature_bytes.len() != 64 {
            return Err("invalid signature length".into());
        }
        let mut signature_bytes_array: [u8; 64] = [0u8; 64];
        signature_bytes_array.copy_from_slice(&signature_bytes[..64]);

        let signature = Signature::from_bytes(&signature_bytes_array);
        Ok(verifying_key.verify(&serialized, &signature).is_ok())
    }

    /// Check if deadline has passed
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now > self.deadline
    }

    /// Validate message fields
    pub fn validate(&self) -> Result<(), String> {
        if self.from.is_empty() {
            return Err("from address missing".into());
        }
        if self.to.is_empty() {
            return Err("to address missing".into());
        }
        if self.amount == 0 {
            return Err("amount must be greater than 0".into());
        }
        if self.file_hash.is_empty() {
            return Err("file_hash missing".into());
        }
        if self.nonce.is_empty() {
            return Err("nonce missing".into());
        }
        if self.deadline == 0 {
            return Err("deadline missing".into());
        }
        if self.is_expired() {
            return Err("deadline has already passed".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionVerdict {
    pub target_id: String,
    /// Optional: NULL for non-payment complaints where payment never reached blockchain
    pub tx_hash: Option<String>,
    pub outcome: VerdictOutcome,
    pub details: Option<String>,
    pub metric: Option<String>,
    pub issued_at: u64,
    pub issuer_id: String,
    pub issuer_seq_no: u64,
    /// hex-encoded ed25519 signature over the canonical signable payload
    pub issuer_sig: String,
    /// optional on-chain receipt pointer (compact representation)
    pub tx_receipt: Option<String>,
    /// optional evidence blobs (references or small encoded blobs)
    /// Critical for non-payment complaints: includes signed_transaction_message,
    /// delivery_proof, and protocol logs
    pub evidence_blobs: Option<Vec<String>>,
}

impl TransactionVerdict {
    /// Basic validation performed client-side before accepting a verdict.
    pub fn validate(&self) -> Result<(), String> {
        if self.issuer_id.is_empty() {
            return Err("issuer_id missing".into());
        }
        if self.target_id.is_empty() {
            return Err("target_id missing".into());
        }
        // tx_hash is now optional (NULL for non-payment complaints)
        if self.issuer_id == self.target_id {
            return Err("issuer_id must not equal target_id".into());
        }
        Ok(())
    }

    /// Compute the DHT key for a specific verdict: H(issuer_id || target_id || "tx-rep")
    /// This allows each issuer to store their own verdict about a target
    pub fn dht_key_for_verdict(issuer_id: &str, target_id: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(issuer_id.as_bytes());
        hasher.update(b"||");
        hasher.update(target_id.as_bytes());
        hasher.update(b"||tx-rep");
        hex::encode(hasher.finalize())
    }

    /// Legacy method - kept for backwards compatibility but now generates issuer-specific key
    /// If called without issuer context, falls back to target-only key
    pub fn dht_key_for_target(target_id: &str) -> String {
        println!(
            "[KEY] Computing DHT key for target: '{}' (len={} bytes)",
            target_id,
            target_id.len()
        );
        let mut hasher = Sha256::new();
        hasher.update(target_id.as_bytes());
        hasher.update(b"tx-rep");
        let hash = hex::encode(hasher.finalize());
        println!("[KEY] Computed target-only key: {}", hash);
        hash
    }

    /// Sign this verdict using the provided signing key. This will set
    /// `issuer_id`, `issuer_seq_no`, and `issuer_sig` on the struct.
    pub fn sign_with(
        &mut self,
        signing_key: &SigningKey,
        issuer_id: &str,
        issuer_seq_no: u64,
    ) -> Result<(), String> {
        self.issuer_id = issuer_id.to_string();
        self.issuer_seq_no = issuer_seq_no;

        // Build a deterministic signable payload. Use explicit field order.
        let signable = serde_json::json!({
            "target_id": self.target_id,
            "tx_hash": &self.tx_hash,
            "outcome": &self.outcome,
            "details": self.details,
            "metric": self.metric,
            "issued_at": self.issued_at,
            "issuer_id": self.issuer_id,
            "issuer_seq_no": self.issuer_seq_no,
            "tx_receipt": self.tx_receipt,
            "evidence_blobs": self.evidence_blobs,
        });

        let serialized = serde_json::to_vec(&signable).map_err(|e| e.to_string())?;

        let signature = signing_key.sign(&serialized);
        self.issuer_sig = hex::encode(signature.to_bytes());
        Ok(())
    }

    /// Verify the signature on this verdict using the provided verifying key.
    pub fn verify_signature(&self, verifying_key: &VerifyingKey) -> Result<bool, String> {
        let signable = serde_json::json!({
            "target_id": self.target_id,
            "tx_hash": &self.tx_hash,
            "outcome": &self.outcome,
            "details": self.details,
            "metric": self.metric,
            "issued_at": self.issued_at,
            "issuer_id": self.issuer_id,
            "issuer_seq_no": self.issuer_seq_no,
            "tx_receipt": self.tx_receipt,
            "evidence_blobs": self.evidence_blobs,
        });

        let serialized = serde_json::to_vec(&signable).map_err(|e| e.to_string())?;

        let signature_bytes = hex::decode(&self.issuer_sig).map_err(|e| e.to_string())?;
        if signature_bytes.len() != 64 {
            return Err("invalid signature length".into());
        }
        let mut signature_bytes_array: [u8; 64] = [0u8; 64];
        signature_bytes_array.copy_from_slice(&signature_bytes[..64]);

        // ed25519_dalek in this workspace exposes `Signature::from_bytes` returning
        // a `Signature` directly (not a `Result`), so we construct it and pass to
        // the verifier. If the API returns a `Result` in other versions this
        // would need to be adapted, but this matches the dependency pinned here.
        let signature = Signature::from_bytes(&signature_bytes_array);

        Ok(verifying_key.verify(&serialized, &signature).is_ok())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventType {
    FileTransferSuccess,
    FileTransferFailure,
    PaymentSuccess,
    PaymentFailure,
    ConnectionEstablished,
    ConnectionLost,
    DhtQueryAnswered,
    StorageOffered,
    MaliciousBehaviorReport,
    TorrentChunkSeeded,
    FileShared,
    // Relay server events
    RelayReservationAccepted,
    RelayCircuitEstablished,
    RelayCircuitSuccessful,
    RelayBandwidthProvided,
    RelayUptime,
    RelayRefused,
    RelayTimeout,
    RelayMaliciousBehavior,
}

impl EventType {
    pub fn default_impact(&self) -> f64 {
        match self {
            EventType::FileTransferSuccess => 10.0,
            EventType::FileTransferFailure => -5.0,
            EventType::PaymentSuccess => 15.0,
            EventType::PaymentFailure => -10.0,
            EventType::ConnectionEstablished => 5.0,
            EventType::ConnectionLost => -2.0,
            EventType::DhtQueryAnswered => 3.0,
            EventType::StorageOffered => 8.0,
            EventType::MaliciousBehaviorReport => -50.0,
            EventType::TorrentChunkSeeded => 2.0, // Small reward for each chunk
            EventType::FileShared => 5.0,
            EventType::RelayReservationAccepted => 5.0,
            EventType::RelayCircuitEstablished => 10.0,
            EventType::RelayCircuitSuccessful => 15.0,
            EventType::RelayBandwidthProvided => 1.0,
            EventType::RelayUptime => 5.0,
            EventType::RelayRefused => -2.0,
            EventType::RelayTimeout => -10.0,
            EventType::RelayMaliciousBehavior => -100.0,
        }
    }

    pub fn is_relay_event(&self) -> bool {
        matches!(
            self,
            EventType::RelayReservationAccepted
                | EventType::RelayCircuitEstablished
                | EventType::RelayCircuitSuccessful
                | EventType::RelayBandwidthProvided
                | EventType::RelayUptime
                | EventType::RelayRefused
                | EventType::RelayTimeout
                | EventType::RelayMaliciousBehavior
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationEpoch {
    pub epoch_id: u64,
    pub merkle_root: String,
    pub timestamp: u64,
    pub block_number: Option<u64>,
    pub event_count: usize,
    pub submitter: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    pub leaf_index: usize,
    pub proof_hashes: Vec<String>,
    pub total_leaves: usize,
}

impl ReputationEvent {
    pub fn new(
        id: String,
        peer_id: String,
        rater_peer_id: String,
        event_type: EventType,
        data: serde_json::Value,
        impact: f64,
    ) -> Self {
        Self {
            id,
            peer_id,
            rater_peer_id,
            event_type,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            data,
            impact,
            signature: String::new(),
            epoch: None,
        }
    }
}

// ============================================================================
// MERKLE TREE FOR REPUTATION EVENTS
// ============================================================================

#[derive(Clone)]
pub struct Sha256Hasher;

impl Hasher for Sha256Hasher {
    type Hash = [u8; 32];

    fn hash(data: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::default();
        hasher.update(data);
        hasher.finalize().into()
    }
}

pub struct ReputationMerkleTree {
    events: Vec<ReputationEvent>,
    merkle_tree: MerkleTree<Sha256Hasher>,
}

impl ReputationMerkleTree {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            merkle_tree: MerkleTree::<Sha256Hasher>::new(),
        }
    }

    pub fn add_event(&mut self, event: ReputationEvent) -> Result<(), String> {
        self.events.push(event);
        self.rebuild_tree()?;
        Ok(())
    }

    pub fn get_root(&self) -> Option<[u8; 32]> {
        self.merkle_tree.root()
    }

    pub fn get_root_hex(&self) -> Option<String> {
        self.get_root().map(|root| hex::encode(root))
    }

    pub fn get_events(&self) -> &[ReputationEvent] {
        &self.events
    }

    fn rebuild_tree(&mut self) -> Result<(), String> {
        let event_hashes: Vec<[u8; 32]> = self
            .events
            .iter()
            .map(|event| self.hash_event(event))
            .collect::<Result<Vec<_>, String>>()?;

        self.merkle_tree = MerkleTree::<Sha256Hasher>::from_leaves(&event_hashes);
        Ok(())
    }

    fn hash_event(&self, event: &ReputationEvent) -> Result<[u8; 32], String> {
        let event_data = serde_json::json!({
            "id": event.id,
            "peer_id": event.peer_id,
            "rater_peer_id": event.rater_peer_id,
            "event_type": event.event_type,
            "timestamp": event.timestamp,
            "data": event.data,
            "impact": event.impact,
        });

        let serialized =
            serde_json::to_vec(&event_data).map_err(|e| format!("Serialization error: {}", e))?;

        Ok(Sha256Hasher::hash(&serialized))
    }
}

// ============================================================================
// ED25519 SIGNING AND VERIFICATION
// ============================================================================

pub struct NodeKeyManager {
    signing_key: SigningKey,
    peer_id: String,
}

impl NodeKeyManager {
    pub fn new() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let peer_id = hex::encode(signing_key.verifying_key().to_bytes());

        Self {
            signing_key,
            peer_id,
        }
    }

    pub fn get_peer_id(&self) -> &str {
        &self.peer_id
    }

    pub fn get_verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub fn sign_reputation_event(
        &self,
        mut event: ReputationEvent,
    ) -> Result<ReputationEvent, String> {
        event.rater_peer_id = self.peer_id.clone();

        let signable_data = serde_json::json!({
            "id": event.id,
            "peer_id": event.peer_id,
            "rater_peer_id": event.rater_peer_id,
            "event_type": event.event_type,
            "timestamp": event.timestamp,
            "data": event.data,
            "impact": event.impact,
        });

        let serialized = serde_json::to_vec(&signable_data)
            .map_err(|e| format!("Serialization error: {}", e))?;

        let signature = self.signing_key.sign(&serialized);
        event.signature = hex::encode(signature.to_bytes());

        Ok(event)
    }

    pub fn verify_reputation_event(
        &self,
        event: &ReputationEvent,
        verifying_key: &VerifyingKey,
    ) -> Result<bool, String> {
        let signable_data = serde_json::json!({
            "id": event.id,
            "peer_id": event.peer_id,
            "rater_peer_id": event.rater_peer_id,
            "event_type": event.event_type,
            "timestamp": event.timestamp,
            "data": event.data,
            "impact": event.impact,
        });

        let serialized = serde_json::to_vec(&signable_data)
            .map_err(|e| format!("Serialization error: {}", e))?;

        let signature_bytes = hex::decode(&event.signature)
            .map_err(|e| format!("Invalid signature format: {}", e))?;

        let signature_bytes_array: [u8; 64] = signature_bytes
            .try_into()
            .map_err(|_| "Invalid signature length")?;

        let signature = Signature::from_bytes(&signature_bytes_array);

        Ok(verifying_key.verify(&serialized, &signature).is_ok())
    }
}

pub struct PublicKeyCache {
    cache: HashMap<String, VerifyingKey>,
}

impl PublicKeyCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn add_peer_key(&mut self, peer_id: String, verifying_key: VerifyingKey) {
        self.cache.insert(peer_id, verifying_key);
    }

    pub fn get_peer_key(&self, peer_id: &str) -> Option<&VerifyingKey> {
        self.cache.get(peer_id)
    }
}

// ============================================================================
// DHT STORAGE FOR REPUTATION DATA
// ============================================================================

pub struct ReputationDhtService {
    dht_service: Option<Arc<crate::dht::DhtService>>,
}

impl ReputationDhtService {
    pub fn new() -> Self {
        Self { dht_service: None }
    }

    pub fn set_dht_service(&mut self, dht_service: Arc<crate::dht::DhtService>) {
        self.dht_service = Some(dht_service);
    }

    pub async fn store_reputation_event(&self, event: &ReputationEvent) -> Result<(), String> {
        let dht_service = self
            .dht_service
            .as_ref()
            .ok_or("DHT service not initialized")?;

        // Create a unique key for this reputation event
        let key = format!("reputation:{}:{}", event.peer_id, event.id);

        // Serialize the event
        let serialized =
            serde_json::to_vec(event).map_err(|e| format!("Serialization error: {}", e))?;

        // Store in DHT (using existing file metadata structure as template)
        let metadata = crate::dht::models::FileMetadata {
            merkle_root: key.clone(),
            file_name: format!("reputation_{}.json", event.id),
            file_size: serialized.len() as u64,
            file_data: serialized,
            seeders: vec![event.rater_peer_id.clone()],
            created_at: event.timestamp,
            mime_type: Some("application/json".to_string()),
            is_encrypted: false,
            encryption_method: None,
            key_fingerprint: None,
            parent_hash: None,
            cids: None, // Not needed for reputation events
            encrypted_key_bundle: None,
            is_root: true,
            download_path: None,
            price: 0.0,
            uploader_address: None,
            ftp_sources: None,
            http_sources: None,
            info_hash: None,
            trackers: None,
            ..Default::default()
        };

        dht_service.publish_file(metadata, None).await
    }

    pub async fn retrieve_reputation_events(
        &self,
        peer_id: &str,
    ) -> Result<Vec<ReputationEvent>, String> {
        let dht_service = self
            .dht_service
            .as_ref()
            .ok_or("DHT service not initialized")?;

        // Search for reputation events for this peer
        let search_key = format!("reputation:{}", peer_id);
        
        // Use synchronous search with timeout to get actual results
        let timeout_ms = 5000; // 5 second timeout
        match dht_service.synchronous_search_metadata(search_key.clone(), timeout_ms).await {
            Ok(Some(metadata)) => {
                // Deserialize the reputation events from the file data
                match serde_json::from_slice::<Vec<ReputationEvent>>(&metadata.file_data) {
                    Ok(events) => {
                        tracing::info!(
                            "Retrieved {} reputation events for peer {}",
                            events.len(),
                            peer_id
                        );
                        Ok(events)
                    }
                    Err(e) => {
                        // If deserialization fails, try as a single event
                        match serde_json::from_slice::<ReputationEvent>(&metadata.file_data) {
                            Ok(event) => {
                                tracing::info!("Retrieved single reputation event for peer {}", peer_id);
                                Ok(vec![event])
                            }
                            Err(_) => {
                                tracing::warn!(
                                    "Failed to deserialize reputation data for peer {}: {}",
                                    peer_id,
                                    e
                                );
                                Ok(vec![])
                            }
                        }
                    }
                }
            }
            Ok(None) => {
                // No data found in DHT
                tracing::debug!("No reputation events found for peer {} in DHT", peer_id);
                Ok(vec![])
            }
            Err(e) => {
                tracing::warn!("DHT search failed for peer {}: {}", peer_id, e);
                Ok(vec![])
            }
        }
    }

    /// Store a TransactionVerdict into the DHT for the given target.
    /// Stores under TWO keys:
    /// 1. Issuer+Target key (for querying "verdicts I issued")
    /// 2. Target-only key (for querying "verdicts about this peer")
    pub async fn store_transaction_verdict(
        &self,
        verdict: &TransactionVerdict,
    ) -> Result<(), String> {
        let dht_service = self
            .dht_service
            .as_ref()
            .ok_or("DHT service not initialized")?;

        // Validate before storing
        verdict
            .validate()
            .map_err(|e| format!("Invalid verdict: {}", e))?;

        let serialized =
            serde_json::to_vec(verdict).map_err(|e| format!("Serialization error: {}", e))?;

        let tx_hash_str = verdict
            .tx_hash
            .as_ref()
            .map(|h| h.as_str())
            .unwrap_or("no_tx");

        println!("📊 STORING VERDICT:");
        println!("   Issuer (who wrote this): {}", verdict.issuer_id);
        println!("   Target (who this is about): {}", verdict.target_id);
        println!("   Outcome: {:?}", verdict.outcome);

        // Store under issuer+target key (for "verdicts I issued")
        let issuer_target_key =
            TransactionVerdict::dht_key_for_verdict(&verdict.issuer_id, &verdict.target_id);
        println!(
            "📊 Storing verdict in DHT with issuer+target key: {}",
            issuer_target_key
        );
        println!(
            "📊 Verdict: issuer={}, target={}, outcome={:?}",
            verdict.issuer_id, verdict.target_id, verdict.outcome
        );
        tracing::info!(
            "📊 Storing verdict in DHT with issuer+target key: {}",
            issuer_target_key
        );

        let metadata1 = crate::dht::models::FileMetadata {
            merkle_root: issuer_target_key.clone(),
            file_name: format!("tx_verdict_{}_{}.json", verdict.issuer_id, tx_hash_str),
            file_size: serialized.len() as u64,
            file_data: serialized.clone(),
            seeders: vec![verdict.issuer_id.clone()],
            created_at: verdict.issued_at,
            mime_type: Some("application/json".to_string()),
            is_encrypted: false,
            encryption_method: None,
            key_fingerprint: None,
            parent_hash: None,
            cids: None,
            encrypted_key_bundle: None,
            is_root: true,
            download_path: None,
            price: 0.0,
            uploader_address: None,
            ftp_sources: None,
            ed2k_sources: None,
            http_sources: None,
            info_hash: None,
            trackers: None,
            ..Default::default()
        };
        dht_service.publish_file(metadata1, None).await?;

        // ALSO store under target-only key (for "verdicts about this peer")
        let target_only_key = TransactionVerdict::dht_key_for_target(&verdict.target_id);
        println!(
            "📊 ALSO storing verdict with target-only key: {}",
            target_only_key
        );
        tracing::info!(
            "📊 ALSO storing verdict with target-only key: {}",
            target_only_key
        );

        let metadata2 = crate::dht::models::FileMetadata {
            merkle_root: target_only_key.clone(),
            file_name: format!(
                "tx_verdict_about_{}_{}.json",
                verdict.target_id, tx_hash_str
            ),
            file_size: serialized.len() as u64,
            file_data: serialized,
            seeders: vec![verdict.issuer_id.clone()],
            created_at: verdict.issued_at,
            mime_type: Some("application/json".to_string()),
            is_encrypted: false,
            encryption_method: None,
            key_fingerprint: None,
            parent_hash: None,
            cids: None,
            encrypted_key_bundle: None,
            is_root: true,
            download_path: None,
            price: 0.0,
            uploader_address: None,
            ftp_sources: None,
            ed2k_sources: None,
            http_sources: None,
            info_hash: None,
            trackers: None,
            ..Default::default()
        };
        dht_service.publish_file(metadata2, None).await?;

        println!("✅ Verdict stored successfully under both keys");
        tracing::info!("✅ Verdict stored successfully under both keys");
        Ok(())
    }

    /// Retrieve TransactionVerdict entries for a target by querying the DHT.
    /// Looks up verdicts ABOUT the target peer (where they are the subject of reputation).
    pub async fn retrieve_transaction_verdicts(
        &self,
        target_id: &str,
    ) -> Result<Vec<TransactionVerdict>, String> {
        let dht_service = self
            .dht_service
            .as_ref()
            .ok_or("DHT service not initialized")?;

        println!("🔍 RETRIEVING VERDICTS ABOUT: '{}'", target_id);

        // Use target-only key to find verdicts ABOUT this peer
        let search_key = TransactionVerdict::dht_key_for_target(target_id);

        println!("🔍 Searching for verdicts ABOUT target: {}", target_id);
        println!("🔍 Using DHT key: {}", search_key);
        tracing::info!(
            "🔍 Searching for verdicts ABOUT target: {}, key: {}",
            target_id,
            search_key
        );

        // Use GetDhtValue to retrieve the verdict data directly
        println!("🔍 Calling get_dht_value for key: {}", search_key);
        match dht_service.get_dht_value(search_key.clone()).await {
            Ok(Some(verdict_bytes)) => {
                println!(
                    "✅ Found verdict data, size={} bytes",
                    verdict_bytes.len()
                );
                tracing::info!(
                    "✅ Found verdict data, size={} bytes",
                    verdict_bytes.len()
                );
                // Try to deserialize the verdict data as a TransactionVerdict
                match serde_json::from_slice::<TransactionVerdict>(&verdict_bytes) {
                    Ok(verdict) => {
                        println!(
                            "✅ Deserialized verdict: issuer={}, outcome={:?}",
                            verdict.issuer_id, verdict.outcome
                        );
                        tracing::info!(
                            "✅ Found verdict: issuer={}, outcome={:?}",
                            verdict.issuer_id,
                            verdict.outcome
                        );
                        Ok(vec![verdict])
                    }
                    Err(e) => {
                        println!("❌ Failed to deserialize verdict: {}", e);
                        tracing::warn!("❌ Failed to deserialize verdict: {}", e);
                        Ok(vec![])
                    }
                }
            }
            Ok(None) => {
                println!("❌ No verdicts found about target: {}", target_id);
                tracing::info!("❌ No verdicts found about target: {}", target_id);
                Ok(vec![])
            }
            Err(e) => {
                println!("❌ DHT search failed: {}", e);
                tracing::warn!("❌ DHT search failed: {}", e);
                Ok(vec![]) // Return empty instead of error to not break UI
            }
        }
    }

    pub async fn store_merkle_root(&self, epoch: &ReputationEpoch) -> Result<(), String> {
        let dht_service = self
            .dht_service
            .as_ref()
            .ok_or("DHT service not initialized")?;

        let key = format!("merkle_root:{}", epoch.epoch_id);

        let serialized =
            serde_json::to_vec(epoch).map_err(|e| format!("Serialization error: {}", e))?;

        let metadata = crate::dht::models::FileMetadata {
            merkle_root: epoch.merkle_root.clone(),
            file_name: format!("merkle_root_{}.json", epoch.epoch_id),
            file_size: serialized.len() as u64,
            file_data: serialized,
            seeders: vec![epoch.submitter.clone()],
            created_at: epoch.timestamp,
            mime_type: Some("application/json".to_string()),
            is_encrypted: false,
            encryption_method: None,
            key_fingerprint: None,
            parent_hash: None,
            cids: None, // Not needed for merkle roots
            encrypted_key_bundle: None,
            is_root: true,
            download_path: None,
            price: 0.0,
            uploader_address: None,
            ftp_sources: None,
            http_sources: None,
            info_hash: None,
            trackers: None,
            ..Default::default()
        };

        dht_service.publish_file(metadata, None).await
    }
}

// ============================================================================
// SMART CONTRACT INTEGRATION
// ============================================================================

pub struct ReputationContract {
    contract_address: Option<String>,
    rpc_url: String,
    chain_id: u64,
}

impl ReputationContract {
    pub fn new(chain_id: u64) -> Self {
        Self {
            contract_address: None,
            rpc_url: crate::ethereum::NETWORK_CONFIG.rpc_endpoint.clone(),
            chain_id,
        }
    }

    pub fn with_rpc_url(chain_id: u64, rpc_url: String) -> Self {
        Self {
            contract_address: None,
            rpc_url,
            chain_id,
        }
    }

    pub fn set_contract_address(&mut self, address: String) {
        self.contract_address = Some(address);
    }

    pub fn set_rpc_url(&mut self, rpc_url: String) {
        self.rpc_url = rpc_url;
    }

    /// Get a provider connected to the Ethereum network
    fn get_provider(&self) -> Result<Provider<Http>, String> {
        Provider::<Http>::try_from(&self.rpc_url)
            .map_err(|e| format!("Failed to connect to Ethereum RPC: {}", e))
    }

    /// Get the contract instance
    fn get_contract<M: Middleware>(&self, client: Arc<M>) -> Result<ReputationEpochContract<M>, String> {
        let address = self.contract_address.as_ref()
            .ok_or("Contract address not set")?;
        
        let contract_address: Address = address.parse()
            .map_err(|e| format!("Invalid contract address: {}", e))?;
        
        Ok(ReputationEpochContract::new(contract_address, client))
    }

    /// Submit a reputation epoch to the blockchain
    pub async fn submit_epoch(
        &self,
        epoch: &ReputationEpoch,
        private_key: &str,
    ) -> Result<String, String> {
        // Parse and validate private key
        let private_key_clean = private_key.strip_prefix("0x").unwrap_or(private_key);
        let wallet: LocalWallet = private_key_clean.parse()
            .map_err(|e| format!("Invalid private key: {}", e))?;
        let wallet = wallet.with_chain_id(self.chain_id);

        // Connect to provider
        let provider = self.get_provider()?;
        let client = Arc::new(SignerMiddleware::new(provider, wallet));

        // Get contract instance
        let contract = self.get_contract(client)?;

        // Convert merkle root to bytes32
        let merkle_root_bytes = hex::decode(epoch.merkle_root.strip_prefix("0x").unwrap_or(&epoch.merkle_root))
            .map_err(|e| format!("Invalid merkle root hex: {}", e))?;
        if merkle_root_bytes.len() != 32 {
            return Err("Merkle root must be 32 bytes".to_string());
        }
        let mut merkle_root: [u8; 32] = [0u8; 32];
        merkle_root.copy_from_slice(&merkle_root_bytes);

        // Call submitEpoch on the contract
        let tx = contract.submit_epoch(
            epoch.epoch_id,
            merkle_root,
            epoch.timestamp,
            U256::from(epoch.event_count),
        );

        let pending_tx = tx.send().await
            .map_err(|e| format!("Failed to send transaction: {}", e))?;

        let tx_hash = format!("{:?}", pending_tx.tx_hash());

        // Wait for confirmation (optional - can be removed for async behavior)
        let _receipt = pending_tx.await
            .map_err(|e| format!("Transaction failed: {}", e))?;

        Ok(tx_hash)
    }

    /// Get a reputation epoch from the blockchain
    pub async fn get_epoch(&self, epoch_id: u64) -> Result<Option<ReputationEpoch>, String> {
        // Connect to provider
        let provider = self.get_provider()?;
        let client = Arc::new(provider);

        // Get contract instance
        let contract = self.get_contract(client)?;

        // Call getEpoch on the contract
        let result = contract.get_epoch(epoch_id).call().await;

        match result {
            Ok((merkle_root, timestamp, event_count, submitter)) => {
                // Check if epoch exists (merkle_root is not zero)
                if merkle_root == [0u8; 32] {
                    return Ok(None);
                }

                Ok(Some(ReputationEpoch {
                    epoch_id,
                    merkle_root: format!("0x{}", hex::encode(merkle_root)),
                    timestamp,
                    block_number: None, // Could be fetched separately if needed
                    event_count: event_count.as_usize(),
                    submitter: format!("{:?}", submitter),
                }))
            }
            Err(e) => {
                // If the call reverts or epoch doesn't exist, return None
                if e.to_string().contains("revert") || e.to_string().contains("not found") {
                    Ok(None)
                } else {
                    Err(format!("Failed to get epoch: {}", e))
                }
            }
        }
    }

    /// Verify a Merkle proof for an event on the blockchain
    pub async fn verify_event_proof(
        &self,
        event_hash: &str,
        proof: Vec<String>,
        epoch_id: u64,
    ) -> Result<bool, String> {
        // Connect to provider
        let provider = self.get_provider()?;
        let client = Arc::new(provider);

        // Get contract instance
        let contract = self.get_contract(client)?;

        // Convert event hash to bytes32
        let event_hash_bytes = hex::decode(event_hash.strip_prefix("0x").unwrap_or(event_hash))
            .map_err(|e| format!("Invalid event hash hex: {}", e))?;
        if event_hash_bytes.len() != 32 {
            return Err("Event hash must be 32 bytes".to_string());
        }
        let mut event_hash_arr: [u8; 32] = [0u8; 32];
        event_hash_arr.copy_from_slice(&event_hash_bytes);

        // Convert proof to Vec<[u8; 32]>
        let proof_bytes: Result<Vec<[u8; 32]>, String> = proof.iter()
            .map(|hash| {
                let bytes = hex::decode(hash.strip_prefix("0x").unwrap_or(hash))
                    .map_err(|e| format!("Invalid proof hash hex: {}", e))?;
                if bytes.len() != 32 {
                    return Err("Proof hash must be 32 bytes".to_string());
                }
                let mut arr: [u8; 32] = [0u8; 32];
                arr.copy_from_slice(&bytes);
                Ok(arr)
            })
            .collect();
        let proof_bytes = proof_bytes?;

        // Call verifyEventProof on the contract
        let result = contract.verify_event_proof(event_hash_arr, proof_bytes, epoch_id)
            .call()
            .await
            .map_err(|e| format!("Failed to verify event proof: {}", e))?;

        Ok(result)
    }

    pub fn get_contract_address(&self) -> Option<&String> {
        self.contract_address.as_ref()
    }

    pub fn get_chain_id(&self) -> u64 {
        self.chain_id
    }

    pub fn get_rpc_url(&self) -> &str {
        &self.rpc_url
    }
}

pub struct ReputationSystem {
    merkle_tree: ReputationMerkleTree,
    dht_service: ReputationDhtService,
    contract: ReputationContract,
    key_manager: NodeKeyManager,
    _key_cache: PublicKeyCache,
    current_epoch: u64,
}

impl ReputationSystem {
    pub fn new(network_id: u64) -> Self {
        Self {
            merkle_tree: ReputationMerkleTree::new(),
            dht_service: ReputationDhtService::new(),
            contract: ReputationContract::new(network_id),
            key_manager: NodeKeyManager::new(),
            _key_cache: PublicKeyCache::new(),
            current_epoch: 0,
        }
    }

    pub fn set_dht_service(&mut self, dht_service: Arc<crate::dht::DhtService>) {
        self.dht_service.set_dht_service(dht_service);
    }

    pub fn set_contract_address(&mut self, address: String) {
        self.contract.set_contract_address(address);
    }

    pub async fn add_reputation_event(&mut self, mut event: ReputationEvent) -> Result<(), String> {
        // Sign the event
        event = self.key_manager.sign_reputation_event(event)?;

        // Add to Merkle tree
        self.merkle_tree.add_event(event.clone())?;

        // Store in DHT
        self.dht_service.store_reputation_event(&event).await?;

        Ok(())
    }

    pub async fn finalize_epoch(&mut self) -> Result<String, String> {
        let epoch_id = self.current_epoch;
        let merkle_root = self
            .merkle_tree
            .get_root_hex()
            .ok_or("No events in current epoch")?;
        let event_count = self.merkle_tree.get_events().len();

        let epoch = ReputationEpoch {
            epoch_id,
            merkle_root,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            block_number: None,
            event_count,
            submitter: self.key_manager.get_peer_id().to_string(),
        };

        // Store epoch in DHT
        self.dht_service.store_merkle_root(&epoch).await?;

        // Submit to smart contract
        let private_key = "mock_private_key"; // In real implementation, get from secure storage
        let tx_hash = self.contract.submit_epoch(&epoch, private_key).await?;

        // Reset for next epoch
        self.merkle_tree = ReputationMerkleTree::new();
        self.current_epoch += 1;

        Ok(tx_hash)
    }
}

// ============================================================================
// EPOCH MANAGEMENT AND ANCHORING
// ============================================================================

pub struct EpochManager {
    current_epoch: u64,
    epoch_duration_seconds: u64,
    max_events_per_epoch: usize,
    last_epoch_time: u64,
    auto_anchor_enabled: bool,
}

impl EpochManager {
    pub fn new(epoch_duration_seconds: u64, max_events_per_epoch: usize) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            current_epoch: 0,
            epoch_duration_seconds,
            max_events_per_epoch,
            last_epoch_time: now,
            auto_anchor_enabled: true,
        }
    }

    pub fn should_finalize_epoch(&self, event_count: usize) -> bool {
        if !self.auto_anchor_enabled {
            return false;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Finalize if time limit reached
        let time_elapsed = now - self.last_epoch_time;
        if time_elapsed >= self.epoch_duration_seconds {
            return true;
        }

        // Finalize if event count limit reached
        if event_count >= self.max_events_per_epoch {
            return true;
        }

        false
    }

    pub fn get_current_epoch(&self) -> u64 {
        self.current_epoch
    }

    pub fn advance_epoch(&mut self) {
        self.current_epoch += 1;
        self.last_epoch_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
    }

    pub fn set_auto_anchor(&mut self, enabled: bool) {
        self.auto_anchor_enabled = enabled;
    }

    pub fn get_epoch_info(&self) -> (u64, u64, usize, bool) {
        (
            self.current_epoch,
            self.epoch_duration_seconds,
            self.max_events_per_epoch,
            self.auto_anchor_enabled,
        )
    }

    pub fn get_time_until_next_epoch(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let elapsed = now - self.last_epoch_time;
        if elapsed >= self.epoch_duration_seconds {
            0
        } else {
            self.epoch_duration_seconds - elapsed
        }
    }
}

pub struct ReputationSystemWithEpochs {
    merkle_tree: ReputationMerkleTree,
    dht_service: ReputationDhtService,
    contract: ReputationContract,
    key_manager: NodeKeyManager,
    epoch_manager: EpochManager,
    pending_events: Vec<ReputationEvent>,
}

impl ReputationSystemWithEpochs {
    pub fn new(network_id: u64, epoch_duration_seconds: u64, max_events_per_epoch: usize) -> Self {
        Self {
            merkle_tree: ReputationMerkleTree::new(),
            dht_service: ReputationDhtService::new(),
            contract: ReputationContract::new(network_id),
            key_manager: NodeKeyManager::new(),
            epoch_manager: EpochManager::new(epoch_duration_seconds, max_events_per_epoch),
            pending_events: Vec::new(),
        }
    }

    pub fn set_dht_service(&mut self, dht_service: Arc<crate::dht::DhtService>) {
        self.dht_service.set_dht_service(dht_service);
    }

    pub fn set_contract_address(&mut self, address: String) {
        self.contract.set_contract_address(address);
    }

    pub async fn add_reputation_event(
        &mut self,
        mut event: ReputationEvent,
    ) -> Result<Option<String>, String> {
        // Set epoch for the event
        event.epoch = Some(self.epoch_manager.get_current_epoch());

        // Sign the event
        event = self.key_manager.sign_reputation_event(event)?;

        // Add to pending events
        self.pending_events.push(event.clone());

        // Add to Merkle tree
        self.merkle_tree.add_event(event.clone())?;

        // Store in DHT
        self.dht_service.store_reputation_event(&event).await?;

        // Check if epoch should be finalized
        if self
            .epoch_manager
            .should_finalize_epoch(self.pending_events.len())
        {
            let tx_hash = self.finalize_current_epoch().await?;
            return Ok(Some(tx_hash));
        }

        Ok(None)
    }

    pub async fn finalize_current_epoch(&mut self) -> Result<String, String> {
        if self.pending_events.is_empty() {
            return Err("No events to finalize".to_string());
        }

        let epoch_id = self.epoch_manager.get_current_epoch();
        let merkle_root = self
            .merkle_tree
            .get_root_hex()
            .ok_or("Failed to get Merkle root")?;
        let event_count = self.pending_events.len();

        let epoch = ReputationEpoch {
            epoch_id,
            merkle_root,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            block_number: None,
            event_count,
            submitter: self.key_manager.get_peer_id().to_string(),
        };

        // Store epoch in DHT
        self.dht_service.store_merkle_root(&epoch).await?;

        // Submit to smart contract
        let private_key = "mock_private_key";
        let tx_hash = self.contract.submit_epoch(&epoch, private_key).await?;

        // Reset for next epoch
        self.merkle_tree = ReputationMerkleTree::new();
        self.pending_events.clear();
        self.epoch_manager.advance_epoch();

        Ok(tx_hash)
    }

    pub fn get_epoch_status(&self) -> (u64, usize, u64, bool) {
        (
            self.epoch_manager.get_current_epoch(),
            self.pending_events.len(),
            self.epoch_manager.get_time_until_next_epoch(),
            self.epoch_manager.auto_anchor_enabled,
        )
    }

    pub fn set_auto_anchor(&mut self, enabled: bool) {
        self.epoch_manager.set_auto_anchor(enabled);
    }

    pub fn get_pending_events(&self) -> &[ReputationEvent] {
        &self.pending_events
    }
}

// ============================================================================
// VERIFICATION AND TESTING
// ============================================================================

pub struct ReputationVerifier {
    key_cache: PublicKeyCache,
}

impl ReputationVerifier {
    pub fn new() -> Self {
        Self {
            key_cache: PublicKeyCache::new(),
        }
    }

    pub fn add_peer_key(&mut self, peer_id: String, verifying_key: VerifyingKey) {
        self.key_cache.add_peer_key(peer_id, verifying_key);
    }

    pub fn verify_event_signature(&self, event: &ReputationEvent) -> Result<bool, String> {
        if let Some(verifying_key) = self.key_cache.get_peer_key(&event.rater_peer_id) {
            // Create a temporary key manager for verification
            let temp_manager = NodeKeyManager::new();
            temp_manager.verify_reputation_event(event, verifying_key)
        } else {
            Err(format!(
                "Public key not found for peer: {}",
                event.rater_peer_id
            ))
        }
    }

    pub fn verify_event_against_merkle_root(
        &self,
        event: &ReputationEvent,
        merkle_root: &str,
        proof: &MerkleProof,
    ) -> Result<bool, String> {
        // 1. Hash the event to get the leaf hash
        let leaf_hash = self.hash_event(event)?;
        
        // 2. Parse the merkle root from hex
        let root_bytes = hex::decode(merkle_root)
            .map_err(|e| format!("Invalid merkle root hex: {}", e))?;
        if root_bytes.len() != 32 {
            return Err("Merkle root must be 32 bytes".to_string());
        }
        let mut expected_root = [0u8; 32];
        expected_root.copy_from_slice(&root_bytes);
        
        // 3. Walk through the proof using the leaf index to determine ordering
        // At each level: if index is even, we're on the left; if odd, we're on the right
        let mut current_hash = leaf_hash;
        let mut index = proof.leaf_index;
        
        for proof_hash_hex in &proof.proof_hashes {
            let sibling_bytes = hex::decode(proof_hash_hex)
                .map_err(|e| format!("Invalid proof hash hex: {}", e))?;
            if sibling_bytes.len() != 32 {
                return Err("Proof hash must be 32 bytes".to_string());
            }
            
            let mut sibling: [u8; 32] = [0u8; 32];
            sibling.copy_from_slice(&sibling_bytes);
            
            // Combine hashes based on position
            let mut combined = Vec::with_capacity(64);
            if index % 2 == 0 {
                // Current node is on the left
                combined.extend_from_slice(&current_hash);
                combined.extend_from_slice(&sibling);
            } else {
                // Current node is on the right
                combined.extend_from_slice(&sibling);
                combined.extend_from_slice(&current_hash);
            }
            
            current_hash = Sha256Hasher::hash(&combined);
            index /= 2; // Move to parent level
        }
        
        // 4. Compare final hash with merkle root
        Ok(current_hash == expected_root)
    }
    
    /// Hash a reputation event for Merkle tree inclusion
    fn hash_event(&self, event: &ReputationEvent) -> Result<[u8; 32], String> {
        let event_data = serde_json::json!({
            "id": event.id,
            "peer_id": event.peer_id,
            "rater_peer_id": event.rater_peer_id,
            "event_type": event.event_type,
            "timestamp": event.timestamp,
            "data": event.data,
            "impact": event.impact,
        });

        let serialized =
            serde_json::to_vec(&event_data).map_err(|e| format!("Serialization error: {}", e))?;

        Ok(Sha256Hasher::hash(&serialized))
    }

    pub fn verify_epoch_integrity(
        &self,
        epoch: &ReputationEpoch,
        events: &[ReputationEvent],
    ) -> Result<bool, String> {
        // Verify that all events belong to this epoch
        for event in events {
            if event.epoch != Some(epoch.epoch_id) {
                return Ok(false);
            }
        }

        // Verify event count matches
        if events.len() != epoch.event_count {
            return Ok(false);
        }

        // In a real implementation, would also verify Merkle root
        Ok(true)
    }
}

pub struct ReputationTestSuite {
    verifier: ReputationVerifier,
}

impl ReputationTestSuite {
    pub fn new() -> Self {
        Self {
            verifier: ReputationVerifier::new(),
        }
    }

    pub async fn run_integration_test(&mut self) -> Result<TestResults, String> {
        let mut results = TestResults::new();

        // Test 1: Create reputation system with epochs
        let mut system = ReputationSystemWithEpochs::new(98765, 60, 5); // 1 minute, 5 events max
        results.add_test(
            "System Creation",
            true,
            "ReputationSystemWithEpochs created successfully",
        );

        // Test 2: Add reputation events
        let event1 = ReputationEvent::new(
            "test-event-1".to_string(),
            "peer-1".to_string(),
            "rater-1".to_string(),
            EventType::FileTransferSuccess,
            serde_json::json!({"test": "data1"}),
            0.5,
        );

        let event2 = ReputationEvent::new(
            "test-event-2".to_string(),
            "peer-2".to_string(),
            "rater-2".to_string(),
            EventType::FileTransferFailure,
            serde_json::json!({"test": "data2"}),
            -0.3,
        );

        let result1 = system.add_reputation_event(event1.clone()).await;
        let result2 = system.add_reputation_event(event2.clone()).await;

        results.add_test(
            "Add Events",
            result1.is_ok() && result2.is_ok(),
            "Events added successfully",
        );

        // Test 3: Verify events are pending
        let pending_count = system.get_pending_events().len();
        results.add_test(
            "Pending Events",
            pending_count == 2,
            &format!("Expected 2 pending events, got {}", pending_count),
        );

        // Test 4: Verify event signatures
        let events = system.get_pending_events();
        let mut signature_verified = true;
        for event in events {
            if let Err(_) = self.verifier.verify_event_signature(event) {
                signature_verified = false;
                break;
            }
        }
        results.add_test(
            "Signature Verification",
            signature_verified,
            "All event signatures verified",
        );

        // Test 5: Finalize epoch
        let finalize_result = system.finalize_current_epoch().await;
        results.add_test(
            "Epoch Finalization",
            finalize_result.is_ok(),
            "Epoch finalized successfully",
        );

        // Test 6: Verify epoch advancement
        let (current_epoch, pending, _, _) = system.get_epoch_status();
        results.add_test(
            "Epoch Advancement",
            current_epoch == 1 && pending == 0,
            "Epoch advanced and cleared",
        );

        Ok(results)
    }

    pub async fn run_performance_test(
        &mut self,
        event_count: usize,
    ) -> Result<PerformanceResults, String> {
        let start_time = SystemTime::now();

        // Create system
        let mut system = ReputationSystemWithEpochs::new(98765, 3600, event_count + 1);

        // Add events
        let mut events = Vec::new();
        for i in 0..event_count {
            let event = ReputationEvent::new(
                format!("perf-event-{}", i),
                format!("peer-{}", i % 10), // Distribute across 10 peers
                "test-rater".to_string(),
                EventType::FileTransferSuccess,
                serde_json::json!({"test": format!("data{}", i)}),
                0.1,
            );
            events.push(event);
        }

        let add_start = SystemTime::now();
        for event in events {
            system.add_reputation_event(event).await?;
        }
        let add_duration = add_start.elapsed().unwrap_or_default();

        // Finalize epoch
        let finalize_start = SystemTime::now();
        system.finalize_current_epoch().await?;
        let finalize_duration = finalize_start.elapsed().unwrap_or_default();

        let total_duration = start_time.elapsed().unwrap_or_default();

        Ok(PerformanceResults {
            event_count,
            add_duration_ms: add_duration.as_millis() as u64,
            finalize_duration_ms: finalize_duration.as_millis() as u64,
            total_duration_ms: total_duration.as_millis() as u64,
            events_per_second: (event_count as f64 / total_duration.as_secs_f64()) as u64,
        })
    }
}

#[derive(Debug)]
pub struct TestResults {
    pub tests: Vec<TestResult>,
    pub passed: usize,
    pub failed: usize,
}

#[derive(Debug)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Debug)]
pub struct PerformanceResults {
    pub event_count: usize,
    pub add_duration_ms: u64,
    pub finalize_duration_ms: u64,
    pub total_duration_ms: u64,
    pub events_per_second: u64,
}

impl TestResults {
    pub fn new() -> Self {
        Self {
            tests: Vec::new(),
            passed: 0,
            failed: 0,
        }
    }

    pub fn add_test(&mut self, name: &str, passed: bool, message: &str) {
        self.tests.push(TestResult {
            name: name.to_string(),
            passed,
            message: message.to_string(),
        });

        if passed {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
    }

    pub fn is_success(&self) -> bool {
        self.failed == 0
    }
}

// reputation score calc with time decay

/// config for rep score calc
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepScoreCfg {
    pub half_life_secs: u64,      // decay half-life in seconds
    pub grace_period_secs: u64,   // no decay within this window
    pub base_score: f64,
    pub min_score: f64,
    pub max_score: f64,
    pub pos_weight: f64,          // weight for positive events
    pub neg_weight: f64,          // weight for negative events
    pub max_events: usize,
    pub min_events_confident: usize,
}

impl Default for RepScoreCfg {
    fn default() -> Self {
        Self {
            half_life_secs: 7 * 24 * 3600, // 7 days
            grace_period_secs: 3600,       // 1 hour
            base_score: 0.5,
            min_score: 0.0,
            max_score: 1.0,
            pos_weight: 1.0,
            neg_weight: 1.5,               // negative events hurt more
            max_events: 1000,
            min_events_confident: 5,
        }
    }
}

/// result from rep score calc
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepScoreResult {
    pub score: f64,
    pub event_cnt: usize,
    pub pos_sum: f64,
    pub neg_sum: f64,
    pub confident: bool,
    pub oldest_ts: Option<u64>,
    pub newest_ts: Option<u64>,
    pub avg_decay: f64,
}

/// calculator for rep scores with time decay
pub struct RepScoreCalc {
    cfg: RepScoreCfg,
}

impl RepScoreCalc {
    pub fn new() -> Self {
        Self { cfg: RepScoreCfg::default() }
    }

    pub fn with_cfg(cfg: RepScoreCfg) -> Self {
        Self { cfg }
    }

    pub fn cfg(&self) -> &RepScoreCfg {
        &self.cfg
    }

    pub fn set_cfg(&mut self, cfg: RepScoreCfg) {
        self.cfg = cfg;
    }

    // exponential decay: 2^(-age/half_life)
    pub fn calc_decay(&self, event_ts: u64, cur_ts: u64) -> f64 {
        if event_ts >= cur_ts {
            return 1.0;
        }

        let age = cur_ts - event_ts;
        if age <= self.cfg.grace_period_secs {
            return 1.0;
        }

        let effective_age = age - self.cfg.grace_period_secs;
        let half_life = self.cfg.half_life_secs as f64;
        let exp = -(effective_age as f64) / half_life;

        (exp * std::f64::consts::LN_2).exp().clamp(0.0, 1.0)
    }

    pub fn calc_weighted_impact(&self, event: &ReputationEvent, cur_ts: u64) -> f64 {
        let decay = self.calc_decay(event.timestamp, cur_ts);
        let weight = if event.impact >= 0.0 {
            self.cfg.pos_weight
        } else {
            self.cfg.neg_weight
        };
        event.impact * weight * decay
    }

    pub fn calc_score(&self, peer_id: &str, events: &[ReputationEvent]) -> RepScoreResult {
        let cur_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.calc_score_at(peer_id, events, cur_ts)
    }

    pub fn calc_score_at(
        &self,
        peer_id: &str,
        events: &[ReputationEvent],
        cur_ts: u64,
    ) -> RepScoreResult {
        let mut peer_events: Vec<&ReputationEvent> = events
            .iter()
            .filter(|e| e.peer_id == peer_id)
            .collect();

        // newest first
        peer_events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        if peer_events.len() > self.cfg.max_events {
            peer_events.truncate(self.cfg.max_events);
        }

        if peer_events.is_empty() {
            return RepScoreResult {
                score: self.cfg.base_score,
                event_cnt: 0,
                pos_sum: 0.0,
                neg_sum: 0.0,
                confident: false,
                oldest_ts: None,
                newest_ts: None,
                avg_decay: 1.0,
            };
        }

        let mut pos_sum = 0.0;
        let mut neg_sum = 0.0;
        let mut decay_sum = 0.0;

        for event in &peer_events {
            let impact = self.calc_weighted_impact(event, cur_ts);
            let decay = self.calc_decay(event.timestamp, cur_ts);
            decay_sum += decay;

            if impact >= 0.0 {
                pos_sum += impact;
            } else {
                neg_sum += impact.abs();
            }
        }

        // tanh keeps score bounded
        let total = pos_sum - neg_sum;
        let adj = (total / 100.0).tanh() * 0.5;
        let score = (self.cfg.base_score + adj).clamp(self.cfg.min_score, self.cfg.max_score);

        let cnt = peer_events.len();
        let avg_decay = if cnt > 0 { decay_sum / cnt as f64 } else { 1.0 };

        RepScoreResult {
            score,
            event_cnt: cnt,
            pos_sum,
            neg_sum,
            confident: cnt >= self.cfg.min_events_confident,
            oldest_ts: peer_events.last().map(|e| e.timestamp),
            newest_ts: peer_events.first().map(|e| e.timestamp),
            avg_decay,
        }
    }

    pub fn calc_batch(
        &self,
        peer_ids: &[String],
        events: &[ReputationEvent],
    ) -> HashMap<String, RepScoreResult> {
        let cur_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        peer_ids
            .iter()
            .map(|id| (id.clone(), self.calc_score_at(id, events, cur_ts)))
            .collect()
    }

    pub fn extract_peer_ids(events: &[ReputationEvent]) -> Vec<String> {
        let mut ids: Vec<String> = events.iter().map(|e| e.peer_id.clone()).collect();
        ids.sort();
        ids.dedup();
        ids
    }

    pub fn calc_all(&self, events: &[ReputationEvent]) -> HashMap<String, RepScoreResult> {
        let ids = Self::extract_peer_ids(events);
        self.calc_batch(&ids, events)
    }
}

impl Default for RepScoreCalc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reputation_event_creation() {
        let event = ReputationEvent::new(
            "test-event-1".to_string(),
            "target-peer".to_string(),
            "rater-peer".to_string(),
            EventType::FileTransferSuccess,
            serde_json::json!({"test": "data"}),
            0.5,
        );

        assert_eq!(event.peer_id, "target-peer");
        assert_eq!(event.rater_peer_id, "rater-peer");
        assert_eq!(event.event_type, EventType::FileTransferSuccess);
        assert_eq!(event.impact, 0.5);
        assert!(event.signature.is_empty()); // Should be empty initially
    }

    #[test]
    fn test_merkle_tree_operations() {
        let mut tree = ReputationMerkleTree::new();

        let event1 = ReputationEvent::new(
            "event-1".to_string(),
            "peer-1".to_string(),
            "rater-1".to_string(),
            EventType::FileTransferSuccess,
            serde_json::json!({"test": "data1"}),
            0.5,
        );

        let event2 = ReputationEvent::new(
            "event-2".to_string(),
            "peer-2".to_string(),
            "rater-2".to_string(),
            EventType::FileTransferFailure,
            serde_json::json!({"test": "data2"}),
            -0.3,
        );

        // Add events
        tree.add_event(event1.clone()).unwrap();
        tree.add_event(event2.clone()).unwrap();

        // Check root exists
        assert!(tree.get_root().is_some());
        assert!(tree.get_root_hex().is_some());

        // Check events are stored
        assert_eq!(tree.get_events().len(), 2);
        assert_eq!(tree.get_events()[0].id, "event-1");
        assert_eq!(tree.get_events()[1].id, "event-2");
    }

    #[test]
    fn test_event_type_equality() {
        let event_type1 = EventType::FileTransferSuccess;
        let event_type2 = EventType::FileTransferSuccess;
        let event_type3 = EventType::FileTransferFailure;

        assert_eq!(event_type1, event_type2);
        assert_ne!(event_type1, event_type3);
    }

    #[test]
    fn test_ed25519_signing_and_verification() {
        let key_manager = NodeKeyManager::new();
        let peer_id = key_manager.get_peer_id().to_string();

        let event = ReputationEvent::new(
            "test-event".to_string(),
            "target-peer".to_string(),
            "rater-peer".to_string(),
            EventType::FileTransferSuccess,
            serde_json::json!({"test": "data"}),
            0.5,
        );

        // Sign the event
        let signed_event = key_manager.sign_reputation_event(event).unwrap();
        assert!(!signed_event.signature.is_empty());
        assert_eq!(signed_event.rater_peer_id, peer_id);

        // Verify the event
        let verifying_key = key_manager.get_verifying_key();
        let is_valid = key_manager
            .verify_reputation_event(&signed_event, &verifying_key)
            .unwrap();
        assert!(is_valid);
    }

    #[test]
    fn test_public_key_cache() {
        let mut cache = PublicKeyCache::new();
        let key_manager = NodeKeyManager::new();
        let peer_id = key_manager.get_peer_id().to_string();
        let verifying_key = key_manager.get_verifying_key();

        // Add peer key
        cache.add_peer_key(peer_id.clone(), verifying_key);

        // Retrieve peer key
        let retrieved_key = cache.get_peer_key(&peer_id);
        assert!(retrieved_key.is_some());

        // Test with non-existent peer
        let non_existent = cache.get_peer_key("non-existent");
        assert!(non_existent.is_none());
    }

    #[test]
    fn test_reputation_dht_service_creation() {
        let dht_service = ReputationDhtService::new();
        assert!(dht_service.dht_service.is_none());
    }

    #[test]
    fn test_reputation_dht_service_without_dht() {
        let dht_service = ReputationDhtService::new();
        let event = ReputationEvent::new(
            "test-event".to_string(),
            "target-peer".to_string(),
            "rater-peer".to_string(),
            EventType::FileTransferSuccess,
            serde_json::json!({"test": "data"}),
            0.5,
        );

        // This should fail because DHT service is not initialized
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(dht_service.store_reputation_event(&event));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("DHT service not initialized"));
    }

    #[test]
    fn test_reputation_contract_creation() {
        let contract = ReputationContract::new(98765);
        assert_eq!(contract.get_chain_id(), 98765);
        assert!(contract.get_contract_address().is_none());
    }

    #[test]
    fn test_reputation_contract_address() {
        let mut contract = ReputationContract::new(98765);
        contract.set_contract_address("0x1234567890abcdef".to_string());
        assert_eq!(
            contract.get_contract_address().unwrap(),
            "0x1234567890abcdef"
        );
    }

    #[test]
    fn test_reputation_system_creation() {
        let system = ReputationSystem::new(98765);
        assert_eq!(system.contract.get_chain_id(), 98765);
        assert_eq!(system.current_epoch, 0);
    }

    #[test]
    fn test_reputation_system_contract_address() {
        let mut system = ReputationSystem::new(98765);
        system.set_contract_address("0xabcdef1234567890".to_string());
        assert_eq!(
            system.contract.get_contract_address().unwrap(),
            "0xabcdef1234567890"
        );
    }

    #[test]
    fn test_epoch_manager_creation() {
        let manager = EpochManager::new(3600, 100); // 1 hour, 100 events max
        let (current_epoch, duration, max_events, auto_anchor) = manager.get_epoch_info();

        assert_eq!(current_epoch, 0);
        assert_eq!(duration, 3600);
        assert_eq!(max_events, 100);
        assert!(auto_anchor);
    }

    #[test]
    fn test_epoch_manager_should_finalize() {
        let manager = EpochManager::new(1, 5); // 1 second, 5 events max

        // Should not finalize with few events
        assert!(!manager.should_finalize_epoch(3));

        // Should finalize when event count limit reached
        assert!(manager.should_finalize_epoch(5));
        assert!(manager.should_finalize_epoch(10));
    }

    #[test]
    fn test_epoch_manager_advance() {
        let mut manager = EpochManager::new(3600, 100);
        assert_eq!(manager.get_current_epoch(), 0);

        manager.advance_epoch();
        assert_eq!(manager.get_current_epoch(), 1);

        manager.advance_epoch();
        assert_eq!(manager.get_current_epoch(), 2);
    }

    #[test]
    fn test_epoch_manager_auto_anchor() {
        let mut manager = EpochManager::new(3600, 100);
        assert!(manager.auto_anchor_enabled);

        manager.set_auto_anchor(false);
        assert!(!manager.auto_anchor_enabled);

        // Should not finalize when auto-anchor is disabled
        assert!(!manager.should_finalize_epoch(1000));
    }

    #[test]
    fn test_reputation_system_with_epochs_creation() {
        let system = ReputationSystemWithEpochs::new(98765, 3600, 100);
        let (epoch, pending, time_left, auto_anchor) = system.get_epoch_status();

        assert_eq!(epoch, 0);
        assert_eq!(pending, 0);
        assert!(time_left <= 3600);
        assert!(auto_anchor);
    }

    #[test]
    fn test_reputation_system_with_epochs_auto_anchor() {
        let mut system = ReputationSystemWithEpochs::new(98765, 1, 2); // 1 second, 2 events max

        system.set_auto_anchor(false);
        let (_, _, _, auto_anchor) = system.get_epoch_status();
        assert!(!auto_anchor);

        system.set_auto_anchor(true);
        let (_, _, _, auto_anchor) = system.get_epoch_status();
        assert!(auto_anchor);
    }

    #[test]
    fn test_reputation_system_with_epochs_pending_events() {
        let system = ReputationSystemWithEpochs::new(98765, 3600, 100);
        let pending_events = system.get_pending_events();
        assert_eq!(pending_events.len(), 0);
    }

    #[test]
    fn test_reputation_verifier_creation() {
        let verifier = ReputationVerifier::new();
        // Verifier should be created successfully
        assert!(true);
    }

    #[test]
    fn test_reputation_verifier_peer_key() {
        let mut verifier = ReputationVerifier::new();
        let key_manager = NodeKeyManager::new();
        let peer_id = "test-peer".to_string();
        let verifying_key = key_manager.get_verifying_key();

        verifier.add_peer_key(peer_id.clone(), verifying_key);

        // Should be able to retrieve the key
        assert!(verifier.key_cache.get_peer_key(&peer_id).is_some());
    }

    #[test]
    fn test_reputation_verifier_epoch_integrity() {
        let verifier = ReputationVerifier::new();

        let epoch = ReputationEpoch {
            epoch_id: 1,
            merkle_root: "test-root".to_string(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            block_number: None,
            event_count: 2,
            submitter: "test-submitter".to_string(),
        };

        let events = vec![
            ReputationEvent::new(
                "event-1".to_string(),
                "peer-1".to_string(),
                "rater-1".to_string(),
                EventType::FileTransferSuccess,
                serde_json::json!({"test": "data1"}),
                0.5,
            ),
            ReputationEvent::new(
                "event-2".to_string(),
                "peer-2".to_string(),
                "rater-2".to_string(),
                EventType::FileTransferFailure,
                serde_json::json!({"test": "data2"}),
                -0.3,
            ),
        ];

        // Set epoch for events
        let mut events_with_epoch = events;
        for event in &mut events_with_epoch {
            event.epoch = Some(1);
        }

        let result = verifier.verify_epoch_integrity(&epoch, &events_with_epoch);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_reputation_test_suite_creation() {
        let test_suite = ReputationTestSuite::new();
        // Test suite should be created successfully
        assert!(true);
    }

    #[test]
    fn test_test_results_creation() {
        let mut results = TestResults::new();
        assert_eq!(results.passed, 0);
        assert_eq!(results.failed, 0);
        assert!(results.is_success());

        results.add_test("Test 1", true, "Test passed");
        results.add_test("Test 2", false, "Test failed");

        assert_eq!(results.passed, 1);
        assert_eq!(results.failed, 1);
        assert!(!results.is_success());
    }

    #[test]
    fn test_performance_results_creation() {
        let results = PerformanceResults {
            event_count: 100,
            add_duration_ms: 50,
            finalize_duration_ms: 10,
            total_duration_ms: 60,
            events_per_second: 1666,
        };

        assert_eq!(results.event_count, 100);
        assert_eq!(results.add_duration_ms, 50);
        assert_eq!(results.finalize_duration_ms, 10);
        assert_eq!(results.total_duration_ms, 60);
        assert_eq!(results.events_per_second, 1666);
    }

    #[test]
    fn test_rep_score_cfg_default() {
        let cfg = RepScoreCfg::default();
        assert_eq!(cfg.half_life_secs, 7 * 24 * 3600);
        assert_eq!(cfg.grace_period_secs, 3600);
        assert!((cfg.base_score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_rep_score_calc_decay() {
        let calc = RepScoreCalc::new();
        let now = 1000000u64;

        // current event has full impact
        assert!((calc.calc_decay(now, now) - 1.0).abs() < f64::EPSILON);

        // within grace period has full impact
        let recent = now - 1800; // 30 min ago
        assert!((calc.calc_decay(recent, now) - 1.0).abs() < f64::EPSILON);

        // at half-life past grace has ~0.5 factor
        let half_life = calc.cfg().half_life_secs;
        let grace = calc.cfg().grace_period_secs;
        let old = now - grace - half_life;
        assert!((calc.calc_decay(old, now) - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_rep_score_calc_no_events() {
        let calc = RepScoreCalc::new();
        let events: Vec<ReputationEvent> = vec![];
        let res = calc.calc_score("peer-1", &events);

        assert!((res.score - 0.5).abs() < f64::EPSILON);
        assert_eq!(res.event_cnt, 0);
        assert!(!res.confident);
    }

    #[test]
    fn test_rep_score_calc_positive_events() {
        let calc = RepScoreCalc::new();
        let now = 1000000u64;

        let events: Vec<ReputationEvent> = (0..10).map(|i| {
            let mut e = ReputationEvent::new(
                format!("e-{}", i),
                "peer-1".to_string(),
                "rater".to_string(),
                EventType::FileTransferSuccess,
                serde_json::json!({}),
                10.0,
            );
            e.timestamp = now - (i * 60);
            e
        }).collect();

        let res = calc.calc_score_at("peer-1", &events, now);
        assert!(res.score > 0.5); // positive events push score up
        assert_eq!(res.event_cnt, 10);
        assert!(res.confident);
    }

    #[test]
    fn test_rep_score_calc_negative_events() {
        let calc = RepScoreCalc::new();
        let now = 1000000u64;

        let events: Vec<ReputationEvent> = (0..10).map(|i| {
            let mut e = ReputationEvent::new(
                format!("e-{}", i),
                "peer-1".to_string(),
                "rater".to_string(),
                EventType::MaliciousBehaviorReport,
                serde_json::json!({}),
                -50.0,
            );
            e.timestamp = now - (i * 60);
            e
        }).collect();

        let res = calc.calc_score_at("peer-1", &events, now);
        assert!(res.score < 0.5); // negative events push score down
    }

    #[test]
    fn test_rep_score_calc_filters_by_peer() {
        let calc = RepScoreCalc::new();
        let now = 1000000u64;

        let mut e1 = ReputationEvent::new(
            "e1".to_string(),
            "peer-1".to_string(),
            "rater".to_string(),
            EventType::FileTransferSuccess,
            serde_json::json!({}),
            10.0,
        );
        e1.timestamp = now;

        let mut e2 = ReputationEvent::new(
            "e2".to_string(),
            "peer-2".to_string(),
            "rater".to_string(),
            EventType::MaliciousBehaviorReport,
            serde_json::json!({}),
            -50.0,
        );
        e2.timestamp = now;

        let events = vec![e1, e2];

        // peer-1 should have good score
        let r1 = calc.calc_score_at("peer-1", &events, now);
        assert!(r1.score > 0.5);
        assert_eq!(r1.event_cnt, 1);

        // peer-2 should have bad score
        let r2 = calc.calc_score_at("peer-2", &events, now);
        assert!(r2.score < 0.5);
        assert_eq!(r2.event_cnt, 1);
    }

    #[test]
    fn test_rep_score_calc_batch() {
        let calc = RepScoreCalc::new();
        // use current time so event doesnt decay
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut e = ReputationEvent::new(
            "e1".to_string(),
            "peer-1".to_string(),
            "rater".to_string(),
            EventType::FileTransferSuccess,
            serde_json::json!({}),
            10.0,
        );
        e.timestamp = now;

        let events = vec![e];
        let ids = vec!["peer-1".to_string(), "peer-2".to_string()];
        let results = calc.calc_batch(&ids, &events);

        assert_eq!(results.len(), 2);
        assert!(results["peer-1"].score > 0.5);
        assert!((results["peer-2"].score - 0.5).abs() < f64::EPSILON);
    }
}
