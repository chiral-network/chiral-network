use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
    pub percentage: f32,
    pub status: String,
}

pub struct GethDownloader {
    base_dir: PathBuf,
}

impl GethDownloader {
    pub fn new() -> Self {
        // Use executable's directory for both dev and prod - bin/ lives next to the exe
        let base_dir = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();

        GethDownloader { base_dir }
    }

    pub fn geth_path(&self) -> PathBuf {
        self.base_dir
            .join("bin")
            .join(if cfg!(target_os = "windows") {
                "geth.exe"
            } else {
                "geth"
            })
    }

    pub fn is_geth_installed(&self) -> bool {
        self.geth_path().exists()
    }

    fn get_download_url(&self) -> Result<String, String> {
        // Core-Geth v1.12.20 URLs for different platforms
        let url = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => "https://github.com/etclabscore/core-geth/releases/download/v1.12.20/core-geth-osx-v1.12.20.zip",
            ("macos", "x86_64") => "https://github.com/etclabscore/core-geth/releases/download/v1.12.20/core-geth-osx-v1.12.20.zip",
            ("linux", "x86_64") => "https://github.com/etclabscore/core-geth/releases/download/v1.12.20/core-geth-linux-v1.12.20.zip",
            ("linux", "aarch64") => "https://github.com/etclabscore/core-geth/releases/download/v1.12.20/core-geth-arm64-v1.12.20.zip",
            ("windows", "x86_64") => "https://github.com/etclabscore/core-geth/releases/download/v1.12.20/core-geth-win64-v1.12.20.zip",
            _ => return Err(format!("Unsupported platform: {} {}", std::env::consts::OS, std::env::consts::ARCH)),
        };

        Ok(url.to_string())
    }

    pub async fn download_geth<F>(&self, progress_callback: F) -> Result<(), String>
    where
        F: Fn(DownloadProgress) + Send + 'static,
    {
        if self.is_geth_installed() {
            return Ok(());
        }

        let url = self.get_download_url()?;
        let bin_dir = self.base_dir.join("bin");

        // Create bin directory if it doesn't exist
        fs::create_dir_all(&bin_dir)
            .map_err(|e| format!("Failed to create bin directory: {}", e))?;

        // Send initial progress
        progress_callback(DownloadProgress {
            downloaded: 0,
            total: 0,
            percentage: 0.0,
            status: "Starting download...".to_string(),
        });

        // Download the file with proper client configuration
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to download from {}: {}", url, e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Download failed with status: {}",
                response.status()
            ));
        }

        let total_size = response.content_length().unwrap_or(0);

        // Download with progress tracking
        let mut downloaded = 0u64;
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();

        use futures_util::StreamExt;
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| format!("Failed to read chunk: {}", e))?;
            downloaded += chunk.len() as u64;
            bytes.extend_from_slice(&chunk);

            let percentage = if total_size > 0 {
                (downloaded as f32 / total_size as f32) * 100.0
            } else {
                0.0
            };

            progress_callback(DownloadProgress {
                downloaded,
                total: total_size,
                percentage,
                status: format!("Downloading... {:.1} MB", downloaded as f32 / 1_048_576.0),
            });
        }

        progress_callback(DownloadProgress {
            downloaded: bytes.len() as u64,
            total: total_size,
            percentage: 100.0,
            status: "Download complete, extracting...".to_string(),
        });

        // Save and extract based on file type
        if url.ends_with(".tar.gz") {
            self.extract_tar_gz(&bytes, &bin_dir)?;
        } else if url.ends_with(".zip") {
            self.extract_zip(&bytes, &bin_dir)?;
        } else {
            return Err("Unsupported archive format".to_string());
        }

        // Make the binary executable on Unix systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let geth_path = self.geth_path();
            let mut perms = fs::metadata(&geth_path)
                .map_err(|e| format!("Failed to get geth metadata: {}", e))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&geth_path, perms)
                .map_err(|e| format!("Failed to set geth permissions: {}", e))?;
        }

        progress_callback(DownloadProgress {
            downloaded: total_size,
            total: total_size,
            percentage: 100.0,
            status: "Installation complete!".to_string(),
        });

        Ok(())
    }

    fn extract_tar_gz(&self, data: &[u8], output_dir: &Path) -> Result<(), String> {
        use flate2::read::GzDecoder;
        use std::io::Cursor;
        use tar::Archive;

        let cursor = Cursor::new(data);
        let tar = GzDecoder::new(cursor);
        let mut archive = Archive::new(tar);

        let mut found_geth = false;

        for entry in archive
            .entries()
            .map_err(|e| format!("Failed to read archive entries: {}", e))?
        {
            let mut entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
            let path = entry
                .path()
                .map_err(|e| format!("Failed to get entry path: {}", e))?;

            // Look for the geth binary (might be in a subdirectory)
            if let Some(file_name) = path.file_name() {
                if file_name == "geth" {
                    let geth_path = output_dir.join("geth");

                    let mut file = fs::File::create(&geth_path)
                        .map_err(|e| format!("Failed to create geth file: {}", e))?;
                    let _bytes_copied = std::io::copy(&mut entry, &mut file)
                        .map_err(|e| format!("Failed to write geth file: {}", e))?;

                    found_geth = true;
                    break;
                }
            }
        }

        if !found_geth {
            return Err("Could not find geth binary in archive".to_string());
        }

        Ok(())
    }

    fn extract_zip(&self, data: &[u8], output_dir: &Path) -> Result<(), String> {
        use std::io::Cursor;
        use zip::ZipArchive;

        let reader = Cursor::new(data);
        let mut archive =
            ZipArchive::new(reader).map_err(|e| format!("Failed to read zip archive: {}", e))?;

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| format!("Failed to read zip entry: {}", e))?;

            // Look for the geth.exe or geth (linux, mac) binary
            if file.name().ends_with("geth.exe") {
                let geth_path = output_dir.join("geth.exe");
                let mut outfile = fs::File::create(&geth_path)
                    .map_err(|e| format!("Failed to create geth.exe file: {}", e))?;
                std::io::copy(&mut file, &mut outfile)
                    .map_err(|e| format!("Failed to write geth.exe file: {}", e))?;
                break;
            } else if file.name().ends_with("geth") {
                let geth_path = output_dir.join("geth");
                let mut outfile = fs::File::create(&geth_path)
                    .map_err(|e| format!("Failed to create geth file: {}", e))?;
                std::io::copy(&mut file, &mut outfile)
                    .map_err(|e| format!("Failed to write geth file: {}", e))?;
                break;
            }
        }

        Ok(())
    }
}
