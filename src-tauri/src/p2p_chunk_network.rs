// p2p chunk network - real network integration for chunk recovery
// integrates with manager.rs merkle, dht.rs providers, multi_source_download.rs

use crate::dht::models::FileMetadata;
use crate::dht::DhtService;
use crate::manager::{FileManifest, Sha256Hasher};
use rs_merkle::MerkleTree;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

// chunk size matches manager.rs and ipfs
pub const CHUNK_SIZE: u64 = 256 * 1024;

// state file extension
const META_EXT: &str = ".chiral.chunk.meta.json";

// =========================================================================
// errors
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChunkNetErr {
    NotFound,
    IoErr(String),
    HashMismatch { idx: u32, expected: String, got: String },
    MerkleInvalid,
    NoProviders,
    ManifestMissing,
    ParseErr(String),
}

impl std::fmt::Display for ChunkNetErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::IoErr(e) => write!(f, "io error: {}", e),
            Self::HashMismatch { idx, expected, got } => {
                write!(f, "chunk {} hash mismatch: {} != {}", idx, expected, got)
            }
            Self::MerkleInvalid => write!(f, "merkle root invalid"),
            Self::NoProviders => write!(f, "no providers found"),
            Self::ManifestMissing => write!(f, "manifest missing"),
            Self::ParseErr(e) => write!(f, "parse error: {}", e),
        }
    }
}

// =========================================================================
// chunk state
// =========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChunkState {
    Pending,
    Downloaded,
    Verified,
    Failed,
}

impl Default for ChunkState {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMeta {
    pub idx: u32,
    pub offset: u64,
    pub size: u32,
    pub hash: String,
    pub state: ChunkState,
}

impl ChunkMeta {
    pub fn new(idx: u32, offset: u64, size: u32, hash: String) -> Self {
        Self {
            idx,
            offset,
            size,
            hash,
            state: ChunkState::Pending,
        }
    }

    // calc offset for chunk idx
    pub fn offset_for(idx: u32) -> u64 {
        idx as u64 * CHUNK_SIZE
    }

    // calc size for chunk at idx given file size
    pub fn size_for(idx: u32, file_size: u64) -> u32 {
        let start = Self::offset_for(idx);
        let remaining = file_size.saturating_sub(start);
        remaining.min(CHUNK_SIZE) as u32
    }
}

// =========================================================================
// download state
// =========================================================================

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
    pub providers: Vec<String>,
    pub manifest_json: Option<String>,
    pub encrypted: bool,
    pub updated_at: u64,
}

impl DlState {
    pub fn new(
        merkle_root: String,
        file_name: String,
        file_size: u64,
        tmp_path: String,
        dst_path: String,
    ) -> Self {
        Self {
            version: 1,
            merkle_root,
            file_name,
            file_size,
            tmp_path,
            dst_path,
            chunks: Vec::new(),
            verified_bytes: 0,
            providers: Vec::new(),
            manifest_json: None,
            encrypted: false,
            updated_at: now_unix(),
        }
    }

    // init chunks from manifest
    pub fn init_from_manifest(&mut self, manifest: &FileManifest) {
        self.chunks.clear();
        for chunk in &manifest.chunks {
            self.chunks.push(ChunkMeta::new(
                chunk.index,
                ChunkMeta::offset_for(chunk.index),
                chunk.size as u32,
                chunk.hash.clone(),
            ));
        }
        self.manifest_json = serde_json::to_string(manifest).ok();
    }

    // init chunks from file size (no hashes yet)
    pub fn init_chunks(&mut self) {
        self.chunks.clear();
        if self.file_size == 0 {
            return;
        }
        let cnt = ((self.file_size + CHUNK_SIZE - 1) / CHUNK_SIZE) as u32;
        for idx in 0..cnt {
            self.chunks.push(ChunkMeta::new(
                idx,
                ChunkMeta::offset_for(idx),
                ChunkMeta::size_for(idx, self.file_size),
                String::new(),
            ));
        }
    }

    pub fn mark_downloaded(&mut self, idx: u32) {
        if let Some(c) = self.chunks.get_mut(idx as usize) {
            c.state = ChunkState::Downloaded;
            self.updated_at = now_unix();
        }
    }

    pub fn mark_verified(&mut self, idx: u32) {
        if let Some(c) = self.chunks.get_mut(idx as usize) {
            if c.state == ChunkState::Downloaded {
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

    pub fn is_complete(&self) -> bool {
        self.chunks.is_empty() || self.chunks.iter().all(|c| c.state == ChunkState::Verified)
    }

    pub fn progress(&self) -> f32 {
        if self.file_size == 0 {
            return 1.0;
        }
        self.verified_bytes as f32 / self.file_size as f32
    }

    pub fn add_provider(&mut self, peer_id: String) {
        if !self.providers.contains(&peer_id) {
            self.providers.push(peer_id);
            self.updated_at = now_unix();
        }
    }
}

// =========================================================================
// merkle verification
// =========================================================================

// compute merkle root from chunk hashes (hex strings)
pub fn compute_merkle_root(chunk_hashes: &[String]) -> Result<String, ChunkNetErr> {
    if chunk_hashes.is_empty() {
        return Err(ChunkNetErr::ManifestMissing);
    }

    let leaves: Vec<[u8; 32]> = chunk_hashes
        .iter()
        .map(|h| {
            hex::decode(h)
                .map_err(|e| ChunkNetErr::ParseErr(e.to_string()))?
                .try_into()
                .map_err(|_| ChunkNetErr::ParseErr("invalid hash len".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let tree = MerkleTree::<Sha256Hasher>::from_leaves(&leaves);
    let root = tree.root().ok_or(ChunkNetErr::MerkleInvalid)?;

    Ok(hex::encode(root))
}

// verify merkle root matches chunk hashes
pub fn verify_merkle_root(merkle_root: &str, chunk_hashes: &[String]) -> Result<bool, ChunkNetErr> {
    let computed = compute_merkle_root(chunk_hashes)?;
    Ok(computed == merkle_root)
}

// hash chunk data with sha256
pub fn hash_chunk(data: &[u8]) -> String {
    let mut hasher = Sha256::default();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

// verify chunk against expected hash
pub fn verify_chunk_hash(data: &[u8], expected: &str) -> bool {
    let computed = hash_chunk(data);
    computed == expected
}

// =========================================================================
// persistence
// =========================================================================

fn meta_path(tmp: &Path) -> PathBuf {
    let name = tmp
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download");
    tmp.with_file_name(format!("{}{}", name, META_EXT))
}

pub async fn persist(state: &DlState) -> Result<(), ChunkNetErr> {
    let tmp = PathBuf::from(&state.tmp_path);
    let path = meta_path(&tmp);
    let tmp_meta = path.with_extension("tmp");

    let json = serde_json::to_string_pretty(state)
        .map_err(|e| ChunkNetErr::ParseErr(e.to_string()))?;

    fs::write(&tmp_meta, json)
        .await
        .map_err(|e| ChunkNetErr::IoErr(e.to_string()))?;

    fs::rename(&tmp_meta, &path)
        .await
        .map_err(|e| ChunkNetErr::IoErr(e.to_string()))?;

    debug!("persisted state for {}", state.merkle_root);
    Ok(())
}

pub async fn load(tmp: &Path) -> Result<DlState, ChunkNetErr> {
    let path = meta_path(tmp);
    let json = fs::read_to_string(&path)
        .await
        .map_err(|e| ChunkNetErr::IoErr(e.to_string()))?;

    let state: DlState =
        serde_json::from_str(&json).map_err(|e| ChunkNetErr::ParseErr(e.to_string()))?;

    Ok(state)
}

pub async fn remove_meta(tmp: &Path) {
    let path = meta_path(tmp);
    let _ = fs::remove_file(&path).await;
}

// scan dir for incomplete downloads
pub async fn scan_incomplete(dir: &Path) -> Vec<DlState> {
    let mut states = Vec::new();

    let mut entries = match fs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return states,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json")
            && path.to_string_lossy().contains(META_EXT.trim_end_matches(".json"))
        {
            // find the tmp file this meta belongs to
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .trim_end_matches(META_EXT);
            let tmp = dir.join(name);

            if let Ok(state) = load(&tmp).await {
                if !state.is_complete() {
                    states.push(state);
                }
            }
        }
    }

    states
}

// =========================================================================
// chunk verification from disk
// =========================================================================

pub async fn verify_chunk_from_disk(
    file_path: &Path,
    chunk: &ChunkMeta,
) -> Result<bool, ChunkNetErr> {
    if chunk.hash.is_empty() {
        return Ok(true); // no hash to verify
    }

    let mut file = fs::File::open(file_path)
        .await
        .map_err(|e| ChunkNetErr::IoErr(e.to_string()))?;

    file.seek(SeekFrom::Start(chunk.offset))
        .await
        .map_err(|e| ChunkNetErr::IoErr(e.to_string()))?;

    let mut buf = vec![0u8; chunk.size as usize];
    let n = file
        .read_exact(&mut buf)
        .await
        .map_err(|e| ChunkNetErr::IoErr(e.to_string()))?;

    Ok(verify_chunk_hash(&buf[..n], &chunk.hash))
}

// verify all downloaded chunks, update state
pub async fn verify_all_chunks(state: &mut DlState) -> Result<VerifyResult, ChunkNetErr> {
    let tmp = PathBuf::from(&state.tmp_path);
    let mut verified = 0u32;
    let mut failed = 0u32;
    let mut failed_idxs = Vec::new();

    for chunk in &state.chunks {
        if chunk.state != ChunkState::Downloaded {
            continue;
        }

        match verify_chunk_from_disk(&tmp, chunk).await {
            Ok(true) => verified += 1,
            Ok(false) => {
                failed += 1;
                failed_idxs.push(chunk.idx);
            }
            Err(_) => {
                failed += 1;
                failed_idxs.push(chunk.idx);
            }
        }
    }

    // collect indices to update
    let to_verify: Vec<u32> = state
        .chunks
        .iter()
        .filter(|c| c.state == ChunkState::Downloaded && !failed_idxs.contains(&c.idx))
        .map(|c| c.idx)
        .collect();

    // mark failed chunks
    for idx in &failed_idxs {
        state.mark_failed(*idx);
    }

    // mark verified chunks
    for idx in to_verify {
        state.mark_verified(idx);
    }

    Ok(VerifyResult { verified, failed })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResult {
    pub verified: u32,
    pub failed: u32,
}

// =========================================================================
// dht integration
// =========================================================================

// query dht for providers of merkle_root
pub async fn query_providers(
    dht: &DhtService,
    merkle_root: &str,
) -> Result<Vec<String>, ChunkNetErr> {
    let providers = dht.get_seeders_for_file(merkle_root).await;
    if providers.is_empty() {
        return Err(ChunkNetErr::NoProviders);
    }
    Ok(providers)
}

// update state with providers from dht
pub async fn refresh_providers(
    state: &mut DlState,
    dht: &DhtService,
) -> Result<usize, ChunkNetErr> {
    let providers = query_providers(dht, &state.merkle_root).await?;
    let mut added = 0;

    for p in providers {
        if !state.providers.contains(&p) {
            state.providers.push(p);
            added += 1;
        }
    }

    if added > 0 {
        state.updated_at = now_unix();
    }

    Ok(added)
}

// announce as provider for merkle_root
pub async fn announce_provider(dht: &DhtService, merkle_root: &str) -> Result<(), ChunkNetErr> {
    // use announce_torrent which calls start_providing
    dht.announce_torrent(merkle_root.to_string())
        .await
        .map_err(|e| ChunkNetErr::IoErr(e))
}

// =========================================================================
// recovery service
// =========================================================================

pub struct RecoverySvc {
    dht: Arc<DhtService>,
    dl_dir: PathBuf,
    active: Arc<RwLock<HashSet<String>>>,
}

impl RecoverySvc {
    pub fn new(dht: Arc<DhtService>, dl_dir: PathBuf) -> Self {
        Self {
            dht,
            dl_dir,
            active: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    // scan and return incomplete downloads
    pub async fn scan(&self) -> Vec<DlState> {
        scan_incomplete(&self.dl_dir).await
    }

    // start recovery for a download
    pub async fn start_recovery(&self, mut state: DlState) -> Result<DlState, ChunkNetErr> {
        let mr = state.merkle_root.clone();

        // check not already active
        {
            let active = self.active.read().await;
            if active.contains(&mr) {
                info!("recovery already active for {}", mr);
                return Ok(state);
            }
        }

        // mark active
        {
            let mut active = self.active.write().await;
            active.insert(mr.clone());
        }

        // verify merkle root if we have manifest
        if let Some(ref manifest_json) = state.manifest_json {
            if let Ok(manifest) = serde_json::from_str::<FileManifest>(manifest_json) {
                let hashes: Vec<String> = manifest.chunks.iter().map(|c| c.hash.clone()).collect();
                if !verify_merkle_root(&state.merkle_root, &hashes)? {
                    error!("merkle root verification failed for {}", mr);
                    // remove from active
                    let mut active = self.active.write().await;
                    active.remove(&mr);
                    return Err(ChunkNetErr::MerkleInvalid);
                }
                info!("merkle root verified for {}", mr);
            }
        }

        // refresh providers from dht
        match refresh_providers(&mut state, &self.dht).await {
            Ok(n) => info!("added {} providers for {}", n, mr),
            Err(e) => warn!("failed to refresh providers: {}", e),
        }

        // verify existing chunks
        let result = verify_all_chunks(&mut state).await?;
        info!(
            "verified {} chunks, {} failed for {}",
            result.verified, result.failed, mr
        );

        // persist updated state
        persist(&state).await?;

        Ok(state)
    }

    // mark recovery complete
    pub async fn complete_recovery(&self, merkle_root: &str) {
        let mut active = self.active.write().await;
        active.remove(merkle_root);
        info!("recovery complete for {}", merkle_root);
    }

    // check if recovery active
    pub async fn is_active(&self, merkle_root: &str) -> bool {
        let active = self.active.read().await;
        active.contains(merkle_root)
    }
}

// =========================================================================
// create state from metadata
// =========================================================================

pub fn from_file_metadata(
    meta: &FileMetadata,
    tmp_path: String,
    dst_path: String,
) -> Result<DlState, ChunkNetErr> {
    let mut state = DlState::new(
        meta.merkle_root.clone(),
        meta.file_name.clone(),
        meta.file_size,
        tmp_path,
        dst_path,
    );

    state.encrypted = meta.is_encrypted;

    // parse manifest if present
    if let Some(ref manifest_str) = meta.manifest {
        match serde_json::from_str::<FileManifest>(manifest_str) {
            Ok(manifest) => {
                state.init_from_manifest(&manifest);
                state.manifest_json = Some(manifest_str.clone());
            }
            Err(e) => {
                warn!("failed to parse manifest: {}", e);
                state.init_chunks();
            }
        }
    } else {
        state.init_chunks();
    }

    // add existing seeders
    for seeder in &meta.seeders {
        state.add_provider(seeder.clone());
    }

    Ok(state)
}

// =========================================================================
// helpers
// =========================================================================

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// =========================================================================
// tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_chunk() {
        let data = b"hello world";
        let hash = hash_chunk(data);
        assert_eq!(hash.len(), 64); // sha256 hex is 64 chars
    }

    #[test]
    fn test_verify_chunk_hash() {
        let data = b"test data";
        let hash = hash_chunk(data);
        assert!(verify_chunk_hash(data, &hash));
        assert!(!verify_chunk_hash(b"other data", &hash));
    }

    #[test]
    fn test_compute_merkle_root() {
        let h1 = hash_chunk(b"chunk 0");
        let h2 = hash_chunk(b"chunk 1");
        let h3 = hash_chunk(b"chunk 2");

        let root = compute_merkle_root(&[h1.clone(), h2.clone(), h3.clone()]).unwrap();
        assert_eq!(root.len(), 64);

        // verify it matches
        assert!(verify_merkle_root(&root, &[h1, h2, h3]).unwrap());
    }

    #[test]
    fn test_merkle_root_mismatch() {
        let h1 = hash_chunk(b"chunk 0");
        let h2 = hash_chunk(b"chunk 1");

        let root = compute_merkle_root(&[h1.clone(), h2.clone()]).unwrap();

        // wrong hash
        let h3 = hash_chunk(b"wrong");
        assert!(!verify_merkle_root(&root, &[h1, h3]).unwrap());
    }

    #[test]
    fn test_dl_state_init() {
        let mut state = DlState::new(
            "abc".into(),
            "test.bin".into(),
            1024 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );

        state.init_chunks();
        assert_eq!(state.chunks.len(), 4); // 1mb / 256kb = 4
    }

    #[test]
    fn test_dl_state_progress() {
        let mut state = DlState::new(
            "abc".into(),
            "test.bin".into(),
            512 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );

        state.init_chunks();
        assert_eq!(state.progress(), 0.0);

        state.mark_downloaded(0);
        state.mark_verified(0);
        assert!(state.progress() > 0.4 && state.progress() < 0.6);

        state.mark_downloaded(1);
        state.mark_verified(1);
        assert_eq!(state.progress(), 1.0);
    }

    #[test]
    fn test_chunk_meta_offset() {
        assert_eq!(ChunkMeta::offset_for(0), 0);
        assert_eq!(ChunkMeta::offset_for(1), CHUNK_SIZE);
        assert_eq!(ChunkMeta::offset_for(4), CHUNK_SIZE * 4);
    }

    #[test]
    fn test_chunk_meta_size() {
        let file_size = CHUNK_SIZE * 2 + 100;
        assert_eq!(ChunkMeta::size_for(0, file_size), CHUNK_SIZE as u32);
        assert_eq!(ChunkMeta::size_for(1, file_size), CHUNK_SIZE as u32);
        assert_eq!(ChunkMeta::size_for(2, file_size), 100);
    }

    #[test]
    fn test_providers() {
        let mut state = DlState::new(
            "abc".into(),
            "test.bin".into(),
            1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );

        state.add_provider("peer1".into());
        state.add_provider("peer2".into());
        state.add_provider("peer1".into()); // dup

        assert_eq!(state.providers.len(), 2);
    }
}
