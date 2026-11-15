use chiral_network::{app::ProviderRuntime, provider_config::ProviderConfig, version};
use clap::Parser;
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(
    name = "chiral_daemon",
    version,
    about = "Chiral private-engine inference provider"
)]
struct Args {
    /// Explicit provider, model and network configuration; no legacy presets.
    #[arg(long, env = "CHIRAL_PROVIDER_CONFIG")]
    config: PathBuf,
    /// Offline-signed network version policy. Required outside development mode.
    #[arg(long)]
    version_policy: Option<PathBuf>,
    /// Validate configuration and protected credentials without starting services.
    #[arg(long)]
    check: bool,
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let config = ProviderConfig::load(&args.config)?;
    version::log_policy_key_status();
    let policy_directory = config
        .network
        .scoped_dir(&config.data_dir)
        .join("version-policy");
    version::restore_persisted_policy(&policy_directory)?;
    if let Some(path) = args.version_policy {
        if std::fs::metadata(&path)?.len() > 65536 {
            return Err("version policy exceeds size limit".into());
        }
        let policy: version::VersionPolicy = serde_json::from_slice(&std::fs::read(path)?)?;
        if !version::verify_policy(&policy)
            || !version::update_persisted_policy(&policy_directory, policy)?
        {
            return Err("version policy signature, time or monotonicity rejected".into());
        }
    } else if !config.development {
        return Err(
            "public providers require --version-policy with an offline-signed policy".into(),
        );
    }
    version::compare_to_policy(version::CURRENT_VERSION, &version::effective_policy())?;
    if args.check {
        let key = chiral_network::wallet::load_private_key(&config.signing_key_file)?;
        let _ = chiral_network::wallet::address_from_private_key(&key)?;
        let token = chiral_network::provider_config::read_secret(&config.admin_token_file)?;
        if token.len() < 32 {
            return Err("administration token must contain at least 32 random characters".into());
        }
        for model in &config.models {
            if model.runtime.is_some() {
                chiral_network::engine_runtime::validate_inventory(model)?;
            } else if let Some(path) = &model.engine_api_key_file {
                chiral_network::provider_config::read_secret(path)?;
            }
        }
        println!("Provider configuration and protected credentials validated; no services started");
        return Ok(());
    }
    // Bind both before starting DHT/maintenance, so a failed operator bind cannot leave
    // an uncontrollable public provider running.
    let public_listener = tokio::net::TcpListener::bind(config.public_bind).await?;
    let admin_listener = tokio::net::TcpListener::bind(config.admin_bind).await?;
    let public_addr = public_listener.local_addr()?;
    let admin_addr = admin_listener.local_addr()?;
    let runtime = ProviderRuntime::start(config).await?;
    let maintenance = runtime.maintenance();
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let mut public_stop = stop_rx.clone();
    let mut admin_stop = stop_rx;
    let mut public = tokio::spawn(
        axum::serve(public_listener, runtime.public_router())
            .with_graceful_shutdown(async move {
                let _ = public_stop.changed().await;
            })
            .into_future(),
    );
    let mut admin = tokio::spawn(
        axum::serve(admin_listener, runtime.admin_router())
            .with_graceful_shutdown(async move {
                let _ = admin_stop.changed().await;
            })
            .into_future(),
    );
    println!(
        "Chiral provider public={public_addr} administration={admin_addr} wallet={} peer_id={}",
        runtime.state.wallet,
        runtime.dht.peer_id()
    );
    let mut server_error = None;
    let mut public_finished = false;
    let mut admin_finished = false;
    tokio::select! {
        result = &mut public => { public_finished = true; server_error = Some(format!("public listener stopped: {result:?}")); },
        result = &mut admin => { admin_finished = true; server_error = Some(format!("administration listener stopped: {result:?}")); },
        _ = shutdown_signal() => {}
    }
    maintenance.abort();
    let _ = maintenance.await;
    runtime.drain().await;
    stop_tx.send_replace(true);
    // Existing generation may finish and settle, but shutdown cannot hang indefinitely.
    let joined = tokio::time::timeout(Duration::from_secs(75), async {
        if !public_finished {
            let _ = (&mut public).await;
        }
        if !admin_finished {
            let _ = (&mut admin).await;
        }
    })
    .await;
    if joined.is_err() {
        public.abort();
        admin.abort();
    }
    runtime.stop_engines().await;
    runtime.dht.stop().await;
    if let Some(error) = server_error {
        return Err(error.into());
    }
    Ok(())
}

use std::future::IntoFuture;
