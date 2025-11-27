use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tauri::command;
use tokio::fs;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize, Deserialize)]
pub struct WriteChunkRequest {
    pub transfer_id: String,
    pub chunk_index: u32,
    pub offset: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyFinalizeRequest {
    pub transfer_id: String,
    pub expected_root: Option<String>,
    pub final_path: String,
    pub tmp_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReassemblyResult {
    pub ok: bool,
    pub error: Option<String>,
}

/// Write chunk data to temporary file at specified offset
/// 
/// This performs a sparse write operation, creating the temp file if needed
/// and writing the chunk bytes at the correct offset for reassembly.
#[command]
pub async fn write_chunk_temp(
    transfer_id: String,
    chunk_index: u32,
    offset: u64,
    bytes: Vec<u8>,
) -> Result<ReassemblyResult, String> {
    // For now, we'll use a simple temp directory structure
    // In production, this should be configurable and use proper temp directories
    let temp_dir = std::env::temp_dir().join("chiral_transfers");
    
    // Create temp directory if it doesn't exist
    if let Err(e) = fs::create_dir_all(&temp_dir).await {
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to create temp directory: {}", e)),
        });
    }
    
    let temp_file_path = temp_dir.join(format!("{}.tmp", transfer_id));
    
    // Open file for writing at offset (create if doesn't exist)
    let mut file = match OpenOptions::new()
        .create(true)
        .write(true)
        .open(&temp_file_path)
    {
        Ok(f) => f,
        Err(e) => {
            return Ok(ReassemblyResult {
                ok: false,
                error: Some(format!("Failed to open temp file: {}", e)),
            });
        }
    };
    
    // Seek to the correct offset
    if let Err(e) = file.seek(SeekFrom::Start(offset)) {
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to seek to offset {}: {}", offset, e)),
        });
    }
    
    // Write the chunk data
    if let Err(e) = file.write_all(&bytes) {
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to write chunk data: {}", e)),
        });
    }
    
    // Flush to ensure data is written to disk
    if let Err(e) = file.flush() {
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to flush chunk data: {}", e)),
        });
    }
    
    // Optional: fsync for durability (can be configured)
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe {
            libc::fsync(file.as_raw_fd());
        }
    }
    
    println!("Written chunk {} ({} bytes) at offset {} for transfer {}", 
             chunk_index, bytes.len(), offset, transfer_id);
    
    Ok(ReassemblyResult { ok: true, error: None })
}

/// Verify file integrity and atomically move to final location
/// 
/// This function verifies the assembled file (checksum/merkle root if provided)
/// and atomically renames it to the final destination path.
#[command]
pub async fn verify_and_finalize(
    transfer_id: String,
    expected_root: Option<String>,
    final_path: String,
    tmp_path: String,
) -> Result<ReassemblyResult, String> {
    let temp_dir = std::env::temp_dir().join("chiral_transfers");
    let temp_file_path = temp_dir.join(format!("{}.tmp", transfer_id));
    
    // Check if temp file exists
    if !temp_file_path.exists() {
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Temp file not found for transfer {}", transfer_id)),
        });
    }
    
    // If expected_root is provided, verify file integrity
    if let Some(expected) = expected_root {
        match verify_file_hash(&temp_file_path, &expected).await {
            Ok(true) => {
                println!("File integrity verified for transfer {}", transfer_id);
            }
            Ok(false) => {
                return Ok(ReassemblyResult {
                    ok: false,
                    error: Some("File integrity verification failed - hash mismatch".to_string()),
                });
            }
            Err(e) => {
                return Ok(ReassemblyResult {
                    ok: false,
                    error: Some(format!("File integrity verification error: {}", e)),
                });
            }
        }
    }
    
    // Create parent directory for final path if needed
    let final_path_buf = PathBuf::from(&final_path);
    if let Some(parent) = final_path_buf.parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            return Ok(ReassemblyResult {
                ok: false,
                error: Some(format!("Failed to create destination directory: {}", e)),
            });
        }
    }
    
    // Atomic rename (move) from temp to final location
    if let Err(e) = fs::rename(&temp_file_path, &final_path).await {
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to move file to final location: {}", e)),
        });
    }
    
    println!("Successfully finalized transfer {} to {}", transfer_id, final_path);
    
    Ok(ReassemblyResult { ok: true, error: None })
}

/// Verify file hash against expected value
async fn verify_file_hash(file_path: &Path, expected_hash: &str) -> io::Result<bool> {
    let contents = fs::read(file_path).await?;
    let mut hasher = Sha256::new();
    hasher.update(&contents);
    let computed_hash = format!("{:x}", hasher.finalize());
    
    Ok(computed_hash.eq_ignore_ascii_case(expected_hash))
}

/// Optional: Save chunk bitmap for resume support
#[command]
pub async fn save_chunk_bitmap(
    transfer_id: String,
    received_chunks: Vec<u32>,
    total_chunks: u32,
) -> Result<ReassemblyResult, String> {
    let temp_dir = std::env::temp_dir().join("chiral_transfers");
    let bitmap_path = temp_dir.join(format!("{}.bitmap", transfer_id));
    
    // Create a simple bitmap format: JSON for now
    let bitmap_data = serde_json::json!({
        "transfer_id": transfer_id,
        "total_chunks": total_chunks,
        "received_chunks": received_chunks,
        "saved_at": chrono::Utc::now().to_rfc3339()
    });
    
    match fs::write(&bitmap_path, bitmap_data.to_string()).await {
        Ok(_) => Ok(ReassemblyResult { ok: true, error: None }),
        Err(e) => Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to save bitmap: {}", e)),
        }),
    }
}

/// Optional: Load chunk bitmap for resume support
#[command]
pub async fn load_chunk_bitmap(
    transfer_id: String,
) -> Result<Option<Vec<u32>>, String> {
    let temp_dir = std::env::temp_dir().join("chiral_transfers");
    let bitmap_path = temp_dir.join(format!("{}.bitmap", transfer_id));
    
    if !bitmap_path.exists() {
        return Ok(None);
    }
    
    match fs::read_to_string(&bitmap_path).await {
        Ok(content) => {
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(data) => {
                    if let Some(chunks) = data["received_chunks"].as_array() {
                        let received: Vec<u32> = chunks
                            .iter()
                            .filter_map(|v| v.as_u64().map(|n| n as u32))
                            .collect();
                        Ok(Some(received))
                    } else {
                        Ok(None)
                    }
                }
                Err(e) => Err(format!("Failed to parse bitmap: {}", e)),
            }
        }
        Err(e) => Err(format!("Failed to read bitmap: {}", e)),
    }
}

/// Cleanup temporary files for a transfer
#[command]
pub async fn cleanup_transfer_temp(transfer_id: String) -> Result<ReassemblyResult, String> {
    let temp_dir = std::env::temp_dir().join("chiral_transfers");
    let temp_file_path = temp_dir.join(format!("{}.tmp", transfer_id));
    let bitmap_path = temp_dir.join(format!("{}.bitmap", transfer_id));
    
    let mut errors = Vec::new();
    
    // Remove temp file if exists
    if temp_file_path.exists() {
        if let Err(e) = fs::remove_file(&temp_file_path).await {
            errors.push(format!("Failed to remove temp file: {}", e));
        }
    }
    
    // Remove bitmap if exists
    if bitmap_path.exists() {
        if let Err(e) = fs::remove_file(&bitmap_path).await {
            errors.push(format!("Failed to remove bitmap: {}", e));
        }
    }
    
    if errors.is_empty() {
        Ok(ReassemblyResult { ok: true, error: None })
    } else {
        Ok(ReassemblyResult {
            ok: false,
            error: Some(errors.join("; ")),
        })
    }
}
