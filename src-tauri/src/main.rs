#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod ethereum;
mod keystore;

use ethereum::{create_new_account, get_account_from_private_key, EthAccount, GethProcess};
use keystore::Keystore;
use std::sync::Mutex;
use tauri::Manager;
use tauri::State;

struct AppState {
    geth: Mutex<GethProcess>,
}

#[tauri::command]
async fn create_etc_account() -> Result<EthAccount, String> {
    create_new_account()
}

#[tauri::command]
async fn import_etc_account(private_key: String) -> Result<EthAccount, String> {
    get_account_from_private_key(&private_key)
}

#[tauri::command]
async fn start_geth_node(state: State<'_, AppState>, data_dir: String) -> Result<(), String> {
    let mut geth = state.geth.lock().map_err(|e| e.to_string())?;
    geth.start(&data_dir)
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

fn main() {
    println!("Starting Chiral Network...");

    tauri::Builder::default()
        .manage(AppState {
            geth: Mutex::new(GethProcess::new()),
        })
        .invoke_handler(tauri::generate_handler![
            create_etc_account,
            import_etc_account,
            start_geth_node,
            stop_geth_node,
            save_account_to_keystore,
            load_account_from_keystore,
            list_keystore_accounts,
            remove_account_from_keystore
        ])
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            println!("App setup complete");
            println!("Window should be visible now!");

            // Get the main window and ensure it's visible
            if let Some(window) = app.get_webview_window("main") {
                window.show().unwrap();
                window.set_focus().unwrap();
                println!("Window shown and focused");
            } else {
                println!("Could not find main window!");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
