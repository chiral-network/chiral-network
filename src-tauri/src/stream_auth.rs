use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use zeroize::{Zeroize, ZeroizeOnDrop};
use tracing::{debug, warn};
use x25519_dalek::{EphemeralSecret, PublicKey, SharedSecret};

type HmacSha256 = Hmac<Sha256>;

/// Stream authentication for data integrity verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamAuth{
    /// Session ID for this transfer
    pub session_id: String,
    /// HMAC key for stream authentication
    pub hmac_key: Vec<u8>,
    /// Sequence number for ordering
    pub sequence: u64,
    /// Timestamp of last activity
    pub last_activity: u64,
}

/// Stream authentication message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMessage {
    /// Message type
    pub message_type: AuthMessageType,
    /// Data payload
    pub data: Vec<u8>,
    /// HMAC signature
    pub signature: String,
    /// Sequence number
    pub sequence: u64,
    /// Timestamp
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AuthMessageType {
    /// Initial handshake
    Handshake,
    /// Data chunk with authentication
    DataChunk,
    /// Heartbeat/keepalive
    Heartbeat,
    /// Transfer completion
    Complete,
    /// Error notification
    Error,
    /// HMAC key exchange request
    KeyExchangeRequest,
    /// HMAC key exchange response
    KeyExchangeResponse,
    /// HMAC key exchange confirmation
    KeyExchangeConfirmation,
}

/// HMAC key exchange request for secure session establishment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HmacKeyExchangeRequest {
    /// Unique exchange ID
    pub exchange_id: String,
    /// Initiator's peer ID
    pub initiator_peer_id: String,
    /// Target peer ID
    pub target_peer_id: String,
    /// Initiator's X25519 public key
    pub initiator_public_key: String,
    /// Session ID for the transfer
    pub session_id: String,
    /// Timestamp
    pub timestamp: u64,
    /// Nonce for freshness
    pub nonce: String,
}

/// Response to HMAC key exchange request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HmacKeyExchangeResponse {
    /// Same exchange ID as request
    pub exchange_id: String,
    /// Responder's peer ID
    pub responder_peer_id: String,
    /// Responder's X25519 public key
    pub responder_public_key: String,
    /// Confirmation that both peers have derived the same HMAC key
    pub hmac_key_confirmation: String,
    /// Timestamp
    pub timestamp: u64,
    /// Nonce for freshness
    pub nonce: String,
}

/// Final confirmation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HmacKeyExchangeConfirmation {
    /// Same exchange ID
    pub exchange_id: String,
    /// Initiator's confirmation
    pub initiator_confirmation: String,
    /// Timestamp
    pub timestamp: u64,
}

/// Key exchange state tracking
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct KeyExchangeState{
    pub exchange_id: String,
    pub initiator_peer_id: String,
    pub target_peer_id: String,
    pub session_id: String,
    pub initiator_public_key: PublicKey,
    pub initiator_secret: Option<EphemeralSecret>,
    pub responder_public_key: Option<PublicKey>,
    #[zeroize(skip)] // This key is shared, but we'll clear it manually when the session ends.
    pub derived_hmac_key: Option<Vec<u8>>,
    #[zeroize(skip)] // This is not sensitive data, no need to zeroize.
    pub state: ExchangeState,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExchangeState {
    /// Exchange initiated, waiting for response
    Initiated,
    /// Response received, waiting for confirmation
    Responded,
    /// Exchange completed successfully
    Completed,
    /// Exchange failed or expired
    Failed,
}

/// Stream authentication service
pub struct StreamAuthService {
    /// Active authenticated sessions
    sessions: HashMap<String, StreamAuth>,
    /// Session timeout (seconds)
    session_timeout: u64,
    /// Active key exchanges
    key_exchanges: HashMap<String, KeyExchangeState>,
    /// Exchange timeout (seconds)
    exchange_timeout: u64,
}

impl StreamAuthService {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            session_timeout: 300, // 5 minutes
            key_exchanges: HashMap::new(),
            exchange_timeout: 300, // 5 minutes
        }
    }

    /// Create a new authenticated session
    pub fn create_session(&mut self, session_id: String, hmac_key: Vec<u8>) -> Result<(), String> {
        if self.sessions.contains_key(&session_id) {
            return Err("Session already exists".to_string());
        }

        let session = StreamAuth {
            session_id: session_id.clone(),
            hmac_key,
            sequence: 0,
            last_activity: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        self.sessions.insert(session_id.clone(), session);
        debug!("Created authenticated session: {}", session_id);
        Ok(())
    }

    /// Generate HMAC signature for data
    pub fn sign_data(
        &mut self,
        session_id: &str,
        data: &[u8],
        message_type: AuthMessageType,
    ) -> Result<AuthMessage, String> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or("Session not found")?;

        // Update sequence and timestamp
        session.sequence += 1;
        session.last_activity = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create message to sign
        let timestamp = session.last_activity;
        let sequence = session.sequence;

        // Combine all data for signing: type + data + sequence + timestamp
        let mut sign_data = Vec::new();
        sign_data.extend_from_slice(&[message_type.clone() as u8]);
        sign_data.extend_from_slice(data);
        sign_data.extend_from_slice(&sequence.to_be_bytes());
        sign_data.extend_from_slice(&timestamp.to_be_bytes());

        // Generate HMAC signature
        let mut mac = HmacSha256::new_from_slice(&session.hmac_key)
            .map_err(|e| format!("HMAC key error: {}", e))?;
        mac.update(&sign_data);
        let signature = hex::encode(mac.finalize().into_bytes());

        Ok(AuthMessage {
            message_type,
            data: data.to_vec(),
            signature,
            sequence,
            timestamp,
        })
    }

    /// Verify HMAC signature for received data
    pub fn verify_data(
        &mut self,
        session_id: &str,
        auth_msg: &AuthMessage,
    ) -> Result<bool, String> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or("Session not found")?;

        // Check sequence number (should be next expected)
        if auth_msg.sequence != session.sequence + 1 {
            warn!(
                "Sequence mismatch: expected {}, got {}",
                session.sequence + 1,
                auth_msg.sequence
            );
            return Ok(false);
        }

        // Check timestamp (should be recent)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if now.saturating_sub(auth_msg.timestamp) > self.session_timeout {
            warn!(
                "Message too old: {} seconds",
                now.saturating_sub(auth_msg.timestamp)
            );
            return Ok(false);
        }

        // Recreate the data that was signed
        let mut sign_data = Vec::new();
        sign_data.extend_from_slice(&[auth_msg.message_type.clone() as u8]);
        sign_data.extend_from_slice(&auth_msg.data);
        sign_data.extend_from_slice(&auth_msg.sequence.to_be_bytes());
        sign_data.extend_from_slice(&auth_msg.timestamp.to_be_bytes());

        // Verify HMAC signature
        let mut mac = HmacSha256::new_from_slice(&session.hmac_key)
            .map_err(|e| format!("HMAC key error: {}", e))?;
        mac.update(&sign_data);
        let expected_signature = hex::encode(mac.finalize().into_bytes());

        if expected_signature != auth_msg.signature {
            warn!("Signature verification failed for session {}", session_id);
            return Ok(false);
        }

        // Update session state
        session.sequence = auth_msg.sequence;
        session.last_activity = auth_msg.timestamp;

        debug!("Verified authenticated message for session {}", session_id);
        Ok(true)
    }

    /// Generate a shared HMAC key for a session
    pub fn generate_hmac_key() -> Vec<u8> {
        use rand::RngCore;
        let mut key = [0u8; 32]; // 256-bit key
        rand::thread_rng().fill_bytes(&mut key);
        key.to_vec()
    }

    /// Clean up expired sessions
    pub fn cleanup_expired_sessions(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let expired: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, session)| now.saturating_sub(session.last_activity) > self.session_timeout)
            .map(|(id, _)| id.clone())
            .collect();

        for session_id in expired {
            self.sessions.remove(&session_id);
            debug!("Cleaned up expired session: {}", session_id);
        }
    }

    /// Get session info
    pub fn get_session_info(&self, session_id: &str) -> Option<&StreamAuth> {
        self.sessions.get(session_id)
    }

    /// Remove a session
    pub fn remove_session(&mut self, session_id: &str) -> bool {
        self.sessions.remove(session_id).is_some()
    }

    /// Create authenticated data chunk
    pub fn create_authenticated_chunk(
        &mut self,
        session_id: &str,
        chunk_data: &[u8],
        chunk_index: u32,
        file_hash: &str,
    ) -> Result<AuthMessage, String> {
        // Create chunk metadata
        let mut metadata = Vec::new();
        metadata.extend_from_slice(&chunk_index.to_be_bytes());
        metadata.extend_from_slice(file_hash.as_bytes());
        metadata.extend_from_slice(chunk_data);

        self.sign_data(session_id, &metadata, AuthMessageType::DataChunk)
    }

    /// Verify authenticated data chunk
    pub fn verify_authenticated_chunk(
        &mut self,
        session_id: &str,
        auth_msg: &AuthMessage,
    ) -> Result<Option<Vec<u8>>, String> {
        if !self.verify_data(session_id, auth_msg)? {
            return Ok(None);
        }

        if let AuthMessageType::DataChunk = auth_msg.message_type {
            // Extract chunk data (skip metadata)
            if auth_msg.data.len() < 4 {
                return Err("Invalid chunk data".to_string());
            }

            // Skip chunk_index (4 bytes) and file_hash (32 bytes for SHA-256)
            let data_start = 4 + 32; // chunk_index + file_hash
            if auth_msg.data.len() <= data_start {
                return Err("No chunk data found".to_string());
            }

            Ok(Some(auth_msg.data[data_start..].to_vec()))
        } else {
            Err("Expected DataChunk message type".to_string())
        }
    }

    /// Create handshake message
    pub fn create_handshake(
        &mut self,
        session_id: &str,
        peer_id: &str,
    ) -> Result<AuthMessage, String> {
        let handshake_data = format!("handshake:{}:{}", session_id, peer_id);
        self.sign_data(
            session_id,
            handshake_data.as_bytes(),
            AuthMessageType::Handshake,
        )
    }

    /// Verify handshake message
    pub fn verify_handshake(
        &mut self,
        session_id: &str,
        auth_msg: &AuthMessage,
    ) -> Result<bool, String> {
        if !self.verify_data(session_id, auth_msg)? {
            return Ok(false);
        }

        if let AuthMessageType::Handshake = auth_msg.message_type {
            let handshake_str = String::from_utf8_lossy(&auth_msg.data);
            debug!("Verified handshake: {}", handshake_str);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Create heartbeat message
    pub fn create_heartbeat(&mut self, session_id: &str) -> Result<AuthMessage, String> {
        let heartbeat_data = b"heartbeat";
        self.sign_data(session_id, heartbeat_data, AuthMessageType::Heartbeat)
    }

    /// Create completion message
    pub fn create_completion(
        &mut self,
        session_id: &str,
        file_hash: &str,
    ) -> Result<AuthMessage, String> {
        let completion_data = format!("complete:{}", file_hash);
        self.sign_data(
            session_id,
            completion_data.as_bytes(),
            AuthMessageType::Complete,
        )
    }

    /// Create error message
    pub fn create_error(
        &mut self,
        session_id: &str,
        error_msg: &str,
    ) -> Result<AuthMessage, String> {
        let error_data = format!("error:{}", error_msg);
        self.sign_data(session_id, error_data.as_bytes(), AuthMessageType::Error)
    }

    // ===== HMAC KEY EXCHANGE METHODS =====

    /// Initiate HMAC key exchange with a peer
    pub fn initiate_key_exchange(
        &mut self,
        initiator_peer_id: String,
        target_peer_id: String,
        session_id: String,
    ) -> Result<HmacKeyExchangeRequest, String> {
        // Generate unique exchange ID
        let exchange_id = format!("{}-{}-{}", initiator_peer_id, target_peer_id, session_id);

        // Check if exchange already exists
        if self.key_exchanges.contains_key(&exchange_id) {
            return Err("Key exchange already in progress".to_string());
        }

        // Generate ephemeral key pair
        let ephemeral_secret = EphemeralSecret::random_from_rng(rand::thread_rng());
        let ephemeral_public_key = PublicKey::from(&ephemeral_secret);

        // Create exchange state
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let exchange_state = KeyExchangeState {
            exchange_id: exchange_id.clone(),
            initiator_peer_id: initiator_peer_id.clone(),
            target_peer_id: target_peer_id.clone(),
            session_id: session_id.clone(),
            initiator_public_key: ephemeral_public_key,
            initiator_secret: Some(ephemeral_secret),
            responder_public_key: None,
            derived_hmac_key: None,
            state: ExchangeState::Initiated,
            created_at: now,
            expires_at: now + self.exchange_timeout,
        };

        self.key_exchanges
            .insert(exchange_id.clone(), exchange_state);

        // Generate nonce for freshness
        let mut nonce_bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = hex::encode(nonce_bytes);

        // Create request
        let request = HmacKeyExchangeRequest {
            exchange_id: exchange_id.clone(),
            initiator_peer_id,
            target_peer_id,
            initiator_public_key: hex::encode(ephemeral_public_key.as_bytes()),
            session_id,
            timestamp: now,
            nonce,
        };

        debug!("Initiated HMAC key exchange: {}", exchange_id);
        Ok(request)
    }

    /// Respond to HMAC key exchange request
    pub fn respond_to_key_exchange(
        &mut self,
        request: HmacKeyExchangeRequest,
        responder_peer_id: String,
    ) -> Result<HmacKeyExchangeResponse, String> {
        // Validate request
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now > request.timestamp + self.exchange_timeout {
            return Err("Key exchange request expired".to_string());
        }

        // Parse initiator's public key
        let initiator_public_key_bytes = hex::decode(&request.initiator_public_key)
            .map_err(|e| format!("Invalid initiator public key: {}", e))?;
        let initiator_public_key: [u8; 32] = initiator_public_key_bytes
            .try_into()
            .map_err(|_| "Invalid initiator public key length")?;
        let initiator_public_key = PublicKey::from(initiator_public_key);

        // Generate responder's ephemeral key pair
        let responder_secret = EphemeralSecret::random_from_rng(rand::thread_rng());
        let responder_public_key = PublicKey::from(&responder_secret);

        // Compute shared secret
        let shared_secret = responder_secret.diffie_hellman(&initiator_public_key);

        // Derive HMAC key from shared secret
        let hmac_key = self.derive_hmac_key(&shared_secret, &request.exchange_id)?;

        // Create exchange state
        let exchange_state = KeyExchangeState {
            exchange_id: request.exchange_id.clone(),
            initiator_peer_id: request.initiator_peer_id,
            target_peer_id: request.target_peer_id,
            session_id: request.session_id,
            initiator_public_key,
            initiator_secret: None,
            responder_public_key: Some(responder_public_key),
            derived_hmac_key: Some(hmac_key.clone()),
            state: ExchangeState::Responded,
            created_at: now,
            expires_at: now + self.exchange_timeout,
        };

        self.key_exchanges
            .insert(request.exchange_id.clone(), exchange_state);

        // Generate confirmation hash
        let mut confirmation_data = Vec::new();
        confirmation_data.extend_from_slice(&hmac_key);
        confirmation_data.extend_from_slice(request.exchange_id.as_bytes());
        confirmation_data.extend_from_slice(&now.to_be_bytes());

        let confirmation_hash = sha2::Sha256::digest(&confirmation_data);
        let hmac_key_confirmation = hex::encode(confirmation_hash);

        // Generate nonce for freshness
        let mut nonce_bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = hex::encode(nonce_bytes);

        let response = HmacKeyExchangeResponse {
            exchange_id: request.exchange_id,
            responder_peer_id,
            responder_public_key: hex::encode(responder_public_key.as_bytes()),
            hmac_key_confirmation,
            timestamp: now,
            nonce,
        };

        debug!("Responded to HMAC key exchange: {}", response.exchange_id);
        Ok(response)
    }

    /// Confirm HMAC key exchange completion
    pub fn confirm_key_exchange(
        &mut self,
        response: HmacKeyExchangeResponse,
        _initiator_peer_id: String,
    ) -> Result<HmacKeyExchangeConfirmation, String> {
        // Extract necessary data from exchange state
        let (initiator_secret, session_id) = {
            let exchange_state = self
                .key_exchanges
                .get_mut(&response.exchange_id)
                .ok_or("Key exchange not found")?;

            if exchange_state.state != ExchangeState::Initiated {
                return Err("Invalid exchange state".to_string());
            }

            let secret = exchange_state
                .initiator_secret
                .take()
                .ok_or("Initiator secret not found")?;
            let session_id = exchange_state.session_id.clone();

            (secret, session_id)
        };

        // Parse responder's public key
        let responder_public_key_bytes = hex::decode(&response.responder_public_key)
            .map_err(|e| format!("Invalid responder public key: {}", e))?;
        let responder_public_key: [u8; 32] = responder_public_key_bytes
            .try_into()
            .map_err(|_| "Invalid responder public key length")?;
        let responder_public_key = PublicKey::from(responder_public_key);

        // Compute shared secret
        let shared_secret = initiator_secret.diffie_hellman(&responder_public_key);

        // Derive HMAC key
        let hmac_key = Self::derive_hmac_key_static(&shared_secret, &response.exchange_id)?;

        // Verify responder's confirmation
        let mut expected_confirmation_data = Vec::new();
        expected_confirmation_data.extend_from_slice(&hmac_key);
        expected_confirmation_data.extend_from_slice(response.exchange_id.as_bytes());
        expected_confirmation_data.extend_from_slice(&response.timestamp.to_be_bytes());

        let expected_confirmation_hash = sha2::Sha256::digest(&expected_confirmation_data);
        let expected_confirmation = hex::encode(expected_confirmation_hash);

        if expected_confirmation != response.hmac_key_confirmation {
            return Err("HMAC key confirmation mismatch".to_string());
        }

        // Generate initiator's confirmation
        let mut initiator_confirmation_data = Vec::new();
        initiator_confirmation_data.extend_from_slice(&hmac_key);
        initiator_confirmation_data.extend_from_slice(response.exchange_id.as_bytes());
        initiator_confirmation_data.extend_from_slice(&response.timestamp.to_be_bytes());
        initiator_confirmation_data.extend_from_slice(b"initiator-confirmed");

        let initiator_confirmation_hash = sha2::Sha256::digest(&initiator_confirmation_data);
        let initiator_confirmation = hex::encode(initiator_confirmation_hash);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Create authenticated session
        self.create_session(session_id.clone(), hmac_key.clone())?;

        // Update exchange state
        {
            let exchange_state = self
                .key_exchanges
                .get_mut(&response.exchange_id)
                .ok_or("Key exchange not found")?;
            exchange_state.responder_public_key = Some(responder_public_key);
            exchange_state.derived_hmac_key = Some(hmac_key);
            exchange_state.state = ExchangeState::Completed;
        }

        let confirmation = HmacKeyExchangeConfirmation {
            exchange_id: response.exchange_id,
            initiator_confirmation,
            timestamp: now,
        };

        debug!("Confirmed HMAC key exchange: {}", confirmation.exchange_id);
        Ok(confirmation)
    }

    /// Finalize key exchange on responder side
    pub fn finalize_key_exchange(
        &mut self,
        confirmation: HmacKeyExchangeConfirmation,
        _responder_peer_id: String,
    ) -> Result<(), String> {
        // Extract necessary data from exchange state
        let (hmac_key, session_id) = {
            let exchange_state = self
                .key_exchanges
                .get(&confirmation.exchange_id)
                .ok_or("Key exchange not found")?;

            if exchange_state.state != ExchangeState::Responded {
                return Err("Invalid exchange state".to_string());
            }

            let hmac_key = exchange_state
                .derived_hmac_key
                .clone()
                .ok_or("Missing derived HMAC key")?;
            let session_id = exchange_state.session_id.clone();

            (hmac_key, session_id)
        };

        // Verify initiator's confirmation
        let mut expected_confirmation_data = Vec::new();
        expected_confirmation_data.extend_from_slice(&hmac_key);
        expected_confirmation_data.extend_from_slice(confirmation.exchange_id.as_bytes());
        expected_confirmation_data.extend_from_slice(&confirmation.timestamp.to_be_bytes());
        expected_confirmation_data.extend_from_slice(b"initiator-confirmed");

        let expected_confirmation_hash = sha2::Sha256::digest(&expected_confirmation_data);
        let expected_confirmation = hex::encode(expected_confirmation_hash);

        if expected_confirmation != confirmation.initiator_confirmation {
            return Err("Initiator confirmation mismatch".to_string());
        }

        // Create authenticated session
        self.create_session(session_id, hmac_key)?;

        // Update state
        {
            let exchange_state = self
                .key_exchanges
                .get_mut(&confirmation.exchange_id)
                .ok_or("Key exchange not found")?;
            exchange_state.state = ExchangeState::Completed;
        }

        debug!("Finalized HMAC key exchange: {}", confirmation.exchange_id);
        Ok(())
    }

    /// Derive HMAC key from shared secret using HKDF (static version)
    fn derive_hmac_key_static(
        shared_secret: &SharedSecret,
        exchange_id: &str,
    ) -> Result<Vec<u8>, String> {
        let hk = Hkdf::<Sha256>::new(Some(exchange_id.as_bytes()), shared_secret.as_bytes());

        let mut hmac_key = [0u8; 32]; // 256-bit key
        hk.expand(b"chiral-hmac-key", &mut hmac_key)
            .map_err(|e| format!("HKDF expansion failed: {}", e))?;

        Ok(hmac_key.to_vec())
    }

    /// Derive HMAC key from shared secret using HKDF
    fn derive_hmac_key(
        &self,
        shared_secret: &SharedSecret,
        exchange_id: &str,
    ) -> Result<Vec<u8>, String> {
        Self::derive_hmac_key_static(shared_secret, exchange_id)
    }

    /// Clean up expired key exchanges
    pub fn cleanup_expired_exchanges(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let expired: Vec<String> = self
            .key_exchanges
            .iter()
            .filter(|(_, exchange)| now > exchange.expires_at)
            .map(|(id, _)| id.clone())
            .collect();

        for exchange_id in expired {
            if let Some(exchange) = self.key_exchanges.remove(&exchange_id) {
                debug!("Cleaned up expired key exchange: {}", exchange_id);
                // Also remove the associated session if it exists
                self.sessions.remove(&exchange.session_id);
            }
        }
    }

    /// Get exchange status
    pub fn get_exchange_status(&self, exchange_id: &str) -> Option<&ExchangeState> {
        self.key_exchanges.get(exchange_id).map(|e| &e.state)
    }

    /// Remove a specific exchange
    pub fn remove_exchange(&mut self, exchange_id: &str) -> bool {
        if let Some(exchange) = self.key_exchanges.remove(exchange_id) {
            self.sessions.remove(&exchange.session_id);
            true
        } else {
            false
        }
    }

    /// Get all active key exchanges
    pub fn get_active_exchanges(&self) -> Vec<String> {
        self.key_exchanges.keys().cloned().collect()
    }
}

impl Default for StreamAuthService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let mut service = StreamAuthService::new();
        let session_id = "test-session".to_string();
        let hmac_key = StreamAuthService::generate_hmac_key();

        assert!(service.create_session(session_id.clone(), hmac_key).is_ok());
        assert!(service.get_session_info(&session_id).is_some());
    }

    #[test]
    fn test_duplicate_session() {
        let mut service = StreamAuthService::new();
        let session_id = "test-session".to_string();
        let hmac_key = StreamAuthService::generate_hmac_key();

        assert!(service
            .create_session(session_id.clone(), hmac_key.clone())
            .is_ok());
        assert!(service.create_session(session_id, hmac_key).is_err());
    }

    #[test]
    fn test_sign_and_verify() {
        let mut service = StreamAuthService::new();
        let session_id = "test-session".to_string();
        let hmac_key = StreamAuthService::generate_hmac_key();

        service
            .create_session(session_id.clone(), hmac_key)
            .unwrap();

        let data = b"test data";
        let auth_msg = service
            .sign_data(&session_id, data, AuthMessageType::DataChunk)
            .unwrap();

        assert!(service.verify_data(&session_id, &auth_msg).unwrap());
    }

    #[test]
    fn test_sequence_verification() {
        let mut service = StreamAuthService::new();
        let session_id = "test-session".to_string();
        let hmac_key = StreamAuthService::generate_hmac_key();

        service
            .create_session(session_id.clone(), hmac_key)
            .unwrap();

        // First message
        let data1 = b"first message";
        let auth_msg1 = service
            .sign_data(&session_id, data1, AuthMessageType::DataChunk)
            .unwrap();
        assert!(service.verify_data(&session_id, &auth_msg1).unwrap());

        // Second message
        let data2 = b"second message";
        let auth_msg2 = service
            .sign_data(&session_id, data2, AuthMessageType::DataChunk)
            .unwrap();
        assert!(service.verify_data(&session_id, &auth_msg2).unwrap());
    }

    #[test]
    fn test_tampered_message() {
        let mut service = StreamAuthService::new();
        let session_id = "test-session".to_string();
        let hmac_key = StreamAuthService::generate_hmac_key();

        service
            .create_session(session_id.clone(), hmac_key)
            .unwrap();

        let data = b"test data";
        let mut auth_msg = service
            .sign_data(&session_id, data, AuthMessageType::DataChunk)
            .unwrap();

        // Tamper with the data
        auth_msg.data[0] = b'X';

        assert!(!service.verify_data(&session_id, &auth_msg).unwrap());
    }

    #[test]
    fn test_authenticated_chunk() {
        let mut service = StreamAuthService::new();
        let session_id = "test-session".to_string();
        let hmac_key = StreamAuthService::generate_hmac_key();

        service
            .create_session(session_id.clone(), hmac_key)
            .unwrap();

        let chunk_data = b"chunk data";
        let auth_msg = service
            .create_authenticated_chunk(&session_id, chunk_data, 0, "file123")
            .unwrap();

        let verified_data = service
            .verify_authenticated_chunk(&session_id, &auth_msg)
            .unwrap();
        assert!(verified_data.is_some());
        assert_eq!(verified_data.unwrap(), chunk_data);
    }
}
