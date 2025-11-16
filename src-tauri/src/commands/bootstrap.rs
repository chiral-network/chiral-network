// Shared bootstrap node configuration
// This module provides bootstrap nodes for both Tauri commands and headless mode

use tauri::command;

pub fn get_bootstrap_nodes() -> Vec<String> {
    vec![
        // "/ip4/134.199.240.145/tcp/4001/p2p/12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE"
        //     .to_string(),
        "/ip4/136.116.190.115/tcp/4001/p2p/12D3KooWPpgX9ZigUgdo6KKMfCjPSD5G6m2iXRBvEzch2SeQKJym"
            .to_string(),
        "/ip4/136.116.190.115/tcp/4002/p2p/12D3KooWSahP5pFRCEfaziPEba7urXGeif6T1y8jmodzdFUvzBHj".to_string()
        // "/ip4/130.245.173.105/tcp/4001/p2p/12D3KooWGFRvjXFBoU9y6xdteqP1kzctAXrYPoaDGmTGRHybZ6rp"
        //     .to_string(),
    ]
}

#[command]
pub fn get_bootstrap_nodes_command() -> Vec<String> {
    get_bootstrap_nodes()
}
