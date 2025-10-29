use serde::{Deserialize, Serialize};
use tracing::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInfo {
    pub public_ip: String,
    pub local_ip: String,
    pub http_server_url: String,
    pub upnp_enabled: bool,
    pub port_forwarded: bool,
}

/// Get public IP address
#[tauri::command]
pub async fn get_public_ip() -> Result<String, String> {
    info!("🌐 Getting public IP address...");

    // Try multiple services for reliability
    let services = vec![
        "https://api.ipify.org",
        "https://icanhazip.com",
        "https://ipinfo.io/ip",
    ];

    for service in services {
        match reqwest::get(service).await {
            Ok(response) => {
                if let Ok(ip) = response.text().await {
                    let ip = ip.trim().to_string();
                    if !ip.is_empty() {
                        info!("✅ Public IP: {}", ip);
                        return Ok(ip);
                    }
                }
            }
            Err(e) => {
                error!("Failed to get IP from {}: {}", service, e);
                continue;
            }
        }
    }

    Err("Failed to get public IP from all services".to_string())
}

/// Get local IP address
#[tauri::command]
pub fn get_local_ip() -> Result<String, String> {
    use std::net::UdpSocket;

    // Connect to a public DNS server to get local IP
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket
        .connect("8.8.8.8:80")
        .map_err(|e| e.to_string())?;

    let local_addr = socket.local_addr().map_err(|e| e.to_string())?;
    Ok(local_addr.ip().to_string())
}

/// Setup UPnP port forwarding
#[tauri::command]
pub async fn setup_upnp_port_forwarding(port: u16) -> Result<bool, String> {
    use igd::aio::search_gateway;
    use igd::PortMappingProtocol;
    use std::time::Duration;

    info!("🔧 Setting up UPnP port forwarding for port {}...", port);

    // Search for gateway
    let gateway = match tokio::time::timeout(Duration::from_secs(5), search_gateway(Default::default())).await {
        Ok(Ok(gateway)) => gateway,
        Ok(Err(e)) => {
            error!("Failed to find UPnP gateway: {}", e);
            return Ok(false);
        }
        Err(_) => {
            error!("UPnP gateway search timed out");
            return Ok(false);
        }
    };

    info!("✅ Found UPnP gateway");

    // Get local IP
    let local_ip = get_local_ip()?;
    let local_addr = format!("{}:{}", local_ip, port)
        .parse()
        .map_err(|e| format!("Invalid address: {}", e))?;

    // Add port mapping
    match gateway
        .add_port(
            PortMappingProtocol::TCP,
            port,
            local_addr,
            0,
            "Chiral Network HTTP Server",
        )
        .await
    {
        Ok(_) => {
            info!("✅ UPnP port forwarding enabled for port {}", port);
            Ok(true)
        }
        Err(e) => {
            error!("Failed to add UPnP port mapping: {}", e);
            Ok(false)
        }
    }
}

/// Remove UPnP port forwarding
#[tauri::command]
pub async fn remove_upnp_port_forwarding(port: u16) -> Result<bool, String> {
    use igd::aio::search_gateway;
    use igd::PortMappingProtocol;
    use std::time::Duration;

    info!("🔧 Removing UPnP port forwarding for port {}...", port);

    let gateway = match tokio::time::timeout(Duration::from_secs(5), search_gateway(Default::default())).await {
        Ok(Ok(gateway)) => gateway,
        Ok(Err(e)) => {
            error!("Failed to find UPnP gateway: {}", e);
            return Ok(false);
        }
        Err(_) => {
            error!("UPnP gateway search timed out");
            return Ok(false);
        }
    };

    match gateway
        .remove_port(PortMappingProtocol::TCP, port)
        .await
    {
        Ok(_) => {
            info!("✅ UPnP port forwarding removed for port {}", port);
            Ok(true)
        }
        Err(e) => {
            error!("Failed to remove UPnP port mapping: {}", e);
            Ok(false)
        }
    }
}

/// Get complete network info for file sharing
#[tauri::command]
pub async fn get_network_info(port: u16) -> Result<NetworkInfo, String> {
    let public_ip = get_public_ip().await?;
    let local_ip = get_local_ip()?;
    let http_server_url = format!("http://{}:{}", public_ip, port);

    // Try to setup UPnP
    let upnp_enabled = setup_upnp_port_forwarding(port).await.unwrap_or(false);

    Ok(NetworkInfo {
        public_ip,
        local_ip,
        http_server_url,
        upnp_enabled,
        port_forwarded: upnp_enabled,
    })
}
