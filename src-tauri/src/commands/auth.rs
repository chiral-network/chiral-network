use crate::{AppState, ProxyAuthToken};
use rand::Rng;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::State;
use tracing::info;

/// In-memory token storage (in production, use a proper database)
type TokenStore = Mutex<HashMap<String, crate::ProxyAuthToken>>;

#[tauri::command]
pub(crate) async fn generate_proxy_auth_token(
    _app: tauri::AppHandle,
    state: State<'_, AppState>,
    proxy_address: String,
    expiry_hours: u32,
) -> Result<serde_json::Value, String> {
    let mut store = state.proxy_auth_tokens.lock().await;

    // Generate a cryptographically secure token
    let token = generate_secure_token();

    // Calculate expiry time
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let expires_at = now + (expiry_hours as u64 * 3600);

    // Store the token
    let token_data = ProxyAuthToken {
        token: token.clone(),
        proxy_address: proxy_address.clone(),
        expires_at,
        created_at: now,
    };

    store.insert(token.clone(), token_data);

    // Clean up expired tokens
    cleanup_expired_tokens(&mut store);

    info!("Generated proxy auth token for {} (expires in {} hours)", proxy_address, expiry_hours);

    Ok(serde_json::json!({
        "token": token,
        "expires_at": expires_at
    }))
}

#[tauri::command]
pub(crate) async fn validate_proxy_auth_token(
    _app: tauri::AppHandle,
    state: State<'_, AppState>,
    proxy_address: String,
    token: String,
) -> Result<bool, String> {
    let mut store = state.proxy_auth_tokens.lock().await;

    // Clean up expired tokens first
    cleanup_expired_tokens(&mut store);

    // Check if token exists and matches the proxy address
    if let Some(token_data) = store.get(&token) {
        if token_data.proxy_address == proxy_address && !is_token_expired(token_data.expires_at) {
            return Ok(true);
        }
    }

    Ok(false)
}

#[tauri::command]
pub(crate) async fn revoke_proxy_auth_token(
    _app: tauri::AppHandle,
    state: State<'_, AppState>,
    token: String,
) -> Result<(), String> {
    let mut store = state.proxy_auth_tokens.lock().await;
    store.remove(&token);
    info!("Revoked proxy auth token");
    Ok(())
}

#[tauri::command]
pub(crate) async fn cleanup_expired_proxy_auth_tokens(
    _app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    let mut store = state.proxy_auth_tokens.lock().await;
    let initial_count = store.len();
    cleanup_expired_tokens(&mut store);
    let final_count = store.len();
    let removed = initial_count - final_count;
    info!("Cleaned up {} expired proxy auth tokens", removed);
    Ok(removed as u32)
}

/// Generate a cryptographically secure random token
fn generate_secure_token() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    hex::encode(bytes)
}

/// Check if a token is expired
fn is_token_expired(expires_at: u64) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    now > expires_at
}

/// Clean up expired tokens from the store
fn cleanup_expired_tokens(store: &mut HashMap<String, ProxyAuthToken>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    store.retain(|_, token_data| !is_token_expired(token_data.expires_at));
}