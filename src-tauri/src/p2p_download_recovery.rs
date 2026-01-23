// p2p chunk recovery with dht integration

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tracing::{debug, info, warn};

// 256kb chunks like ipfs
pub const CHUNK_SIZE: u64 = 256 * 1024;

// max concurrent resumes to avoid bandwidth saturation
pub const MAX_RESUMES: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoveryErr {
    NotFound,
    VersionMismatch(u32),
    IoErr(String),
    ParseErr(String),
    NoSpace,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChunkState {
    Pending,
    Downloaded,
    Verified,
    Failed,
}

impl Default for ChunkState {
    fn default() -> Self {
        ChunkState::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMeta {
    pub idx: u32,
    pub offset: u64,
    pub size: u32,
    pub hash: Option<String>,
    pub state: ChunkState,
}

impl ChunkMeta {
    pub fn new(idx: u32, offset: u64, size: u32, hash: Option<String>) -> Self {
        Self {
            idx,
            offset,
            size,
            hash,
            state: ChunkState::Pending,
        }
    }
}

// persistent state stored as .chiral.p2p.meta.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlState {
    pub version: u32,
    pub merkle_root: String,
    pub file_name: String,
    pub file_size: u64,
    pub tmp_path: String,
    pub dst_path: String,
    pub chunks: Vec<ChunkMeta>,
    pub verified_bytes: u64,
    pub peers: Vec<String>,
    pub manifest: Option<String>,
    pub encrypted: bool,
    pub updated_at: u64,
}

impl DlState {
    pub const VERSION: u32 = 1;

    pub fn new(
        merkle_root: String,
        file_name: String,
        file_size: u64,
        tmp_path: String,
        dst_path: String,
    ) -> Self {
        Self {
            version: Self::VERSION,
            merkle_root,
            file_name,
            file_size,
            tmp_path,
            dst_path,
            chunks: Vec::new(),
            verified_bytes: 0,
            peers: Vec::new(),
            manifest: None,
            encrypted: false,
            updated_at: now_unix(),
        }
    }

    pub fn init_chunks(&mut self, hashes: Option<&[String]>) {
        let cnt = ((self.file_size + CHUNK_SIZE - 1) / CHUNK_SIZE) as u32;
        self.chunks.clear();

        for i in 0..cnt {
            let offset = i as u64 * CHUNK_SIZE;
            let rem = self.file_size.saturating_sub(offset);
            let size = rem.min(CHUNK_SIZE) as u32;
            let hash = hashes.and_then(|h| h.get(i as usize).cloned());
            self.chunks.push(ChunkMeta::new(i, offset, size, hash));
        }
    }

    pub fn mark_downloaded(&mut self, idx: u32) {
        if let Some(c) = self.chunks.get_mut(idx as usize) {
            if c.state == ChunkState::Pending {
                c.state = ChunkState::Downloaded;
                self.updated_at = now_unix();
            }
        }
    }

    pub fn mark_verified(&mut self, idx: u32) {
        if let Some(c) = self.chunks.get_mut(idx as usize) {
            if c.state != ChunkState::Verified {
                c.state = ChunkState::Verified;
                self.verified_bytes += c.size as u64;
                self.updated_at = now_unix();
            }
        }
    }

    pub fn mark_failed(&mut self, idx: u32) {
        if let Some(c) = self.chunks.get_mut(idx as usize) {
            if c.state == ChunkState::Verified {
                self.verified_bytes = self.verified_bytes.saturating_sub(c.size as u64);
            }
            c.state = ChunkState::Failed;
            self.updated_at = now_unix();
        }
    }

    pub fn pending_chunks(&self) -> Vec<u32> {
        self.chunks
            .iter()
            .filter(|c| c.state == ChunkState::Pending || c.state == ChunkState::Failed)
            .map(|c| c.idx)
            .collect()
    }

    pub fn unverified_chunks(&self) -> Vec<u32> {
        self.chunks
            .iter()
            .filter(|c| c.state == ChunkState::Downloaded)
            .map(|c| c.idx)
            .collect()
    }

    pub fn is_complete(&self) -> bool {
        !self.chunks.is_empty() && self.chunks.iter().all(|c| c.state == ChunkState::Verified)
    }

    pub fn progress(&self) -> f32 {
        if self.file_size == 0 {
            return 1.0;
        }
        self.verified_bytes as f32 / self.file_size as f32
    }

    pub fn add_peer(&mut self, peer: String) {
        if !self.peers.contains(&peer) {
            self.peers.push(peer);
            self.updated_at = now_unix();
        }
    }
}

pub fn meta_path(tmp: &Path) -> PathBuf {
    let name = tmp.file_name().and_then(|n| n.to_str()).unwrap_or("dl");
    tmp.with_file_name(format!(".{}.chiral.p2p.meta.json", name))
}

// atomic write: tmp file then rename
pub async fn persist(state: &DlState) -> Result<(), String> {
    let tmp = PathBuf::from(&state.tmp_path);
    let path = meta_path(&tmp);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("mkdir failed: {}", e))?;
    }

    let data = serde_json::to_vec_pretty(state)
        .map_err(|e| format!("serialize failed: {}", e))?;

    let tmp_meta = path.with_extension("tmp");
    fs::write(&tmp_meta, &data)
        .await
        .map_err(|e| format!("write tmp failed: {}", e))?;

    fs::rename(&tmp_meta, &path)
        .await
        .map_err(|e| format!("rename failed: {}", e))?;

    debug!(
        "persisted {} ({} chunks, {:.1}%)",
        state.merkle_root,
        state.chunks.len(),
        state.progress() * 100.0
    );

    Ok(())
}

pub async fn load(path: &Path) -> Result<DlState, String> {
    let data = fs::read(path)
        .await
        .map_err(|e| format!("read failed: {}", e))?;

    let state: DlState =
        serde_json::from_slice(&data).map_err(|e| format!("parse failed: {}", e))?;

    if state.version > DlState::VERSION {
        return Err(format!("version {} unsupported", state.version));
    }

    Ok(state)
}

pub async fn remove_meta(tmp: &Path) {
    let path = meta_path(tmp);
    if let Err(e) = fs::remove_file(&path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!("rm meta {} failed: {}", path.display(), e);
        }
    }
}

pub async fn scan_incomplete(dir: &Path) -> Vec<DlState> {
    let mut res = Vec::new();

    let mut entries = match fs::read_dir(dir).await {
        Ok(e) => e,
        Err(e) => {
            warn!("readdir {} failed: {}", dir.display(), e);
            return res;
        }
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if name.ends_with(".chiral.p2p.meta.json") {
            match load(&path).await {
                Ok(state) => {
                    if !state.is_complete() {
                        info!("found incomplete: {} ({:.1}%)", state.merkle_root, state.progress() * 100.0);
                        res.push(state);
                    } else {
                        debug!("removing stale meta for {}", state.merkle_root);
                        let _ = fs::remove_file(&path).await;
                    }
                }
                Err(e) => warn!("load {} failed: {}", path.display(), e),
            }
        }
    }

    res
}

pub async fn verify_chunk(file: &Path, chunk: &ChunkMeta) -> Result<bool, String> {
    let expected = match &chunk.hash {
        Some(h) => h,
        None => return Ok(true), // no hash to verify
    };

    let mut f = tokio::fs::File::open(file)
        .await
        .map_err(|e| format!("open failed: {}", e))?;

    f.seek(std::io::SeekFrom::Start(chunk.offset))
        .await
        .map_err(|e| format!("seek chunk {} failed: {}", chunk.idx, e))?;

    let mut buf = vec![0u8; chunk.size as usize];
    if f.read_exact(&mut buf).await.is_err() {
        return Ok(false); // eof or short read
    }

    let mut hasher = Sha256::new();
    hasher.update(&buf);
    let actual = hex::encode(hasher.finalize());

    let ok = actual.eq_ignore_ascii_case(expected);
    if !ok {
        debug!("chunk {} mismatch: {} vs {}", chunk.idx, expected, actual);
    }

    Ok(ok)
}

pub async fn verify_all(state: &mut DlState) -> Result<VerifyResult, String> {
    let tmp = PathBuf::from(&state.tmp_path);

    if !tmp.exists() {
        return Ok(VerifyResult {
            total: state.chunks.len(),
            verified: 0,
            failed: 0,
            skipped: state.chunks.len(),
        });
    }

    let mut res = VerifyResult::default();
    res.total = state.chunks.len();

    let to_verify: Vec<_> = state
        .chunks
        .iter()
        .filter(|c| c.state == ChunkState::Downloaded || c.state == ChunkState::Verified)
        .cloned()
        .collect();

    for chunk in to_verify {
        if chunk.hash.is_none() {
            res.skipped += 1;
            continue;
        }

        match verify_chunk(&tmp, &chunk).await {
            Ok(true) => {
                state.mark_verified(chunk.idx);
                res.verified += 1;
            }
            Ok(false) => {
                state.mark_failed(chunk.idx);
                res.failed += 1;
            }
            Err(e) => {
                warn!("verify chunk {} error: {}", chunk.idx, e);
                state.mark_failed(chunk.idx);
                res.failed += 1;
            }
        }
    }

    Ok(res)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerifyResult {
    pub total: usize,
    pub verified: usize,
    pub failed: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoverInfo {
    pub merkle_root: String,
    pub file_name: String,
    pub file_size: u64,
    pub progress: f32,
    pub verified_bytes: u64,
    pub pending_cnt: usize,
    pub peers: Vec<String>,
    pub tmp_path: String,
    pub dst_path: String,
}

impl From<&DlState> for RecoverInfo {
    fn from(s: &DlState) -> Self {
        Self {
            merkle_root: s.merkle_root.clone(),
            file_name: s.file_name.clone(),
            file_size: s.file_size,
            progress: s.progress(),
            verified_bytes: s.verified_bytes,
            pending_cnt: s.pending_chunks().len(),
            peers: s.peers.clone(),
            tmp_path: s.tmp_path.clone(),
            dst_path: s.dst_path.clone(),
        }
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// =========================================================================
// dht integration
// =========================================================================

use crate::dht::models::FileMetadata;
use crate::manager::FileManifest;

pub fn from_metadata(
    meta: &FileMetadata,
    tmp_path: String,
    dst_path: String,
) -> DlState {
    let mut state = DlState::new(
        meta.merkle_root.clone(),
        meta.file_name.clone(),
        meta.file_size,
        tmp_path,
        dst_path,
    );

    let hashes = meta.manifest.as_ref().and_then(|j| parse_hashes(j));
    state.init_chunks(hashes.as_deref());

    for seeder in &meta.seeders {
        state.add_peer(seeder.clone());
    }

    state.manifest = meta.manifest.clone();
    state.encrypted = meta.is_encrypted;
    state
}

pub fn parse_hashes(json: &str) -> Option<Vec<String>> {
    let manifest: FileManifest = serde_json::from_str(json).ok()?;
    Some(manifest.chunks.iter().map(|c| c.hash.clone()).collect())
}

pub fn sync_received(state: &mut DlState, received: &HashSet<u32>) {
    for &idx in received {
        if let Some(c) = state.chunks.get_mut(idx as usize) {
            if c.state == ChunkState::Pending {
                c.state = ChunkState::Downloaded;
            }
        }
    }
    state.updated_at = now_unix();
}

pub fn add_providers(state: &mut DlState, providers: &[String]) {
    for p in providers {
        state.add_peer(p.clone());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPlan {
    pub state: DlState,
    pub to_fetch: Vec<u32>,
    pub verified: bool,
    pub verify_res: Option<VerifyResult>,
}

pub async fn plan_recovery(mut state: DlState, verify: bool) -> Result<RecoveryPlan, String> {
    let verify_res = if verify {
        let res = verify_all(&mut state).await?;
        info!(
            "verified {}: {}/{} ok, {} failed",
            state.merkle_root, res.verified, res.total, res.failed
        );
        Some(res)
    } else {
        None
    };

    let to_fetch = state.pending_chunks();
    info!(
        "recovery plan for {}: {} to fetch, {:.1}% done",
        state.merkle_root,
        to_fetch.len(),
        state.progress() * 100.0
    );

    Ok(RecoveryPlan {
        state,
        to_fetch,
        verified: verify,
        verify_res,
    })
}

pub async fn finalize(state: &DlState) -> Result<(), String> {
    let tmp = PathBuf::from(&state.tmp_path);
    let dst = PathBuf::from(&state.dst_path);

    if !tmp.exists() {
        return Err(format!("tmp file missing: {}", tmp.display()));
    }

    fs::rename(&tmp, &dst)
        .await
        .map_err(|e| format!("rename failed: {}", e))?;

    remove_meta(&tmp).await;

    info!("finalized {}: {} -> {}", state.merkle_root, tmp.display(), dst.display());
    Ok(())
}

pub fn to_file_meta(state: &DlState) -> FileMetadata {
    FileMetadata {
        merkle_root: state.merkle_root.clone(),
        file_name: state.file_name.clone(),
        file_size: state.file_size,
        seeders: state.peers.clone(),
        is_encrypted: state.encrypted,
        manifest: state.manifest.clone(),
        download_path: Some(state.dst_path.clone()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_state_default() {
        let s: ChunkState = Default::default();
        assert_eq!(s, ChunkState::Pending);
    }

    #[test]
    fn test_init_chunks() {
        let mut state = DlState::new(
            "abc".into(),
            "test.bin".into(),
            1024 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );

        state.init_chunks(None);

        // 1mb / 256kb = 4 chunks
        assert_eq!(state.chunks.len(), 4);
        assert_eq!(state.chunks[0].offset, 0);
        assert_eq!(state.chunks[0].size, 256 * 1024);
        assert_eq!(state.chunks[3].offset, 3 * 256 * 1024);
    }

    #[test]
    fn test_progress() {
        let mut state = DlState::new(
            "abc".into(),
            "test.bin".into(),
            1024 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );

        state.init_chunks(None);
        assert_eq!(state.progress(), 0.0);

        state.mark_downloaded(0);
        state.mark_verified(0);

        assert!((state.progress() - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_pending_chunks() {
        let mut state = DlState::new(
            "abc".into(),
            "test.bin".into(),
            512 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );

        state.init_chunks(None);
        assert_eq!(state.pending_chunks().len(), 2);

        state.mark_downloaded(0);
        state.mark_verified(0);

        let pending = state.pending_chunks();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0], 1);
    }

    #[test]
    fn test_is_complete() {
        let mut state = DlState::new(
            "abc".into(),
            "test.bin".into(),
            256 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );

        state.init_chunks(None);
        assert!(!state.is_complete());

        state.mark_downloaded(0);
        assert!(!state.is_complete());

        state.mark_verified(0);
        assert!(state.is_complete());
    }

    #[test]
    fn test_add_peer() {
        let mut state = DlState::new(
            "abc".into(),
            "test.bin".into(),
            1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );

        state.add_peer("peer1".into());
        state.add_peer("peer2".into());
        state.add_peer("peer1".into()); // dup

        assert_eq!(state.peers.len(), 2);
    }

    #[test]
    fn test_meta_path() {
        let tmp = PathBuf::from("/dl/test.tmp");
        let path = meta_path(&tmp);
        assert_eq!(path.to_str().unwrap(), "/dl/.test.tmp.chiral.p2p.meta.json");
    }

    #[test]
    fn test_mark_failed_reduces_verified() {
        let mut state = DlState::new(
            "abc".into(),
            "test.bin".into(),
            512 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );

        state.init_chunks(None);
        state.mark_downloaded(0);
        state.mark_verified(0);

        let before = state.verified_bytes;
        state.mark_failed(0);

        assert!(state.verified_bytes < before);
        assert_eq!(state.chunks[0].state, ChunkState::Failed);
    }
}
