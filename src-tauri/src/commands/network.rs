use serde::Serialize;
use crate::ethereum::{
    get_network_hashrate,
    get_network_difficulty_as_u64,
    get_peer_count,
};
use crate::get_power_consumption;
use futures::join;

#[derive(Serialize)]
pub struct FullNetworkStats {
    pub network_difficulty: f64,
    pub network_hashrate: f64,
    pub active_miners: u32,
    pub power_usage: f64,
    pub current_block: u64,
    pub peer_count: u32,
    pub blocks_mined: Option<u64>,
}

#[tauri::command]
pub async fn get_full_network_stats(app: tauri::AppHandle, address: Option<String>)-> Result<FullNetworkStats, String> {
    use futures::join;
    let power_usage = get_power_consumption().await.unwrap_or(0.0) as f64;
    let (hashrate_res, difficulty_res, peers_res) = join!(
        get_network_hashrate(),
        get_network_difficulty_as_u64(),
        get_peer_count(),
    );

    let hashrate_str = hashrate_res
    .map_err(|e| format!("Failed to get hashrate: {}", e))?;

    // Convert string to numeric value
    let hashrate = parse_hashrate(&hashrate_str).unwrap_or(0.0);

    let difficulty = difficulty_res
        .map_err(|e| format!("Failed to get difficulty: {}", e))?
        as f64;

    let active_miners = peers_res.unwrap_or(1); // prevent division by zero

    // Optionally get blocks mined for a given address
    let blocks_mined = if let Some(addr) = &address {
        Some(crate::get_total_mined_blocks(addr).await)
    } else {
        None
    };

    Ok(FullNetworkStats {
        network_difficulty: difficulty,
        network_hashrate: hashrate,
        active_miners,
        power_usage,
        current_block: 0, // optionally fetch eth_blockNumber
        peer_count: active_miners,
        blocks_mined,
    })
}

fn parse_hashrate(formatted: &str) -> Option<f64> {
    // Split the string into the number and the unit
    let parts: Vec<&str> = formatted.split_whitespace().collect();
    if parts.len() != 2 {
        return None;
    }

    let value: f64 = parts[0].parse().ok()?;
    let multiplier = match parts[1] {
        "H/s" => 1.0,
        "KH/s" => 1_000.0,
        "MH/s" => 1_000_000.0,
        "GH/s" => 1_000_000_000.0,
        _ => return None,
    };

    Some(value * multiplier)
}