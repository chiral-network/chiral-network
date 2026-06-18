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

    /// Public relay base URL used by frontend/runtime services.
    pub relay_base_url: &'static str,
    /// Reputation API base URL. Usually the same deployment as the relay.
    pub rating_base_url: &'static str,
    /// Drive relay base URL used for public share registration and web CRUD fallback.
    pub drive_relay_base_url: &'static str,
    /// CDN search endpoints queried alongside DHT search results.
    pub cdn_search_base_urls: &'static [&'static str],
    /// CDN servers exposed in the Hosts page.
    pub cdn_servers: &'static [CdnEndpointConfig],

    /// `None` → persist state directly under `<data_dir>/chiral-network/` (the
    /// legacy unprefixed layout, kept for existing testnet installs).
    /// `Some("mainnet")` → persist under `<data_dir>/chiral-network/networks/mainnet/`
    /// so networks can't cross-contaminate each other's chain state, DHT
    /// identity, wallet tx history, or Drive files.
    pub data_subdir: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct CdnEndpointConfig {
    pub url: &'static str,
    pub name: &'static str,
    pub region: &'static str,
}

pub const LAUNCH_RELAY_BASE_URL: &str = "http://130.245.173.73:8080";
pub const LAUNCH_CDN_SERVERS: &[CdnEndpointConfig] = &[
    CdnEndpointConfig {
        url: "http://130.245.173.73:9420",
        name: "CDN Primary (US East)",
        region: "New York",
    },
    CdnEndpointConfig {
        url: "http://130.245.173.231:9420",
        name: "CDN Secondary (US East)",
        region: "Stony Brook",
    },
];
pub const LAUNCH_CDN_SEARCH_BASE_URLS: &[&str] =
    &["http://130.245.173.73:9420", "http://130.245.173.231:9420"];

pub const TESTNET: NetworkConfig = NetworkConfig {
    name: "testnet",
    display_name: "Testnet (legacy)",
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
    relay_base_url: LAUNCH_RELAY_BASE_URL,
    rating_base_url: LAUNCH_RELAY_BASE_URL,
    drive_relay_base_url: LAUNCH_RELAY_BASE_URL,
    cdn_search_base_urls: LAUNCH_CDN_SEARCH_BASE_URLS,
    cdn_servers: LAUNCH_CDN_SERVERS,
    // Legacy unprefixed layout — existing installs already live here.
    data_subdir: None,
};

/// Fresh production chain. New chain id, fresh genesis, low starting
/// difficulty so the first few blocks come quickly. Bootstrap and RPC
/// fallback both point at the CDN primary on `.73` — that server runs a
/// public geth on the freshnet chain that every client syncs from, so
/// wallets without a local geth can still read state and CDN payment
/// verification works (same chain on every side).
pub const FRESHNET: NetworkConfig = NetworkConfig {
    name: "freshnet",
    display_name: "Freshnet",
    chain_id: 98763,
    network_id: 98763,
    // Low difficulty so solo mining reaches block ~10 in seconds, not minutes.
    // Ethash adjusts upward toward target block time after the first window.
    genesis_difficulty: "0x10000",
    // hex("Chiral Freshnet v1")
    genesis_extra_data: "0x43686972616c2046726573686e65742076310000000000000000000000000000",
    genesis_nonce: "0x0000000000000043",
    genesis_timestamp: "0x0",
    genesis_gas_limit: "0x47b760",
    genesis_coinbase: "0x0000000000000000000000000000000000000000",
    rpc_fallback: "http://130.245.173.73:8545",
    geth_bootstrap_enode: "enode://2fa9c8979d1b780ca3a3a366fd1bf132259ecb82caaf6bfb6bc8d4d50dc5e2ac5abcffb901d7bdb3983c4060e2fe39a8ccc2fc8ffaa7a87dc5e45f7c6aaa232e@130.245.173.73:30303",
    // libp2p bootstrap + relay: .73 runs relay-server.service on :4001
    // with a stable peer identity. Empty lists here meant clients had no
    // entry point to the DHT and their routing tables stayed empty —
    // get_providers returned nothing for every hash and search never
    // surfaced any seeders.
    libp2p_bootstrap_addrs: &[
        "/ip4/130.245.173.73/tcp/4001/p2p/12D3KooWEfUVEbmkeH5C7TUNDn26hQTqs5TBYvKZgrCGMJroHRF1",
        "/ip6/2002:82f5:ad49::1/tcp/4001/p2p/12D3KooWEfUVEbmkeH5C7TUNDn26hQTqs5TBYvKZgrCGMJroHRF1",
    ],
    libp2p_relay_addrs: &[
        "/ip4/130.245.173.73/tcp/4001/p2p/12D3KooWEfUVEbmkeH5C7TUNDn26hQTqs5TBYvKZgrCGMJroHRF1",
        "/ip6/2002:82f5:ad49::1/tcp/4001/p2p/12D3KooWEfUVEbmkeH5C7TUNDn26hQTqs5TBYvKZgrCGMJroHRF1",
    ],
    relay_base_url: LAUNCH_RELAY_BASE_URL,
    rating_base_url: LAUNCH_RELAY_BASE_URL,
    drive_relay_base_url: LAUNCH_RELAY_BASE_URL,
    cdn_search_base_urls: LAUNCH_CDN_SEARCH_BASE_URLS,
    cdn_servers: LAUNCH_CDN_SERVERS,
    data_subdir: Some("freshnet"),
};

/// All configured networks. FRESHNET is first so it's the default for new
/// installs; existing testnet users keep their data under `networks/testnet/`
/// (the migration is automatic because TESTNET now has its own data_subdir).
pub const ALL: &[&NetworkConfig] = &[&FRESHNET, &TESTNET];

/// Currently active network. Resolved once per process from env/disk, cached.
pub fn active() -> &'static NetworkConfig {
    static CACHE: OnceLock<&'static NetworkConfig> = OnceLock::new();
    CACHE.get_or_init(resolve_from_env_or_disk)
}

fn resolve_from_env_or_disk() -> &'static NetworkConfig {
    let selector = std::env::var("CHIRAL_NETWORK")
        .ok()
        .map(|value| ("CHIRAL_NETWORK", value))
        .or_else(|| {
            std::fs::read_to_string(active_network_file())
                .ok()
                .map(|value| ("active-network", value))
        });
    let (cfg, warning) = resolve_config_from_selector(
        selector
            .as_ref()
            .map(|(source, value)| (*source, value.as_str())),
    );
    if let Some(warning) = warning {
        eprintln!("[NETWORK] {}", warning);
    }
    cfg
}

fn resolve_config_from_selector(
    selector: Option<(&str, &str)>,
) -> (&'static NetworkConfig, Option<String>) {
    let Some((source, raw_name)) = selector else {
        return (ALL[0], None);
    };

    let name = raw_name.trim();
    if name.is_empty() {
        return (ALL[0], None);
    }

    for cfg in ALL {
        if cfg.name == name {
            return (cfg, None);
        }
    }
    let default = ALL[0];
    (
        default,
        Some(format!(
            "Unknown {} network selector '{}'; using default network '{}'",
            source, name, default.name
        )),
    )
}

/// Write the active-network choice to disk. Takes effect on next launch.
pub fn set_active(name: &str) -> Result<(), String> {
    if !ALL.iter().any(|c| c.name == name) {
        return Err(format!("Unknown network: {}", name));
    }
    let path = active_network_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_network_selector_uses_default_without_warning() {
        let (cfg, warning) = resolve_config_from_selector(None);

        assert_eq!(cfg.name, FRESHNET.name);
        assert!(warning.is_none());
    }

    #[test]
    fn valid_network_selector_uses_matching_config_without_warning() {
        let (cfg, warning) = resolve_config_from_selector(Some(("CHIRAL_NETWORK", "testnet")));

        assert_eq!(cfg.name, TESTNET.name);
        assert!(warning.is_none());
    }

    #[test]
    fn invalid_network_selector_warns_and_uses_default() {
        let (cfg, warning) = resolve_config_from_selector(Some(("active-network", "unknownnet")));

        assert_eq!(cfg.name, FRESHNET.name);
        let warning = warning.expect("invalid selector should be surfaced");
        assert!(warning.contains("unknownnet"));
        assert!(warning.contains("active-network"));
        assert!(warning.contains(FRESHNET.name));
    }
}
