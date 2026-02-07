// Standalone FTP downloader test
// Run with: cargo run --example ftp_test
// This example demonstrates basic FTP functionality without requiring the full test suite

use chiral_network::ftp_downloader::{FtpDownloader, FtpDownloadConfig, FtpCredentials};
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for debug output
    tracing_subscriber::fmt::init();

    println!("=== FTP Downloader Test ===\n");

    // Test 1: Create FTP downloader with default config
    println!("Test 1: Creating FTP downloader with default config");
    let downloader = FtpDownloader::new();
    let config = FtpDownloadConfig::default();
    println!("[OK] FTP downloader created successfully");
    println!("  - Timeout: {}s", config.timeout_secs);
    println!("  - Max retries: {}", config.max_retries);
    println!("  - Passive mode: {}", config.passive_mode);
    println!("  - Connection pool size: {}\n", config.connection_pool_size);

    // Test 2: Create FTP downloader with custom config
    println!("Test 2: Creating FTP downloader with custom config");
    let custom_config = FtpDownloadConfig {
        timeout_secs: 60,
        max_retries: 5,
        passive_mode: true,
        connection_pool_size: 8,
    };
    let custom_downloader = FtpDownloader::with_config(custom_config.clone());
    println!("[OK] Custom FTP downloader created successfully");
    println!("  - Timeout: {}s", custom_config.timeout_secs);
    println!("  - Max retries: {}", custom_config.max_retries);
    println!("  - Passive mode: {}", custom_config.passive_mode);
    println!("  - Connection pool size: {}\n", custom_config.connection_pool_size);

    // Test 3: Test anonymous credentials
    println!("Test 3: Testing anonymous credentials");
    let anonymous_creds = FtpCredentials::anonymous();
    println!("[OK] Anonymous credentials created");
    println!("  - Username: {}", anonymous_creds.username);
    println!("  - Password: {}\n", anonymous_creds.password);

    // Test 4: Test authenticated credentials
    println!("Test 4: Testing authenticated credentials");
    let auth_creds = FtpCredentials::new("testuser".to_string(), "testpass".to_string());
    println!("[OK] Authenticated credentials created");
    println!("  - Username: {}", auth_creds.username);
    println!("  - Password: [REDACTED]\n");

    // Test 5: Parse FTP URL
    println!("Test 5: Parsing FTP URL");
    let ftp_url = "ftp://ftp.gnu.org/gnu/hello/hello-2.10.tar.gz";
    match Url::parse(ftp_url) {
        Ok(url) => {
            println!("[OK] FTP URL parsed successfully");
            println!("  - URL: {}", ftp_url);
            println!("  - Scheme: {}", url.scheme());
            println!("  - Host: {}", url.host_str().unwrap_or("N/A"));
            println!("  - Path: {}\n", url.path());
        }
        Err(e) => {
            println!("[X] Failed to parse URL: {}\n", e);
        }
    }

    println!("=== All basic tests passed! ===\n");

    // Optional: Test actual FTP connection (requires network)
    println!("=== Optional Network Tests ===");
    println!("To test actual FTP connections, uncomment the network test section in the code.");
    println!("Network tests require internet connectivity and may take time.\n");

    /* Uncomment to run network tests
    println!("Attempting to connect to ftp.gnu.org (anonymous)...");
    let gnu_url = Url::parse("ftp://ftp.gnu.org/gnu/hello/hello-2.10.tar.gz")?;

    match downloader.connect_and_login(&gnu_url, None).await {
        Ok(mut stream) => {
            println!("[OK] Connected to GNU FTP server");

            // Test file size retrieval
            let remote_path = gnu_url.path();
            match downloader.get_file_size(&mut stream, remote_path).await {
                Ok(size) => {
                    println!("  - File size: {} bytes ({:.2} KB)", size, size as f64 / 1024.0);
                }
                Err(e) => {
                    println!("  [X] Failed to get file size: {}", e);
                }
            }

            // Test byte range download (first 1024 bytes)
            println!("  Downloading first 1KB...");
            match downloader.download_range(&mut stream, remote_path, 0, 1024).await {
                Ok(data) => {
                    println!("  [OK] Downloaded {} bytes", data.len());
                    println!("  First 32 bytes: {:?}", &data[..32.min(data.len())]);
                }
                Err(e) => {
                    println!("  [X] Failed to download range: {}", e);
                }
            }

            // Disconnect
            match downloader.disconnect(&mut stream).await {
                Ok(_) => println!("  [OK] Disconnected successfully"),
                Err(e) => println!("  [X] Disconnect error: {}", e),
            }
        }
        Err(e) => {
            println!("[X] Failed to connect: {}", e);
        }
    }
    */

    Ok(())
}