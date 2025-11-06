// Shared bootstrap node configuration
// This module provides bootstrap nodes for both Tauri commands and headless mode

use tauri::command;

pub fn get_bootstrap_nodes() -> Vec<String> {
    vec![
        "/ip4/54.198.145.146/tcp/4001/p2p/12D3KooWNHdYWRTe98KMF1cDXXqGXvNjd1SAchDaeP5o4MsoJLu2"
            .to_string(),
        "/ip4/136.116.190.115/tcp/4001/p2p/12D3KooWETLNJUVLbkAbenbSPPdwN9ZLkBU3TLfyAeEUW2dsVptr"
            .to_string(),
    ]
}

#[command]
pub fn get_bootstrap_nodes_command() -> Vec<String> {
    get_bootstrap_nodes()
}
