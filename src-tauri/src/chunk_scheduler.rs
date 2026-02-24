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

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manifest(n: usize) -> ChunkManifest {
        ChunkManifest {
            chunks: (0..n)
                .map(|i| ChunkMeta {
                    index: i,
                    size: 262144,
                    checksum: Some(format!("hash-{}", i)),
                })
                .collect(),
        }
    }

    fn make_scheduler(chunks: usize, peers: &[&str]) -> ChunkScheduler {
        let mut s = ChunkScheduler::new(None);
        s.init_scheduler(make_manifest(chunks));
        for p in peers {
            s.add_peer(p.to_string(), None);
        }
        s
    }

    // --- Initialization ---

    #[test]
    fn test_new_default_config() {
        let s = ChunkScheduler::new(None);
        assert_eq!(s.config.max_concurrent_per_peer, 3);
        assert_eq!(s.config.chunk_timeout_ms, 30_000);
        assert_eq!(s.config.max_retries, 3);
        assert_eq!(s.config.peer_selection_strategy, "load-balanced");
    }

    #[test]
    fn test_new_custom_config() {
        let cfg = SchedulerConfig {
            max_concurrent_per_peer: 5,
            chunk_timeout_ms: 10_000,
            max_retries: 1,
            peer_selection_strategy: "fastest-first".to_string(),
        };
        let s = ChunkScheduler::new(Some(cfg));
        assert_eq!(s.config.max_concurrent_per_peer, 5);
        assert_eq!(s.config.max_retries, 1);
    }

    #[test]
    fn test_init_scheduler_creates_chunk_states() {
        let mut s = ChunkScheduler::new(None);
        s.init_scheduler(make_manifest(5));
        assert_eq!(s.chunk_states.len(), 5);
        assert!(s.chunk_states.iter().all(|st| *st == ChunkState::UNREQUESTED));
    }

    #[test]
    fn test_init_scheduler_resets_state() {
        let mut s = make_scheduler(3, &["peer-1"]);
        s.get_next_requests(1);
        assert!(!s.active_requests.is_empty());

        // Re-init should clear everything
        s.init_scheduler(make_manifest(5));
        assert!(s.active_requests.is_empty());
        assert!(s.retry_count.is_empty());
        assert_eq!(s.chunk_states.len(), 5);
    }

    // --- Peer management ---

    #[test]
    fn test_add_peer() {
        let mut s = ChunkScheduler::new(None);
        s.add_peer("peer-A".to_string(), None);
        assert_eq!(s.peers.len(), 1);
        let peer = s.peers.get("peer-A").unwrap();
        assert!(peer.available);
        assert_eq!(peer.pending_requests, 0);
        assert_eq!(peer.max_concurrent, 3); // default
    }

    #[test]
    fn test_add_peer_custom_max_concurrent() {
        let mut s = ChunkScheduler::new(None);
        s.add_peer("peer-A".to_string(), Some(10));
        assert_eq!(s.peers.get("peer-A").unwrap().max_concurrent, 10);
    }

    #[test]
    fn test_remove_peer_unrequests_active_chunks() {
        let mut s = make_scheduler(5, &["peer-A"]);
        s.get_next_requests(3);
        assert_eq!(s.active_requests.len(), 3);

        s.remove_peer("peer-A");
        assert!(s.active_requests.is_empty());
        assert!(s.peers.is_empty());
        // Chunks should be back to UNREQUESTED
        assert!(s.chunk_states[0..3].iter().all(|st| *st == ChunkState::UNREQUESTED));
    }

    #[test]
    fn test_update_peer_health_available() {
        let mut s = make_scheduler(3, &["peer-A"]);
        s.update_peer_health("peer-A", false, None);
        let p = s.peers.get("peer-A").unwrap();
        assert!(!p.available);
        assert_eq!(p.failure_count, 1);
    }

    #[test]
    fn test_update_peer_health_response_time_ewma() {
        let mut s = make_scheduler(3, &["peer-A"]);
        // Initial avg_response_time is 1000
        s.update_peer_health("peer-A", true, Some(200));
        let p = s.peers.get("peer-A").unwrap();
        // EWMA: 1000 * 0.8 + 200 * 0.2 = 840
        assert_eq!(p.avg_response_time, 840);
    }

    // --- Chunk requests and scheduling ---

    #[test]
    fn test_get_next_requests_basic() {
        let mut s = make_scheduler(5, &["peer-A"]);
        let reqs = s.get_next_requests(3);
        assert_eq!(reqs.len(), 3);
        assert_eq!(reqs[0].chunk_index, 0);
        assert_eq!(reqs[1].chunk_index, 1);
        assert_eq!(reqs[2].chunk_index, 2);
        assert!(reqs.iter().all(|r| r.peer_id == "peer-A"));
    }

    #[test]
    fn test_get_next_requests_respects_max_concurrent() {
        let mut s = ChunkScheduler::new(Some(SchedulerConfig {
            max_concurrent_per_peer: 2,
            chunk_timeout_ms: 30_000,
            max_retries: 3,
            peer_selection_strategy: "load-balanced".to_string(),
        }));
        s.init_scheduler(make_manifest(10));
        s.add_peer("peer-A".to_string(), None);

        let reqs = s.get_next_requests(10);
        // Only 2 requests because max_concurrent_per_peer = 2
        assert_eq!(reqs.len(), 2);
    }

    #[test]
    fn test_get_next_requests_distributes_across_peers() {
        let mut s = make_scheduler(6, &["peer-A", "peer-B"]);
        let reqs = s.get_next_requests(6);
        assert_eq!(reqs.len(), 6);

        let a_count = reqs.iter().filter(|r| r.peer_id == "peer-A").count();
        let b_count = reqs.iter().filter(|r| r.peer_id == "peer-B").count();
        assert_eq!(a_count, 3);
        assert_eq!(b_count, 3);
    }

    #[test]
    fn test_get_next_requests_no_peers_returns_empty() {
        let mut s = ChunkScheduler::new(None);
        s.init_scheduler(make_manifest(5));
        let reqs = s.get_next_requests(5);
        assert!(reqs.is_empty());
    }

    #[test]
    fn test_get_next_requests_skips_unavailable_peer() {
        let mut s = make_scheduler(5, &["peer-A", "peer-B"]);
        s.update_peer_health("peer-A", false, None);

        let reqs = s.get_next_requests(5);
        // Only peer-B should be used
        assert!(reqs.iter().all(|r| r.peer_id == "peer-B"));
        assert_eq!(reqs.len(), 3); // max_concurrent default = 3
    }

    #[test]
    fn test_get_next_requests_skips_already_requested() {
        let mut s = make_scheduler(5, &["peer-A"]);
        let first = s.get_next_requests(2);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].chunk_index, 0);
        assert_eq!(first[1].chunk_index, 1);

        // Next batch should start from chunk 2
        let second = s.get_next_requests(2);
        assert_eq!(second.len(), 1); // only 1 more because max_concurrent=3 and 2 pending
        assert_eq!(second[0].chunk_index, 2);
    }

    // --- Chunk received / failed ---

    #[test]
    fn test_on_chunk_received() {
        let mut s = make_scheduler(3, &["peer-A"]);
        s.get_next_requests(1);

        s.on_chunk_received(0);
        assert_eq!(s.chunk_states[0], ChunkState::RECEIVED);
        assert!(s.active_requests.is_empty());
        assert_eq!(s.peers.get("peer-A").unwrap().pending_requests, 0);
    }

    #[test]
    fn test_on_chunk_failed_marks_unrequested() {
        let mut s = make_scheduler(3, &["peer-A"]);
        s.get_next_requests(1);

        s.on_chunk_failed(0, false);
        assert_eq!(s.chunk_states[0], ChunkState::UNREQUESTED);
        assert_eq!(*s.retry_count.get(&0).unwrap(), 1);
        assert_eq!(s.peers.get("peer-A").unwrap().failure_count, 1);
    }

    #[test]
    fn test_on_chunk_failed_marks_corrupted() {
        let mut s = make_scheduler(3, &["peer-A"]);
        s.get_next_requests(1);

        s.on_chunk_failed(0, true);
        assert_eq!(s.chunk_states[0], ChunkState::CORRUPTED);
    }

    #[test]
    fn test_retry_limit_prevents_re_request() {
        let mut s = ChunkScheduler::new(Some(SchedulerConfig {
            max_concurrent_per_peer: 10,
            chunk_timeout_ms: 30_000,
            max_retries: 2,
            peer_selection_strategy: "load-balanced".to_string(),
        }));
        s.init_scheduler(make_manifest(1));
        s.add_peer("peer-A".to_string(), None);

        // Request and fail max_retries times
        for _ in 0..2 {
            s.get_next_requests(1);
            s.on_chunk_failed(0, false);
        }

        // Should not schedule again after max_retries
        let reqs = s.get_next_requests(1);
        assert!(reqs.is_empty());
    }

    // --- Completion ---

    #[test]
    fn test_is_complete() {
        let mut s = make_scheduler(3, &["peer-A"]);
        assert!(!s.is_complete());

        s.get_next_requests(3);
        s.on_chunk_received(0);
        s.on_chunk_received(1);
        s.on_chunk_received(2);
        assert!(s.is_complete());
    }

    #[test]
    fn test_is_complete_with_failures_false() {
        let mut s = make_scheduler(3, &["peer-A"]);
        s.get_next_requests(3);
        s.on_chunk_received(0);
        s.on_chunk_received(1);
        s.on_chunk_failed(2, false);
        assert!(!s.is_complete());
    }

    // --- Scheduler state reporting ---

    #[test]
    fn test_get_scheduler_state() {
        let mut s = make_scheduler(5, &["peer-A", "peer-B"]);
        s.get_next_requests(3);
        s.on_chunk_received(0);

        let state = s.get_scheduler_state();
        assert_eq!(state["total_chunks"], 5);
        assert_eq!(state["completed_chunks"], 1);
        assert_eq!(state["active_request_count"], 2);
        assert_eq!(state["total_peer_count"], 2);
    }

    #[test]
    fn test_get_active_requests() {
        let mut s = make_scheduler(5, &["peer-A"]);
        s.get_next_requests(2);
        let active = s.get_active_requests();
        assert_eq!(active.len(), 2);
    }

    #[test]
    fn test_get_peers() {
        let s = make_scheduler(3, &["peer-A", "peer-B", "peer-C"]);
        let peers = s.get_peers();
        assert_eq!(peers.len(), 3);
    }

    // --- Multi-seeder scenarios ---

    #[test]
    fn test_three_seeders_round_robin() {
        let mut s = make_scheduler(9, &["s1", "s2", "s3"]);
        let reqs = s.get_next_requests(9);
        assert_eq!(reqs.len(), 9);

        let s1_count = reqs.iter().filter(|r| r.peer_id == "s1").count();
        let s2_count = reqs.iter().filter(|r| r.peer_id == "s2").count();
        let s3_count = reqs.iter().filter(|r| r.peer_id == "s3").count();
        assert_eq!(s1_count, 3);
        assert_eq!(s2_count, 3);
        assert_eq!(s3_count, 3);
    }

    #[test]
    fn test_seeder_failure_redistributes_chunks() {
        let mut s = make_scheduler(6, &["fast", "slow"]);
        let reqs = s.get_next_requests(6);
        assert_eq!(reqs.len(), 6);

        // "slow" seeder fails all its chunks
        for r in &reqs {
            if r.peer_id == "slow" {
                s.on_chunk_failed(r.chunk_index, false);
            } else {
                s.on_chunk_received(r.chunk_index);
            }
        }

        // Remove slow seeder
        s.remove_peer("slow");

        // Retry failed chunks — should go to "fast" only
        let retries = s.get_next_requests(3);
        assert!(retries.iter().all(|r| r.peer_id == "fast"));
    }

    #[test]
    fn test_fastest_first_strategy() {
        let mut s = ChunkScheduler::new(Some(SchedulerConfig {
            max_concurrent_per_peer: 10,
            chunk_timeout_ms: 30_000,
            max_retries: 3,
            peer_selection_strategy: "fastest-first".to_string(),
        }));
        s.init_scheduler(make_manifest(6));
        s.add_peer("slow-peer".to_string(), None);
        s.add_peer("fast-peer".to_string(), None);

        // Make fast-peer actually fast
        s.update_peer_health("fast-peer", true, Some(50));
        // Make slow-peer slow
        s.update_peer_health("slow-peer", true, Some(5000));

        let reqs = s.get_next_requests(6);
        // fast-peer should get first assignments
        assert_eq!(reqs[0].peer_id, "fast-peer");
    }
}
