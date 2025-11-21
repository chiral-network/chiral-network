//! Chiral Client Identification using BitTorrent Extension Protocol (BEP 10)
//!
//! This module implements a custom BitTorrent extension to identify and
//! prioritize Chiral clients within standard BitTorrent swarms.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tracing::{info, debug, warn};
use librqbit::Session;

/// Chiral BitTorrent Extension Protocol identifier
pub const CHIRAL_EXTENSION_NAME: &str = "chiral_network";

/// Chiral client capabilities and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChiralClientInfo {
    /// Chiral Network client version
    pub client_version: String,
    /// Supported Chiral features
    pub features: ChiralFeatures,
    /// Wallet address for payments
    pub wallet_address: Option<String>,
    /// Peer reputation score
    pub reputation_score: f64,
    /// Preferred protocols for direct P2P
    pub supported_protocols: Vec<String>,
    /// DHT peer ID for Chiral network
    pub chiral_peer_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChiralFeatures {
    /// Supports direct P2P payments
    pub direct_payments: bool,
    /// Supports encrypted transfers
    pub encryption: bool,
    /// Supports resume/partial downloads
    pub resume_support: bool,
    /// Supports multi-source downloads
    pub multi_source: bool,
    /// Supports bandwidth scheduling
    pub bandwidth_scheduling: bool,
}

/// Extension message types for Chiral protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChiralMessage {
    /// Initial handshake with client info
    Handshake {
        client_info: ChiralClientInfo,
        protocol_version: u8,
    },
    /// Request direct P2P connection
    DirectConnectionRequest {
        endpoint: String,
        protocol: String,
    },
    /// Response to direct connection request
    DirectConnectionResponse {
        accepted: bool,
        endpoint: Option<String>,
        reason: Option<String>,
    },
    /// Payment notification
    PaymentNotification {
        transaction_id: String,
        amount: u64,
        chunks: Vec<u32>,
    },
    /// Reputation update
    ReputationUpdate {
        score: f64,
        feedback: String,
    },
}

/// Chiral peer information in a torrent swarm
#[derive(Debug, Clone)]
pub struct ChiralPeer {
    pub peer_id: String,
    pub client_info: ChiralClientInfo,
    pub last_seen: std::time::SystemTime,
    pub connection_established: bool,
    pub direct_endpoint: Option<String>,
}

/// Events emitted by the Chiral extension
#[derive(Debug, Clone)]
pub enum ChiralExtensionEvent {
    /// New Chiral client discovered in swarm
    ChiralPeerDiscovered {
        peer_id: String,
        client_info: ChiralClientInfo,
    },
    /// Direct P2P connection established
    DirectConnectionEstablished {
        peer_id: String,
        endpoint: String,
    },
    /// Payment received from peer
    PaymentReceived {
        peer_id: String,
        amount: u64,
        transaction_id: String,
    },
    /// Reputation updated
    ReputationUpdated {
        peer_id: String,
        new_score: f64,
    },
}

/// Chiral BitTorrent Extension handler
pub struct ChiralBitTorrentExtension {
    /// Our client information
    client_info: ChiralClientInfo,
    /// Discovered Chiral peers per torrent
    chiral_peers: Arc<RwLock<HashMap<String, HashMap<String, ChiralPeer>>>>,
    /// Event broadcaster
    event_sender: broadcast::Sender<ChiralExtensionEvent>,
    /// LibRQBit session reference
    session: Arc<Session>,
}

impl ChiralBitTorrentExtension {
    /// Create new Chiral extension handler
    pub fn new(session: Arc<Session>, wallet_address: Option<String>) -> Self {
        let client_info = ChiralClientInfo {
            client_version: "0.1.0".to_string(),
            features: ChiralFeatures {
                direct_payments: wallet_address.is_some(),
                encryption: true,
                resume_support: true,
                multi_source: true,
                bandwidth_scheduling: true,
            },
            wallet_address,
            reputation_score: 1.0,
            supported_protocols: vec![
                "webrtc".to_string(),
                "http".to_string(),
                "bitswap".to_string(),
            ],
            chiral_peer_id: None, // Set during DHT integration
        };

        let (event_sender, _) = broadcast::channel(1000);

        Self {
            client_info,
            chiral_peers: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
            session,
        }
    }

    /// Subscribe to Chiral extension events
    pub fn subscribe_events(&self) -> broadcast::Receiver<ChiralExtensionEvent> {
        self.event_sender.subscribe()
    }

    /// Register extension with a torrent
    pub async fn register_with_torrent(&self, info_hash: &str) -> Result<()> {
        info!("Registering Chiral extension with torrent: {}", info_hash);

        // TODO: Use librqbit's extension API to register our extension
        // This is where we'd integrate with librqbit's BEP 10 support
        // 
        // Example (pseudo-code based on librqbit's extension API):
        // torrent.register_extension(CHIRAL_EXTENSION_NAME, self.create_extension_handler()).await?;

        // For now, we'll simulate the registration
        let mut peers = self.chiral_peers.write().await;
        peers.insert(info_hash.to_string(), HashMap::new());

        info!("Chiral extension registered for torrent: {}", info_hash);
        Ok(())
    }

    /// Handle incoming extension message from a peer
    pub async fn handle_extension_message(
        &self,
        info_hash: &str,
        peer_id: &str,
        message_data: &[u8],
    ) -> Result<()> {
        debug!("Received Chiral extension message from peer: {}", peer_id);

        // Deserialize the message
        let message: ChiralMessage = serde_json::from_slice(message_data)
            .map_err(|e| anyhow!("Failed to parse Chiral message: {}", e))?;

        match message {
            ChiralMessage::Handshake { client_info, protocol_version } => {
                self.handle_handshake(info_hash, peer_id, client_info, protocol_version).await?;
            }
            ChiralMessage::DirectConnectionRequest { endpoint, protocol } => {
                self.handle_direct_connection_request(info_hash, peer_id, endpoint, protocol).await?;
            }
            ChiralMessage::DirectConnectionResponse { accepted, endpoint, reason } => {
                self.handle_direct_connection_response(info_hash, peer_id, accepted, endpoint, reason).await?;
            }
            ChiralMessage::PaymentNotification { transaction_id, amount, chunks } => {
                self.handle_payment_notification(info_hash, peer_id, transaction_id, amount, chunks).await?;
            }
            ChiralMessage::ReputationUpdate { score, feedback } => {
                self.handle_reputation_update(info_hash, peer_id, score, feedback).await?;
            }
        }

        Ok(())
    }

    /// Send our handshake to a newly connected peer
    pub async fn send_handshake(&self, info_hash: &str, peer_id: &str) -> Result<()> {
        let handshake = ChiralMessage::Handshake {
            client_info: self.client_info.clone(),
            protocol_version: 1,
        };

        self.send_extension_message(info_hash, peer_id, &handshake).await
    }

    /// Get all Chiral peers for a torrent
    pub async fn get_chiral_peers(&self, info_hash: &str) -> Vec<ChiralPeer> {
        let peers = self.chiral_peers.read().await;
        if let Some(torrent_peers) = peers.get(info_hash) {
            torrent_peers.values().cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// Request direct P2P connection with a Chiral peer
    pub async fn request_direct_connection(
        &self,
        info_hash: &str,
        peer_id: &str,
        our_endpoint: String,
        protocol: String,
    ) -> Result<()> {
        let request = ChiralMessage::DirectConnectionRequest {
            endpoint: our_endpoint,
            protocol,
        };

        self.send_extension_message(info_hash, peer_id, &request).await
    }

    /// Prioritize Chiral peers in piece selection
    pub async fn get_prioritized_peers(&self, info_hash: &str) -> Vec<String> {
        let chiral_peers = self.get_chiral_peers(info_hash).await;
        
        // Sort Chiral peers by reputation and capabilities
        let mut sorted_peers: Vec<_> = chiral_peers.into_iter().collect();
        sorted_peers.sort_by(|a, b| {
            b.client_info.reputation_score
                .partial_cmp(&a.client_info.reputation_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        sorted_peers.into_iter().map(|p| p.peer_id).collect()
    }

    // Private helper methods

    async fn handle_handshake(
        &self,
        info_hash: &str,
        peer_id: &str,
        client_info: ChiralClientInfo,
        _protocol_version: u8,
    ) -> Result<()> {
        info!("Received Chiral handshake from peer: {} (version: {})", peer_id, client_info.client_version);

        let chiral_peer = ChiralPeer {
            peer_id: peer_id.to_string(),
            client_info: client_info.clone(),
            last_seen: std::time::SystemTime::now(),
            connection_established: false,
            direct_endpoint: None,
        };

        // Store the peer
        let mut peers = self.chiral_peers.write().await;
        if let Some(torrent_peers) = peers.get_mut(info_hash) {
            torrent_peers.insert(peer_id.to_string(), chiral_peer);
        }

        // Emit discovery event
        let _ = self.event_sender.send(ChiralExtensionEvent::ChiralPeerDiscovered {
            peer_id: peer_id.to_string(),
            client_info,
        });

        // Send our handshake back if we haven't already
        self.send_handshake(info_hash, peer_id).await?;

        Ok(())
    }

    async fn handle_direct_connection_request(
        &self,
        info_hash: &str,
        peer_id: &str,
        endpoint: String,
        protocol: String,
    ) -> Result<()> {
        info!("Direct connection request from {}: {} ({})", peer_id, endpoint, protocol);

        // Check if we support the requested protocol
        let accepted = self.client_info.supported_protocols.contains(&protocol);
        
        let response = if accepted {
            // TODO: Set up our endpoint for the direct connection
            ChiralMessage::DirectConnectionResponse {
                accepted: true,
                endpoint: Some("our_endpoint_here".to_string()), // TODO: Get actual endpoint
                reason: None,
            }
        } else {
            ChiralMessage::DirectConnectionResponse {
                accepted: false,
                endpoint: None,
                reason: Some(format!("Protocol '{}' not supported", protocol)),
            }
        };

        self.send_extension_message(info_hash, peer_id, &response).await
    }

    async fn handle_direct_connection_response(
        &self,
        info_hash: &str,
        peer_id: &str,
        accepted: bool,
        endpoint: Option<String>,
        reason: Option<String>,
    ) -> Result<()> {
        if accepted {
            if let Some(endpoint) = endpoint {
                info!("Direct connection accepted by {}: {}", peer_id, endpoint);
                
                // Update peer information
                let mut peers = self.chiral_peers.write().await;
                if let Some(torrent_peers) = peers.get_mut(info_hash) {
                    if let Some(peer) = torrent_peers.get_mut(peer_id) {
                        peer.direct_endpoint = Some(endpoint.clone());
                        peer.connection_established = true;
                    }
                }

                // Emit event
                let _ = self.event_sender.send(ChiralExtensionEvent::DirectConnectionEstablished {
                    peer_id: peer_id.to_string(),
                    endpoint,
                });
            }
        } else {
            warn!("Direct connection rejected by {}: {:?}", peer_id, reason);
        }

        Ok(())
    }

    async fn handle_payment_notification(
        &self,
        _info_hash: &str,
        peer_id: &str,
        transaction_id: String,
        amount: u64,
        _chunks: Vec<u32>,
    ) -> Result<()> {
        info!("Payment notification from {}: {} ({})", peer_id, amount, transaction_id);

        // Emit payment event
        let _ = self.event_sender.send(ChiralExtensionEvent::PaymentReceived {
            peer_id: peer_id.to_string(),
            amount,
            transaction_id,
        });

        Ok(())
    }

    async fn handle_reputation_update(
        &self,
        info_hash: &str,
        peer_id: &str,
        score: f64,
        _feedback: String,
    ) -> Result<()> {
        info!("Reputation update for {}: {}", peer_id, score);

        // Update peer reputation
        let mut peers = self.chiral_peers.write().await;
        if let Some(torrent_peers) = peers.get_mut(info_hash) {
            if let Some(peer) = torrent_peers.get_mut(peer_id) {
                peer.client_info.reputation_score = score;
            }
        }

        // Emit reputation event
        let _ = self.event_sender.send(ChiralExtensionEvent::ReputationUpdated {
            peer_id: peer_id.to_string(),
            new_score: score,
        });

        Ok(())
    }

    async fn send_extension_message(
        &self,
        info_hash: &str,
        peer_id: &str,
        message: &ChiralMessage,
    ) -> Result<()> {
        let message_data = serde_json::to_vec(message)
            .map_err(|e| anyhow!("Failed to serialize Chiral message: {}", e))?;

        debug!("Sending Chiral extension message to peer: {}", peer_id);

        // TODO: Use librqbit's extension API to send the message
        // This would be something like:
        // torrent.send_extension_message(peer_id, CHIRAL_EXTENSION_NAME, message_data).await?;

        // For now, we'll just log the intent
        debug!("Would send {} bytes to peer {} for torrent {}", 
               message_data.len(), peer_id, info_hash);

        Ok(())
    }
}

impl Default for ChiralFeatures {
    fn default() -> Self {
        Self {
            direct_payments: false,
            encryption: true,
            resume_support: true,
            multi_source: true,
            bandwidth_scheduling: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chiral_message_serialization() {
        let client_info = ChiralClientInfo {
            client_version: "0.1.0".to_string(),
            features: ChiralFeatures::default(),
            wallet_address: Some("0x123...".to_string()),
            reputation_score: 1.0,
            supported_protocols: vec!["webrtc".to_string()],
            chiral_peer_id: Some("peer123".to_string()),
        };

        let message = ChiralMessage::Handshake {
            client_info,
            protocol_version: 1,
        };

        let serialized = serde_json::to_string(&message).unwrap();
        let deserialized: ChiralMessage = serde_json::from_str(&serialized).unwrap();

        match deserialized {
            ChiralMessage::Handshake { protocol_version, .. } => {
                assert_eq!(protocol_version, 1);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_chiral_features_default() {
        let features = ChiralFeatures::default();
        assert!(!features.direct_payments);
        assert!(features.encryption);
        assert!(features.resume_support);
        assert!(features.multi_source);
        assert!(features.bandwidth_scheduling);
    }
}