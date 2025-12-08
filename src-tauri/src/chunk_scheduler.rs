use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use once_cell::sync::Lazy;
use tauri::command;

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChunkRequest {
    pub chunk_index: usize,
    pub peer_id: String,
    pub requested_at: u128,
    pub timeout_ms: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub peer_id: String,
    pub available: bool,
    pub last_seen: u128,
    pub pending_requests: usize,
    pub max_concurrent: usize,
    pub avg_response_time: u64,
    pub failure_count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerConfig {
    pub max_concurrent_per_peer: usize,
    pub chunk_timeout_ms: u64,
    pub max_retries: usize,
    pub peer_selection_strategy: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChunkMeta {
    pub index: usize,
    pub size: usize,
    pub checksum: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChunkManifest {
    pub chunks: Vec<ChunkMeta>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ChunkState {
    UNREQUESTED,
    REQUESTED,
    RECEIVED,
    CORRUPTED,
}

pub struct ChunkScheduler {
    config: SchedulerConfig,
    peers: HashMap<String, PeerInfo>,
    active_requests: HashMap<usize, ChunkRequest>,
    chunk_states: Vec<ChunkState>,
    retry_count: HashMap<usize, usize>,
}

impl ChunkScheduler {
    pub fn new(cfg: Option<SchedulerConfig>) -> Self {
        let default = SchedulerConfig {
            max_concurrent_per_peer: 3,
            chunk_timeout_ms: 30_000,
            max_retries: 3,
            peer_selection_strategy: "load-balanced".to_string(),
        };
        let config = cfg.unwrap_or(default);
        Self {
            config,
            peers: HashMap::new(),
            active_requests: HashMap::new(),
            chunk_states: Vec::new(),
            retry_count: HashMap::new(),
        }
    }

    pub fn init_scheduler(&mut self, manifest: ChunkManifest) {
        self.chunk_states = manifest
            .chunks
            .iter()
            .map(|_| ChunkState::UNREQUESTED)
            .collect();
        self.active_requests.clear();
        self.retry_count.clear();
    }

    pub fn add_peer(&mut self, peer_id: String, max_concurrent: Option<usize>) {
        self.peers.insert(
            peer_id.clone(),
            PeerInfo {
                peer_id,
                available: true,
                last_seen: now_ms(),
                pending_requests: 0,
                max_concurrent: max_concurrent
                    .unwrap_or(self.config.max_concurrent_per_peer),
                avg_response_time: 1000,
                failure_count: 0,
            },
        );
    }

    pub fn remove_peer(&mut self, peer_id: &str) {
        let mut to_unreq = Vec::new();
        for (&chunk_index, req) in self.active_requests.iter() {
            if req.peer_id == peer_id {
                to_unreq.push(chunk_index);
            }
        }
        for idx in to_unreq {
            self.active_requests.remove(&idx);
            if idx < self.chunk_states.len() {
                self.chunk_states[idx] = ChunkState::UNREQUESTED;
            }
        }
        self.peers.remove(peer_id);
    }

    pub fn update_peer_health(&mut self, peer_id: &str, available: bool, response_time_ms: Option<u64>) {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.available = available;
            peer.last_seen = now_ms();
            if let Some(rt) = response_time_ms {
                peer.avg_response_time = ((peer.avg_response_time as f64) * 0.8
                    + (rt as f64) * 0.2) as u64;
            }
            if !available {
                peer.failure_count += 1;
            }
        }
    }

    pub fn on_chunk_received(&mut self, chunk_index: usize) {
        if let Some(req) = self.active_requests.remove(&chunk_index) {
            // Avoid borrowing self mutably twice by updating the peer directly here
            if let Some(peer) = self.peers.get_mut(&req.peer_id) {
                peer.pending_requests = peer.pending_requests.saturating_sub(1);
                let response_time = now_ms().saturating_sub(req.requested_at);
                // Update last_seen and avg_response_time directly instead of calling update_peer_health
                peer.last_seen = now_ms();
                peer.avg_response_time = ((peer.avg_response_time as f64) * 0.8
                    + (response_time as f64) * 0.2) as u64;
                // successful response -> don't increment failure_count
            }
        }
        if chunk_index < self.chunk_states.len() {
            self.chunk_states[chunk_index] = ChunkState::RECEIVED;
        }
    }

    pub fn on_chunk_failed(&mut self, chunk_index: usize, mark_corrupted: bool) {
        if let Some(req) = self.active_requests.remove(&chunk_index) {
            if let Some(peer) = self.peers.get_mut(&req.peer_id) {
                peer.pending_requests = peer.pending_requests.saturating_sub(1);
                peer.failure_count += 1;
            }
        }

        if chunk_index < self.chunk_states.len() {
            self.chunk_states[chunk_index] = if mark_corrupted { ChunkState::CORRUPTED } else { ChunkState::UNREQUESTED };
        }

        let retries = self.retry_count.get(&chunk_index).cloned().unwrap_or(0);
        self.retry_count.insert(chunk_index, retries + 1);
    }

    pub fn get_next_requests(&mut self, max_requests: usize) -> Vec<ChunkRequest> {
        let mut requests = Vec::new();
        let now = now_ms();

        self.handle_timeouts(now);

        // Compute chunks to request before taking mutable borrows to peers
        let chunks_to_request = self.get_chunks_to_request(max_requests);

        let mut available_peers: Vec<_> = self.peers.values_mut()
            .filter(|p| p.available && p.pending_requests < p.max_concurrent)
            .collect();

        match self.config.peer_selection_strategy.as_str() {
            "fastest-first" => available_peers.sort_by_key(|p| p.avg_response_time),
            "load-balanced" => available_peers.sort_by_key(|p| (p.pending_requests, p.max_concurrent)),
            _ => {}
        }

        let mut peer_index = 0usize;

        for chunk_index in chunks_to_request {
            if requests.len() >= max_requests { break; }
            if available_peers.is_empty() { break; }

            // wrap-around selection
            let mut selected = None;
            let len = available_peers.len();
            for _ in 0..len {
                let idx = peer_index % len;
                let p = &mut available_peers[idx];
                if p.pending_requests < p.max_concurrent {
                    selected = Some(p.peer_id.clone());
                    p.pending_requests += 1;
                    break;
                }
                peer_index += 1;
            }

            if let Some(peer_id) = selected {
                let req = ChunkRequest {
                    chunk_index,
                    peer_id: peer_id.clone(),
                    requested_at: now,
                    timeout_ms: self.config.chunk_timeout_ms,
                };
                self.active_requests.insert(chunk_index, req.clone());
                if chunk_index < self.chunk_states.len() {
                    self.chunk_states[chunk_index] = ChunkState::REQUESTED;
                }
                requests.push(req);
                peer_index += 1;
            } else {
                // no peer can accept more requests
                break;
            }
        }

        requests
    }

    fn handle_timeouts(&mut self, now: u128) {
        let timed_out: Vec<usize> = self.active_requests.iter()
            .filter_map(|(&idx, req)| {
                if now.saturating_sub(req.requested_at) > (req.timeout_ms as u128) {
                    Some(idx)
                } else { None }
            })
            .collect();

        for idx in timed_out {
            self.on_chunk_failed(idx, false);
        }
    }

    fn get_chunks_to_request(&self, max_chunks: usize) -> Vec<usize> {
        let mut out = Vec::new();
        for (i, state) in self.chunk_states.iter().enumerate() {
            if out.len() >= max_chunks { break; }
            let retries = self.retry_count.get(&i).cloned().unwrap_or(0);
            if *state == ChunkState::UNREQUESTED && retries < self.config.max_retries {
                out.push(i);
            }
        }
        out
    }

    pub fn get_scheduler_state(&self) -> serde_json::Value {
        serde_json::json!({
            "chunk_states": self.chunk_states.iter().map(|s| format!("{:?}", s)).collect::<Vec<_>>(),
            "active_request_count": self.active_requests.len(),
            "available_peer_count": self.peers.values().filter(|p| p.available).count(),
            "total_peer_count": self.peers.len(),
            "completed_chunks": self.chunk_states.iter().filter(|s| **s == ChunkState::RECEIVED).count(),
            "total_chunks": self.chunk_states.len()
        })
    }

    pub fn is_complete(&self) -> bool {
        self.chunk_states.iter().all(|s| *s == ChunkState::RECEIVED)
    }

    pub fn get_active_requests(&self) -> Vec<ChunkRequest> {
        self.active_requests.values().cloned().collect()
    }

    pub fn get_peers(&self) -> Vec<PeerInfo> {
        self.peers.values().cloned().collect()
    }
}

// Global scheduler instance guarded by a mutex. This keeps state in the Tauri backend.
static SCHEDULER: Lazy<Mutex<ChunkScheduler>> = Lazy::new(|| Mutex::new(ChunkScheduler::new(None)));

#[command]
pub fn init_scheduler(manifest: ChunkManifest) -> Result<(), String> {
    let mut s = SCHEDULER.lock().map_err(|e| format!("lock error: {}", e))?;
    s.init_scheduler(manifest);
    Ok(())
}

#[command]
pub fn add_peer(peer_id: String, max_concurrent: Option<usize>) -> Result<(), String> {
    let mut s = SCHEDULER.lock().map_err(|e| format!("lock error: {}", e))?;
    s.add_peer(peer_id, max_concurrent);
    Ok(())
}

#[command]
pub fn remove_peer(peer_id: String) -> Result<(), String> {
    let mut s = SCHEDULER.lock().map_err(|e| format!("lock error: {}", e))?;
    s.remove_peer(&peer_id);
    Ok(())
}

#[command]
pub fn update_peer_health(peer_id: String, available: bool, response_time_ms: Option<u64>) -> Result<(), String> {
    let mut s = SCHEDULER.lock().map_err(|e| format!("lock error: {}", e))?;
    s.update_peer_health(&peer_id, available, response_time_ms);
    Ok(())
}

#[command]
pub fn on_chunk_received(chunk_index: usize) -> Result<(), String> {
    let mut s = SCHEDULER.lock().map_err(|e| format!("lock error: {}", e))?;
    s.on_chunk_received(chunk_index);
    Ok(())
}

#[command]
pub fn on_chunk_failed(chunk_index: usize, mark_corrupted: bool) -> Result<(), String> {
    let mut s = SCHEDULER.lock().map_err(|e| format!("lock error: {}", e))?;
    s.on_chunk_failed(chunk_index, mark_corrupted);
    Ok(())
}

#[command]
pub fn get_next_requests(max_requests: usize) -> Result<Vec<ChunkRequest>, String> {
    let mut s = SCHEDULER.lock().map_err(|e| format!("lock error: {}", e))?;
    Ok(s.get_next_requests(max_requests))
}

#[command]
pub fn get_scheduler_state() -> Result<serde_json::Value, String> {
    let s = SCHEDULER.lock().map_err(|e| format!("lock error: {}", e))?;
    Ok(s.get_scheduler_state())
}

#[command]
pub fn is_complete() -> Result<bool, String> {
    let s = SCHEDULER.lock().map_err(|e| format!("lock error: {}", e))?;
    Ok(s.is_complete())
}

#[command]
pub fn get_active_requests() -> Result<Vec<ChunkRequest>, String> {
    let s = SCHEDULER.lock().map_err(|e| format!("lock error: {}", e))?;
    Ok(s.get_active_requests())
}

#[command]
pub fn get_peers() -> Result<Vec<PeerInfo>, String> {
    let s = SCHEDULER.lock().map_err(|e| format!("lock error: {}", e))?;
    Ok(s.get_peers())
}
