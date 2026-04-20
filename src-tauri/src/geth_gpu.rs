//! GPU mining — simplified ethminer wrapper.
//!
//! This is a deliberate rebuild of the previous GPU integration. The old
//! version was ~1500 lines: auto-download, CUDA/OpenCL backend detection,
//! fallback chains, per-device utilization throttling, complex stdout
//! parsing, auto-recovery retries. That accumulated complexity was the
//! biggest source of bloat in the old geth.rs.
//!
//! This version does exactly three things:
//!
//! 1. Locate ethminer on PATH or next to the geth binary. If not found,
//!    report unsupported; no auto-download, no package-manager poking.
//! 2. Enumerate GPU devices via `ethminer --list-devices` and parse
//!    the output into a flat list.
//! 3. Start ethminer as a subprocess pointed at our local geth over
//!    HTTP getwork. A background thread tails stdout for hashrate
//!    updates. Stop kills the child and reaps the thread.
//!
//! GPU utilization % is accepted and echoed back in status but not
//! enforced — ethminer doesn't have a direct "run at X% of GPU" knob,
//! and the previous implementation's pseudo-throttling was fragile.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

// ============================================================================
// Public types (consumed by the frontend Mining page)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuDevice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuMiningCapabilities {
    pub supported: bool,
    pub binary_path: Option<String>,
    pub devices: Vec<GpuDevice>,
    pub running: bool,
    pub active_devices: Vec<String>,
    pub utilization_percent: u8,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuMiningStatus {
    pub running: bool,
    pub hash_rate: u64,
    pub active_devices: Vec<String>,
    pub utilization_percent: u8,
    pub last_error: Option<String>,
}

// ============================================================================
// GpuMiner
// ============================================================================

pub struct GpuMiner {
    child: Option<Child>,
    binary_path: Option<PathBuf>,
    /// Parsed hash rate from ethminer stdout, in H/s. Updated by the
    /// background reader thread — lock-free read from status paths.
    hash_rate: Arc<AtomicU64>,
    /// Most recent error line from ethminer stdout/stderr.
    last_error: Arc<Mutex<Option<String>>>,
    active_devices: Vec<String>,
    utilization_percent: u8,
    reader_thread: Option<JoinHandle<()>>,
}

impl GpuMiner {
    pub fn new() -> Self {
        Self {
            child: None,
            binary_path: find_ethminer(),
            hash_rate: Arc::new(AtomicU64::new(0)),
            last_error: Arc::new(Mutex::new(None)),
            active_devices: Vec::new(),
            utilization_percent: 100,
            reader_thread: None,
        }
    }

    pub fn is_installed(&self) -> bool {
        self.binary_path.is_some()
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    /// Full capabilities snapshot — installed-ness, discovered devices,
    /// running state. Runs `ethminer --list-devices` each call, so don't
    /// poll it; the frontend only fetches it on the Mining page open.
    pub fn capabilities(&self) -> GpuMiningCapabilities {
        let devices = if self.is_installed() {
            self.list_devices().unwrap_or_default()
        } else {
            Vec::new()
        };
        GpuMiningCapabilities {
            supported: self.is_installed(),
            binary_path: self.binary_path.as_ref().map(|p| p.display().to_string()),
            devices,
            running: self.is_running(),
            active_devices: self.active_devices.clone(),
            utilization_percent: self.utilization_percent,
            last_error: self.last_error.lock().ok().and_then(|g| g.clone()),
        }
    }

    pub fn list_devices(&self) -> Result<Vec<GpuDevice>, String> {
        let path = self.binary_path.as_ref()
            .ok_or_else(|| "ethminer not installed".to_string())?;
        let out = Command::new(path)
            .arg("--list-devices")
            .output()
            .map_err(|e| format!("run ethminer: {e}"))?;
        // ethminer prints device info on stdout. Some builds print on
        // stderr; concatenate both.
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push('\n');
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        Ok(parse_device_list(&text))
    }

    pub fn start(
        &mut self,
        miner_address: &str,
        device_ids: Option<Vec<String>>,
        utilization_percent: Option<u8>,
    ) -> Result<(), String> {
        if self.child.is_some() {
            return Err("GPU mining is already running".to_string());
        }
        let path = self.binary_path.as_ref()
            .ok_or_else(|| "ethminer not installed — install it manually and restart the app".to_string())?;
        if miner_address.is_empty() {
            return Err("Miner address required to start GPU mining".to_string());
        }

        // ethminer getwork URL against local geth. The scheme prefix `http://`
        // (not `stratum://`) tells ethminer to use HTTP getwork, which is what
        // geth exposes on port 8545. The address-as-user is a convention
        // ethminer carries over from pool mining; geth ignores it because
        // the coinbase comes from geth's own --miner.etherbase flag.
        let url = format!("http://{miner_address}@127.0.0.1:8545");

        let mut cmd = Command::new(path);
        cmd.arg("-G").arg("-P").arg(&url);

        if let Some(ref devices) = device_ids {
            if !devices.is_empty() {
                // ethminer expects space-separated device indices. Works for
                // both CUDA and OpenCL — ethminer auto-selects backend based
                // on what was used to list them.
                cmd.arg("--opencl-devices").arg(devices.join(" "));
            }
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("spawn ethminer: {e}"))?;
        let stdout = child.stdout.take().ok_or("child stdout unavailable")?;
        let stderr = child.stderr.take();

        // Clear any stale state from a previous run.
        self.hash_rate.store(0, Ordering::Relaxed);
        if let Ok(mut guard) = self.last_error.lock() {
            *guard = None;
        }

        let hash_rate = Arc::clone(&self.hash_rate);
        let last_error = Arc::clone(&self.last_error);
        let thread = std::thread::spawn(move || {
            // Tail stdout line-by-line. Thread exits when ethminer closes
            // its stdout (i.e. the process exited) — no explicit shutdown
            // signal needed.
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                if let Some(rate) = parse_hashrate_line(&line) {
                    hash_rate.store(rate, Ordering::Relaxed);
                }
                if looks_like_error(&line) {
                    if let Ok(mut guard) = last_error.lock() {
                        *guard = Some(line);
                    }
                }
            }
        });

        // Tail stderr too, into the same error slot. Short-lived thread.
        if let Some(stderr) = stderr {
            let last_error = Arc::clone(&self.last_error);
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().flatten() {
                    if looks_like_error(&line) {
                        if let Ok(mut guard) = last_error.lock() {
                            *guard = Some(line);
                        }
                    }
                }
            });
        }

        self.active_devices = device_ids.unwrap_or_default();
        self.utilization_percent = utilization_percent.unwrap_or(100).clamp(1, 100);
        self.child = Some(child);
        self.reader_thread = Some(thread);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // Reader thread exits on stdout EOF after the child dies. Joining
        // blocks briefly so the hashrate state is consistent by the time
        // the next status call comes through.
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
        self.hash_rate.store(0, Ordering::Relaxed);
        self.active_devices.clear();
        Ok(())
    }

    pub fn status(&self) -> GpuMiningStatus {
        GpuMiningStatus {
            running: self.is_running(),
            hash_rate: self.hash_rate.load(Ordering::Relaxed),
            active_devices: self.active_devices.clone(),
            utilization_percent: self.utilization_percent,
            last_error: self.last_error.lock().ok().and_then(|g| g.clone()),
        }
    }
}

impl Default for GpuMiner {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for GpuMiner {
    fn drop(&mut self) {
        // Best-effort cleanup if the struct is dropped while mining.
        let _ = self.stop();
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn ethminer_filename() -> &'static str {
    if cfg!(windows) { "ethminer.exe" } else { "ethminer" }
}

/// Locate ethminer: PATH first, then the app's `bin/` directory (next to geth).
fn find_ethminer() -> Option<PathBuf> {
    let name = ethminer_filename();
    if let Ok(path_var) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for dir in path_var.split(sep) {
            let candidate = Path::new(dir).join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .map(|exe_dir| exe_dir.join("bin").join(name))
        .filter(|p| p.is_file())
}

/// Parse ethminer's `--list-devices` output into (id, name) pairs.
///
/// ethminer output varies slightly by version and backend. Both of these
/// shapes are common:
///
///   [CL]:
///   [0] NVIDIA GeForce RTX 3080
///       Memory: 10737418240
///
///   [CUDA]:
///   [0] NVIDIA GeForce RTX 3080 (cc 8.6)
///
/// We look for a line whose trimmed content starts with `[<n>]` where
/// `<n>` parses as an integer, and take the rest of the line as the name.
fn parse_device_list(output: &str) -> Vec<GpuDevice> {
    let mut out = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix('[') else { continue };
        let Some(idx_end) = rest.find(']') else { continue };
        let id_str = &rest[..idx_end];
        if id_str.parse::<u32>().is_err() {
            continue;
        }
        let name = rest[idx_end + 1..].trim();
        if !name.is_empty() {
            out.push(GpuDevice { id: id_str.to_string(), name: name.to_string() });
        }
    }
    out
}

/// Parse an ethminer stdout line's hashrate. Return total H/s or None.
///
/// Example lines:
///   m 15:22:08|main Speed 32.45 Mh/s gpu/0 32.45
///   i 15:22:22|ethminer Speed 1.23 Gh/s
fn parse_hashrate_line(line: &str) -> Option<u64> {
    let idx = line.find("Speed")?;
    let rest = line[idx + "Speed".len()..].trim_start();
    let mut parts = rest.split_whitespace();
    let number: f64 = parts.next()?.parse().ok()?;
    let unit = parts.next()?;
    let multiplier = if unit.starts_with("Gh") || unit.starts_with("GH") {
        1_000_000_000.0
    } else if unit.starts_with("Mh") || unit.starts_with("MH") {
        1_000_000.0
    } else if unit.starts_with("Kh") || unit.starts_with("KH") {
        1_000.0
    } else if unit.starts_with("H/") || unit.starts_with("h/") {
        1.0
    } else {
        return None;
    };
    Some((number * multiplier) as u64)
}

fn looks_like_error(line: &str) -> bool {
    // ethminer log levels: f (fatal), e (error), w (warn), i (info), m (miner).
    // We only surface fatal / error lines to the UI — warnings are common
    // during startup (kernel compile) and would create noise.
    let trimmed = line.trim_start();
    trimmed.starts_with("f ")
        || trimmed.starts_with("e ")
        || trimmed.contains("FATAL")
        || trimmed.contains("ERROR")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashrate_mh() {
        assert_eq!(
            parse_hashrate_line(" m  15:22:08|main  Speed 32.45 Mh/s gpu/0 32.45"),
            Some(32_450_000),
        );
    }

    #[test]
    fn hashrate_kh() {
        assert_eq!(parse_hashrate_line("Speed 1500 Kh/s"), Some(1_500_000));
    }

    #[test]
    fn hashrate_gh() {
        assert_eq!(parse_hashrate_line("Speed 2.5 Gh/s"), Some(2_500_000_000));
    }

    #[test]
    fn hashrate_line_without_speed_ignored() {
        assert_eq!(parse_hashrate_line("just a log line"), None);
    }

    #[test]
    fn hashrate_weird_unit_ignored() {
        // Future-proofing: unknown unit shouldn't claim hash-rate.
        assert_eq!(parse_hashrate_line("Speed 10 floops/s"), None);
    }

    #[test]
    fn parse_cuda_device_list() {
        let out = "[CUDA]:\n[0] NVIDIA GeForce RTX 3080 (cc 8.6)\n    Memory: 10737418240\n[1] NVIDIA GeForce RTX 3060\n";
        let devices = parse_device_list(out);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "0");
        assert_eq!(devices[0].name, "NVIDIA GeForce RTX 3080 (cc 8.6)");
        assert_eq!(devices[1].id, "1");
    }

    #[test]
    fn parse_opencl_device_list() {
        let out = "[CL]:\n[0] NVIDIA GeForce RTX 3080\n    Memory: 10737418240 bytes\n[1] AMD Radeon Pro 5700 XT\n";
        let devices = parse_device_list(out);
        assert_eq!(devices.len(), 2);
        assert!(devices[1].name.contains("AMD"));
    }

    #[test]
    fn parse_device_list_ignores_non_device_lines() {
        let out = "Random header\n[not a number] ignored\nblah [0] NVIDIA ignored too\n";
        let devices = parse_device_list(out);
        assert_eq!(devices.len(), 0);
    }

    #[test]
    fn error_classifier_flags_fatal_lines() {
        assert!(looks_like_error("f 15:22:08|main FATAL: kernel compile failed"));
        assert!(looks_like_error("e 15:22:08|main Device 0 crashed"));
        assert!(!looks_like_error("m 15:22:08|main Speed 32.45 Mh/s"));
        assert!(!looks_like_error("w 15:22:08|main Warn: slow"));
    }

    #[test]
    fn capabilities_unsupported_when_binary_missing() {
        let miner = GpuMiner {
            child: None,
            binary_path: None,
            hash_rate: Arc::new(AtomicU64::new(0)),
            last_error: Arc::new(Mutex::new(None)),
            active_devices: Vec::new(),
            utilization_percent: 100,
            reader_thread: None,
        };
        let cap = miner.capabilities();
        assert!(!cap.supported);
        assert!(cap.devices.is_empty());
        assert!(cap.binary_path.is_none());
    }
}
