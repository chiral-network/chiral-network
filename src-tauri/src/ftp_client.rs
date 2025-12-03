// ftp_client.rs
// FTP/FTPS download client implementation
//
// This module provides FTP and FTPS download functionality using the suppaftp library.
// It supports both regular FTP and FTP over TLS (FTPS), passive/active modes,
// and encrypted password handling.

use crate::download_source::FtpSourceInfo;
use anyhow::{Context, Result};
use std::net::ToSocketAddrs;
use std::path::Path;
use std::time::Duration;
use suppaftp::types::FileType;
use suppaftp::{FtpStream, NativeTlsConnector, NativeTlsFtpStream};
use tokio::task::spawn_blocking;
use tracing::{debug, info, warn};

/// Default FTP connection timeout in seconds
///
/// This timeout is used when connecting to FTP servers if the FtpSourceInfo
/// does not specify a custom timeout. A 30-second timeout is chosen as a
/// reasonable balance between:
/// - Allowing time for slow network connections to establish
/// - Preventing indefinite hangs on unresponsive servers
const DEFAULT_FTP_TIMEOUT_SECS: u64 = 30;

/// FTP download progress callback
pub type ProgressCallback = Box<dyn Fn(u64, u64) + Send>;

/// FTP client for handling file downloads
pub struct FtpClient {
    source_info: FtpSourceInfo,
}

impl FtpClient {
    /// Create a new FTP client with source information
    pub fn new(source_info: FtpSourceInfo) -> Self {
        Self { source_info }
    }

    /// Download a file from FTP server to specified path
    pub async fn download_file(&self, output_path: &Path) -> Result<u64> {
        info!(
            url = %self.source_info.url,
            output = ?output_path,
            ftps = self.source_info.use_ftps,
            passive = self.source_info.passive_mode,
            "Starting FTP download"
        );

        // Clone data for blocking task
        let source_info = self.source_info.clone();
        let output_path_clone = output_path.to_path_buf();
        let output_path_log = output_path.to_path_buf();

        // Run FTP download in blocking task pool
        let bytes = spawn_blocking(move || {
            if source_info.use_ftps {
                Self::download_with_ftps_sync(&source_info, &output_path_clone)
            } else {
                Self::download_with_ftp_sync(&source_info, &output_path_clone)
            }
        })
        .await
        .context("FTP download task panicked")??;

        info!(
            bytes = bytes,
            output = ?output_path_log,
            "FTP download completed"
        );

        Ok(bytes)
    }

    /// Download using regular FTP (no encryption) - synchronous
    fn download_with_ftp_sync(source_info: &FtpSourceInfo, output_path: &Path) -> Result<u64> {
        let (host, port, remote_path) = Self::parse_ftp_url(&source_info.url)?;

        // Get timeout from source info or use default
        let timeout_secs = source_info.timeout_secs.unwrap_or(DEFAULT_FTP_TIMEOUT_SECS);
        let timeout = Duration::from_secs(timeout_secs);

        debug!(
            host = %host,
            port = port,
            path = %remote_path,
            timeout_secs = timeout_secs,
            "Connecting to FTP server"
        );

        // Connect to FTP server with timeout
        let addr = format!("{}:{}", host, port)
            .to_socket_addrs()
            .context("Failed to resolve FTP server address")?
            .next()
            .context("No addresses found for FTP server")?;

        let mut ftp_stream =
            FtpStream::connect_timeout(addr, timeout).context("Failed to connect to FTP server")?;

        // Set read/write timeout on the underlying stream
        ftp_stream
            .get_ref()
            .set_read_timeout(Some(timeout))
            .context("Failed to set read timeout")?;
        ftp_stream
            .get_ref()
            .set_write_timeout(Some(timeout))
            .context("Failed to set write timeout")?;

        // Login
        let (username, password) = Self::get_credentials(source_info, None)?;
        debug!(username = %username, "Logging in to FTP server");

        ftp_stream
            .login(&username, &password)
            .context("FTP login failed")?;

        debug!("FTP login successful");

        // Set transfer type to binary
        ftp_stream
            .transfer_type(FileType::Binary)
            .context("Failed to set binary transfer mode")?;

        // Download file
        let cursor = ftp_stream
            .retr_as_buffer(&remote_path)
            .context("Failed to retrieve file from FTP server")?;

        let data = cursor.into_inner();
        let bytes_downloaded = data.len() as u64;

        debug!(bytes = bytes_downloaded, "File retrieved from FTP server");

        // Write to output file
        std::fs::write(output_path, &data).context("Failed to write file to disk")?;

        debug!(output = ?output_path, "File written to disk");

        // Quit connection
        ftp_stream.quit().context("Failed to quit FTP session")?;

        Ok(bytes_downloaded)
    }

    /// Download using FTPS (FTP over TLS) - synchronous
    fn download_with_ftps_sync(source_info: &FtpSourceInfo, output_path: &Path) -> Result<u64> {
        let (host, port, remote_path) = Self::parse_ftp_url(&source_info.url)?;

        // Get timeout from source info or use default
        let timeout_secs = source_info.timeout_secs.unwrap_or(DEFAULT_FTP_TIMEOUT_SECS);
        let timeout = Duration::from_secs(timeout_secs);

        debug!(
            host = %host,
            port = port,
            path = %remote_path,
            timeout_secs = timeout_secs,
            "Connecting to FTPS server"
        );

        // Create TLS connector
        let tls_connector = NativeTlsConnector::from(
            native_tls::TlsConnector::new().context("Failed to create TLS connector")?,
        );

        // Note: connect_secure_implicit doesn't support timeout directly,
        // so we use the deprecated method but set timeouts after connection
        let mut ftp_stream = NativeTlsFtpStream::connect_secure_implicit(
            format!("{}:{}", host, port),
            tls_connector,
            &host,
        )
        .context("Failed to connect to FTPS server")?;

        // Set read/write timeouts on the underlying TCP stream after connection
        ftp_stream
            .get_ref()
            .set_read_timeout(Some(timeout))
            .context("Failed to set read timeout")?;
        ftp_stream
            .get_ref()
            .set_write_timeout(Some(timeout))
            .context("Failed to set write timeout")?;

        debug!("FTPS connection established with timeout configured");

        // Login
        let (username, password) = Self::get_credentials(source_info, None)?;
        debug!(username = %username, "Logging in to FTPS server");

        ftp_stream
            .login(&username, &password)
            .context("FTPS login failed")?;

        debug!("FTPS login successful");

        // Set transfer type to binary
        ftp_stream
            .transfer_type(FileType::Binary)
            .context("Failed to set binary transfer mode")?;

        // Download file
        let cursor = ftp_stream
            .retr_as_buffer(&remote_path)
            .context("Failed to retrieve file from FTPS server")?;

        let data = cursor.into_inner();
        let bytes_downloaded = data.len() as u64;

        debug!(bytes = bytes_downloaded, "File retrieved from FTPS server");

        // Write to output file
        std::fs::write(output_path, &data).context("Failed to write file to disk")?;

        debug!(output = ?output_path, "File written to disk");

        // Quit connection
        ftp_stream.quit().context("Failed to quit FTPS session")?;

        Ok(bytes_downloaded)
    }

    /// Parse FTP URL to extract host, port, and path
    fn parse_ftp_url(url: &str) -> Result<(String, u16, String)> {
        // Remove ftp:// or ftps:// prefix
        let url_without_protocol = url
            .strip_prefix("ftp://")
            .or_else(|| url.strip_prefix("ftps://"))
            .context("Invalid FTP URL: missing protocol")?;

        // Split into host[:port]/path
        let parts: Vec<&str> = url_without_protocol.splitn(2, '/').collect();

        if parts.is_empty() {
            anyhow::bail!("Invalid FTP URL: no host specified");
        }

        let host_and_port = parts[0];
        let remote_path = if parts.len() > 1 {
            format!("/{}", parts[1])
        } else {
            "/".to_string()
        };

        // Parse host and port
        let (host, port) = if host_and_port.contains(':') {
            let host_port: Vec<&str> = host_and_port.splitn(2, ':').collect();
            let port_num = host_port
                .get(1)
                .and_then(|p| p.parse::<u16>().ok())
                .context("Invalid port number")?;
            (host_port[0].to_string(), port_num)
        } else {
            // Default FTP port
            (host_and_port.to_string(), 21)
        };

        Ok((host, port, remote_path))
    }

    /// Get FTP credentials (username and decrypted password)
    ///
    /// # Arguments
    /// * `source_info` - FTP source information
    /// * `decryption_key` - Optional AES-256 key for decrypting the password
    fn get_credentials(
        source_info: &FtpSourceInfo,
        decryption_key: Option<&[u8; 32]>,
    ) -> Result<(String, String)> {
        let username = source_info
            .username
            .clone()
            .unwrap_or_else(|| "anonymous".to_string());

        let password = if let Some(encrypted_password) = &source_info.encrypted_password {
            if let Some(key) = decryption_key {
                match crate::encryption::FileEncryption::decrypt_string(encrypted_password, key) {
                    Ok(decrypted) => decrypted,
                    Err(e) => {
                        warn!("Failed to decrypt FTP password: {}", e);
                        String::new()
                    }
                }
            } else {
                warn!("Encrypted password provided but no decryption key available");
                String::new()
            }
        } else {
            // Anonymous FTP or no password
            String::new()
        };

        Ok((username, password))
    }
}

/// Download a file from FTP server
///
/// This is a convenience function that creates an FTP client and downloads a file.
///
/// # Arguments
/// * `source_info` - FTP source information
/// * `output_path` - Path where the file will be saved
///
/// # Returns
/// Number of bytes downloaded
pub async fn download_from_ftp(source_info: &FtpSourceInfo, output_path: &Path) -> Result<u64> {
    let client = FtpClient::new(source_info.clone());
    client.download_file(output_path).await
}

/// Download a file from FTP server with progress callback
///
/// # Arguments
/// * `source_info` - FTP source information
/// * `output_path` - Path where the file will be saved
/// * `progress_callback` - Callback function for progress updates
///
/// # Returns
/// Number of bytes downloaded
pub async fn download_from_ftp_with_progress(
    source_info: &FtpSourceInfo,
    output_path: &Path,
    progress_callback: ProgressCallback,
) -> Result<u64> {
    let source_clone = source_info.clone();
    let output_clone = output_path.to_path_buf();
    
    spawn_blocking(move || {
        // Connect to FTP server
        let (host, port, remote_path) = FtpClient::parse_ftp_url(&source_clone.url)?;
        
        let mut ftp_stream = FtpStream::connect(format!("{}:{}", host, port))
            .context("Failed to connect to FTP server")?;
        
        // Login
        let (username, password) = FtpClient::get_credentials(&source_clone, None)?;
        ftp_stream
            .login(&username, &password)
            .context("Failed to login to FTP server")?;
        
        // Set binary mode and passive mode
        ftp_stream
            .transfer_type(FileType::Binary)
            .context("Failed to set binary transfer mode")?;
        
        if source_clone.passive_mode {
            ftp_stream.set_mode(suppaftp::Mode::Passive);
        }
        
        // Get file size for progress calculation
        let file_size = ftp_stream
            .size(&remote_path)
            .unwrap_or(0) as u64;
        
        // Open retrieve stream
        let mut reader = ftp_stream
            .retr_as_stream(&remote_path)
            .context("Failed to start file retrieval")?;
        
        // Create output file
        use std::io::Write;
        let mut output_file = std::fs::File::create(&output_clone)
            .context("Failed to create output file")?;
        
        // Download in chunks with progress reporting
        const CHUNK_SIZE: usize = 64 * 1024; // 64KB chunks
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut total_downloaded = 0u64;
        
        loop {
            use std::io::Read;
            match reader.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    output_file.write_all(&buffer[..n])
                        .context("Failed to write to output file")?;
                    total_downloaded += n as u64;
                    progress_callback(total_downloaded, file_size);
                }
                Err(e) => return Err(anyhow::anyhow!("Failed to read from FTP stream: {}", e)),
            }
        }
        
        // Finalize the transfer
        ftp_stream.finalize_retr_stream(reader)
            .context("Failed to finalize retrieval")?;
        
        // Quit connection
        ftp_stream.quit().context("Failed to quit FTP session")?;
        
        Ok(total_downloaded)
    })
    .await
    .context("FTP download task panicked")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ftp_url() {
        let (host, port, path) =
            FtpClient::parse_ftp_url("ftp://ftp.example.com/pub/file.tar.gz").unwrap();
        assert_eq!(host, "ftp.example.com");
        assert_eq!(port, 21);
        assert_eq!(path, "/pub/file.tar.gz");
    }

    #[test]
    fn test_parse_ftp_url_with_port() {
        let (host, port, path) =
            FtpClient::parse_ftp_url("ftp://ftp.example.com:2121/data/file.zip").unwrap();
        assert_eq!(host, "ftp.example.com");
        assert_eq!(port, 2121);
        assert_eq!(path, "/data/file.zip");
    }

    #[test]
    fn test_parse_ftps_url() {
        let (host, port, path) =
            FtpClient::parse_ftp_url("ftps://secure.example.com/file.tar.gz").unwrap();
        assert_eq!(host, "secure.example.com");
        assert_eq!(port, 21);
        assert_eq!(path, "/file.tar.gz");
    }

    #[test]
    fn test_get_credentials_anonymous() {
        let source_info = FtpSourceInfo {
            url: "ftp://ftp.example.com/file".to_string(),
            username: None,
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: None,
        };

        let (username, password) =
            FtpClient::get_credentials(&source_info, None).unwrap();
        assert_eq!(username, "anonymous");
        assert_eq!(password, "");
    }

    #[test]
    fn test_get_credentials_with_username() {
        let source_info = FtpSourceInfo {
            url: "ftp://ftp.example.com/file".to_string(),
            username: Some("testuser".to_string()),
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: None,
        };

        let (username, password) =
            FtpClient::get_credentials(&source_info, None).unwrap();
        assert_eq!(username, "testuser");
        assert_eq!(password, "");
    }

    #[test]
    fn test_timeout_secs_default() {
        // Test that default timeout is used when not specified
        let source_info = FtpSourceInfo {
            url: "ftp://ftp.example.com/file".to_string(),
            username: None,
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: None,
        };

        let timeout = source_info.timeout_secs.unwrap_or(DEFAULT_FTP_TIMEOUT_SECS);
        assert_eq!(timeout, 30);
    }

    #[test]
    fn test_timeout_secs_custom() {
        // Test that custom timeout is used when specified
        let source_info = FtpSourceInfo {
            url: "ftp://ftp.example.com/file".to_string(),
            username: None,
            encrypted_password: None,
            passive_mode: true,
            use_ftps: false,
            timeout_secs: Some(60),
        };

        let timeout = source_info.timeout_secs.unwrap_or(DEFAULT_FTP_TIMEOUT_SECS);
        assert_eq!(timeout, 60);
    }
}
