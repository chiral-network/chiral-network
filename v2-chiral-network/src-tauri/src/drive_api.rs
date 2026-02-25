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
    self, collect_descendants, generate_id, generate_share_token, hash_password, now_secs,
    DriveItem, DriveManifest, ShareLink,
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

    async fn persist(&self) {
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
    password: Option<String>,
    is_public: Option<bool>,
}

#[derive(Serialize)]
struct ShareLinkResponse {
    id: String,
    item_id: String,
    url: String,
    is_public: bool,
    has_password: bool,
    created_at: u64,
    download_count: u64,
}

#[derive(Deserialize)]
struct PublicBrowseQuery {
    p: Option<String>, // password for protected shares
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
    };

    {
        let mut m = state.manifest.write().await;
        m.items.push(item.clone());
    }
    state.persist().await;

    println!("[DRIVE] Uploaded file: {} ({} bytes)", item.name, data.len());
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
    let Some(item) = m.items.iter_mut().find(|i| i.id == item_id && i.owner == owner) else {
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
    let mut m = state.manifest.write().await;

    // Check exists and belongs to owner
    if !m.items.iter().any(|i| i.id == item_id && i.owner == owner) {
        return (StatusCode::NOT_FOUND, "Item not found").into_response();
    }

    // Collect all IDs to delete (recursive)
    let to_delete: HashSet<String> = collect_descendants(&item_id, &m.items)
        .into_iter()
        .collect();

    // Delete files from disk
    if let Some(files_dir) = drive_storage::drive_files_dir() {
        for id in &to_delete {
            if let Some(item) = m.items.iter().find(|i| &i.id == id) {
                if let Some(sp) = &item.storage_path {
                    let path = files_dir.join(sp);
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }

    // Remove items
    m.items.retain(|i| !to_delete.contains(&i.id));

    // Remove any share links pointing to deleted items
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
    let Some(item) = m.items.iter().find(|i| i.id == item_id && (owner.is_none() || i.owner == *owner.as_ref().unwrap())) else {
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

    // Verify item exists and belongs to owner
    if !m.items.iter().any(|i| i.id == req.item_id && i.owner == owner) {
        return (StatusCode::NOT_FOUND, "Item not found").into_response();
    }

    let token = generate_share_token();
    let share = ShareLink {
        id: token.clone(),
        item_id: req.item_id,
        created_at: now_secs(),
        expires_at: None,
        password_hash: req.password.as_deref().map(hash_password),
        is_public: req.is_public.unwrap_or(false),
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
        has_password: share.password_hash.is_some(),
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
        m.items.iter().any(|i| i.id == s.item_id && i.owner == owner)
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
async fn list_shares(
    Extension(state): Extension<Arc<DriveState>>,
    headers: HeaderMap,
) -> Response {
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
            has_password: s.password_hash.is_some(),
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
    let m = state.manifest.read().await;
    let Some(share) = m.shares.iter().find(|s| s.id == token) else {
        return (StatusCode::NOT_FOUND, Html(error_page("Share link not found"))).into_response();
    };

    // Password check
    if let Some(pw_hash) = &share.password_hash {
        match &q.p {
            Some(pw) if &hash_password(pw) == pw_hash => {} // OK
            _ => {
                return Html(password_page(&token)).into_response();
            }
        }
    }

    let Some(item) = m.items.iter().find(|i| i.id == share.item_id) else {
        return (StatusCode::NOT_FOUND, Html(error_page("Shared item no longer exists")))
            .into_response();
    };

    let pw_param = q.p.as_deref().map(|p| format!("&p={}", p)).unwrap_or_default();

    if item.item_type == "file" {
        // Single file share — show download page
        Html(file_download_page(item, &token, &pw_param)).into_response()
    } else {
        // Folder share — list contents
        let children: Vec<&DriveItem> = m
            .items
            .iter()
            .filter(|i| i.parent_id.as_deref() == Some(&item.id))
            .collect();
        Html(folder_browse_page(item, &children, &token, "", &pw_param)).into_response()
    }
}

/// GET /drive/:token/download  — download the shared file
async fn public_download(
    Extension(state): Extension<Arc<DriveState>>,
    Path(token): Path<String>,
    Query(q): Query<PublicBrowseQuery>,
) -> Response {
    let mut m = state.manifest.write().await;
    let Some(share) = m.shares.iter_mut().find(|s| s.id == token) else {
        return (StatusCode::NOT_FOUND, "Share link not found").into_response();
    };

    // Password check
    if let Some(pw_hash) = &share.password_hash {
        match &q.p {
            Some(pw) if &hash_password(pw) == pw_hash => {}
            _ => return (StatusCode::UNAUTHORIZED, "Password required").into_response(),
        }
    }

    share.download_count += 1;
    let item_id = share.item_id.clone();

    let Some(item) = m.items.iter().find(|i| i.id == item_id) else {
        return (StatusCode::NOT_FOUND, "Item not found").into_response();
    };
    if item.item_type != "file" {
        return (StatusCode::BAD_REQUEST, "Cannot download a folder").into_response();
    }
    let Some(sp) = &item.storage_path else {
        return (StatusCode::NOT_FOUND, "File not stored").into_response();
    };
    let Some(files_dir) = drive_storage::drive_files_dir() else {
        return (StatusCode::INTERNAL_SERVER_ERROR, "Storage error").into_response();
    };
    let file_path = files_dir.join(sp);
    let file_name = item.name.clone();
    let content_type = item
        .mime_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".to_string());

    drop(m);
    state.persist().await;

    let data = match std::fs::read(&file_path) {
        Ok(d) => d,
        Err(_) => return (StatusCode::NOT_FOUND, "File not found on disk").into_response(),
    };

    (
        StatusCode::OK,
        [
            ("Content-Type", content_type),
            ("Content-Length", data.len().to_string()),
            (
                "Content-Disposition",
                format!("attachment; filename=\"{}\"", file_name),
            ),
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
    // Handle "download" or "download/filename.ext" as the special download route
    if subpath == "download" || subpath.starts_with("download/") {
        return public_download(Extension(state), Path(token), Query(q)).await;
    }

    let m = state.manifest.read().await;
    let Some(share) = m.shares.iter().find(|s| s.id == token) else {
        return (StatusCode::NOT_FOUND, Html(error_page("Share link not found"))).into_response();
    };

    // Password check
    if let Some(pw_hash) = &share.password_hash {
        match &q.p {
            Some(pw) if &hash_password(pw) == pw_hash => {}
            _ => return Html(password_page(&token)).into_response(),
        }
    }

    // Navigate to subfolder by path segments (item IDs)
    let target_id = subpath.trim_matches('/');
    let Some(item) = m.items.iter().find(|i| i.id == target_id) else {
        return (StatusCode::NOT_FOUND, Html(error_page("Item not found"))).into_response();
    };

    let pw_param = q.p.as_deref().map(|p| format!("&p={}", p)).unwrap_or_default();

    if item.item_type == "file" {
        // It's a file within the shared folder — offer download
        let Some(sp) = &item.storage_path else {
            return (StatusCode::NOT_FOUND, Html(error_page("File not stored"))).into_response();
        };
        let Some(files_dir) = drive_storage::drive_files_dir() else {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Storage error").into_response();
        };
        let file_path = files_dir.join(sp);
        let data = match std::fs::read(&file_path) {
            Ok(d) => d,
            Err(_) => {
                return (StatusCode::NOT_FOUND, Html(error_page("File not found on disk")))
                    .into_response()
            }
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
    } else {
        // It's a subfolder — list contents
        let children: Vec<&DriveItem> = m
            .items
            .iter()
            .filter(|i| i.parent_id.as_deref() == Some(&item.id))
            .collect();
        Html(folder_browse_page(
            item,
            &children,
            &token,
            &subpath,
            &pw_param,
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

fn password_page(token: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Chiral Drive - Password Required</title>
<script src="https://cdn.tailwindcss.com"></script>
</head><body class="bg-gray-900 text-white flex items-center justify-center min-h-screen">
<div class="bg-gray-800 rounded-xl p-8 max-w-md w-full mx-4 shadow-2xl">
<h1 class="text-xl font-bold mb-2">Password Required</h1>
<p class="text-gray-400 text-sm mb-6">This shared content is password protected.</p>
<form method="GET" action="/drive/{}">
<input type="password" name="p" placeholder="Enter password" required
  class="w-full px-4 py-3 bg-gray-700 border border-gray-600 rounded-lg text-white placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-blue-500 mb-4" />
<button type="submit" class="w-full px-4 py-3 bg-blue-600 hover:bg-blue-700 text-white rounded-lg font-medium transition">
Access Files</button>
</form></div></body></html>"#,
        token
    )
}

fn file_download_page(item: &DriveItem, token: &str, pw_param: &str) -> String {
    let size_str = item
        .size
        .map(|s| format_bytes(s))
        .unwrap_or_else(|| "Unknown size".into());
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Chiral Drive - {name}</title>
<script src="https://cdn.tailwindcss.com"></script>
</head><body class="bg-gray-900 text-white flex items-center justify-center min-h-screen">
<div class="bg-gray-800 rounded-xl p-8 max-w-md w-full mx-4 shadow-2xl text-center">
<div class="w-16 h-16 bg-gray-700 rounded-full flex items-center justify-center mx-auto mb-4">
<svg class="w-8 h-8 text-blue-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M9 19l3 3m0 0l3-3m-3 3V10"/></svg>
</div>
<h1 class="text-xl font-bold mb-1">{name}</h1>
<p class="text-gray-400 text-sm mb-6">{size}</p>
<a href="/drive/{token}/download/{urlname}?dl=1{pw}"
  class="inline-block px-6 py-3 bg-blue-600 hover:bg-blue-700 text-white rounded-lg font-medium transition">
Download File</a>
<p class="text-xs text-gray-500 mt-4">Shared via Chiral Network</p>
</div></body></html>"#,
        name = html_escape(&item.name),
        urlname = url_encode(&item.name),
        size = size_str,
        token = token,
        pw = pw_param,
    )
}

fn folder_browse_page(
    folder: &DriveItem,
    children: &[&DriveItem],
    token: &str,
    _current_path: &str,
    pw_param: &str,
) -> String {
    let mut rows = String::new();
    for child in children {
        let icon = if child.item_type == "folder" {
            r#"<svg class="w-5 h-5 text-yellow-400" fill="currentColor" viewBox="0 0 24 24"><path d="M10 4H4a2 2 0 00-2 2v12a2 2 0 002 2h16a2 2 0 002-2V8a2 2 0 00-2-2h-8l-2-2z"/></svg>"#
        } else {
            r#"<svg class="w-5 h-5 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"/></svg>"#
        };
        let href = if child.item_type == "folder" {
            format!("/drive/{}/{}?{}", token, child.id, pw_param.trim_start_matches('&'))
        } else {
            format!("/drive/{}/{}?{}", token, child.id, pw_param.trim_start_matches('&'))
        };
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
