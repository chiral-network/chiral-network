//! Network configuration — consensus-critical constants for each Chiral
//! Network deployment (testnet / mainnet).
//!
//! All blockchain parameters (chain id, genesis, bootstrap enode), libp2p
//! discovery (bootstrap + relay multiaddrs), and persistent state paths route
//! through the preset returned by [`active()`].
//!
//! ## Switching networks
//!
//! The active network is read from `CHIRAL_NETWORK` env var first, then from
//! `<data_dir>/chiral-network/active-network` on disk. The value is cached for
//! the lifetime of the process, so switching networks requires a restart
//! (intentional — geth chain state, DHT identity, and wallet tx metadata must
//! all swap atomically).
//!
//! Use [`set_active`] to change the choice; the new value takes effect on the
//! next launch.

use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Stable short identifier used in config files and the switch command.
    pub name: &'static str,
    /// Human-readable name for UI display.
    pub display_name: &'static str,
    pub chain_id: u64,
    pub network_id: u64,

    // Genesis block fields. The concatenation of these values determines the
    // genesis hash, so any change forks the network.
    pub genesis_difficulty: &'static str,
    pub genesis_extra_data: &'static str,
    pub genesis_nonce: &'static str,
    pub genesis_timestamp: &'static str,
    pub genesis_gas_limit: &'static str,
    pub genesis_coinbase: &'static str,

    /// Remote RPC endpoint used when local geth isn't running.
    pub rpc_fallback: &'static str,
    /// Geth devp2p bootstrap enode.
    pub geth_bootstrap_enode: &'static str,
    /// libp2p Kademlia bootstrap multiaddrs.
    pub libp2p_bootstrap_addrs: &'static [&'static str],
    /// libp2p relay nodes (circuit relay reservation targets).
    pub libp2p_relay_addrs: &'static [&'static str],

    /// `None` → persist state directly under `<data_dir>/chiral-network/` (the
    /// legacy unprefixed layout, kept for existing testnet installs).
    /// `Some("mainnet")` → persist under `<data_dir>/chiral-network/networks/mainnet/`
    /// so networks can't cross-contaminate each other's chain state, DHT
    /// identity, wallet tx history, or Drive files.
    pub data_subdir: Option<&'static str>,
}

pub const TESTNET: NetworkConfig = NetworkConfig {
    name: "testnet",
    display_name: "Testnet",
    chain_id: 98765,
    network_id: 98765,
    genesis_difficulty: "0x400000",
    genesis_extra_data: "0x4b656570206f6e206b656570696e67206f6e21",
    genesis_nonce: "0x0000000000000042",
    genesis_timestamp: "0x68b3b2ca",
    genesis_gas_limit: "0x47b760",
    genesis_coinbase: "0x0000000000000000000000000000000000000000",
    rpc_fallback: "http://130.245.173.73:8545",
    geth_bootstrap_enode: "enode://45cc5ba89142b2c82180986f411aa16dbfe6041043d1f7112f08e710f23fdeb7283551ec15ca9d23a0da91ac12e080e014f8c32230a8109d6d0b01be8ca71102@130.245.173.73:30303",
    libp2p_bootstrap_addrs: &[
        "/ip4/130.245.173.73/tcp/4001/p2p/12D3KooWEfUVEbmkeH5C7TUNDn26hQTqs5TBYvKZgrCGMJroHRF1",
        "/ip6/2002:82f5:ad49::1/tcp/4001/p2p/12D3KooWEfUVEbmkeH5C7TUNDn26hQTqs5TBYvKZgrCGMJroHRF1",
    ],
    libp2p_relay_addrs: &[
        "/ip4/130.245.173.73/tcp/4001/p2p/12D3KooWEfUVEbmkeH5C7TUNDn26hQTqs5TBYvKZgrCGMJroHRF1",
        "/ip6/2002:82f5:ad49::1/tcp/4001/p2p/12D3KooWEfUVEbmkeH5C7TUNDn26hQTqs5TBYvKZgrCGMJroHRF1",
    ],
    data_subdir: None,
};

/// All configured networks. Mainnet will be appended in a later PR once the
/// launch-day genesis (with NYT-headline commitment in `extraData`) is locked in.
pub const ALL: &[&NetworkConfig] = &[&TESTNET];

/// Currently active network. Resolved once per process from env/disk, cached.
pub fn active() -> &'static NetworkConfig {
    static CACHE: OnceLock<&'static NetworkConfig> = OnceLock::new();
    CACHE.get_or_init(resolve_from_env_or_disk)
}

fn resolve_from_env_or_disk() -> &'static NetworkConfig {
    let name = std::env::var("CHIRAL_NETWORK")
        .ok()
        .or_else(|| std::fs::read_to_string(active_network_file()).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    for cfg in ALL {
        if cfg.name == name {
            return cfg;
        }
    }
    &TESTNET
}

/// Write the active-network choice to disk. Takes effect on next launch.
pub fn set_active(name: &str) -> Result<(), String> {
    if !ALL.iter().any(|c| c.name == name) {
        return Err(format!("Unknown network: {}", name));
    }
    let path = active_network_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    std::fs::write(&path, name).map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(())
}

fn data_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chiral-network")
}

fn active_network_file() -> PathBuf {
    // Always at the root — the marker itself must live outside any network's
    // data_subdir, otherwise a fresh install on a new network would never see
    // the pre-existing choice.
    data_root().join("active-network")
}

/// Root persistent-state directory for the active network.
///
/// - Testnet (legacy): `<data_dir>/chiral-network/`
/// - Any other network: `<data_dir>/chiral-network/networks/<name>/`
///
/// All paths that currently write under `chiral-network/...` should be
/// anchored at this helper so network switches don't mix state.
pub fn data_dir() -> PathBuf {
    match active().data_subdir {
        Some(sub) => data_root().join("networks").join(sub),
        None => data_root(),
    }
}

/// Genesis JSON for the active network. Matches the format core-geth's
/// `init` command expects.
pub fn genesis_json(cfg: &NetworkConfig) -> String {
    serde_json::json!({
        "config": {
            "chainId": cfg.chain_id,
            "homesteadBlock": 0,
            "eip150Block": 0,
            "eip155Block": 0,
            "eip158Block": 0,
            "byzantiumBlock": 0,
            "constantinopleBlock": 0,
            "petersburgBlock": 0,
            "istanbulBlock": 0,
            "berlinBlock": 0,
            "londonBlock": 0,
            "ethash": {}
        },
        "difficulty": cfg.genesis_difficulty,
        "gasLimit": cfg.genesis_gas_limit,
        "alloc": {},
        "coinbase": cfg.genesis_coinbase,
        "extraData": cfg.genesis_extra_data,
        "nonce": cfg.genesis_nonce,
        "mixhash": "0x0000000000000000000000000000000000000000000000000000000000000000",
        "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
        "timestamp": cfg.genesis_timestamp
    })
    .to_string()
}
