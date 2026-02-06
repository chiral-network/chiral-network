// p2p chunk network - real network integration for chunk recovery
// integrates with manager.rs merkle, dht.rs providers, multi_source_download.rs

use crate::dht::models::FileMetadata;
use crate::dht::DhtService;
use crate::manager::{FileManifest, Sha256Hasher};
use rs_merkle::MerkleTree;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use ed25519_dalek::Verifier;

// chunk size matches manager.rs and ipfs
pub const CHUNK_SIZE: u64 = 256 * 1024;

// maximum manifest JSON size we accept for signature verification (protect against DoS)
const MAX_MANIFEST_BYTES: usize = 16 * 1024; // 16 KiB

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
    // read_exact returns () on success; just ensure it completes
    file.read_exact(&mut buf)
        .await
        .map_err(|e| ChunkNetErr::IoErr(e.to_string()))?;

    Ok(verify_chunk_hash(&buf, &chunk.hash))
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
// download integration hooks
// =========================================================================

// hook for multi_source_download - call when download starts
pub async fn on_download_start(
    merkle_root: &str,
    file_name: &str,
    file_size: u64,
    tmp_path: &str,
    dst_path: &str,
    manifest: Option<&FileManifest>,
) -> Result<DlState, ChunkNetErr> {
    let mut state = DlState::new(
        merkle_root.to_string(),
        file_name.to_string(),
        file_size,
        tmp_path.to_string(),
        dst_path.to_string(),
    );

    if let Some(m) = manifest {
        state.init_from_manifest(m);
    } else {
        state.init_chunks();
    }

    persist(&state).await?;
    info!("download started: {} -> {}", merkle_root, file_name);

    Ok(state)
}

// hook for multi_source_download - call when chunk completes
pub async fn on_chunk_complete(
    tmp_path: &str,
    chunk_idx: u32,
    chunk_hash: &str,
) -> Result<(), ChunkNetErr> {
    let tmp = PathBuf::from(tmp_path);
    let mut state = load(&tmp).await?;

    // update chunk state
    if let Some(chunk) = state.chunks.get_mut(chunk_idx as usize) {
        chunk.state = ChunkState::Downloaded;
        if !chunk_hash.is_empty() {
            chunk.hash = chunk_hash.to_string();
        }
    }

    state.updated_at = now_unix();
    persist(&state).await?;

    debug!("chunk {} complete for {}", chunk_idx, state.merkle_root);
    Ok(())
}

// hook for multi_source_download - call when chunk verified
pub async fn on_chunk_verified(tmp_path: &str, chunk_idx: u32) -> Result<(), ChunkNetErr> {
    let tmp = PathBuf::from(tmp_path);
    let mut state = load(&tmp).await?;

    state.mark_verified(chunk_idx);
    persist(&state).await?;

    debug!("chunk {} verified for {}", chunk_idx, state.merkle_root);
    Ok(())
}

// hook for multi_source_download - call when download completes
pub async fn on_download_complete(tmp_path: &str) -> Result<(), ChunkNetErr> {
    let tmp = PathBuf::from(tmp_path);
    let state = load(&tmp).await?;

    // verify merkle root
    if let Some(ref manifest_json) = state.manifest_json {
        if let Ok(manifest) = serde_json::from_str::<FileManifest>(manifest_json) {
            let hashes: Vec<String> = manifest.chunks.iter().map(|c| c.hash.clone()).collect();
            if !verify_merkle_root(&state.merkle_root, &hashes)? {
                warn!("merkle root mismatch for completed download: {}", state.merkle_root);
            } else {
                info!("merkle root verified for completed download: {}", state.merkle_root);
            }
        }
    }

    // remove metadata file on success
    remove_meta(&tmp).await;
    info!("download complete: {}", state.merkle_root);

    Ok(())
}

// get default chunks directory
pub fn default_chunks_dir() -> PathBuf {
    PathBuf::from("./chunks")
}

// hook for loading existing chunks on resume
pub async fn load_existing_chunks(tmp_path: &str, chunks_dir: &Path) -> Result<DlState, ChunkNetErr> {
    let tmp = PathBuf::from(tmp_path);
    let mut state = load(&tmp).await?;

    // scan chunks dir for existing chunks
    let file_dir = chunks_dir.join(&state.merkle_root);
    if file_dir.exists() {
        let mut entries = fs::read_dir(&file_dir)
            .await
            .map_err(|e| ChunkNetErr::IoErr(e.to_string()))?;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // parse chunk_{idx}.dat
            if name_str.starts_with("chunk_") && name_str.ends_with(".dat") {
                if let Ok(idx) = name_str
                    .trim_start_matches("chunk_")
                    .trim_end_matches(".dat")
                    .parse::<u32>()
                {
                    if let Some(chunk) = state.chunks.get_mut(idx as usize) {
                        if chunk.state == ChunkState::Pending {
                            chunk.state = ChunkState::Downloaded;
                        }
                    }
                }
            }
        }
    }

    persist(&state).await?;
    Ok(state)
}

// =========================================================================
// corruption detection
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorruptionReport {
    pub total_checked: u32,
    pub verified_ok: u32,
    pub corrupted: Vec<u32>,
    pub missing: Vec<u32>,
}

// detect corrupted chunks by reading from disk and verifying hash
pub async fn detect_corruption(state: &DlState, chunks_dir: &Path) -> Result<CorruptionReport, ChunkNetErr> {
    let file_dir = chunks_dir.join(&state.merkle_root);
    let mut report = CorruptionReport {
        total_checked: 0,
        verified_ok: 0,
        corrupted: Vec::new(),
        missing: Vec::new(),
    };

    for chunk in &state.chunks {
        if chunk.hash.is_empty() {
            continue; // no hash to verify
        }

        let chunk_path = file_dir.join(format!("chunk_{}.dat", chunk.idx));

        if !chunk_path.exists() {
            if chunk.state == ChunkState::Downloaded || chunk.state == ChunkState::Verified {
                report.missing.push(chunk.idx);
            }
            continue;
        }

        report.total_checked += 1;

        // read chunk and verify
        match fs::read(&chunk_path).await {
            Ok(data) => {
                if verify_chunk_hash(&data, &chunk.hash) {
                    report.verified_ok += 1;
                } else {
                    report.corrupted.push(chunk.idx);
                }
            }
            Err(_) => {
                report.corrupted.push(chunk.idx);
            }
        }
    }

    Ok(report)
}

// mark corrupted chunks as failed for re-download
pub async fn mark_corrupted_for_redownload(
    state: &mut DlState,
    corrupted: &[u32],
) -> u32 {
    let mut marked = 0u32;

    for &idx in corrupted {
        if let Some(chunk) = state.chunks.get_mut(idx as usize) {
            chunk.state = ChunkState::Failed;
            marked += 1;
        }
    }

    if marked > 0 {
        state.updated_at = now_unix();
    }

    marked
}

// full corruption check and fix
pub async fn check_and_fix_corruption(
    tmp_path: &str,
    chunks_dir: &Path,
) -> Result<CorruptionReport, ChunkNetErr> {
    let tmp = PathBuf::from(tmp_path);
    let mut state = load(&tmp).await?;

    let report = detect_corruption(&state, chunks_dir).await?;

    if !report.corrupted.is_empty() || !report.missing.is_empty() {
        // mark corrupted and missing for re-download
        let mut to_mark = report.corrupted.clone();
        to_mark.extend(&report.missing);

        mark_corrupted_for_redownload(&mut state, &to_mark).await;
        persist(&state).await?;

        info!(
            "marked {} corrupted + {} missing chunks for re-download",
            report.corrupted.len(),
            report.missing.len()
        );
    }

    Ok(report)
}

// =========================================================================
// tauri commands
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryInfo {
    pub merkle_root: String,
    pub file_name: String,
    pub file_size: u64,
    pub progress: f32,
    pub pending_chunks: usize,
    pub verified_chunks: usize,
    pub failed_chunks: usize,
    pub provider_count: usize,
    pub is_complete: bool,
}

impl From<&DlState> for RecoveryInfo {
    fn from(s: &DlState) -> Self {
        let verified = s.chunks.iter().filter(|c| c.state == ChunkState::Verified).count();
        let failed = s.chunks.iter().filter(|c| c.state == ChunkState::Failed).count();
        let pending = s.chunks.iter().filter(|c| c.state == ChunkState::Pending).count();

        Self {
            merkle_root: s.merkle_root.clone(),
            file_name: s.file_name.clone(),
            file_size: s.file_size,
            progress: s.progress(),
            pending_chunks: pending,
            verified_chunks: verified,
            failed_chunks: failed,
            provider_count: s.providers.len(),
            is_complete: s.is_complete(),
        }
    }
}

#[tauri::command]
pub async fn p2p_chunk_scan(dl_dir: String) -> Result<Vec<RecoveryInfo>, String> {
    let dir = PathBuf::from(dl_dir);
    let states = scan_incomplete(&dir).await;
    Ok(states.iter().map(RecoveryInfo::from).collect())
}

#[tauri::command]
pub async fn p2p_chunk_get_state(tmp_path: String) -> Result<Option<RecoveryInfo>, String> {
    let tmp = PathBuf::from(&tmp_path);

    match load(&tmp).await {
        Ok(state) => Ok(Some(RecoveryInfo::from(&state))),
        Err(ChunkNetErr::IoErr(_)) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn p2p_chunk_verify(tmp_path: String) -> Result<VerifyResult, String> {
    let tmp = PathBuf::from(&tmp_path);
    let mut state = load(&tmp).await.map_err(|e| e.to_string())?;

    let result = verify_all_chunks(&mut state).await.map_err(|e| e.to_string())?;
    persist(&state).await.map_err(|e| e.to_string())?;

    Ok(result)
}

#[tauri::command]
pub async fn p2p_chunk_remove(tmp_path: String) -> Result<(), String> {
    let tmp = PathBuf::from(&tmp_path);
    remove_meta(&tmp).await;
    Ok(())
}

#[tauri::command]
pub fn p2p_chunk_compute_merkle(chunk_hashes: Vec<String>) -> Result<String, String> {
    compute_merkle_root(&chunk_hashes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn p2p_chunk_verify_merkle(merkle_root: String, chunk_hashes: Vec<String>) -> Result<bool, String> {
    verify_merkle_root(&merkle_root, &chunk_hashes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn p2p_chunk_hash(data: Vec<u8>) -> String {
    hash_chunk(&data)
}

#[tauri::command]
pub async fn p2p_chunk_check_corruption(tmp_path: String) -> Result<CorruptionReport, String> {
    let chunks_dir = default_chunks_dir();
    check_and_fix_corruption(&tmp_path, &chunks_dir)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn p2p_chunk_startup_recovery(dl_dir: String) -> Result<Vec<RecoveryInfo>, String> {
    let dir = PathBuf::from(&dl_dir);
    let chunks_dir = default_chunks_dir();

    let mut recovered = Vec::new();

    // scan for incomplete downloads
    let states = scan_incomplete(&dir).await;

    for state in states {
        let tmp = PathBuf::from(&state.tmp_path);

        // try to load existing chunks from disk
        match load_existing_chunks(&state.tmp_path, &chunks_dir).await {
            Ok(updated_state) => {
                recovered.push(RecoveryInfo::from(&updated_state));
                info!("recovered state for {}: {} chunks",
                    updated_state.merkle_root,
                    updated_state.chunks.iter().filter(|c| c.state != ChunkState::Pending).count()
                );
            }
            Err(e) => {
                warn!("failed to recover {}: {}", state.merkle_root, e);
            }
        }
    }

    Ok(recovered)
}

// =========================================================================
// multi-peer coordinator
// =========================================================================

/// Assignment of a chunk to a peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkAssignment {
    pub chunk_idx: u32,
    pub peer_id: String,
    pub assigned_at: u64,
    pub attempts: u32,
}

/// Stats for a peer during download
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerStats {
    pub peer_id: String,
    pub chunks_completed: u32,
    pub chunks_failed: u32,
    pub bytes_downloaded: u64,
    pub avg_speed: f64,   // bytes per second
    pub last_active: u64,
    pub consecutive_fails: u32,
    pub is_banned: bool,
}

impl PeerStats {
    pub fn new(peer_id: String) -> Self {
        Self {
            peer_id,
            chunks_completed: 0,
            chunks_failed: 0,
            bytes_downloaded: 0,
            avg_speed: 0.0,
            last_active: now_unix(),
            consecutive_fails: 0,
            is_banned: false,
        }
    }

    pub fn record_success(&mut self, bytes: u64, duration_ms: u64) {
        self.chunks_completed += 1;
        self.bytes_downloaded += bytes;
        self.consecutive_fails = 0;
        self.last_active = now_unix();

        if duration_ms > 0 {
            let speed = (bytes as f64) * 1000.0 / (duration_ms as f64);
            // EMA for speed
            let alpha = 0.3;
            if self.avg_speed == 0.0 {
                self.avg_speed = speed;
            } else {
                self.avg_speed = self.avg_speed * (1.0 - alpha) + speed * alpha;
            }
        }
    }

    pub fn record_failure(&mut self) {
        self.chunks_failed += 1;
        self.consecutive_fails += 1;
        self.last_active = now_unix();

        // ban after 5 consecutive fails
        if self.consecutive_fails >= 5 {
            self.is_banned = true;
        }
    }

    /// Score for peer selection (higher is better)
    pub fn score(&self) -> f64 {
        if self.is_banned {
            return 0.0;
        }

        let total = self.chunks_completed + self.chunks_failed;
        let reliability = if total > 0 {
            self.chunks_completed as f64 / total as f64
        } else {
            0.5 // unknown
        };

        // normalize speed (assume 1MB/s is good)
        let speed_score = (self.avg_speed / (1024.0 * 1024.0)).min(1.0);

        // combine reliability and speed
        reliability * 0.6 + speed_score * 0.4
    }
}

/// Coordinator for multi-peer chunk downloads
pub struct MultiPeerCoordinator {
    pub state: DlState,
    pub peer_stats: HashMap<String, PeerStats>,
    pub assignments: HashMap<u32, ChunkAssignment>,
    pub max_concurrent_per_peer: usize,
    pub max_retries: u32,
    pub chunk_timeout_ms: u64,
}

impl MultiPeerCoordinator {
    pub fn new(state: DlState) -> Self {
        let mut peer_stats = HashMap::new();
        for peer in &state.providers {
            peer_stats.insert(peer.clone(), PeerStats::new(peer.clone()));
        }

        Self {
            state,
            peer_stats,
            assignments: HashMap::new(),
            max_concurrent_per_peer: 4,
            max_retries: 3,
            chunk_timeout_ms: 30_000,
        }
    }

    /// Add a peer to the coordinator
    pub fn add_peer(&mut self, peer_id: String) {
        if !self.peer_stats.contains_key(&peer_id) {
            self.peer_stats.insert(peer_id.clone(), PeerStats::new(peer_id.clone()));
            self.state.add_provider(peer_id);
        }
    }

    /// Remove a peer from the coordinator
    pub fn remove_peer(&mut self, peer_id: &str) {
        self.peer_stats.remove(peer_id);
        self.state.providers.retain(|p| p != peer_id);

        // reassign chunks from this peer
        let to_reassign: Vec<u32> = self.assignments
            .iter()
            .filter(|(_, a)| a.peer_id == peer_id)
            .map(|(idx, _)| *idx)
            .collect();

        for idx in to_reassign {
            self.assignments.remove(&idx);
        }
    }

    /// Get available peers (not banned, not at max concurrent)
    pub fn available_peers(&self) -> Vec<(String, f64)> {
        let mut peers: Vec<(String, f64)> = self.peer_stats
            .iter()
            .filter(|(peer_id, stats)| {
                if stats.is_banned {
                    return false;
                }

                // check concurrent assignments
                let current_assignments = self.assignments
                    .values()
                    .filter(|a| &a.peer_id == *peer_id)
                    .count();

                current_assignments < self.max_concurrent_per_peer
            })
            .map(|(peer_id, stats)| (peer_id.clone(), stats.score()))
            .collect();

        // sort by score descending
        peers.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        peers
    }

    /// Select best peer for a chunk
    pub fn select_peer_for_chunk(&self, chunk_idx: u32) -> Option<String> {
        // check if already assigned
        if self.assignments.contains_key(&chunk_idx) {
            return None;
        }

        let peers = self.available_peers();
        if peers.is_empty() {
            return None;
        }

        // weighted random selection
        let total_score: f64 = peers.iter().map(|(_, s)| s).sum();
        if total_score <= 0.0 {
            return Some(peers[0].0.clone());
        }

        let mut r = rand::random::<f64>() * total_score;
        for (peer_id, score) in &peers {
            if r <= *score {
                return Some(peer_id.clone());
            }
            r -= score;
        }

        Some(peers[0].0.clone())
    }

    /// Assign a chunk to a peer
    pub fn assign_chunk(&mut self, chunk_idx: u32, peer_id: String) -> bool {
        if self.assignments.contains_key(&chunk_idx) {
            return false;
        }

        self.assignments.insert(chunk_idx, ChunkAssignment {
            chunk_idx,
            peer_id,
            assigned_at: now_unix(),
            attempts: 0,
        });

        true
    }

    /// Complete a chunk assignment successfully
    pub fn complete_chunk(&mut self, chunk_idx: u32, bytes: u64, duration_ms: u64) {
        if let Some(assignment) = self.assignments.remove(&chunk_idx) {
            if let Some(stats) = self.peer_stats.get_mut(&assignment.peer_id) {
                stats.record_success(bytes, duration_ms);
            }
            self.state.mark_downloaded(chunk_idx);
        }
    }

    /// Fail a chunk assignment
    pub fn fail_chunk(&mut self, chunk_idx: u32) -> bool {
        if let Some(mut assignment) = self.assignments.remove(&chunk_idx) {
            if let Some(stats) = self.peer_stats.get_mut(&assignment.peer_id) {
                stats.record_failure();
            }

            assignment.attempts += 1;

            // retry if under max retries
            if assignment.attempts < self.max_retries {
                // select a different peer
                if let Some(new_peer) = self.select_peer_for_chunk(chunk_idx) {
                    self.assignments.insert(chunk_idx, ChunkAssignment {
                        chunk_idx,
                        peer_id: new_peer,
                        assigned_at: now_unix(),
                        attempts: assignment.attempts,
                    });
                    return true;
                }
            }

            // mark as failed in state
            self.state.mark_failed(chunk_idx);
            return false;
        }
        false
    }

    /// Check for timed out assignments
    pub fn check_timeouts(&mut self) -> Vec<u32> {
        let now = now_unix();
        let timeout_secs = self.chunk_timeout_ms / 1000;
        let mut timed_out = Vec::new();

        for (idx, assignment) in &self.assignments {
            if now.saturating_sub(assignment.assigned_at) > timeout_secs {
                timed_out.push(*idx);
            }
        }

        // handle timeouts as failures
        for idx in &timed_out {
            self.fail_chunk(*idx);
        }

        timed_out
    }

    /// Get pending chunks that need assignment
    pub fn pending_chunks(&self) -> Vec<u32> {
        self.state.chunks
            .iter()
            .filter(|c| {
                (c.state == ChunkState::Pending || c.state == ChunkState::Failed)
                    && !self.assignments.contains_key(&c.idx)
            })
            .map(|c| c.idx)
            .collect()
    }

    /// Assign chunks to available peers
    pub fn assign_pending(&mut self) -> Vec<ChunkAssignment> {
        let pending = self.pending_chunks();
        let mut assigned = Vec::new();

        for idx in pending {
            if let Some(peer_id) = self.select_peer_for_chunk(idx) {
                if self.assign_chunk(idx, peer_id.clone()) {
                    if let Some(a) = self.assignments.get(&idx) {
                        assigned.push(a.clone());
                    }
                }
            }
        }

        assigned
    }

    /// Get download progress info
    pub fn progress_info(&self) -> CoordinatorProgress {
        let total = self.state.chunks.len() as u32;
        let verified = self.state.chunks.iter().filter(|c| c.state == ChunkState::Verified).count() as u32;
        let downloaded = self.state.chunks.iter().filter(|c| c.state == ChunkState::Downloaded).count() as u32;
        let pending = self.state.chunks.iter().filter(|c| c.state == ChunkState::Pending).count() as u32;
        let failed = self.state.chunks.iter().filter(|c| c.state == ChunkState::Failed).count() as u32;
        let in_flight = self.assignments.len() as u32;

        let active_peers = self.peer_stats.iter()
            .filter(|(_, s)| !s.is_banned && s.chunks_completed > 0)
            .count() as u32;

        let total_speed: f64 = self.peer_stats.values()
            .filter(|s| !s.is_banned)
            .map(|s| s.avg_speed)
            .sum();

        CoordinatorProgress {
            total_chunks: total,
            verified_chunks: verified,
            downloaded_chunks: downloaded,
            pending_chunks: pending,
            failed_chunks: failed,
            in_flight_chunks: in_flight,
            active_peers,
            total_peers: self.peer_stats.len() as u32,
            avg_speed: total_speed,
            progress_pct: if total > 0 { verified as f64 / total as f64 * 100.0 } else { 100.0 },
        }
    }

    /// Check if download is complete
    pub fn is_complete(&self) -> bool {
        self.state.is_complete()
    }
}

/// Progress info from coordinator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorProgress {
    pub total_chunks: u32,
    pub verified_chunks: u32,
    pub downloaded_chunks: u32,
    pub pending_chunks: u32,
    pub failed_chunks: u32,
    pub in_flight_chunks: u32,
    pub active_peers: u32,
    pub total_peers: u32,
    pub avg_speed: f64,
    pub progress_pct: f64,
}

/// Request to fetch a chunk from a peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkFetchRequest {
    pub merkle_root: String,
    pub chunk_idx: u32,
    pub chunk_hash: String,
    pub chunk_size: u32,
    pub peer_id: String,
}

/// Result of a chunk fetch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkFetchResult {
    pub chunk_idx: u32,
    pub success: bool,
    pub bytes: u64,
    pub duration_ms: u64,
    pub error: Option<String>,
}

// tauri commands for multi-peer coordination

#[tauri::command]
pub async fn p2p_chunk_create_coordinator(tmp_path: String) -> Result<CoordinatorProgress, String> {
    let tmp = PathBuf::from(&tmp_path);
    let state = load(&tmp).await.map_err(|e| e.to_string())?;
    let coordinator = MultiPeerCoordinator::new(state);
    Ok(coordinator.progress_info())
}

#[tauri::command]
pub async fn p2p_chunk_assign_pending(tmp_path: String) -> Result<Vec<ChunkFetchRequest>, String> {
    let tmp = PathBuf::from(&tmp_path);
    let state = load(&tmp).await.map_err(|e| e.to_string())?;
    let mut coordinator = MultiPeerCoordinator::new(state.clone());

    let assignments = coordinator.assign_pending();

    // create fetch requests
    let requests: Vec<ChunkFetchRequest> = assignments
        .iter()
        .filter_map(|a| {
            state.chunks.get(a.chunk_idx as usize).map(|c| ChunkFetchRequest {
                merkle_root: state.merkle_root.clone(),
                chunk_idx: a.chunk_idx,
                chunk_hash: c.hash.clone(),
                chunk_size: c.size,
                peer_id: a.peer_id.clone(),
            })
        })
        .collect();

    Ok(requests)
}

#[tauri::command]
pub async fn p2p_chunk_report_result(
    tmp_path: String,
    chunk_idx: u32,
    success: bool,
    bytes: u64,
    duration_ms: u64,
) -> Result<(), String> {
    let tmp = PathBuf::from(&tmp_path);
    let mut state = load(&tmp).await.map_err(|e| e.to_string())?;

    if success {
        state.mark_downloaded(chunk_idx);
    } else {
        state.mark_failed(chunk_idx);
    }

    persist(&state).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn p2p_chunk_get_progress(tmp_path: String) -> Result<CoordinatorProgress, String> {
    let tmp = PathBuf::from(&tmp_path);
    let state = load(&tmp).await.map_err(|e| e.to_string())?;
    let coordinator = MultiPeerCoordinator::new(state);
    Ok(coordinator.progress_info())
}

#[tauri::command]
pub fn p2p_verify_manifest(manifest_json: String) -> Result<bool, String> {
    // Avoid processing overly large manifests
    if manifest_json.as_bytes().len() > MAX_MANIFEST_BYTES {
        return Err(format!("Manifest too large: {} bytes", manifest_json.as_bytes().len()));
    }

    // Parse the manifest JSON
    let v: serde_json::Value = serde_json::from_str(&manifest_json)
        .map_err(|e| format!("Failed to parse manifest JSON: {}", e))?;

    // Extract signature and owner_pubkey
    let signature_b64 = v.get("signature").and_then(|s| s.as_str()).ok_or("Manifest missing 'signature' field")?;
    let owner_pub_b64 = v.get("owner_pubkey").and_then(|s| s.as_str()).ok_or("Manifest missing 'owner_pubkey' field")?;

    // Remove signature and owner_pubkey to form payload
    let mut payload = v.clone();
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("signature");
        obj.remove("owner_pubkey");
    }

    // Canonicalize payload for deterministic verification
    let canonical = canonicalize_value(&payload);
    let payload_bytes = serde_json::to_vec(&canonical).map_err(|e| format!("Failed to canonicalize manifest payload: {}", e))?;

    // Decode base64 using modern API
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;

    let pk_bytes = STANDARD.decode(owner_pub_b64).map_err(|e| format!("Failed to decode owner_pubkey: {}", e))?;
    let sig_bytes = STANDARD.decode(signature_b64).map_err(|e| format!("Failed to decode signature: {}", e))?;

    if pk_bytes.len() != 32 {
        return Err("owner_pubkey must be 32 bytes".into());
    }
    if sig_bytes.len() != 64 {
        return Err("signature must be 64 bytes".into());
    }

    let pk_arr: [u8; 32] = pk_bytes.as_slice().try_into().map_err(|_| "owner_pubkey invalid" )?;
    let sig_arr: [u8; 64] = sig_bytes.as_slice().try_into().map_err(|_| "signature invalid" )?;

    let vk = ed25519_dalek::VerifyingKey::from_bytes(&pk_arr).map_err(|e| format!("Invalid public key: {}", e))?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);

    match vk.verify(&payload_bytes, &sig) {
        Ok(()) => Ok(true),
        Err(e) => {
            debug!("manifest signature verification failed: {}", e);
            Err(format!("Signature verification failed: {}", e))
        }
    }
}

// Canonicalize JSON by sorting object keys recursively (deterministic, lightweight)
fn canonicalize_value(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(map) => {
            let mut items: Vec<(&String, &serde_json::Value)> = map.iter().collect();
            items.sort_by(|a, b| a.0.cmp(b.0));
            let mut new_map = serde_json::Map::new();
            for (k, val) in items {
                new_map.insert(k.clone(), canonicalize_value(val));
            }
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(arr.iter().map(canonicalize_value).collect()),
        _ => v.clone(),
    }
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

    #[test]
    fn test_init_from_manifest() {
        use crate::manager::{ChunkInfo as MgrChunk, FileManifest};

        let manifest = FileManifest {
            merkle_root: "abc123".to_string(),
            chunks: vec![
                MgrChunk {
                    index: 0,
                    hash: "hash0".to_string(),
                    size: 1000,
                    encrypted_hash: "".to_string(),
                    encrypted_size: 0,
                },
                MgrChunk {
                    index: 1,
                    hash: "hash1".to_string(),
                    size: 500,
                    encrypted_hash: "".to_string(),
                    encrypted_size: 0,
                },
            ],
            encrypted_key_bundle: None,
        };

        let mut state = DlState::new(
            "abc123".into(),
            "test.bin".into(),
            1500,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );

        state.init_from_manifest(&manifest);

        assert_eq!(state.chunks.len(), 2);
        assert_eq!(state.chunks[0].hash, "hash0");
        assert_eq!(state.chunks[1].hash, "hash1");
        assert!(state.manifest_json.is_some());
    }

    #[test]
    fn test_recovery_info() {
        let mut state = DlState::new(
            "abc".into(),
            "test.bin".into(),
            512 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );

        state.init_chunks();
        state.mark_downloaded(0);
        state.mark_verified(0);
        state.add_provider("peer1".into());

        let info = RecoveryInfo::from(&state);
        assert_eq!(info.merkle_root, "abc");
        assert_eq!(info.verified_chunks, 1);
        assert_eq!(info.pending_chunks, 1);
        assert_eq!(info.provider_count, 1);
        assert!(info.progress > 0.4 && info.progress < 0.6);
    }

    // =========================================================================
    // Multi-peer coordinator tests
    // =========================================================================

    #[test]
    fn test_peer_stats_new() {
        let stats = PeerStats::new("peer1".into());
        assert_eq!(stats.peer_id, "peer1");
        assert_eq!(stats.chunks_completed, 0);
        assert_eq!(stats.chunks_failed, 0);
        assert!(!stats.is_banned);
    }

    #[test]
    fn test_peer_stats_record_success() {
        let mut stats = PeerStats::new("peer1".into());

        stats.record_success(256 * 1024, 1000); // 256KB in 1 second
        assert_eq!(stats.chunks_completed, 1);
        assert_eq!(stats.bytes_downloaded, 256 * 1024);
        assert!(stats.avg_speed > 200_000.0); // ~256KB/s

        stats.record_success(256 * 1024, 500); // faster
        assert_eq!(stats.chunks_completed, 2);
        assert!(stats.avg_speed > 300_000.0); // speed should increase
    }

    #[test]
    fn test_peer_stats_record_failure() {
        let mut stats = PeerStats::new("peer1".into());

        for _ in 0..4 {
            stats.record_failure();
        }
        assert_eq!(stats.chunks_failed, 4);
        assert_eq!(stats.consecutive_fails, 4);
        assert!(!stats.is_banned);

        stats.record_failure(); // 5th failure
        assert!(stats.is_banned);
    }

    #[test]
    fn test_peer_stats_score() {
        let mut stats = PeerStats::new("peer1".into());

        // unknown peer has 0.5 reliability
        let score = stats.score();
        assert!(score > 0.0);

        // after successes, score should increase
        stats.record_success(1024 * 1024, 1000);
        stats.record_success(1024 * 1024, 1000);
        let new_score = stats.score();
        assert!(new_score > score);

        // banned peer has 0 score
        stats.is_banned = true;
        assert_eq!(stats.score(), 0.0);
    }

    #[test]
    fn test_coordinator_new() {
        let mut state = DlState::new(
            "merkle123".into(),
            "test.bin".into(),
            1024 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();
        state.add_provider("peer1".into());
        state.add_provider("peer2".into());

        let coordinator = MultiPeerCoordinator::new(state);

        assert_eq!(coordinator.peer_stats.len(), 2);
        assert!(coordinator.peer_stats.contains_key("peer1"));
        assert!(coordinator.peer_stats.contains_key("peer2"));
    }

    #[test]
    fn test_coordinator_add_remove_peer() {
        let mut state = DlState::new(
            "merkle123".into(),
            "test.bin".into(),
            1024 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();

        let mut coordinator = MultiPeerCoordinator::new(state);
        assert_eq!(coordinator.peer_stats.len(), 0);

        coordinator.add_peer("peer1".into());
        assert_eq!(coordinator.peer_stats.len(), 1);

        coordinator.add_peer("peer2".into());
        assert_eq!(coordinator.peer_stats.len(), 2);

        coordinator.remove_peer("peer1");
        assert_eq!(coordinator.peer_stats.len(), 1);
        assert!(!coordinator.peer_stats.contains_key("peer1"));
    }

    #[test]
    fn test_coordinator_assign_chunk() {
        let mut state = DlState::new(
            "merkle123".into(),
            "test.bin".into(),
            512 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();
        state.add_provider("peer1".into());

        let mut coordinator = MultiPeerCoordinator::new(state);

        assert!(coordinator.assign_chunk(0, "peer1".into()));
        assert!(!coordinator.assign_chunk(0, "peer1".into())); // already assigned

        assert_eq!(coordinator.assignments.len(), 1);
        assert_eq!(coordinator.assignments.get(&0).unwrap().peer_id, "peer1");
    }

    #[test]
    fn test_coordinator_complete_chunk() {
        let mut state = DlState::new(
            "merkle123".into(),
            "test.bin".into(),
            512 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();
        state.add_provider("peer1".into());

        let mut coordinator = MultiPeerCoordinator::new(state);
        coordinator.assign_chunk(0, "peer1".into());

        coordinator.complete_chunk(0, 256 * 1024, 1000);

        assert!(coordinator.assignments.is_empty());
        assert_eq!(coordinator.state.chunks[0].state, ChunkState::Downloaded);
        assert_eq!(coordinator.peer_stats.get("peer1").unwrap().chunks_completed, 1);
    }

    #[test]
    fn test_coordinator_fail_chunk() {
        let mut state = DlState::new(
            "merkle123".into(),
            "test.bin".into(),
            512 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();
        state.add_provider("peer1".into());
        state.add_provider("peer2".into());

        let mut coordinator = MultiPeerCoordinator::new(state);
        coordinator.assign_chunk(0, "peer1".into());

        // fail should reassign to different peer
        let reassigned = coordinator.fail_chunk(0);
        assert!(reassigned); // should have reassigned

        assert_eq!(coordinator.peer_stats.get("peer1").unwrap().chunks_failed, 1);
    }

    #[test]
    fn test_coordinator_pending_chunks() {
        let mut state = DlState::new(
            "merkle123".into(),
            "test.bin".into(),
            512 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();
        state.add_provider("peer1".into());

        let mut coordinator = MultiPeerCoordinator::new(state);

        let pending = coordinator.pending_chunks();
        assert_eq!(pending.len(), 2); // 512KB = 2 chunks

        coordinator.assign_chunk(0, "peer1".into());

        let pending = coordinator.pending_chunks();
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn test_coordinator_progress_info() {
        let mut state = DlState::new(
            "merkle123".into(),
            "test.bin".into(),
            512 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();
        state.add_provider("peer1".into());

        let mut coordinator = MultiPeerCoordinator::new(state);

        let progress = coordinator.progress_info();
        assert_eq!(progress.total_chunks, 2);
        assert_eq!(progress.pending_chunks, 2);
        assert_eq!(progress.verified_chunks, 0);
        assert_eq!(progress.total_peers, 1);

        coordinator.assign_chunk(0, "peer1".into());

        let progress = coordinator.progress_info();
        assert_eq!(progress.in_flight_chunks, 1);
    }

    #[test]
    fn test_coordinator_available_peers() {
        let mut state = DlState::new(
            "merkle123".into(),
            "test.bin".into(),
            1024 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();
        state.add_provider("peer1".into());
        state.add_provider("peer2".into());

        let mut coordinator = MultiPeerCoordinator::new(state);

        // both peers should be available
        let available = coordinator.available_peers();
        assert_eq!(available.len(), 2);

        // ban peer1
        coordinator.peer_stats.get_mut("peer1").unwrap().is_banned = true;

        let available = coordinator.available_peers();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].0, "peer2");
    }

    // =========================================================================
    // Corruption detection tests
    // =========================================================================

    #[test]
    fn test_corruption_report_empty() {
        let report = CorruptionReport {
            total_checked: 0,
            verified_ok: 0,
            corrupted: Vec::new(),
            missing: Vec::new(),
        };

        assert!(report.corrupted.is_empty());
        assert!(report.missing.is_empty());
    }

    #[test]
    fn test_mark_corrupted_for_redownload() {
        let mut state = DlState::new(
            "merkle123".into(),
            "test.bin".into(),
            512 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();
        state.mark_downloaded(0);
        state.mark_downloaded(1);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let marked = rt.block_on(async {
            mark_corrupted_for_redownload(&mut state, &[0]).await
        });

        assert_eq!(marked, 1);
        assert_eq!(state.chunks[0].state, ChunkState::Failed);
        assert_eq!(state.chunks[1].state, ChunkState::Downloaded);
    }

    // =========================================================================
    // Error display tests
    // =========================================================================

    #[test]
    fn test_error_display() {
        assert_eq!(ChunkNetErr::NotFound.to_string(), "not found");
        assert_eq!(ChunkNetErr::IoErr("test".into()).to_string(), "io error: test");
        assert_eq!(
            ChunkNetErr::HashMismatch { idx: 5, expected: "a".into(), got: "b".into() }.to_string(),
            "chunk 5 hash mismatch: a != b"
        );
        assert_eq!(ChunkNetErr::MerkleInvalid.to_string(), "merkle root invalid");
        assert_eq!(ChunkNetErr::NoProviders.to_string(), "no providers found");
        assert_eq!(ChunkNetErr::ManifestMissing.to_string(), "manifest missing");
        assert_eq!(ChunkNetErr::ParseErr("json".into()).to_string(), "parse error: json");
    }

    // =========================================================================
    // Edge case tests
    // =========================================================================

    #[test]
    fn test_empty_file() {
        let mut state = DlState::new(
            "abc".into(),
            "empty.bin".into(),
            0,
            "/tmp/empty.tmp".into(),
            "/dl/empty.bin".into(),
        );

        state.init_chunks();
        assert!(state.chunks.is_empty());
        assert!(state.is_complete());
        assert_eq!(state.progress(), 1.0);
    }

    #[test]
    fn test_small_file() {
        let mut state = DlState::new(
            "abc".into(),
            "small.bin".into(),
            100, // less than one chunk
            "/tmp/small.tmp".into(),
            "/dl/small.bin".into(),
        );

        state.init_chunks();
        assert_eq!(state.chunks.len(), 1);
        assert_eq!(state.chunks[0].size, 100);
    }

    #[test]
    fn test_exact_chunk_boundary() {
        let mut state = DlState::new(
            "abc".into(),
            "exact.bin".into(),
            CHUNK_SIZE * 3, // exactly 3 chunks
            "/tmp/exact.tmp".into(),
            "/dl/exact.bin".into(),
        );

        state.init_chunks();
        assert_eq!(state.chunks.len(), 3);
        assert_eq!(state.chunks[0].size, CHUNK_SIZE as u32);
        assert_eq!(state.chunks[1].size, CHUNK_SIZE as u32);
        assert_eq!(state.chunks[2].size, CHUNK_SIZE as u32);
    }

    #[test]
    fn test_single_chunk_merkle() {
        let h = hash_chunk(b"single chunk data");
        let root = compute_merkle_root(&[h.clone()]).unwrap();
        assert!(verify_merkle_root(&root, &[h]).unwrap());
    }

    #[test]
    fn test_large_chunk_count_merkle() {
        let hashes: Vec<String> = (0..100)
            .map(|i| hash_chunk(format!("chunk {}", i).as_bytes()))
            .collect();

        let root = compute_merkle_root(&hashes).unwrap();
        assert_eq!(root.len(), 64);
        assert!(verify_merkle_root(&root, &hashes).unwrap());
    }

    #[test]
    fn test_mark_failed_verified_chunk() {
        let mut state = DlState::new(
            "abc".into(),
            "test.bin".into(),
            512 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();

        state.mark_downloaded(0);
        state.mark_verified(0);
        assert!(state.verified_bytes > 0);

        let bytes_before = state.verified_bytes;
        state.mark_failed(0);

        // verified_bytes should decrease
        assert!(state.verified_bytes < bytes_before);
        assert_eq!(state.chunks[0].state, ChunkState::Failed);
    }

    #[test]
    fn test_pending_chunks_includes_failed() {
        let mut state = DlState::new(
            "abc".into(),
            "test.bin".into(),
            512 * 1024,
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();

        state.mark_downloaded(0);
        state.mark_verified(0);
        state.mark_downloaded(1);
        state.mark_failed(1);

        let pending = state.pending_chunks();
        assert_eq!(pending.len(), 1);
        assert!(pending.contains(&1)); // failed should be in pending
    }

    #[test]
    fn test_coordinator_max_concurrent() {
        let mut state = DlState::new(
            "merkle123".into(),
            "test.bin".into(),
            CHUNK_SIZE * 10, // 10 chunks
            "/tmp/test.tmp".into(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();
        state.add_provider("peer1".into());

        let mut coordinator = MultiPeerCoordinator::new(state);
        coordinator.max_concurrent_per_peer = 2;

        // assign 2 chunks
        coordinator.assign_chunk(0, "peer1".into());
        coordinator.assign_chunk(1, "peer1".into());

        // peer1 should not be available now
        let available = coordinator.available_peers();
        assert!(available.is_empty());
    }

    // =========================================================================
    // Integration tests with file I/O
    // =========================================================================

    #[tokio::test]
    async fn test_persist_and_load() {
        let tmp_dir = std::env::temp_dir().join(format!("p2p_test_{}", rand::random::<u32>()));
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;

        let tmp_file = tmp_dir.join("test_download.tmp");
        let meta_file = tmp_dir.join("test_download.tmp.chiral.chunk.meta.json");

        // create a test file
        tokio::fs::write(&tmp_file, b"test content").await.unwrap();

        let mut state = DlState::new(
            "merkle_persist_test".into(),
            "test.bin".into(),
            512 * 1024,
            tmp_file.to_string_lossy().to_string(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();
        state.add_provider("peer1".into());
        state.mark_downloaded(0);

        // persist
        persist(&state).await.unwrap();

        // verify meta file exists
        assert!(meta_file.exists());

        // load
        let loaded = load(&tmp_file).await.unwrap();
        assert_eq!(loaded.merkle_root, "merkle_persist_test");
        assert_eq!(loaded.providers.len(), 1);
        assert_eq!(loaded.chunks[0].state, ChunkState::Downloaded);

        // cleanup
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    #[tokio::test]
    async fn test_verify_chunk_from_disk_valid() {
        let tmp_dir = std::env::temp_dir().join(format!("p2p_test_{}", rand::random::<u32>()));
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;

        let tmp_file = tmp_dir.join("test_verify.bin");

        // write test data
        let chunk_data = b"hello world test chunk data";
        tokio::fs::write(&tmp_file, chunk_data).await.unwrap();

        let chunk = ChunkMeta {
            idx: 0,
            offset: 0,
            size: chunk_data.len() as u32,
            hash: hash_chunk(chunk_data),
            state: ChunkState::Downloaded,
        };

        let result = verify_chunk_from_disk(&tmp_file, &chunk).await.unwrap();
        assert!(result);

        // cleanup
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    #[tokio::test]
    async fn test_verify_chunk_from_disk_corrupted() {
        let tmp_dir = std::env::temp_dir().join(format!("p2p_test_{}", rand::random::<u32>()));
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;

        let tmp_file = tmp_dir.join("test_corrupted.bin");

        // write corrupted data
        let original_data = b"original data";
        let corrupted_data = b"corrupted!!!";
        tokio::fs::write(&tmp_file, corrupted_data).await.unwrap();

        let chunk = ChunkMeta {
            idx: 0,
            offset: 0,
            size: corrupted_data.len() as u32,
            hash: hash_chunk(original_data), // hash of original, not corrupted
            state: ChunkState::Downloaded,
        };

        let result = verify_chunk_from_disk(&tmp_file, &chunk).await.unwrap();
        assert!(!result); // should fail verification

        // cleanup
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    #[tokio::test]
    async fn test_scan_incomplete_finds_downloads() {
        let tmp_dir = std::env::temp_dir().join(format!("p2p_scan_{}", rand::random::<u32>()));
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;

        // create an incomplete download
        let tmp_file = tmp_dir.join("incomplete.tmp");
        tokio::fs::write(&tmp_file, b"partial data").await.unwrap();

        let mut state = DlState::new(
            "merkle_scan_test".into(),
            "incomplete.bin".into(),
            1024 * 1024,
            tmp_file.to_string_lossy().to_string(),
            "/dl/incomplete.bin".into(),
        );
        state.init_chunks();
        state.mark_downloaded(0); // only first chunk done

        persist(&state).await.unwrap();

        // scan for incomplete
        let incomplete = scan_incomplete(&tmp_dir).await;
        assert_eq!(incomplete.len(), 1);
        assert_eq!(incomplete[0].merkle_root, "merkle_scan_test");
        assert!(!incomplete[0].is_complete());

        // cleanup
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    #[tokio::test]
    async fn test_scan_incomplete_ignores_complete() {
        let tmp_dir = std::env::temp_dir().join(format!("p2p_scan_c_{}", rand::random::<u32>()));
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;

        // create a complete download
        let tmp_file = tmp_dir.join("complete.tmp");
        tokio::fs::write(&tmp_file, b"complete data").await.unwrap();

        let mut state = DlState::new(
            "merkle_complete_test".into(),
            "complete.bin".into(),
            512 * 1024,
            tmp_file.to_string_lossy().to_string(),
            "/dl/complete.bin".into(),
        );
        state.init_chunks();
        // mark all chunks verified
        for i in 0..state.chunks.len() {
            state.mark_downloaded(i as u32);
            state.mark_verified(i as u32);
        }

        persist(&state).await.unwrap();

        // scan should not find complete downloads
        let incomplete = scan_incomplete(&tmp_dir).await;
        assert!(incomplete.is_empty());

        // cleanup
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    #[tokio::test]
    async fn test_detect_corruption_missing_chunks() {
        let tmp_dir = std::env::temp_dir().join(format!("p2p_corrupt_{}", rand::random::<u32>()));
        let chunks_dir = tmp_dir.join("chunks");
        let file_dir = chunks_dir.join("merkle_missing_test");
        let _ = tokio::fs::create_dir_all(&file_dir).await;

        let mut state = DlState::new(
            "merkle_missing_test".into(),
            "test.bin".into(),
            512 * 1024,
            tmp_dir.join("test.tmp").to_string_lossy().to_string(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();

        // set chunk hashes
        state.chunks[0].hash = hash_chunk(b"chunk 0 data");
        state.chunks[1].hash = hash_chunk(b"chunk 1 data");

        // mark as downloaded but don't create files
        state.mark_downloaded(0);
        state.mark_downloaded(1);

        // detect corruption
        let report = detect_corruption(&state, &chunks_dir).await.unwrap();

        // both chunks should be missing
        assert_eq!(report.missing.len(), 2);
        assert!(report.missing.contains(&0));
        assert!(report.missing.contains(&1));

        // cleanup
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    #[tokio::test]
    async fn test_detect_corruption_actual_corruption() {
        let tmp_dir = std::env::temp_dir().join(format!("p2p_corrupt2_{}", rand::random::<u32>()));
        let chunks_dir = tmp_dir.join("chunks");
        let file_dir = chunks_dir.join("merkle_corrupt_test");
        let _ = tokio::fs::create_dir_all(&file_dir).await;

        let mut state = DlState::new(
            "merkle_corrupt_test".into(),
            "test.bin".into(),
            512 * 1024,
            tmp_dir.join("test.tmp").to_string_lossy().to_string(),
            "/dl/test.bin".into(),
        );
        state.init_chunks();

        // set correct hashes
        let chunk0_data = b"correct chunk 0 data";
        let chunk1_data = b"correct chunk 1 data";
        state.chunks[0].hash = hash_chunk(chunk0_data);
        state.chunks[1].hash = hash_chunk(chunk1_data);
        state.mark_downloaded(0);
        state.mark_downloaded(1);

        // write correct chunk 0
        tokio::fs::write(file_dir.join("chunk_0.dat"), chunk0_data).await.unwrap();
        // write corrupted chunk 1
        tokio::fs::write(file_dir.join("chunk_1.dat"), b"CORRUPTED DATA!!!").await.unwrap();

        // detect corruption
        let report = detect_corruption(&state, &chunks_dir).await.unwrap();

        assert_eq!(report.total_checked, 2);
        assert_eq!(report.verified_ok, 1);
        assert_eq!(report.corrupted.len(), 1);
        assert!(report.corrupted.contains(&1));

        // cleanup
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    #[tokio::test]
    async fn test_full_download_flow() {
        let tmp_dir = std::env::temp_dir().join(format!("p2p_flow_{}", rand::random::<u32>()));
        let chunks_dir = tmp_dir.join("chunks");
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;

        let tmp_file = tmp_dir.join("test_download.tmp");
        tokio::fs::write(&tmp_file, b"").await.unwrap();

        // simulate download start
        let chunk0_data = b"chunk 0 content here";
        let chunk1_data = b"chunk 1 content here";
        let h0 = hash_chunk(chunk0_data);
        let h1 = hash_chunk(chunk1_data);

        use crate::manager::{ChunkInfo as MgrChunk, FileManifest};
        let merkle_root = compute_merkle_root(&[h0.clone(), h1.clone()]).unwrap();

        let manifest = FileManifest {
            merkle_root: merkle_root.clone(),
            chunks: vec![
                MgrChunk {
                    index: 0,
                    hash: h0.clone(),
                    size: chunk0_data.len(),
                    encrypted_hash: "".into(),
                    encrypted_size: 0,
                },
                MgrChunk {
                    index: 1,
                    hash: h1.clone(),
                    size: chunk1_data.len(),
                    encrypted_hash: "".into(),
                    encrypted_size: 0,
                },
            ],
            encrypted_key_bundle: None,
        };

        // call on_download_start
        let state = on_download_start(
            &merkle_root,
            "test.bin",
            (chunk0_data.len() + chunk1_data.len()) as u64,
            &tmp_file.to_string_lossy(),
            "/dl/test.bin",
            Some(&manifest),
        ).await.unwrap();

        assert_eq!(state.merkle_root, merkle_root);
        assert_eq!(state.chunks.len(), 2);
        assert_eq!(state.chunks[0].hash, h0);
        assert_eq!(state.chunks[1].hash, h1);

        // simulate chunk complete
        on_chunk_complete(&tmp_file.to_string_lossy(), 0, &h0).await.unwrap();
        on_chunk_complete(&tmp_file.to_string_lossy(), 1, &h1).await.unwrap();

        // simulate chunk verified
        on_chunk_verified(&tmp_file.to_string_lossy(), 0).await.unwrap();
        on_chunk_verified(&tmp_file.to_string_lossy(), 1).await.unwrap();

        // load and check state
        let final_state = load(&tmp_file).await.unwrap();
        assert!(final_state.is_complete());

        // call on_download_complete
        on_download_complete(&tmp_file.to_string_lossy()).await.unwrap();

        // meta file should be removed
        let meta_path = tmp_dir.join("test_download.tmp.chiral.chunk.meta.json");
        assert!(!meta_path.exists());

        // cleanup
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }

    #[tokio::test]
    async fn test_remove_meta() {
        let tmp_dir = std::env::temp_dir().join(format!("p2p_rm_{}", rand::random::<u32>()));
        let _ = tokio::fs::create_dir_all(&tmp_dir).await;

        let tmp_file = tmp_dir.join("to_remove.tmp");
        tokio::fs::write(&tmp_file, b"data").await.unwrap();

        let state = DlState::new(
            "merkle_rm".into(),
            "test.bin".into(),
            100,
            tmp_file.to_string_lossy().to_string(),
            "/dl/test.bin".into(),
        );

        persist(&state).await.unwrap();

        let meta_path = tmp_dir.join("to_remove.tmp.chiral.chunk.meta.json");
        assert!(meta_path.exists());

        remove_meta(&tmp_file).await;
        assert!(!meta_path.exists());

        // cleanup
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
    }
}
