//! HTTP Download Integration Test
//!
//! Tests the complete HTTP download flow:
//! 1. Locating data (finding seeder via DHT or direct URL)
//! 2. Handshake (fetching metadata from HTTP server)
//! 3. Download (downloading file chunks via Range requests)
//! 4. Payment (payment checkpoint integration - when implemented)
//!
//! This test validates the entire HTTP download pipeline end-to-end.

use chiral_network::http_download::{HttpDownloadClient, HttpDownloadProgress, DownloadStatus, HttpFileMetadata};
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

mod mock_http_server;
use mock_http_server::MockHttpServer;

/// Test helper: Create a test file with known content
fn create_test_file(size_mb: u64) -> (Vec<u8>, String) {
    let size_bytes = size_mb * 1024 * 1024;
    let mut data = Vec::with_capacity(size_bytes as usize);
    
    // Fill with pattern for verification
    for i in 0..size_bytes {
        data.push((i % 256) as u8);
    }
    
    // Simple hash for file identifier
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    let file_hash = format!("{:x}", hasher.finish());
    
    (data, file_hash)
}

/// Test Phase 1: Locating Data
/// 
/// In a real scenario, this would involve:
/// - Querying DHT for seeders
/// - Getting seeder URLs/IPs
/// - Selecting best seeder
/// 
/// For HTTP tests, we use a direct URL from mock server.
async fn test_locating_data(server_url: &str, file_hash: &str) -> Result<String, String> {
    // In real implementation, this would query DHT:
    // let seeders = dht.get_seeders_for_file(file_hash).await?;
    // let seeder_url = select_best_seeder(seeders)?;
    
    // For test, we use the mock server URL directly
    tracing::info!("[LOC] Locating data: Found seeder at {}", server_url);
    Ok(server_url.to_string())
}

/// Test Phase 2: Handshake
/// 
/// Fetches file metadata from the HTTP server to:
/// - Verify file exists
/// - Get file size
/// - Check encryption status
/// - Establish connection
async fn test_handshake(
    _client: &HttpDownloadClient,
    seeder_url: &str,
    file_hash: &str,
) -> Result<HttpFileMetadata, String> {
    tracing::info!("[HANDSHAKE] Handshake: Fetching metadata from {}", seeder_url);
    
    // Fetch metadata from HTTP server
    let url = format!("{}/files/{}/metadata", seeder_url, file_hash);
    let response = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to fetch metadata: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!("Metadata request failed: {}", response.status()));
    }
    
    let metadata: HttpFileMetadata = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse metadata: {}", e))?;
    
    tracing::info!(
        "[OK] Handshake complete: {} ({} bytes, encrypted: {})",
        metadata.name,
        metadata.size,
        metadata.encrypted
    );
    
    Ok(metadata)
}

/// Test Phase 3: Download
/// 
/// Downloads file using Range requests:
/// - Splits file into chunks
/// - Downloads chunks in parallel
/// - Tracks progress
/// - Verifies downloaded data
async fn test_download(
    client: &HttpDownloadClient,
    seeder_url: &str,
    file_hash: &str,
    output_path: &PathBuf,
    expected_data: &[u8],
) -> Result<(), String> {
    tracing::info!("[DOWN]  Download: Starting download from {}", seeder_url);
    
    // Create progress channel
    let (progress_tx, mut progress_rx) = mpsc::channel::<HttpDownloadProgress>(100);
    
    // Start download in background
    let seeder_url_clone = seeder_url.to_string();
    let file_hash_clone = file_hash.to_string();
    let output_path_clone = output_path.clone();
    let progress_tx_clone = progress_tx.clone();
    
    // Create a new client for the spawned task
    let client_for_task = HttpDownloadClient::new();
    
    let download_handle = tokio::spawn(async move {
        client_for_task
            .download_file(
                &seeder_url_clone,
                &file_hash_clone,
                &output_path_clone,
                Some(progress_tx_clone),
            )
            .await
    });
    
    // Monitor progress
    let mut last_progress: Option<HttpDownloadProgress> = None;
    let progress_monitor = tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            tracing::debug!(
                "Progress: {}/{} chunks, {}/{} bytes, status: {:?}",
                progress.chunks_downloaded,
                progress.chunks_total,
                progress.bytes_downloaded,
                progress.bytes_total,
                progress.status
            );
            last_progress = Some(progress);
        }
    });
    
    // Wait for download to complete (with timeout)
    timeout(Duration::from_secs(60), download_handle)
        .await
        .map_err(|_| "Download timeout".to_string())?
        .map_err(|e| format!("Download task failed: {}", e))??;
    
    drop(progress_tx);
    let _ = progress_monitor.await;
    
    // Verify downloaded file
    let downloaded_data = tokio::fs::read(output_path)
        .await
        .map_err(|e| format!("Failed to read downloaded file: {}", e))?;
    
    if downloaded_data.len() != expected_data.len() {
        return Err(format!(
            "File size mismatch: expected {}, got {}",
            expected_data.len(),
            downloaded_data.len()
        ));
    }
    
    if downloaded_data != expected_data {
        // Check first few bytes for debugging
        let mismatch_pos = downloaded_data
            .iter()
            .zip(expected_data.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        return Err(format!(
            "File content mismatch at position {}",
            mismatch_pos
        ));
    }
    
    tracing::info!("[OK] Download complete: File verified successfully");
    Ok(())
}

/// Test Phase 4: Payment (when implemented)
/// 
/// Tests payment checkpoint integration:
/// - Initializes payment checkpoint session
/// - Updates progress and checks for checkpoints
/// - Processes payment
/// - Resumes download after payment
/// 
/// Note: Currently HTTP downloads don't have payment checkpoint integration,
/// but this test structure is ready for when it's implemented.
async fn test_payment_checkpoint(
    _session_id: &str,
    _file_hash: &str,
    _bytes_transferred: u64,
) -> Result<(), String> {
    // TODO: Implement when payment checkpoint is integrated into HTTP downloads
    // This would involve:
    // 1. Initialize checkpoint session at download start
    // 2. Update progress as chunks are downloaded
    // 3. Check if checkpoint reached (e.g., 10 MB)
    // 4. Pause download and wait for payment
    // 5. Process payment
    // 6. Resume download
    
    tracing::info!("[PAY] Payment checkpoint: Not yet implemented for HTTP downloads");
    Ok(())
}

/// Complete end-to-end HTTP download test
#[tokio::test]
async fn test_http_download_complete_flow() {
    let _ = tracing_subscriber::fmt::try_init();
    
    // Setup: Create test file and mock server
    let (test_data, file_hash) = create_test_file(1); // 1 MB test file
    let file_name = "test_file.bin".to_string();
    
    let mut server = MockHttpServer::new();
    server.add_file(
        file_hash.clone(),
        file_name.clone(),
        test_data.clone(),
        false, // Not encrypted
    );
    
    let (server_url, _server_handle) = server.start().await.unwrap();
    tracing::info!("[START] Mock HTTP server started at {}", server_url);
    
    // Phase 1: Locating Data
    let seeder_url = test_locating_data(&server_url, &file_hash)
        .await
        .expect("Failed to locate data");
    
    // Phase 2: Handshake
    let client = HttpDownloadClient::new();
    let metadata = test_handshake(&client, &seeder_url, &file_hash)
        .await
        .expect("Handshake failed");
    
    assert_eq!(metadata.name, file_name);
    assert_eq!(metadata.size, test_data.len() as u64);
    assert_eq!(metadata.encrypted, false);
    
    // Phase 3: Download
    let output_path = std::env::temp_dir().join(format!("http_test_{}.bin", file_hash));
    
    test_download(&client, &seeder_url, &file_hash, &output_path, &test_data)
        .await
        .expect("Download failed");
    
    // Cleanup
    let _ = tokio::fs::remove_file(&output_path).await;
    
    tracing::info!("[OK] Complete HTTP download flow test passed");
}

/// Test download with retry logic (network errors)
#[tokio::test]
async fn test_http_download_with_retries() {
    let _ = tracing_subscriber::fmt::try_init();
    
    // Create a smaller test file for faster retry testing
    let (test_data, file_hash) = create_test_file(0); // 256 KB (default chunk size)
    let file_name = "retry_test.bin".to_string();
    
    let mut server = MockHttpServer::new();
    server.add_file(
        file_hash.clone(),
        file_name.clone(),
        test_data.clone(),
        false,
    );
    
    let (server_url, _server_handle) = server.start().await.unwrap();
    
    let client = HttpDownloadClient::new();
    let output_path = std::env::temp_dir().join(format!("http_retry_test_{}.bin", file_hash));
    
    // Download should succeed even with potential transient errors
    // (retry logic is tested in http_download.rs unit tests)
    let result = client
        .download_file(
            &server_url,
            &file_hash,
            &output_path,
            None, // No progress tracking for this test
        )
        .await;
    
    assert!(result.is_ok(), "Download should succeed");
    
    // Verify file
    let downloaded_data = tokio::fs::read(&output_path)
        .await
        .expect("Failed to read downloaded file");
    assert_eq!(downloaded_data, test_data);
    
    // Cleanup
    let _ = tokio::fs::remove_file(&output_path).await;
}

/// Test metadata fetching (handshake phase)
#[tokio::test]
async fn test_http_handshake_metadata() {
    let _ = tracing_subscriber::fmt::try_init();
    
    let (test_data, file_hash) = create_test_file(0);
    let file_name = "metadata_test.bin".to_string();
    
    let mut server = MockHttpServer::new();
    server.add_file(
        file_hash.clone(),
        file_name.clone(),
        test_data,
        false,
    );
    
    let (server_url, _server_handle) = server.start().await.unwrap();
    
    let client = HttpDownloadClient::new();
    let metadata = test_handshake(&client, &server_url, &file_hash)
        .await
        .expect("Handshake failed");
    
    assert_eq!(metadata.hash, file_hash);
    assert_eq!(metadata.name, file_name);
    assert_eq!(metadata.encrypted, false);
}

/// Test Range request handling
#[tokio::test]
async fn test_http_range_requests() {
    let _ = tracing_subscriber::fmt::try_init();
    
    let (test_data, file_hash) = create_test_file(1); // 1 MB
    let file_name = "range_test.bin".to_string();
    
    let mut server = MockHttpServer::new();
    server.add_file(
        file_hash.clone(),
        file_name,
        test_data.clone(),
        false,
    );
    
    let (server_url, _server_handle) = server.start().await.unwrap();
    
    // Test downloading specific ranges
    let client = reqwest::Client::new();
    
    // Test first 256 KB
    let response = client
        .get(&format!("{}/files/{}", server_url, file_hash))
        .header("Range", "bytes=0-262143")
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), reqwest::StatusCode::PARTIAL_CONTENT);
    let chunk = response.bytes().await.unwrap();
    assert_eq!(chunk.len(), 262144);
    assert_eq!(&chunk[..], &test_data[0..262144]);
    
    // Test middle range
    let response = client
        .get(&format!("{}/files/{}", server_url, file_hash))
        .header("Range", "bytes=524288-786431")
        .send()
        .await
        .unwrap();
    
    assert_eq!(response.status(), reqwest::StatusCode::PARTIAL_CONTENT);
    let chunk = response.bytes().await.unwrap();
    assert_eq!(chunk.len(), 262144);
    assert_eq!(&chunk[..], &test_data[524288..786432]);
}

/// Test download progress tracking
#[tokio::test]
async fn test_http_download_progress() {
    let _ = tracing_subscriber::fmt::try_init();
    
    let (test_data, file_hash) = create_test_file(1); // 1 MB
    let file_name = "progress_test.bin".to_string();
    
    let mut server = MockHttpServer::new();
    server.add_file(
        file_hash.clone(),
        file_name,
        test_data.clone(),
        false,
    );
    
    let (server_url, _server_handle) = server.start().await.unwrap();
    
    let client = HttpDownloadClient::new();
    let output_path = std::env::temp_dir().join(format!("http_progress_test_{}.bin", file_hash));
    
    let (progress_tx, mut progress_rx) = mpsc::channel::<HttpDownloadProgress>(100);
    
    let download_handle = tokio::spawn({
        let server_url = server_url.clone();
        let file_hash = file_hash.clone();
        let output_path = output_path.clone();
        let client_for_task = HttpDownloadClient::new();
        async move {
            client_for_task
                .download_file(&server_url, &file_hash, &output_path, Some(progress_tx))
                .await
        }
    });
    
    // Collect progress updates
    use std::sync::{Arc, Mutex};
    let progress_updates = Arc::new(Mutex::new(Vec::new()));
    let progress_updates_clone = progress_updates.clone();
    let progress_handle = tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            progress_updates_clone.lock().unwrap().push(progress.clone());
            tracing::debug!(
                "Progress update: {}/{} chunks, {}/{} bytes",
                progress.chunks_downloaded,
                progress.chunks_total,
                progress.bytes_downloaded,
                progress.bytes_total
            );
        }
    });
    
    let _ = download_handle.await.unwrap().unwrap();
    let _ = progress_handle.await;
    
    // Verify progress updates
    let progress_updates_guard = progress_updates.lock().unwrap();
    assert!(!progress_updates_guard.is_empty(), "Should receive progress updates");
    
    // Check that we got FetchingMetadata status
    assert!(progress_updates_guard
        .iter()
        .any(|p| p.status == DownloadStatus::FetchingMetadata));
    
    // Check that we got Downloading status
    assert!(progress_updates_guard
        .iter()
        .any(|p| p.status == DownloadStatus::Downloading));
    
    // Check that we got Completed status
    assert!(progress_updates_guard
        .iter()
        .any(|p| p.status == DownloadStatus::Completed));
    
    // Verify final progress
    let final_progress = progress_updates_guard.last().unwrap();
    assert_eq!(final_progress.chunks_downloaded, final_progress.chunks_total);
    assert_eq!(final_progress.bytes_downloaded, final_progress.bytes_total);
    assert_eq!(final_progress.bytes_total, test_data.len() as u64);
    
    // Cleanup
    let _ = tokio::fs::remove_file(&output_path).await;
}

/// Test payment checkpoint integration (placeholder for future implementation)
#[tokio::test]
#[ignore] // Ignore until payment checkpoint is implemented for HTTP
async fn test_http_download_with_payment_checkpoint() {
    let _ = tracing_subscriber::fmt::try_init();
    
    // This test will be implemented when payment checkpoint is integrated
    // into HTTP downloads. The structure is:
    //
    // 1. Initialize payment checkpoint session
    // 2. Start download
    // 3. Download reaches checkpoint (e.g., 10 MB)
    // 4. Server returns 402 Payment Required
    // 5. Client processes payment
    // 6. Download resumes
    // 7. Download completes
    
    let result = test_payment_checkpoint("test_session", "test_hash", 10 * 1024 * 1024).await;
    assert!(result.is_ok());
    
    tracing::info!("[PAY] Payment checkpoint test (placeholder) - will be implemented");
}

