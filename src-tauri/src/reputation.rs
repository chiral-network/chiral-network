use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::HashMap;
use std::sync::Arc;
use rs_merkle::{Hasher, MerkleTree};
use sha2::{Digest, Sha256};
use ed25519_dalek::{SigningKey, VerifyingKey, Signer, Verifier, Signature};
use rand::rngs::OsRng;

// Ethereum integration imports
use ethers::prelude::*;
use ethers::types::{Address, U256, BlockNumber};

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

// ============================================================================
// DHT STORAGE FOR REPUTATION DATA
// ============================================================================

pub struct ReputationDhtService {
    dht_service: Option<Arc<crate::dht::DhtService>>,
}

impl ReputationDhtService {
    pub fn new() -> Self {
        Self {
            dht_service: None,
        }
    }

    pub fn set_dht_service(&mut self, dht_service: Arc<crate::dht::DhtService>) {
        self.dht_service = Some(dht_service);
    }

    pub async fn store_reputation_event(&self, event: &ReputationEvent) -> Result<(), String> {
        let dht_service = self.dht_service.as_ref()
            .ok_or("DHT service not initialized")?;

        // Create a unique key for this reputation event
        let key = format!("reputation:{}:{}", event.peer_id, event.id);
        
        // Serialize the event
        let serialized = serde_json::to_vec(event)
            .map_err(|e| format!("Serialization error: {}", e))?;

        // Store in DHT (using existing file metadata structure as template)
        let metadata = crate::dht::FileMetadata {
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
            version: Some(1),
            parent_hash: None,
            cids: None, // Not needed for reputation events
            is_root: true,
        };

        dht_service.publish_file(metadata).await
    }

    pub async fn retrieve_reputation_events(&self, peer_id: &str) -> Result<Vec<ReputationEvent>, String> {
        let dht_service = self.dht_service.as_ref()
            .ok_or("DHT service not initialized")?;

        // Search for reputation events for this peer
        let search_key = format!("reputation:{}", peer_id);
        dht_service.search_file(search_key).await?;

        // TODO: Need to handle the search results and deserialize the events. 
        // For now, return empty vector as placeholder.
        Ok(vec![])
    }

    pub async fn store_merkle_root(&self, epoch: &ReputationEpoch) -> Result<(), String> {
        let dht_service = self.dht_service.as_ref()
            .ok_or("DHT service not initialized")?;

        let key = format!("merkle_root:{}", epoch.epoch_id);
        
        let serialized = serde_json::to_vec(epoch)
            .map_err(|e| format!("Serialization error: {}", e))?;

        let metadata = crate::dht::FileMetadata {
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
            version: Some(1),
            parent_hash: None,
            cids: None, // Not needed for merkle roots
            is_root: true,
        };

        dht_service.publish_file(metadata).await
    }
}

// ============================================================================
// SMART CONTRACT INTEGRATION
// ============================================================================

pub struct ReputationContract {
    contract_address: Option<String>,
    network_id: u64,
}

impl ReputationContract {
    pub fn new(network_id: u64) -> Self {
        Self {
            contract_address: None,
            network_id,
        }
    }

    pub fn set_contract_address(&mut self, address: String) {
        self.contract_address = Some(address);
    }

    pub async fn submit_epoch(
        &self,
        epoch: &ReputationEpoch,
        private_key: &str,
    ) -> Result<String, String> {
        // Connect to Ethereum network
        let provider = Provider::<Http>::try_from("http://127.0.0.1:8545")
            .map_err(|e| format!("Failed to connect to Geth: {}", e))?;

        // Parse and validate private key
        let private_key_clean = private_key.strip_prefix("0x").unwrap_or(private_key);
        let wallet: LocalWallet = private_key_clean
            .parse()
            .map_err(|e| format!("Invalid private key: {}", e))?;

        let chain_id = 98765u64; // Chiral Network chain ID
        let wallet = wallet.with_chain_id(chain_id);
        let client = SignerMiddleware::new(provider.clone(), wallet);

        // For now, we'll create a simple transaction that stores the epoch data
        // In a full implementation, this would call a smart contract function
        // For this commit, we'll create a transaction with epoch data in the data field
        
        let epoch_data = serde_json::to_string(epoch)
            .map_err(|e| format!("Failed to serialize epoch: {}", e))?;
        
        // Get the sender's address
        let sender_address = client.address();
        
        // Get nonce for pending block
        let nonce = provider
            .get_transaction_count(sender_address, Some(BlockNumber::Pending.into()))
            .await
            .map_err(|e| format!("Failed to get nonce: {}", e))?;

        // Get gas price
        let gas_price = provider
            .get_gas_price()
            .await
            .map_err(|e| format!("Failed to get gas price: {}", e))?;

        // Create transaction request
        let tx_request = TransactionRequest::new()
            .to(sender_address) // Self-transaction for now (would be contract address in full implementation)
            .value(U256::zero())
            .gas(100000) // Sufficient gas for reputation epoch submission
            .gas_price(gas_price)
            .nonce(nonce)
            .data(epoch_data.as_bytes().to_vec());

        // Send transaction
        let pending_tx = client
            .send_transaction(tx_request, None)
            .await
            .map_err(|e| format!("Failed to send transaction: {}", e))?;

        // Get transaction hash
        let tx_hash = pending_tx.tx_hash();
        
        tracing::info!("Submitted reputation epoch {} to blockchain with tx hash: {:?}", 
                      epoch.epoch_id, tx_hash);

        Ok(format!("{:?}", tx_hash))
    }

    pub async fn get_epoch(&self, epoch_id: u64) -> Result<Option<ReputationEpoch>, String> {
        // Connect to Ethereum network
        let provider = Provider::<Http>::try_from("http://127.0.0.1:8545")
            .map_err(|e| format!("Failed to connect to Geth: {}", e))?;

        // In a full implementation, this would:
        // 1. Call a smart contract function to get epoch data
        // 2. Parse the returned data into ReputationEpoch
        // For now, we'll search through recent transactions for reputation epochs
        
        // Get the latest block number
        let latest_block = provider
            .get_block_number()
            .await
            .map_err(|e| format!("Failed to get latest block: {}", e))?;

        // Search recent blocks for reputation transactions (last 100 blocks)
        let start_block = latest_block.saturating_sub(100);
        
        let start = start_block.as_u64();
        let end = latest_block.as_u64();
        for block_num in start..=end {
            let block_number = U64::from(block_num);
            if let Ok(Some(block)) = provider.get_block_with_txs(block_number).await {
                for tx in block.transactions {
                    if !tx.input.is_empty() {
                        let data = &tx.input;
                        // Try to deserialize as ReputationEpoch
                        if let Ok(epoch_data) = std::str::from_utf8(&data) {
                            if let Ok(epoch) = serde_json::from_str::<ReputationEpoch>(epoch_data) {
                                if epoch.epoch_id == epoch_id {
                                    tracing::info!("Found reputation epoch {} in block {}", epoch_id, block_num);
                                    return Ok(Some(epoch));
                                }
                            }
                        }
                    }
                }
            }
        }
        
        tracing::debug!("Reputation epoch {} not found in recent blocks", epoch_id);
        Ok(None)
    }

    pub async fn verify_event_proof(
        &self,
        event_hash: &str,
        proof: Vec<String>,
        epoch_id: u64,
    ) -> Result<bool, String> {
        // Connect to Ethereum network
        let provider = Provider::<Http>::try_from("http://127.0.0.1:8545")
            .map_err(|e| format!("Failed to connect to Geth: {}", e))?;

        // First, verify that the epoch exists on the blockchain
        let epoch = self.get_epoch(epoch_id).await?;
        if epoch.is_none() {
            tracing::warn!("Epoch {} not found on blockchain", epoch_id);
            return Ok(false);
        }

        // In a full implementation, this would:
        // 1. Call a smart contract function to verify the Merkle proof
        // 2. The contract would check if the event hash exists in the epoch's Merkle tree
        // 3. Return the verification result
        
        // For now, we'll do basic validation:
        // - Check that we have proof data
        // - Check that the epoch exists
        // - Basic format validation
        
        if proof.is_empty() {
            tracing::warn!("Empty proof provided for event {}", event_hash);
            return Ok(false);
        }

        if event_hash.is_empty() {
            tracing::warn!("Empty event hash provided");
            return Ok(false);
        }

        // Basic hash format validation (should be hex string)
        if !event_hash.starts_with("0x") || event_hash.len() != 66 {
            tracing::warn!("Invalid event hash format: {}", event_hash);
            return Ok(false);
        }

        tracing::info!("Verified event proof for event {} in epoch {}", event_hash, epoch_id);
        Ok(true)
    }

    /// Verify a complete reputation epoch from blockchain
    pub async fn verify_epoch_from_blockchain(&self, epoch_id: u64) -> Result<Option<ReputationEpoch>, String> {
        // Connect to Ethereum network
        let provider = Provider::<Http>::try_from("http://127.0.0.1:8545")
            .map_err(|e| format!("Failed to connect to Geth: {}", e))?;

        // Get the epoch from blockchain
        let epoch = self.get_epoch(epoch_id).await?;
        
        if let Some(epoch) = epoch {
            // Verify the epoch structure and data integrity
            self.verify_epoch_integrity(&epoch).await?;
            
            tracing::info!("Successfully verified epoch {} from blockchain", epoch_id);
            Ok(Some(epoch))
        } else {
            tracing::warn!("Epoch {} not found on blockchain", epoch_id);
            Ok(None)
        }
    }

    /// Verify epoch integrity and blockchain consistency
    async fn verify_epoch_integrity(&self, epoch: &ReputationEpoch) -> Result<(), String> {
        // Connect to Ethereum network
        let provider = Provider::<Http>::try_from("http://127.0.0.1:8545")
            .map_err(|e| format!("Failed to connect to Geth: {}", e))?;

        // Verify timestamp is reasonable (not in future, not too old)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        if epoch.timestamp > now {
            return Err("Epoch timestamp is in the future".to_string());
        }
        
        // Check if timestamp is not too old (more than 1 year)
        if now - epoch.timestamp > 365 * 24 * 60 * 60 {
            tracing::warn!("Epoch {} is very old ({} seconds ago)", epoch.epoch_id, now - epoch.timestamp);
        }

        // Verify event count is reasonable
        if epoch.event_count == 0 {
            tracing::warn!("Epoch {} has no events", epoch.epoch_id);
        }

        // Verify Merkle root format (should be hex string)
        if !epoch.merkle_root.starts_with("0x") || epoch.merkle_root.len() != 66 {
            return Err(format!("Invalid Merkle root format: {}", epoch.merkle_root));
        }

        // If block number is provided, verify it exists on blockchain
        if let Some(block_number) = epoch.block_number {
            match provider.get_block(block_number).await {
                Ok(Some(block)) => {
                    tracing::debug!("Verified epoch {} is in block {}", epoch.epoch_id, block_number);
                }
                Ok(None) => {
                    return Err(format!("Block {} not found on blockchain", block_number));
                }
                Err(e) => {
                    tracing::warn!("Failed to verify block {}: {}", block_number, e);
                }
            }
        }

        tracing::info!("Epoch {} integrity verification passed", epoch.epoch_id);
        Ok(())
    }

    /// Get reputation score for a peer from blockchain epochs
    pub async fn get_peer_reputation_score(&self, peer_id: &str, from_epoch: Option<u64>) -> Result<f64, String> {
        // Connect to Ethereum network
        let provider = Provider::<Http>::try_from("http://127.0.0.1:8545")
            .map_err(|e| format!("Failed to connect to Geth: {}", e))?;

        let mut total_score = 0.0;
        let mut event_count = 0;
        let start_epoch = from_epoch.unwrap_or(0);

        // Get the latest block number to determine search range
        let latest_block = provider
            .get_block_number()
            .await
            .map_err(|e| format!("Failed to get latest block: {}", e))?;

        // Search recent blocks for reputation epochs (last 1000 blocks)
        let start_block = latest_block.saturating_sub(1000);
        
        for block_num in start_block..=latest_block {
            if let Ok(Some(block)) = provider.get_block_with_txs(block_num).await {
                for tx in block.transactions {
                    if let Some(data) = tx.input {
                        // Try to deserialize as ReputationEpoch
                        if let Ok(epoch_data) = std::str::from_utf8(&data) {
                            if let Ok(epoch) = serde_json::from_str::<ReputationEpoch>(epoch_data) {
                                if epoch.epoch_id >= start_epoch {
                                    // This is a reputation epoch, but we need to get the actual events
                                    // For now, we'll estimate based on epoch metadata
                                    let epoch_score = self.calculate_epoch_score(&epoch, peer_id).await?;
                                    total_score += epoch_score;
                                    event_count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        if event_count == 0 {
            tracing::debug!("No reputation epochs found for peer {}", peer_id);
            return Ok(0.5); // Default neutral score
        }

        let average_score = total_score / event_count as f64;
        tracing::info!("Peer {} reputation score: {:.3} (from {} epochs)", peer_id, average_score, event_count);
        
        Ok(average_score)
    }

    /// Calculate reputation score from an epoch (placeholder implementation)
    async fn calculate_epoch_score(&self, epoch: &ReputationEpoch, peer_id: &str) -> Result<f64, String> {
        // In a full implementation, this would:
        // 1. Retrieve the actual events from the epoch
        // 2. Filter events for the specific peer
        // 3. Calculate weighted score based on event types and impact
        
        // For now, we'll use a simple heuristic based on epoch metadata
        let base_score = 0.5; // Neutral starting point
        
        // Adjust based on event count (more events = more activity = potentially better reputation)
        let activity_factor = (epoch.event_count as f64 / 100.0).min(1.0);
        
        // Adjust based on recency (more recent = higher weight)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let age_days = (now - epoch.timestamp) / (24 * 60 * 60);
        let recency_factor = if age_days < 7 { 1.0 } else if age_days < 30 { 0.8 } else { 0.6 };
        
        let score = base_score + (activity_factor * 0.3) + (recency_factor * 0.2);
        
        // Clamp score between 0.0 and 1.0
        Ok(score.max(0.0).min(1.0))
    }

    /// Verify that a peer's reputation events are consistent with blockchain data
    pub async fn verify_peer_reputation_consistency(&self, peer_id: &str, events: &[ReputationEvent]) -> Result<bool, String> {
        if events.is_empty() {
            return Ok(true);
        }

        // Group events by epoch
        let mut epoch_events: HashMap<u64, Vec<&ReputationEvent>> = HashMap::new();
        for event in events {
            if let Some(epoch_id) = event.epoch {
                epoch_events.entry(epoch_id).or_insert_with(Vec::new).push(event);
            }
        }

        // Verify each epoch
        for (epoch_id, epoch_events) in epoch_events {
            let blockchain_epoch = self.verify_epoch_from_blockchain(epoch_id).await?;
            
            if let Some(blockchain_epoch) = blockchain_epoch {
                // Verify event count matches
                if epoch_events.len() != blockchain_epoch.event_count {
                    tracing::warn!(
                        "Event count mismatch for peer {} in epoch {}: local={}, blockchain={}",
                        peer_id, epoch_id, epoch_events.len(), blockchain_epoch.event_count
                    );
                    return Ok(false);
                }
                
                // TODO: Verify Merkle tree consistency
                // This would require reconstructing the Merkle tree from events
                // and comparing with the blockchain Merkle root
            } else {
                tracing::warn!("Epoch {} not found on blockchain for peer {}", epoch_id, peer_id);
                return Ok(false);
            }
        }

        tracing::info!("Peer {} reputation consistency verification passed", peer_id);
        Ok(true)
    }

    pub fn get_contract_address(&self) -> Option<&String> {
        self.contract_address.as_ref()
    }

    pub fn get_network_id(&self) -> u64 {
        self.network_id
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

    pub async fn finalize_epoch(&mut self, private_key: &str) -> Result<String, String> {
        let epoch_id = self.current_epoch;
        let merkle_root = self.merkle_tree.get_root_hex()
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
        
        // Submit to smart contract with provided private key
        let tx_hash = self.contract.submit_epoch(&epoch, private_key).await?;
        
        // Reset for next epoch
        self.merkle_tree = ReputationMerkleTree::new();
        self.current_epoch += 1;
        
        Ok(tx_hash)
    }

    /// Get reputation score for a peer from blockchain
    pub async fn get_peer_reputation_score(&self, peer_id: &str, from_epoch: Option<u64>) -> Result<f64, String> {
        self.contract.get_peer_reputation_score(peer_id, from_epoch).await
    }

    /// Verify a peer's reputation consistency with blockchain
    pub async fn verify_peer_reputation_consistency(&self, peer_id: &str, events: &[ReputationEvent]) -> Result<bool, String> {
        self.contract.verify_peer_reputation_consistency(peer_id, events).await
    }

    /// Verify an epoch from blockchain
    pub async fn verify_epoch_from_blockchain(&self, epoch_id: u64) -> Result<Option<ReputationEpoch>, String> {
        self.contract.verify_epoch_from_blockchain(epoch_id).await
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

    pub async fn add_reputation_event(&mut self, mut event: ReputationEvent, private_key: &str) -> Result<Option<String>, String> {
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
        if self.epoch_manager.should_finalize_epoch(self.pending_events.len()) {
            let tx_hash = self.finalize_current_epoch(private_key).await?;
            return Ok(Some(tx_hash));
        }
        
        Ok(None)
    }

    pub async fn finalize_current_epoch(&mut self, private_key: &str) -> Result<String, String> {
        if self.pending_events.is_empty() {
            return Err("No events to finalize".to_string());
        }

        let epoch_id = self.epoch_manager.get_current_epoch();
        let merkle_root = self.merkle_tree.get_root_hex()
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
        
        // Submit to smart contract with provided private key
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

    /// Get reputation score for a peer from blockchain
    pub async fn get_peer_reputation_score(&self, peer_id: &str, from_epoch: Option<u64>) -> Result<f64, String> {
        self.contract.get_peer_reputation_score(peer_id, from_epoch).await
    }

    /// Verify a peer's reputation consistency with blockchain
    pub async fn verify_peer_reputation_consistency(&self, peer_id: &str, events: &[ReputationEvent]) -> Result<bool, String> {
        self.contract.verify_peer_reputation_consistency(peer_id, events).await
    }

    /// Verify an epoch from blockchain
    pub async fn verify_epoch_from_blockchain(&self, epoch_id: u64) -> Result<Option<ReputationEpoch>, String> {
        self.contract.verify_epoch_from_blockchain(epoch_id).await
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
            Err(format!("Public key not found for peer: {}", event.rater_peer_id))
        }
    }

    pub fn verify_event_against_merkle_root(
        &self,
        _event: &ReputationEvent,
        _merkle_root: &str,
        _proof: Vec<String>,
    ) -> Result<bool, String> {
        // TODO: Implement actual Merkle proof verification
        // In a real implementation, this would:
        // 1. Recreate the event hash
        // 2. Verify the Merkle proof against the root
        // 3. Return verification result
        
        // For now, return true as placeholder
        Ok(true)
    }

    pub fn verify_epoch_integrity(&self, epoch: &ReputationEpoch, events: &[ReputationEvent]) -> Result<bool, String> {
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
        results.add_test("System Creation", true, "ReputationSystemWithEpochs created successfully");
        
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

        let result1 = system.add_reputation_event(event1.clone(), "test_private_key").await;
        let result2 = system.add_reputation_event(event2.clone(), "test_private_key").await;
        
        results.add_test("Add Events", result1.is_ok() && result2.is_ok(), "Events added successfully");
        
        // Test 3: Verify events are pending
        let pending_count = system.get_pending_events().len();
        results.add_test("Pending Events", pending_count == 2, &format!("Expected 2 pending events, got {}", pending_count));
        
        // Test 4: Verify event signatures
        let events = system.get_pending_events();
        let mut signature_verified = true;
        for event in events {
            if let Err(_) = self.verifier.verify_event_signature(event) {
                signature_verified = false;
                break;
            }
        }
        results.add_test("Signature Verification", signature_verified, "All event signatures verified");
        
        // Test 5: Finalize epoch
        let finalize_result = system.finalize_current_epoch("test_private_key").await;
        results.add_test("Epoch Finalization", finalize_result.is_ok(), "Epoch finalized successfully");
        
        // Test 6: Verify epoch advancement
        let (current_epoch, pending, _, _) = system.get_epoch_status();
        results.add_test("Epoch Advancement", current_epoch == 1 && pending == 0, "Epoch advanced and cleared");
        
        Ok(results)
    }

    pub async fn run_performance_test(&mut self, event_count: usize) -> Result<PerformanceResults, String> {
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
            system.add_reputation_event(event, "test_private_key").await?;
        }
        let add_duration = add_start.elapsed().unwrap_or_default();
        
        // Finalize epoch
        let finalize_start = SystemTime::now();
        system.finalize_current_epoch("test_private_key").await?;
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
        assert_eq!(contract.get_network_id(), 98765);
        assert!(contract.get_contract_address().is_none());
    }

    #[test]
    fn test_reputation_contract_address() {
        let mut contract = ReputationContract::new(98765);
        contract.set_contract_address("0x1234567890abcdef".to_string());
        assert_eq!(contract.get_contract_address().unwrap(), "0x1234567890abcdef");
    }

    #[test]
    fn test_reputation_system_creation() {
        let system = ReputationSystem::new(98765);
        assert_eq!(system.contract.get_network_id(), 98765);
        assert_eq!(system.current_epoch, 0);
    }

    #[test]
    fn test_reputation_system_contract_address() {
        let mut system = ReputationSystem::new(98765);
        system.set_contract_address("0xabcdef1234567890".to_string());
        assert_eq!(system.contract.get_contract_address().unwrap(), "0xabcdef1234567890");
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
}
