// Shared bootstrap node configuration
// This module provides bootstrap nodes for both Tauri commands and headless mode

use tauri::command;

pub fn get_bootstrap_nodes() -> Vec<String> {
    vec![
        // Primary bootstrap node (IPv4)
        "/ip4/130.245.173.73/tcp/4001/p2p/12D3KooWRNWj5Knx1yV6mqBt6Wf7buVMh3g4vpsHRxKtTr9ynXPu"
            .to_string(),
        // Primary bootstrap node (IPv6 via 6to4 tunnel)
        "/ip6/2002:82f5:ad49::1/tcp/4001/p2p/12D3KooWRNWj5Knx1yV6mqBt6Wf7buVMh3g4vpsHRxKtTr9ynXPu"
            .to_string(),
        "/ip4/134.199.240.145/tcp/4001/p2p/12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE"
            .to_string(),
        "/ip4/34.44.149.113/tcp/4001/p2p/12D3KooWETLNJUVLbkAbenbSPPdwN9ZLkBU3TLfyAeEUW2dsVptr"
            .to_string(),
        "/ip4/130.245.173.105/tcp/4001/p2p/12D3KooWSDDA2jyo6Cynr7SHPfhdQoQazu1jdUEAp7rLKKKLqqTr"
            .to_string(),
    ]
}

#[command]
pub fn get_bootstrap_nodes_command() -> Vec<String> {
    get_bootstrap_nodes()
}
