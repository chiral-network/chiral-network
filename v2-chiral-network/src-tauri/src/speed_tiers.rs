use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::Emitter;
use tokio::io::AsyncWriteExt;

/// Download speed tiers - free is rate-limited, paid tiers are faster
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SpeedTier {
    Free,
    Standard,
    Premium,
}

impl SpeedTier {
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "free" => Ok(SpeedTier::Free),
            "standard" => Ok(SpeedTier::Standard),
            "premium" => Ok(SpeedTier::Premium),
            _ => Err(format!("Unknown speed tier: {}", s)),
        }
    }

    /// Returns the speed limit in bytes per second, or None for unlimited
    pub fn bytes_per_second(&self) -> Option<usize> {
        match self {
            SpeedTier::Free => Some(100 * 1024),      // 100 KB/s
            SpeedTier::Standard => Some(1024 * 1024),  // 1 MB/s
            SpeedTier::Premium => None,                 // Unlimited
        }
    }

    /// Cost per MB in wei (1 CHI = 10^18 wei)
    pub fn cost_per_mb_wei(&self) -> u128 {
        match self {
            SpeedTier::Free => 0,
            SpeedTier::Standard => 1_000_000_000_000_000,   // 0.001 CHI = 10^15 wei
            SpeedTier::Premium => 5_000_000_000_000_000,    // 0.005 CHI = 5*10^15 wei
        }
    }
}

/// Calculate total download cost in wei for a given tier and file size
pub fn calculate_cost(tier: &SpeedTier, file_size_bytes: u64) -> u128 {
    let cost_per_mb = tier.cost_per_mb_wei();
    if cost_per_mb == 0 {
        return 0;
    }
    // Calculate: (file_size_bytes * cost_per_mb) / 1_000_000
    // Use u128 to avoid overflow
    let size = file_size_bytes as u128;
    (size * cost_per_mb + 999_999) / 1_000_000 // Round up to nearest wei
}

/// Format wei amount as CHI string for display
pub fn format_wei_as_chi(wei: u128) -> String {
    let whole = wei / 1_000_000_000_000_000_000;
    let frac = wei % 1_000_000_000_000_000_000;
    if frac == 0 {
        format!("{}", whole)
    } else {
        // Format with up to 6 decimal places
        let frac_str = format!("{:018}", frac);
        let trimmed = frac_str.trim_end_matches('0');
        let decimals = if trimmed.len() > 6 { &trimmed[..6] } else { trimmed };
        format!("{}.{}", whole, decimals)
    }
}

/// Calculate the delay between chunk requests for rate limiting.
/// Returns None for unlimited (Premium) tier.
pub fn chunk_request_delay(chunk_size: u32, tier: &SpeedTier) -> Option<std::time::Duration> {
    tier.bytes_per_second().map(|bps| {
        let delay_us = (chunk_size as u64 * 1_000_000) / bps as u64;
        std::time::Duration::from_micros(delay_us)
    })
}

const CHUNK_SIZE: usize = 8192; // 8KB chunks (for rate_limited_write)
const PROGRESS_INTERVAL: usize = 65536; // Emit progress every 64KB

/// Write file data with rate limiting based on speed tier.
/// Emits `download-progress` events during the write.
pub async fn rate_limited_write(
    app: &tauri::AppHandle,
    file_path: &Path,
    file_data: &[u8],
    tier: &SpeedTier,
    request_id: &str,
    file_hash: &str,
    file_name: &str,
) -> Result<(), String> {
    let total_bytes = file_data.len();

    match tier.bytes_per_second() {
        None => {
            // Premium: write all at once, no rate limiting
            std::fs::write(file_path, file_data)
                .map_err(|e| format!("Failed to write file: {}", e))?;

            let _ = app.emit("download-progress", serde_json::json!({
                "requestId": request_id,
                "fileHash": file_hash,
                "fileName": file_name,
                "bytesWritten": total_bytes,
                "totalBytes": total_bytes,
                "speedBps": 0,
                "progress": 100.0
            }));
        }
        Some(speed_limit) => {
            // Rate-limited write: write in chunks with delays
            let mut file = tokio::fs::File::create(file_path).await
                .map_err(|e| format!("Failed to create file: {}", e))?;

            let mut bytes_written: usize = 0;
            let mut last_progress_bytes: usize = 0;
            let start_time = std::time::Instant::now();

            // Calculate delay per chunk: chunk_size / bytes_per_second (in microseconds)
            let delay_us = (CHUNK_SIZE as u64 * 1_000_000) / speed_limit as u64;

            for chunk in file_data.chunks(CHUNK_SIZE) {
                file.write_all(chunk).await
                    .map_err(|e| format!("Failed to write chunk: {}", e))?;

                bytes_written += chunk.len();

                // Emit progress event periodically
                if bytes_written - last_progress_bytes >= PROGRESS_INTERVAL || bytes_written == total_bytes {
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let speed_bps = if elapsed > 0.0 {
                        (bytes_written as f64 / elapsed) as u64
                    } else {
                        0
                    };
                    let progress = (bytes_written as f64 / total_bytes as f64) * 100.0;

                    let _ = app.emit("download-progress", serde_json::json!({
                        "requestId": request_id,
                        "fileHash": file_hash,
                        "fileName": file_name,
                        "bytesWritten": bytes_written,
                        "totalBytes": total_bytes,
                        "speedBps": speed_bps,
                        "progress": progress
                    }));

                    last_progress_bytes = bytes_written;
                }

                // Sleep to enforce rate limit (skip on last chunk)
                if bytes_written < total_bytes {
                    tokio::time::sleep(tokio::time::Duration::from_micros(delay_us)).await;
                }
            }

            file.flush().await
                .map_err(|e| format!("Failed to flush file: {}", e))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_from_str() {
        assert_eq!(SpeedTier::from_str("free").unwrap(), SpeedTier::Free);
        assert_eq!(SpeedTier::from_str("standard").unwrap(), SpeedTier::Standard);
        assert_eq!(SpeedTier::from_str("premium").unwrap(), SpeedTier::Premium);
        assert_eq!(SpeedTier::from_str("Free").unwrap(), SpeedTier::Free);
        assert!(SpeedTier::from_str("invalid").is_err());
    }

    #[test]
    fn test_speed_limits() {
        assert_eq!(SpeedTier::Free.bytes_per_second(), Some(102_400));
        assert_eq!(SpeedTier::Standard.bytes_per_second(), Some(1_048_576));
        assert_eq!(SpeedTier::Premium.bytes_per_second(), None);
    }

    #[test]
    fn test_cost_calculation() {
        // Free tier: always 0
        assert_eq!(calculate_cost(&SpeedTier::Free, 10_000_000), 0);

        // Standard: 10 MB * 0.001 CHI/MB = 0.01 CHI = 10^16 wei
        assert_eq!(calculate_cost(&SpeedTier::Standard, 10_000_000), 10_000_000_000_000_000);

        // Premium: 10 MB * 0.005 CHI/MB = 0.05 CHI = 5*10^16 wei
        assert_eq!(calculate_cost(&SpeedTier::Premium, 10_000_000), 50_000_000_000_000_000);

        // Small file: 1 byte should still have non-zero cost for paid tiers (rounds up)
        assert!(calculate_cost(&SpeedTier::Standard, 1) > 0);
    }

    #[test]
    fn test_format_wei_as_chi() {
        assert_eq!(format_wei_as_chi(0), "0");
        assert_eq!(format_wei_as_chi(1_000_000_000_000_000_000), "1");
        assert_eq!(format_wei_as_chi(1_500_000_000_000_000_000), "1.5");
        assert_eq!(format_wei_as_chi(10_000_000_000_000_000), "0.01");
    }
}
