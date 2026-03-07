use clap::{Parser, Subcommand};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chiral_network::dht;
use chiral_network::drive_storage;
use chiral_network::drive_storage::DriveItem;
use chiral_network::rating_storage::{self, compute_reputation_for_wallet, RatingState, LOOKBACK_SECS};

#[derive(Parser, Debug)]
#[command(name = "chiral")]
#[command(about = "Chiral Network headless CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Daemon {
        #[command(subcommand)]
        cmd: DaemonCommand,
    },
    Settings {
        #[command(subcommand)]
        cmd: SettingsCommand,
    },
    Network {
        #[command(subcommand)]
        cmd: NetworkCommand,
    },
    Reputation {
        #[command(subcommand)]
        cmd: ReputationCommand,
    },
    Diagnostics {
        #[command(subcommand)]
        cmd: DiagnosticsCommand,
    },
    Wallet {
        #[command(subcommand)]
        cmd: WalletCommand,
    },
    Account {
        #[command(subcommand)]
        cmd: AccountCommand,
    },
    Dht {
        #[command(subcommand)]
        cmd: DhtCommand,
    },
    Download {
        #[command(subcommand)]
        cmd: DownloadCommand,
    },
    Drive {
        #[command(subcommand)]
        cmd: DriveCommand,
    },
    #[command(name = "drop")]
    ChiralDrop {
        #[command(subcommand)]
        cmd: DropCommand,
    },
    Hosting {
        #[command(subcommand)]
        cmd: HostingCommand,
    },
    Market {
        #[command(subcommand)]
        cmd: MarketCommand,
    },
    Mining {
        #[command(subcommand)]
        cmd: MiningCommand,
    },
    Geth {
        #[command(subcommand)]
        cmd: GethCommand,
    },
}

#[derive(Subcommand, Debug)]
enum DaemonCommand {
    Start {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Stop,
    Status {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
}

#[derive(Subcommand, Debug)]
enum SettingsCommand {
    Get {
        key: Option<String>,
    },
    Set {
        key: String,
        value: String,
    },
    Reset,
    Path,
}

#[derive(Subcommand, Debug)]
enum NetworkCommand {
    Bootstrap,
    Status {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
}

#[derive(Subcommand, Debug)]
enum ReputationCommand {
    Show {
        wallet: String,
    },
    Batch {
        wallets: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
enum DiagnosticsCommand {
    Report {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
}

#[derive(Subcommand, Debug)]
enum WalletCommand {
    Create,
    Import,
    Login,
    Show,
    Export,
}

#[derive(Subcommand, Debug)]
enum AccountCommand {
    Balance,
    Send,
    History,
    Meta,
}

#[derive(Subcommand, Debug)]
enum DhtCommand {
    Put,
    Get,
    Echo,
}

#[derive(Subcommand, Debug)]
enum DownloadCommand {
    Search,
    Start,
    Cost,
    List,
    History,
    Watch,
}

#[derive(Subcommand, Debug)]
enum DriveCommand {
    Ls {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        parent_id: Option<String>,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Tree,
    Mkdir {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        parent_id: Option<String>,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Upload,
    Rename,
    Move,
    Delete {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        item_id: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Star {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        item_id: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Unstar {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        item_id: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Visibility,
    Share,
    Publish,
    Unpublish,
    ExportTorrent,
}

#[derive(Subcommand, Debug)]
enum DropCommand {
    Peers,
    Send,
    SendPaid,
    Inbox,
    Accept,
    Decline,
    History,
}

#[derive(Subcommand, Debug)]
enum HostingCommand {
    Server,
    Site,
    PublishRelay,
    UnpublishRelay,
}

#[derive(Subcommand, Debug)]
enum MarketCommand {
    Advertise,
    Unadvertise,
    Browse,
    Propose,
    Agreements,
    Respond,
    Cancel,
    Cleanup,
}

#[derive(Subcommand, Debug)]
enum MiningCommand {
    Install,
    Start,
    Stop,
    Status,
    Blocks,
}

#[derive(Subcommand, Debug)]
enum GethCommand {
    Start,
    Stop,
    Status,
    Logs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct NotificationsConfig {
    download_complete: bool,
    download_failed: bool,
    peer_connected: bool,
    peer_disconnected: bool,
    mining_block: bool,
    payment_received: bool,
    network_status: bool,
    file_shared: bool,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            download_complete: true,
            download_failed: true,
            peer_connected: false,
            peer_disconnected: false,
            mining_block: true,
            payment_received: true,
            network_status: true,
            file_shared: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct HostingConfig {
    enabled: bool,
    max_storage_bytes: u64,
    price_per_mb_per_day_wei: String,
    min_deposit_wei: String,
}

impl Default for HostingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_storage_bytes: 10 * 1024 * 1024 * 1024,
            price_per_mb_per_day_wei: "1000000000000000".to_string(),
            min_deposit_wei: "100000000000000000".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct DaemonConfig {
    auto_start: bool,
    port: u16,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            auto_start: false,
            port: 9419,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct HeadlessConfig {
    theme: String,
    color_theme: String,
    nav_style: String,
    reduced_motion: bool,
    compact_mode: bool,
    download_directory: String,
    notifications: NotificationsConfig,
    hosting: HostingConfig,
    daemon: DaemonConfig,
}

impl Default for HeadlessConfig {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            color_theme: "blue".to_string(),
            nav_style: "navbar".to_string(),
            reduced_motion: false,
            compact_mode: false,
            download_directory: String::new(),
            notifications: NotificationsConfig::default(),
            hosting: HostingConfig::default(),
            daemon: DaemonConfig::default(),
        }
    }
}

fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chiral-network")
}

fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chiral-network")
        .join("config.toml")
}

fn default_pid_file() -> PathBuf {
    default_data_dir().join("headless").join("chiral-daemon.pid")
}

fn load_config() -> Result<HeadlessConfig, String> {
    let path = default_config_path();
    if !path.exists() {
        return Ok(HeadlessConfig::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    toml::from_str::<HeadlessConfig>(&raw)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

fn save_config(cfg: &HeadlessConfig) -> Result<(), String> {
    let path = default_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    let raw =
        toml::to_string_pretty(cfg).map_err(|e| format!("Failed to serialize config: {}", e))?;
    std::fs::write(&path, raw).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

fn config_get(cfg: &HeadlessConfig, key: &str) -> Option<String> {
    match key {
        "theme" => Some(cfg.theme.clone()),
        "color_theme" | "colorTheme" => Some(cfg.color_theme.clone()),
        "nav_style" | "navStyle" => Some(cfg.nav_style.clone()),
        "reduced_motion" | "reducedMotion" => Some(cfg.reduced_motion.to_string()),
        "compact_mode" | "compactMode" => Some(cfg.compact_mode.to_string()),
        "download_directory" | "downloadDirectory" => Some(cfg.download_directory.clone()),
        "hosting.enabled" => Some(cfg.hosting.enabled.to_string()),
        "hosting.max_storage_bytes" | "hosting.maxStorageBytes" => {
            Some(cfg.hosting.max_storage_bytes.to_string())
        }
        "hosting.price_per_mb_per_day_wei" | "hosting.pricePerMbPerDayWei" => {
            Some(cfg.hosting.price_per_mb_per_day_wei.clone())
        }
        "hosting.min_deposit_wei" | "hosting.minDepositWei" => {
            Some(cfg.hosting.min_deposit_wei.clone())
        }
        "daemon.auto_start" | "daemon.autoStart" => Some(cfg.daemon.auto_start.to_string()),
        "daemon.port" => Some(cfg.daemon.port.to_string()),
        _ => None,
    }
}

fn parse_bool(input: &str) -> Result<bool, String> {
    match input.trim().to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(format!("Expected boolean, got '{}'", input)),
    }
}

fn config_set(cfg: &mut HeadlessConfig, key: &str, value: &str) -> Result<(), String> {
    match key {
        "theme" => cfg.theme = value.to_string(),
        "color_theme" | "colorTheme" => cfg.color_theme = value.to_string(),
        "nav_style" | "navStyle" => cfg.nav_style = value.to_string(),
        "reduced_motion" | "reducedMotion" => cfg.reduced_motion = parse_bool(value)?,
        "compact_mode" | "compactMode" => cfg.compact_mode = parse_bool(value)?,
        "download_directory" | "downloadDirectory" => cfg.download_directory = value.to_string(),
        "hosting.enabled" => cfg.hosting.enabled = parse_bool(value)?,
        "hosting.max_storage_bytes" | "hosting.maxStorageBytes" => {
            cfg.hosting.max_storage_bytes = value
                .parse::<u64>()
                .map_err(|e| format!("Invalid max storage: {}", e))?
        }
        "hosting.price_per_mb_per_day_wei" | "hosting.pricePerMbPerDayWei" => {
            cfg.hosting.price_per_mb_per_day_wei = value.to_string()
        }
        "hosting.min_deposit_wei" | "hosting.minDepositWei" => {
            cfg.hosting.min_deposit_wei = value.to_string()
        }
        "daemon.auto_start" | "daemon.autoStart" => cfg.daemon.auto_start = parse_bool(value)?,
        "daemon.port" => {
            cfg.daemon.port = value
                .parse::<u16>()
                .map_err(|e| format!("Invalid daemon port: {}", e))?
        }
        _ => return Err(format!("Unknown config key '{}'", key)),
    }
    Ok(())
}

fn read_pid_file(path: &Path) -> Result<u32, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    raw.trim()
        .parse::<u32>()
        .map_err(|e| format!("Invalid PID in {}: {}", path.display(), e))
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn process_exists(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid)])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

fn terminate_process(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .arg(pid.to_string())
            .status()
            .map_err(|e| format!("Failed to send SIGTERM to {}: {}", pid, e))?;
        if !status.success() {
            return Err(format!("kill exited with status {}", status));
        }
    }
    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
            .map_err(|e| format!("Failed to terminate {}: {}", pid, e))?;
        if !status.success() {
            return Err(format!("taskkill exited with status {}", status));
        }
    }
    Ok(())
}

async fn check_gateway_health(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/health", port);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build();
    let Ok(client) = client else {
        return false;
    };
    match client.get(url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

fn locate_daemon_binary() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(|e| format!("Failed to get current exe: {}", e))?;
    let daemon_name = if cfg!(windows) {
        "chiral_daemon.exe"
    } else {
        "chiral_daemon"
    };
    let sibling = current.with_file_name(daemon_name);
    if sibling.exists() {
        return Ok(sibling);
    }
    Err(format!(
        "Could not locate daemon binary at {}",
        sibling.display()
    ))
}

fn print_placeholder(feature: &str, command: &str) -> Result<(), String> {
    Err(format!(
        "{} command '{}' is scaffolded but not implemented in milestone 1",
        feature, command
    ))
}

async fn handle_daemon(cmd: DaemonCommand) -> Result<(), String> {
    match cmd {
        DaemonCommand::Start { port } => {
            let pid_path = default_pid_file();
            if pid_path.exists() {
                let existing_pid = read_pid_file(&pid_path)?;
                if process_exists(existing_pid) {
                    let healthy = check_gateway_health(port).await;
                    println!(
                        "Daemon already running (pid={}, health={})",
                        existing_pid, healthy
                    );
                    return Ok(());
                }
                let _ = std::fs::remove_file(&pid_path);
            }

            if let Some(parent) = pid_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
            }

            let mut launch = if let Ok(path) = locate_daemon_binary() {
                Command::new(path)
            } else {
                Command::new("chiral_daemon")
            };

            launch
                .arg("--port")
                .arg(port.to_string())
                .arg("--pid-file")
                .arg(pid_path.as_os_str())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Failed to start daemon: {}", e))?;

            let mut ready = false;
            for _ in 0..180 {
                if check_gateway_health(port).await {
                    ready = true;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            if !ready {
                return Err("Daemon started but health endpoint did not become ready".to_string());
            }

            let pid = read_pid_file(&pid_path)?;
            println!(
                "Daemon started (pid={}) on http://127.0.0.1:{}",
                pid, port
            );
            Ok(())
        }
        DaemonCommand::Stop => {
            let pid_path = default_pid_file();
            if !pid_path.exists() {
                println!("Daemon is not running (no PID file)");
                return Ok(());
            }
            let pid = read_pid_file(&pid_path)?;
            if process_exists(pid) {
                terminate_process(pid)?;
            }
            let _ = std::fs::remove_file(&pid_path);
            println!("Daemon stopped");
            Ok(())
        }
        DaemonCommand::Status { port } => {
            let pid_path = default_pid_file();
            if !pid_path.exists() {
                println!("stopped");
                return Ok(());
            }
            let pid = read_pid_file(&pid_path)?;
            let alive = process_exists(pid);
            let healthy = check_gateway_health(port).await;
            println!(
                "pid={} alive={} health={} url=http://127.0.0.1:{}",
                pid, alive, healthy, port
            );
            Ok(())
        }
    }
}

fn handle_settings(cmd: SettingsCommand) -> Result<(), String> {
    match cmd {
        SettingsCommand::Path => {
            println!("{}", default_config_path().display());
            Ok(())
        }
        SettingsCommand::Reset => {
            let cfg = HeadlessConfig::default();
            save_config(&cfg)?;
            println!("Settings reset: {}", default_config_path().display());
            Ok(())
        }
        SettingsCommand::Get { key } => {
            let cfg = load_config()?;
            match key {
                Some(k) => {
                    let Some(value) = config_get(&cfg, &k) else {
                        return Err(format!("Unknown config key '{}'", k));
                    };
                    println!("{}", value);
                }
                None => {
                    let rendered = toml::to_string_pretty(&cfg)
                        .map_err(|e| format!("Failed to render config: {}", e))?;
                    println!("{}", rendered);
                }
            }
            Ok(())
        }
        SettingsCommand::Set { key, value } => {
            let mut cfg = load_config()?;
            config_set(&mut cfg, &key, &value)?;
            save_config(&cfg)?;
            println!("Updated {}={}", key, value);
            Ok(())
        }
    }
}

async fn handle_network(cmd: NetworkCommand) -> Result<(), String> {
    match cmd {
        NetworkCommand::Bootstrap => {
            for peer in dht::get_bootstrap_peer_ids() {
                println!("{}", peer);
            }
            Ok(())
        }
        NetworkCommand::Status { port } => {
            let healthy = check_gateway_health(port).await;
            println!("gateway_health={}", healthy);
            Ok(())
        }
    }
}

async fn handle_reputation(cmd: ReputationCommand) -> Result<(), String> {
    let state = RatingState::new(default_data_dir());
    let manifest = state.manifest.read().await;
    let now = rating_storage::now_secs();

    match cmd {
        ReputationCommand::Show { wallet } => {
            let snapshot = compute_reputation_for_wallet(&manifest.events, &wallet, now);
            let recent_events = manifest
                .events
                .iter()
                .filter(|e| e.seeder_wallet.eq_ignore_ascii_case(&wallet))
                .filter(|e| now.saturating_sub(e.created_at) <= LOOKBACK_SECS)
                .count();

            println!("wallet={}", wallet);
            println!("elo={:.1}", snapshot.elo);
            println!("base_elo={:.1}", snapshot.base_elo);
            println!("transactions={}", snapshot.transaction_count);
            println!("completed={}", snapshot.completed_count);
            println!("failed={}", snapshot.failed_count);
            println!("ratings={}", snapshot.rating_count);
            println!("earned_wei={}", snapshot.total_earned_wei);
            println!("recent_events={}", recent_events);
            Ok(())
        }
        ReputationCommand::Batch { wallets } => {
            if wallets.is_empty() {
                return Err("Provide at least one wallet".to_string());
            }
            for wallet in wallets {
                let s = compute_reputation_for_wallet(&manifest.events, &wallet, now);
                println!(
                    "{} elo={:.1} tx={} completed={} failed={} ratings={} earned_wei={}",
                    wallet,
                    s.elo,
                    s.transaction_count,
                    s.completed_count,
                    s.failed_count,
                    s.rating_count,
                    s.total_earned_wei
                );
            }
            Ok(())
        }
    }
}

async fn handle_diagnostics(cmd: DiagnosticsCommand) -> Result<(), String> {
    match cmd {
        DiagnosticsCommand::Report { port } => {
            let cfg_path = default_config_path();
            let data_dir = default_data_dir();
            let pid_path = default_pid_file();
            let gateway_healthy = check_gateway_health(port).await;

            let drive_manifest = drive_storage::load_manifest();
            let rating_state = RatingState::new(default_data_dir());
            let rating_events = rating_state.manifest.read().await.events.len();
            let bootstrap_count = dht::get_bootstrap_peer_ids().len();

            println!("config_path={}", cfg_path.display());
            println!("config_exists={}", cfg_path.exists());
            println!("data_dir={}", data_dir.display());
            println!("data_dir_exists={}", data_dir.exists());
            println!("pid_file={}", pid_path.display());
            println!("pid_file_exists={}", pid_path.exists());
            println!("gateway_health={}", gateway_healthy);
            println!("bootstrap_nodes={}", bootstrap_count);
            println!("drive_items={}", drive_manifest.items.len());
            println!("drive_shares={}", drive_manifest.shares.len());
            println!("reputation_events={}", rating_events);
            Ok(())
        }
    }
}

fn gateway_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{}", port)
}

async fn parse_json_or_error<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, String> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp
            .text()
            .await
            .unwrap_or_else(|_| "<no response body>".to_string());
        return Err(format!("HTTP {}: {}", status, body));
    }
    resp.json::<T>()
        .await
        .map_err(|e| format!("Failed to parse JSON response: {}", e))
}

#[derive(Serialize)]
struct CreateFolderPayload {
    name: String,
    parent_id: Option<String>,
}

#[derive(Serialize)]
struct UpdateItemPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    starred: Option<bool>,
}

async fn drive_list_items(
    client: &reqwest::Client,
    port: u16,
    owner: &str,
    parent_id: Option<&str>,
) -> Result<Vec<DriveItem>, String> {
    let mut req = client
        .get(format!("{}/api/drive/items", gateway_base_url(port)))
        .header("X-Owner", owner);
    if let Some(pid) = parent_id {
        req = req.query(&[("parent_id", pid)]);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Drive list request failed: {}", e))?;
    parse_json_or_error(resp).await
}

async fn drive_create_folder(
    client: &reqwest::Client,
    port: u16,
    owner: &str,
    name: String,
    parent_id: Option<String>,
) -> Result<DriveItem, String> {
    let resp = client
        .post(format!("{}/api/drive/folders", gateway_base_url(port)))
        .header("X-Owner", owner)
        .json(&CreateFolderPayload { name, parent_id })
        .send()
        .await
        .map_err(|e| format!("Drive mkdir request failed: {}", e))?;
    parse_json_or_error(resp).await
}

async fn drive_set_starred(
    client: &reqwest::Client,
    port: u16,
    owner: &str,
    item_id: &str,
    starred: bool,
) -> Result<DriveItem, String> {
    let payload = UpdateItemPayload {
        name: None,
        parent_id: None,
        starred: Some(starred),
    };
    let resp = client
        .put(format!(
            "{}/api/drive/items/{}",
            gateway_base_url(port),
            item_id
        ))
        .header("X-Owner", owner)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Drive update request failed: {}", e))?;
    parse_json_or_error(resp).await
}

async fn drive_delete_item(
    client: &reqwest::Client,
    port: u16,
    owner: &str,
    item_id: &str,
) -> Result<(), String> {
    let resp = client
        .delete(format!(
            "{}/api/drive/items/{}",
            gateway_base_url(port),
            item_id
        ))
        .header("X-Owner", owner)
        .send()
        .await
        .map_err(|e| format!("Drive delete request failed: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, body));
    }
    Ok(())
}

async fn handle_drive(cmd: DriveCommand) -> Result<(), String> {
    let client = reqwest::Client::new();
    match cmd {
        DriveCommand::Ls {
            owner,
            parent_id,
            port,
        } => {
            let items = drive_list_items(&client, port, &owner, parent_id.as_deref()).await?;
            for item in items {
                let marker = if item.starred { "*" } else { "-" };
                println!(
                    "{} {} {} {}",
                    marker, item.id, item.item_type, item.name
                );
            }
            Ok(())
        }
        DriveCommand::Mkdir {
            owner,
            name,
            parent_id,
            port,
        } => {
            let item = drive_create_folder(&client, port, &owner, name, parent_id).await?;
            println!("created folder id={} name={}", item.id, item.name);
            Ok(())
        }
        DriveCommand::Delete {
            owner,
            item_id,
            port,
        } => {
            drive_delete_item(&client, port, &owner, &item_id).await?;
            println!("deleted item {}", item_id);
            Ok(())
        }
        DriveCommand::Star {
            owner,
            item_id,
            port,
        } => {
            let item = drive_set_starred(&client, port, &owner, &item_id, true).await?;
            println!("starred item {} ({})", item.id, item.name);
            Ok(())
        }
        DriveCommand::Unstar {
            owner,
            item_id,
            port,
        } => {
            let item = drive_set_starred(&client, port, &owner, &item_id, false).await?;
            println!("unstarred item {} ({})", item.id, item.name);
            Ok(())
        }
        DriveCommand::Tree => print_placeholder("drive", "Tree"),
        DriveCommand::Upload => print_placeholder("drive", "Upload"),
        DriveCommand::Rename => print_placeholder("drive", "Rename"),
        DriveCommand::Move => print_placeholder("drive", "Move"),
        DriveCommand::Visibility => print_placeholder("drive", "Visibility"),
        DriveCommand::Share => print_placeholder("drive", "Share"),
        DriveCommand::Publish => print_placeholder("drive", "Publish"),
        DriveCommand::Unpublish => print_placeholder("drive", "Unpublish"),
        DriveCommand::ExportTorrent => print_placeholder("drive", "ExportTorrent"),
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Daemon { cmd } => handle_daemon(cmd).await,
        Commands::Settings { cmd } => handle_settings(cmd),
        Commands::Network { cmd } => handle_network(cmd).await,
        Commands::Reputation { cmd } => handle_reputation(cmd).await,
        Commands::Diagnostics { cmd } => handle_diagnostics(cmd).await,
        Commands::Wallet { cmd } => print_placeholder("wallet", &format!("{:?}", cmd)),
        Commands::Account { cmd } => print_placeholder("account", &format!("{:?}", cmd)),
        Commands::Dht { cmd } => print_placeholder("dht", &format!("{:?}", cmd)),
        Commands::Download { cmd } => print_placeholder("download", &format!("{:?}", cmd)),
        Commands::Drive { cmd } => handle_drive(cmd).await,
        Commands::ChiralDrop { cmd } => print_placeholder("drop", &format!("{:?}", cmd)),
        Commands::Hosting { cmd } => print_placeholder("hosting", &format!("{:?}", cmd)),
        Commands::Market { cmd } => print_placeholder("market", &format!("{:?}", cmd)),
        Commands::Mining { cmd } => print_placeholder("mining", &format!("{:?}", cmd)),
        Commands::Geth { cmd } => print_placeholder("geth", &format!("{:?}", cmd)),
    };

    if let Err(err) = result {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}
