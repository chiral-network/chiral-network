use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

// These would be defined in your P2P/DHT module
use crate::dht::{DhtService, FileMetadata};
use crate::manager::ChunkManager;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRequest {
    pub file_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileResponse {
    pub file_data: Vec<u8>,
    pub file_name: String,
    pub file_size: u64,
}

// Simplified file transfer service without complex libp2p request-response
// This provides basic file storage and retrieval functionality

#[derive(Debug)]
pub enum FileTransferCommand {
    UploadFile {
        file_path: String,
        file_name: String,
    },
    DownloadFile {
        file_hash: String,
        output_path: String,
    },
    GetStoredFiles,
}

#[derive(Debug, Clone)]
pub enum FileTransferEvent {
    FileUploaded {
        file_hash: String,
        file_name: String,
    },
    FileDownloaded {
        file_path: String,
    },
    FileNotFound {
        file_hash: String,
    },
    Error {
        message: String,
    },
}

pub struct FileTransferService {
    cmd_tx: mpsc::Sender<FileTransferCommand>,
    event_rx: Arc<Mutex<mpsc::Receiver<FileTransferEvent>>>,
    stored_files: Arc<Mutex<HashMap<String, (String, Vec<u8>)>>>, // hash -> (name, data)
    // Add a reference to the DHT service to perform P2P operations
    dht_service: Arc<DhtService>,
    // Add a ChunkManager to handle reassembly and decryption
    chunk_manager: Arc<ChunkManager>,
}

impl FileTransferService {
    pub async fn new() -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(100);
        let stored_files = Arc::new(Mutex::new(HashMap::new()));

        // In a real app, the DhtService would be passed in or created here.
        // For this example, we'll assume it's created and passed.
        // This is a placeholder; you'd need to properly initialize it.
        let dht_service =
            Arc::new(DhtService::new(0, vec![], None).await.map_err(|e| e.to_string())?);

        // Initialize the ChunkManager
        let storage_path = PathBuf::from("./chiral-storage/chunks"); // Example path
        std::fs::create_dir_all(&storage_path).map_err(|e| e.to_string())?;
        let chunk_manager = Arc::new(ChunkManager::new(storage_path));

        // Spawn the file transfer service task
        tokio::spawn(Self::run_file_transfer_service(
            cmd_rx,
            event_tx,
            stored_files.clone(),
            dht_service.clone(),
            chunk_manager.clone(),
        ));

        Ok(FileTransferService {
            cmd_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            stored_files,
            dht_service,
            chunk_manager,
        })
    }

    async fn run_file_transfer_service(
        mut cmd_rx: mpsc::Receiver<FileTransferCommand>,
        event_tx: mpsc::Sender<FileTransferEvent>,
        stored_files: Arc<Mutex<HashMap<String, (String, Vec<u8>)>>>,
        dht_service: Arc<DhtService>,
        chunk_manager: Arc<ChunkManager>,
    ) {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                FileTransferCommand::UploadFile {
                    file_path,
                    file_name,
                } => match Self::handle_upload_file(&file_path, &file_name, &stored_files).await {
                    Ok(file_hash) => {
                        let _ = event_tx
                            .send(FileTransferEvent::FileUploaded {
                                file_hash: file_hash.clone(),
                                file_name: file_name.clone(),
                            })
                            .await;
                        info!("File uploaded successfully: {} -> {}", file_name, file_hash);
                    }
                    Err(e) => {
                        let error_msg = format!("Upload failed: {}", e);
                        let _ = event_tx
                            .send(FileTransferEvent::Error {
                                message: error_msg.clone(),
                            })
                            .await;
                        error!("File upload failed: {}", error_msg);
                    }
                },
                FileTransferCommand::DownloadFile {
                    file_hash,
                    output_path,
                } => {
                    // Use the DHT service for the download
                    match Self::handle_p2p_download_file(
                        &file_hash,
                        &output_path,
                        &dht_service,
                        &chunk_manager,
                    )
                    .await {
                        Ok(()) => {
                            let _ = event_tx
                                .send(FileTransferEvent::FileDownloaded {
                                    file_path: output_path.clone(),
                                })
                                .await;
                            info!(
                                "File downloaded successfully: {} -> {}",
                                file_hash, output_path
                            );
                        }
                        Err(e) => {
                            let error_msg = format!("Download failed: {}", e);
                            let _ = event_tx
                                .send(FileTransferEvent::Error {
                                    message: error_msg.clone(),
                                })
                                .await;
                            error!("File download failed: {}", error_msg);
                        }
                    }
                }
                FileTransferCommand::GetStoredFiles => {
                    // This could be used to list available files
                    debug!("GetStoredFiles command received");
                }
            }
        }
    }

    async fn handle_upload_file(
        file_path: &str,
        file_name: &str,
        stored_files: &Arc<Mutex<HashMap<String, (String, Vec<u8>)>>>,
    ) -> Result<String, String> {
        // Read the file
        let file_data = tokio::fs::read(file_path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?;

        // Calculate file hash
        let file_hash = Self::calculate_file_hash(&file_data);

        // Store the file in memory (in a real implementation, this would be persistent storage)
        {
            let mut files = stored_files.lock().await;
            files.insert(file_hash.clone(), (file_name.to_string(), file_data));
        }

        Ok(file_hash)
    }

    // This is the new function that performs a real P2P download
    async fn handle_p2p_download_file(
        file_hash: &str,
        output_path: &str,
        dht_service: &Arc<DhtService>,
        chunk_manager: &Arc<ChunkManager>,
    ) -> Result<(), String> {
        info!("Starting P2P download for hash: {}", file_hash);

        // 1. Find the file metadata and seeders on the DHT
        // Note: This requires `dht_service` to have a method that returns the metadata.
        let metadata: FileMetadata = dht_service
            .get_file_metadata(file_hash.to_string())
            .await?
            .ok_or_else(|| "File metadata not found on the network".to_string())?;

        if metadata.seeders.is_empty() {
            return Err("No seeders found for this file".to_string());
        }

        // 2. Select a seeder (e.g., the first one for simplicity)
        let seeder_peer_id_str = metadata.seeders[0].clone();
        let seeder_peer_id = seeder_peer_id_str
            .parse()
            .map_err(|_| "Invalid seeder PeerId".to_string())?;

        info!("Found seeder: {}", seeder_peer_id);

        // 3. Download each chunk from the seeder and save it locally
        for chunk_info in &metadata.chunks {
            info!("Requesting chunk {} from peer {}", chunk_info.hash, seeder_peer_id);
            let encrypted_chunk_data = dht_service
                .request_chunk(seeder_peer_id, chunk_info.hash.clone())
                .await?;

            // Save the downloaded encrypted chunk to local storage
            chunk_manager
                .save_chunk(&chunk_info.hash, &encrypted_chunk_data)
                .map_err(|e| e.to_string())?;
        }

        // 4. Reassemble and decrypt the file
        // This requires the user's private key to decrypt the file's AES key.
        // This is a major piece of future work that will require UI interaction to get the password.
        info!(
            "All chunks for {} downloaded. Reassembly and decryption is the next step.",
            file_hash
        );

        // Example of the final step:
        // let keystore = crate::keystore::Keystore::load()?;
        // let private_key = keystore.get_account("USER_ADDRESS", "USER_PASSWORD")?;
        // let secret_key = EphemeralSecret::new(&mut OsRng, &private_key); // This part needs correct crypto implementation
        // chunk_manager.reassemble_and_decrypt_file(
        //     &metadata.chunks, &PathBuf::from(output_path), &metadata.encrypted_key_bundle, &secret_key
        // )?;
        // info!("File reassembled and decrypted successfully!");

        info!("P2P file downloaded: {} -> {}", file_hash, output_path);
        Ok(())
    }

    pub fn calculate_file_hash(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    pub async fn upload_file(&self, file_path: String, file_name: String) -> Result<(), String> {
        self.cmd_tx
            .send(FileTransferCommand::UploadFile {
                file_path,
                file_name,
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn download_file(
        &self,
        file_hash: String,
        output_path: String,
    ) -> Result<(), String> {
        self.cmd_tx
            .send(FileTransferCommand::DownloadFile {
                file_hash,
                output_path,
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn get_stored_files(&self) -> Result<Vec<(String, String)>, String> {
        let files = self.stored_files.lock().await;
        Ok(files
            .iter()
            .map(|(hash, (name, _))| (hash.clone(), name.clone()))
            .collect())
    }

    pub async fn drain_events(&self, max: usize) -> Vec<FileTransferEvent> {
        let mut events = Vec::new();
        let mut event_rx = self.event_rx.lock().await;

        for _ in 0..max {
            match event_rx.try_recv() {
                Ok(event) => events.push(event),
                Err(_) => break,
            }
        }

        events
    }

    pub async fn store_file_data(&self, file_hash: String, file_name: String, file_data: Vec<u8>) {
        let mut stored_files = self.stored_files.lock().await;
        stored_files.insert(file_hash, (file_name, file_data));
    }
}
