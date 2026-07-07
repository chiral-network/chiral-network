//! Desktop provider mode — run a **light provider** from inside the desktop
//! process, no full node required. It reuses the provider server assembly
//! (`provider_daemon::build`: contract handshake + data-plane router, backed by
//! the real remote-RPC `RpcChainVerifier`) and publishes its signed offer through
//! the desktop's existing **client-mode** DHT node (`provider_daemon::publish_offer`)
//! instead of spinning up a second DHT.
//!
//! What is *not* lighter: the provider still terminates a public HTTPS endpoint
//! for its data plane (the "no NAT in v1" rule — see the book's Node model). This
//! module binds the configured address and serves; reachability is the operator's
//! responsibility.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use tokio::task::AbortHandle;

use crate::dht::DhtService;
use crate::provider_daemon::{self, Assembled, ProviderConfig};
use crate::resource_offer::ResourceOffer;

/// Republish the offer a little more often than the DHT record TTL so it stays
/// discoverable (matches the book's ≈2–3 min refresh).
const REPUBLISH_EVERY: Duration = Duration::from_secs(150);

/// A provider server running inside the desktop process. Dropping/stopping it
/// aborts the serving + republish tasks.
pub struct RunningProvider {
    handles: Vec<AbortHandle>,
    pub offer: ResourceOffer,
    pub control_bind: String,
    pub data_bind: Option<String>,
}

impl RunningProvider {
    /// Abort the serving + republish task(s). The published offer lingers in the
    /// DHT until its TTL expires (a couple of minutes); consumers that reach the
    /// now-closed endpoint just fail over to another provider.
    pub fn stop(&self) {
        for h in &self.handles {
            h.abort();
        }
    }
}

/// Bind `addr` and spawn an axum server for `router`, returning its abort handle.
/// Binds eagerly so "address in use" / permission errors surface to the caller
/// rather than dying silently in the background task.
async fn spawn_serve(addr: &str, router: Router) -> Result<AbortHandle, String> {
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    let task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            eprintln!("[provider] serve error: {e}");
        }
    });
    Ok(task.abort_handle())
}

/// Start a light provider: assemble the server for `config`, serve it on the
/// configured bind address(es), publish the signed offer through `dht` (the
/// desktop's client-mode node), and keep republishing it on an interval.
///
/// On any bind or publish failure the already-started tasks are torn down, so a
/// partial start never leaves a listener bound or an unadvertised server running.
pub async fn start(config: ProviderConfig, dht: Arc<DhtService>) -> Result<RunningProvider, String> {
    // Inference is disabled this version; `build` would still assemble it, so
    // refuse here (defense-in-depth alongside `ResourceClass::parse`).
    if !config.class.is_enabled() {
        return Err(format!(
            "resource class '{}' is disabled in this version",
            config.class.as_str()
        ));
    }

    let (offer, assembled) = provider_daemon::build(&config)?;

    let mut handles = Vec::new();
    let abort_all = |handles: &[AbortHandle]| {
        for h in handles {
            h.abort();
        }
    };

    let data_bind = match assembled {
        Assembled::Single(router) => {
            match spawn_serve(&config.control_bind, router).await {
                Ok(h) => handles.push(h),
                Err(e) => return Err(e),
            }
            None
        }
        Assembled::Storage { control, data } => {
            match spawn_serve(&config.control_bind, control).await {
                Ok(h) => handles.push(h),
                Err(e) => return Err(e),
            }
            match spawn_serve(&config.data_bind, data).await {
                Ok(h) => handles.push(h),
                Err(e) => {
                    abort_all(&handles);
                    return Err(e);
                }
            }
            Some(config.data_bind.clone())
        }
    };

    // Publish through the desktop's DHT; tear the servers down if it fails so we
    // don't leave an unadvertised provider bound.
    if let Err(e) = provider_daemon::publish_offer(&dht, &offer).await {
        abort_all(&handles);
        return Err(format!("offer publish failed: {e}"));
    }

    // Keep the offer fresh (records TTL out) until the provider is stopped.
    let dht_republish = dht.clone();
    let offer_republish = offer.clone();
    let republish = tokio::spawn(async move {
        loop {
            tokio::time::sleep(REPUBLISH_EVERY).await;
            if let Err(e) = provider_daemon::publish_offer(&dht_republish, &offer_republish).await {
                eprintln!("[provider] offer republish failed: {e}");
            }
        }
    });
    handles.push(republish.abort_handle());

    Ok(RunningProvider {
        handles,
        offer,
        control_bind: config.control_bind.clone(),
        data_bind,
    })
}
