#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod ethereum;
mod keystore;
mod geth_downloader;

use ethereum::{
    create_new_account, get_account_from_private_key, get_balance, get_peer_count,
    start_mining, stop_mining, get_mining_status, get_hashrate, get_block_number,
    get_network_difficulty, get_network_hashrate, get_mining_logs,
    EthAccount, GethProcess
};
use keystore::Keystore;
use geth_downloader::GethDownloader;
use sysinfo::{Components, System, MINIMUM_CPU_UPDATE_INTERVAL};
use systemstat::{Platform, System as SystemStat};
use std::{sync::{Arc, Mutex}, time::Instant, fs, io::{self, Read}, time::Duration};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, Emitter, State
};

struct AppState {
    geth: Mutex<GethProcess>,
    downloader: Arc<GethDownloader>,
    miner_address: Mutex<Option<String>>,
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
async fn save_account_to_keystore(address: String, private_key: String, password: String) -> Result<(), String> {
    let mut keystore = Keystore::load()?;
    keystore.add_account(address, &private_key, &password)?;
    Ok(())
}

#[tauri::command]
async fn load_account_from_keystore(address: String, password: String) -> Result<EthAccount, String> {
    let keystore = Keystore::load()?;
    let private_key = keystore.get_account(&address, &password)?;
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
async fn download_geth_binary(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let downloader = state.downloader.clone();
    let app_handle = app.clone();
    
    downloader.download_geth(move |progress| {
        let _ = app_handle.emit("geth-download-progress", progress);
    }).await
}

#[tauri::command]
async fn set_miner_address(state: State<'_, AppState>, address: String) -> Result<(), String> {
    let mut miner_address = state.miner_address.lock().map_err(|e| e.to_string())?;
    *miner_address = Some(address);
    Ok(())
}

#[tauri::command]
async fn start_miner(state: State<'_, AppState>, address: String, threads: u32, data_dir: String) -> Result<(), String> {
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
                
                println!("Waiting for geth to start up... (attempt {}/{})", attempts, max_attempts);
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
        },
        Err(e) => Err(format!("Failed to start mining: {}", e))
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
    
    // Try systemstat for additional platforms
    let stat_sys = SystemStat::new();
    if let Ok(temp) = stat_sys.cpu_temp() {
        return Some(temp);
    }

    // Platform-specific fallbacks
    #[cfg(target_os = "linux")]
    {
        if let Ok(temp) = get_cpu_temperature_linux_sync() {
            return Some(temp);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(temp) = get_cpu_temperature_windows_sync() {
            return Some(temp);
        }
    }

    None
}

// ---------------- PLATFORM-SPECIFIC TEMPERATURE IMPLEMENTATIONS ----------------
#[cfg(target_os = "linux")]
fn get_cpu_temperature_linux_sync() -> Result<f32, String> {
    // Search hwmon devices for CPU package temperature
    let hwmon_root = "/sys/class/hwmon";
    let dirs = fs::read_dir(hwmon_root).map_err(|e| format!("read_dir hwmon failed: {}", e))?;
    for entry in dirs {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        // Try to read name to identify sensor
        let name_path = path.join("name");
        let name = fs::read_to_string(&name_path).unwrap_or_default().to_lowercase();
        if !(name.contains("coretemp") || name.contains("k10temp") || name.contains("cpu") || name.contains("zenpower")) {
            // still scan labels below
        }
        // Iterate temp*_label to find package/core label
        if let Ok(dir_iter) = fs::read_dir(&path) {
            for file in dir_iter.flatten() {
                let fname = file.file_name();
                let fname = fname.to_string_lossy();
                if fname.starts_with("temp") && fname.ends_with("_label") {
                    let label = fs::read_to_string(file.path()).unwrap_or_default().to_lowercase();
                    if label.contains("package") || label.contains("tdie") || label.contains("tctl") || label.contains("cpu") {
                        let input_path = file.path().with_file_name(fname.replace("_label", "_input"));
                        if let Ok(raw) = fs::read_to_string(&input_path) {
                            if let Ok(millideg) = raw.trim().parse::<i64>() {
                                return Ok(millideg as f32 / 1000.0);
                            }
                        }
                    }
                }
            }
        }
        // Fallback: any temp*_input
        if let Ok(dir_iter) = fs::read_dir(&path) {
            for file in dir_iter.flatten() {
                let fname = file.file_name();
                let fname = fname.to_string_lossy();
                if fname.starts_with("temp") && fname.ends_with("_input") {
                    if let Ok(raw) = fs::read_to_string(file.path()) {
                        if let Ok(millideg) = raw.trim().parse::<i64>() {
                            return Ok(millideg as f32 / 1000.0);
                        }
                    }
                }
            }
        }
    }
    Err("No temperature sensor found".to_string())
}

#[cfg(target_os = "windows")]
fn get_cpu_temperature_windows_sync() -> Result<f32, String> {
    use wmi::{COMLibrary, WMIConnection};
    use serde::Deserialize;

    #[derive(Deserialize, Debug)]
    struct Temp { CurrentTemperature: Option<i64> }

    let com_con = COMLibrary::new().map_err(|e| format!("COM init failed: {}", e))?;
    let wmi_con = WMIConnection::new(com_con).map_err(|e| format!("WMI connect failed: {}", e))?;
    let results: Vec<Temp> = wmi_con.query().map_err(|e| format!("WMI query failed: {}", e))?;
    // MSAcpi_ThermalZoneTemperature returns tenths of Kelvin
    for t in results {
        if let Some(val) = t.CurrentTemperature {
            if val > 0 {
                let c = (val as f32 / 10.0) - 273.15;
                return Ok(c);
            }
        }
    }
    Err("No temperature sensor found".to_string())
}

// CPU usage command
#[tauri::command]
async fn get_cpu_usage() -> Result<f32, String> {
    let mut sys = System::new_all();
    sys.refresh_cpu_all();
    Ok(sys.global_cpu_usage())
}

// Platform-specific: CPU package power (watts)
#[tauri::command]
async fn get_cpu_package_power() -> Result<Option<f32>, String> {
    #[cfg(target_os = "linux")]
    {
        return get_cpu_power_linux().await;
    }
    #[cfg(target_os = "windows")]
    {
        // Windows requires vendor/tooling (e.g., Intel Power Gadget, LibreHardwareMonitor). Not bundled.
        return Ok(None);
    }
    #[cfg(target_os = "macos")]
    {
        // macOS power requires powermetrics with elevated privileges. Not used here.
        return Ok(None);
    }
}

// ---------------- LINUX IMPLEMENTATIONS ----------------
#[cfg(target_os = "linux")]
async fn get_cpu_power_linux() -> Result<Option<f32>, String> {
    // Intel RAPL energy counter in microjoules
    // Typical path: /sys/devices/virtual/powercap/intel-rapl:0/energy_uj
    let rapl_root = "/sys/devices/virtual/powercap";
    let mut energy_file: Option<String> = None;
    if let Ok(entries) = fs::read_dir(rapl_root) {
        for entry in entries.flatten() {
            let p = entry.path();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.starts_with("intel-rapl:") {
                let ef = p.join("energy_uj");
                if ef.exists() {
                    energy_file = Some(ef.to_string_lossy().to_string());
                    break;
                }
            }
        }
    }
    let energy_path = match energy_file { Some(p) => p, None => return Ok(None) };

    // Read two samples and compute watts = dE/dt
    let e1 = read_u64_from_file(&energy_path).map_err(|e| e.to_string())?;
    let t1 = std::time::Instant::now();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let e2 = read_u64_from_file(&energy_path).map_err(|e| e.to_string())?;
    let dt = t1.elapsed().as_secs_f32();
    if dt <= 0.0 { return Ok(None); }

    // Handle wrap using max_energy_range_uj if present
    let mut de = if e2 >= e1 { e2 - e1 } else { 0 } as f64;
    if e2 < e1 {
        let max_path = std::path::Path::new(&energy_path).with_file_name("max_energy_range_uj");
        if let Ok(max) = read_u64_from_file(max_path.to_string_lossy().as_ref()) {
            de = (e2 as u128 + max as u128 - e1 as u128) as f64;
        }
    }
    // microjoules to joules
    let joules = de / 1_000_000.0;
    let watts = (joules as f32) / dt;
    Ok(Some(watts))
}

#[cfg(target_os = "linux")]
fn read_u64_from_file(path: &str) -> io::Result<u64> {
    let mut s = String::new();
    fs::File::open(path)?.read_to_string(&mut s)?;
    let v = s.trim().parse::<u64>().map_err(|_e| io::Error::new(io::ErrorKind::Other, "parse u64"))?;
    Ok(v)
}

fn main() {
    println!("Starting Chiral Network...");

    tauri::Builder::default()
        .manage(AppState {
            geth: Mutex::new(GethProcess::new()),
            downloader: Arc::new(GethDownloader::new()),
            miner_address: Mutex::new(None),
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
            get_cpu_usage,
            get_cpu_temperature,
            get_cpu_package_power
        ])
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
