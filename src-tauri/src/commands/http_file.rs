use reqwest::multipart;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HttpFileInfo {
    pub file_hash: String,
    pub file_name: String,
    pub file_size: u64,
    pub uploader_address: String,
    pub upload_time: u64,
    pub download_url: String,
}

/// Upload a file to HTTP server
#[tauri::command]
pub async fn upload_file_http(
    file_path: String,
    server_url: Option<String>,
) -> Result<HttpFileInfo, String> {
    let server_url = server_url.unwrap_or_else(|| "http://localhost:8080".to_string());

    info!("📤 Uploading file via HTTP: {}", file_path);

    // Read file
    let file_data = tokio::fs::read(&file_path)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let file_name = PathBuf::from(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Create multipart form
    let part = multipart::Part::bytes(file_data).file_name(file_name.clone());
    let form = multipart::Form::new().part("file", part);

    // Send request
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/upload", server_url))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Failed to upload file: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Upload failed with status: {}", response.status()));
    }

    let metadata: HttpFileInfo = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    info!("✅ File uploaded: {} (hash: {})", file_name, metadata.file_hash);

    Ok(metadata)
}

/// Download a file from HTTP server
#[tauri::command]
pub async fn download_file_http(
    file_hash: String,
    output_path: String,
    server_url: Option<String>,
) -> Result<String, String> {
    let server_url = server_url.unwrap_or_else(|| "http://localhost:8080".to_string());

    info!("📥 Downloading file via HTTP: {}", file_hash);

    // Send request
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/download/{}", server_url, file_hash))
        .send()
        .await
        .map_err(|e| format!("Failed to download file: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed with status: {}", response.status()));
    }

    // Save file
    let file_data = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    tokio::fs::write(&output_path, &file_data)
        .await
        .map_err(|e| format!("Failed to save file: {}", e))?;

    info!("✅ File downloaded: {}", output_path);

    Ok(output_path)
}

/// List all files on HTTP server
#[tauri::command]
pub async fn list_files_http(
    server_url: Option<String>,
) -> Result<Vec<HttpFileInfo>, String> {
    let server_url = server_url.unwrap_or_else(|| "http://localhost:8080".to_string());

    info!("📋 Listing files from HTTP server");

    // Send request
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/files", server_url))
        .send()
        .await
        .map_err(|e| format!("Failed to list files: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("List files failed with status: {}", response.status()));
    }

    let files: Vec<HttpFileInfo> = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    info!("✅ Found {} files", files.len());

    Ok(files)
}

/// Get file metadata from HTTP server
#[tauri::command]
pub async fn get_file_metadata_http(
    file_hash: String,
    server_url: Option<String>,
) -> Result<HttpFileInfo, String> {
    let server_url = server_url.unwrap_or_else(|| "http://localhost:8080".to_string());

    info!("🔍 Getting file metadata: {}", file_hash);

    // Send request
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/file/{}", server_url, file_hash))
        .send()
        .await
        .map_err(|e| format!("Failed to get metadata: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Get metadata failed with status: {}", response.status()));
    }

    let metadata: HttpFileInfo = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    info!("✅ Got metadata for: {}", metadata.file_name);

    Ok(metadata)
}

/// Check HTTP server health
#[tauri::command]
pub async fn check_http_server_health(
    server_url: Option<String>,
) -> Result<bool, String> {
    let server_url = server_url.unwrap_or_else(|| "http://localhost:8080".to_string());

    let client = reqwest::Client::new();
    match client
        .get(format!("{}/health", server_url))
        .send()
        .await
    {
        Ok(response) => Ok(response.status().is_success()),
        Err(_) => Ok(false),
    }
}
