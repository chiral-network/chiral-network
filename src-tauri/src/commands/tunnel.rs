use crate::tunnel_service::{TunnelInfo, TunnelProvider, TUNNEL_MANAGER};
use tracing::{info, warn};

/// Start tunnel with specified provider
#[tauri::command]
pub async fn start_tunnel(port: u16, provider: String) -> Result<String, String> {
    let tunnel_provider = match provider.as_str() {
        "ngrok" => TunnelProvider::Ngrok,
        "cloudflared" => TunnelProvider::Cloudflared,
        "bore" => TunnelProvider::Bore,
        "localtunnel" => TunnelProvider::Localtunnel,
        "self_hosted" => TunnelProvider::SelfHosted,
        _ => return Err(format!("Unknown tunnel provider: {}", provider)),
    };
    
    TUNNEL_MANAGER.start_tunnel(port, tunnel_provider).await
}

/// Start tunnel with auto-detection of best available provider
#[tauri::command]
pub async fn start_tunnel_auto(port: u16) -> Result<String, String> {
    let available = TUNNEL_MANAGER.check_available_providers().await;
    
    // Try providers in order of preference
    for provider in available {
        match provider {
            TunnelProvider::Ngrok => {
                info!("🚀 Trying ngrok...");
                match TUNNEL_MANAGER.start_tunnel(port, provider).await {
                    Ok(url) => return Ok(url),
                    Err(e) => {
                        warn!("❌ Ngrok failed: {}", e);
                        continue;
                    }
                }
            }
            TunnelProvider::Cloudflared => {
                info!("🚀 Trying cloudflared...");
                match TUNNEL_MANAGER.start_tunnel(port, provider).await {
                    Ok(url) => return Ok(url),
                    Err(e) => {
                        warn!("❌ Cloudflared failed: {}", e);
                        continue;
                    }
                }
            }
            TunnelProvider::Bore => {
                info!("🚀 Trying bore...");
                match TUNNEL_MANAGER.start_tunnel(port, provider).await {
                    Ok(url) => return Ok(url),
                    Err(e) => {
                        warn!("❌ Bore failed: {}", e);
                        continue;
                    }
                }
            }
            TunnelProvider::Localtunnel => {
                info!("🚀 Trying localtunnel...");
                match TUNNEL_MANAGER.start_tunnel(port, provider).await {
                    Ok(url) => return Ok(url),
                    Err(e) => {
                        warn!("❌ Localtunnel failed: {}", e);
                        continue;
                    }
                }
            }
            TunnelProvider::SelfHosted => {
                info!("🚀 Using self-hosted tunnel...");
                return TUNNEL_MANAGER.start_tunnel(port, provider).await;
            }
        }
    }
    
    Err("No tunnel providers available. Please install ngrok, cloudflared, or bore.".to_string())
}

/// Stop the active tunnel
#[tauri::command]
pub async fn stop_tunnel() -> Result<(), String> {
    TUNNEL_MANAGER.stop().await
}

/// Get current tunnel status and information
#[tauri::command]
pub async fn get_tunnel_info() -> Result<TunnelInfo, String> {
    Ok(TUNNEL_MANAGER.get_info().await)
}

/// Get available tunnel providers
#[tauri::command]
pub async fn get_available_providers() -> Result<Vec<String>, String> {
    let providers = TUNNEL_MANAGER.check_available_providers().await;
    Ok(providers.iter().map(|p| p.as_str().to_string()).collect())
}
