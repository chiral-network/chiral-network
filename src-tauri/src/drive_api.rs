use axum::{
    extract::{Extension, Multipart, Path, Query},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::drive_storage::{
    self, collect_descendants, generate_id, generate_share_token, now_secs, DriveItem,
    DriveManifest, ShareLink,
};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct DriveState {
    pub manifest: Arc<RwLock<DriveManifest>>,
}

impl DriveState {
    pub fn new() -> Self {
        Self {
            manifest: Arc::new(RwLock::new(DriveManifest::default())),
        }
    }

    pub fn load_from_disk(&self) {
        let loaded = drive_storage::load_manifest();
        // We can't await here since this is sync, so use try_write
        if let Ok(mut m) = self.manifest.try_write() {
            *m = loaded;
        }
    }

    pub async fn load_from_disk_async(&self) {
        let loaded = drive_storage::load_manifest();
        let mut m = self.manifest.write().await;
        *m = loaded;
    }

    pub async fn persist(&self) {
        let m = self.manifest.read().await;
        drive_storage::save_manifest(&m);
    }
}

/// Extract the owner wallet address from X-Owner header.
fn get_owner(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-owner")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn is_valid_wallet(addr: &str) -> bool {
    addr.len() == 42 && addr.starts_with("0x") && addr[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_chi_to_wei(amount: &str) -> Result<u128, String> {
    let amount = amount.trim();
    let parts: Vec<&str> = amount.split('.').collect();
    if parts.len() > 2 {
        return Err("Invalid amount format".to_string());
    }

    let whole: u128 = if parts[0].is_empty() {
        0
    } else {
        parts[0].parse().map_err(|_| "Invalid amount".to_string())?
    };

    let frac_wei = if parts.len() == 2 {
        let frac_str = parts[1];
        if frac_str.len() > 18 {
            frac_str[..18]
                .parse::<u128>()
                .map_err(|_| "Invalid amount".to_string())?
        } else {
            let padded = format!("{:0<18}", frac_str);
            padded
                .parse::<u128>()
                .map_err(|_| "Invalid amount".to_string())?
        }
    } else {
        0u128
    };

    whole
        .checked_mul(1_000_000_000_000_000_000u128)
        .and_then(|w| w.checked_add(frac_wei))
        .ok_or("Amount overflow".to_string())
}

fn parse_hex_u128(hex: &str) -> Result<u128, String> {
    let value = hex.trim_start_matches("0x");
    u128::from_str_radix(value, 16).map_err(|e| format!("Invalid hex value: {}", e))
}

async fn verify_payment_tx(
    tx_hash: &str,
    expected_to: &str,
    min_value_wei: u128,
) -> Result<(), String> {
    if !tx_hash.starts_with("0x") || tx_hash.len() != 66 {
        return Err("Invalid transaction hash".to_string());
    }

    let rpc = crate::geth::rpc_endpoint();
    let client = reqwest::Client::new();

    let tx_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionByHash",
        "params": [tx_hash],
        "id": 1
    });
    let tx_resp = client
        .post(&rpc)
        .json(&tx_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to query tx: {}", e))?;
    let tx_json: serde_json::Value = tx_resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse tx response: {}", e))?;
    let tx = tx_json.get("result").ok_or("Tx result missing")?;
    if tx.is_null() {
        return Err("Transaction not found".to_string());
    }

    let tx_to = tx.get("to").and_then(|v| v.as_str()).unwrap_or_default();
    if !tx_to.eq_ignore_ascii_case(expected_to) {
        return Err("Transaction recipient does not match share recipient wallet".to_string());
    }

    let tx_value_hex = tx
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or("Transaction value missing")?;
    let tx_value = parse_hex_u128(tx_value_hex)?;
    if tx_value < min_value_wei {
        return Err("Transaction value is below required share price".to_string());
    }

    let receipt_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
        "id": 2
    });
    let receipt_resp = client
        .post(&rpc)
        .json(&receipt_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to query receipt: {}", e))?;
    let receipt_json: serde_json::Value = receipt_resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse receipt response: {}", e))?;
    let receipt = receipt_json.get("result").ok_or("Receipt result missing")?;
    if receipt.is_null() {
        return Err("Transaction not yet confirmed".to_string());
    }

    let status = receipt
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0");
    if status != "0x1" {
        return Err("Transaction failed on-chain".to_string());
    }

    Ok(())
}

fn is_item_under_shared_root(item: &DriveItem, root: &DriveItem, all_items: &[DriveItem]) -> bool {
    if item.id == root.id {
        return true;
    }
    let mut current_parent = item.parent_id.as_deref();
    while let Some(pid) = current_parent {
        if pid == root.id {
            return true;
        }
        current_parent = all_items
            .iter()
            .find(|i| i.id == pid)
            .and_then(|i| i.parent_id.as_deref());
    }
    false
}

fn normalize_share_price(value: &str) -> String {
    value.trim().to_string()
}

fn share_price_wei(share: &ShareLink) -> Result<u128, String> {
    parse_chi_to_wei(&share.price_chi)
}

async fn verify_share_access(share: &ShareLink, access: Option<&str>) -> Result<(), String> {
    let required_wei = share_price_wei(share)?;
    if required_wei == 0 {
        return Err("This share is not configured with a valid payment amount.".to_string());
    }
    if !is_valid_wallet(&share.recipient_wallet) {
        return Err("Share recipient wallet is invalid.".to_string());
    }
    let tx_hash = access.unwrap_or("").trim();
    if tx_hash.is_empty() {
        return Err("Payment is required to unlock this shared content.".to_string());
    }
    verify_payment_tx(tx_hash, &share.recipient_wallet, required_wei).await
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ListItemsQuery {
    parent_id: Option<String>,
}

#[derive(Deserialize)]
struct CreateFolderRequest {
    name: String,
    parent_id: Option<String>,
}

#[derive(Deserialize)]
struct UpdateItemRequest {
    name: Option<String>,
    parent_id: Option<String>,
    starred: Option<bool>,
}

#[derive(Deserialize)]
struct CreateShareRequest {
    item_id: String,
    price_chi: Option<String>,
    is_public: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShareLinkResponse {
    id: String,
    item_id: String,
    url: String,
    is_public: bool,
    price_chi: String,
    recipient_wallet: String,
    created_at: u64,
    download_count: u64,
}

#[derive(Deserialize)]
struct PublicBrowseQuery {
    access: Option<String>, // on-chain tx hash used as access proof
    dl: Option<u8>,         // 1 = force attachment
    view: Option<u8>,       // 1 = inline preview mode
}

// ---------------------------------------------------------------------------
// API handlers
// ---------------------------------------------------------------------------

/// GET /api/drive/items?parent_id=X
async fn list_items(
    Extension(state): Extension<Arc<DriveState>>,
    headers: HeaderMap,
    Query(q): Query<ListItemsQuery>,
) -> Response {
    let owner = match get_owner(&headers) {
        Some(o) => o,
        None => return (StatusCode::BAD_REQUEST, "X-Owner header required").into_response(),
    };
    let m = state.manifest.read().await;
    let parent = q.parent_id.as_deref();
    let mut items: Vec<&DriveItem> = m
        .items
        .iter()
        .filter(|i| i.parent_id.as_deref() == parent && i.owner == owner)
        .collect();
    // Folders first, then by name
    items.sort_by(|a, b| {
        if a.item_type != b.item_type {
            if a.item_type == "folder" {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    });
    Json(items).into_response()
}

/// POST /api/drive/folders
async fn create_folder(
    Extension(state): Extension<Arc<DriveState>>,
    headers: HeaderMap,
    Json(req): Json<CreateFolderRequest>,
) -> Response {
    let owner = match get_owner(&headers) {
        Some(o) => o,
        None => return (StatusCode::BAD_REQUEST, "X-Owner header required").into_response(),
    };
    if req.name.is_empty() || req.name.len() > 255 {
        return (StatusCode::BAD_REQUEST, "Invalid folder name").into_response();
    }
    let item = DriveItem {
        id: generate_id(),
        name: req.name,
        item_type: "folder".into(),
        parent_id: req.parent_id,
        size: None,
        mime_type: None,
        created_at: now_secs(),
        modified_at: now_secs(),
        starred: false,
        storage_path: None,
        owner,
        is_public: true,
        merkle_root: None,
        protocol: None,
        price_chi: None,
        seed_enabled: false,
        seeding: false,
    };
    {
        let mut m = state.manifest.write().await;
        m.items.push(item.clone());
    }
    state.persist().await;
    (StatusCode::CREATED, Json(item)).into_response()
}

/// POST /api/drive/upload  (multipart: file + optional parent_id field)
async fn upload_file(
    Extension(state): Extension<Arc<DriveState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    let owner = match get_owner(&headers) {
        Some(o) => o,
        None => return (StatusCode::BAD_REQUEST, "X-Owner header required").into_response(),
    };
    let mut parent_id: Option<String> = None;
    let mut file_name: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "parent_id" => {
                if let Ok(text) = field.text().await {
                    if !text.is_empty() {
                        parent_id = Some(text);
                    }
                }
            }
            "file" => {
                file_name = field.file_name().map(|s| s.to_string());
                match field.bytes().await {
                    Ok(bytes) => file_data = Some(bytes.to_vec()),
                    Err(e) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            format!("Failed to read file: {}", e),
                        )
                            .into_response();
                    }
                }
            }
            _ => {}
        }
    }

    let Some(name) = file_name else {
        return (StatusCode::BAD_REQUEST, "No file provided").into_response();
    };
    let Some(data) = file_data else {
        return (StatusCode::BAD_REQUEST, "Empty file").into_response();
    };

    // 500 MB upload limit
    if data.len() > 500 * 1024 * 1024 {
        return (StatusCode::PAYLOAD_TOO_LARGE, "File exceeds 500 MB limit").into_response();
    }

    let item_id = generate_id();
    let storage_name = format!("{}_{}", item_id, name);
    let mime = drive_storage::mime_from_name(&name);

    // Write file to disk
    let files_dir = match drive_storage::drive_files_dir() {
        Some(d) => d,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Cannot determine storage directory",
            )
                .into_response();
        }
    };
    if let Err(e) = std::fs::create_dir_all(&files_dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create storage directory: {}", e),
        )
            .into_response();
    }

    let dest = files_dir.join(&storage_name);
    if let Err(e) = std::fs::write(&dest, &data) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write file: {}", e),
        )
            .into_response();
    }

    let item = DriveItem {
        id: item_id,
        name,
        item_type: "file".into(),
        parent_id,
        size: Some(data.len() as u64),
        mime_type: Some(mime),
        created_at: now_secs(),
        modified_at: now_secs(),
        starred: false,
        storage_path: Some(storage_name),
        owner,
        is_public: true,
        merkle_root: None,
        protocol: None,
        price_chi: None,
        seed_enabled: false,
        seeding: false,
    };

    {
        let mut m = state.manifest.write().await;
        m.items.push(item.clone());
    }
    state.persist().await;

    println!(
        "[DRIVE] Uploaded file: {} ({} bytes)",
        item.name,
        data.len()
    );
    (StatusCode::CREATED, Json(item)).into_response()
}

/// PUT /api/drive/items/:id
async fn update_item(
    Extension(state): Extension<Arc<DriveState>>,
    headers: HeaderMap,
    Path(item_id): Path<String>,
    Json(req): Json<UpdateItemRequest>,
) -> Response {
    let owner = match get_owner(&headers) {
        Some(o) => o,
        None => return (StatusCode::BAD_REQUEST, "X-Owner header required").into_response(),
    };
    let mut m = state.manifest.write().await;
    let Some(item) = m
        .items
        .iter_mut()
        .find(|i| i.id == item_id && i.owner == owner)
    else {
        return (StatusCode::NOT_FOUND, "Item not found").into_response();
    };

    if let Some(name) = req.name {
        if name.is_empty() || name.len() > 255 {
            return (StatusCode::BAD_REQUEST, "Invalid name").into_response();
        }
        item.name = name;
    }
    if let Some(pid) = req.parent_id {
        item.parent_id = if pid.is_empty() { None } else { Some(pid) };
    }
    if let Some(starred) = req.starred {
        item.starred = starred;
    }
    item.modified_at = now_secs();

    let updated = item.clone();
    drop(m);
    state.persist().await;

    Json(updated).into_response()
}

/// DELETE /api/drive/items/:id
async fn delete_item(
    Extension(state): Extension<Arc<DriveState>>,
    headers: HeaderMap,
    Path(item_id): Path<String>,
) -> Response {
    let owner = match get_owner(&headers) {
        Some(o) => o,
        None => return (StatusCode::BAD_REQUEST, "X-Owner header required").into_response(),
    };
    // Snapshot owned items to avoid holding the lock during filesystem I/O.
    let (to_delete, file_paths): (HashSet<String>, Vec<std::path::PathBuf>) = {
        let m = state.manifest.read().await;
        let owned_items: Vec<DriveItem> = m
            .items
            .iter()
            .filter(|i| i.owner == owner)
            .cloned()
            .collect();

        if !owned_items.iter().any(|i| i.id == item_id) {
            return (StatusCode::NOT_FOUND, "Item not found").into_response();
        }

        let to_delete: HashSet<String> = collect_descendants(&item_id, &owned_items)
            .into_iter()
            .collect();

        let files = if let Some(files_dir) = drive_storage::drive_files_dir() {
            owned_items
                .iter()
                .filter(|i| to_delete.contains(&i.id) && i.item_type == "file")
                .filter_map(|i| i.storage_path.as_ref().map(|sp| files_dir.join(sp)))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        (to_delete, files)
    };

    let mut errors = Vec::new();
    for path in file_paths {
        match std::fs::remove_file(&path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => errors.push(format!("{}: {}", path.display(), e)),
        }
    }

    if !errors.is_empty() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete file(s): {}", errors.join(" | ")),
        )
            .into_response();
    }

    let mut m = state.manifest.write().await;
    m.items.retain(|i| !to_delete.contains(&i.id));
    m.shares.retain(|s| !to_delete.contains(&s.item_id));
    drop(m);
    state.persist().await;

    (StatusCode::OK, "Deleted").into_response()
}

/// GET /api/drive/download/:id/:filename  — direct file download
/// The filename in the URL path ensures browsers save with the correct extension.
async fn download_file(
    Extension(state): Extension<Arc<DriveState>>,
    headers: HeaderMap,
    Path((item_id, _filename)): Path<(String, String)>,
) -> Response {
    let m = state.manifest.read().await;
    let owner = get_owner(&headers);
    let Some(item) = m
        .items
        .iter()
        .find(|i| i.id == item_id && (owner.is_none() || i.owner == *owner.as_ref().unwrap()))
    else {
        return (StatusCode::NOT_FOUND, "Item not found").into_response();
    };
    if item.item_type != "file" {
        return (StatusCode::BAD_REQUEST, "Cannot download a folder").into_response();
    }
    let Some(sp) = &item.storage_path else {
        return (StatusCode::NOT_FOUND, "File not stored on server").into_response();
    };
    let Some(files_dir) = drive_storage::drive_files_dir() else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Storage error").into_response();
    };
    let path = files_dir.join(sp);
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(_) => return (StatusCode::NOT_FOUND, "File not found on disk").into_response(),
    };
    let content_type = item
        .mime_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".to_string());

    (
        StatusCode::OK,
        [
            ("Content-Type", content_type),
            ("Content-Length", data.len().to_string()),
            (
                "Content-Disposition",
                format!("attachment; filename=\"{}\"", item.name),
            ),
        ],
        data,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Share link handlers
// ---------------------------------------------------------------------------

/// POST /api/drive/share
async fn create_share(
    Extension(state): Extension<Arc<DriveState>>,
    headers: HeaderMap,
    Json(req): Json<CreateShareRequest>,
) -> Response {
    let owner = match get_owner(&headers) {
        Some(o) => o,
        None => return (StatusCode::BAD_REQUEST, "X-Owner header required").into_response(),
    };
    let mut m = state.manifest.write().await;

    let Some(item) = m
        .items
        .iter()
        .find(|i| i.id == req.item_id && i.owner == owner)
        .cloned()
    else {
        return (StatusCode::NOT_FOUND, "Item not found").into_response();
    };

    if !is_valid_wallet(&item.owner) {
        return (
            StatusCode::BAD_REQUEST,
            "Item owner wallet must be a valid 0x address",
        )
            .into_response();
    }

    let requested_price = req
        .price_chi
        .or_else(|| item.price_chi.clone())
        .unwrap_or_default();
    let normalized_price = normalize_share_price(&requested_price);
    let price_wei = match parse_chi_to_wei(&normalized_price) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    if price_wei == 0 {
        return (
            StatusCode::BAD_REQUEST,
            "Share price must be greater than 0 CHI",
        )
            .into_response();
    }

    let token = generate_share_token();
    let share = ShareLink {
        id: token.clone(),
        item_id: req.item_id,
        created_at: now_secs(),
        expires_at: None,
        price_chi: normalized_price,
        recipient_wallet: item.owner,
        is_public: req.is_public.unwrap_or(true),
        download_count: 0,
    };
    m.shares.push(share.clone());
    drop(m);
    state.persist().await;

    let resp = ShareLinkResponse {
        id: share.id.clone(),
        item_id: share.item_id,
        url: format!("/drive/{}", share.id),
        is_public: share.is_public,
        price_chi: share.price_chi,
        recipient_wallet: share.recipient_wallet,
        created_at: share.created_at,
        download_count: 0,
    };
    (StatusCode::CREATED, Json(resp)).into_response()
}

/// DELETE /api/drive/share/:token
async fn revoke_share(
    Extension(state): Extension<Arc<DriveState>>,
    headers: HeaderMap,
    Path(token): Path<String>,
) -> Response {
    let owner = match get_owner(&headers) {
        Some(o) => o,
        None => return (StatusCode::BAD_REQUEST, "X-Owner header required").into_response(),
    };
    let mut m = state.manifest.write().await;
    // Only allow revoking shares for items the owner owns
    let share = m.shares.iter().find(|s| s.id == token);
    let is_owner = share.map_or(false, |s| {
        m.items
            .iter()
            .any(|i| i.id == s.item_id && i.owner == owner)
    });
    if !is_owner {
        return (StatusCode::NOT_FOUND, "Share link not found").into_response();
    }
    m.shares.retain(|s| s.id != token);
    drop(m);
    state.persist().await;
    (StatusCode::OK, "Revoked").into_response()
}

/// GET /api/drive/shares
async fn list_shares(Extension(state): Extension<Arc<DriveState>>, headers: HeaderMap) -> Response {
    let owner = match get_owner(&headers) {
        Some(o) => o,
        None => return (StatusCode::BAD_REQUEST, "X-Owner header required").into_response(),
    };
    let m = state.manifest.read().await;
    // Only return shares for items owned by this user
    let owner_item_ids: HashSet<&str> = m
        .items
        .iter()
        .filter(|i| i.owner == owner)
        .map(|i| i.id.as_str())
        .collect();
    let responses: Vec<ShareLinkResponse> = m
        .shares
        .iter()
        .filter(|s| owner_item_ids.contains(s.item_id.as_str()))
        .map(|s| ShareLinkResponse {
            id: s.id.clone(),
            item_id: s.item_id.clone(),
            url: format!("/drive/{}", s.id),
            is_public: s.is_public,
            price_chi: s.price_chi.clone(),
            recipient_wallet: s.recipient_wallet.clone(),
            created_at: s.created_at,
            download_count: s.download_count,
        })
        .collect();
    Json(responses).into_response()
}

// ---------------------------------------------------------------------------
// Public browse & download handlers
// ---------------------------------------------------------------------------

/// GET /drive/:token  — public browse page
async fn public_browse(
    Extension(state): Extension<Arc<DriveState>>,
    Path(token): Path<String>,
    Query(q): Query<PublicBrowseQuery>,
) -> Response {
    let (share, item, children) = {
        let m = state.manifest.read().await;
        let Some(share) = m.shares.iter().find(|s| s.id == token).cloned() else {
            return (
                StatusCode::NOT_FOUND,
                Html(error_page("Share link not found")),
            )
                .into_response();
        };
        let Some(item) = m.items.iter().find(|i| i.id == share.item_id).cloned() else {
            return (
                StatusCode::NOT_FOUND,
                Html(error_page("Shared item no longer exists")),
            )
                .into_response();
        };
        let children = m
            .items
            .iter()
            .filter(|i| i.parent_id.as_deref() == Some(&item.id))
            .cloned()
            .collect::<Vec<_>>();
        (share, item, children)
    };

    if !item.is_public {
        return (
            StatusCode::FORBIDDEN,
            Html(error_page("This file is currently unavailable")),
        )
            .into_response();
    }

    if let Err(reason) = verify_share_access(&share, q.access.as_deref()).await {
        return Html(payment_page(&item, &token, &share, &reason)).into_response();
    }

    let access = q.access.as_deref().unwrap_or("");

    if item.item_type == "file" {
        Html(file_download_page(&item, &token, access)).into_response()
    } else {
        Html(folder_browse_page(&item, &children, &token, "", access)).into_response()
    }
}

/// GET /drive/:token/download/:item_id/:filename  — download or preview shared file data
async fn public_download(
    Extension(state): Extension<Arc<DriveState>>,
    Path((token, subpath)): Path<(String, String)>,
    Query(q): Query<PublicBrowseQuery>,
) -> Response {
    let item_id_hint = if subpath == "download" {
        None
    } else if let Some(rest) = subpath.strip_prefix("download/") {
        rest.split('/').next().filter(|s| !s.trim().is_empty())
    } else {
        None
    };

    let (share, root_item, target_item) = {
        let m = state.manifest.read().await;
        let Some(share) = m.shares.iter().find(|s| s.id == token).cloned() else {
            return (StatusCode::NOT_FOUND, "Share link not found").into_response();
        };

        let Some(root_item) = m.items.iter().find(|i| i.id == share.item_id).cloned() else {
            return (StatusCode::NOT_FOUND, "Shared item not found").into_response();
        };
        if !root_item.is_public {
            return (StatusCode::FORBIDDEN, "This file is currently unavailable").into_response();
        }

        let target_id = item_id_hint.unwrap_or(share.item_id.as_str());
        let Some(target_item) = m.items.iter().find(|i| i.id == target_id).cloned() else {
            return (StatusCode::NOT_FOUND, "Item not found").into_response();
        };

        if !is_item_under_shared_root(&target_item, &root_item, &m.items) {
            return (StatusCode::FORBIDDEN, "Item is outside shared scope").into_response();
        }
        if target_item.item_type != "file" {
            return (StatusCode::BAD_REQUEST, "Cannot download a folder").into_response();
        }
        (share, root_item, target_item)
    };

    if let Err(reason) = verify_share_access(&share, q.access.as_deref()).await {
        return Html(payment_page(&root_item, &token, &share, &reason)).into_response();
    }

    let Some(sp) = target_item.storage_path.clone() else {
        return (StatusCode::NOT_FOUND, "File not stored").into_response();
    };
    let file_name = target_item.name.clone();
    let content_type = target_item
        .mime_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let as_attachment = q.view.unwrap_or(0) == 0 && q.dl.unwrap_or(1) != 0;
    if as_attachment {
        let mut m = state.manifest.write().await;
        if let Some(link) = m.shares.iter_mut().find(|s| s.id == token) {
            link.download_count += 1;
        }
        drop(m);
        state.persist().await;
    }

    let Some(files_dir) = drive_storage::drive_files_dir() else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Storage error").into_response();
    };
    let file_path = files_dir.join(&sp);

    let data = match std::fs::read(&file_path) {
        Ok(d) => d,
        Err(_) => return (StatusCode::NOT_FOUND, "File not found on disk").into_response(),
    };

    let disposition = if as_attachment {
        format!("attachment; filename=\"{}\"", file_name)
    } else {
        format!("inline; filename=\"{}\"", file_name)
    };

    (
        StatusCode::OK,
        [
            ("Content-Type", content_type),
            ("Content-Length", data.len().to_string()),
            ("Content-Disposition", disposition),
        ],
        data,
    )
        .into_response()
}

/// GET /drive/:token/*path  — browse subfolder of shared folder
async fn public_browse_path(
    Extension(state): Extension<Arc<DriveState>>,
    Path((token, subpath)): Path<(String, String)>,
    Query(q): Query<PublicBrowseQuery>,
) -> Response {
    // Handle "download" and "download/..." as special file-data routes
    if subpath == "download" || subpath.starts_with("download/") {
        return public_download(Extension(state), Path((token, subpath)), Query(q)).await;
    }

    let (share, root_item, item, children) = {
        let m = state.manifest.read().await;
        let Some(share) = m.shares.iter().find(|s| s.id == token).cloned() else {
            return (
                StatusCode::NOT_FOUND,
                Html(error_page("Share link not found")),
            )
                .into_response();
        };

        let Some(root_item) = m.items.iter().find(|i| i.id == share.item_id).cloned() else {
            return (
                StatusCode::NOT_FOUND,
                Html(error_page("Shared item no longer exists")),
            )
                .into_response();
        };
        if !root_item.is_public {
            return (
                StatusCode::FORBIDDEN,
                Html(error_page("This content is currently unavailable")),
            )
                .into_response();
        }

        // Navigate to subfolder by path segments (item IDs)
        let target_id = subpath.trim_matches('/');
        let Some(item) = m.items.iter().find(|i| i.id == target_id).cloned() else {
            return (StatusCode::NOT_FOUND, Html(error_page("Item not found"))).into_response();
        };

        if !is_item_under_shared_root(&item, &root_item, &m.items) {
            return (
                StatusCode::FORBIDDEN,
                Html(error_page("Item is outside shared scope")),
            )
                .into_response();
        }

        let children = m
            .items
            .iter()
            .filter(|i| i.parent_id.as_deref() == Some(&item.id))
            .cloned()
            .collect::<Vec<_>>();

        (share, root_item, item, children)
    };

    if let Err(reason) = verify_share_access(&share, q.access.as_deref()).await {
        return Html(payment_page(&root_item, &token, &share, &reason)).into_response();
    }

    let access = q.access.as_deref().unwrap_or("");
    if item.item_type == "file" {
        Html(file_download_page(&item, &token, access)).into_response()
    } else {
        Html(folder_browse_page(
            &item, &children, &token, &subpath, access,
        ))
        .into_response()
    }
}

// ---------------------------------------------------------------------------
// HTML templates for public pages
// ---------------------------------------------------------------------------

fn error_page(msg: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Chiral Drive</title>
<script src="https://cdn.tailwindcss.com"></script>
</head><body class="bg-gray-900 text-white flex items-center justify-center min-h-screen">
<div class="text-center"><h1 class="text-2xl font-bold mb-2">Not Found</h1><p class="text-gray-400">{}</p></div>
</body></html>"#,
        msg
    )
}

fn payment_page(item: &DriveItem, token: &str, share: &ShareLink, reason: &str) -> String {
    let size_str = item
        .size
        .map(|s| format_bytes(s))
        .unwrap_or_else(|| "Unknown size".into());
    let reason = html_escape(reason);
    let name = html_escape(&item.name);
    let recipient = html_escape(&share.recipient_wallet);
    let price = html_escape(&share.price_chi);
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Chiral Drive - Payment Required</title>
<script src="https://cdn.tailwindcss.com"></script>
</head><body class="bg-gray-900 text-white min-h-screen">
<div class="max-w-xl mx-auto px-4 py-10">
  <div class="bg-gray-800 border border-gray-700 rounded-xl p-6 shadow-2xl">
    <h1 class="text-2xl font-bold mb-2">Payment Required</h1>
    <p class="text-sm text-gray-300 mb-4">Log in with your wallet in-browser, then pay to unlock this shared content.</p>
    <div class="rounded-lg bg-amber-950/50 border border-amber-800 px-3 py-2 text-sm text-amber-200 mb-4">{reason}</div>

    <div class="grid grid-cols-2 gap-3 text-sm mb-6">
      <div class="bg-gray-900/60 border border-gray-700 rounded-lg p-3">
        <p class="text-gray-400 text-xs mb-1">Item</p>
        <p class="font-medium break-all">{name}</p>
        <p class="text-gray-500 text-xs mt-1">{size}</p>
      </div>
      <div class="bg-gray-900/60 border border-gray-700 rounded-lg p-3">
        <p class="text-gray-400 text-xs mb-1">Unlock Price</p>
        <p class="font-semibold text-green-300">{price} CHI</p>
        <p class="text-gray-500 text-xs mt-1 break-all">Recipient: {recipient}</p>
      </div>
    </div>

    <div class="space-y-3 mb-5">
      <label class="block text-sm text-gray-300">Wallet Private Key</label>
      <input id="pk" type="password" placeholder="0x..." class="w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 focus:outline-none focus:ring-2 focus:ring-blue-500" />
      <button id="loginBtn" class="px-4 py-2 rounded-lg bg-blue-600 hover:bg-blue-700 text-sm font-medium transition">Log In</button>
      <button id="logoutBtn" class="px-4 py-2 rounded-lg bg-gray-700 hover:bg-gray-600 text-sm font-medium transition hidden">Log Out</button>
      <p id="walletInfo" class="text-xs text-gray-400"></p>
    </div>

    <div class="border-t border-gray-700 pt-5">
      <button id="payBtn" disabled class="w-full px-4 py-3 rounded-lg bg-green-600/70 text-white font-semibold disabled:opacity-40 disabled:cursor-not-allowed hover:bg-green-700 transition">Pay & Unlock</button>
      <p id="status" class="text-xs text-gray-400 mt-3 min-h-4"></p>
      <p class="text-xs text-gray-500 mt-2">Your private key stays in this browser session and is not uploaded.</p>
    </div>
  </div>
</div>
<script type="module">
  import {{ ethers }} from "https://esm.sh/ethers@6.13.5";

  const token = "{token}";
  const recipient = "{recipient_js}";
  const priceChi = "{price_js}";
  const redirectBase = `/drive/${{token}}`;
  const rpcUrl = new URL("/api/chain/rpc", window.location.origin).toString();
  const storageKey = `chiral-share-wallet-${{token}}`;

  const pkInput = document.getElementById("pk");
  const loginBtn = document.getElementById("loginBtn");
  const logoutBtn = document.getElementById("logoutBtn");
  const walletInfo = document.getElementById("walletInfo");
  const payBtn = document.getElementById("payBtn");
  const statusEl = document.getElementById("status");

  let privateKey = sessionStorage.getItem(storageKey) || "";
  let walletAddress = "";

  function setStatus(msg, isError = false) {{
    statusEl.textContent = msg;
    statusEl.className = isError ? "text-xs text-red-300 mt-3 min-h-4" : "text-xs text-gray-300 mt-3 min-h-4";
  }}

  function normalizePrivateKey(input) {{
    const raw = (input || "").trim();
    if (!raw) return "";
    return raw.startsWith("0x") ? raw : `0x${{raw}}`;
  }}

  function setLoggedOut() {{
    privateKey = "";
    walletAddress = "";
    payBtn.disabled = true;
    pkInput.value = "";
    pkInput.type = "password";
    loginBtn.classList.remove("hidden");
    logoutBtn.classList.add("hidden");
    walletInfo.textContent = "Not logged in";
    sessionStorage.removeItem(storageKey);
  }}

  function setLoggedIn(nextPrivateKey) {{
    const wallet = new ethers.Wallet(nextPrivateKey);
    privateKey = nextPrivateKey;
    walletAddress = wallet.address;
    payBtn.disabled = false;
    pkInput.value = "";
    pkInput.type = "password";
    loginBtn.classList.add("hidden");
    logoutBtn.classList.remove("hidden");
    walletInfo.textContent = `Logged in: ${{walletAddress}}`;
    sessionStorage.setItem(storageKey, privateKey);
  }}

  loginBtn.addEventListener("click", () => {{
    try {{
      const normalized = normalizePrivateKey(pkInput.value);
      if (!normalized) {{
        setStatus("Enter a private key to continue.", true);
        return;
      }}
      setLoggedIn(normalized);
      setStatus("Wallet login successful.");
    }} catch (err) {{
      setStatus(err?.message || "Invalid private key.", true);
    }}
  }});

  logoutBtn.addEventListener("click", () => {{
    setLoggedOut();
    setStatus("Logged out.");
  }});

  payBtn.addEventListener("click", async () => {{
    if (!privateKey) {{
      setStatus("Log in with a wallet first.", true);
      return;
    }}
    payBtn.disabled = true;
    try {{
      const provider = new ethers.JsonRpcProvider(rpcUrl);
      const wallet = new ethers.Wallet(privateKey, provider);
      setStatus("Submitting payment transaction...");
      const tx = await wallet.sendTransaction({{
        to: recipient,
        value: ethers.parseEther(priceChi),
      }});
      setStatus(`Transaction sent: ${{tx.hash}}. Waiting for confirmation...`);
      await tx.wait(1);
      setStatus("Payment confirmed. Unlocking...");
      const nextUrl = new URL(redirectBase, window.location.origin);
      nextUrl.searchParams.set("access", tx.hash);
      window.location.href = nextUrl.toString();
    }} catch (err) {{
      payBtn.disabled = false;
      setStatus(err?.message || "Payment failed.", true);
    }}
  }});

  if (privateKey) {{
    try {{
      setLoggedIn(privateKey);
      setStatus("Restored wallet session.");
    }} catch {{
      setLoggedOut();
    }}
  }} else {{
    setLoggedOut();
  }}
</script>
</body></html>"#,
        reason = reason,
        name = name,
        size = size_str,
        price = price,
        recipient = recipient,
        token = token,
        recipient_js = share.recipient_wallet,
        price_js = share.price_chi,
    )
}

fn preview_kind(item: &DriveItem) -> &'static str {
    let mime = item.mime_type.as_deref().unwrap_or("");
    if mime.starts_with("image/") {
        return "image";
    }
    if mime.starts_with("video/") {
        return "video";
    }
    if mime.starts_with("audio/") {
        return "audio";
    }
    if mime == "application/pdf" {
        return "pdf";
    }
    if mime.starts_with("text/") || mime == "application/json" {
        return "text";
    }
    let ext = item
        .name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => "image",
        "mp4" | "webm" => "video",
        "mp3" | "wav" => "audio",
        "pdf" => "pdf",
        "txt" | "md" | "json" | "csv" | "log" => "text",
        _ => "none",
    }
}

fn file_download_page(item: &DriveItem, token: &str, access: &str) -> String {
    let size_str = item
        .size
        .map(|s| format_bytes(s))
        .unwrap_or_else(|| "Unknown size".into());
    let encoded_access = url_encode(access);
    let encoded_name = url_encode(&item.name);
    let encoded_item_id = url_encode(&item.id);
    let preview_url = format!(
        "/drive/{}/download/{}/{}?access={}&view=1",
        token, encoded_item_id, encoded_name, encoded_access
    );
    let download_url = format!(
        "/drive/{}/download/{}/{}?access={}&dl=1",
        token, encoded_item_id, encoded_name, encoded_access
    );
    let preview_html = match preview_kind(item) {
        "image" => format!(
            r#"<img src="{url}" alt="{name}" class="max-h-[68vh] max-w-full object-contain rounded-xl border border-gray-700 bg-gray-900" />"#,
            url = preview_url,
            name = html_escape(&item.name),
        ),
        "video" => format!(
            r#"<video controls class="w-full rounded-xl border border-gray-700 bg-black max-h-[68vh]"><source src="{url}" /></video>"#,
            url = preview_url,
        ),
        "audio" => format!(
            r#"<audio controls class="w-full"><source src="{url}" /></audio>"#,
            url = preview_url,
        ),
        "pdf" | "text" => format!(
            r#"<iframe src="{url}" class="w-full h-[70vh] rounded-xl border border-gray-700 bg-gray-900" title="preview"></iframe>"#,
            url = preview_url,
        ),
        _ => r#"<div class="rounded-xl border border-gray-700 bg-gray-900 px-4 py-10 text-center text-gray-400 text-sm">Preview is not available for this file type.</div>"#.to_string(),
    };

    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Chiral Drive - {name}</title>
<script src="https://cdn.tailwindcss.com"></script>
</head><body class="bg-gray-900 text-white min-h-screen">
<div class="max-w-5xl mx-auto py-8 px-4">
  <div class="flex items-center justify-between gap-4 mb-5">
    <div>
      <h1 class="text-xl font-bold break-all">{name}</h1>
      <p class="text-sm text-gray-400">{size}</p>
    </div>
    <a href="{download_url}" class="inline-flex items-center justify-center px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg font-medium transition">Download</a>
  </div>
  <div class="bg-gray-800 rounded-xl border border-gray-700 p-4">
    {preview}
  </div>
  <p class="text-xs text-gray-500 mt-4">Shared via Chiral Network</p>
</div></body></html>"#,
        name = html_escape(&item.name),
        size = size_str,
        download_url = download_url,
        preview = preview_html,
    )
}

fn folder_browse_page(
    folder: &DriveItem,
    children: &[DriveItem],
    token: &str,
    _current_path: &str,
    access: &str,
) -> String {
    let mut rows = String::new();
    let encoded_access = url_encode(access);
    for child in children {
        let icon = if child.item_type == "folder" {
            r#"<svg class="w-5 h-5 text-yellow-400" fill="currentColor" viewBox="0 0 24 24"><path d="M10 4H4a2 2 0 00-2 2v12a2 2 0 002 2h16a2 2 0 002-2V8a2 2 0 00-2-2h-8l-2-2z"/></svg>"#
        } else {
            r#"<svg class="w-5 h-5 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"/></svg>"#
        };
        let href = format!("/drive/{}/{}?access={}", token, child.id, encoded_access);
        let size = child
            .size
            .map(|s| format_bytes(s))
            .unwrap_or_else(|| "—".into());
        rows.push_str(&format!(
            r#"<a href="{href}" class="flex items-center gap-3 px-4 py-3 hover:bg-gray-700/50 transition border-b border-gray-700/50">
{icon}
<span class="flex-1 text-sm font-medium">{name}</span>
<span class="text-xs text-gray-500">{size}</span>
</a>"#,
            href = href,
            icon = icon,
            name = html_escape(&child.name),
            size = size,
        ));
    }

    if rows.is_empty() {
        rows = r#"<div class="px-4 py-8 text-center text-gray-500 text-sm">This folder is empty</div>"#.to_string();
    }

    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Chiral Drive - {name}</title>
<script src="https://cdn.tailwindcss.com"></script>
</head><body class="bg-gray-900 text-white min-h-screen">
<div class="max-w-2xl mx-auto py-8 px-4">
<div class="flex items-center gap-3 mb-6">
<svg class="w-8 h-8 text-blue-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4"/></svg>
<div><h1 class="text-xl font-bold">{name}</h1><p class="text-xs text-gray-400">Shared via Chiral Network</p></div>
</div>
<div class="bg-gray-800 rounded-xl border border-gray-700 overflow-hidden">
{rows}
</div>
</div></body></html>"#,
        name = html_escape(&folder.name),
        rows = rows,
    )
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Create the Drive API router. Uses Extension for state injection.
pub fn drive_routes(state: Arc<DriveState>) -> Router {
    Router::new()
        // API routes
        .route("/api/drive/items", get(list_items))
        .route("/api/drive/folders", post(create_folder))
        .route("/api/drive/upload", post(upload_file))
        .route("/api/drive/items/:id", put(update_item).delete(delete_item))
        .route("/api/drive/download/:id/:filename", get(download_file))
        .route("/api/drive/share", post(create_share))
        .route("/api/drive/share/:token", delete(revoke_share))
        .route("/api/drive/shares", get(list_shares))
        // Public browse/download routes
        .route("/drive/:token", get(public_browse))
        .route("/drive/:token/*path", get(public_browse_path))
        .layer(Extension(state))
}
