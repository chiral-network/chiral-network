// Headless mode for running as a bootstrap node on servers
use crate::commands::bootstrap::get_bootstrap_nodes;
use crate::dht::{models::DhtMetricsSnapshot, models::FileMetadata, DhtConfig, DhtService};
use crate::download_restart::{DownloadRestartService, StartDownloadRequest};
use crate::e2e_api_headless::{start_headless_e2e_api_server, HeadlessE2eState};
use crate::ethereum::GethProcess;
use crate::bittorrent_handler;
use crate::file_transfer::FileTransferService;
use crate::http_server;
use crate::keystore::Keystore;
use crate::webrtc_service::{set_webrtc_service, WebRTCService};
use crate::{bandwidth::BandwidthController, manager::ChunkManager};
use clap::Parser;
use std::{sync::Arc, time::Duration};
use tokio::signal;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[command(name = "chiral-network")]
#[command(about = "Chiral Network - P2P File Sharing", long_about = None)]
pub struct CliArgs {
    /// Run in headless mode (no GUI)
    #[arg(long)]
    pub headless: bool,

    /// Run in interactive REPL mode
    #[arg(long)]
    pub interactive: bool,

    /// Run in TUI (Terminal User Interface) mode
    #[arg(long)]
    pub tui: bool,

    /// DHT port to listen on
    #[arg(long, default_value = "4001")]
    pub dht_port: u16,

    /// Bootstrap nodes to connect to (can be specified multiple times)
    #[arg(long)]
    pub bootstrap: Vec<String>,

    /// Download Geth binary and exit
    #[arg(long)]
    pub download_geth: bool,

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

    /// Interval in seconds between AutoNAT probes
    #[arg(long, default_value = "30")]
    pub autonat_probe_interval: u64,

    /// Additional AutoNAT servers to dial (multiaddr form)
    #[arg(long)]
    pub autonat_server: Vec<String>,

    /// Print reachability snapshot at startup (and periodically)
    #[arg(long)]
    pub show_reachability: bool,

    // SOCKS5 Proxy address (e.g., 127.0.0.1:9050 for Tor or a private VPN SOCKS endpoint)
    #[arg(long)]
    pub socks5_proxy: Option<String>,

    /// Print local download metrics snapshot at startup
    #[arg(long)]
    pub show_downloads: bool,

    /// Enable pure DHT client mode (cannot seed files or act as DHT server)
    /// This mode uses minimal blockchain sync (~100 blocks instead of ~10,000)
    /// Useful for lightweight clients or hard NAT environments
    #[arg(long)]
    pub pure_client_mode: bool,

    /// Force DHT server mode even if behind NAT (for testing/development)
    /// WARNING: May cause connectivity issues if behind strict NAT/firewall
    #[arg(long)]
    pub force_server_mode: bool,

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

pub fn create_dht_config_from_args(args: &CliArgs) -> DhtConfig<'static> {
    DhtConfig::builder()
        // always present
        .port(args.dht_port)
        .bootstrap_nodes(args.bootstrap.clone())
        .enable_autonat(!args.disable_autonat)
        .pure_client_mode(args.pure_client_mode)
        .force_server_mode(args.force_server_mode)
        .autonat_servers(args.autonat_server.clone())
        // optional fields using maybe_<field_name>
        .maybe_autonat_probe_interval(if args.autonat_probe_interval == 0 {
            None
        } else {
            Some(Duration::from_secs(args.autonat_probe_interval))
        })
        .maybe_proxy_address(args.socks5_proxy.clone().filter(|s| !s.is_empty()))
        .build()
}

pub async fn run_headless(mut args: CliArgs) -> Result<(), Box<dyn std::error::Error>> {
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
    args.bootstrap = bootstrap_nodes.clone();
    let enable_autonat = !args.disable_autonat;

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

    // For real P2P transfers (WebRTC/Bitswap), we need FileTransfer + ChunkManager (+ WebRTCService).
    // Enable automatically when running the headless E2E API (Attach-mode tests), or when explicitly requested.
    let enable_p2p = std::env::var("CHIRAL_E2E_API_PORT").ok().is_some()
        || std::env::var("CHIRAL_ENABLE_P2P").ok().as_deref() == Some("1")
        || args.show_downloads;

    let file_transfer_service = if enable_p2p {
        Some(Arc::new(FileTransferService::new().await.map_err(|e| {
            format!("Failed to start file transfer service: {}", e)
        })?))
    } else {
        None
    };

    let chunk_manager: Option<Arc<ChunkManager>> = if enable_p2p {
        let chunk_storage_path = std::env::temp_dir().join("chiral-chunks");
        let _ = std::fs::create_dir_all(&chunk_storage_path);
        Some(Arc::new(ChunkManager::new(chunk_storage_path)))
    } else {
        None
    };

    let webrtc_service: Option<Arc<WebRTCService>> = if enable_p2p {
        let Some(ref ft) = file_transfer_service else {
            error!("P2P enabled but FileTransferService is not available");
            return Ok(());
        };
        let keystore = Arc::new(Mutex::new(Keystore::load().unwrap_or_default()));
        let bandwidth = Arc::new(BandwidthController::new());
        match WebRTCService::new_headless(ft.clone(), keystore, bandwidth, None).await {
            Ok(svc) => {
                let arc = Arc::new(svc);
                set_webrtc_service(arc.clone()).await;
                Some(arc)
            }
            Err(e) => {
                error!("Failed to initialize WebRTCService in headless mode: {}", e);
                None
            }
        }
    } else {
        None
    };
    // Build DHT configuration from CLI arguments
    let dht_config = create_dht_config_from_args(&args);

    // Start DHT node
    let dht_service = DhtService::new(
        dht_config,
        file_transfer_service.clone(),
        webrtc_service,
        chunk_manager.clone(),
    )
    .await?;
    let dht_arc = Arc::new(dht_service);
    let peer_id = dht_arc.get_peer_id().await;

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
        geth.start(
            &args.geth_data_dir,
            args.miner_address.as_deref(),
            args.pure_client_mode,
        )?;
        if args.pure_client_mode {
            info!(
                "✅ Geth node started in pure-client mode (minimal blockchain sync: ~100 blocks)"
            );
        } else {
            info!("✅ Geth node started (full blockchain sync: ~10,000 blocks)");
        }
        Some(geth)
    } else {
        None
    };

    // Add some example bootstrap data if this is a primary bootstrap node.
    //
    // IMPORTANT:
    // Do not fail the entire headless process if this best-effort publish fails.
    // Also avoid inline `file_data` here because it can trigger Bitswap block publishing
    // even when P2P/Bitswap is not enabled for this headless run.
    if !provided_bootstrap {
        info!("Running as primary bootstrap node (no peers specified)");

        let example_metadata = FileMetadata {
            merkle_root: "QmBootstrap123Example".to_string(),
            file_name: "welcome.txt".to_string(),
            file_size: 1024,
            file_data: Vec::new(),
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
            manifest: None,
        };

        if let Err(e) = dht_arc.publish_file(example_metadata, None).await {
            warn!("Best-effort bootstrap publish failed (continuing): {}", e);
        } else {
            info!("Published bootstrap file metadata");
        }
    } else {
        info!(
            "Connecting to bootstrap nodes: {:?}",
            bootstrap_nodes.clone()
        );
        for bootstrap_addr in &bootstrap_nodes {
            match dht_arc.connect_peer(bootstrap_addr.clone()).await {
                Ok(_) => {
                    info!("Connected to bootstrap: {}", bootstrap_addr);

                    // Verify the connection by checking if we have any connected peers
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    let connected_peers = dht_arc.get_connected_peers().await;
                    if connected_peers.is_empty() {
                        warn!(
                            "Bootstrap connection to {} succeeded but no peers connected yet",
                            bootstrap_addr
                        );
                    } else {
                        info!(
                            "Verified bootstrap connection: {} peer(s) connected",
                            connected_peers.len()
                        );
                    }
                }
                Err(e) => error!("Failed to connect to {}: {}", bootstrap_addr, e),
            }
        }
    }

    // --------------------------------------------------------------------
    // Headless Real-E2E support (VM-friendly, no GUI):
    // - Start HTTP file server (8080-8090) for Range downloads
    // - Start E2E control API if CHIRAL_E2E_API_PORT is set
    // - Load wallet from CHIRAL_PRIVATE_KEY (required for upload/pay in option1)
    // --------------------------------------------------------------------
    let storage_dir = std::env::var("CHIRAL_STORAGE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap().join("files"));
    let _ = std::fs::create_dir_all(&storage_dir);

    let http_server_state = Arc::new(http_server::HttpServerState::new(storage_dir.clone()));
    http_server_state.set_dht(dht_arc.clone()).await;

    // Start HTTP file server on a free port in 8080..=8090 and keep shutdown sender alive.
    let mut http_base_url: Option<String> = None;
    let mut http_shutdown_tx_keepalive: Option<tokio::sync::oneshot::Sender<()>> = None;
    for port in 8080u16..=8090u16 {
        let bind_addr: std::net::SocketAddr = ([0, 0, 0, 0], port).into();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        match http_server::start_server(http_server_state.clone(), bind_addr, shutdown_rx).await {
            Ok(bound) => {
                let host =
                    std::env::var("CHIRAL_PUBLIC_IP").unwrap_or_else(|_| "127.0.0.1".to_string());
                http_base_url = Some(format!("http://{}:{}", host, bound.port()));
                http_shutdown_tx_keepalive = Some(shutdown_tx);
                info!(
                    "HTTP file server listening on http://{} (advertised host={})",
                    bound, host
                );
                break;
            }
            Err(e) => {
                // Try next port
                tracing::debug!("HTTP file server port {} failed: {}", port, e);
            }
        }
    }

    if http_base_url.is_none() {
        warn!("Could not start HTTP file server on any port (8080-8090). Downloads will fail.");
    }

    // Load account from CHIRAL_PRIVATE_KEY (headless has no GUI login).
    let (uploader_address, private_key) = match std::env::var("CHIRAL_PRIVATE_KEY") {
        Ok(pk) if !pk.trim().is_empty() => match crate::ethereum::get_account_from_private_key(&pk)
        {
            Ok(acct) => (Some(acct.address), Some(acct.private_key)),
            Err(e) => {
                warn!("Invalid CHIRAL_PRIVATE_KEY: {}", e);
                (None, None)
            }
        },
        _ => (None, None),
    };

    // Start headless E2E API if requested.
    let mut e2e_shutdown_tx_keepalive: Option<tokio::sync::oneshot::Sender<()>> = None;
    if let Ok(port_str) = std::env::var("CHIRAL_E2E_API_PORT") {
        if let Ok(port) = port_str.trim().parse::<u16>() {
            if let Some(ref http_base_url) = http_base_url {
                // Initialize BitTorrent handler for headless E2E (BitTorrent upload/download).
                // Use an isolated directory under the storage root so runs don't collide.
                let bt_download_dir = storage_dir.join("bittorrent");
                let _ = std::fs::create_dir_all(&bt_download_dir);
                let bt_port: u16 = std::env::var("E2E_BITTORRENT_SEED_PORT")
                    .or_else(|_| std::env::var("CHIRAL_BITTORRENT_SEED_PORT"))
                    .ok()
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(30000);
                // For real-network E2E (VM seeder), use a fixed listen port so the downloader can
                // deterministically connect (avoid "picked some other port in a range").
                let bt_port_end = bt_port.saturating_add(1);
                info!(
                    "Headless E2E BitTorrent listen port range: {}..{} (end-exclusive)",
                    bt_port, bt_port_end
                );
                let bt_handler = match bittorrent_handler::BitTorrentHandler::new_with_port_range(
                    bt_download_dir,
                    dht_arc.clone(),
                    Some(bt_port..bt_port_end),
                )
                .await
                {
                    Ok(h) => Some(Arc::new(h)),
                    Err(e) => {
                        warn!("Failed to initialize BitTorrent handler in headless mode: {}", e);
                        None
                    }
                };

                let state = HeadlessE2eState {
                    dht: dht_arc.clone(),
                    http_server_state: http_server_state.clone(),
                    http_base_url: http_base_url.clone(),
                    storage_dir: storage_dir.clone(),
                    uploader_address: uploader_address.clone(),
                    private_key: private_key.clone(),
                    file_transfer_service: file_transfer_service.clone(),
                    chunk_manager: chunk_manager.clone(),
                    ftp_server: {
                        // Enable embedded FTP server for E2E FTP protocol (upload + download).
                        // Note: FTP uses passive ports 50000-50100 (see ftp_server.rs).
                        let port: u16 = std::env::var("CHIRAL_FTP_PORT")
                            .ok()
                            .and_then(|s| s.trim().parse().ok())
                            .unwrap_or(2121);
                        Some(Arc::new(chiral_network::ftp_server::FtpServer::new(
                            storage_dir.join("ftp"),
                            port,
                        )))
                    },
                    bittorrent_handler: bt_handler,
                };
                match start_headless_e2e_api_server(state, port).await {
                    Ok((bound, shutdown_tx)) => {
                        e2e_shutdown_tx_keepalive = Some(shutdown_tx);
                        info!("E2E API server listening on http://{}", bound);
                    }
                    Err(e) => error!("Failed to start E2E API server: {}", e),
                }
            } else {
                warn!("CHIRAL_E2E_API_PORT is set but HTTP file server base URL is unavailable.");
            }
        } else {
            warn!(
                "CHIRAL_E2E_API_PORT is set but not a valid u16: {}",
                port_str
            );
        }
    }

    // Keep the service running (and keep shutdown senders alive)
    info!("Bootstrap node is running. Press Ctrl+C to stop.");
    let _keep_http = http_shutdown_tx_keepalive;
    let _keep_e2e = e2e_shutdown_tx_keepalive;

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
