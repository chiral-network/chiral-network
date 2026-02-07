use chiral_network::ftp_downloader::{FtpDownloadConfig, FtpDownloader, FtpCredentials};
use url::Url;

/// Test FTP downloader creation
#[tokio::test]
async fn test_create_default_downloader() {
    let downloader = FtpDownloader::new();
    assert_eq!(downloader.config().timeout_secs, 30);
    assert_eq!(downloader.config().max_retries, 3);
    assert!(downloader.config().passive_mode);
}

/// Test custom configuration
#[tokio::test]
async fn test_create_custom_downloader() {
    let config = FtpDownloadConfig {
        timeout_secs: 60,
        max_retries: 5,
        passive_mode: false,
        connection_pool_size: 10,
    };

    let downloader = FtpDownloader::with_config(config.clone());
    assert_eq!(downloader.config().timeout_secs, 60);
    assert_eq!(downloader.config().max_retries, 5);
    assert!(!downloader.config().passive_mode);
    assert_eq!(downloader.config().connection_pool_size, 10);
}

/// Test anonymous credentials
#[tokio::test]
async fn test_anonymous_credentials() {
    let creds = FtpCredentials::anonymous();
    assert_eq!(creds.username, "anonymous");
    assert!(creds.password.contains("anonymous"));
}

/// Test custom credentials
#[tokio::test]
async fn test_custom_credentials() {
    let creds = FtpCredentials::new("testuser".to_string(), "testpass".to_string());
    assert_eq!(creds.username, "testuser");
    assert_eq!(creds.password, "testpass");
}

/// Test URL parsing
#[tokio::test]
async fn test_url_parsing() {
    // URL with explicit port
    let url_with_port = Url::parse("ftp://ftp.example.com:2121/path/to/file.bin").unwrap();
    assert_eq!(url_with_port.scheme(), "ftp");
    assert_eq!(url_with_port.host_str(), Some("ftp.example.com"));
    assert_eq!(url_with_port.port(), Some(2121));
    assert_eq!(url_with_port.path(), "/path/to/file.bin");

    // URL without explicit port (default port is not returned by url crate)
    let url_default = Url::parse("ftp://ftp.example.com/path/to/file.bin").unwrap();
    assert_eq!(url_default.scheme(), "ftp");
    assert_eq!(url_default.host_str(), Some("ftp.example.com"));
    assert_eq!(url_default.port(), None); // Default FTP port (21) is not returned
    assert_eq!(url_default.path(), "/path/to/file.bin");
}

// ========================================================================
// INTEGRATION TESTS (require internet connection)
// Run with: cargo test -- --ignored
// ========================================================================

#[tokio::test]
#[ignore]
async fn test_connect_to_gnu_ftp_anonymous() {
    let downloader = FtpDownloader::new();
    let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

    let result = downloader.connect_and_login(&url, None).await;

    match result {
        Ok(mut stream) => {
            println!("[OK] Successfully connected to ftp.gnu.org");

            // Test getting working directory
            match downloader.get_working_directory(&mut stream).await {
                Ok(pwd) => println!("[OK] Working directory: {}", pwd),
                Err(e) => println!("[X] PWD failed: {}", e),
            }

            // Disconnect
            match downloader.disconnect(&mut stream).await {
                Ok(_) => println!("[OK] Disconnected successfully"),
                Err(e) => println!("[X] Disconnect failed: {}", e),
            }
        }
        Err(e) => {
            println!("[X] Connection failed: {}", e);
            panic!("Failed to connect to ftp.gnu.org");
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_passive_mode_connection() {
    let config = FtpDownloadConfig {
        passive_mode: true,
        ..Default::default()
    };

    let downloader = FtpDownloader::with_config(config);
    let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

    match downloader.connect_and_login(&url, None).await {
        Ok(mut stream) => {
            println!("[OK] Passive mode connection successful");
            let _ = downloader.disconnect(&mut stream).await;
        }
        Err(e) => {
            println!("[X] Passive mode failed: {}", e);
            panic!("Passive mode connection failed");
        }
    }
}

#[tokio::test]
#[ignore]
async fn test_get_file_size() {
    let downloader = FtpDownloader::new();
    let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

    let mut stream = match downloader.connect_and_login(&url, None).await {
        Ok(s) => s,
        Err(e) => {
            println!("Test skipped (connection failed): {}", e);
            return;
        }
    };

    // Try to get size of README file (common on GNU FTP)
    match downloader.get_file_size(&mut stream, "/README").await {
        Ok(size) => {
            println!("[OK] File size: {} bytes", size);
            assert!(size > 0, "File size should be positive");
        }
        Err(e) => {
            println!("[X] SIZE command failed: {}", e);
        }
    }

    let _ = downloader.disconnect(&mut stream).await;
}

#[tokio::test]
#[ignore]
async fn test_supports_resume_command() {
    let downloader = FtpDownloader::new();
    let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

    let mut stream = match downloader.connect_and_login(&url, None).await {
        Ok(s) => s,
        Err(e) => {
            println!("Test skipped: {}", e);
            return;
        }
    };

    match downloader.supports_resume(&mut stream).await {
        Ok(supported) => {
            println!("[OK] REST command support detected: {}", supported);
        }
        Err(e) => {
            println!("[X] Error testing REST support: {}", e);
        }
    }

    let _ = downloader.disconnect(&mut stream).await;
}

#[tokio::test]
#[ignore]
async fn test_download_byte_range() {
    let downloader = FtpDownloader::new();
    let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

    let mut stream = match downloader.connect_and_login(&url, None).await {
        Ok(s) => s,
        Err(e) => {
            println!("Test skipped: {}", e);
            return;
        }
    };

    // Download first 100 bytes of README
    match downloader
        .download_range(&mut stream, "/README", 0, 100)
        .await
    {
        Ok(data) => {
            println!("[OK] Downloaded {} bytes (requested 100)", data.len());
            assert!(data.len() <= 100, "Should not exceed requested size");
            assert!(!data.is_empty(), "Should have downloaded some data");

            // Print first few bytes as ASCII
            let preview = String::from_utf8_lossy(&data[..std::cmp::min(50, data.len())]);
            println!("Preview: {}", preview);
        }
        Err(e) => {
            println!("[X] Range download failed: {}", e);
            panic!("Failed to download byte range");
        }
    }

    let _ = downloader.disconnect(&mut stream).await;
}

#[tokio::test]
#[ignore]
async fn test_download_multiple_ranges() {
    let downloader = FtpDownloader::new();
    let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

    let mut stream = match downloader.connect_and_login(&url, None).await {
        Ok(s) => s,
        Err(e) => {
            println!("Test skipped: {}", e);
            return;
        }
    };

    // Download three consecutive ranges
    let ranges = vec![
        (0, 50),    // First 50 bytes
        (50, 50),   // Next 50 bytes
        (100, 50),  // Next 50 bytes
    ];

    let mut chunks = Vec::new();

    for (start, size) in ranges {
        match downloader
            .download_range(&mut stream, "/README", start, size)
            .await
        {
            Ok(data) => {
                println!("[OK] Downloaded range {}-{}: {} bytes", start, start + size, data.len());
                chunks.push(data);
            }
            Err(e) => {
                println!("[X] Range {}-{} failed: {}", start, start + size, e);
                panic!("Multi-range download failed");
            }
        }
    }

    // Reassemble
    let full_data: Vec<u8> = chunks.into_iter().flatten().collect();
    println!("[OK] Reassembled {} bytes from 3 ranges", full_data.len());
    assert_eq!(full_data.len(), 150, "Should have 150 bytes total");

    let _ = downloader.disconnect(&mut stream).await;
}

#[tokio::test]
#[ignore]
async fn test_list_directory() {
    let downloader = FtpDownloader::new();
    let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

    let mut stream = match downloader.connect_and_login(&url, None).await {
        Ok(s) => s,
        Err(e) => {
            println!("Test skipped: {}", e);
            return;
        }
    };

    match downloader.list_directory(&mut stream, "/").await {
        Ok(files) => {
            println!("[OK] Found {} entries in root directory", files.len());
            for file in files.iter().take(10) {
                println!("  - {}", file);
            }
            assert!(!files.is_empty(), "Directory should have entries");
        }
        Err(e) => {
            println!("[X] Directory listing failed: {}", e);
        }
    }

    let _ = downloader.disconnect(&mut stream).await;
}

#[tokio::test]
#[ignore]
async fn test_download_offset_from_middle() {
    let downloader = FtpDownloader::new();
    let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

    let mut stream = match downloader.connect_and_login(&url, None).await {
        Ok(s) => s,
        Err(e) => {
            println!("Test skipped: {}", e);
            return;
        }
    };

    // Download from byte 500 onwards (50 bytes)
    match downloader
        .download_range(&mut stream, "/README", 500, 50)
        .await
    {
        Ok(data) => {
            println!("[OK] Downloaded {} bytes from offset 500", data.len());
            assert!(data.len() <= 50);
        }
        Err(e) => {
            println!("[X] Offset download failed: {}", e);
        }
    }

    let _ = downloader.disconnect(&mut stream).await;
}

#[tokio::test]
#[ignore]
async fn test_concurrent_connections() {
    // Test multiple simultaneous FTP connections
    let downloaders: Vec<_> = (0..3)
        .map(|_| FtpDownloader::new())
        .collect();

    let mut handles = Vec::new();

    for (i, downloader) in downloaders.into_iter().enumerate() {
        let handle = tokio::spawn(async move {
            let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

            let mut stream = match downloader.connect_and_login(&url, None).await {
                Ok(s) => s,
                Err(e) => {
                    println!("Connection {} failed: {}", i, e);
                    return Err(e);
                }
            };

            let result = downloader
                .download_range(&mut stream, "/README", i as u64 * 100, 50)
                .await;

            let _ = downloader.disconnect(&mut stream).await;

            match result {
                Ok(data) => {
                    println!("[OK] Connection {} downloaded {} bytes", i, data.len());
                    Ok(data)
                }
                Err(e) => {
                    println!("[X] Connection {} download failed: {}", i, e);
                    Err(e)
                }
            }
        });

        handles.push(handle);
    }

    // Wait for all connections
    let mut success_count = 0;
    for (i, handle) in handles.into_iter().enumerate() {
        match handle.await {
            Ok(Ok(_)) => success_count += 1,
            Ok(Err(e)) => println!("Task {} error: {}", i, e),
            Err(e) => println!("Task {} join error: {}", i, e),
        }
    }

    println!("[OK] {} / 3 concurrent connections succeeded", success_count);
    assert!(success_count >= 2, "At least 2 connections should succeed");
}

#[tokio::test]
#[ignore]
async fn test_retry_on_timeout() {
    let config = FtpDownloadConfig {
        timeout_secs: 2,  // Very short timeout
        max_retries: 3,
        passive_mode: true,
        connection_pool_size: 5,
    };

    let downloader = FtpDownloader::with_config(config);
    let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

    let mut stream = match downloader.connect_and_login(&url, None).await {
        Ok(s) => s,
        Err(e) => {
            println!("Test skipped: {}", e);
            return;
        }
    };

    // Download with retries enabled
    let result = downloader
        .download_range(&mut stream, "/README", 0, 100)
        .await;

    match result {
        Ok(data) => {
            println!("[OK] Download succeeded (possibly with retries): {} bytes", data.len());
        }
        Err(e) => {
            println!("[X] Download failed after retries: {}", e);
        }
    }

    let _ = downloader.disconnect(&mut stream).await;
}

#[tokio::test]
#[ignore]
async fn test_large_file_chunk() {
    let downloader = FtpDownloader::new();
    let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

    let mut stream = match downloader.connect_and_login(&url, None).await {
        Ok(s) => s,
        Err(e) => {
            println!("Test skipped: {}", e);
            return;
        }
    };

    // Download 256KB (typical chunk size)
    match downloader
        .download_range(&mut stream, "/README", 0, 256 * 1024)
        .await
    {
        Ok(data) => {
            println!("[OK] Downloaded {} bytes (256KB chunk)", data.len());
        }
        Err(e) => {
            // File might be smaller than 256KB
            println!("Large chunk download: {}", e);
        }
    }

    let _ = downloader.disconnect(&mut stream).await;
}

#[tokio::test]
#[ignore]
async fn test_change_directory() {
    let downloader = FtpDownloader::new();
    let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

    let mut stream = match downloader.connect_and_login(&url, None).await {
        Ok(s) => s,
        Err(e) => {
            println!("Test skipped: {}", e);
            return;
        }
    };

    // Get initial directory
    let initial_dir = downloader.get_working_directory(&mut stream).await.ok();
    println!("Initial directory: {:?}", initial_dir);

    // Try to change to a common directory
    match downloader.change_directory(&mut stream, "/gnu").await {
        Ok(_) => {
            println!("[OK] Changed directory to /gnu");

            // Get new directory
            if let Ok(new_dir) = downloader.get_working_directory(&mut stream).await {
                println!("New directory: {}", new_dir);
            }
        }
        Err(e) => {
            println!("Directory change failed (expected if /gnu doesn't exist): {}", e);
        }
    }

    let _ = downloader.disconnect(&mut stream).await;
}

#[tokio::test]
#[ignore]
async fn test_error_handling_invalid_file() {
    let downloader = FtpDownloader::new();
    let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

    let mut stream = match downloader.connect_and_login(&url, None).await {
        Ok(s) => s,
        Err(e) => {
            println!("Test skipped: {}", e);
            return;
        }
    };

    // Try to download non-existent file
    let result = downloader
        .download_range(&mut stream, "/this_file_does_not_exist_12345.txt", 0, 100)
        .await;

    assert!(result.is_err(), "Should fail for non-existent file");

    if let Err(e) = result {
        println!("[OK] Expected error for missing file: {}", e);
        assert!(
            e.contains("RETR") || e.contains("550") || e.contains("not found"),
            "Error should indicate file not found"
        );
    }

    let _ = downloader.disconnect(&mut stream).await;
}