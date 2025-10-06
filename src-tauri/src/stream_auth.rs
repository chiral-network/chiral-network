use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

type HmacSha256 = Hmac<Sha256>;

/// Stream authentication for data integrity verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamAuth {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// Stream authentication service
pub struct StreamAuthService {
    /// Active authenticated sessions
    sessions: HashMap<String, StreamAuth>,
    /// Session timeout (seconds)
    session_timeout: u64,
}

impl StreamAuthService {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            session_timeout: 300, // 5 minutes
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
    pub fn sign_data(&mut self, session_id: &str, data: &[u8], message_type: AuthMessageType) -> Result<AuthMessage, String> {
        let session = self.sessions.get_mut(session_id)
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
    pub fn verify_data(&mut self, session_id: &str, auth_msg: &AuthMessage) -> Result<bool, String> {
        let session = self.sessions.get_mut(session_id)
            .ok_or("Session not found")?;

        // Check sequence number (should be next expected)
        if auth_msg.sequence != session.sequence + 1 {
            warn!("Sequence mismatch: expected {}, got {}", session.sequence + 1, auth_msg.sequence);
            return Ok(false);
        }

        // Check timestamp (should be recent)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if now.saturating_sub(auth_msg.timestamp) > self.session_timeout {
            warn!("Message too old: {} seconds", now.saturating_sub(auth_msg.timestamp));
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

        let expired: Vec<String> = self.sessions
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
    pub fn create_handshake(&mut self, session_id: &str, peer_id: &str) -> Result<AuthMessage, String> {
        let handshake_data = format!("handshake:{}:{}", session_id, peer_id);
        self.sign_data(session_id, handshake_data.as_bytes(), AuthMessageType::Handshake)
    }

    /// Verify handshake message
    pub fn verify_handshake(&mut self, session_id: &str, auth_msg: &AuthMessage) -> Result<bool, String> {
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
    pub fn create_completion(&mut self, session_id: &str, file_hash: &str) -> Result<AuthMessage, String> {
        let completion_data = format!("complete:{}", file_hash);
        self.sign_data(session_id, completion_data.as_bytes(), AuthMessageType::Complete)
    }

    /// Create error message
    pub fn create_error(&mut self, session_id: &str, error_msg: &str) -> Result<AuthMessage, String> {
        let error_data = format!("error:{}", error_msg);
        self.sign_data(session_id, error_data.as_bytes(), AuthMessageType::Error)
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

        assert!(service.create_session(session_id.clone(), hmac_key.clone()).is_ok());
        assert!(service.create_session(session_id, hmac_key).is_err());
    }

    #[test]
    fn test_sign_and_verify() {
        let mut service = StreamAuthService::new();
        let session_id = "test-session".to_string();
        let hmac_key = StreamAuthService::generate_hmac_key();

        service.create_session(session_id.clone(), hmac_key).unwrap();

        let data = b"test data";
        let auth_msg = service.sign_data(&session_id, data, AuthMessageType::DataChunk).unwrap();

        assert!(service.verify_data(&session_id, &auth_msg).unwrap());
    }

    #[test]
    fn test_sequence_verification() {
        let mut service = StreamAuthService::new();
        let session_id = "test-session".to_string();
        let hmac_key = StreamAuthService::generate_hmac_key();

        service.create_session(session_id.clone(), hmac_key).unwrap();

        // First message
        let data1 = b"first message";
        let auth_msg1 = service.sign_data(&session_id, data1, AuthMessageType::DataChunk).unwrap();
        assert!(service.verify_data(&session_id, &auth_msg1).unwrap());

        // Second message
        let data2 = b"second message";
        let auth_msg2 = service.sign_data(&session_id, data2, AuthMessageType::DataChunk).unwrap();
        assert!(service.verify_data(&session_id, &auth_msg2).unwrap());
    }

    #[test]
    fn test_tampered_message() {
        let mut service = StreamAuthService::new();
        let session_id = "test-session".to_string();
        let hmac_key = StreamAuthService::generate_hmac_key();

        service.create_session(session_id.clone(), hmac_key).unwrap();

        let data = b"test data";
        let mut auth_msg = service.sign_data(&session_id, data, AuthMessageType::DataChunk).unwrap();

        // Tamper with the data
        auth_msg.data[0] = b'X';

        assert!(!service.verify_data(&session_id, &auth_msg).unwrap());
    }

    #[test]
    fn test_authenticated_chunk() {
        let mut service = StreamAuthService::new();
        let session_id = "test-session".to_string();
        let hmac_key = StreamAuthService::generate_hmac_key();

        service.create_session(session_id.clone(), hmac_key).unwrap();

        let chunk_data = b"chunk data";
        let auth_msg = service.create_authenticated_chunk(&session_id, chunk_data, 0, "file123").unwrap();

        let verified_data = service.verify_authenticated_chunk(&session_id, &auth_msg).unwrap();
        assert!(verified_data.is_some());
        assert_eq!(verified_data.unwrap(), chunk_data);
    }
}
