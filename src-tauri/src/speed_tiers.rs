use std::path::Path;
use tauri::Emitter;

/// Fixed download cost: 0.01 CHI per MB
pub const LAUNCH_DOWNLOAD_COST_PER_MB_WEI: u128 = 10_000_000_000_000_000; // 0.01 CHI = 10^16 wei
pub const LAUNCH_DOWNLOAD_COST_PER_MB_CHI: &str = "0.01";
const DECIMAL_BYTES_PER_MB: u128 = 1_000_000;

/// Platform fee: 0.5% of all transactions
pub const PLATFORM_FEE_BPS: u128 = 50; // 50 basis points = 0.5%
const BASIS_POINTS_DENOMINATOR: u128 = 10_000;

/// Platform wallet address (receives the 0.5% fee)
pub const PLATFORM_WALLET: &str = "0x9ad90a2f8b72092154a5b5259295e33df3541ede";

/// Calculate total download cost in wei for a given file size (before platform fee)
pub fn calculate_cost(file_size_bytes: u64) -> u128 {
    if file_size_bytes == 0 {
        return 0;
    }
    let size = file_size_bytes as u128;
    (size * LAUNCH_DOWNLOAD_COST_PER_MB_WEI + DECIMAL_BYTES_PER_MB - 1) / DECIMAL_BYTES_PER_MB
}

/// Calculate the platform fee (0.5%) on a given amount in wei. The fee
/// is a CUT of the listed price, not a markup added on top — i.e.
/// buyer pays `total`, seller receives `total - fee`, platform receives
/// `fee`. Always rounded up so the platform never under-collects.
pub fn calculate_platform_fee(amount_wei: u128) -> u128 {
    (amount_wei * PLATFORM_FEE_BPS + BASIS_POINTS_DENOMINATOR - 1) / BASIS_POINTS_DENOMINATOR
}

/// Split a payment into `(seller_amount, platform_fee)`. The two
/// components always sum to `total_wei` exactly — `split_payment` is
/// the single source of truth for how the listed price is divided.
///
/// (FM-A24: a previous `calculate_total_with_fee` helper added a fee on
/// top of the base cost, producing a non-invertible round-trip with
/// `split_payment`. It was unused in the runtime payment flow — the
/// real convention has always been "buyer pays the listed price, the
/// fee is a cut" — and was removed to eliminate the inconsistency.)
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

    std::fs::write(file_path, file_data).map_err(|e| format!("Failed to write file: {}", e))?;

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
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SharedLaunchFeePolicy {
        download_cost_per_mb_chi: String,
        download_cost_per_mb_wei: String,
        platform_fee_bps: u128,
        platform_wallet: String,
    }

    #[test]
    fn test_shared_launch_fee_policy_matches_backend_constants() {
        let policy: SharedLaunchFeePolicy =
            serde_json::from_str(include_str!("../../src/lib/launchFeePolicy.json")).unwrap();

        assert_eq!(
            policy.download_cost_per_mb_chi,
            LAUNCH_DOWNLOAD_COST_PER_MB_CHI
        );
        assert_eq!(
            policy.download_cost_per_mb_wei.parse::<u128>().unwrap(),
            LAUNCH_DOWNLOAD_COST_PER_MB_WEI
        );
        assert_eq!(policy.platform_fee_bps, PLATFORM_FEE_BPS);
        assert_eq!(policy.platform_wallet, PLATFORM_WALLET);
    }

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
    fn test_split_payment_invariant_holds_across_inputs() {
        // FM-A24 regression: seller + fee must equal total exactly.
        for total in [
            0u128,
            1,
            199,
            20_099,
            10_000_000_000_000_000,
            1_234_567_890_987_654_321,
        ] {
            let (seller, fee) = split_payment(total);
            assert_eq!(
                seller + fee,
                total,
                "split_payment({}) breaks invariant",
                total
            );
        }
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
