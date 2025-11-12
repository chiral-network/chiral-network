//! FTP Demo - Demonstrates FTP download functionality
//!
//! This example shows how to use the FTP client to download files from a local FTP server.
//! Works on all platforms: Windows, Mac, Linux
//!
//! Setup:
//!   Linux/Mac: python3 -m pyftpdlib -p 21 -w -d /tmp/ftp_test
//!   Windows:   python -m pyftpdlib -p 21 -w -d C:\FTP_Test
//!
//! Usage:
//!   cargo run --example ftp_demo local              # Uses local FTP server (127.0.0.1:21)

use chiral_network::download_source::FtpSourceInfo;
use chiral_network::ftp_client::FtpClient;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for log output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Chiral Network FTP Demo ===\n");

    println!("Test: Download from Local FTP server (127.0.0.1:21)");
    println!("-----------------------------------------------");
    println!("Setup local FTP server first:");
    if cfg!(target_os = "windows") {
        println!("  python -m pyftpdlib -p 21 -w -d C:\\FTP_Test");
    } else {
        println!("  python3 -m pyftpdlib -p 21 -w -d /tmp/ftp_test");
    }
    println!();

    // Use local FTP server with active mode for better performance
    let ftp_info = FtpSourceInfo {
        url: "ftp://127.0.0.1:21/test_file.txt".to_string(),
        username: Some("anonymous".to_string()),
        encrypted_password: None,
        passive_mode: false,  // Active mode works best for local servers
        use_ftps: false,
        timeout_secs: Some(30),
    };

    let client = FtpClient::new(ftp_info);
    let output_path = Path::new("downloaded_test_file.txt");

    println!("Downloading to: {:?}", output_path);

    match client.download_file(output_path).await {
        Ok(bytes) => {
            println!("✓ Download successful!");
            println!("  Bytes downloaded: {}", bytes);
            println!("  File saved to: {:?}\n", output_path);

            // Verify file exists
            if output_path.exists() {
                let metadata = std::fs::metadata(output_path)?;
                println!("  File size on disk: {} bytes", metadata.len());
            }
        }
        Err(e) => {
            println!("✗ Download failed: {}", e);
            println!("  Note: Make sure FTP server is running!");
            return Err(e.into());
        }
    }

    println!("\n=== FTP Demo Complete ===");
    println!("\nSummary:");
    println!("  FTP client successfully demonstrated:");
    println!("  ✓ URL parsing (host, port, path)");
    println!("  ✓ FTP connection");
    println!("  ✓ Anonymous authentication");
    println!("  ✓ Binary mode file transfer");
    println!("  ✓ File download and save");
    println!("  ✓ Connection cleanup");

    Ok(())
}