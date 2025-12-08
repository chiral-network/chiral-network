//! Embedded FTP Server
//!
//! Provides an FTP server for serving uploaded files to other peers.
//! Uses libunftp with filesystem backend.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, error};

/// FTP Server state
pub struct FtpServer {
    /// Root directory for serving files
    root_dir: PathBuf,
    /// Server port
    port: u16,
    /// Whether the server is running
    running: Arc<Mutex<bool>>,
    /// Shutdown handle
    shutdown_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl FtpServer {
    /// Create a new FTP server
    pub fn new(root_dir: PathBuf, port: u16) -> Self {
        Self {
            root_dir,
            port,
            running: Arc::new(Mutex::new(false)),
            shutdown_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the server port
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the root directory
    pub fn root_dir(&self) -> &PathBuf {
        &self.root_dir
    }

    /// Check if the server is running
    pub async fn is_running(&self) -> bool {
        *self.running.lock().await
    }

    /// Start the FTP server
    pub async fn start(&self) -> Result<(), String> {
        // Check if already running
        if *self.running.lock().await {
            return Ok(());
        }

        // Ensure root directory exists
        if !self.root_dir.exists() {
            std::fs::create_dir_all(&self.root_dir)
                .map_err(|e| format!("Failed to create FTP root directory: {}", e))?;
        }

        let root_dir = self.root_dir.clone();
        let port = self.port;
        let running = self.running.clone();
        let shutdown_tx_holder = self.shutdown_tx.clone();

        // Create shutdown channel
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        *shutdown_tx_holder.lock().await = Some(tx);

        // Spawn server in background
        tokio::spawn(async move {
            info!("Starting FTP server on port {} serving {}", port, root_dir.display());

            // Create the FTP server with filesystem backend
            let server = libunftp::ServerBuilder::new(Box::new(move || {
                unftp_sbe_fs::Filesystem::new(root_dir.clone())
            }))
            .greeting("Welcome to Chiral Network FTP Server")
            .passive_ports(50000..50100)
            .build()
            .unwrap();

            // Mark as running
            *running.lock().await = true;

            // Bind to address
            let addr = format!("0.0.0.0:{}", port);
            
            // Run server with shutdown signal
            tokio::select! {
                result = server.listen(addr) => {
                    match result {
                        Ok(_) => info!("FTP server stopped normally"),
                        Err(e) => error!("FTP server error: {}", e),
                    }
                }
                _ = rx => {
                    info!("FTP server received shutdown signal");
                }
            }

            *running.lock().await = false;
            info!("FTP server stopped");
        });

        // Wait a moment for the server to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        info!("FTP server started on port {}", self.port);
        Ok(())
    }

    /// Stop the FTP server
    pub async fn stop(&self) -> Result<(), String> {
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
            info!("FTP server shutdown signal sent");
        }
        Ok(())
    }

    /// Get the FTP URL for a file
    pub fn get_file_url(&self, file_name: &str) -> String {
        // Get local IP address
        let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
        format!("ftp://{}:{}/{}", local_ip, self.port, file_name)
    }

    /// Copy a file to the FTP server directory
    pub async fn add_file(&self, source_path: &PathBuf, file_name: &str) -> Result<String, String> {
        // Ensure root directory exists
        if !self.root_dir.exists() {
            std::fs::create_dir_all(&self.root_dir)
                .map_err(|e| format!("Failed to create FTP root directory: {}", e))?;
        }

        let dest_path = self.root_dir.join(file_name);
        
        // Copy the file
        tokio::fs::copy(source_path, &dest_path)
            .await
            .map_err(|e| format!("Failed to copy file to FTP directory: {}", e))?;

        info!("Added file to FTP server: {}", file_name);
        Ok(self.get_file_url(file_name))
    }

    /// Add file data directly to the FTP server directory
    pub async fn add_file_data(&self, data: &[u8], file_name: &str) -> Result<String, String> {
        // Ensure root directory exists
        if !self.root_dir.exists() {
            std::fs::create_dir_all(&self.root_dir)
                .map_err(|e| format!("Failed to create FTP root directory: {}", e))?;
        }

        let dest_path = self.root_dir.join(file_name);
        
        // Write the file
        tokio::fs::write(&dest_path, data)
            .await
            .map_err(|e| format!("Failed to write file to FTP directory: {}", e))?;

        info!("Added file data to FTP server: {} ({} bytes)", file_name, data.len());
        Ok(self.get_file_url(file_name))
    }

    /// Remove a file from the FTP server directory
    pub async fn remove_file(&self, file_name: &str) -> Result<(), String> {
        let file_path = self.root_dir.join(file_name);
        if file_path.exists() {
            tokio::fs::remove_file(&file_path)
                .await
                .map_err(|e| format!("Failed to remove file from FTP directory: {}", e))?;
            info!("Removed file from FTP server: {}", file_name);
        }
        Ok(())
    }
}

/// Get the local IP address by connecting to an external address
fn get_local_ip() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let local_addr = socket.local_addr().ok()?;
    Some(local_addr.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_ftp_server_file_url() {
        let temp_dir = tempdir().unwrap();
        let server = FtpServer::new(temp_dir.path().to_path_buf(), 2121);
        
        let url = server.get_file_url("test.txt");
        assert!(url.contains(":2121/test.txt"));
    }
}

