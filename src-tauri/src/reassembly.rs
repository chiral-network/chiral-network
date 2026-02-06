use fs2::FileExt; // Add this import for file locking
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tauri::command;
use tokio::fs;
use tokio::io::AsyncReadExt;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyFinalizeRequest {
    pub transfer_id: String,
    // renamed from expected_root to expected_sha256 to clarify semantics
    pub expected_sha256: Option<String>,
    pub final_path: String,
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
    bytes: serde_json::Value,
    chunk_checksum: Option<String>, // NEW optional checksum param (hex sha256)
) -> Result<ReassemblyResult, String> {
    let temp_dir = std::env::temp_dir().join("chiral_transfers");

    if let Err(e) = fs::create_dir_all(&temp_dir).await {
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to create temp directory: {}", e)),
        });
    }
    
    let temp_file_path = temp_dir.join(format!("{}.tmp", transfer_id));
    
    // Decode incoming bytes which may be provided as a JSON array of numbers or a base64 string.
    let bytes_vec: Vec<u8> = match bytes {
        serde_json::Value::Array(arr) => {
            let mut v = Vec::with_capacity(arr.len());
            for item in arr.iter() {
                if let Some(n) = item.as_u64() {
                    v.push(n as u8);
                } else {
                    return Ok(ReassemblyResult { ok: false, error: Some("Invalid byte array element".to_string()) });
                }
            }
            v
        }
        serde_json::Value::String(s) => {
            match base64::decode(&s) {
                Ok(v) => v,
                Err(e) => return Ok(ReassemblyResult { ok: false, error: Some(format!("Failed to decode base64 bytes: {}", e)) }),
            }
        }
        serde_json::Value::Object(map) => {
            // Support `{ data: [...] }` shapes sometimes emitted
            if let Some(data_val) = map.get("data") {
                if let Some(arr) = data_val.as_array() {
                    let mut v = Vec::with_capacity(arr.len());
                    for item in arr.iter() {
                        if let Some(n) = item.as_u64() {
                            v.push(n as u8);
                        } else {
                            return Ok(ReassemblyResult { ok: false, error: Some("Invalid byte array element in object.data".to_string()) });
                        }
                    }
                    v
                } else if let Some(s) = data_val.as_str() {
                    match base64::decode(s) {
                        Ok(v) => v,
                        Err(e) => return Ok(ReassemblyResult { ok: false, error: Some(format!("Failed to decode base64 in object.data: {}", e)) }),
                    }
                } else {
                    return Ok(ReassemblyResult { ok: false, error: Some("Unsupported object.data format".to_string()) });
                }
            } else {
                return Ok(ReassemblyResult { ok: false, error: Some("Unsupported bytes object format".to_string()) });
            }
        }
        _ => return Ok(ReassemblyResult { ok: false, error: Some("Unsupported bytes format".to_string()) }),
    };
    
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

    // Verify checksum if provided BEFORE writing (compute SHA256 of provided bytes)
    if let Some(expected_hex) = chunk_checksum {
        // compute sha256 hex of bytes
        let mut hasher = Sha256::new();
        hasher.update(&bytes_vec);
        let computed = format!("{:x}", hasher.finalize());
        if !computed.eq_ignore_ascii_case(&expected_hex) {
            return Ok(ReassemblyResult {
                ok: false,
                error: Some(format!(
                    "Chunk checksum mismatch for transfer {} chunk {}: expected {}, got {}",
                    transfer_id, chunk_index, expected_hex, computed
                )),
            });
        }
    }

    // Acquire an exclusive lock on the temp file to serialize writes to the file itself
    if let Err(e) = file.lock_exclusive() {
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to acquire file lock: {}", e)),
        });
    }

    // Seek to the correct offset
    if let Err(e) = file.seek(SeekFrom::Start(offset)) {
        let _ = file.unlock();
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to seek to offset {}: {}", offset, e)),
        });
    }

    // Write the chunk data
    if let Err(e) = file.write_all(&bytes_vec) {
        let _ = file.unlock();
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to write chunk data: {}", e)),
        });
    }

    // Flush to ensure data is written to disk
    if let Err(e) = file.flush() {
        let _ = file.unlock();
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

    // We have finished writing to the temp file; release the file lock before bitmap update
    let _ = file.unlock();

    // Update bitmap atomically and with a per-transfer lock to avoid read-modify-write races
    if let Err(e) = update_bitmap_add_chunk(&temp_dir, &transfer_id, chunk_index).await {
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to update bitmap: {}", e)),
        });
    }

    println!("Written chunk {} ({} bytes) at offset {} for transfer {}", 
             chunk_index, bytes_vec.len(), offset, transfer_id);
    
    Ok(ReassemblyResult { ok: true, error: None })
}

/// Verify file integrity and atomically move to final location
/// 
/// This function verifies the assembled file (checksum/merkle root if provided)
/// and atomically renames it to the final destination path.
#[command]
pub async fn verify_and_finalize(
    transfer_id: String,
    expected_sha256: Option<String>,
    final_path: String,
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

    // If expected_sha256 is provided, verify file integrity
    if let Some(expected) = expected_sha256 {
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

/// Read-modify-write helper that acquires a per-transfer lock, reads the bitmap,
/// adds the given chunk index if missing, and writes back atomically.
async fn update_bitmap_add_chunk(
    temp_dir: &Path,
    transfer_id: &str,
    chunk_index: u32,
) -> Result<(), String> {
    let bitmap_path = temp_dir.join(format!("{}.bitmap", transfer_id));
    let tmp_bitmap_path = temp_dir.join(format!("{}.bitmap.tmp", transfer_id));
    let lock_path = temp_dir.join(format!("{}.bitmap.lock", transfer_id));

    // Ensure temp dir exists
    if let Err(e) = fs::create_dir_all(&temp_dir).await {
        return Err(format!("Failed to create temp dir for bitmap: {}", e));
    }

    // Open or create a lock file for this transfer
    let lock_file = match OpenOptions::new().create(true).write(true).open(&lock_path) {
        Ok(f) => f,
        Err(e) => return Err(format!("Failed to open bitmap lock file: {}", e)),
    };

    // Acquire exclusive lock
    if let Err(e) = lock_file.lock_exclusive() {
        return Err(format!("Failed to lock bitmap file: {}", e));
    }

    // Read existing bitmap JSON
    let bitmap_json = if bitmap_path.exists() {
        match fs::read_to_string(&bitmap_path).await {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(val) => val,
                Err(_) => serde_json::json!({}),
            },
            Err(_) => serde_json::json!({}),
        }
    } else {
        serde_json::json!({})
    };

    // Extract received_chunks
    let mut received: Vec<u32> = bitmap_json["received_chunks"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_u64().map(|n| n as u32))
                .collect()
        })
        .unwrap_or_else(Vec::new);

    if !received.contains(&chunk_index) {
        received.push(chunk_index);
    }

    let total_chunks_val = bitmap_json.get("total_chunks").cloned();

    let mut new_bitmap = serde_json::json!({
        "transfer_id": transfer_id,
        "received_chunks": received,
        "saved_at": chrono::Utc::now().to_rfc3339()
    });

    if let Some(tc) = total_chunks_val {
        new_bitmap["total_chunks"] = tc;
    }

    // Write to tmp then rename
    if let Err(e) = fs::write(&tmp_bitmap_path, new_bitmap.to_string()).await {
        let _ = lock_file.unlock();
        return Err(format!("Failed to write tmp bitmap: {}", e));
    }

    if let Err(e) = fs::rename(&tmp_bitmap_path, &bitmap_path).await {
        let _ = lock_file.unlock();
        return Err(format!("Failed to rename tmp bitmap into place: {}", e));
    }

    // Release the lock
    let _ = lock_file.unlock();

    Ok(())
}

/// Verify file hash against expected value using streaming reads
async fn verify_file_hash(file_path: &Path, expected_hash: &str) -> io::Result<bool> {
    let mut file = tokio::fs::File::open(file_path).await?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];

    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

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
    let tmp_bitmap_path = temp_dir.join(format!("{}.bitmap.tmp", transfer_id));
    let lock_path = temp_dir.join(format!("{}.lock", transfer_id));

    // Open or create a lock file for this transfer
    let lock_file = match OpenOptions::new().create(true).write(true).open(&lock_path) {
        Ok(file) => file,
        Err(e) => {
            return Ok(ReassemblyResult {
                ok: false,
                error: Some(format!("Failed to open lock file: {}", e)),
            });
        }
    };

    // Acquire an exclusive lock on the file
    if let Err(e) = lock_file.lock_exclusive() {
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to acquire lock: {}", e)),
        });
    }

    // Create a simple bitmap format: JSON for now
    let bitmap_data = serde_json::json!({
        "transfer_id": transfer_id,
        "total_chunks": total_chunks,
        "received_chunks": received_chunks,
        "saved_at": chrono::Utc::now().to_rfc3339()
    });

    // Write to a temporary file first
    if let Err(e) = fs::write(&tmp_bitmap_path, bitmap_data.to_string()).await {
        // Release the lock before returning
        let _ = lock_file.unlock();
        return Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to save bitmap temp file: {}", e)),
        });
    }

    // Rename the temporary file to the final bitmap file atomically
    let result = fs::rename(&tmp_bitmap_path, &bitmap_path).await;

    // Release the lock
    let _ = lock_file.unlock();

    match result {
        Ok(_) => Ok(ReassemblyResult { ok: true, error: None }),
        Err(e) => Ok(ReassemblyResult {
            ok: false,
            error: Some(format!("Failed to atomically save bitmap: {}", e)),
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
