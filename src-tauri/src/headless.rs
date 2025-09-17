// Headless mode for running as a bootstrap node on servers
use crate::dht::{DhtService, FileMetadata};
use crate::ethereum::GethProcess;
use clap::Parser;
use tokio::signal;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(name = "chiral-network")]
#[command(about = "Chiral Network - P2P File Sharing", long_about = None)]
pub struct CliArgs {
    /// Run in headless mode (no GUI)
    #[arg(long)]
    pub headless: bool,

    /// DHT port to listen on
    #[arg(long, default_value = "4001")]
    pub dht_port: u16,

    /// Bootstrap nodes to connect to (can be specified multiple times)
    #[arg(long)]
    pub bootstrap: Vec<String>,

    /// Enable geth node
    #[arg(long)]
    pub enable_geth: bool,

    /// Geth data directory
    #[arg(long, default_value = "./bin/geth-data")]
    pub geth_data_dir: String,

    /// Miner address for geth
    #[arg(long)]
    pub miner_address: Option<String>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    pub log_level: String,

    /// Generate multiaddr for this node (shows the address others can connect to)
    #[arg(long)]
    pub show_multiaddr: bool,
}

pub async fn run_headless(args: CliArgs) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(args.log_level)
        .init();

    info!("Starting Chiral Network in headless mode");
    info!("DHT Port: {}", args.dht_port);

    // Add default bootstrap nodes if no custom ones specified
    let mut bootstrap_nodes = args.bootstrap.clone();
    let provided_bootstrap = !bootstrap_nodes.is_empty();
    if !provided_bootstrap {
        // Use reliable IP-based bootstrap nodes so fresh nodes can join the mesh
        bootstrap_nodes.extend([
            "/ip4/104.131.131.82/tcp/4001/p2p/QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ"
                .to_string(),
        ]);
        info!("Using default bootstrap nodes: {:?}", bootstrap_nodes);
    }

    // Start DHT node
    let dht_service = DhtService::new(args.dht_port, bootstrap_nodes.clone()).await?;
    let peer_id = dht_service.get_peer_id().await;

    // Start the DHT running in background
    dht_service.run().await;

    info!("✅ DHT node started");
    info!("📍 Local Peer ID: {}", peer_id);

    if args.show_multiaddr {
        // Get local IP addresses
        let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
        info!("🔗 Multiaddr for other nodes to connect:");
        info!("   /ip4/{}/tcp/{}/p2p/{}", local_ip, args.dht_port, peer_id);
        info!("   /ip4/127.0.0.1/tcp/{}/p2p/{}", args.dht_port, peer_id);
    }

    // Optionally start geth
    let _geth_handle = if args.enable_geth {
        info!("Starting geth node...");
        let mut geth = GethProcess::new();
        geth.start(&args.geth_data_dir, args.miner_address.as_deref())?;
        info!("✅ Geth node started");
        Some(geth)
    } else {
        None
    };

    // Add some example bootstrap data if this is a primary bootstrap node
    if !provided_bootstrap {
        info!("Running as primary bootstrap node (no peers specified)");

        // Publish some example metadata to seed the network
        let example_metadata = FileMetadata {
            file_hash: "QmBootstrap123Example".to_string(),
            file_name: "welcome.txt".to_string(),
            file_size: 1024,
            seeders: vec![peer_id.clone()],
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            mime_type: Some("text/plain".to_string()),
        };

        dht_service.publish_file(example_metadata).await?;
        info!("Published bootstrap file metadata");
    } else {
        info!("Connecting to bootstrap nodes: {:?}", bootstrap_nodes);
        for bootstrap_addr in &bootstrap_nodes {
            match dht_service.connect_peer(bootstrap_addr.clone()).await {
                Ok(_) => {
                    info!("Connected to bootstrap: {}", bootstrap_addr);
                    // In a real implementation, the bootstrap nodes would add us as a peer
                    // For now, simulate this by adding the bootstrap as a connected peer
                }
                Err(e) => error!("Failed to connect to {}: {}", bootstrap_addr, e),
            }
        }
    }

    info!("Bootstrap node is running. Press Ctrl+C to stop.");

    // Keep the service running
    signal::ctrl_c().await?;

    info!("Shutting down...");
    Ok(())
}

fn get_local_ip() -> Option<String> {
    // Try to get the local IP address
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                return Some(addr.ip().to_string());
            }
        }
    }
    None
}
