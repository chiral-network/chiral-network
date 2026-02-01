use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::Emitter;

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileTransferRequest {
    pub transfer_id: String,
    pub from_peer_id: String,
    pub file_name: String,
    pub file_size: usize,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PendingTransfer {
    pub transfer_id: String,
    pub peer_id: String,
    pub file_name: String,
    pub file_data: Vec<u8>,
    pub status: TransferStatus,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TransferStatus {
    Pending,
    Accepted,
    Declined,
    InProgress,
    Completed,
    Failed,
}

pub struct FileTransferService {
    pending_incoming: Arc<Mutex<HashMap<String, PendingTransfer>>>,
    pending_outgoing: Arc<Mutex<HashMap<String, PendingTransfer>>>,
}

impl FileTransferService {
    pub fn new() -> Self {
        Self {
            pending_incoming: Arc::new(Mutex::new(HashMap::new())),
            pending_outgoing: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn send_file(
        &self,
        app: tauri::AppHandle,
        peer_id: String,
        file_name: String,
        file_data: Vec<u8>,
        transfer_id: String,
    ) -> Result<(), String> {
        let file_size = file_data.len();

        // Store as pending outgoing
        let transfer = PendingTransfer {
            transfer_id: transfer_id.clone(),
            peer_id: peer_id.clone(),
            file_name: file_name.clone(),
            file_data,
            status: TransferStatus::Pending,
        };

        {
            let mut outgoing = self.pending_outgoing.lock().await;
            outgoing.insert(transfer_id.clone(), transfer);
        }

        // Emit file-sent event
        let _ = app.emit("file-sent", serde_json::json!({
            "transferId": transfer_id,
            "peerId": peer_id,
            "fileName": file_name,
            "fileSize": file_size
        }));

        // In a real implementation, this would use libp2p request-response protocol
        // to send the file to the peer. For now, we emit the event and mark as completed.
        // The actual P2P transfer would be handled by extending the DHT behavior.

        // Update status to completed (simulated for now)
        {
            let mut outgoing = self.pending_outgoing.lock().await;
            if let Some(t) = outgoing.get_mut(&transfer_id) {
                t.status = TransferStatus::Completed;
            }
        }

        let _ = app.emit("file-transfer-complete", serde_json::json!({
            "transferId": transfer_id,
            "status": "completed"
        }));

        Ok(())
    }

    pub async fn receive_file_request(
        &self,
        app: tauri::AppHandle,
        from_peer_id: String,
        file_name: String,
        file_data: Vec<u8>,
        transfer_id: String,
    ) -> Result<(), String> {
        let file_size = file_data.len();

        let transfer = PendingTransfer {
            transfer_id: transfer_id.clone(),
            peer_id: from_peer_id.clone(),
            file_name: file_name.clone(),
            file_data,
            status: TransferStatus::Pending,
        };

        {
            let mut incoming = self.pending_incoming.lock().await;
            incoming.insert(transfer_id.clone(), transfer);
        }

        // Emit event to frontend for user to accept/decline
        let _ = app.emit("file-transfer-request", serde_json::json!({
            "transferId": transfer_id,
            "fromPeerId": from_peer_id,
            "fileName": file_name,
            "fileSize": file_size
        }));

        Ok(())
    }

    pub async fn accept_transfer(&self, transfer_id: String) -> Result<(), String> {
        let mut incoming = self.pending_incoming.lock().await;

        if let Some(transfer) = incoming.get_mut(&transfer_id) {
            transfer.status = TransferStatus::Accepted;

            // In a real implementation, save the file to disk
            // For now, just mark as completed
            transfer.status = TransferStatus::Completed;

            Ok(())
        } else {
            Err("Transfer not found".to_string())
        }
    }

    pub async fn decline_transfer(&self, transfer_id: String) -> Result<(), String> {
        let mut incoming = self.pending_incoming.lock().await;

        if let Some(transfer) = incoming.get_mut(&transfer_id) {
            transfer.status = TransferStatus::Declined;
            Ok(())
        } else {
            Err("Transfer not found".to_string())
        }
    }

    pub async fn get_pending_incoming(&self) -> Vec<PendingTransfer> {
        let incoming = self.pending_incoming.lock().await;
        incoming.values()
            .filter(|t| t.status == TransferStatus::Pending)
            .cloned()
            .collect()
    }

    pub async fn get_pending_outgoing(&self) -> Vec<PendingTransfer> {
        let outgoing = self.pending_outgoing.lock().await;
        outgoing.values()
            .filter(|t| t.status == TransferStatus::Pending || t.status == TransferStatus::InProgress)
            .cloned()
            .collect()
    }
}
