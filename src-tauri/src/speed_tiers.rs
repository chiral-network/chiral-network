use std::path::Path;
use tauri::Emitter;

/// Fixed download cost: 0.01 CHI per MB
const COST_PER_MB_WEI: u128 = 10_000_000_000_000_000; // 0.01 CHI = 10^16 wei

/// Platform fee: 0.5% of all transactions
pub const PLATFORM_FEE_BPS: u128 = 50; // 50 basis points = 0.5%

/// Platform wallet address (receives the 0.5% fee)
pub const PLATFORM_WALLET: &str = "0x9ad90a2f8b72092154a5b5259295e33df3541ede";

/// Calculate total download cost in wei for a given file size (before platform fee)
pub fn calculate_cost(file_size_bytes: u64) -> u128 {
    if file_size_bytes == 0 {
        return 0;
    }
    let size = file_size_bytes as u128;
    (size * COST_PER_MB_WEI + 999_999) / 1_000_000 // Round up to nearest wei
}

/// Calculate the platform fee (0.5%) on a given amount in wei.
pub fn calculate_platform_fee(amount_wei: u128) -> u128 {
    (amount_wei * PLATFORM_FEE_BPS + 9999) / 10000 // Round up
}

/// Calculate total cost including platform fee.
#[allow(dead_code)]
pub fn calculate_total_with_fee(base_cost_wei: u128) -> u128 {
    base_cost_wei + calculate_platform_fee(base_cost_wei)
}

/// Split a payment amount into seller portion and platform fee.
/// Returns (seller_amount, platform_fee).
pub fn split_payment(total_wei: u128) -> (u128, u128) {
    let fee = calculate_platform_fee(total_wei);
    (total_wei.saturating_sub(fee), fee)
}

/// Format wei amount as CHI string for display
pub fn format_wei_as_chi(wei: u128) -> String {
    let whole = wei / 1_000_000_000_000_000_000;
    let frac = wei % 1_000_000_000_000_000_000;
    if frac == 0 {
        format!("{}", whole)
    } else {
        let frac_str = format!("{:018}", frac);
        let trimmed = frac_str.trim_end_matches('0');
        let decimals = if trimmed.len() > 6 {
            &trimmed[..6]
        } else {
            trimmed
        };
        format!("{}.{}", whole, decimals)
    }
}

/// Write file data at full speed (no rate limiting).
/// Emits `download-progress` events during the write.
pub async fn write_file(
    app: &tauri::AppHandle,
    file_path: &Path,
    file_data: &[u8],
    request_id: &str,
    file_hash: &str,
    file_name: &str,
) -> Result<(), String> {
    let total_bytes = file_data.len();

    std::fs::write(file_path, file_data)
        .map_err(|e| format!("Failed to write file: {}", e))?;

    let _ = app.emit(
        "download-progress",
        serde_json::json!({
            "requestId": request_id,
            "fileHash": file_hash,
            "fileName": file_name,
            "bytesWritten": total_bytes,
            "totalBytes": total_bytes,
            "speedBps": 0,
            "progress": 100.0
        }),
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cost_calculation() {
        // 10 MB * 0.01 CHI/MB = 0.1 CHI = 10^17 wei
        assert_eq!(calculate_cost(10_000_000), 100_000_000_000_000_000);
    }

    #[test]
    fn test_cost_calculation_small_file() {
        assert!(calculate_cost(1) > 0);
    }

    #[test]
    fn test_cost_calculation_zero_bytes() {
        assert_eq!(calculate_cost(0), 0);
    }

    #[test]
    fn test_cost_calculation_1mb() {
        // 1 MB = 0.01 CHI = 10^16 wei
        assert_eq!(calculate_cost(1_000_000), 10_000_000_000_000_000);
    }

    #[test]
    fn test_platform_fee() {
        // 0.5% of 1 CHI (10^18 wei) = 0.005 CHI = 5*10^15 wei
        let fee = calculate_platform_fee(1_000_000_000_000_000_000);
        assert_eq!(fee, 5_000_000_000_000_000);
    }

    #[test]
    fn test_split_payment() {
        let total = 1_000_000_000_000_000_000u128; // 1 CHI
        let (seller, fee) = split_payment(total);
        assert_eq!(fee, 5_000_000_000_000_000); // 0.005 CHI
        assert_eq!(seller + fee, total);
    }

    #[test]
    fn test_total_with_fee() {
        let base = 10_000_000_000_000_000u128; // 0.01 CHI
        let total = calculate_total_with_fee(base);
        assert!(total > base);
        assert_eq!(total, base + calculate_platform_fee(base));
    }

    #[test]
    fn test_format_wei_as_chi() {
        assert_eq!(format_wei_as_chi(0), "0");
        assert_eq!(format_wei_as_chi(1_000_000_000_000_000_000), "1");
        assert_eq!(format_wei_as_chi(1_500_000_000_000_000_000), "1.5");
        assert_eq!(format_wei_as_chi(10_000_000_000_000_000), "0.01");
    }

    #[test]
    fn test_format_wei_as_chi_large_amount() {
        assert_eq!(format_wei_as_chi(1_000_000_000_000_000_000_000), "1000");
    }

    #[test]
    fn test_format_wei_as_chi_one_wei() {
        let result = format_wei_as_chi(1);
        assert!(result.starts_with("0."));
        assert!(result.len() > 2);
    }
}
