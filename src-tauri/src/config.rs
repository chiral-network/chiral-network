use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolConfig {
    pub download_dir: String,
    pub port_range: Range<u16>,
    pub enable_dht: bool,
    pub connect_timeout: u64,
    pub max_download_speed: Option<u64>, // bytes per second
    pub max_upload_speed: Option<u64>,   // bytes per second
    pub max_connections: u32,
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            download_dir: "./downloads".to_string(),
            port_range: 6881..6891,
            enable_dht: true,
            connect_timeout: 10,
            max_download_speed: None,
            max_upload_speed: None,
            max_connections: 50,
        }
    }
}