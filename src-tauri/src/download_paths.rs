use std::path::{Path, PathBuf};

/// Expand tilde (~) in path to home directory.
///
/// Note: On Windows this still works for "~" / "~/" by resolving the user's home directory.
fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") || path == "~" {
        if let Some(base_dirs) = directories::BaseDirs::new() {
            return base_dirs
                .home_dir()
                .join(path.strip_prefix("~/").unwrap_or(""));
        }
    }
    PathBuf::from(path)
}

fn load_storage_path_from_settings(app_handle: &tauri::AppHandle) -> Option<String> {
    use tauri::Manager;

    let app_data_dir = app_handle.path().app_data_dir().ok()?;
    let settings_file = app_data_dir.join("settings.json");
    if !settings_file.exists() {
        return None;
    }

    let contents = std::fs::read_to_string(&settings_file).ok()?;
    let json = serde_json::from_str::<serde_json::Value>(&contents).ok()?;
    json.get("storagePath")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
}

/// Resolve the download directory (string path).
///
/// This mirrors the `get_download_directory` Tauri command logic in `main.rs`,
/// but lives in the library crate so other modules (e.g. WebRTC downloads) can reuse it.
pub fn get_download_directory(app_handle: &tauri::AppHandle) -> Result<String, String> {
    if let Some(storage_path) = load_storage_path_from_settings(app_handle) {
        let expanded = expand_tilde(&storage_path);
        return expanded
            .to_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Failed to convert path to string".to_string());
    }

    // Cross-platform default
    let default_path = "~/Downloads/Chiral-Network-Storage";
    let expanded = expand_tilde(default_path);
    expanded
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Failed to convert path to string".to_string())
}

/// Ensure a directory exists.
///
/// If `path` looks like a file path (has an extension), create its parent directory.
pub async fn ensure_directory_exists(path: &str) -> Result<(), String> {
    let path_obj = Path::new(path);

    let dir_to_create = if path_obj.extension().is_some() {
        path_obj.parent().ok_or_else(|| "Invalid path".to_string())?
    } else {
        path_obj
    };

    tokio::fs::create_dir_all(dir_to_create)
        .await
        .map_err(|e| format!("Failed to create directory: {}", e))
}


