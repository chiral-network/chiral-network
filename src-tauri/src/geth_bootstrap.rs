use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapNode {
    pub enode: String,
    pub description: String,
    pub region: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapNodeHealth {
    pub enode: String,
    pub description: String,
    pub region: String,
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapHealthReport {
    pub total_nodes: usize,
    pub reachable_nodes: usize,
    pub unreachable_nodes: usize,
    pub nodes: Vec<BootstrapNodeHealth>,
}

/// Hardcoded bootstrap nodes for Chiral Network
pub fn get_bootstrap_nodes() -> Vec<BootstrapNode> {
    vec![
        BootstrapNode {
            enode: "enode://ae987db6399b50addb75d7822bfad9b4092fbfd79cbfe97e6864b1f17d3e8fcd8e9e190ad109572c1439230fa688a9837e58f0b1ad7c0dc2bc6e4ab328f3991e@130.245.173.105:30303".to_string(),
            description: "Primary US Bootstrap Node".to_string(),
            region: "US East".to_string(),
        },
        BootstrapNode {
            enode: "enode://b3ead5f07d0dbeda56023435a7c05877d67b055df3a8bf18f3d5f7c56873495cd4de5cf031ae9052827c043c12f1d30704088c79fb539c96834bfa74b78bf80b@20.85.124.187:30303".to_string(),
            description: "Secondary US Bootstrap Node".to_string(),
            region: "US West".to_string(),
        },
    ]
}

/// Parse enode address to extract IP and port
fn parse_enode_address(enode: &str) -> Result<(String, u16), String> {
    // enode format: enode://[node_id]@[ip]:[port]
    let parts: Vec<&str> = enode.split('@').collect();
    if parts.len() != 2 {
        return Err("Invalid enode format".to_string());
    }

    let addr_parts: Vec<&str> = parts[1].split(':').collect();
    if addr_parts.len() != 2 {
        return Err("Invalid address format".to_string());
    }

    let ip = addr_parts[0].to_string();
    let port = addr_parts[1]
        .parse::<u16>()
        .map_err(|_| "Invalid port number".to_string())?;

    Ok((ip, port))
}

/// Check health of a single bootstrap node via TCP connection
pub async fn check_bootstrap_node_health(node: &BootstrapNode) -> BootstrapNodeHealth {
    let start = std::time::Instant::now();

    match parse_enode_address(&node.enode) {
        Ok((ip, port)) => {
            match tokio::time::timeout(
                Duration::from_secs(5),
                tokio::net::TcpStream::connect(format!("{}:{}", ip, port)),
            )
            .await
            {
                Ok(Ok(_stream)) => {
                    let latency = start.elapsed().as_millis() as u64;
                    BootstrapNodeHealth {
                        enode: node.enode.clone(),
                        description: node.description.clone(),
                        region: node.region.clone(),
                        reachable: true,
                        latency_ms: Some(latency),
                        error: None,
                    }
                }
                Ok(Err(e)) => BootstrapNodeHealth {
                    enode: node.enode.clone(),
                    description: node.description.clone(),
                    region: node.region.clone(),
                    reachable: false,
                    latency_ms: None,
                    error: Some(format!("Connection failed: {}", e)),
                },
                Err(_) => BootstrapNodeHealth {
                    enode: node.enode.clone(),
                    description: node.description.clone(),
                    region: node.region.clone(),
                    reachable: false,
                    latency_ms: None,
                    error: Some("Connection timeout (5s)".to_string()),
                },
            }
        }
        Err(e) => BootstrapNodeHealth {
            enode: node.enode.clone(),
            description: node.description.clone(),
            region: node.region.clone(),
            reachable: false,
            latency_ms: None,
            error: Some(e),
        },
    }
}

/// Check health of all bootstrap nodes
pub async fn check_all_bootstrap_nodes() -> BootstrapHealthReport {
    let nodes = get_bootstrap_nodes();
    let mut health_checks = Vec::new();

    for node in &nodes {
        health_checks.push(check_bootstrap_node_health(node));
    }

    let results = futures::future::join_all(health_checks).await;

    let reachable_nodes = results.iter().filter(|h| h.reachable).count();
    let unreachable_nodes = results.len() - reachable_nodes;

    BootstrapHealthReport {
        total_nodes: results.len(),
        reachable_nodes,
        unreachable_nodes,
        nodes: results,
    }
}

/// Get a comma-separated enode string of only healthy bootstrap nodes
pub async fn get_healthy_bootstrap_enode_string() -> String {
    let report = check_all_bootstrap_nodes().await;

    let healthy_enodes: Vec<String> = report
        .nodes
        .iter()
        .filter(|node| node.reachable)
        .map(|node| node.enode.clone())
        .collect();

    if healthy_enodes.is_empty() {
        // If no nodes are reachable, return all nodes (fallback)
        get_bootstrap_nodes()
            .iter()
            .map(|n| n.enode.clone())
            .collect::<Vec<String>>()
            .join(",")
    } else {
        healthy_enodes.join(",")
    }
}
