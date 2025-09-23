use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
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
    DownloadChunk {
        file_hash: String,
        chunk_index: usize,
        peer_id: String,
        output_path: String,
    },
    PauseDownload {
        file_hash: String,
    },
    ResumeDownload {
        file_hash: String,
        output_path: String,
    },
    GetChunkMap {
        file_hash: String,
    },
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
    ChunkDownloaded {
        file_hash: String,
        chunk_index: usize,
    },
    DownloadProgress {
        file_hash: String,
        completed_chunks: usize,
        total_chunks: usize,
    },
    DownloadPaused {
        file_hash: String,
    },
    DownloadResumed {
        file_hash: String,
    },
}

pub struct FileTransferService {
    cmd_tx: mpsc::Sender<FileTransferCommand>,
    event_rx: Arc<Mutex<mpsc::Receiver<FileTransferEvent>>>,
    stored_files: Arc<Mutex<HashMap<String, (String, Vec<Vec<u8>>)>>>, // hash -> (name, chunks)
    download_chunk_maps: Arc<Mutex<HashMap<String, Vec<bool>>>>, // hash -> chunk map
}

impl FileTransferService {
    pub async fn new() -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(100);
        let stored_files = Arc::new(Mutex::new(HashMap::new()));
        let download_chunk_maps = Arc::new(Mutex::new(HashMap::new()));

        // Spawn the file transfer service task
        tokio::spawn(Self::run_file_transfer_service(
            cmd_rx,
            event_tx,
            stored_files.clone(),
            download_chunk_maps.clone(),
        ));

        Ok(FileTransferService {
            cmd_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            stored_files,
            download_chunk_maps,
        })
    }

    async fn run_file_transfer_service(
        mut cmd_rx: mpsc::Receiver<FileTransferCommand>,
        event_tx: mpsc::Sender<FileTransferEvent>,
        stored_files: Arc<Mutex<HashMap<String, (String, Vec<Vec<u8>>>>>>,
        download_chunk_maps: Arc<Mutex<HashMap<String, Vec<bool>>>>,
    ) {
        // Track paused downloads
        let mut paused_downloads: HashMap<String, bool> = HashMap::new();

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
                    match Self::handle_download_file(&file_hash, &output_path, &stored_files).await
                    {
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
                FileTransferCommand::DownloadChunk {
                    file_hash,
                    chunk_index,
                    peer_id: _,
                    output_path,
                } => {
                    // Simulate chunk download (replace with real P2P logic)
                    let chunk_opt = {
                        let files = stored_files.lock().await;
                        files.get(&file_hash).and_then(|(_, chunks)| chunks.get(chunk_index)).cloned()
                    };

                    if let Some(chunk) = chunk_opt {
                        // Write chunk to output_path (append or write at offset in real impl)
                        use tokio::io::AsyncWriteExt;
                        use tokio::fs::OpenOptions;
                        let mut file = match OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&output_path)
                            .await
                        {
                            Ok(f) => f,
                            Err(e) => {
                                let _ = event_tx.send(FileTransferEvent::Error {
                                    message: format!("Failed to open output file: {}", e),
                                }).await;
                                continue;
                            }
                        };
                        if let Err(e) = file.write_all(&chunk).await {
                            let _ = event_tx.send(FileTransferEvent::Error {
                                message: format!("Failed to write chunk: {}", e),
                            }).await;
                            continue;
                        }

                        // Update chunk map
                        {
                            let mut maps = download_chunk_maps.lock().await;
                            let chunk_map = maps.entry(file_hash.clone()).or_insert_with(|| {
                                let files = stored_files.blocking_lock();
                                files.get(&file_hash).map(|(_, chunks)| vec![false; chunks.len()]).unwrap_or_default()
                            });
                            if chunk_index < chunk_map.len() {
                                chunk_map[chunk_index] = true;
                            }
                        }

                        // Emit events
                        let (completed, total) = {
                            let maps = download_chunk_maps.lock().await;
                            let chunk_map = maps.get(&file_hash);
                            if let Some(map) = chunk_map {
                                (map.iter().filter(|&&b| b).count(), map.len())
                            } else {
                                (0, 0)
                            }
                        };
                        let _ = event_tx.send(FileTransferEvent::ChunkDownloaded {
                            file_hash: file_hash.clone(),
                            chunk_index,
                        }).await;
                        let _ = event_tx.send(FileTransferEvent::DownloadProgress {
                            file_hash: file_hash.clone(),
                            completed_chunks: completed,
                            total_chunks: total,
                        }).await;
                    } else {
                        let _ = event_tx.send(FileTransferEvent::Error {
                            message: format!("Chunk {} not found for file {}", chunk_index, file_hash),
                        }).await;
                    }
                }
                FileTransferCommand::PauseDownload { file_hash } => {
                    paused_downloads.insert(file_hash.clone(), true);
                    let _ = event_tx
                        .send(FileTransferEvent::DownloadPaused {
                            file_hash: file_hash.clone(),
                        })
                        .await;
                    info!("Download paused: {}", file_hash);
                }
                FileTransferCommand::ResumeDownload {
                    file_hash,
                    output_path: _,
                } => {
                    paused_downloads.remove(&file_hash);
                    let _ = event_tx
                        .send(FileTransferEvent::DownloadResumed {
                            file_hash: file_hash.clone(),
                        })
                        .await;
                    info!("Download resumed: {}", file_hash);
                }
                FileTransferCommand::GetChunkMap { file_hash } => {
                    let chunk_map = {
                        let maps = download_chunk_maps.lock().await;
                        maps.get(&file_hash).cloned()
                    };
                    debug!("GetChunkMap command received: {} {:?}", file_hash, chunk_map);
                }
            }
        }
    }

    async fn handle_upload_file(
        file_path: &str,
        file_name: &str,
        stored_files: &Arc<Mutex<HashMap<String, (String, Vec<Vec<u8>>>>>,
    ) -> Result<String, String> {
        // Read the file
        let file_data = tokio::fs::read(file_path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?;

        // Split file into chunks
        let chunk_size = 1024 * 1024; // 1MB chunks
        let file_chunks = Self::split_into_chunks(&file_data, chunk_size);

        // Calculate file hash
        let file_hash = Self::calculate_file_hash(&file_data);

        // Store the file chunks in memory
        {
            let mut files = stored_files.lock().await;
            files.insert(file_hash.clone(), (file_name.to_string(), file_chunks));
        }

        Ok(file_hash)
    }

    async fn handle_download_file(
        file_hash: &str,
        output_path: &str,
        stored_files: &Arc<Mutex<HashMap<String, (String, Vec<Vec<u8>>>>>,
    ) -> Result<(), String> {
        // Check if we have the file locally
        let (file_name, file_chunks) = {
            let files = stored_files.lock().await;
            files
                .get(file_hash)
                .ok_or_else(|| "File not found locally".to_string())?
                .clone()
        };

        // Assemble file from chunks
        let file_data = Self::assemble_from_chunks(&file_chunks);

        // Write the file to the output path
        tokio::fs::write(output_path, file_data)
            .await
            .map_err(|e| format!("Failed to write file: {}", e))?;

        info!("File downloaded: {} -> {}", file_name, output_path);
        Ok(())
    }

    pub fn split_into_chunks(data: &[u8], chunk_size: usize) -> Vec<Vec<u8>> {
        data.chunks(chunk_size).map(|chunk| chunk.to_vec()).collect()
    }

    pub fn assemble_from_chunks(chunks: &[Vec<u8>]) -> Vec<u8> {
        chunks.concat()
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
        let chunks = Self::split_into_chunks(&file_data, 1024 * 1024);
        stored_files.insert(file_hash, (file_name, chunks));
    }
}
