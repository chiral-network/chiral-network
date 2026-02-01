// app_state.rs
// Core network services shared across modules
//
// This struct provides a clean way for modules like protocol_manager to access
// shared services (DHT, FileTransfer, WebRTC) via Tauri's state management
// instead of requiring setter methods.

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::dht::DhtService;
use crate::file_transfer::FileTransferService;
use crate::webrtc_service::WebRTCService;

/// Core network services shared across modules.
/// Access via: `app_handle.state::<CoreServices>()`
pub struct CoreServices {
    pub dht: Mutex<Option<Arc<DhtService>>>,
    pub file_transfer: Mutex<Option<Arc<FileTransferService>>>,
    pub webrtc: Mutex<Option<Arc<WebRTCService>>>,
}

impl CoreServices {
    pub fn new() -> Self {
        Self {
            dht: Mutex::new(None),
            file_transfer: Mutex::new(None),
            webrtc: Mutex::new(None),
        }
    }
}

impl Default for CoreServices {
    fn default() -> Self {
        Self::new()
    }
}
