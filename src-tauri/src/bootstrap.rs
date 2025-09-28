use std::{
    collections::HashSet,
    str::FromStr,
};

use libp2p::{
    multiaddr::Protocol,
    Multiaddr,
};
use tracing::{info, warn};
use trust_dns_resolver::TokioAsyncResolver;

/// Default DNS zones that publish libp2p bootstrap records for the network.
pub const DEFAULT_BOOTSTRAP_DOMAINS: &[&str] = &["bootstrap.chiral.network"];

/// Resolve bootstrap peers from DNS `dnsaddr` TXT records.
pub async fn resolve_bootstrap_nodes(
    domains: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    let entries: Vec<String> = match domains {
        Some(values) if !values.is_empty() => values,
        _ => DEFAULT_BOOTSTRAP_DOMAINS
            .iter()
            .map(|d| d.to_string())
            .collect(),
    };

    if entries.is_empty() {
        return Err("No bootstrap domains provided".to_string());
    }

    let resolver = TokioAsyncResolver::tokio_from_system_conf()
        .map_err(|err| format!("Failed to initialise DNS resolver: {}", err))?;

    let mut discovered = HashSet::new();

    for entry in entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Allow callers to pass direct multiaddresses alongside domains.
        if let Ok(multiaddr) = Multiaddr::from_str(trimmed) {
            if multiaddr.iter().any(|p| matches!(p, Protocol::P2p(_))) {
                discovered.insert(multiaddr.to_string());
                continue;
            } else {
                warn!(
                    "Ignoring multiaddr without peer id from bootstrap list: {}",
                    trimmed
                );
                continue;
            }
        }

        let lookup_name = if trimmed.starts_with("_dnsaddr.") {
            trimmed.to_string()
        } else {
            format!("_dnsaddr.{}", trimmed)
        };

        match resolver.txt_lookup(lookup_name.clone()).await {
            Ok(response) => {
                let mut found_for_domain = false;

                for record in response.iter() {
                    let mut payload = String::new();
                    for data in record.txt_data() {
                        payload.push_str(&String::from_utf8_lossy(data));
                    }
                    let payload = payload.trim();
                    if let Some(multiaddr_str) = payload.strip_prefix("dnsaddr=") {
                        match Multiaddr::from_str(multiaddr_str) {
                            Ok(addr) => {
                                if addr
                                    .iter()
                                    .any(|component| matches!(component, Protocol::P2p(_)))
                                {
                                    discovered.insert(addr.to_string());
                                    found_for_domain = true;
                                } else {
                                    warn!(
                                        "dnsaddr record for {} missing peer id: {}",
                                        trimmed,
                                        multiaddr_str
                                    );
                                }
                            }
                            Err(err) => warn!(
                                "Invalid multiaddr in dnsaddr record for {}: {} ({})",
                                trimmed,
                                multiaddr_str,
                                err
                            ),
                        }
                    }
                }

                if !found_for_domain {
                    warn!(
                        "No usable dnsaddr multiaddresses discovered for {}",
                        lookup_name
                    );
                }
            }
            Err(err) => warn!(
                "Failed to resolve dnsaddr records for {}: {}",
                lookup_name,
                err
            ),
        }
    }

    if discovered.is_empty() {
        Err("No bootstrap multiaddresses discovered from DNS".to_string())
    } else {
        let mut results: Vec<String> = discovered.into_iter().collect();
        results.sort();
        info!("Resolved {} bootstrap peers from DNS", results.len());
        Ok(results)
    }
}

pub fn default_bootstrap_domains() -> Vec<String> {
    DEFAULT_BOOTSTRAP_DOMAINS
        .iter()
        .map(|d| d.to_string())
        .collect()
}
