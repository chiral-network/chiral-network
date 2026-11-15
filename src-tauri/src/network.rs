//! Explicit Conflux eSpace public-testnet identity; no implicit endpoint or mainnet fallback.
use crate::protocol::NetworkIdentity;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const ESPACE_TESTNET_CHAIN_ID: u64 = 71;
pub const ESPACE_TESTNET_GENESIS: &str =
    "0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkConfig {
    pub identity: NetworkIdentity,
    pub rpc_url: String,
    pub rpc_bearer_file: Option<PathBuf>,
    pub faucet_url: Option<String>,
    pub explorer_url: Option<String>,
    pub policy_url: Option<String>,
}
impl std::fmt::Debug for NetworkConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkConfig")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}
impl NetworkConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let metadata = std::fs::metadata(path).map_err(|_| "cannot read network profile")?;
        if metadata.len() > 64 * 1024 {
            return Err("network profile exceeds size limit".into());
        }
        let config: Self = serde_json::from_slice(
            &std::fs::read(path).map_err(|_| "cannot read network profile")?,
        )
        .map_err(|_| "invalid network profile")?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.identity.chain_id != ESPACE_TESTNET_CHAIN_ID
            || self.identity.genesis_hash != ESPACE_TESTNET_GENESIS
        {
            return Err("this beta requires the Conflux eSpace public testnet (chain 71 and its pinned genesis); mainnet and private chains are not enabled".into());
        }
        validate_hash(&self.identity.genesis_hash)?;
        if self
            .identity
            .genesis_hash
            .bytes()
            .any(|b| b.is_ascii_uppercase())
        {
            return Err("genesis hash must be lowercase to keep deployment and journal identities canonical".into());
        }
        let id = &self.identity.deployment_id;
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(
                "deployment id must be 1..128 ASCII letters, digits, hyphens or underscores".into(),
            );
        }
        validate_endpoint(&self.rpc_url)?;
        for url in [&self.faucet_url, &self.explorer_url, &self.policy_url]
            .into_iter()
            .flatten()
        {
            validate_endpoint(url)?;
        }
        Ok(())
    }
    pub fn namespace(&self) -> String {
        // Hash the complete identity, including deployment length boundaries.
        crate::protocol::hash_hex(
            &crate::protocol::canonical_json(&self.identity)
                .expect("network identity is serializable"),
        )[2..]
            .to_owned()
    }
    pub fn scoped_dir(&self, base: &Path) -> PathBuf {
        base.join("inference").join(self.namespace())
    }
}
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chiral-network")
}
pub fn validate_hash(value: &str) -> Result<(), String> {
    if value.len() != 66
        || !value.starts_with("0x")
        || !value[2..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("hash must be 32 bytes of 0x-prefixed hex".into());
    }
    Ok(())
}
pub fn normalize_address(value: &str) -> Result<String, String> {
    if value.len() != 42
        || !value.starts_with("0x")
        || !value[2..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("address must be 20 bytes of 0x-prefixed hex".into());
    }
    Ok(value.to_ascii_lowercase())
}
pub fn validate_endpoint(value: &str) -> Result<(), String> {
    let url = url::Url::parse(value).map_err(|_| "invalid endpoint URL")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "endpoint credentials, queries and fragments are forbidden; use a bearer file".into(),
        );
    }
    let host = url.host().ok_or("endpoint must have a host")?;
    let loopback = match host {
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
        url::Host::Domain(_) => false,
    };
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err("remote endpoints require HTTPS; HTTP is only allowed for explicit literal-loopback development endpoints".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transport_boundary() {
        for url in [
            "http://127.0.0.1:8545",
            "http://[::1]:8545",
            "https://rpc.example/api/chain/rpc",
        ] {
            assert!(validate_endpoint(url).is_ok());
        }
        for url in [
            "http://localhost:8545",
            "http://example.com",
            "http://10.0.0.1",
            "https://user:pass@example.com",
            "https://example.com?token=secret",
            "file:///tmp/rpc",
        ] {
            assert!(validate_endpoint(url).is_err());
        }
    }
    #[test]
    fn deployment_namespaces_do_not_collide() {
        let mut config = NetworkConfig {
            identity: NetworkIdentity {
                chain_id: ESPACE_TESTNET_CHAIN_ID,
                genesis_hash: ESPACE_TESTNET_GENESIS.into(),
                deployment_id: "test-1".into(),
            },
            rpc_url: "http://127.0.0.1:8545".into(),
            rpc_bearer_file: None,
            faucet_url: None,
            explorer_url: None,
            policy_url: None,
        };
        let old = config.namespace();
        config.identity.deployment_id = "test-2".into();
        assert_ne!(old, config.namespace());
        assert!(!config
            .scoped_dir(Path::new("/data"))
            .ends_with("tx_metadata.json"));
        assert!(config.validate().is_ok());
        config.identity.chain_id = 1030;
        assert!(config.validate().is_err());
        config.identity.chain_id = ESPACE_TESTNET_CHAIN_ID;
        config.identity.genesis_hash = format!("0x{}", "ab".repeat(32));
        assert!(config.validate().is_err());
    }
}
