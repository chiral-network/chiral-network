use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{
    accept_async, tungstenite::protocol::Message,
};
use futures_util::{SinkExt, StreamExt};
use tracing::{debug, error, info, warn};

/// WebSocket signaling server for WebRTC peer connections
/// This provides a fallback when DHT-based signaling is unavailable

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SignalingMessage {
    #[serde(rename = "register")]
    Register {
        #[serde(rename = "clientId")]
        client_id: String,
    },
    #[serde(rename = "offer")]
    Offer {
        from: String,
        to: String,
        sdp: serde_json::Value,
    },
    #[serde(rename = "answer")]
    Answer {
        from: String,
        to: String,
        sdp: serde_json::Value,
    },
    #[serde(rename = "candidate")]
    Candidate {
        from: String,
        to: String,
        candidate: serde_json::Value,
    },
    #[serde(rename = "ping")]
    Ping {
        ts: u64,
        from: String,
    },
    #[serde(rename = "pong")]
    Pong {
        ts: u64,
        from: String,
    },
    #[serde(rename = "peers")]
    Peers {
        peers: Vec<String>,
    },
    #[serde(rename = "error")]
    Error {
        message: String,
    },
}

type PeerMap = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Message>>>>;

pub struct SignalingServer {
    addr: SocketAddr,
    peers: PeerMap,
}

impl SignalingServer {
    pub fn new(port: u16) -> Self {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        Self {
            addr,
            peers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(&self.addr).await?;
        info!("WebSocket signaling server listening on {}", self.addr);

        while let Ok((stream, peer_addr)) = listener.accept().await {
            let peers = Arc::clone(&self.peers);
            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, peer_addr, peers).await {
                    error!("Error handling connection from {}: {}", peer_addr, e);
                }
            });
        }

        Ok(())
    }
}

async fn handle_connection(
    stream: TcpStream,
    peer_addr: SocketAddr,
    peers: PeerMap,
) -> Result<(), Box<dyn std::error::Error>> {
    let ws_stream = accept_async(stream).await?;
    info!("New WebSocket connection from {}", peer_addr);

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    let mut client_id: Option<String> = None;

    // Spawn a task to forward messages from the channel to the WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages
    while let Some(msg_result) = ws_receiver.next().await {
        match msg_result {
            Ok(msg) => {
                if msg.is_text() || msg.is_binary() {
                    let text = msg.to_text().unwrap_or("");

                    match serde_json::from_str::<SignalingMessage>(text) {
                        Ok(parsed_msg) => {
                            match handle_message(
                                parsed_msg,
                                &mut client_id,
                                &tx,
                                &peers,
                            ).await {
                                Ok(_) => {},
                                Err(e) => {
                                    warn!("Error handling message: {}", e);
                                    let error_msg = SignalingMessage::Error {
                                        message: e.to_string(),
                                    };
                                    if let Ok(json) = serde_json::to_string(&error_msg) {
                                        let _ = tx.send(Message::Text(json));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Invalid message format: {}", e);
                        }
                    }
                } else if msg.is_close() {
                    info!("Client {} disconnected", client_id.as_deref().unwrap_or("unknown"));
                    break;
                }
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
        }
    }

    // Cleanup
    if let Some(id) = client_id {
        peers.lock().await.remove(&id);
        debug!("Removed peer {} from active peers", id);
    }

    send_task.abort();
    Ok(())
}

async fn handle_message(
    msg: SignalingMessage,
    client_id: &mut Option<String>,
    tx: &mpsc::UnboundedSender<Message>,
    peers: &PeerMap,
) -> Result<(), Box<dyn std::error::Error>> {
    match msg {
        SignalingMessage::Register { client_id: new_id } => {
            info!("Registering client: {}", new_id);
            *client_id = Some(new_id.clone());
            peers.lock().await.insert(new_id, tx.clone());

            // Send current peer list
            let peer_list: Vec<String> = peers.lock().await.keys().cloned().collect();
            let peers_msg = SignalingMessage::Peers { peers: peer_list };
            let json = serde_json::to_string(&peers_msg)?;
            tx.send(Message::Text(json))?;
        }

        SignalingMessage::Offer { from, to, sdp } => {
            debug!("Forwarding offer from {} to {}", from, to);
            let peers_lock = peers.lock().await;
            if let Some(recipient) = peers_lock.get(&to) {
                let offer_msg = SignalingMessage::Offer { from, to, sdp };
                let json = serde_json::to_string(&offer_msg)?;
                recipient.send(Message::Text(json))?;
            } else {
                warn!("Recipient {} not found for offer", to);
                return Err(format!("Peer {} not found", to).into());
            }
        }

        SignalingMessage::Answer { from, to, sdp } => {
            debug!("Forwarding answer from {} to {}", from, to);
            let peers_lock = peers.lock().await;
            if let Some(recipient) = peers_lock.get(&to) {
                let answer_msg = SignalingMessage::Answer { from, to, sdp };
                let json = serde_json::to_string(&answer_msg)?;
                recipient.send(Message::Text(json))?;
            } else {
                warn!("Recipient {} not found for answer", to);
                return Err(format!("Peer {} not found", to).into());
            }
        }

        SignalingMessage::Candidate { from, to, candidate } => {
            debug!("Forwarding ICE candidate from {} to {}", from, to);
            let peers_lock = peers.lock().await;
            if let Some(recipient) = peers_lock.get(&to) {
                let candidate_msg = SignalingMessage::Candidate { from, to, candidate };
                let json = serde_json::to_string(&candidate_msg)?;
                recipient.send(Message::Text(json))?;
            } else {
                warn!("Recipient {} not found for ICE candidate", to);
                // Don't fail for ICE candidates - they can arrive late
            }
        }

        SignalingMessage::Ping { ts, from } => {
            debug!("Ping from {} at {}", from, ts);
            let pong_msg = SignalingMessage::Pong { ts, from };
            let json = serde_json::to_string(&pong_msg)?;
            tx.send(Message::Text(json))?;
        }

        SignalingMessage::Pong { .. } => {
            // Just acknowledge pong, no action needed
        }

        SignalingMessage::Peers { .. } | SignalingMessage::Error { .. } => {
            // These are server-sent messages, ignore if received from client
            warn!("Received unexpected message type from client");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_signaling_server_creation() {
        let server = SignalingServer::new(9000);
        assert_eq!(server.addr.port(), 9000);
    }
}
