use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use chrono::Local;
use tracing_subscriber::fmt::MakeWriter;

/// Configuration for file logging
#[derive(Clone, Debug)]
pub struct LogConfig {
    /// Path to the logs directory
    pub logs_dir: PathBuf,
    /// Maximum size of a single log file in MB
    pub max_log_size_mb: u64,
    /// Whether file logging is enabled
    pub enabled: bool,
}

impl LogConfig {
    pub fn new(logs_dir: impl AsRef<Path>, max_log_size_mb: u64, enabled: bool) -> Self {
        Self {
            logs_dir: logs_dir.as_ref().to_path_buf(),
            max_log_size_mb,
            enabled,
        }
    }
}

/// Custom file writer that handles log rotation
pub struct RotatingFileWriter {
    config: Arc<Mutex<LogConfig>>,
    current_file: Arc<Mutex<Option<File>>>,
    current_file_path: Arc<Mutex<Option<PathBuf>>>,
}

impl RotatingFileWriter {
    pub fn new(config: LogConfig) -> io::Result<Self> {
        // Create logs directory if it doesn't exist
        if config.enabled {
            fs::create_dir_all(&config.logs_dir)?;
        }

        Ok(Self {
            config: Arc::new(Mutex::new(config)),
            current_file: Arc::new(Mutex::new(None)),
            current_file_path: Arc::new(Mutex::new(None)),
        })
    }

    /// Update the logger configuration
    pub fn update_config(&self, new_config: LogConfig) -> io::Result<()> {
        let mut config = self.config.lock().unwrap();
        
        // If logs directory changed or logging was just enabled, create the new directory
        if new_config.enabled && (!config.enabled || new_config.logs_dir != config.logs_dir) {
            fs::create_dir_all(&new_config.logs_dir)?;
        }

        // If logging is being disabled, close the current file
        if !new_config.enabled && config.enabled {
            let mut current_file = self.current_file.lock().unwrap();
            *current_file = None;
            let mut current_path = self.current_file_path.lock().unwrap();
            *current_path = None;
        }

        *config = new_config;
        Ok(())
    }

    fn get_new_log_file_path(&self, config: &LogConfig) -> PathBuf {
        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
        config.logs_dir.join(format!("chiral_{}.log", timestamp))
    }

    fn should_rotate(&self, config: &LogConfig) -> io::Result<bool> {
        let current_path_lock = self.current_file_path.lock().unwrap();
        
        if let Some(path) = current_path_lock.as_ref() {
            if let Ok(metadata) = fs::metadata(path) {
                let size_mb = metadata.len() / (1024 * 1024);
                return Ok(size_mb >= config.max_log_size_mb);
            }
        }
        
        Ok(false)
    }

    fn rotate_if_needed(&self, config: &LogConfig) -> io::Result<()> {
        if self.should_rotate(config)? {
            let mut current_file = self.current_file.lock().unwrap();
            let mut current_path = self.current_file_path.lock().unwrap();
            
            // Close current file
            *current_file = None;
            *current_path = None;
        }

        Ok(())
    }

    fn cleanup_old_logs(&self, config: &LogConfig) -> io::Result<()> {
        // Get all log files sorted by modification time
        let mut log_files: Vec<_> = fs::read_dir(&config.logs_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.path().extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext == "log")
                    .unwrap_or(false)
            })
            .collect();

        // Sort by modification time (newest first)
        log_files.sort_by_key(|entry| {
            entry.metadata()
                .and_then(|m| m.modified())
                .ok()
        });
        log_files.reverse();

        // Calculate total size
        let mut total_size_mb = 0u64;
        let max_total_size_mb = config.max_log_size_mb * 10; // Keep max 10x the individual file limit

        for (idx, entry) in log_files.iter().enumerate() {
            if let Ok(metadata) = entry.metadata() {
                let file_size_mb = metadata.len() / (1024 * 1024);
                total_size_mb += file_size_mb;

                // Delete old files if we exceed the total limit (but keep at least the newest file)
                if idx > 0 && total_size_mb > max_total_size_mb {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }

        Ok(())
    }

    fn get_or_create_file(&self) -> io::Result<()> {
        let config = self.config.lock().unwrap();
        
        if !config.enabled {
            return Ok(());
        }

        let mut current_file = self.current_file.lock().unwrap();
        let mut current_path = self.current_file_path.lock().unwrap();

        // If we don't have a file or need to rotate, create a new one
        if current_file.is_none() {
            let new_path = self.get_new_log_file_path(&config);
            let file = File::create(&new_path)?;
            *current_file = Some(file);
            *current_path = Some(new_path);
        }

        Ok(())
    }

    pub fn current_log_file_path(&self) -> Option<PathBuf> {
        let path = self.current_file_path.lock().unwrap();
        path.clone()
    }
}



impl Write for RotatingFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let config = self.config.lock().unwrap();
        
        if !config.enabled {
            // If logging is disabled, just pretend we wrote the data
            return Ok(buf.len());
        }

        drop(config); // Release the lock before potentially rotating

        // Check if rotation is needed
        self.rotate_if_needed(&self.config.lock().unwrap())?;
        
        // Clean up old logs periodically (we could optimize this to not run every write)
        let _ = self.cleanup_old_logs(&self.config.lock().unwrap());

        // Ensure we have a file
        self.get_or_create_file()?;

        // Write to the current file
        let mut current_file = self.current_file.lock().unwrap();
        if let Some(ref mut file) = *current_file {
            file.write(buf)
        } else {
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut current_file = self.current_file.lock().unwrap();
        if let Some(ref mut file) = *current_file {
            file.flush()
        } else {
            Ok(())
        }
    }
}

// Thread-safe wrapper
#[derive(Clone)]
pub struct ThreadSafeWriter {
    inner: Arc<Mutex<RotatingFileWriter>>,
}

impl ThreadSafeWriter {
    pub fn new(writer: RotatingFileWriter) -> Self {
        Self {
            inner: Arc::new(Mutex::new(writer)),
        }
    }

    pub fn update_config(&self, config: LogConfig) -> io::Result<()> {
        let writer = self.inner.lock().unwrap();
        writer.update_config(config)
    }

    pub fn current_log_file_path(&self) -> Option<PathBuf> {
        let writer = self.inner.lock().unwrap();
        writer.current_log_file_path()
    }
}

impl Write for ThreadSafeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut writer = self.inner.lock().unwrap();
        writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut writer = self.inner.lock().unwrap();
        writer.flush()
    }
}

// Implement MakeWriter for tracing_subscriber compatibility
impl<'a> MakeWriter<'a> for ThreadSafeWriter {
    type Writer = ThreadSafeWriterGuard;

    fn make_writer(&'a self) -> Self::Writer {
        ThreadSafeWriterGuard {
            inner: self.inner.clone(),
        }
    }
}

// Guard struct that implements Write for the MakeWriter trait
pub struct ThreadSafeWriterGuard {
    inner: Arc<Mutex<RotatingFileWriter>>,
}

impl Write for ThreadSafeWriterGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut writer = self.inner.lock().unwrap();
        writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut writer = self.inner.lock().unwrap();
        writer.flush()
    }
}
