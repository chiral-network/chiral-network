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
use std::sync::{Arc, Mutex};
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
            
            // Wait for geth to start up
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            
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
            get_miner_logs
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
