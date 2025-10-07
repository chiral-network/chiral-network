use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;
use rs_merkle::{Hasher, MerkleTree};
use sha2::{Digest, Sha256};
use ed25519_dalek::{SigningKey, VerifyingKey, Signer, Verifier, Signature};
use rand::rngs::OsRng;

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
    FileShared,
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

        let serialized = serde_json::to_vec(&event_data)
            .map_err(|e| format!("Serialization error: {}", e))?;

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
        
        Self { signing_key, peer_id }
    }

    pub fn get_peer_id(&self) -> &str {
        &self.peer_id
    }

    pub fn get_verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub fn sign_reputation_event(&self, mut event: ReputationEvent) -> Result<ReputationEvent, String> {
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

    pub fn verify_reputation_event(&self, event: &ReputationEvent, verifying_key: &VerifyingKey) -> Result<bool, String> {
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
        
        let signature_bytes_array: [u8; 64] = signature_bytes.try_into()
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
        let is_valid = key_manager.verify_reputation_event(&signed_event, &verifying_key).unwrap();
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
}
