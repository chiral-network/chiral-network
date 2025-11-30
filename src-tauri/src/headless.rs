// Headless mode for running as a bootstrap node on servers
use crate::commands::bootstrap::get_bootstrap_nodes;
use crate::dht::{models::DhtMetricsSnapshot, models::FileMetadata, DhtService};
use crate::download_restart::{DownloadRestartService, StartDownloadRequest};
use crate::ethereum::GethProcess;
use crate::file_transfer::FileTransferService;
use clap::Parser;
use std::{sync::Arc, time::Duration};
use tokio::signal;

use tracing::{error, info, warn};

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

    // Generate consistent peerid
    #[arg(long)]
    pub secret: Option<String>,

    // Runs in bootstrap mode
    #[arg(long)]
    pub is_bootstrap: bool,

    /// Disable AutoNAT reachability probes
    #[arg(long)]
    pub disable_autonat: bool,

    #[arg(long)]
    pub enable_relay: bool,

    /// Interval in seconds between AutoNAT probes
    #[arg(long, default_value = "30")]
    pub autonat_probe_interval: u64,

    /// Additional AutoNAT servers to dial (multiaddr form)
    #[arg(long)]
    pub autonat_server: Vec<String>,

    /// Print reachability snapshot at startup (and periodically)
    #[arg(long)]
    pub show_reachability: bool,

    /// Print DCUtR hole-punching metrics at startup
    #[arg(long)]
    pub show_dcutr: bool,

    // SOCKS5 Proxy address (e.g., 127.0.0.1:9050 for Tor or a private VPN SOCKS endpoint)
    #[arg(long)]
    pub socks5_proxy: Option<String>,

    /// Print local download metrics snapshot at startup
    #[arg(long)]
    pub show_downloads: bool,

    /// Disable AutoRelay behavior
    #[arg(long)]
    pub disable_autorelay: bool,

    /// Preferred relay nodes (multiaddr form, can be specified multiple times)
    #[arg(long)]
    pub relay: Vec<String>,

    /// Start a restartable HTTP download when the node boots
    #[arg(long)]
    pub download_url: Option<String>,

    /// Destination path for the restartable download (required with --download-url)
    #[arg(long)]
    pub download_dest: Option<String>,

    /// Optional download identifier for reuse
    #[arg(long)]
    pub download_id: Option<String>,

    /// Optional expected SHA-256 for verification
    #[arg(long)]
    pub download_sha256: Option<String>,

    /// Pause an active restartable download by ID
    #[arg(long)]
    pub pause_download: Option<String>,

    /// Resume a paused restartable download by ID
    #[arg(long)]
    pub resume_download: Option<String>,
}

pub async fn run_headless(args: CliArgs) -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    let _ = tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::from_default_env()
                .add_directive("chiral_network=info".parse().unwrap())
                .add_directive("libp2p=info".parse().unwrap())
                .add_directive("libp2p_kad=debug".parse().unwrap())
                .add_directive("libp2p_swarm=debug".parse().unwrap()),
        )
        .try_init();

    info!("Starting Chiral Network in headless mode");
    info!("CLI args: {:#?}", args);

    let download_restart_service = Arc::new(DownloadRestartService::new(None));

    // Add default bootstrap nodes if no custom ones specified
    let mut bootstrap_nodes = args.bootstrap.clone();
    let provided_bootstrap = !bootstrap_nodes.is_empty();
    if !provided_bootstrap {
        // Use reliable IP-based bootstrap nodes so fresh nodes can join the mesh
        // Using the same comprehensive set as the frontend for network consistency
        bootstrap_nodes.extend(get_bootstrap_nodes());
        info!("Using default bootstrap nodes: {:?}", bootstrap_nodes);
    }

    let enable_autonat = !args.disable_autonat;
    let probe_interval = if enable_autonat {
        Some(Duration::from_secs(args.autonat_probe_interval))
    } else {
        None
    };

    if enable_autonat {
        info!(
            "AutoNAT probes enabled (interval: {}s)",
            args.autonat_probe_interval
        );
        if !args.autonat_server.is_empty() {
            info!("AutoNAT servers: {:?}", args.autonat_server);
        }
    } else {
        info!("AutoNAT probes disabled via CLI");
    }

    if args.download_url.is_some()
        || args.pause_download.is_some()
        || args.resume_download.is_some()
    {
        if let Err(err) = handle_download_cli_commands(
            download_restart_service.clone(),
            args.download_url.as_deref(),
            args.download_dest.as_deref(),
            args.download_sha256.as_deref(),
            args.download_id.as_deref(),
            args.pause_download.as_deref(),
            args.resume_download.as_deref(),
        )
        .await
        {
            error!("Failed to process download CLI arguments: {}", err);
        }
    }

    // Optionally start local file-transfer service for metrics insight
    let file_transfer_service = if args.show_downloads {
        Some(Arc::new(FileTransferService::new().await.map_err(|e| {
            format!("Failed to start file transfer service: {}", e)
        })?))
    } else {
        None
    };
    // ---- finalize AutoRelay flag (bootstrap OFF + ENV OFF)
    let mut final_enable_autorelay = !args.disable_autorelay;
    if std::env::var("CHIRAL_DISABLE_AUTORELAY").ok().as_deref() == Some("1") {
        final_enable_autorelay = false;
        info!("AutoRelay disabled via env CHIRAL_DISABLE_AUTORELAY=1");
    }
    if final_enable_autorelay {
        if !args.relay.is_empty() {
            info!(
                "AutoRelay enabled with {} preferred relays",
                args.relay.len()
            );
        } else {
            info!("AutoRelay enabled, will discover relays from bootstrap nodes");
        }
    } else {
        info!("AutoRelay disabled");
    }

    // Start DHT node
    let dht_service = DhtService::new(
        args.dht_port,
        bootstrap_nodes.clone(),
        args.secret,
        args.is_bootstrap,
        enable_autonat,
        probe_interval,
        args.autonat_server.clone(),
        args.socks5_proxy,
        file_transfer_service.clone(),
        None, // chunk_manager
        None, // chunk_size_kb: use default
        None, // cache_size_mb: use default
        final_enable_autorelay,
        args.relay.clone(),
        args.enable_relay,
        true,
        None,
        None,
        None,
    )
    .await?;
    let peer_id = dht_service.get_peer_id().await;

    // DHT is already running in a spawned background task

    if let Some(ft) = &file_transfer_service {
        let snapshot = ft.download_metrics_snapshot().await;
        info!(
            "📊 Download metrics: success={}, failures={}, retries={}",
            snapshot.total_success, snapshot.total_failures, snapshot.total_retries
        );
        if let Some(latest) = snapshot.recent_attempts.first() {
            info!(
                "   Last attempt: hash={} status={:?} attempt {}/{}",
                latest.file_hash, latest.status, latest.attempt, latest.max_attempts
            );
        }
    }

    if args.show_multiaddr {
        // Get local IP addresses
        let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
        info!("🔗 Multiaddr for other nodes to connect:");
        info!("   /ip4/{}/tcp/{}/p2p/{}", local_ip, args.dht_port, peer_id);
        info!("   /ip4/127.0.0.1/tcp/{}/p2p/{}", args.dht_port, peer_id);
    }

    // Optionally start geth
    let geth_handle = if args.enable_geth {
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
            merkle_root: "QmBootstrap123Example".to_string(),
            file_name: "welcome.txt".to_string(),
            file_size: 1024,
            file_data: b"Hello, world!".to_vec(),
            seeders: vec![peer_id.clone()],
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            mime_type: Some("text/plain".to_string()),
            is_encrypted: false,
            encryption_method: None,
            key_fingerprint: None,
            parent_hash: None,
            cids: None,
            is_root: true,
            encrypted_key_bundle: None,
            download_path: None,
            price: 0.0,
            uploader_address: None,
            ftp_sources: None,
            http_sources: None,
            info_hash: None,
            trackers: None,
            ed2k_sources: None,
        };

        dht_service.publish_file(example_metadata, None).await?;
        info!("Published bootstrap file metadata");
    } else {
        info!("Connecting to bootstrap nodes: {:?}", bootstrap_nodes);
        for bootstrap_addr in &bootstrap_nodes {
            match dht_service.connect_peer(bootstrap_addr.clone()).await {
                Ok(_) => {
                    info!("Connected to bootstrap: {}", bootstrap_addr);
                    
                    // Verify the connection by checking if we have any connected peers
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    let connected_peers = dht_service.get_connected_peers().await;
                    if connected_peers.is_empty() {
                        warn!("Bootstrap connection to {} succeeded but no peers connected yet", bootstrap_addr);
                    } else {
                        info!("Verified bootstrap connection: {} peer(s) connected", connected_peers.len());
                    }
                }
                Err(e) => error!("Failed to connect to {}: {}", bootstrap_addr, e),
            }
        }
    }

    info!("Bootstrap node is running. Press Ctrl+C to stop.");
    let dht_arc = Arc::new(dht_service);

    if args.show_reachability {
        let snapshot = dht_arc.metrics_snapshot().await;
        log_reachability_snapshot(&snapshot);

        let dht_for_logs = dht_arc.clone();
        tokio::spawn(async move {
            loop {
                if Arc::strong_count(&dht_for_logs) <= 1 {
                    break;
                }

                tokio::time::sleep(Duration::from_secs(60)).await;

                let snapshot = dht_for_logs.metrics_snapshot().await;
                log_reachability_snapshot(&snapshot);

                if !snapshot.autonat_enabled {
                    break;
                }
            }
        });
    }

    if args.show_dcutr {
        let snapshot = dht_arc.metrics_snapshot().await;
        log_dcutr_snapshot(&snapshot);

        let dht_for_logs = dht_arc.clone();
        tokio::spawn(async move {
            loop {
                if Arc::strong_count(&dht_for_logs) <= 1 {
                    break;
                }

                tokio::time::sleep(Duration::from_secs(60)).await;

                let snapshot = dht_for_logs.metrics_snapshot().await;
                log_dcutr_snapshot(&snapshot);

                if !snapshot.dcutr_enabled {
                    break;
                }
            }
        });
    }

    // Spawn the event pump
    let dht_clone_for_pump = Arc::clone(&dht_arc);

    tokio::spawn(async move {
        loop {
            // If the DHT service has been shut down, the weak reference will be None
            let events = dht_clone_for_pump.drain_events(100).await;
            if events.is_empty() {
                // Avoid busy-waiting
                tokio::time::sleep(Duration::from_millis(200)).await;
                // Check if the DHT is still alive before continuing
                if Arc::strong_count(&dht_clone_for_pump) <= 1 {
                    // 1 is the pump itself
                    info!("DHT service appears to be shut down. Exiting event pump.");
                    break;
                }
                continue;
            }
        }
    });
    // Keep the service running
    signal::ctrl_c().await?;

    info!("Shutting down...");
    Ok(())
}

fn log_reachability_snapshot(snapshot: &DhtMetricsSnapshot) {
    info!(
        "📡 Reachability: {:?} (confidence {:?})",
        snapshot.reachability, snapshot.reachability_confidence
    );
    if let Some(ts) = snapshot.last_probe_at {
        info!("   Last probe epoch: {}", ts);
    }
    if let Some(err) = snapshot.last_reachability_error.as_ref() {
        info!("   Last error: {}", err);
    }
    if !snapshot.observed_addrs.is_empty() {
        info!("   Observed addresses: {:?}", snapshot.observed_addrs);
    }
    info!("   AutoNAT enabled: {}", snapshot.autonat_enabled);
}

fn log_dcutr_snapshot(snapshot: &DhtMetricsSnapshot) {
    let success_rate = if snapshot.dcutr_hole_punch_attempts > 0 {
        (snapshot.dcutr_hole_punch_successes as f64 / snapshot.dcutr_hole_punch_attempts as f64)
            * 100.0
    } else {
        0.0
    };
    info!(
        "🔀 DCUtR Metrics: {} attempts, {} successes, {} failures ({:.1}% success rate)",
        snapshot.dcutr_hole_punch_attempts,
        snapshot.dcutr_hole_punch_successes,
        snapshot.dcutr_hole_punch_failures,
        success_rate
    );
    if let Some(ts) = snapshot.last_dcutr_success {
        info!("   Last success epoch: {}", ts);
    }
    if let Some(ts) = snapshot.last_dcutr_failure {
        info!("   Last failure epoch: {}", ts);
    }
    info!("   DCUtR enabled: {}", snapshot.dcutr_enabled);
}

pub fn get_local_ip() -> Option<String> {
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

pub async fn handle_download_cli_commands(
    download_service: Arc<DownloadRestartService>,
    download_url: Option<&str>,
    download_dest: Option<&str>,
    download_sha256: Option<&str>,
    download_id: Option<&str>,
    pause_download: Option<&str>,
    resume_download: Option<&str>,
) -> Result<(), String> {
    if let Some(url) = download_url {
        let dest = download_dest
            .ok_or_else(|| "Missing --download-dest when using --download-url".to_string())?;
        let request = StartDownloadRequest {
            download_id: download_id.map(|s| s.to_string()),
            source_url: url.to_string(),
            destination_path: dest.to_string(),
            expected_sha256: download_sha256.map(|s| s.to_string()),
        };
        let id = download_service
            .start_download(request)
            .await
            .map_err(|e| e.to_string())?;
        info!("Started restartable download {}", id);
    }

    if let Some(id) = pause_download {
        download_service
            .pause_download(id)
            .await
            .map_err(|e| e.to_string())?;
        info!("Paused restartable download {}", id);
    }

    if let Some(id) = resume_download {
        download_service
            .resume_download(id)
            .await
            .map_err(|e| e.to_string())?;
        info!("Resumed restartable download {}", id);
    }

    Ok(())
}
