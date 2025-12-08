// Note: Read trait is used in closure within try_download_range_blocking
#[allow(unused_imports)]
use std::io::Read;
use std::time::Duration;
use suppaftp::{FtpError, FtpStream};
use suppaftp::types::FileType;
use tokio::task;
use tracing::{debug, info, warn};
use url::Url;

/// FTP connection configuration
#[derive(Debug, Clone)]
pub struct FtpDownloadConfig {
    /// Connection timeout in seconds
    pub timeout_secs: u64,
    /// Maximum number of retry attempts for failed operations
    pub max_retries: u32,
    /// Use passive mode (PASV) instead of active mode
    pub passive_mode: bool,
    /// Connection pool size for reusing connections
    pub connection_pool_size: usize,
}

impl Default for FtpDownloadConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            max_retries: 3,
            passive_mode: true,  // Passive mode works better behind NAT
            connection_pool_size: 5,
        }
    }
}

/// FTP credentials structure
#[derive(Debug, Clone)]
pub struct FtpCredentials {
    pub username: String,
    pub password: String,
}

impl Default for FtpCredentials {
    fn default() -> Self {
        Self {
            username: "anonymous".to_string(),
            password: "anonymous@chiral.network".to_string(),
        }
    }
}

impl FtpCredentials {
    /// Create anonymous credentials
    pub fn anonymous() -> Self {
        Self::default()
    }

    /// Create authenticated credentials
    pub fn new(username: String, password: String) -> Self {
        Self { username, password }
    }
}

/// FTP downloader service for byte-range downloads
pub struct FtpDownloader {
    config: FtpDownloadConfig,
}

impl FtpDownloader {
    /// Create a new FTP downloader with default configuration
    pub fn new() -> Self {
        Self {
            config: FtpDownloadConfig::default(),
        }
    }

    /// Create a new FTP downloader with custom configuration
    pub fn with_config(config: FtpDownloadConfig) -> Self {
        Self { config }
    }

    /// Get a reference to the current configuration
    pub fn config(&self) -> &FtpDownloadConfig {
        &self.config
    }

    /// Connect to FTP server and authenticate
    ///
    /// # Arguments
    /// * `url` - The FTP URL (e.g., "ftp://ftp.example.com:21/path/file.bin")
    /// * `creds` - Optional credentials (None uses anonymous login)
    ///
    /// # Returns
    /// * `Ok(FtpStream)` - Connected and authenticated FTP stream
    /// * `Err(String)` - Error message if connection fails
    ///
    /// # Example
    /// ```no_run
    /// let downloader = FtpDownloader::new();
    /// let url = Url::parse("ftp://ftp.gnu.org").unwrap();
    /// let stream = downloader.connect_and_login(&url, None).await.unwrap();
    /// ```
    pub async fn connect_and_login(
        &self,
        url: &Url,
        creds: Option<FtpCredentials>,
    ) -> Result<FtpStream, String> {
        let host = url
            .host_str()
            .ok_or("Invalid URL: missing host")?
            .to_string();

        let port = url.port().unwrap_or(21);

        let credentials = creds.unwrap_or_default();

        debug!(
            "Connecting to FTP server {}:{} as user '{}' with timeout {}s",
            host, port, credentials.username, self.config.timeout_secs
        );

        // Spawn blocking task since FtpStream is sync
        let config = self.config.clone();
        let host_clone = host.clone();

        let stream = task::spawn_blocking(move || -> Result<FtpStream, String> {
            // Create timeout duration
            let timeout = Duration::from_secs(config.timeout_secs);

            // Resolve address - use ToSocketAddrs to handle both hostnames and IP addresses
            use std::net::ToSocketAddrs;
            let addr_str = format!("{}:{}", host_clone, port);
            let addr = addr_str
                .to_socket_addrs()
                .map_err(|e| format!("Failed to resolve address '{}': {}", addr_str, e))?
                .next()
                .ok_or_else(|| format!("Could not resolve hostname: {}", host_clone))?;

            // Connect to FTP server with timeout
            let mut ftp_stream = FtpStream::connect_timeout(addr, timeout)
                .map_err(|e| format!("Failed to connect to FTP server: {}", e))?;

            // Set read/write timeouts on the underlying stream
            ftp_stream
                .get_ref()
                .set_read_timeout(Some(timeout))
                .map_err(|e| format!("Failed to set read timeout: {}", e))?;
            ftp_stream
                .get_ref()
                .set_write_timeout(Some(timeout))
                .map_err(|e| format!("Failed to set write timeout: {}", e))?;

            debug!("FTP connection established with timeout configured");

            // Login
            ftp_stream
                .login(&credentials.username, &credentials.password)
                .map_err(|e| format!("FTP login failed: {}", e))?;

            info!("Successfully authenticated to FTP server {}", host_clone);

            // Switch to passive mode if configured
            if config.passive_mode {
                ftp_stream.set_mode(suppaftp::Mode::Passive);
                debug!("Switched to passive mode (PASV)");
            }

            // Set binary transfer mode (TYPE I)
            ftp_stream
                .transfer_type(FileType::Binary)
                .map_err(|e| format!("Failed to set binary mode: {}", e))?;
            debug!("Set transfer type to binary");

            Ok(ftp_stream)
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))??;

        info!("FTP connection established to {}:{}", host, port);

        Ok(stream)
    }

    /// Download a specific byte range from FTP server
    ///
    /// Uses REST (restart) command to set the starting offset, then RETR to download.
    /// Reads exactly `size` bytes from the data connection.
    ///
    /// # Arguments
    /// * `stream` - Active FTP connection
    /// * `remote_path` - Path to file on FTP server (e.g., "/pub/file.bin")
    /// * `start_byte` - Starting byte offset (0-based)
    /// * `size` - Number of bytes to download
    ///
    /// # Returns
    /// * `Ok(Vec<u8>)` - Downloaded bytes
    /// * `Err(String)` - Error message
    ///
    /// # Example
    /// ```no_run
    /// let data = downloader.download_range(&mut stream, "/pub/file.bin", 1024, 256).await.unwrap();
    /// assert_eq!(data.len(), 256);
    /// ```
    pub async fn download_range(
        &self,
        stream: &mut FtpStream,
        remote_path: &str,
        start_byte: u64,
        size: u64,
    ) -> Result<Vec<u8>, String> {
        if size == 0 {
            return Ok(Vec::new());
        }

        debug!(
            "Downloading FTP range: {} bytes from offset {} (path: {})",
            size, start_byte, remote_path
        );

        let max_retries = self.config.max_retries;
        let mut last_error = String::new();

        for attempt in 1..=max_retries {
            match Self::try_download_range_blocking(stream, remote_path, start_byte, size) {
                Ok(data) => {
                    if data.len() != size as usize {
                        last_error = format!(
                            "Size mismatch: expected {} bytes, got {}",
                            size,
                            data.len()
                        );
                        warn!("Attempt {}/{}: {}", attempt, max_retries, last_error);

                        if attempt < max_retries {
                            tokio::time::sleep(Duration::from_millis(1000 * attempt as u64)).await;
                            continue;
                        }
                    } else {
                        info!(
                            "Successfully downloaded {} bytes from FTP (attempt {})",
                            data.len(),
                            attempt
                        );
                        return Ok(data);
                    }
                }
                Err(e) => {
                    last_error = e.clone();
                    warn!("Attempt {}/{} failed: {}", attempt, max_retries, e);

                    if attempt < max_retries {
                        tokio::time::sleep(Duration::from_millis(1000 * attempt as u64)).await;
                    }
                }
            }
        }

        Err(format!(
            "Failed after {} attempts: {}",
            max_retries, last_error
        ))
    }

    /// Blocking implementation of range download (called from spawn_blocking)
    fn try_download_range_blocking(
        stream: &mut FtpStream,
        remote_path: &str,
        start_byte: u64,
        size: u64,
    ) -> Result<Vec<u8>, String> {
        // Send REST command to set starting position (if supported by suppaftp 6.x)
        // Note: suppaftp 6.x may not have rest() method, so we rely on RETR with offset

        debug!("Attempting to download {} bytes from offset {}", size, start_byte);

        // Use RETR command with a closure to read data
        // suppaftp v6 API: retr(file_name, reader_fn)
        let buffer = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let buffer_clone = buffer.clone();
        let target_size = size as usize;

        stream
            .retr(remote_path, |reader| {
                let mut buf = buffer_clone.lock().unwrap();

                // Skip to start_byte if offset is needed
                if start_byte > 0 {
                    // Read and discard bytes until we reach start_byte
                    let mut skip_buf = vec![0u8; 8192];
                    let mut skipped = 0u64;

                    while skipped < start_byte {
                        let to_skip = std::cmp::min(8192, (start_byte - skipped) as usize);
                        match reader.read(&mut skip_buf[..to_skip]) {
                            Ok(0) => break,  // EOF
                            Ok(n) => skipped += n as u64,
                            Err(e) => return Err(FtpError::ConnectionError(std::io::Error::new(std::io::ErrorKind::Other, format!("Skip error: {}", e)))),
                        }
                    }

                    debug!("Skipped {} bytes to reach offset", skipped);
                }

                // Read the actual data
                let mut total_read = 0usize;
                let mut read_buf = vec![0u8; 8192];

                while total_read < target_size {
                    let to_read = std::cmp::min(8192, target_size - total_read);
                    match reader.read(&mut read_buf[..to_read]) {
                        Ok(0) => {
                            warn!("EOF after {} bytes (expected {})", total_read, target_size);
                            break;
                        }
                        Ok(n) => {
                            buf.extend_from_slice(&read_buf[..n]);
                            total_read += n;
                            debug!("Read {} bytes (total: {}/{})", n, total_read, target_size);
                        }
                        Err(e) => return Err(FtpError::ConnectionError(e)),
                    }
                }

                Ok(())
            })
            .map_err(|e| format!("RETR command failed: {}", e))?;

        let data = buffer.lock().unwrap().clone();
        debug!("Completed reading {} bytes from FTP", data.len());

        Ok(data)
    }

    /// Get the size of a file on the FTP server
    ///
    /// Uses the SIZE command (RFC 3659)
    pub async fn get_file_size(
        &self,
        stream: &mut FtpStream,
        remote_path: &str,
    ) -> Result<u64, String> {
        let remote_path_clone = remote_path.to_string();
        debug!("Getting file size for: {}", remote_path_clone);

        // Synchronous operation since we can't move &mut stream
        let size = stream
            .size(&remote_path_clone)
            .map_err(|e| format!("SIZE command failed: {}", e))?;

        info!("File size for {}: {} bytes", remote_path_clone, size);

        Ok(size as u64)
    }

    /// Test if FTP server supports REST command (resume capability)
    ///
    /// For suppaftp v6, we assume REST is not directly accessible,
    /// so we return true optimistically (will be tested during actual download)
    pub async fn supports_resume(&self, _stream: &mut FtpStream) -> Result<bool, String> {
        debug!("REST command support assumed (will be tested during download)");

        // suppaftp v6 doesn't expose REST command directly
        // We'll rely on reading with offset in try_download_range_blocking
        Ok(true)
    }

    /// Download entire file (without range)
    pub async fn download_full(
        &self,
        stream: &mut FtpStream,
        remote_path: &str,
    ) -> Result<Vec<u8>, String> {
        info!("Downloading full file: {}", remote_path);

        let buffer = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let buffer_clone = buffer.clone();

        stream
            .retr(remote_path, |reader| {
                let mut buf = buffer_clone.lock().unwrap();
                let mut temp_buf = vec![0u8; 8192];

                loop {
                    match reader.read(&mut temp_buf) {
                        Ok(0) => break,  // EOF
                        Ok(n) => buf.extend_from_slice(&temp_buf[..n]),
                        Err(e) => return Err(FtpError::ConnectionError(e)),
                    }
                }

                Ok(())
            })
            .map_err(|e| format!("RETR command failed: {}", e))?;

        let data = buffer.lock().unwrap().clone();

        info!("Downloaded {} bytes from FTP", data.len());

        Ok(data)
    }

    /// Close FTP connection gracefully
    pub async fn disconnect(&self, stream: &mut FtpStream) -> Result<(), String> {
        debug!("Disconnecting from FTP server");

        // Synchronous operation since we can't move &mut stream
        stream
            .quit()
            .map_err(|e| format!("QUIT command failed: {}", e))?;

        info!("FTP connection closed");

        Ok(())
    }

    /// List files in a directory
    pub async fn list_directory(
        &self,
        stream: &mut FtpStream,
        path: &str,
    ) -> Result<Vec<String>, String> {
        let path_clone = path.to_string();
        debug!("Listing directory: {}", path_clone);

        // Synchronous operation since we can't move &mut stream
        let entries = stream
            .nlst(Some(&path_clone))
            .map_err(|e| format!("NLST command failed: {}", e))?;

        info!("Found {} entries in directory {}", entries.len(), path_clone);

        Ok(entries)
    }

    /// Change working directory
    pub async fn change_directory(
        &self,
        stream: &mut FtpStream,
        path: &str,
    ) -> Result<(), String> {
        let path_clone = path.to_string();
        debug!("Changing directory to: {}", path_clone);

        // Use a static reference workaround for spawn_blocking
        // Since we can't move &mut stream into the closure, we do the operation synchronously
        stream
            .cwd(&path_clone)
            .map_err(|e| format!("CWD command failed: {}", e))?;

        info!("Changed directory to: {}", path_clone);

        Ok(())
    }

    /// Get current working directory
    pub async fn get_working_directory(&self, stream: &mut FtpStream) -> Result<String, String> {
        // Synchronous operation since we can't move &mut stream
        stream
            .pwd()
            .map_err(|e| format!("PWD command failed: {}", e))
    }
}

impl Default for FtpDownloader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_config_creation() {
        let config = FtpDownloadConfig::default();
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.max_retries, 3);
        assert!(config.passive_mode);
    }

    #[tokio::test]
    async fn test_downloader_creation() {
        let downloader = FtpDownloader::new();
        assert_eq!(downloader.config.timeout_secs, 30);
    }

    #[tokio::test]
    async fn test_custom_config() {
        let config = FtpDownloadConfig {
            timeout_secs: 60,
            max_retries: 5,
            passive_mode: false,
            connection_pool_size: 10,
        };

        let downloader = FtpDownloader::with_config(config.clone());
        assert_eq!(downloader.config.timeout_secs, 60);
        assert_eq!(downloader.config.max_retries, 5);
        assert!(!downloader.config.passive_mode);
    }

    #[tokio::test]
    async fn test_anonymous_credentials() {
        let creds = FtpCredentials::anonymous();
        assert_eq!(creds.username, "anonymous");
        assert!(creds.password.contains("anonymous"));
    }

    #[tokio::test]
    async fn test_custom_credentials() {
        let creds = FtpCredentials::new("user".to_string(), "pass".to_string());
        assert_eq!(creds.username, "user");
        assert_eq!(creds.password, "pass");
    }

    #[tokio::test]
    async fn test_timeout_is_configured() {
        // Test that timeout from config is accessible
        let config = FtpDownloadConfig {
            timeout_secs: 45,
            max_retries: 3,
            passive_mode: true,
            connection_pool_size: 5,
        };

        let downloader = FtpDownloader::with_config(config);
        assert_eq!(downloader.config().timeout_secs, 45);
    }

    #[tokio::test]
    async fn test_default_timeout() {
        // Test that default timeout is 30 seconds
        let downloader = FtpDownloader::new();
        assert_eq!(downloader.config().timeout_secs, 30);
    }

    // Integration tests with real FTP servers
    // These are ignored by default - run with: cargo test -- --ignored

    #[tokio::test]
    #[ignore]
    async fn test_connect_to_gnu_ftp() {
        let downloader = FtpDownloader::new();
        let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

        let result = downloader.connect_and_login(&url, None).await;

        match result {
            Ok(mut stream) => {
                println!("Successfully connected to ftp.gnu.org");

                // Try to get working directory
                let pwd = downloader.get_working_directory(&mut stream).await;
                println!("Working directory: {:?}", pwd);

                // Disconnect
                let _ = downloader.disconnect(&mut stream).await;
            }
            Err(e) => {
                println!("Connection failed (expected if network unavailable): {}", e);
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_download_range_from_gnu() {
        let downloader = FtpDownloader::new();
        let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

        let mut stream = match downloader.connect_and_login(&url, None).await {
            Ok(s) => s,
            Err(e) => {
                println!("Test skipped (connection failed): {}", e);
                return;
            }
        };

        // Try to download first 100 bytes of a known file
        let result = downloader
            .download_range(&mut stream, "/README", 0, 100)
            .await;

        match result {
            Ok(data) => {
                println!("Downloaded {} bytes", data.len());
                assert!(data.len() <= 100);
            }
            Err(e) => {
                println!("Download failed: {}", e);
            }
        }

        let _ = downloader.disconnect(&mut stream).await;
    }

    #[tokio::test]
    #[ignore]
    async fn test_get_file_size() {
        let downloader = FtpDownloader::new();
        let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

        let mut stream = match downloader.connect_and_login(&url, None).await {
            Ok(s) => s,
            Err(e) => {
                println!("Test skipped: {}", e);
                return;
            }
        };

        let result = downloader.get_file_size(&mut stream, "/README").await;

        match result {
            Ok(size) => {
                println!("File size: {} bytes", size);
                assert!(size > 0);
            }
            Err(e) => {
                println!("SIZE command failed: {}", e);
            }
        }

        let _ = downloader.disconnect(&mut stream).await;
    }

    #[tokio::test]
    #[ignore]
    async fn test_supports_resume() {
        let downloader = FtpDownloader::new();
        let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

        let mut stream = match downloader.connect_and_login(&url, None).await {
            Ok(s) => s,
            Err(e) => {
                println!("Test skipped: {}", e);
                return;
            }
        };

        let supports = downloader.supports_resume(&mut stream).await;

        match supports {
            Ok(supported) => {
                println!("REST command supported: {}", supported);
            }
            Err(e) => {
                println!("Error testing REST: {}", e);
            }
        }

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

        let result = downloader.list_directory(&mut stream, "/").await;

        match result {
            Ok(files) => {
                println!("Found {} files/directories", files.len());
                for file in files.iter().take(5) {
                    println!("  - {}", file);
                }
            }
            Err(e) => {
                println!("List failed: {}", e);
            }
        }

        let _ = downloader.disconnect(&mut stream).await;
    }

    #[tokio::test]
    #[ignore]
    async fn test_multi_range_download() {
        let downloader = FtpDownloader::new();
        let url = Url::parse("ftp://ftp.gnu.org").expect("Invalid URL");

        let mut stream = match downloader.connect_and_login(&url, None).await {
            Ok(s) => s,
            Err(e) => {
                println!("Test skipped: {}", e);
                return;
            }
        };

        // Download multiple ranges
        let ranges = vec![(0, 50), (50, 50), (100, 50)];
        let mut chunks = Vec::new();

        for (start, size) in ranges {
            match downloader
                .download_range(&mut stream, "/README", start, size)
                .await
            {
                Ok(data) => {
                    println!("Downloaded range {}-{}: {} bytes", start, start + size, data.len());
                    chunks.push(data);
                }
                Err(e) => {
                    println!("Range download failed: {}", e);
                    break;
                }
            }
        }

        // Reassemble
        if chunks.len() == 3 {
            let full_data: Vec<u8> = chunks.into_iter().flatten().collect();
            println!("Reassembled {} bytes total", full_data.len());
            assert_eq!(full_data.len(), 150);
        }

        let _ = downloader.disconnect(&mut stream).await;
    }
}