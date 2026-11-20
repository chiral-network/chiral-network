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

/// Termination requests that the daemon can intercept: `SIGTERM`, `SIGINT` and a terminal
/// hang-up (`SIGHUP`) on Unix; Ctrl-C, Ctrl-Break and closing the console window on Windows.
/// Each takes the shutdown path, during startup as while serving (provider-daemon-lifecycle
/// requirement 11, design decision 7). tokio installs a handler only when a stream is created,
/// so these exist before any engine starts: a request during startup must stop the engines
/// already started instead of ending the daemon by the default action, which on macOS orphans
/// them (book lines 634 and 642). `SIGQUIT` keeps its default action (design observation 9).
/// A `SIGHUP` that the daemon started ignoring, as under `nohup`, stays ignored (design, Risks):
/// tokio's handler would replace that disposition, so no hang-up stream is created then.
/// On Windows, the system ends the process a short, system-defined time after a console close
/// (tokio keeps its handler from returning until then), so the path can be cut short there; the
/// job object and startup recovery cover the rest.
struct Termination {
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    /// `None` when the daemon started with `SIGHUP` ignored.
    #[cfg(unix)]
    hangup: Option<tokio::signal::unix::Signal>,
    #[cfg(windows)]
    ctrl_c: tokio::signal::windows::CtrlC,
    #[cfg(windows)]
    ctrl_break: tokio::signal::windows::CtrlBreak,
    #[cfg(windows)]
    ctrl_close: tokio::signal::windows::CtrlClose,
}

impl Termination {
    fn install() -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            Ok(Self {
                terminate: signal(SignalKind::terminate())?,
                interrupt: signal(SignalKind::interrupt())?,
                hangup: if hangup_ignored() { None } else { Some(signal(SignalKind::hangup())?) },
            })
        }
        #[cfg(windows)]
        {
            use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close};
            Ok(Self {
                ctrl_c: ctrl_c()?,
                ctrl_break: ctrl_break()?,
                ctrl_close: ctrl_close()?,
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Ok(Self {})
        }
    }

    async fn requested(&mut self) {
        #[cfg(unix)]
        tokio::select! {
            _ = self.terminate.recv() => {},
            _ = self.interrupt.recv() => {},
            _ = next_or_never(self.hangup.as_mut()) => {},
        }
        #[cfg(windows)]
        tokio::select! {
            _ = self.ctrl_c.recv() => {},
            _ = self.ctrl_break.recv() => {},
            _ = self.ctrl_close.recv() => {},
        }
        #[cfg(not(any(unix, windows)))]
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Whether this process started with `SIGHUP` ignored, as `nohup` starts it. Read before any
/// hang-up handler is installed.
#[cfg(unix)]
fn hangup_ignored() -> bool {
    // SAFETY: with a null new action, `sigaction` only reads the current disposition into
    // `current`, a plain C struct for which all-zero bytes are valid.
    let mut current: libc::sigaction = unsafe { std::mem::zeroed() };
    let read = unsafe { libc::sigaction(libc::SIGHUP, std::ptr::null(), &mut current) };
    read == 0 && current.sa_sigaction == libc::SIG_IGN
}

/// The next delivery of `signal`, or never when its stream was not created.
#[cfg(unix)]
async fn next_or_never(signal: Option<&mut tokio::signal::unix::Signal>) {
    match signal {
        Some(signal) => {
            signal.recv().await;
        }
        None => std::future::pending().await,
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
        config.verify_separate_engine_keys()?;
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
    let mut termination = Termination::install()?;
    // Bind both before starting DHT/maintenance, so a failed operator bind cannot leave
    // an uncontrollable public provider running.
    let public_listener = tokio::net::TcpListener::bind(config.public_bind).await?;
    let admin_listener = tokio::net::TcpListener::bind(config.admin_bind).await?;
    let public_addr = public_listener.local_addr()?;
    let admin_addr = admin_listener.local_addr()?;
    // Dropping an unfinished start drops its `ManagedEngine`s and the child of a launch in
    // progress, which kills them; nothing has been served or published by then.
    let runtime = tokio::select! {
        started = ProviderRuntime::start(config) => started?,
        _ = termination.requested() => {
            return Err("termination requested during startup; the engines already started were stopped and nothing was served".into());
        }
    };
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
    // A failed starting pass is not a failed start (provider-daemon-lifecycle requirement 7);
    // `/admin/status` keeps reporting it until a later pass succeeds.
    if let Some(error) = runtime.refresh_error().await {
        eprintln!("Starting maintenance pass failed: {error}; running without new funding or published offers until a later pass succeeds; see /admin/status for admission");
    }
    let mut server_error = None;
    let mut public_finished = false;
    let mut admin_finished = false;
    tokio::select! {
        result = &mut public => { public_finished = true; server_error = Some(format!("public listener stopped: {result:?}")); },
        result = &mut admin => { admin_finished = true; server_error = Some(format!("administration listener stopped: {result:?}")); },
        _ = termination.requested() => {}
    }
    maintenance.abort();
    let _ = maintenance.await;
    // The shutdown drain: from here no administration action, including a `POST /admin/resume`
    // already received on an accepted connection, reopens admission or publishes
    // (provider-daemon-lifecycle requirement 11, design code difference 9).
    runtime.drain_for_shutdown().await;
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
