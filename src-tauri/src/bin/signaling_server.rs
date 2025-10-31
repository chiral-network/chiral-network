use chiral_network::signaling_server::SignalingServer;
use tracing::{error, info};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("🚀 Starting WebRTC Signaling Server");

    // Get port from environment or use default
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(9000);

    info!("📡 Binding to 0.0.0.0:{}", port);

    let server = SignalingServer::new(port);

    info!("✅ Signaling Server ready - waiting for connections...");

    if let Err(e) = server.run().await {
        error!("❌ Server error: {}", e);
        return Err(e);
    }

    Ok(())
}
