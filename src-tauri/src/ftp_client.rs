// ftp_client.rs
// FTP/FTPS download client implementation
//
// This module provides FTP and FTPS download functionality using the suppaftp library.
// It supports both regular FTP and FTP over TLS (FTPS), passive/active modes,
// and encrypted password handling.

use crate::download_source::FtpSourceInfo;
use anyhow::{anyhow, Context, Result};
use std::net::ToSocketAddrs;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use suppaftp::types::FileType;
use suppaftp::{FtpStream, RustlsConnector, RustlsFtpStream};
use tokio::task::spawn_blocking;
use tracing::{debug, info, warn};
use serde::{Serialize, Deserialize};
use url::Url;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use sha2::{Sha256, Digest};
use rand::RngCore;
use base64::{Engine as _, engine::general_purpose};

/// FTP file entry information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtpFileEntry {
    pub name: String,
    pub size: u64,
    pub is_directory: bool,
    pub modified: Option<String>,
    pub permissions: Option<String>,
}

/// Default FTP connection timeout in seconds
///
/// This timeout is used when connecting to FTP servers if the FtpSourceInfo
/// does not specify a custom timeout. A 30-second timeout is chosen as a
/// reasonable balance between:
/// - Allowing time for slow network connections to establish
/// - Preventing indefinite hangs on unresponsive servers
const DEFAULT_FTP_TIMEOUT_SECS: u64 = 30;

/// Create a rustls TLS connector for FTPS connections
fn create_rustls_connector() -> Result<RustlsConnector> {
    let mut root_cert_store = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs()
        .context("Failed to load native root certificates")?
    {
        root_cert_store
            .add(&rustls::Certificate(cert.0))
            .context("Failed to add certificate to store")?;
    }
    
    let tls_config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_cert_store)
        .with_no_client_auth();

    Ok(RustlsConnector::from(Arc::new(tls_config)))
}

/// FTP download progress callback
pub type ProgressCallback = Box<dyn Fn(u64, u64) + Send>;

/// Control flag for an in-flight FTP transfer (checked from the blocking download loop)
/// 0 = running, 1 = pause, 2 = cancel
pub type TransferControl = Arc<AtomicU8>;

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

        // Create TLS connector with rustls
        let tls_connector = create_rustls_connector()?;

        // Note: connect_secure_implicit doesn't support timeout directly,
        // so we use the deprecated method but set timeouts after connection
        let mut ftp_stream = RustlsFtpStream::connect_secure_implicit(
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
        let parsed = Url::parse(url).context("Invalid FTP URL")?;
        let scheme = parsed.scheme();
        if scheme != "ftp" && scheme != "ftps" {
            anyhow::bail!("Invalid FTP URL scheme: {}", scheme);
        }

        let host = parsed.host_str().context("Invalid FTP URL: missing host")?.to_string();
        let port = parsed.port().unwrap_or_else(|| if scheme == "ftps" { 990 } else { 21 });
        let remote_path = {
            let p = parsed.path();
            if p.is_empty() { "/".to_string() } else { p.to_string() }
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
        // Prefer credentials embedded in the URL (ftp://user:pass@host/path)
        if let Ok(parsed) = Url::parse(&source_info.url) {
            if !parsed.username().is_empty() {
                return Ok((
                    parsed.username().to_string(),
                    parsed.password().unwrap_or("").to_string(),
                ));
            }
        }

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
    let control = Arc::new(AtomicU8::new(0));
    download_from_ftp_with_progress_controlled(source_info, output_path, progress_callback, control).await
}

/// Download a file from FTP server with progress callback and pause/cancel control.
///
/// The core resume logic is unchanged: if a partial output file exists and the server supports
/// REST, the transfer resumes automatically from the existing file size.
pub async fn download_from_ftp_with_progress_controlled(
    source_info: &FtpSourceInfo,
    output_path: &Path,
    progress_callback: ProgressCallback,
    control: TransferControl,
) -> Result<u64> {
    let source_clone = source_info.clone();
    let output_clone = output_path.to_path_buf();
    let control_clone = control.clone();

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

        // Check if partial file exists for resume
        let resume_position = if output_clone.exists() {
            let metadata = std::fs::metadata(&output_clone)
                .context("Failed to get partial file metadata")?;
            let partial_size = metadata.len();

            // Only resume if partial file is smaller than remote file
            if partial_size < file_size && partial_size > 0 {
                info!(
                    partial_bytes = partial_size,
                    total_bytes = file_size,
                    "Resuming FTP download from previous position"
                );
                partial_size
            } else {
                0
            }
        } else {
            0
        };

        // Open retrieve stream (with resume if applicable)
        let mut reader = if resume_position > 0 {
            // Resume from position using REST command
            ftp_stream
                .resume_transfer(resume_position as usize)
                .context("Failed to set resume position")?;
            ftp_stream
                .retr_as_stream(&remote_path)
                .context("Failed to start file retrieval (resume)")?
        } else {
            ftp_stream
                .retr_as_stream(&remote_path)
                .context("Failed to start file retrieval")?
        };

        // Create or append to output file
        use std::io::Write;
        use std::fs::OpenOptions;
        let mut output_file = if resume_position > 0 {
            OpenOptions::new()
                .append(true)
                .open(&output_clone)
                .context("Failed to open file for resume")?
        } else {
            std::fs::File::create(&output_clone)
                .context("Failed to create output file")?
        };

        // Download in chunks with progress reporting
        const CHUNK_SIZE: usize = 64 * 1024; // 64KB chunks
        let mut buffer = vec![0u8; CHUNK_SIZE];
        let mut total_downloaded = resume_position;

        loop {
            use std::io::Read;

            match control_clone.load(Ordering::Relaxed) {
                1 => return Err(anyhow!("paused")),
                2 => return Err(anyhow!("canceled")),
                _ => {}
            }

            match reader.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    output_file.write_all(&buffer[..n])
                        .context("Failed to write to output file")?;
                    total_downloaded += n as u64;
                    progress_callback(total_downloaded, file_size);
                }
                Err(e) => return Err(anyhow!("Failed to read from FTP stream: {}", e)),
            }
        }

        // Finalize the transfer
        // Best-effort finalize; some servers may error after an interrupted transfer.
        if let Err(e) = ftp_stream.finalize_retr_stream(reader) {
            warn!("Failed to finalize FTP retrieval stream: {}", e);
        }

        // Quit connection
        ftp_stream.quit().context("Failed to quit FTP session")?;

        Ok(total_downloaded)
    })
    .await
    .context("FTP download task panicked")?
}

/// List files and directories in an FTP directory
///
/// # Arguments
/// * `source_info` - FTP source information (URL should point to a directory)
///
/// # Returns
/// Vector of file entries in the directory
pub async fn list_ftp_directory(source_info: &FtpSourceInfo) -> Result<Vec<FtpFileEntry>> {
    let source_clone = source_info.clone();

    spawn_blocking(move || {
        let (host, port, remote_path) = FtpClient::parse_ftp_url(&source_clone.url)?;

        // Connect to FTP server
        let ftp_stream = if source_clone.use_ftps {
            // FTPS connection
            let tls_connector = create_rustls_connector()?;

            let mut stream = RustlsFtpStream::connect_secure_implicit(
                format!("{}:{}", host, port),
                tls_connector,
                &host,
            )
            .context("Failed to connect to FTPS server")?;

            // Login
            let (username, password) = FtpClient::get_credentials(&source_clone, None)?;
            stream
                .login(&username, &password)
                .context("FTPS login failed")?;

            if source_clone.passive_mode {
                stream.set_mode(suppaftp::Mode::Passive);
            }

            // List directory
            let files = stream
                .list(Some(&remote_path))
                .context("Failed to list FTPS directory")?;

            stream.quit().ok();

            files
        } else {
            // Regular FTP connection
            let mut stream = FtpStream::connect(format!("{}:{}", host, port))
                .context("Failed to connect to FTP server")?;

            // Login
            let (username, password) = FtpClient::get_credentials(&source_clone, None)?;
            stream
                .login(&username, &password)
                .context("FTP login failed")?;

            if source_clone.passive_mode {
                stream.set_mode(suppaftp::Mode::Passive);
            }

            // List directory
            let files = stream
                .list(Some(&remote_path))
                .context("Failed to list FTP directory")?;

            stream.quit().ok();

            files
        };

        // Parse file listings
        let mut entries = Vec::new();

        for file_line in ftp_stream {
            // Parse Unix-style listing (most common format)
            // Example: "drwxr-xr-x   2 user  group    4096 Jan 01 12:00 dirname"
            // Example: "-rw-r--r--   1 user  group   12345 Jan 01 12:00 filename.txt"

            let parts: Vec<&str> = file_line.split_whitespace().collect();

            if parts.len() < 9 {
                // Skip malformed lines
                continue;
            }

            let permissions = parts[0].to_string();
            let is_directory = permissions.starts_with('d');

            // Size is typically at index 4
            let size = parts[4].parse::<u64>().unwrap_or(0);

            // Filename is everything after the time (index 8 onwards)
            let name = parts[8..].join(" ");

            // Skip current and parent directory entries
            if name == "." || name == ".." {
                continue;
            }

            // Date/time is at indices 5, 6, 7
            let modified = if parts.len() >= 8 {
                Some(format!("{} {} {}", parts[5], parts[6], parts[7]))
            } else {
                None
            };

            entries.push(FtpFileEntry {
                name,
                size,
                is_directory,
                modified,
                permissions: Some(permissions),
            });
        }

        info!(
            entries_count = entries.len(),
            path = %remote_path,
            "Listed FTP directory"
        );

        Ok(entries)
    })
    .await
    .context("FTP directory listing task panicked")?
}

/// Delete a file or directory on FTP server
///
/// # Arguments
/// * `source_info` - FTP source information (URL should point to the file/directory to delete)
///
/// # Returns
/// Success or error
pub async fn delete_ftp_file(source_info: &FtpSourceInfo) -> Result<()> {
    let source_clone = source_info.clone();

    spawn_blocking(move || {
        let (host, port, remote_path) = FtpClient::parse_ftp_url(&source_clone.url)?;

        if source_clone.use_ftps {
            // FTPS connection
            let tls_connector = create_rustls_connector()?;

            let mut stream = RustlsFtpStream::connect_secure_implicit(
                format!("{}:{}", host, port),
                tls_connector,
                &host,
            )
            .context("Failed to connect to FTPS server")?;

            let (username, password) = FtpClient::get_credentials(&source_clone, None)?;
            stream.login(&username, &password).context("FTPS login failed")?;

            if source_clone.passive_mode {
                stream.set_mode(suppaftp::Mode::Passive);
            }

            // Try to delete as file first, then as directory
            match stream.rm(&remote_path) {
                Ok(_) => {
                    info!(path = %remote_path, "Deleted file via FTPS");
                }
                Err(_) => {
                    // If file deletion fails, try removing as directory
                    stream.rmdir(&remote_path)
                        .context("Failed to delete file or directory via FTPS")?;
                    info!(path = %remote_path, "Deleted directory via FTPS");
                }
            }

            stream.quit().ok();
        } else {
            // Regular FTP
            let mut stream = FtpStream::connect(format!("{}:{}", host, port))
                .context("Failed to connect to FTP server")?;

            let (username, password) = FtpClient::get_credentials(&source_clone, None)?;
            stream.login(&username, &password).context("FTP login failed")?;

            if source_clone.passive_mode {
                stream.set_mode(suppaftp::Mode::Passive);
            }

            // Try to delete as file first, then as directory
            match stream.rm(&remote_path) {
                Ok(_) => {
                    info!(path = %remote_path, "Deleted file via FTP");
                }
                Err(_) => {
                    // If file deletion fails, try removing as directory
                    stream.rmdir(&remote_path)
                        .context("Failed to delete file or directory via FTP")?;
                    info!(path = %remote_path, "Deleted directory via FTP");
                }
            }

            stream.quit().ok();
        }

        Ok(())
    })
    .await
    .context("FTP delete task panicked")?
}

/// Rename a file or directory on FTP server
///
/// # Arguments
/// * `source_info` - FTP source information (URL should point to the file/directory to rename)
/// * `new_name` - New name for the file/directory
///
/// # Returns
/// Success or error
pub async fn rename_ftp_file(source_info: &FtpSourceInfo, new_name: &str) -> Result<()> {
    let source_clone = source_info.clone();
    let new_name_clone = new_name.to_string();

    spawn_blocking(move || {
        let (host, port, remote_path) = FtpClient::parse_ftp_url(&source_clone.url)?;

        if source_clone.use_ftps {
            // FTPS connection
            let tls_connector = create_rustls_connector()?;

            let mut stream = RustlsFtpStream::connect_secure_implicit(
                format!("{}:{}", host, port),
                tls_connector,
                &host,
            )
            .context("Failed to connect to FTPS server")?;

            let (username, password) = FtpClient::get_credentials(&source_clone, None)?;
            stream.login(&username, &password).context("FTPS login failed")?;

            if source_clone.passive_mode {
                stream.set_mode(suppaftp::Mode::Passive);
            }

            stream.rename(&remote_path, &new_name_clone)
                .context("Failed to rename file via FTPS")?;

            info!(old_path = %remote_path, new_name = %new_name_clone, "Renamed file via FTPS");

            stream.quit().ok();
        } else {
            // Regular FTP
            let mut stream = FtpStream::connect(format!("{}:{}", host, port))
                .context("Failed to connect to FTP server")?;

            let (username, password) = FtpClient::get_credentials(&source_clone, None)?;
            stream.login(&username, &password).context("FTP login failed")?;

            if source_clone.passive_mode {
                stream.set_mode(suppaftp::Mode::Passive);
            }

            stream.rename(&remote_path, &new_name_clone)
                .context("Failed to rename file via FTP")?;

            info!(old_path = %remote_path, new_name = %new_name_clone, "Renamed file via FTP");

            stream.quit().ok();
        }

        Ok(())
    })
    .await
    .context("FTP rename task panicked")?
}

/// Create a directory on FTP server
///
/// # Arguments
/// * `source_info` - FTP source information (URL should point to the directory to create)
///
/// # Returns
/// Success or error
pub async fn create_ftp_directory(source_info: &FtpSourceInfo) -> Result<()> {
    let source_clone = source_info.clone();

    spawn_blocking(move || {
        let (host, port, remote_path) = FtpClient::parse_ftp_url(&source_clone.url)?;

        if source_clone.use_ftps {
            // FTPS connection
            let tls_connector = create_rustls_connector()?;

            let mut stream = RustlsFtpStream::connect_secure_implicit(
                format!("{}:{}", host, port),
                tls_connector,
                &host,
            )
            .context("Failed to connect to FTPS server")?;

            let (username, password) = FtpClient::get_credentials(&source_clone, None)?;
            stream.login(&username, &password).context("FTPS login failed")?;

            if source_clone.passive_mode {
                stream.set_mode(suppaftp::Mode::Passive);
            }

            stream.mkdir(&remote_path)
                .context("Failed to create directory via FTPS")?;

            info!(path = %remote_path, "Created directory via FTPS");

            stream.quit().ok();
        } else {
            // Regular FTP
            let mut stream = FtpStream::connect(format!("{}:{}", host, port))
                .context("Failed to connect to FTP server")?;

            let (username, password) = FtpClient::get_credentials(&source_clone, None)?;
            stream.login(&username, &password).context("FTP login failed")?;

            if source_clone.passive_mode {
                stream.set_mode(suppaftp::Mode::Passive);
            }

            stream.mkdir(&remote_path)
                .context("Failed to create directory via FTP")?;

            info!(path = %remote_path, "Created directory via FTP");

            stream.quit().ok();
        }

        Ok(())
    })
    .await
    .context("FTP mkdir task panicked")?
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

// ------ FTP Credential Encryption ------

/// Encrypts an FTP password using AES-256-GCM with file hash as key derivation material
///
/// # Security Properties
/// - Uses AES-256-GCM for authenticated encryption (detects tampering)
/// - Key derived from SHA-256(file_hash + static_salt)
/// - Random 12-byte nonce prepended to ciphertext
/// - Result is base64-encoded for storage/transmission
///
/// # Security Considerations
/// - File hash is public, so anyone with the hash can decrypt the password
/// - This is intentional: credentials are shared with peers who have the file hash
/// - NOT suitable for private FTP servers - use WebRTC protocol instead
///
/// # Arguments
/// * `password` - The plaintext FTP password to encrypt
/// * `file_hash` - The file hash used as key derivation material
///
/// # Returns
/// Base64-encoded string containing: nonce (12 bytes) + ciphertext
pub fn encrypt_ftp_password(password: &str, file_hash: &str) -> Result<String> {
    // Derive 256-bit key from file hash
    let mut hasher = Sha256::new();
    hasher.update(file_hash.as_bytes());
    hasher.update(b"chiral_ftp_credential_salt_v1");
    let key_bytes = hasher.finalize();

    // Create cipher
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| anyhow!("Failed to create cipher: {}", e))?;

    // Generate random nonce
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Encrypt
    let ciphertext = cipher
        .encrypt(nonce, password.as_bytes())
        .map_err(|e| anyhow!("Encryption failed: {}", e))?;

    // Combine nonce + ciphertext and base64 encode
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(general_purpose::STANDARD.encode(&combined))
}

/// Decrypts an FTP password encrypted with encrypt_ftp_password
///
/// # Arguments
/// * `encrypted` - Base64-encoded string containing nonce + ciphertext
/// * `file_hash` - The file hash used as key derivation material (must match encryption)
///
/// # Returns
/// Decrypted plaintext password
///
/// # Errors
/// Returns error if:
/// - Base64 decoding fails
/// - Data is too short (< 12 bytes for nonce)
/// - Key derivation fails
/// - Decryption fails (wrong key or tampered data)
/// - UTF-8 decoding fails
pub fn decrypt_ftp_password(encrypted: &str, file_hash: &str) -> Result<String> {
    // Decode base64
    let combined = general_purpose::STANDARD.decode(encrypted)
        .map_err(|e| anyhow!("Base64 decode failed: {}", e))?;

    if combined.len() < 12 {
        return Err(anyhow!("Invalid encrypted password: too short"));
    }

    // Split nonce and ciphertext
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    // Derive same key
    let mut hasher = Sha256::new();
    hasher.update(file_hash.as_bytes());
    hasher.update(b"chiral_ftp_credential_salt_v1");
    let key_bytes = hasher.finalize();

    // Create cipher
    let cipher = Aes256Gcm::new_from_slice(&key_bytes)
        .map_err(|e| anyhow!("Failed to create cipher: {}", e))?;

    // Decrypt
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow!("Decryption failed (wrong key or tampered data): {}", e))?;

    String::from_utf8(plaintext)
        .map_err(|e| anyhow!("UTF-8 decode failed: {}", e))
}
