use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::Mutex;
use tracing::{info, error, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TunnelInfo {
    pub is_active: bool,
    pub public_url: Option<String>,
    pub local_port: u16,
    pub tunnel_type: String, // "localtunnel", "ngrok", "cloudflared", "bore", "self_hosted"
    pub provider: String,
    pub status: String, // "connecting", "connected", "failed", "stopped"
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelProvider {
    Localtunnel,
    Ngrok,
    Cloudflared,
    Bore,
    SelfHosted,
}

impl TunnelProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            TunnelProvider::Localtunnel => "localtunnel",
            TunnelProvider::Ngrok => "ngrok",
            TunnelProvider::Cloudflared => "cloudflared",
            TunnelProvider::Bore => "bore",
            TunnelProvider::SelfHosted => "self_hosted",
        }
    }
}

/// Global tunnel state manager
pub struct TunnelManager {
    info: Arc<Mutex<TunnelInfo>>,
}

impl TunnelManager {
    pub fn new() -> Self {
        Self {
            info: Arc::new(Mutex::new(TunnelInfo {
                is_active: false,
                public_url: None,
                local_port: 8080,
                tunnel_type: "ngrok".to_string(), // Default to ngrok for better reliability
                provider: "ngrok".to_string(),
                status: "stopped".to_string(),
                error_message: None,
            })),
        }
    }

    /// Check which tunnel providers are available
    pub async fn check_available_providers(&self) -> Vec<TunnelProvider> {
        let mut available = Vec::new();
        
        // Check ngrok
        if Command::new("ngrok").arg("version").output().is_ok() {
            available.push(TunnelProvider::Ngrok);
        }
        
        // Check cloudflared
        if Command::new("cloudflared").arg("version").output().is_ok() {
            available.push(TunnelProvider::Cloudflared);
        }
        
        // Check bore
        if Command::new("bore").arg("--help").output().is_ok() {
            available.push(TunnelProvider::Bore);
        }
        
        // Check localtunnel (fallback)
        if Command::new("lt").arg("--version").output().is_ok() {
            available.push(TunnelProvider::Localtunnel);
        }
        
        // Self-hosted is always available
        available.push(TunnelProvider::SelfHosted);
        
        available
    }

    /// Start tunnel with specified provider
    pub async fn start_tunnel(&self, port: u16, provider: TunnelProvider) -> Result<String, String> {
        let mut info_guard = self.info.lock().await;
        
        // Update status
        info_guard.status = "connecting".to_string();
        info_guard.error_message = None;
        info_guard.local_port = port;
        info_guard.tunnel_type = provider.as_str().to_string();
        info_guard.provider = provider.as_str().to_string();
        
        drop(info_guard);
        
        match provider {
            TunnelProvider::Ngrok => self.start_ngrok(port).await,
            TunnelProvider::Cloudflared => self.start_cloudflared(port).await,
            TunnelProvider::Bore => self.start_bore(port).await,
            TunnelProvider::Localtunnel => self.start_localtunnel(port).await,
            TunnelProvider::SelfHosted => self.start_self_hosted(port).await,
        }
    }

    /// Start ngrok tunnel (most reliable)
    pub async fn start_ngrok(&self, port: u16) -> Result<String, String> {
        let mut info_guard = self.info.lock().await;

        // Check if already running
        if info_guard.is_active {
            if let Some(url) = &info_guard.public_url {
                return Ok(url.clone());
            }
        }

        // Stop any existing ngrok processes
        let _ = Command::new("pkill").arg("-f").arg("ngrok").output();

        info!("🌐 Starting ngrok tunnel on port {}", port);

        // Check if ngrok is installed
        let ngrok_check = Command::new("ngrok").arg("version").output();
        if ngrok_check.is_err() || !ngrok_check.unwrap().status.success() {
            return Err("Ngrok is not installed. Please install it from https://ngrok.com/download".to_string());
        }

        // Start ngrok with HTTP tunnel
        let mut child = TokioCommand::new("ngrok")
            .arg("http")
            .arg(port.to_string())
            .arg("--log")
            .arg("stdout")
            .arg("--log-format")
            .arg("logfmt")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start ngrok: {}", e))?;

        // Get stdout and stderr
        let stdout = child
            .stdout
            .take()
            .ok_or("Failed to capture ngrok stdout")?;

        let stderr = child
            .stderr
            .take()
            .ok_or("Failed to capture ngrok stderr")?;

        // Spawn task to read ngrok output
        let info_clone = self.info.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                info!("📝 Ngrok stdout: {}", line);

                // Look for tunnel URL in ngrok output
                if line.contains("started tunnel") && line.contains("url=") {
                    // Extract URL from logfmt format
                    for part in line.split_whitespace() {
                        if part.starts_with("url=") {
                            let url = part.strip_prefix("url=").unwrap_or("");
                            if url.starts_with("https://") || url.starts_with("http://") {
                                info!("✅ Ngrok tunnel established: {}", url);
                                
                                let mut info = info_clone.lock().await;
                                if info.public_url.is_none() {
                                    info.public_url = Some(url.to_string());
                                    info.is_active = true;
                                    info.status = "connected".to_string();
                                }
                                break;
                            }
                        }
                    }
                }
            }
        });

        // Also read stderr for errors
        let info_clone2 = self.info.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.contains("error") || line.contains("Error") || line.contains("ERROR") {
                    error!("❌ Ngrok error: {}", line);
                    
                    let mut info = info_clone2.lock().await;
                    info.status = "failed".to_string();
                    info.error_message = Some(line.clone());
                    
                    // If it's an authentication error, fail immediately
                    if line.contains("authentication failed") || line.contains("authtoken") {
                        info!("🚫 Ngrok authentication failed, will try next provider");
                        break;
                    }
                }
            }
        });

        // Store process info
        info_guard.local_port = port;
        info_guard.tunnel_type = "ngrok".to_string();
        info_guard.provider = "ngrok".to_string();
        
        let pid = child.id().ok_or("Failed to get process ID")?;
        info!("🚀 Started ngrok with PID: {}", pid);
        
        drop(info_guard);

        // Wait up to 10 seconds for URL (reduced timeout for faster fallback)
        for i in 0..20 {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let info = self.info.lock().await;
            if let Some(url) = &info.public_url {
                info!("🎉 Successfully established ngrok tunnel: {}", url);
                return Ok(url.clone());
            }

            if i == 5 {
                info!("⏳ Still waiting for ngrok tunnel URL...");
            } else if i == 15 {
                info!("⏳ Still waiting for ngrok tunnel URL... (this can take up to 10 seconds)");
            } else if i == 18 {
                info!("⏳ Final attempt to get ngrok tunnel URL...");
            }
        }

        // Timeout
        let mut info = self.info.lock().await;
        info.status = "failed".to_string();
        info.error_message = Some("Timeout waiting for ngrok tunnel URL".to_string());
        
        Err("Timeout waiting for ngrok tunnel URL. Check ngrok installation and try again.".to_string())
    }

    /// Start cloudflared tunnel (very reliable and fast)
    pub async fn start_cloudflared(&self, port: u16) -> Result<String, String> {
        let mut info_guard = self.info.lock().await;

        if info_guard.is_active {
            if let Some(url) = &info_guard.public_url {
                return Ok(url.clone());
            }
        }

        // Stop any existing cloudflared processes
        let _ = Command::new("pkill").arg("-f").arg("cloudflared").output();

        info!("🌐 Starting cloudflared tunnel on port {}", port);

        // Check if cloudflared is installed
        let cf_check = Command::new("cloudflared").arg("version").output();
        if cf_check.is_err() || !cf_check.unwrap().status.success() {
            return Err("Cloudflared is not installed. Please install it from https://github.com/cloudflare/cloudflared/releases".to_string());
        }

        // Start cloudflared tunnel
        let mut child = TokioCommand::new("cloudflared")
            .arg("tunnel")
            .arg("--url")
            .arg(format!("http://localhost:{}", port))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start cloudflared: {}", e))?;

        let stdout = child.stdout.take().ok_or("Failed to capture cloudflared stdout")?;
        let stderr = child.stderr.take().ok_or("Failed to capture cloudflared stderr")?;

        // Parse cloudflared output
        let info_clone = self.info.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                info!("📝 Cloudflared stdout: {}", line);

                // Look for tunnel URL
                if line.contains("https://") && line.contains(".trycloudflare.com") {
                    let url = line.trim();
                    info!("✅ Cloudflared tunnel established: {}", url);
                    
                    let mut info = info_clone.lock().await;
                    if info.public_url.is_none() {
                        info.public_url = Some(url.to_string());
                        info.is_active = true;
                        info.status = "connected".to_string();
                    }
                }
            }
        });

        // Handle stderr
        let info_clone2 = self.info.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.contains("error") || line.contains("Error") || line.contains("ERROR") {
                    error!("❌ Cloudflared error: {}", line);
                    
                    let mut info = info_clone2.lock().await;
                    info.status = "failed".to_string();
                    info.error_message = Some(line);
                }
            }
        });

        info_guard.local_port = port;
        info_guard.tunnel_type = "cloudflared".to_string();
        info_guard.provider = "cloudflared".to_string();
        
        let pid = child.id().ok_or("Failed to get process ID")?;
        info!("🚀 Started cloudflared with PID: {}", pid);
        
        drop(info_guard);

        // Wait for URL
        for i in 0..60 {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let info = self.info.lock().await;
            if let Some(url) = &info.public_url {
                info!("🎉 Successfully established cloudflared tunnel: {}", url);
                return Ok(url.clone());
            }

            if i == 20 {
                info!("⏳ Still waiting for cloudflared tunnel URL...");
            } else if i == 40 {
                info!("⏳ Still waiting for cloudflared tunnel URL... (this can take up to 30 seconds)");
            }
        }

        let mut info = self.info.lock().await;
        info.status = "failed".to_string();
        info.error_message = Some("Timeout waiting for cloudflared tunnel URL".to_string());
        
        Err("Timeout waiting for cloudflared tunnel URL. Check cloudflared installation and try again.".to_string())
    }

    /// Start bore tunnel (simple and reliable)
    pub async fn start_bore(&self, port: u16) -> Result<String, String> {
        let mut info_guard = self.info.lock().await;

        if info_guard.is_active {
            if let Some(url) = &info_guard.public_url {
                return Ok(url.clone());
            }
        }

        // Stop any existing bore processes
        let _ = Command::new("pkill").arg("-f").arg("bore").output();

        info!("🌐 Starting bore tunnel on port {}", port);

        // Check if bore is installed
        let bore_check = Command::new("bore").arg("--help").output();
        if bore_check.is_err() || !bore_check.unwrap().status.success() {
            return Err("Bore is not installed. Please install it with: cargo install bore-cli".to_string());
        }

        // Start bore tunnel
        let mut child = TokioCommand::new("bore")
            .arg("local")
            .arg(port.to_string())
            .arg("--to")
            .arg("bore.pub")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start bore: {}", e))?;

        let stdout = child.stdout.take().ok_or("Failed to capture bore stdout")?;
        let stderr = child.stderr.take().ok_or("Failed to capture bore stderr")?;

        // Parse bore output
        let info_clone = self.info.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                info!("📝 Bore stdout: {}", line);

                // Look for tunnel URL
                if line.contains("bore.pub") {
                    let url = if line.starts_with("https://") || line.starts_with("http://") {
                        line.trim().to_string()
                    } else {
                        format!("https://{}", line.trim())
                    };
                    
                    info!("✅ Bore tunnel established: {}", url);
                    
                    let mut info = info_clone.lock().await;
                    if info.public_url.is_none() {
                        info.public_url = Some(url.clone());
                        info.is_active = true;
                        info.status = "connected".to_string();
                    }
                }
            }
        });

        // Handle stderr
        let info_clone2 = self.info.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.contains("error") || line.contains("Error") || line.contains("ERROR") {
                    error!("❌ Bore error: {}", line);
                    
                    let mut info = info_clone2.lock().await;
                    info.status = "failed".to_string();
                    info.error_message = Some(line);
                }
            }
        });

        info_guard.local_port = port;
        info_guard.tunnel_type = "bore".to_string();
        info_guard.provider = "bore".to_string();
        
        let pid = child.id().ok_or("Failed to get process ID")?;
        info!("🚀 Started bore with PID: {}", pid);
        
        drop(info_guard);

        // Wait for URL
        for i in 0..40 {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let info = self.info.lock().await;
            if let Some(url) = &info.public_url {
                info!("🎉 Successfully established bore tunnel: {}", url);
                return Ok(url.clone());
            }

            if i == 10 {
                info!("⏳ Still waiting for bore tunnel URL...");
            } else if i == 30 {
                info!("⏳ Still waiting for bore tunnel URL... (this can take up to 20 seconds)");
            }
        }

        let mut info = self.info.lock().await;
        info.status = "failed".to_string();
        info.error_message = Some("Timeout waiting for bore tunnel URL".to_string());
        
        Err("Timeout waiting for bore tunnel URL. Check bore installation and try again.".to_string())
    }

    /// Start self-hosted tunnel (most private)
    pub async fn start_self_hosted(&self, port: u16) -> Result<String, String> {
        // For self-hosted, we'll use a simple HTTP server with instructions
        let mut info_guard = self.info.lock().await;
        
        info_guard.local_port = port;
        info_guard.tunnel_type = "self_hosted".to_string();
        info_guard.provider = "self_hosted".to_string();
        info_guard.status = "connected".to_string();
        info_guard.is_active = true;
        info_guard.public_url = Some(format!("http://localhost:{}", port));
        
        info!("🏠 Self-hosted tunnel active on port {}", port);
        info!("💡 For external access, configure port forwarding on your router:");
        info!("   - Forward external port 8080 to internal port {}", port);
        info!("   - Your public URL will be: http://YOUR_PUBLIC_IP:8080");
        
        Ok(format!("http://localhost:{}", port))
    }

    /// Start localtunnel for the given port (fallback)
    pub async fn start_localtunnel(&self, port: u16) -> Result<String, String> {
        let mut info_guard = self.info.lock().await;

        // Check if already running
        if info_guard.is_active {
            if let Some(url) = &info_guard.public_url {
                return Ok(url.clone());
            }
        }

        // Stop any existing localtunnel processes
        let _ = Command::new("pkill").arg("-f").arg("^lt .*--port").output();
        let _ = Command::new("pkill").arg("-f").arg("localtunnel").output();

        info!("🌐 Starting localtunnel on port {}", port);

        // Check if localtunnel is installed
        let lt_check = Command::new("which").arg("lt").output();

        if lt_check.is_err() || !lt_check.unwrap().status.success() {
            return Err("Localtunnel (lt) is not installed. Please install it with: npm install -g localtunnel".to_string());
        }

        // Start localtunnel process with additional options for better output
        // Try with subdomain first for more predictable output
        let mut child = TokioCommand::new("lt")
            .arg("--port")
            .arg(port.to_string())
            .arg("--print-requests")
            .arg("--local-host")
            .arg("127.0.0.1")
            .arg("--subdomain")
            .arg(format!("chiral-{}", port)) // Use a predictable subdomain
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start localtunnel: {}", e))?;

        // Get stdout and stderr to read the URL
        let stdout = child
            .stdout
            .take()
            .ok_or("Failed to capture localtunnel stdout")?;

        let stderr = child
            .stderr
            .take()
            .ok_or("Failed to capture localtunnel stderr")?;

        // Spawn a task to read the URL from stdout
        let info_clone = self.info.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                info!("📝 Localtunnel stdout: {}", line);

                // Try multiple patterns to find the URL
                let url_patterns = [
                    "your url is:",
                    "tunnel established at:",
                    "tunnel url:",
                    "public url:",
                    "url:",
                    "tunnel:",
                    "https://",
                    "http://",
                    "localtunnel.me",
                    "loca.lt"
                ];

                for pattern in &url_patterns {
                    if line.contains(pattern) {
                        // Extract URL using different methods
                        let url = if pattern == &"your url is:" {
                            line.split("your url is:").nth(1)
                                .and_then(|s| s.trim().split_whitespace().next())
                        } else if pattern == &"tunnel established at:" {
                            line.split("tunnel established at:").nth(1)
                                .and_then(|s| s.trim().split_whitespace().next())
                        } else if pattern == &"tunnel url:" {
                            line.split("tunnel url:").nth(1)
                                .and_then(|s| s.trim().split_whitespace().next())
                        } else if pattern == &"public url:" {
                            line.split("public url:").nth(1)
                                .and_then(|s| s.trim().split_whitespace().next())
                        } else if pattern == &"url:" {
                            line.split("url:").nth(1)
                                .and_then(|s| s.trim().split_whitespace().next())
                        } else if pattern == &"tunnel:" {
                            line.split("tunnel:").nth(1)
                                .and_then(|s| s.trim().split_whitespace().next())
                        } else if pattern == &"localtunnel.me" || pattern == &"loca.lt" {
                            // Find any word containing these domains
                            line.split_whitespace()
                                .find(|word| word.contains("localtunnel.me") || word.contains("loca.lt"))
                        } else {
                            // For https:// or http:// patterns, find the first URL
                            line.split_whitespace()
                                .find(|word| word.starts_with("https://") || word.starts_with("http://"))
                        };

                        if let Some(url_str) = url {
                            let clean_url = url_str.trim_end_matches(|c| c == ',' || c == '.' || c == ';' || c == '\n' || c == '\r').to_string();
                            
                            // Validate URL format
                            if clean_url.starts_with("https://") || clean_url.starts_with("http://") {
                                info!("✅ Tunnel established: {}", clean_url);

                                let mut info = info_clone.lock().await;
                                if info.public_url.is_none() {
                                    info.public_url = Some(clean_url.clone());
                                    info.is_active = true;
                                }
                                break;
                            }
                        }
                    }
                }
            }
        });

        // Also read stderr for any errors and potential URL output
        let info_clone2 = self.info.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                info!("📝 Localtunnel stderr: {}", line);

                // Check for error messages
                if line.contains("error") || line.contains("Error") || line.contains("ERROR") {
                    error!("❌ Localtunnel error: {}", line);
                }

                // Some versions output URL to stderr - check for various patterns
                let stderr_patterns = [
                    "https://",
                    "http://",
                    "localtunnel.me",
                    "loca.lt",
                    "your url is:",
                    "tunnel established at:",
                    "tunnel url:",
                    "public url:",
                    "url:",
                    "tunnel:"
                ];
                
                for pattern in &stderr_patterns {
                    if line.contains(pattern) {
                        // Extract URL using similar logic as stdout
                        let url = if pattern == &"your url is:" {
                            line.split("your url is:").nth(1)
                                .and_then(|s| s.trim().split_whitespace().next())
                        } else if pattern == &"tunnel established at:" {
                            line.split("tunnel established at:").nth(1)
                                .and_then(|s| s.trim().split_whitespace().next())
                        } else if pattern == &"tunnel url:" {
                            line.split("tunnel url:").nth(1)
                                .and_then(|s| s.trim().split_whitespace().next())
                        } else if pattern == &"public url:" {
                            line.split("public url:").nth(1)
                                .and_then(|s| s.trim().split_whitespace().next())
                        } else if pattern == &"url:" {
                            line.split("url:").nth(1)
                                .and_then(|s| s.trim().split_whitespace().next())
                        } else if pattern == &"tunnel:" {
                            line.split("tunnel:").nth(1)
                                .and_then(|s| s.trim().split_whitespace().next())
                        } else if pattern == &"localtunnel.me" || pattern == &"loca.lt" {
                            line.split_whitespace()
                                .find(|word| word.contains("localtunnel.me") || word.contains("loca.lt"))
                        } else {
                            line.split_whitespace()
                                .find(|word| word.starts_with("https://") || word.starts_with("http://"))
                        };

                        if let Some(url_str) = url {
                            let clean_url = url_str.trim_end_matches(|c| c == ',' || c == '.' || c == ';' || c == '\n' || c == '\r').to_string();
                            
                            // Validate URL format
                            if clean_url.starts_with("https://") || clean_url.starts_with("http://") {
                                info!("✅ Tunnel established (from stderr): {}", clean_url);

                                let mut info = info_clone2.lock().await;
                                if info.public_url.is_none() {
                                    info.public_url = Some(clean_url.clone());
                                    info.is_active = true;
                                }
                                break;
                            }
                        }
                    }
                }
            }
        });

        // Store process info
        info_guard.local_port = port;
        info_guard.tunnel_type = "localtunnel".to_string();
        
        // Get process ID for tracking
        let pid = child.id().ok_or("Failed to get process ID")?;
        info!("🚀 Started localtunnel with PID: {}", pid);
        
        // We can't store tokio::process::Child in std::process::Child
        // So we'll track it differently - the process will be managed by the spawned tasks

        // Wait a bit for the URL to be captured
        drop(info_guard);

        // Wait up to 45 seconds for URL (increased timeout)
        for i in 0..90 {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let info = self.info.lock().await;
            if let Some(url) = &info.public_url {
                info!("🎉 Successfully established tunnel: {}", url);
                return Ok(url.clone());
            }

            if i == 10 {
                info!("⏳ Still waiting for tunnel URL... (this can take 10-20 seconds)");
            } else if i == 30 {
                info!("⏳ Still waiting for tunnel URL... (this can take up to 30 seconds)");
            } else if i == 60 {
                info!("⏳ Still waiting for tunnel URL... (this can take up to 45 seconds)");
            } else if i == 80 {
                info!("⏳ Final attempt to get tunnel URL...");
            }
        }

        // Try to get more debug info
        let info = self.info.lock().await;
        if info.is_active {
            info!("⚠️ Tunnel is active but no URL captured. This might be a parsing issue.");
            info!("💡 Try running 'lt --port {} --print-requests' manually to see the output format", port);
        } else {
            info!("⚠️ Tunnel failed to establish. Check localtunnel installation and network connectivity.");
            info!("💡 Make sure localtunnel is installed: npm install -g localtunnel");
            info!("💡 Test manually: lt --port {} --print-requests", port);
        }

        // Kill any remaining localtunnel processes
        let _ = Command::new("pkill").arg("-f").arg("^lt .*--port").output();
        let _ = Command::new("pkill").arg("-f").arg("localtunnel").output();

        Err(format!("Timeout waiting for tunnel URL after 45 seconds. Localtunnel may be taking longer than expected or there might be a network issue. Try running 'lt --port {}' manually to test.", port))
    }

    /// Stop the tunnel
    pub async fn stop(&self) -> Result<(), String> {
        let mut info_guard = self.info.lock().await;

        info!("🛑 Stopping tunnel...");

        // Kill all tunnel processes
        let _ = Command::new("pkill").arg("-f").arg("ngrok").output();
        let _ = Command::new("pkill").arg("-f").arg("cloudflared").output();
        let _ = Command::new("pkill").arg("-f").arg("bore").output();
        let _ = Command::new("pkill").arg("-f").arg("^lt .*--port").output();
        let _ = Command::new("pkill").arg("-f").arg("localtunnel").output();

        info_guard.is_active = false;
        info_guard.public_url = None;
        info_guard.status = "stopped".to_string();
        info_guard.error_message = None;

        info!("✅ Tunnel stopped");

        Ok(())
    }

    /// Get current tunnel status
    pub async fn get_info(&self) -> TunnelInfo {
        self.info.lock().await.clone()
    }
}

// Global tunnel manager instance
lazy_static::lazy_static! {
    pub static ref TUNNEL_MANAGER: TunnelManager = TunnelManager::new();
}
