use base64::Engine;
use clap::{Parser, Subcommand};
use rand::RngCore;
use rlp::RlpStream;
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tiny_keccak::{Hasher, Keccak};

use chiral_network::dht;
use chiral_network::drive_storage;
use chiral_network::drive_storage::DriveItem;
use chiral_network::geth;
use chiral_network::hosting;
use chiral_network::rating_storage::{
    self, compute_reputation_for_wallet, RatingState, LOOKBACK_SECS,
};

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
    Get { key: Option<String> },
    Set { key: String, value: String },
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
    Show { wallet: String },
    Batch { wallets: Vec<String> },
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
    Import {
        #[arg(long)]
        private_key: String,
    },
    Login {
        #[arg(long)]
        private_key: Option<String>,
    },
    Show,
    Export {
        #[arg(long, default_value_t = false)]
        reveal_private_key: bool,
    },
}

#[derive(Subcommand, Debug)]
enum AccountCommand {
    Balance {
        #[arg(long)]
        address: Option<String>,
    },
    Send {
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        private_key: Option<String>,
    },
    History {
        #[arg(long)]
        address: Option<String>,
        #[arg(long, default_value_t = 20_000)]
        max_blocks: u64,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    Meta,
}

#[derive(Subcommand, Debug)]
enum DhtCommand {
    Start {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Stop {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Status {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    PeerId {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Peers {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Put {
        #[arg(long)]
        key: String,
        #[arg(long)]
        value: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Get {
        #[arg(long)]
        key: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Echo {
        #[arg(long)]
        peer_id: String,
        #[arg(long)]
        payload: String,
        #[arg(long, default_value_t = false)]
        payload_is_base64: bool,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Listening {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
}

#[derive(Subcommand, Debug)]
enum DownloadCommand {
    Search {
        #[arg(long)]
        hash: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Start {
        #[arg(long)]
        hash: String,
        #[arg(long)]
        file_name: String,
        #[arg(long)]
        peer_id: String,
        #[arg(long)]
        request_id: Option<String>,
        #[arg(long)]
        multiaddr: Vec<String>,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Cost {
        #[arg(long)]
        file_size_bytes: u64,
        #[arg(long)]
        tier: String,
    },
    List,
    History,
    Watch {
        #[arg(long)]
        request_id: String,
        #[arg(long, default_value_t = 1500)]
        interval_ms: u64,
        #[arg(long, default_value_t = 60)]
        attempts: u32,
    },
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
    Tree {
        #[arg(long)]
        owner: String,
    },
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
    Upload {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        file_path: String,
        #[arg(long)]
        parent_id: Option<String>,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Rename {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        item_id: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Move {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        item_id: String,
        #[arg(long)]
        parent_id: Option<String>,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
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
    Visibility {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        item_id: String,
        #[arg(long)]
        public: bool,
    },
    Share {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        item_id: String,
        #[arg(long)]
        password: Option<String>,
        #[arg(long, default_value_t = false)]
        public: bool,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Publish {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        item_id: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
        #[arg(long, default_value = "WebRTC")]
        protocol: String,
        #[arg(long)]
        price_chi: Option<String>,
        #[arg(long)]
        wallet_address: Option<String>,
    },
    Unpublish {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        item_id: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    ExportTorrent {
        #[arg(long)]
        owner: String,
        #[arg(long)]
        item_id: String,
        #[arg(long)]
        output: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum DropCommand {
    Peers {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Send {
        #[arg(long)]
        peer_id: String,
        #[arg(long)]
        file_path: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    SendPaid {
        #[arg(long)]
        peer_id: String,
        #[arg(long)]
        file_path: String,
        #[arg(long)]
        price_wei: String,
        #[arg(long)]
        sender_wallet: String,
        #[arg(long)]
        file_hash: Option<String>,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Inbox {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Accept {
        #[arg(long)]
        transfer_id: String,
        #[arg(long)]
        download_dir: Option<String>,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Decline {
        #[arg(long)]
        transfer_id: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    History,
}

#[derive(Subcommand, Debug)]
enum HostingCommand {
    Server {
        #[command(subcommand)]
        cmd: DaemonCommand,
    },
    Site {
        #[command(subcommand)]
        cmd: HostingSiteCommand,
    },
    PublishRelay {
        #[arg(long)]
        site_id: String,
        #[arg(long)]
        relay_url: String,
    },
    UnpublishRelay {
        #[arg(long)]
        site_id: String,
    },
}

#[derive(Subcommand, Debug)]
enum HostingSiteCommand {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        directory: String,
        #[arg(long)]
        site_id: Option<String>,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    List,
    Delete {
        #[arg(long)]
        site_id: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
}

#[derive(Subcommand, Debug)]
enum MarketCommand {
    Advertise {
        #[arg(long)]
        wallet: String,
        #[arg(long, default_value_t = 10 * 1024 * 1024 * 1024u64)]
        max_storage_bytes: u64,
        #[arg(long, default_value = "1000000000000000")]
        price_per_mb_per_day_wei: String,
        #[arg(long, default_value = "100000000000000000")]
        min_deposit_wei: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Unadvertise {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Browse {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Propose {
        #[arg(long)]
        peer_id: String,
        #[arg(long)]
        agreement_id: String,
        #[arg(long)]
        agreement_json: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Agreements,
    Respond {
        #[arg(long)]
        peer_id: String,
        #[arg(long)]
        agreement_id: String,
        #[arg(long)]
        status: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Cancel {
        #[arg(long)]
        peer_id: String,
        #[arg(long)]
        agreement_id: String,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Cleanup,
}

#[derive(Subcommand, Debug)]
enum MiningCommand {
    Install {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Start {
        #[arg(long, default_value_t = 1)]
        threads: u32,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Stop {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Status {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Blocks {
        #[arg(long, default_value_t = 500)]
        max: u64,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
}

#[derive(Subcommand, Debug)]
enum GethCommand {
    Start {
        #[arg(long)]
        miner_address: Option<String>,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Stop {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Status {
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
    Logs {
        #[arg(long)]
        lines: Option<usize>,
        #[arg(long, default_value_t = 9419)]
        port: u16,
    },
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WalletProfile {
    address: String,
    private_key: String,
    created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct WalletStore {
    active: Option<WalletProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DownloadRecord {
    request_id: String,
    file_hash: String,
    file_name: String,
    peer_id: String,
    status: String,
    created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DropRecord {
    transfer_id: String,
    peer_id: String,
    file_name: String,
    file_path: String,
    status: String,
    paid: bool,
    created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WalletBalanceResult {
    balance: String,
    balance_wei: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendTransactionResult {
    hash: String,
    status: String,
    balance_before: String,
    balance_after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TransactionHistoryItem {
    hash: String,
    from: String,
    to: String,
    value: String,
    value_wei: String,
    block_number: u64,
    timestamp: u64,
    status: String,
    gas_used: u64,
    direction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeederInfo {
    peer_id: String,
    #[serde(default)]
    price_wei: String,
    #[serde(default)]
    wallet_address: String,
    #[serde(default)]
    multiaddrs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileMetadata {
    hash: String,
    file_name: String,
    file_size: u64,
    protocol: String,
    created_at: u64,
    #[serde(default)]
    peer_id: String,
    #[serde(default)]
    price_wei: String,
    #[serde(default)]
    wallet_address: String,
    #[serde(default)]
    seeders: Vec<SeederInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostRegistryEntry {
    peer_id: String,
    wallet_address: String,
    updated_at: u64,
}

fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chiral-network")
}

fn headless_dir() -> PathBuf {
    default_data_dir().join("headless")
}

fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chiral-network")
        .join("config.toml")
}

fn default_pid_file() -> PathBuf {
    default_data_dir()
        .join("headless")
        .join("chiral-daemon.pid")
}

fn wallet_store_path() -> PathBuf {
    headless_dir().join("wallet.json")
}

fn download_history_path() -> PathBuf {
    headless_dir().join("download_history.json")
}

fn drop_history_path() -> PathBuf {
    headless_dir().join("drop_history.json")
}

fn agreement_dir_path() -> PathBuf {
    default_data_dir().join("agreements")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    Ok(())
}

fn read_json_file<T>(path: &Path) -> Result<T, String>
where
    T: DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    serde_json::from_str(&raw).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

fn write_json_file<T>(path: &Path, value: &T) -> Result<(), String>
where
    T: Serialize,
{
    ensure_parent_dir(path)?;
    let raw = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Failed to serialize JSON: {}", e))?;
    std::fs::write(path, raw).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

fn load_wallet_store() -> Result<WalletStore, String> {
    read_json_file(&wallet_store_path())
}

fn save_wallet_store(store: &WalletStore) -> Result<(), String> {
    write_json_file(&wallet_store_path(), store)
}

fn load_download_history() -> Result<Vec<DownloadRecord>, String> {
    read_json_file(&download_history_path())
}

fn save_download_history(items: &[DownloadRecord]) -> Result<(), String> {
    write_json_file(&download_history_path(), &items)
}

fn load_drop_history() -> Result<Vec<DropRecord>, String> {
    read_json_file(&drop_history_path())
}

fn save_drop_history(items: &[DropRecord]) -> Result<(), String> {
    write_json_file(&drop_history_path(), &items)
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

async fn check_headless_api(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/api/headless/runtime", port);
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
    let current =
        std::env::current_exe().map_err(|e| format!("Failed to get current exe: {}", e))?;
    let daemon_candidates: &[&str] = if cfg!(windows) {
        &["chiral_daemon.exe", "chiral-daemon.exe"]
    } else {
        &["chiral_daemon", "chiral-daemon"]
    };

    for daemon_name in daemon_candidates {
        let sibling = current.with_file_name(daemon_name);
        if sibling.exists() {
            return Ok(sibling);
        }
    }

    let expected = current.with_file_name(daemon_candidates[0]);
    Err(format!(
        "Could not locate daemon binary at {}",
        expected.display()
    ))
}

fn gateway_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{}", port)
}

fn print_json(value: &Value) -> Result<(), String> {
    let rendered =
        serde_json::to_string_pretty(value).map_err(|e| format!("Failed to render JSON: {}", e))?;
    println!("{}", rendered);
    Ok(())
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

async fn daemon_post_json<T: Serialize>(
    port: u16,
    path: &str,
    payload: &T,
) -> Result<Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}{}", gateway_base_url(port), path))
        .json(payload)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    parse_json_or_error(resp).await
}

async fn daemon_post_empty(port: u16, path: &str) -> Result<Value, String> {
    daemon_post_json(port, path, &serde_json::json!({})).await
}

async fn daemon_get_json(port: u16, path: &str) -> Result<Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}{}", gateway_base_url(port), path))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    parse_json_or_error(resp).await
}

async fn daemon_get_json_with_query(
    port: u16,
    path: &str,
    params: &[(&str, String)],
) -> Result<Value, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}{}", gateway_base_url(port), path))
        .query(params)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    parse_json_or_error(resp).await
}

fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut output = [0u8; 32];
    hasher.update(data);
    hasher.finalize(&mut output);
    output
}

fn parse_hex_u64(hex: &str) -> u64 {
    let hex = hex.trim_start_matches("0x");
    u64::from_str_radix(hex, 16).unwrap_or(0)
}

fn parse_chi_to_wei(amount: &str) -> Result<u128, String> {
    let amount = amount.trim();
    let parts: Vec<&str> = amount.split('.').collect();
    if parts.len() > 2 {
        return Err("Invalid amount format".to_string());
    }

    let whole: u128 = if parts[0].is_empty() {
        0
    } else {
        parts[0].parse().map_err(|_| "Invalid amount".to_string())?
    };

    let frac_wei = if parts.len() == 2 {
        let frac_str = parts[1];
        if frac_str.len() > 18 {
            frac_str[..18]
                .parse::<u128>()
                .map_err(|_| "Invalid amount".to_string())?
        } else {
            let padded = format!("{:0<18}", frac_str);
            padded
                .parse::<u128>()
                .map_err(|_| "Invalid amount".to_string())?
        }
    } else {
        0u128
    };

    let wei = whole
        .checked_mul(1_000_000_000_000_000_000u128)
        .and_then(|w| w.checked_add(frac_wei))
        .ok_or("Amount overflow".to_string())?;

    Ok(wei)
}

fn encode_unsigned_tx(
    nonce: u64,
    gas_price: u128,
    gas_limit: u64,
    to: &[u8],
    value: u128,
    data: &[u8],
    chain_id: u64,
) -> Vec<u8> {
    let mut stream = RlpStream::new_list(9);
    stream.append(&nonce);
    stream.append(&gas_price);
    stream.append(&gas_limit);
    stream.append(&to.to_vec());
    stream.append(&value);
    stream.append(&data.to_vec());
    stream.append(&chain_id);
    stream.append(&0u8);
    stream.append(&0u8);
    stream.out().to_vec()
}

fn strip_leading_zeros(bytes: &[u8]) -> &[u8] {
    let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    &bytes[first_nonzero..]
}

fn rlp_append_bytes_as_uint(stream: &mut RlpStream, bytes: &[u8]) {
    let stripped = strip_leading_zeros(bytes);
    if stripped.is_empty() {
        stream.append(&0u8);
    } else {
        stream.append(&stripped.to_vec());
    }
}

fn encode_signed_tx(
    nonce: u64,
    gas_price: u128,
    gas_limit: u64,
    to: &[u8],
    value: u128,
    data: &[u8],
    v: u64,
    r: &[u8],
    s: &[u8],
) -> Vec<u8> {
    let mut stream = RlpStream::new_list(9);
    stream.append(&nonce);
    stream.append(&gas_price);
    stream.append(&gas_limit);
    stream.append(&to.to_vec());
    stream.append(&value);
    stream.append(&data.to_vec());
    stream.append(&v);
    rlp_append_bytes_as_uint(&mut stream, r);
    rlp_append_bytes_as_uint(&mut stream, s);
    stream.out().to_vec()
}

async fn rpc_call(method: &str, params: Value) -> Result<Value, String> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(geth::rpc_endpoint())
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("RPC request failed: {}", e))?;

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse RPC response: {}", e))?;

    if let Some(error) = json.get("error") {
        return Err(format!("RPC error: {}", error));
    }

    Ok(json.get("result").cloned().unwrap_or(Value::Null))
}

async fn get_wallet_balance(address: &str) -> Result<WalletBalanceResult, String> {
    let result = rpc_call("eth_getBalance", serde_json::json!([address, "pending"])).await?;
    let balance_hex = result
        .as_str()
        .ok_or("Invalid balance response from blockchain")?;
    let hex_str = balance_hex.trim_start_matches("0x");
    let balance_wei = if hex_str.is_empty() {
        0u128
    } else {
        u128::from_str_radix(hex_str, 16)
            .map_err(|e| format!("Failed to parse balance hex '{}': {}", balance_hex, e))?
    };
    let balance_chi = balance_wei as f64 / 1e18;

    Ok(WalletBalanceResult {
        balance: format!("{:.6}", balance_chi),
        balance_wei: balance_wei.to_string(),
    })
}

async fn send_transaction(
    from_address: &str,
    to_address: &str,
    amount_chi: &str,
    private_key: &str,
) -> Result<SendTransactionResult, String> {
    let client = reqwest::Client::new();

    let pk_hex = private_key.trim_start_matches("0x");
    let pk_bytes = hex::decode(pk_hex).map_err(|e| format!("Invalid private key hex: {}", e))?;

    let secp = Secp256k1::new();
    let secret_key =
        SecretKey::from_slice(&pk_bytes).map_err(|e| format!("Invalid private key: {}", e))?;

    let amount_wei = parse_chi_to_wei(amount_chi)?;

    let nonce_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [from_address, "pending"],
        "id": 1
    });
    let nonce_response = client
        .post(geth::rpc_endpoint())
        .json(&nonce_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get nonce: {}", e))?;
    let nonce_json: Value = nonce_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse nonce response: {}", e))?;
    if let Some(error) = nonce_json.get("error") {
        return Err(format!("RPC error getting nonce: {}", error));
    }
    let nonce = parse_hex_u64(nonce_json["result"].as_str().unwrap_or("0x0"));

    let balance = get_wallet_balance(from_address).await?;
    let balance_wei = balance
        .balance_wei
        .parse::<u128>()
        .map_err(|e| format!("Invalid balance value: {}", e))?;

    let gas_price_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_gasPrice",
        "params": [],
        "id": 1
    });
    let gas_price_response = client
        .post(geth::rpc_endpoint())
        .json(&gas_price_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get gas price: {}", e))?;
    let gas_price_json: Value = gas_price_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse gas price response: {}", e))?;

    let gas_price = parse_hex_u64(gas_price_json["result"].as_str().unwrap_or("0x0"));
    let gas_price = if gas_price == 0 {
        1_000_000_000
    } else {
        gas_price
    };

    let gas_limit: u64 = 21000;
    let chain_id = geth::CHAIN_ID;
    let gas_price_u128 = gas_price as u128;
    let gas_cost = gas_price_u128 * gas_limit as u128;
    let total_cost = amount_wei
        .checked_add(gas_cost)
        .ok_or("Amount overflow".to_string())?;

    if balance_wei < total_cost {
        return Err(format!(
            "Insufficient balance: have {:.6} CHI, need {:.6} CHI + {:.6} CHI gas",
            balance_wei as f64 / 1e18,
            amount_wei as f64 / 1e18,
            gas_cost as f64 / 1e18
        ));
    }

    let to_bytes = hex::decode(to_address.trim_start_matches("0x"))
        .map_err(|e| format!("Invalid to address: {}", e))?;

    let unsigned_tx = encode_unsigned_tx(
        nonce,
        gas_price_u128,
        gas_limit,
        &to_bytes,
        amount_wei,
        &[],
        chain_id,
    );

    let tx_hash = keccak256(&unsigned_tx);
    let message = Message::from_digest_slice(&tx_hash)
        .map_err(|e| format!("Failed to create message: {}", e))?;
    let (recovery_id, signature) = secp
        .sign_ecdsa_recoverable(&message, &secret_key)
        .serialize_compact();

    let v = chain_id * 2 + 35 + recovery_id.to_i32() as u64;
    let r = &signature[0..32];
    let s = &signature[32..64];

    let signed_tx = encode_signed_tx(
        nonce,
        gas_price_u128,
        gas_limit,
        &to_bytes,
        amount_wei,
        &[],
        v,
        r,
        s,
    );

    let signed_tx_hex = format!("0x{}", hex::encode(&signed_tx));

    let send_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [signed_tx_hex],
        "id": 1
    });

    let send_response = client
        .post(geth::rpc_endpoint())
        .json(&send_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to send transaction: {}", e))?;

    let send_json: Value = send_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse send response: {}", e))?;

    if let Some(error) = send_json.get("error") {
        let msg = error
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if msg != "already known" {
            return Err(format!("Transaction failed: {}", error));
        }
    }

    let hash = if let Some(v) = send_json["result"].as_str() {
        v.to_string()
    } else {
        format!("0x{}", hex::encode(keccak256(&signed_tx)))
    };

    let balance_before = format!("{:.6}", balance_wei as f64 / 1e18);
    let balance_after = format!(
        "{:.6}",
        balance_wei.saturating_sub(total_cost) as f64 / 1e18
    );

    Ok(SendTransactionResult {
        hash,
        status: "pending".to_string(),
        balance_before,
        balance_after,
    })
}

async fn get_transaction_history(
    address: &str,
    max_blocks: u64,
    limit: usize,
) -> Result<Vec<TransactionHistoryItem>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let block_payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });
    let block_response = client
        .post(geth::rpc_endpoint())
        .json(&block_payload)
        .send()
        .await
        .map_err(|e| format!("Failed to get block number: {}", e))?;

    let block_json: Value = block_response
        .json()
        .await
        .map_err(|e| format!("Failed to parse block response: {}", e))?;

    let latest_block_hex = block_json["result"].as_str().unwrap_or("0x0");
    let latest_block =
        u64::from_str_radix(latest_block_hex.trim_start_matches("0x"), 16).unwrap_or(0);

    const BATCH_SIZE: u64 = 100;
    let first_block_to_scan = latest_block.saturating_sub(max_blocks.saturating_sub(1));
    let mut cursor = latest_block;
    let mut transactions = Vec::new();
    let address_lower = address.to_lowercase();

    'outer: loop {
        let batch_start = cursor
            .saturating_sub(BATCH_SIZE - 1)
            .max(first_block_to_scan);

        let batch: Vec<Value> = (batch_start..=cursor)
            .rev()
            .enumerate()
            .map(|(i, block_num)| {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "eth_getBlockByNumber",
                    "params": [format!("0x{:x}", block_num), true],
                    "id": i + 1
                })
            })
            .collect();

        let batch_response = client.post(geth::rpc_endpoint()).json(&batch).send().await;

        let Ok(response) = batch_response else {
            break;
        };
        let Ok(results) = response.json::<Vec<Value>>().await else {
            break;
        };

        for item in &results {
            let Some(result) = item.get("result") else {
                continue;
            };
            let Some(txs) = result.get("transactions").and_then(|t| t.as_array()) else {
                continue;
            };
            let block_timestamp = result
                .get("timestamp")
                .and_then(|t| t.as_str())
                .map(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0))
                .unwrap_or(0);
            let block_number_hex = result
                .get("number")
                .and_then(|n| n.as_str())
                .unwrap_or("0x0");
            let block_num =
                u64::from_str_radix(block_number_hex.trim_start_matches("0x"), 16).unwrap_or(0);

            for tx in txs {
                let from = tx
                    .get("from")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_lowercase();
                let to = tx
                    .get("to")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_lowercase();

                if from != address_lower && to != address_lower {
                    continue;
                }

                let value_hex = tx.get("value").and_then(|v| v.as_str()).unwrap_or("0x0");
                let value_wei =
                    u128::from_str_radix(value_hex.trim_start_matches("0x"), 16).unwrap_or(0);
                let value_chi = value_wei as f64 / 1e18;

                let gas_hex = tx.get("gas").and_then(|g| g.as_str()).unwrap_or("0x0");
                let gas_used =
                    u64::from_str_radix(gas_hex.trim_start_matches("0x"), 16).unwrap_or(0);

                let hash = tx
                    .get("hash")
                    .and_then(|h| h.as_str())
                    .unwrap_or_default()
                    .to_string();
                let tx_from = tx
                    .get("from")
                    .and_then(|h| h.as_str())
                    .unwrap_or_default()
                    .to_string();
                let tx_to = tx
                    .get("to")
                    .and_then(|h| h.as_str())
                    .unwrap_or_default()
                    .to_string();

                let direction = if tx_from.eq_ignore_ascii_case(address) {
                    "send"
                } else {
                    "receive"
                };

                transactions.push(TransactionHistoryItem {
                    hash,
                    from: tx_from,
                    to: tx_to,
                    value: format!("{:.6}", value_chi),
                    value_wei: value_wei.to_string(),
                    block_number: block_num,
                    timestamp: block_timestamp,
                    status: "confirmed".to_string(),
                    gas_used,
                    direction: direction.to_string(),
                });

                if transactions.len() >= limit {
                    break 'outer;
                }
            }
        }

        if batch_start <= first_block_to_scan {
            break;
        }
        cursor = batch_start - 1;
    }

    transactions.sort_by(|a, b| b.block_number.cmp(&a.block_number));
    Ok(transactions)
}

fn normalize_private_key(input: &str) -> Result<String, String> {
    let mut key = input.trim().to_string();
    if key.starts_with("0x") {
        key = key.trim_start_matches("0x").to_string();
    }
    if key.len() != 64 || !key.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Private key must be 64 hex characters".to_string());
    }
    let bytes = hex::decode(&key).map_err(|e| format!("Invalid private key: {}", e))?;
    SecretKey::from_slice(&bytes).map_err(|e| format!("Invalid private key: {}", e))?;
    Ok(format!("0x{}", key))
}

fn address_from_private_key(private_key: &str) -> Result<String, String> {
    let key_hex = private_key.trim_start_matches("0x");
    let sk_bytes = hex::decode(key_hex).map_err(|e| format!("Invalid private key: {}", e))?;
    let secret_key =
        SecretKey::from_slice(&sk_bytes).map_err(|e| format!("Invalid private key: {}", e))?;
    let secp = Secp256k1::new();
    let public_key = PublicKey::from_secret_key(&secp, &secret_key);
    let uncompressed = public_key.serialize_uncompressed();
    let hash = keccak256(&uncompressed[1..]);
    Ok(format!("0x{}", hex::encode(&hash[12..])))
}

fn generate_wallet() -> Result<WalletProfile, String> {
    let mut rng = rand::thread_rng();
    let private_key = loop {
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        if SecretKey::from_slice(&bytes).is_ok() {
            break format!("0x{}", hex::encode(bytes));
        }
    };
    let address = address_from_private_key(&private_key)?;
    Ok(WalletProfile {
        address,
        private_key,
        created_at: now_secs(),
    })
}

fn require_wallet() -> Result<WalletProfile, String> {
    let store = load_wallet_store()?;
    store
        .active
        .ok_or("No active wallet. Use `chiral wallet create` or `chiral wallet import`".to_string())
}

fn speed_tier_cost_per_mb_wei(tier: &str) -> Result<u128, String> {
    match tier.to_lowercase().as_str() {
        "standard" => Ok(1_000_000_000_000_000),
        "premium" => Ok(5_000_000_000_000_000),
        "ultra" => Ok(10_000_000_000_000_000),
        _ => Err(format!("Unknown speed tier: {}", tier)),
    }
}

fn calculate_download_cost_wei(file_size_bytes: u64, tier: &str) -> Result<u128, String> {
    let cost_per_mb = speed_tier_cost_per_mb_wei(tier)?;
    let size = file_size_bytes as u128;
    Ok((size * cost_per_mb + 999_999) / 1_000_000)
}

fn format_wei_as_chi(wei: u128) -> String {
    let whole = wei / 1_000_000_000_000_000_000;
    let frac = wei % 1_000_000_000_000_000_000;
    if frac == 0 {
        return whole.to_string();
    }
    let frac_str = format!("{:018}", frac);
    let trimmed = frac_str.trim_end_matches('0');
    let decimals = if trimmed.len() > 6 {
        &trimmed[..6]
    } else {
        trimmed
    };
    format!("{}.{}", whole, decimals)
}

async fn dht_get_value(port: u16, key: &str) -> Result<Option<String>, String> {
    let response = daemon_post_json(
        port,
        "/api/headless/dht/get",
        &serde_json::json!({ "key": key }),
    )
    .await?;
    Ok(response
        .get("value")
        .and_then(|v| if v.is_null() { None } else { v.as_str() })
        .map(|s| s.to_string()))
}

async fn dht_put_value(port: u16, key: &str, value: &str) -> Result<(), String> {
    let _ = daemon_post_json(
        port,
        "/api/headless/dht/put",
        &serde_json::json!({ "key": key, "value": value }),
    )
    .await?;
    Ok(())
}

async fn dht_peer_id(port: u16) -> Result<String, String> {
    let response = daemon_get_json(port, "/api/headless/dht/peer-id").await?;
    response
        .get("peerId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or("Peer ID not available. Start daemon and DHT first.".to_string())
}

async fn dht_listening_addresses(port: u16) -> Result<Vec<String>, String> {
    let response = daemon_get_json(port, "/api/headless/dht/listening-addresses").await?;
    let addresses = response
        .get("addresses")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    Ok(addresses)
}

fn ensure_agreements_dir() -> Result<PathBuf, String> {
    let dir = agreement_dir_path();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create agreements dir {}: {}", dir.display(), e))?;
    Ok(dir)
}

fn list_agreements() -> Result<Vec<String>, String> {
    let dir = ensure_agreements_dir()?;
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| format!("Failed to read agreements dir {}: {}", dir.display(), e))?;
    for entry in entries.flatten() {
        if let Some(name) = entry.file_name().to_str() {
            if let Some(id) = name.strip_suffix(".json") {
                out.push(id.to_string());
            }
        }
    }
    out.sort();
    Ok(out)
}

fn store_agreement(agreement_id: &str, json_str: &str) -> Result<(), String> {
    let dir = ensure_agreements_dir()?;
    let path = dir.join(format!("{}.json", agreement_id));
    std::fs::write(&path, json_str)
        .map_err(|e| format!("Failed to write agreement {}: {}", path.display(), e))
}

fn load_agreement(agreement_id: &str) -> Result<Option<String>, String> {
    let dir = ensure_agreements_dir()?;
    let path = dir.join(format!("{}.json", agreement_id));
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read agreement {}: {}", path.display(), e))?;
    Ok(Some(data))
}

fn compute_file_hash(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn encode_html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn build_torrent_file(
    file_hash: &str,
    file_name: &str,
    file_size: u64,
    file_path: &str,
) -> Result<Vec<u8>, String> {
    let announce = "udp://dht.chiral.network:6881/announce";
    let piece_length: u64 = 262144;

    let mut torrent_content = Vec::new();
    torrent_content.push(b'd');

    let announce_key = format!("8:announce{}:{}", announce.len(), announce);
    torrent_content.extend_from_slice(announce_key.as_bytes());

    let created_by = "chiral-network";
    let created_by_entry = format!("10:created by{}:{}", created_by.len(), created_by);
    torrent_content.extend_from_slice(created_by_entry.as_bytes());

    let creation_date = now_secs();
    let creation_date_entry = format!("13:creation datei{}e", creation_date);
    torrent_content.extend_from_slice(creation_date_entry.as_bytes());

    torrent_content.extend_from_slice(b"4:infod");

    let hash_bytes = hex::decode(file_hash).map_err(|e| format!("Invalid hash: {}", e))?;
    let pieces_entry = format!("6:pieces{}:", hash_bytes.len());
    torrent_content.extend_from_slice(pieces_entry.as_bytes());
    torrent_content.extend_from_slice(&hash_bytes);

    let length_entry = format!("6:lengthi{}e", file_size);
    torrent_content.extend_from_slice(length_entry.as_bytes());

    let name_entry = format!("4:name{}:{}", file_name.len(), file_name);
    torrent_content.extend_from_slice(name_entry.as_bytes());

    let piece_length_entry = format!("12:piece lengthi{}e", piece_length);
    torrent_content.extend_from_slice(piece_length_entry.as_bytes());

    let source_path_key = "11:source path";
    let source_entry = format!("{}{}:{}", source_path_key, file_path.len(), file_path);
    torrent_content.extend_from_slice(source_entry.as_bytes());

    torrent_content.push(b'e');
    torrent_content.push(b'e');

    Ok(torrent_content)
}

fn print_drive_tree(owner: &str) {
    let manifest = drive_storage::load_manifest();
    let mut items: Vec<DriveItem> = manifest
        .items
        .into_iter()
        .filter(|i| i.owner == owner)
        .collect();
    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    fn walk(parent: Option<&str>, depth: usize, items: &[DriveItem]) {
        let mut children: Vec<&DriveItem> = items
            .iter()
            .filter(|i| i.parent_id.as_deref() == parent)
            .collect();
        children.sort_by(|a, b| {
            if a.item_type != b.item_type {
                if a.item_type == "folder" {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            } else {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            }
        });

        for child in children {
            let prefix = "  ".repeat(depth);
            let star = if child.starred { "*" } else { "-" };
            println!(
                "{}{} {} {}",
                prefix,
                star,
                child.item_type,
                encode_html_escape(&child.name)
            );
            if child.item_type == "folder" {
                walk(Some(&child.id), depth + 1, items);
            }
        }
    }

    walk(None, 0, &items);
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

async fn drive_update_item(
    client: &reqwest::Client,
    port: u16,
    owner: &str,
    item_id: &str,
    payload: &UpdateItemPayload,
) -> Result<DriveItem, String> {
    let resp = client
        .put(format!(
            "{}/api/drive/items/{}",
            gateway_base_url(port),
            item_id
        ))
        .header("X-Owner", owner)
        .json(payload)
        .send()
        .await
        .map_err(|e| format!("Drive update request failed: {}", e))?;
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
    drive_update_item(client, port, owner, item_id, &payload).await
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

async fn drive_upload_file(
    client: &reqwest::Client,
    port: u16,
    owner: &str,
    file_path: &str,
    parent_id: Option<String>,
) -> Result<DriveItem, String> {
    let path = PathBuf::from(file_path);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid file name")?
        .to_string();
    let data =
        std::fs::read(&path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

    let part = reqwest::multipart::Part::bytes(data).file_name(file_name);
    let mut form = reqwest::multipart::Form::new().part("file", part);
    if let Some(pid) = parent_id {
        form = form.text("parent_id", pid);
    }

    let resp = client
        .post(format!("{}/api/drive/upload", gateway_base_url(port)))
        .header("X-Owner", owner)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Drive upload request failed: {}", e))?;
    parse_json_or_error(resp).await
}

async fn drive_create_share(
    client: &reqwest::Client,
    port: u16,
    owner: &str,
    item_id: &str,
    password: Option<String>,
    public: bool,
) -> Result<Value, String> {
    let resp = client
        .post(format!("{}/api/drive/share", gateway_base_url(port)))
        .header("X-Owner", owner)
        .json(&serde_json::json!({
            "item_id": item_id,
            "password": password,
            "is_public": public
        }))
        .send()
        .await
        .map_err(|e| format!("Drive share request failed: {}", e))?;
    parse_json_or_error(resp).await
}

async fn publish_drive_item(
    owner: &str,
    item_id: &str,
    port: u16,
    protocol: &str,
    price_chi: Option<String>,
    wallet_address: Option<String>,
) -> Result<String, String> {
    let mut manifest = drive_storage::load_manifest();
    let Some(item) = manifest
        .items
        .iter_mut()
        .find(|i| i.owner == owner && i.id == item_id)
    else {
        return Err("Drive item not found".to_string());
    };
    if item.item_type != "file" {
        return Err("Only files can be published".to_string());
    }

    let Some(storage_path) = item.storage_path.clone() else {
        return Err("File has no storage path".to_string());
    };
    let base = drive_storage::drive_files_dir().ok_or("Drive files directory not available")?;
    let full_path = base.join(&storage_path);
    if !full_path.exists() {
        return Err(format!("File missing on disk: {}", full_path.display()));
    }

    let file_hash = compute_file_hash(&full_path)?;
    let file_size = std::fs::metadata(&full_path)
        .map_err(|e| format!("Failed to stat {}: {}", full_path.display(), e))?
        .len();

    let price_wei = if let Some(price) = &price_chi {
        if price.trim().is_empty() || price.trim() == "0" {
            "0".to_string()
        } else {
            parse_chi_to_wei(price)?.to_string()
        }
    } else {
        "0".to_string()
    };

    daemon_post_json(
        port,
        "/api/headless/dht/register-shared-file",
        &serde_json::json!({
            "fileHash": file_hash,
            "filePath": full_path.to_string_lossy(),
            "fileName": item.name,
            "fileSize": file_size,
            "priceWei": price_wei,
            "walletAddress": wallet_address.clone().unwrap_or_default(),
        }),
    )
    .await?;

    let peer_id = dht_peer_id(port).await?;
    let addresses = dht_listening_addresses(port).await?;

    let dht_key = format!("chiral_file_{}", file_hash);
    let existing = dht_get_value(port, &dht_key).await?;
    let mut metadata = if let Some(json) = existing {
        serde_json::from_str::<FileMetadata>(&json).unwrap_or(FileMetadata {
            hash: file_hash.clone(),
            file_name: item.name.clone(),
            file_size,
            protocol: protocol.to_string(),
            created_at: now_secs(),
            peer_id: String::new(),
            price_wei: String::new(),
            wallet_address: String::new(),
            seeders: Vec::new(),
        })
    } else {
        FileMetadata {
            hash: file_hash.clone(),
            file_name: item.name.clone(),
            file_size,
            protocol: protocol.to_string(),
            created_at: now_secs(),
            peer_id: String::new(),
            price_wei: String::new(),
            wallet_address: String::new(),
            seeders: Vec::new(),
        }
    };

    let seeder = SeederInfo {
        peer_id: peer_id.clone(),
        price_wei: price_wei.clone(),
        wallet_address: wallet_address.clone().unwrap_or_default(),
        multiaddrs: addresses,
    };

    if let Some(existing) = metadata.seeders.iter_mut().find(|s| s.peer_id == peer_id) {
        *existing = seeder;
    } else {
        metadata.seeders.push(seeder);
    }

    metadata.peer_id = peer_id;
    metadata.price_wei = price_wei;
    metadata.wallet_address = wallet_address.unwrap_or_default();
    metadata.protocol = protocol.to_string();

    let metadata_json = serde_json::to_string(&metadata)
        .map_err(|e| format!("Failed to serialize metadata: {}", e))?;
    dht_put_value(port, &dht_key, &metadata_json).await?;

    item.merkle_root = Some(file_hash.clone());
    item.protocol = Some(protocol.to_string());
    item.price_chi = price_chi;
    item.seeding = true;
    drive_storage::save_manifest(&manifest);

    Ok(file_hash)
}

async fn unpublish_drive_item(owner: &str, item_id: &str, port: u16) -> Result<String, String> {
    let mut manifest = drive_storage::load_manifest();
    let Some(item) = manifest
        .items
        .iter_mut()
        .find(|i| i.owner == owner && i.id == item_id)
    else {
        return Err("Drive item not found".to_string());
    };
    if item.item_type != "file" {
        return Err("Only files can be unpublished".to_string());
    }

    let hash = if let Some(h) = &item.merkle_root {
        h.clone()
    } else {
        let Some(storage_path) = item.storage_path.clone() else {
            return Err("File has no storage path".to_string());
        };
        let base = drive_storage::drive_files_dir().ok_or("Drive files directory not available")?;
        let full_path = base.join(storage_path);
        compute_file_hash(&full_path)?
    };

    let dht_key = format!("chiral_file_{}", hash);
    let peer_id = dht_peer_id(port).await?;

    if let Some(raw) = dht_get_value(port, &dht_key).await? {
        if let Ok(mut metadata) = serde_json::from_str::<FileMetadata>(&raw) {
            metadata.seeders.retain(|s| s.peer_id != peer_id);
            metadata.peer_id = metadata
                .seeders
                .first()
                .map(|s| s.peer_id.clone())
                .unwrap_or_default();
            metadata.price_wei = metadata
                .seeders
                .first()
                .map(|s| s.price_wei.clone())
                .unwrap_or_default();
            metadata.wallet_address = metadata
                .seeders
                .first()
                .map(|s| s.wallet_address.clone())
                .unwrap_or_default();
            let json = serde_json::to_string(&metadata)
                .map_err(|e| format!("Failed to serialize metadata: {}", e))?;
            dht_put_value(port, &dht_key, &json).await?;
        }
    }

    daemon_post_json(
        port,
        "/api/headless/dht/unregister-shared-file",
        &serde_json::json!({ "fileHash": hash }),
    )
    .await?;

    item.seeding = false;
    item.protocol = None;
    item.price_chi = None;
    drive_storage::save_manifest(&manifest);

    Ok(hash)
}

fn export_torrent_for_drive(
    owner: &str,
    item_id: &str,
    output: Option<String>,
) -> Result<PathBuf, String> {
    let manifest = drive_storage::load_manifest();
    let item = manifest
        .items
        .iter()
        .find(|i| i.owner == owner && i.id == item_id)
        .ok_or("Drive item not found")?;
    if item.item_type != "file" {
        return Err("Only files can be exported as torrent".to_string());
    }

    let storage = item
        .storage_path
        .as_ref()
        .ok_or("File has no storage path")?;
    let base = drive_storage::drive_files_dir().ok_or("Drive files directory not available")?;
    let full_path = base.join(storage);

    let file_hash = if let Some(h) = &item.merkle_root {
        h.clone()
    } else {
        compute_file_hash(&full_path)?
    };

    let file_size = std::fs::metadata(&full_path)
        .map_err(|e| format!("Failed to stat {}: {}", full_path.display(), e))?
        .len();

    let torrent = build_torrent_file(
        &file_hash,
        &item.name,
        file_size,
        &full_path.to_string_lossy(),
    )?;

    let out_path = if let Some(path) = output {
        PathBuf::from(path)
    } else {
        let downloads = dirs::download_dir().unwrap_or_else(|| PathBuf::from("."));
        downloads.join(format!("{}.torrent", item.name))
    };

    ensure_parent_dir(&out_path)?;
    std::fs::write(&out_path, torrent)
        .map_err(|e| format!("Failed to write {}: {}", out_path.display(), e))?;
    Ok(out_path)
}

async fn handle_daemon(cmd: DaemonCommand) -> Result<(), String> {
    match cmd {
        DaemonCommand::Start { port } => {
            let pid_path = default_pid_file();
            if pid_path.exists() {
                let existing_pid = read_pid_file(&pid_path)?;
                if process_exists(existing_pid) {
                    let healthy = check_gateway_health(port).await;
                    let headless_api = check_headless_api(port).await;
                    if healthy && headless_api {
                        println!(
                            "Daemon already running (pid={}, health={}, headless_api={})",
                            existing_pid, healthy, headless_api
                        );
                        return Ok(());
                    }

                    if healthy && !headless_api {
                        println!(
                            "Found incompatible daemon (pid={}) without headless API; restarting...",
                            existing_pid
                        );
                        terminate_process(existing_pid)?;
                    }
                }
                let _ = std::fs::remove_file(&pid_path);
            }

            if let Some(parent) = pid_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
            }

            let located = locate_daemon_binary().ok();
            let mut launch = if let Some(path) = located {
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
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        return "Failed to start daemon: `chiral_daemon` executable was not found. Build it with `cargo build --manifest-path src-tauri/Cargo.toml --bin chiral_daemon` and retry.".to_string();
                    }
                    format!("Failed to start daemon: {}", e)
                })?;

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
            let mut headless_ready = false;
            for _ in 0..180 {
                if check_headless_api(port).await {
                    headless_ready = true;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            if !headless_ready {
                let _ = terminate_process(pid);
                let _ = std::fs::remove_file(&pid_path);
                return Err(
                    "Daemon started but headless API did not become ready. Rebuild `chiral_daemon` and retry."
                        .to_string(),
                );
            }

            println!("Daemon started (pid={}) on http://127.0.0.1:{}", pid, port);
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
            let headless_api = check_headless_api(port).await;
            println!(
                "pid={} alive={} health={} headless_api={} url=http://127.0.0.1:{}",
                pid, alive, healthy, headless_api, port
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

fn derive_wallet_from_optional(
    address: Option<String>,
    private_key: Option<String>,
) -> Result<(String, String), String> {
    if let (Some(addr), Some(pk)) = (address.clone(), private_key.clone()) {
        return Ok((addr, normalize_private_key(&pk)?));
    }

    let wallet = require_wallet()?;
    let addr = address.unwrap_or(wallet.address);
    let pk = match private_key {
        Some(v) => normalize_private_key(&v)?,
        None => wallet.private_key,
    };
    Ok((addr, pk))
}

async fn handle_wallet(cmd: WalletCommand) -> Result<(), String> {
    match cmd {
        WalletCommand::Create => {
            let wallet = generate_wallet()?;
            let mut store = load_wallet_store()?;
            store.active = Some(wallet.clone());
            save_wallet_store(&store)?;

            println!("address={}", wallet.address);
            println!("private_key={}", wallet.private_key);
            println!("wallet_store={}", wallet_store_path().display());
            Ok(())
        }
        WalletCommand::Import { private_key } => {
            let normalized = normalize_private_key(&private_key)?;
            let address = address_from_private_key(&normalized)?;
            let wallet = WalletProfile {
                address,
                private_key: normalized,
                created_at: now_secs(),
            };
            let mut store = load_wallet_store()?;
            store.active = Some(wallet.clone());
            save_wallet_store(&store)?;
            println!("Imported wallet {}", wallet.address);
            Ok(())
        }
        WalletCommand::Login { private_key } => {
            if let Some(pk) = private_key {
                let normalized = normalize_private_key(&pk)?;
                let address = address_from_private_key(&normalized)?;
                let wallet = WalletProfile {
                    address,
                    private_key: normalized,
                    created_at: now_secs(),
                };
                let mut store = load_wallet_store()?;
                store.active = Some(wallet.clone());
                save_wallet_store(&store)?;
                println!("Imported wallet {}", wallet.address);
                return Ok(());
            }
            let wallet = require_wallet()?;
            println!("Active wallet: {}", wallet.address);
            Ok(())
        }
        WalletCommand::Show => {
            let wallet = require_wallet()?;
            println!("address={}", wallet.address);
            Ok(())
        }
        WalletCommand::Export { reveal_private_key } => {
            let wallet = require_wallet()?;
            println!("address={}", wallet.address);
            if reveal_private_key {
                println!("private_key={}", wallet.private_key);
            }
            Ok(())
        }
    }
}

async fn handle_account(cmd: AccountCommand) -> Result<(), String> {
    match cmd {
        AccountCommand::Balance { address } => {
            let addr = match address {
                Some(v) => v,
                None => require_wallet()?.address,
            };
            let balance = get_wallet_balance(&addr).await?;
            println!("address={}", addr);
            println!("balance_chi={}", balance.balance);
            println!("balance_wei={}", balance.balance_wei);
            Ok(())
        }
        AccountCommand::Send {
            to,
            amount,
            from,
            private_key,
        } => {
            let (from_address, pk) = derive_wallet_from_optional(from, private_key)?;
            let result = send_transaction(&from_address, &to, &amount, &pk).await?;
            println!("hash={}", result.hash);
            println!("status={}", result.status);
            println!("balance_before={}", result.balance_before);
            println!("balance_after={}", result.balance_after);
            Ok(())
        }
        AccountCommand::History {
            address,
            max_blocks,
            limit,
        } => {
            let addr = match address {
                Some(v) => v,
                None => require_wallet()?.address,
            };
            let txs = get_transaction_history(&addr, max_blocks, limit).await?;
            for tx in txs {
                println!(
                    "{} {} {}->{} value={}CHI block={} ts={}",
                    tx.hash, tx.direction, tx.from, tx.to, tx.value, tx.block_number, tx.timestamp
                );
            }
            Ok(())
        }
        AccountCommand::Meta => {
            let wallet = load_wallet_store()?.active;
            println!("rpc_endpoint={}", geth::rpc_endpoint());
            println!("chain_id={}", geth::CHAIN_ID);
            println!(
                "active_wallet={}",
                wallet
                    .map(|w| w.address)
                    .unwrap_or_else(|| "<none>".to_string())
            );
            Ok(())
        }
    }
}

async fn handle_dht(cmd: DhtCommand) -> Result<(), String> {
    match cmd {
        DhtCommand::Start { port } => {
            let value = daemon_post_empty(port, "/api/headless/dht/start").await?;
            print_json(&value)
        }
        DhtCommand::Stop { port } => {
            let value = daemon_post_empty(port, "/api/headless/dht/stop").await?;
            print_json(&value)
        }
        DhtCommand::Status { port } => {
            let value = daemon_get_json(port, "/api/headless/dht/health").await?;
            print_json(&value)
        }
        DhtCommand::PeerId { port } => {
            let value = daemon_get_json(port, "/api/headless/dht/peer-id").await?;
            print_json(&value)
        }
        DhtCommand::Peers { port } => {
            let value = daemon_get_json(port, "/api/headless/dht/peers").await?;
            print_json(&value)
        }
        DhtCommand::Put { key, value, port } => {
            let value = daemon_post_json(
                port,
                "/api/headless/dht/put",
                &serde_json::json!({ "key": key, "value": value }),
            )
            .await?;
            print_json(&value)
        }
        DhtCommand::Get { key, port } => {
            let value = daemon_post_json(
                port,
                "/api/headless/dht/get",
                &serde_json::json!({ "key": key }),
            )
            .await?;
            print_json(&value)
        }
        DhtCommand::Echo {
            peer_id,
            payload,
            payload_is_base64,
            port,
        } => {
            let payload_base64 = if payload_is_base64 {
                payload
            } else {
                base64::engine::general_purpose::STANDARD.encode(payload.as_bytes())
            };
            let value = daemon_post_json(
                port,
                "/api/headless/dht/echo",
                &serde_json::json!({
                    "peerId": peer_id,
                    "payloadBase64": payload_base64,
                }),
            )
            .await?;
            print_json(&value)
        }
        DhtCommand::Listening { port } => {
            let value = daemon_get_json(port, "/api/headless/dht/listening-addresses").await?;
            print_json(&value)
        }
    }
}

async fn handle_download(cmd: DownloadCommand) -> Result<(), String> {
    match cmd {
        DownloadCommand::Search { hash, port } => {
            let key = format!("chiral_file_{}", hash);
            let maybe_json = dht_get_value(port, &key).await?;
            if let Some(raw) = maybe_json {
                let metadata: FileMetadata = serde_json::from_str(&raw)
                    .map_err(|e| format!("Failed to parse metadata: {}", e))?;
                let mut seeders = metadata.seeders.clone();
                if seeders.is_empty() && !metadata.peer_id.is_empty() {
                    seeders.push(SeederInfo {
                        peer_id: metadata.peer_id.clone(),
                        price_wei: metadata.price_wei.clone(),
                        wallet_address: metadata.wallet_address.clone(),
                        multiaddrs: vec![],
                    });
                }
                println!("hash={}", metadata.hash);
                println!("file_name={}", metadata.file_name);
                println!("file_size={}", metadata.file_size);
                println!("protocol={}", metadata.protocol);
                println!("created_at={}", metadata.created_at);
                println!("seeders={}", seeders.len());
                for s in seeders {
                    println!(
                        "  peer_id={} price_wei={} wallet={} addrs={}",
                        s.peer_id,
                        s.price_wei,
                        s.wallet_address,
                        s.multiaddrs.join(",")
                    );
                }
            } else {
                println!("not_found");
            }
            Ok(())
        }
        DownloadCommand::Start {
            hash,
            file_name,
            peer_id,
            request_id,
            multiaddr,
            port,
        } => {
            let rid = request_id.unwrap_or_else(|| {
                format!(
                    "dl-{}-{}",
                    &hash[..std::cmp::min(8, hash.len())],
                    now_secs()
                )
            });

            let _ = daemon_post_json(
                port,
                "/api/headless/dht/request-file",
                &serde_json::json!({
                    "peerId": peer_id,
                    "fileHash": hash,
                    "requestId": rid,
                    "multiaddrs": multiaddr,
                }),
            )
            .await?;

            let mut history = load_download_history()?;
            history.push(DownloadRecord {
                request_id: rid.clone(),
                file_hash: hash,
                file_name,
                peer_id,
                status: "requested".to_string(),
                created_at: now_secs(),
            });
            save_download_history(&history)?;

            println!("request_id={}", rid);
            println!("status=requested");
            Ok(())
        }
        DownloadCommand::Cost {
            file_size_bytes,
            tier,
        } => {
            let cost_wei = calculate_download_cost_wei(file_size_bytes, &tier)?;
            println!("tier={}", tier);
            println!("file_size_bytes={}", file_size_bytes);
            println!("cost_wei={}", cost_wei);
            println!("cost_chi={}", format_wei_as_chi(cost_wei));
            Ok(())
        }
        DownloadCommand::List | DownloadCommand::History => {
            let history = load_download_history()?;
            for item in history {
                println!(
                    "{} {} hash={} file={} peer={} ts={}",
                    item.request_id,
                    item.status,
                    item.file_hash,
                    item.file_name,
                    item.peer_id,
                    item.created_at
                );
            }
            Ok(())
        }
        DownloadCommand::Watch {
            request_id,
            interval_ms,
            attempts,
        } => {
            let mut history = load_download_history()?;
            let Some(index) = history.iter().position(|r| r.request_id == request_id) else {
                return Err(format!("request_id {} not found in history", request_id));
            };

            for _ in 0..attempts {
                if history[index].status == "completed" {
                    println!("status=completed");
                    return Ok(());
                }

                let download_dir = dirs::download_dir().unwrap_or_else(|| PathBuf::from("."));
                let expected_file = download_dir.join(&history[index].file_name);
                if expected_file.exists() {
                    history[index].status = "completed".to_string();
                    save_download_history(&history)?;
                    println!("status=completed");
                    println!("file_path={}", expected_file.display());
                    return Ok(());
                }

                println!("status=pending");
                tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
            }

            Err("Timed out while waiting for download completion".to_string())
        }
    }
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
                println!("{} {} {} {}", marker, item.id, item.item_type, item.name);
            }
            Ok(())
        }
        DriveCommand::Tree { owner } => {
            print_drive_tree(&owner);
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
        DriveCommand::Upload {
            owner,
            file_path,
            parent_id,
            port,
        } => {
            let item = drive_upload_file(&client, port, &owner, &file_path, parent_id).await?;
            println!(
                "uploaded id={} name={} size={}",
                item.id,
                item.name,
                item.size.unwrap_or(0)
            );
            Ok(())
        }
        DriveCommand::Rename {
            owner,
            item_id,
            name,
            port,
        } => {
            let payload = UpdateItemPayload {
                name: Some(name),
                parent_id: None,
                starred: None,
            };
            let item = drive_update_item(&client, port, &owner, &item_id, &payload).await?;
            println!("renamed id={} name={}", item.id, item.name);
            Ok(())
        }
        DriveCommand::Move {
            owner,
            item_id,
            parent_id,
            port,
        } => {
            let payload = UpdateItemPayload {
                name: None,
                parent_id,
                starred: None,
            };
            let item = drive_update_item(&client, port, &owner, &item_id, &payload).await?;
            println!("moved id={} parent_id={:?}", item.id, item.parent_id);
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
        DriveCommand::Visibility {
            owner,
            item_id,
            public,
        } => {
            let mut manifest = drive_storage::load_manifest();
            let Some(item) = manifest
                .items
                .iter_mut()
                .find(|i| i.owner == owner && i.id == item_id)
            else {
                return Err("Drive item not found".to_string());
            };
            item.is_public = public;
            item.modified_at = now_secs();
            drive_storage::save_manifest(&manifest);
            println!("visibility id={} public={}", item_id, public);
            Ok(())
        }
        DriveCommand::Share {
            owner,
            item_id,
            password,
            public,
            port,
        } => {
            let value =
                drive_create_share(&client, port, &owner, &item_id, password, public).await?;
            print_json(&value)
        }
        DriveCommand::Publish {
            owner,
            item_id,
            port,
            protocol,
            price_chi,
            wallet_address,
        } => {
            let hash =
                publish_drive_item(&owner, &item_id, port, &protocol, price_chi, wallet_address)
                    .await?;
            println!("published hash={}", hash);
            Ok(())
        }
        DriveCommand::Unpublish {
            owner,
            item_id,
            port,
        } => {
            let hash = unpublish_drive_item(&owner, &item_id, port).await?;
            println!("unpublished hash={}", hash);
            Ok(())
        }
        DriveCommand::ExportTorrent {
            owner,
            item_id,
            output,
        } => {
            let out = export_torrent_for_drive(&owner, &item_id, output)?;
            println!("torrent_path={}", out.display());
            Ok(())
        }
    }
}

async fn handle_drop(cmd: DropCommand) -> Result<(), String> {
    match cmd {
        DropCommand::Peers { port } => {
            let value = daemon_get_json(port, "/api/headless/dht/peers").await?;
            print_json(&value)
        }
        DropCommand::Send {
            peer_id,
            file_path,
            port,
        } => {
            let name = Path::new(&file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("Invalid file path")?
                .to_string();
            let transfer_id = format!("drop-{}", now_secs());
            let value = daemon_post_json(
                port,
                "/api/headless/dht/send-file",
                &serde_json::json!({
                    "peerId": peer_id,
                    "transferId": transfer_id,
                    "fileName": name,
                    "filePath": file_path,
                    "priceWei": "0",
                    "senderWallet": "",
                    "fileHash": "",
                }),
            )
            .await?;
            print_json(&value)?;

            let mut history = load_drop_history()?;
            history.push(DropRecord {
                transfer_id,
                peer_id,
                file_name: name,
                file_path,
                status: "sent".to_string(),
                paid: false,
                created_at: now_secs(),
            });
            save_drop_history(&history)?;
            Ok(())
        }
        DropCommand::SendPaid {
            peer_id,
            file_path,
            price_wei,
            sender_wallet,
            file_hash,
            port,
        } => {
            let name = Path::new(&file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("Invalid file path")?
                .to_string();
            let transfer_id = format!("drop-paid-{}", now_secs());
            let hash = if let Some(h) = file_hash {
                h
            } else {
                compute_file_hash(Path::new(&file_path))?
            };

            let value = daemon_post_json(
                port,
                "/api/headless/dht/send-file",
                &serde_json::json!({
                    "peerId": peer_id,
                    "transferId": transfer_id,
                    "fileName": name,
                    "filePath": file_path,
                    "priceWei": price_wei,
                    "senderWallet": sender_wallet,
                    "fileHash": hash,
                }),
            )
            .await?;
            print_json(&value)?;

            let mut history = load_drop_history()?;
            history.push(DropRecord {
                transfer_id,
                peer_id,
                file_name: name,
                file_path,
                status: "sent".to_string(),
                paid: true,
                created_at: now_secs(),
            });
            save_drop_history(&history)?;
            Ok(())
        }
        DropCommand::Inbox { port } => {
            let value = daemon_get_json(port, "/api/headless/drop/inbox").await?;
            print_json(&value)
        }
        DropCommand::Accept {
            transfer_id,
            download_dir,
            port,
        } => {
            let value = daemon_post_json(
                port,
                "/api/headless/drop/accept",
                &serde_json::json!({
                    "transferId": transfer_id,
                    "downloadDir": download_dir,
                }),
            )
            .await?;
            print_json(&value)
        }
        DropCommand::Decline { transfer_id, port } => {
            let value = daemon_post_json(
                port,
                "/api/headless/drop/decline",
                &serde_json::json!({ "key": transfer_id }),
            )
            .await?;
            print_json(&value)
        }
        DropCommand::History => {
            let history = load_drop_history()?;
            for item in history {
                println!(
                    "{} {} peer={} file={} paid={} ts={}",
                    item.transfer_id,
                    item.status,
                    item.peer_id,
                    item.file_name,
                    item.paid,
                    item.created_at
                );
            }
            Ok(())
        }
    }
}

fn collect_site_files(directory: &Path, base: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(directory)
        .map_err(|e| format!("Failed to read {}: {}", directory.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(collect_site_files(&path, base)?);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(base)
            .map_err(|e| format!("Failed to build relative path: {}", e))?
            .to_string_lossy()
            .replace('\\', "/");
        let data = std::fs::read(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        out.push((rel, data));
    }

    Ok(out)
}

async fn handle_hosting(cmd: HostingCommand) -> Result<(), String> {
    match cmd {
        HostingCommand::Server { cmd } => handle_daemon(cmd).await,
        HostingCommand::Site { cmd } => match cmd {
            HostingSiteCommand::Create {
                name,
                directory,
                site_id,
                port,
            } => {
                let dir = PathBuf::from(&directory);
                if !dir.exists() || !dir.is_dir() {
                    return Err(format!("Directory not found: {}", directory));
                }
                let id = site_id.unwrap_or_else(hosting::generate_site_id);
                let files = collect_site_files(&dir, &dir)?;
                if files.is_empty() {
                    return Err("No files found in site directory".to_string());
                }

                let payload_files: Vec<Value> = files
                    .into_iter()
                    .map(|(path, data)| {
                        serde_json::json!({
                            "path": path,
                            "data": base64::engine::general_purpose::STANDARD.encode(data),
                        })
                    })
                    .collect();

                let value = daemon_post_json(
                    port,
                    "/api/sites",
                    &serde_json::json!({
                        "id": id,
                        "name": name,
                        "files": payload_files,
                    }),
                )
                .await?;
                print_json(&value)
            }
            HostingSiteCommand::List => {
                let mut sites = hosting::load_sites();
                sites.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                for site in sites {
                    println!(
                        "id={} name={} files={} created_at={} relay_url={}",
                        site.id,
                        site.name,
                        site.files.len(),
                        site.created_at,
                        site.relay_url.unwrap_or_default()
                    );
                }
                Ok(())
            }
            HostingSiteCommand::Delete { site_id, port } => {
                let client = reqwest::Client::new();
                let resp = client
                    .delete(format!("{}/api/sites/{}", gateway_base_url(port), site_id))
                    .send()
                    .await
                    .map_err(|e| format!("Delete request failed: {}", e))?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(format!("HTTP {}: {}", status, body));
                }
                println!("deleted site {}", site_id);
                Ok(())
            }
        },
        HostingCommand::PublishRelay { site_id, relay_url } => {
            let mut sites = hosting::load_sites();
            let Some(site) = sites.iter_mut().find(|s| s.id == site_id) else {
                return Err(format!("Site {} not found", site_id));
            };
            site.relay_url = Some(relay_url.clone());
            hosting::save_sites(&sites);
            println!("site={} relay_url={}", site_id, relay_url);
            Ok(())
        }
        HostingCommand::UnpublishRelay { site_id } => {
            let mut sites = hosting::load_sites();
            let Some(site) = sites.iter_mut().find(|s| s.id == site_id) else {
                return Err(format!("Site {} not found", site_id));
            };
            site.relay_url = None;
            hosting::save_sites(&sites);
            println!("site={} relay_url=<none>", site_id);
            Ok(())
        }
    }
}

async fn handle_market(cmd: MarketCommand) -> Result<(), String> {
    match cmd {
        MarketCommand::Advertise {
            wallet,
            max_storage_bytes,
            price_per_mb_per_day_wei,
            min_deposit_wei,
            port,
        } => {
            let peer_id = dht_peer_id(port).await?;
            let ad = serde_json::json!({
                "peerId": peer_id,
                "walletAddress": wallet,
                "maxStorageBytes": max_storage_bytes,
                "pricePerMbPerDayWei": price_per_mb_per_day_wei,
                "minDepositWei": min_deposit_wei,
                "updatedAt": now_secs(),
            });
            let ad_json = serde_json::to_string(&ad)
                .map_err(|e| format!("Failed to serialize advertisement: {}", e))?;

            dht_put_value(port, &format!("chiral_host_{}", peer_id), &ad_json).await?;

            let registry_key = "chiral_host_registry";
            let mut registry: Vec<HostRegistryEntry> =
                if let Some(raw) = dht_get_value(port, registry_key).await? {
                    serde_json::from_str(&raw).unwrap_or_default()
                } else {
                    Vec::new()
                };
            registry.retain(|e| e.peer_id != peer_id);
            registry.push(HostRegistryEntry {
                peer_id,
                wallet_address: wallet,
                updated_at: now_secs(),
            });

            let reg_json = serde_json::to_string(&registry)
                .map_err(|e| format!("Failed to serialize registry: {}", e))?;
            dht_put_value(port, registry_key, &reg_json).await?;
            println!("advertised=true");
            Ok(())
        }
        MarketCommand::Unadvertise { port } => {
            let peer_id = dht_peer_id(port).await?;
            let registry_key = "chiral_host_registry";
            let mut registry: Vec<HostRegistryEntry> =
                if let Some(raw) = dht_get_value(port, registry_key).await? {
                    serde_json::from_str(&raw).unwrap_or_default()
                } else {
                    Vec::new()
                };
            registry.retain(|e| e.peer_id != peer_id);
            let json = serde_json::to_string(&registry)
                .map_err(|e| format!("Failed to serialize registry: {}", e))?;
            dht_put_value(port, registry_key, &json).await?;
            println!("advertised=false");
            Ok(())
        }
        MarketCommand::Browse { port } => {
            let registry_key = "chiral_host_registry";
            let Some(raw) = dht_get_value(port, registry_key).await? else {
                println!("[]");
                return Ok(());
            };
            let registry: Vec<HostRegistryEntry> = serde_json::from_str(&raw).unwrap_or_default();
            for entry in registry {
                let key = format!("chiral_host_{}", entry.peer_id);
                let ad = dht_get_value(port, &key).await?;
                println!(
                    "peer_id={} wallet={} updated_at={} ad={}",
                    entry.peer_id,
                    entry.wallet_address,
                    entry.updated_at,
                    ad.unwrap_or_default()
                );
            }
            Ok(())
        }
        MarketCommand::Propose {
            peer_id,
            agreement_id,
            agreement_json,
            port,
        } => {
            let agreement_value: Value = serde_json::from_str(&agreement_json)
                .map_err(|e| format!("Invalid agreement JSON: {}", e))?;
            let payload = serde_json::json!({
                "type": "hosting_proposal",
                "agreement": agreement_value,
            });
            let bytes = serde_json::to_vec(&payload)
                .map_err(|e| format!("Failed to serialize proposal: {}", e))?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);

            let _ = daemon_post_json(
                port,
                "/api/headless/dht/echo",
                &serde_json::json!({
                    "peerId": peer_id,
                    "payloadBase64": encoded,
                }),
            )
            .await?;

            store_agreement(&agreement_id, &agreement_json)?;
            println!("proposal_sent agreement_id={}", agreement_id);
            Ok(())
        }
        MarketCommand::Agreements => {
            for id in list_agreements()? {
                println!("{}", id);
            }
            Ok(())
        }
        MarketCommand::Respond {
            peer_id,
            agreement_id,
            status,
            port,
        } => {
            if let Some(existing) = load_agreement(&agreement_id)? {
                if let Ok(mut json) = serde_json::from_str::<Value>(&existing) {
                    if let Some(obj) = json.as_object_mut() {
                        obj.insert("status".to_string(), Value::String(status.clone()));
                        let updated = serde_json::to_string_pretty(&json)
                            .map_err(|e| format!("Failed to serialize agreement: {}", e))?;
                        store_agreement(&agreement_id, &updated)?;
                    }
                }
            }

            let payload = serde_json::json!({
                "type": "hosting_response",
                "agreementId": agreement_id,
                "status": status,
            });
            let encoded = base64::engine::general_purpose::STANDARD.encode(
                serde_json::to_vec(&payload)
                    .map_err(|e| format!("Failed to encode response: {}", e))?,
            );

            let _ = daemon_post_json(
                port,
                "/api/headless/dht/echo",
                &serde_json::json!({
                    "peerId": peer_id,
                    "payloadBase64": encoded,
                }),
            )
            .await?;
            println!("response_sent");
            Ok(())
        }
        MarketCommand::Cancel {
            peer_id,
            agreement_id,
            port,
        } => {
            let payload = serde_json::json!({
                "type": "hosting_cancel_request",
                "agreementId": agreement_id,
            });
            let encoded = base64::engine::general_purpose::STANDARD.encode(
                serde_json::to_vec(&payload)
                    .map_err(|e| format!("Failed to encode cancel request: {}", e))?,
            );

            let _ = daemon_post_json(
                port,
                "/api/headless/dht/echo",
                &serde_json::json!({
                    "peerId": peer_id,
                    "payloadBase64": encoded,
                }),
            )
            .await?;
            println!("cancel_request_sent");
            Ok(())
        }
        MarketCommand::Cleanup => {
            let dir = ensure_agreements_dir()?;
            let mut removed = 0usize;
            let entries = std::fs::read_dir(&dir)
                .map_err(|e| format!("Failed to read agreements dir {}: {}", dir.display(), e))?;
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let raw = std::fs::read_to_string(&path).unwrap_or_default();
                if let Ok(json) = serde_json::from_str::<Value>(&raw) {
                    let status = json
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();
                    if matches!(status, "cancelled" | "completed" | "rejected") {
                        let _ = std::fs::remove_file(&path);
                        removed += 1;
                    }
                }
            }
            println!("removed={}", removed);
            Ok(())
        }
    }
}

async fn handle_mining(cmd: MiningCommand) -> Result<(), String> {
    match cmd {
        MiningCommand::Install { port } => {
            let value = daemon_post_empty(port, "/api/headless/geth/install").await?;
            print_json(&value)
        }
        MiningCommand::Start { threads, port } => {
            let value = daemon_post_json(
                port,
                "/api/headless/mining/start",
                &serde_json::json!({ "threads": threads }),
            )
            .await?;
            print_json(&value)
        }
        MiningCommand::Stop { port } => {
            let value = daemon_post_empty(port, "/api/headless/mining/stop").await?;
            print_json(&value)
        }
        MiningCommand::Status { port } => {
            let value = daemon_get_json(port, "/api/headless/mining/status").await?;
            print_json(&value)
        }
        MiningCommand::Blocks { max, port } => {
            let value = daemon_get_json_with_query(
                port,
                "/api/headless/mining/blocks",
                &[("max", max.to_string())],
            )
            .await?;
            print_json(&value)
        }
    }
}

async fn handle_geth(cmd: GethCommand) -> Result<(), String> {
    match cmd {
        GethCommand::Start {
            miner_address,
            port,
        } => {
            let value = daemon_post_json(
                port,
                "/api/headless/geth/start",
                &serde_json::json!({ "minerAddress": miner_address }),
            )
            .await?;
            print_json(&value)
        }
        GethCommand::Stop { port } => {
            let value = daemon_post_empty(port, "/api/headless/geth/stop").await?;
            print_json(&value)
        }
        GethCommand::Status { port } => {
            let value = daemon_get_json(port, "/api/headless/geth/status").await?;
            print_json(&value)
        }
        GethCommand::Logs { lines, port } => {
            let value = if let Some(lines) = lines {
                daemon_get_json_with_query(
                    port,
                    "/api/headless/geth/logs",
                    &[("lines", lines.to_string())],
                )
                .await?
            } else {
                daemon_get_json(port, "/api/headless/geth/logs").await?
            };
            if let Some(logs) = value.get("logs").and_then(|v| v.as_str()) {
                println!("{}", logs);
                Ok(())
            } else {
                print_json(&value)
            }
        }
    }
}

async fn handle_hosting_daemon_passthrough(cmd: DaemonCommand) -> Result<(), String> {
    handle_daemon(cmd).await
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
        Commands::Wallet { cmd } => handle_wallet(cmd).await,
        Commands::Account { cmd } => handle_account(cmd).await,
        Commands::Dht { cmd } => handle_dht(cmd).await,
        Commands::Download { cmd } => handle_download(cmd).await,
        Commands::Drive { cmd } => handle_drive(cmd).await,
        Commands::ChiralDrop { cmd } => handle_drop(cmd).await,
        Commands::Hosting { cmd } => match cmd {
            HostingCommand::Server { cmd } => handle_hosting_daemon_passthrough(cmd).await,
            other => handle_hosting(other).await,
        },
        Commands::Market { cmd } => handle_market(cmd).await,
        Commands::Mining { cmd } => handle_mining(cmd).await,
        Commands::Geth { cmd } => handle_geth(cmd).await,
    };

    if let Err(err) = result {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }
}
