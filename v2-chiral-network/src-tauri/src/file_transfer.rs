use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::Emitter;

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

    pub async fn accept_transfer(&self, app: tauri::AppHandle, transfer_id: String, custom_download_dir: Option<String>) -> Result<String, String> {
        let mut incoming = self.pending_incoming.lock().await;

        if let Some(transfer) = incoming.get_mut(&transfer_id) {
            transfer.status = TransferStatus::Accepted;

            // Get Downloads folder path
            let downloads_dir = if let Some(ref dir) = custom_download_dir {
                let p = std::path::PathBuf::from(dir);
                if p.exists() && p.is_dir() { p } else {
                    dirs::download_dir().ok_or_else(|| "Could not find Downloads folder".to_string())?
                }
            } else {
                dirs::download_dir().ok_or_else(|| "Could not find Downloads folder".to_string())?
            };

            // Create file path
            let file_path = downloads_dir.join(&transfer.file_name);
            
            // Save file to disk
            std::fs::write(&file_path, &transfer.file_data)
                .map_err(|e| format!("Failed to save file: {}", e))?;

            transfer.status = TransferStatus::Completed;

            let file_path_str = file_path.to_string_lossy().to_string();

            // Emit file-received event with file path
            let _ = app.emit("file-received", serde_json::json!({
                "transferId": transfer_id,
                "fileName": transfer.file_name,
                "fromPeerId": transfer.peer_id,
                "filePath": file_path_str
            }));

            Ok(file_path_str)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_status_equality() {
        assert_eq!(TransferStatus::Pending, TransferStatus::Pending);
        assert_ne!(TransferStatus::Pending, TransferStatus::Accepted);
        assert_ne!(TransferStatus::Completed, TransferStatus::Failed);
    }

    #[test]
    fn test_transfer_status_serialization() {
        let status = TransferStatus::Pending;
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: TransferStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);
    }

    #[test]
    fn test_all_transfer_statuses_serialize() {
        let statuses = vec![
            TransferStatus::Pending,
            TransferStatus::Accepted,
            TransferStatus::Declined,
            TransferStatus::InProgress,
            TransferStatus::Completed,
            TransferStatus::Failed,
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let deserialized: TransferStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);
        }
    }

    #[test]
    fn test_pending_transfer_serialization() {
        let transfer = PendingTransfer {
            transfer_id: "tx-001".to_string(),
            peer_id: "peer-abc".to_string(),
            file_name: "test.txt".to_string(),
            file_data: vec![1, 2, 3],
            status: TransferStatus::Pending,
        };
        let json = serde_json::to_string(&transfer).unwrap();
        assert!(json.contains("transferId"));
        assert!(json.contains("peerId"));
        assert!(json.contains("fileName"));
        let deserialized: PendingTransfer = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.transfer_id, "tx-001");
        assert_eq!(deserialized.file_data, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_file_transfer_service_new() {
        let service = FileTransferService::new();
        let incoming = service.get_pending_incoming().await;
        let outgoing = service.get_pending_outgoing().await;
        assert!(incoming.is_empty());
        assert!(outgoing.is_empty());
    }

    #[tokio::test]
    async fn test_decline_nonexistent_transfer() {
        let service = FileTransferService::new();
        let result = service.decline_transfer("nonexistent".to_string()).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Transfer not found");
    }
}
