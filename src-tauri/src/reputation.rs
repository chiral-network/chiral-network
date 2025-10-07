use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use rs_merkle::{Hasher, MerkleTree};
use sha2::{Digest, Sha256};

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
}
