use clap::Parser;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chiral_network::drive_api::DriveState;
use chiral_network::hosting_server::{self, HostingServerState};
use chiral_network::rating_storage::RatingState;

#[derive(Parser, Debug)]
#[command(name = "chiral_daemon")]
#[command(about = "Chiral Network headless daemon")]
struct DaemonArgs {
    /// Local gateway port (Drive + Rating + Hosting routes)
    #[arg(long, default_value_t = 9419)]
    port: u16,

    /// Optional PID file path
    #[arg(long)]
    pid_file: Option<PathBuf>,
}

fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("chiral-network")
}

fn default_pid_file() -> PathBuf {
    default_data_dir().join("headless").join("chiral-daemon.pid")
}

fn write_pid_file(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create PID directory {}: {}", parent.display(), e))?;
    }
    let pid = std::process::id();
    std::fs::write(path, pid.to_string())
        .map_err(|e| format!("Failed to write PID file {}: {}", path.display(), e))?;
    Ok(())
}

fn remove_pid_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[tokio::main]
async fn main() {
    let args = DaemonArgs::parse();
    let pid_file = args.pid_file.unwrap_or_else(default_pid_file);

    if let Err(e) = write_pid_file(&pid_file) {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    let hosting_state = Arc::new(HostingServerState::new());
    hosting_state.load_from_disk().await;

    let drive_state = Arc::new(DriveState::new());
    drive_state.load_from_disk_async().await;

    let rating_state = Arc::new(RatingState::new(default_data_dir()));

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let bound = match hosting_server::start_gateway_server(
        Arc::clone(&hosting_state),
        Some(Arc::clone(&drive_state)),
        Some(Arc::clone(&rating_state)),
        None,
        args.port,
        shutdown_rx,
    )
    .await
    {
        Ok(addr) => addr,
        Err(e) => {
            remove_pid_file(&pid_file);
            eprintln!("Failed to start headless gateway: {}", e);
            std::process::exit(1);
        }
    };

    println!("chiral-daemon running on http://{}", bound);
    println!("PID file: {}", pid_file.display());

    if let Err(e) = tokio::signal::ctrl_c().await {
        eprintln!("Signal handler error: {}", e);
    }

    let _ = shutdown_tx.send(());
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    remove_pid_file(&pid_file);
}

