#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod dht;
mod ethereum;
mod file_transfer;
mod geth_downloader;
mod headless;
mod keystore;

use dht::{DhtEvent, DhtMetricsSnapshot, DhtService, FileMetadata};
use ethereum::{
    create_new_account, get_account_from_private_key, get_balance, get_block_number, get_hashrate,
    get_mined_blocks_count, get_mining_logs, get_mining_performance, get_mining_status,
    get_network_difficulty, get_network_hashrate, get_peer_count, get_recent_mined_blocks,
    start_mining, stop_mining, EthAccount, GethProcess, MinedBlock,
};
use file_transfer::{FileTransferEvent, FileTransferService};
use fs2::available_space;
use geth_downloader::GethDownloader;
use keystore::Keystore;
use std::path::Path;
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
use sysinfo::{Components, System, MINIMUM_CPU_UPDATE_INTERVAL};
use systemstat::{Platform, System as SystemStat};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, State,
};
use tracing::{info, warn};

struct AppState {
    geth: Mutex<GethProcess>,
    downloader: Arc<GethDownloader>,
    miner_address: Mutex<Option<String>>,
    dht: Mutex<Option<Arc<DhtService>>>,
    file_transfer: Mutex<Option<Arc<FileTransferService>>>,
}

#[tauri::command]
async fn create_chiral_account() -> Result<EthAccount, String> {
    create_new_account()
}

#[tauri::command]
async fn import_chiral_account(private_key: String) -> Result<EthAccount, String> {
    get_account_from_private_key(&private_key)
}

#[tauri::command]
async fn start_geth_node(state: State<'_, AppState>, data_dir: String) -> Result<(), String> {
    let mut geth = state.geth.lock().map_err(|e| e.to_string())?;
    let miner_address = state.miner_address.lock().map_err(|e| e.to_string())?;
    geth.start(&data_dir, miner_address.as_deref())
}

#[tauri::command]
async fn stop_geth_node(state: State<'_, AppState>) -> Result<(), String> {
    let mut geth = state.geth.lock().map_err(|e| e.to_string())?;
    geth.stop()
}

#[tauri::command]
async fn save_account_to_keystore(
    address: String,
    private_key: String,
    password: String,
) -> Result<(), String> {
    let mut keystore = Keystore::load()?;
    keystore.add_account(address, &private_key, &password)?;
    Ok(())
}

#[tauri::command]
async fn load_account_from_keystore(
    address: String,
    password: String,
) -> Result<EthAccount, String> {
    let keystore = Keystore::load()?;

    // Get decrypted private key from keystore
    let private_key = keystore.get_account(&address, &password)?;

    // Derive account details from private key
    get_account_from_private_key(&private_key)
}

#[tauri::command]
async fn list_keystore_accounts() -> Result<Vec<String>, String> {
    let keystore = Keystore::load()?;
    Ok(keystore.list_accounts())
}

#[tauri::command]
async fn remove_account_from_keystore(address: String) -> Result<(), String> {
    let mut keystore = Keystore::load()?;
    keystore.remove_account(&address)?;
    Ok(())
}

#[tauri::command]
async fn get_account_balance(address: String) -> Result<String, String> {
    get_balance(&address).await
}

#[tauri::command]
async fn get_network_peer_count() -> Result<u32, String> {
    get_peer_count().await
}

#[tauri::command]
async fn is_geth_running(state: State<'_, AppState>) -> Result<bool, String> {
    let geth = state.geth.lock().map_err(|e| e.to_string())?;
    Ok(geth.is_running())
}

#[tauri::command]
async fn check_geth_binary(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.downloader.is_geth_installed())
}

#[tauri::command]
async fn download_geth_binary(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let downloader = state.downloader.clone();
    let app_handle = app.clone();

    downloader
        .download_geth(move |progress| {
            let _ = app_handle.emit("geth-download-progress", progress);
        })
        .await
}

#[tauri::command]
async fn set_miner_address(state: State<'_, AppState>, address: String) -> Result<(), String> {
    let mut miner_address = state.miner_address.lock().map_err(|e| e.to_string())?;
    *miner_address = Some(address);
    Ok(())
}

#[tauri::command]
async fn start_miner(
    state: State<'_, AppState>,
    address: String,
    threads: u32,
    data_dir: String,
) -> Result<(), String> {
    // Store the miner address for future geth restarts
    {
        let mut miner_address = state.miner_address.lock().map_err(|e| e.to_string())?;
        *miner_address = Some(address.clone());
    } // MutexGuard is dropped here

    // Try to start mining
    match start_mining(&address, threads).await {
        Ok(_) => Ok(()),
        Err(e) if e.contains("-32601") || e.to_lowercase().contains("does not exist") => {
            // miner_setEtherbase method doesn't exist, need to restart with etherbase
            println!("miner_setEtherbase not supported, restarting geth with miner address...");

            // Need to restart geth with the miner address
            // First stop geth
            {
                let mut geth = state.geth.lock().map_err(|e| e.to_string())?;
                geth.stop()?;
            }

            // Wait a moment for it to shut down
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            // Restart with miner address
            {
                let mut geth = state.geth.lock().map_err(|e| e.to_string())?;
                let miner_address = state.miner_address.lock().map_err(|e| e.to_string())?;
                println!("Restarting geth with miner address: {:?}", miner_address);
                geth.start(&data_dir, miner_address.as_deref())?;
            }

            // Wait for geth to start up and be ready to accept RPC connections
            let mut attempts = 0;
            let max_attempts = 30; // 30 seconds max wait
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                attempts += 1;

                // Check if geth is responding to RPC calls
                if let Ok(response) = reqwest::Client::new()
                    .post("http://127.0.0.1:8545")
                    .json(&serde_json::json!({
                        "jsonrpc": "2.0",
                        "method": "net_version",
                        "params": [],
                        "id": 1
                    }))
                    .send()
                    .await
                {
                    if response.status().is_success() {
                        if let Ok(json) = response.json::<serde_json::Value>().await {
                            if json.get("result").is_some() {
                                println!("Geth is ready for RPC calls");
                                break;
                            }
                        }
                    }
                }

                if attempts >= max_attempts {
                    return Err("Geth failed to start up within 30 seconds".to_string());
                }

                println!(
                    "Waiting for geth to start up... (attempt {}/{})",
                    attempts, max_attempts
                );
            }

            // Try mining again without setting etherbase (it's set via command line now)
            let client = reqwest::Client::new();
            let start_mining_direct = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "miner_start",
                "params": [threads],
                "id": 1
            });

            let response = client
                .post("http://127.0.0.1:8545")
                .json(&start_mining_direct)
                .send()
                .await
                .map_err(|e| format!("Failed to start mining after restart: {}", e))?;

            let json_response: serde_json::Value = response
                .json()
                .await
                .map_err(|e| format!("Failed to parse response: {}", e))?;

            if let Some(error) = json_response.get("error") {
                Err(format!("Failed to start mining after restart: {}", error))
            } else {
                Ok(())
            }
        }
        Err(e) => Err(format!("Failed to start mining: {}", e)),
    }
}

#[tauri::command]
async fn stop_miner() -> Result<(), String> {
    stop_mining().await
}

#[tauri::command]
async fn get_miner_status() -> Result<bool, String> {
    get_mining_status().await
}

#[tauri::command]
async fn get_miner_hashrate() -> Result<String, String> {
    get_hashrate().await
}

#[tauri::command]
async fn get_current_block() -> Result<u64, String> {
    get_block_number().await
}

#[tauri::command]
async fn get_network_stats() -> Result<(String, String), String> {
    let difficulty = get_network_difficulty().await?;
    let hashrate = get_network_hashrate().await?;
    Ok((difficulty, hashrate))
}

#[tauri::command]
async fn get_miner_logs(data_dir: String, lines: usize) -> Result<Vec<String>, String> {
    get_mining_logs(&data_dir, lines)
}

#[tauri::command]
async fn get_miner_performance(data_dir: String) -> Result<(u64, f64), String> {
    get_mining_performance(&data_dir)
}

#[tauri::command]
async fn get_blocks_mined(address: String) -> Result<u64, String> {
    get_mined_blocks_count(&address).await
}
#[tauri::command]
async fn get_recent_mined_blocks_pub(
    address: String,
    lookback: u64,
    limit: usize,
) -> Result<Vec<MinedBlock>, String> {
    get_recent_mined_blocks(&address, lookback, limit).await
}
#[tauri::command]
async fn start_dht_node(
    state: State<'_, AppState>,
    port: u16,
    bootstrap_nodes: Vec<String>,
) -> Result<String, String> {
    {
        let dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
        if dht_guard.is_some() {
            return Err("DHT node is already running".to_string());
        }
    }

    let dht_service = DhtService::new(port, bootstrap_nodes, None)
        .await
        .map_err(|e| format!("Failed to start DHT: {}", e))?;

    let peer_id = dht_service.get_peer_id().await;

    // Start the DHT node running in background
    dht_service.run().await;

    {
        let mut dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
        *dht_guard = Some(Arc::new(dht_service));
    }

    Ok(peer_id)
}

#[tauri::command]
async fn stop_dht_node(state: State<'_, AppState>) -> Result<(), String> {
    let dht = {
        let mut dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
        dht_guard.take()
    };

    if let Some(dht) = dht {
        dht.shutdown()
            .await
            .map_err(|e| format!("Failed to stop DHT: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
async fn publish_file_metadata(
    state: State<'_, AppState>,
    file_hash: String,
    file_name: String,
    file_size: u64,
    mime_type: Option<String>,
) -> Result<(), String> {
    let dht = {
        let dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        let metadata = FileMetadata {
            file_hash,
            file_name,
            file_size,
            seeders: vec![],
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            mime_type,
        };

        dht.publish_file(metadata).await
    } else {
        Err("DHT node is not running".to_string())
    }
}

#[tauri::command]
async fn search_file_metadata(state: State<'_, AppState>, file_hash: String) -> Result<(), String> {
    let dht = {
        let dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        dht.get_file(file_hash).await
    } else {
        Err("DHT node is not running".to_string())
    }
}

#[tauri::command]
async fn connect_to_peer(state: State<'_, AppState>, peer_address: String) -> Result<(), String> {
    let dht = {
        let dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        dht.connect_peer(peer_address).await
    } else {
        Err("DHT node is not running".to_string())
    }
}

#[tauri::command]
async fn get_dht_peer_count(state: State<'_, AppState>) -> Result<usize, String> {
    let dht = {
        let dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        Ok(dht.get_peer_count().await)
    } else {
        Ok(0) // Return 0 if DHT is not running
    }
}

#[tauri::command]
async fn get_dht_peer_id(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let dht = {
        let dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        Ok(Some(dht.get_peer_id().await))
    } else {
        Ok(None) // Return None if DHT is not running
    }
}

#[tauri::command]
async fn get_dht_health(state: State<'_, AppState>) -> Result<Option<DhtMetricsSnapshot>, String> {
    let dht = {
        let dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        Ok(Some(dht.metrics_snapshot().await))
    } else {
        Ok(None)
    }
}

#[tauri::command]
async fn get_nat_status(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let dht = {
        let dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        let snap = dht.metrics_snapshot().await;
        Ok(snap.nat_status)
    } else {
        Ok(None)
    }
}

#[tauri::command]
async fn get_dht_events(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let dht = {
        let dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
        dht_guard.as_ref().cloned()
    };

    if let Some(dht) = dht {
        let events = dht.drain_events(100).await;
        // Convert events to concise human-readable strings for the UI
        let mapped: Vec<String> = events
            .into_iter()
            .map(|e| match e {
                DhtEvent::PeerDiscovered(p) => format!("peer_discovered:{}", p),
                DhtEvent::PeerConnected(p) => format!("peer_connected:{}", p),
                DhtEvent::PeerDisconnected(p) => format!("peer_disconnected:{}", p),
                DhtEvent::FileDiscovered(meta) => format!(
                    "file_discovered:{}:{}:{}",
                    meta.file_hash, meta.file_name, meta.file_size
                ),
                DhtEvent::FileNotFound(hash) => format!("file_not_found:{}", hash),
                DhtEvent::Error(err) => format!("error:{}", err),
            })
            .collect();
        Ok(mapped)
    } else {
        Ok(vec![])
    }
}

#[tauri::command]
fn get_cpu_temperature() -> Option<f32> {
    static mut LAST_UPDATE: Option<Instant> = None;
    unsafe {
        if let Some(last) = LAST_UPDATE {
            if last.elapsed() < MINIMUM_CPU_UPDATE_INTERVAL {
                return None;
            }
        }
        LAST_UPDATE = Some(Instant::now());
    }
    // Try sysinfo first (works on some platforms including M1 macs)
    let mut sys = System::new_all();
    sys.refresh_cpu_all();
    let components = Components::new_with_refreshed_list();

    let mut core_count = 0;

    let sum: f32 = components
        .iter()
        .filter(|c| {
            let label = c.label().to_lowercase();
            label.contains("cpu") || label.contains("package") || label.contains("tdie")
        })
        .map(|c| {
            core_count += 1;
            c.temperature()
        })
        .sum();
    if core_count > 0 {
        return Some(sum / core_count as f32);
    }
    // handles Windows case?
    let stat_sys = SystemStat::new();
    if let Ok(temp) = stat_sys.cpu_temp() {
        return Some(temp);
    }

    None
}
#[tauri::command]
fn detect_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en-US".into())
}

#[tauri::command]
async fn start_file_transfer_service(state: State<'_, AppState>) -> Result<(), String> {
    {
        let ft_guard = state.file_transfer.lock().map_err(|e| e.to_string())?;
        if ft_guard.is_some() {
            return Err("File transfer service is already running".to_string());
        }
    }

    let file_transfer_service = FileTransferService::new()
        .await
        .map_err(|e| format!("Failed to start file transfer service: {}", e))?;

    {
        let mut ft_guard = state.file_transfer.lock().map_err(|e| e.to_string())?;
        *ft_guard = Some(Arc::new(file_transfer_service));
    }

    Ok(())
}

#[tauri::command]
async fn upload_file_to_network(
    state: State<'_, AppState>,
    file_path: String,
    file_name: String,
) -> Result<String, String> {
    let ft = {
        let ft_guard = state.file_transfer.lock().map_err(|e| e.to_string())?;
        ft_guard.as_ref().cloned()
    };

    if let Some(ft) = ft {
        // Upload the file
        ft.upload_file(file_path.clone(), file_name.clone()).await?;

        // Get the file hash by reading the file and calculating it
        let file_data = tokio::fs::read(&file_path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?;
        let file_hash = file_transfer::FileTransferService::calculate_file_hash(&file_data);

        // Also publish to DHT if it's running
        let dht = {
            let dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
            dht_guard.as_ref().cloned()
        };

        if let Some(dht) = dht {
            let metadata = FileMetadata {
                file_hash: file_hash.clone(),
                file_name: file_name.clone(),
                file_size: file_data.len() as u64,
                seeders: vec![],
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                mime_type: None,
            };

            if let Err(e) = dht.publish_file(metadata).await {
                warn!("Failed to publish file metadata to DHT: {}", e);
            }
        }

        Ok(file_hash)
    } else {
        Err("File transfer service is not running".to_string())
    }
}

#[tauri::command]
async fn download_file_from_network(
    state: State<'_, AppState>,
    file_hash: String,
    output_path: String,
) -> Result<(), String> {
    let ft = {
        let ft_guard = state.file_transfer.lock().map_err(|e| e.to_string())?;
        ft_guard.as_ref().cloned()
    };

    if let Some(ft) = ft {
        // First try to download from local storage
        match ft
            .download_file(file_hash.clone(), output_path.clone())
            .await
        {
            Ok(()) => {
                info!("File downloaded successfully from local storage");
                return Ok(());
            }
            Err(_) => {
                // File not found locally, would need to implement P2P download here
                // For now, return an error
                return Err(
                    "File not found in local storage. P2P download not yet implemented."
                        .to_string(),
                );
            }
        }
    } else {
        Err("File transfer service is not running".to_string())
    }
}

#[tauri::command]
async fn upload_file_data_to_network(
    state: State<'_, AppState>,
    file_name: String,
    file_data: Vec<u8>,
) -> Result<String, String> {
    let ft = {
        let ft_guard = state.file_transfer.lock().map_err(|e| e.to_string())?;
        ft_guard.as_ref().cloned()
    };

    if let Some(ft) = ft {
        // Calculate file hash from the data
        let file_hash = file_transfer::FileTransferService::calculate_file_hash(&file_data);

        // Store the file data directly in memory
        let file_size = file_data.len() as u64;
        ft.store_file_data(file_hash.clone(), file_name.clone(), file_data)
            .await;

        // Also publish to DHT if it's running
        let dht = {
            let dht_guard = state.dht.lock().map_err(|e| e.to_string())?;
            dht_guard.as_ref().cloned()
        };

        if let Some(dht) = dht {
            let metadata = FileMetadata {
                file_hash: file_hash.clone(),
                file_name: file_name.clone(),
                file_size: file_size,
                seeders: vec![],
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                mime_type: None,
            };

            if let Err(e) = dht.publish_file(metadata).await {
                warn!("Failed to publish file metadata to DHT: {}", e);
            }
        }

        Ok(file_hash)
    } else {
        Err("File transfer service is not running".to_string())
    }
}

#[tauri::command]
async fn show_in_folder(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .args([&std::path::Path::new(&path)
                .parent()
                .unwrap_or(std::path::Path::new("/"))])
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    Ok(())
}

#[tauri::command]
async fn get_file_transfer_events(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let ft = {
        let ft_guard = state.file_transfer.lock().map_err(|e| e.to_string())?;
        ft_guard.as_ref().cloned()
    };

    if let Some(ft) = ft {
        let events = ft.drain_events(100).await;
        let mapped: Vec<String> = events
            .into_iter()
            .map(|e| match e {
                FileTransferEvent::FileUploaded {
                    file_hash,
                    file_name,
                } => {
                    format!("file_uploaded:{}:{}", file_hash, file_name)
                }
                FileTransferEvent::FileDownloaded { file_path } => {
                    format!("file_downloaded:{}", file_path)
                }
                FileTransferEvent::FileNotFound { file_hash } => {
                    format!("file_not_found:{}", file_hash)
                }
                FileTransferEvent::Error { message } => {
                    format!("error:{}", message)
                }
            })
            .collect();
        Ok(mapped)
    } else {
        Ok(vec![])
    }
}

#[tauri::command]
fn get_available_storage() -> f64 {
    let storage = available_space(Path::new("/")).unwrap_or(0);
    (storage as f64 / 1024.0 / 1024.0 / 1024.0).floor()
}

fn main() {
    // Initialize logging for debug builds
    #[cfg(debug_assertions)]
    {
        use tracing_subscriber::{fmt, prelude::*, EnvFilter};
        tracing_subscriber::registry()
            .with(fmt::layer())
            .with(
                EnvFilter::from_default_env()
                    .add_directive("chiral_network=info".parse().unwrap())
                    .add_directive("libp2p=info".parse().unwrap())
                    .add_directive("libp2p_kad=debug".parse().unwrap())
                    .add_directive("libp2p_swarm=debug".parse().unwrap()),
            )
            .init();
    }

    // Parse command line arguments
    use clap::Parser;
    let args = headless::CliArgs::parse();

    // If running in headless mode, don't start the GUI
    if args.headless {
        println!("Running in headless mode...");

        // Create a tokio runtime for async operations
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

        // Run the headless mode
        if let Err(e) = runtime.block_on(headless::run_headless(args)) {
            eprintln!("Error in headless mode: {}", e);
            std::process::exit(1);
        }
        return;
    }

    println!("Starting Chiral Network...");

    tauri::Builder::default()
        .manage(AppState {
            geth: Mutex::new(GethProcess::new()),
            downloader: Arc::new(GethDownloader::new()),
            miner_address: Mutex::new(None),
            dht: Mutex::new(None),
            file_transfer: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            create_chiral_account,
            import_chiral_account,
            start_geth_node,
            stop_geth_node,
            save_account_to_keystore,
            load_account_from_keystore,
            list_keystore_accounts,
            remove_account_from_keystore,
            get_account_balance,
            get_network_peer_count,
            is_geth_running,
            check_geth_binary,
            download_geth_binary,
            set_miner_address,
            start_miner,
            stop_miner,
            get_miner_status,
            get_miner_hashrate,
            get_current_block,
            get_network_stats,
            get_miner_logs,
            get_miner_performance,
            get_blocks_mined,
            get_recent_mined_blocks_pub,
            get_cpu_temperature,
            start_dht_node,
            stop_dht_node,
            publish_file_metadata,
            search_file_metadata,
            connect_to_peer,
            get_dht_events,
            detect_locale,
            get_dht_health,
            get_dht_peer_count,
            get_dht_peer_id,
            get_nat_status,
            start_file_transfer_service,
            upload_file_to_network,
            upload_file_data_to_network,
            download_file_from_network,
            get_file_transfer_events,
            show_in_folder,
            get_available_storage
        ])
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // When window is destroyed, stop geth
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    if let Ok(mut geth) = state.geth.lock() {
                        let _ = geth.stop();
                        println!("Geth node stopped on window destroy");
                    }
                }
            }
        })
        .setup(|app| {
            // Clean up any orphaned geth processes on startup
            println!("Cleaning up any orphaned geth processes from previous sessions...");
            #[cfg(unix)]
            {
                use std::process::Command;
                // Kill any geth processes that might be running from previous sessions
                let _ = Command::new("pkill")
                    .arg("-9")
                    .arg("-f")
                    .arg("geth.*--datadir.*geth-data")
                    .output();
            }

            println!("App setup complete");
            println!("Window should be visible now!");

            let show_i = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let hide_i = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &hide_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("Chiral Network")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| match event {
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } => {
                        println!("Tray icon left-clicked");
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        println!("Show menu item clicked");
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "hide" => {
                        println!("Hide menu item clicked");
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                    "quit" => {
                        println!("Quit menu item clicked");
                        // Stop geth before exiting
                        if let Some(state) = app.try_state::<AppState>() {
                            if let Ok(mut geth) = state.geth.lock() {
                                let _ = geth.stop();
                                println!("Geth node stopped");
                            }
                        }
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // Get the main window and ensure it's visible
            if let Some(window) = app.get_webview_window("main") {
                window.show().unwrap();
                window.set_focus().unwrap();
                println!("Window shown and focused");

                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        // Prevent the window from closing and hide it instead
                        api.prevent_close();
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                });
            } else {
                println!("Could not find main window!");
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            tauri::RunEvent::ExitRequested { .. } => {
                println!("Exit requested event received");
                // Don't prevent exit, let it proceed naturally
            }
            tauri::RunEvent::Exit => {
                println!("App exiting, cleaning up geth...");
                // Stop geth before exiting
                if let Some(state) = app_handle.try_state::<AppState>() {
                    if let Ok(mut geth) = state.geth.lock() {
                        let _ = geth.stop();
                        println!("Geth node stopped on exit");
                    }
                }
            }
            _ => {}
        });
}
