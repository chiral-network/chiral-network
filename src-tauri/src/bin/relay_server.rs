//! Discovery/reputation bootstrap service. No inference, file, site, or wallet-key routes.
use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use chiral_network::{
    chain_client::ChainClient,
    dht::DhtService,
    error::ApiError,
    network::NetworkConfig,
    offer_registry::OfferRegistry,
    protocol::{InferenceOffer, Signed},
    provider_config::DhtConfig,
    rating_api::{self, RatingState},
    rating_storage::RatingStore,
    version,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

#[derive(Parser)]
#[command(about = "Chiral public-testnet discovery and payment-gated reputation relay")]
struct Args {
    #[arg(long)]
    config: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    network: NetworkConfig,
    data_dir: PathBuf,
    bind: SocketAddr,
    dht: DhtConfig,
    #[serde(default)]
    development: bool,
    version_policy_file: Option<PathBuf>,
}
struct RelayState {
    registry: Arc<OfferRegistry>,
    dht: DhtService,
    chain: Arc<ChainClient>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogQuery {
    model: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
}
#[derive(Serialize)]
struct CatalogPage {
    offers: Vec<Signed<InferenceOffer>>,
    next_offset: Option<usize>,
}
async fn catalog(
    State(state): State<Arc<RelayState>>,
    Query(query): Query<CatalogQuery>,
) -> Result<Json<CatalogPage>, ApiError> {
    if query
        .model
        .as_ref()
        .is_some_and(|m| m.is_empty() || m.len() > 256)
    {
        return Err(ApiError::bad_request(
            "invalid_model",
            "model selector must be 1..256 bytes",
        ));
    }
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(50);
    if offset > 8192 || limit == 0 || limit > 100 {
        return Err(ApiError::bad_request(
            "invalid_page",
            "limit must be 1..100; offset at most 8192",
        ));
    }
    let offers: Vec<_> = state
        .registry
        .list()
        .into_iter()
        .filter(|o| {
            query
                .model
                .as_ref()
                .map_or(true, |m| &o.payload.model.id == m)
        })
        .collect();
    let next_offset = (offers.len() > offset + limit).then_some(offset + limit);
    Ok(Json(CatalogPage {
        offers: offers.into_iter().skip(offset).take(limit).collect(),
        next_offset,
    }))
}
async fn health(State(state): State<Arc<RelayState>>) -> Json<serde_json::Value> {
    Json(
        serde_json::json!({"service":"chiral-discovery-relay","network":state.chain.identity(),"experimental_testnet":true,"dht":state.dht.status().await}),
    )
}
async fn ready(State(state): State<Arc<RelayState>>) -> Result<Json<serde_json::Value>, ApiError> {
    tokio::time::timeout(Duration::from_secs(10), state.chain.identity_check())
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "chain_timeout",
                "chain identity check timed out",
            )
        })?
        .map_err(|_| {
            ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "chain_unavailable",
                "configured chain is not ready",
            )
        })?;
    let dht = state.dht.status().await;
    if dht.get("running").and_then(serde_json::Value::as_bool) != Some(true) {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "dht_unavailable",
            "discovery is stopped",
        ));
    }
    Ok(Json(
        serde_json::json!({"ready":true,"network":state.chain.identity()}),
    ))
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
    let metadata = std::fs::metadata(&args.config)?;
    if metadata.len() > 1024 * 1024 {
        return Err("relay config exceeds 1 MiB".into());
    }
    let mut config: Config = serde_json::from_slice(&std::fs::read(&args.config)?)?;
    let base = args
        .config
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    for path in [&mut config.data_dir, &mut config.dht.identity_file] {
        if path.is_relative() {
            *path = base.join(&*path);
        }
    }
    for path in [
        &mut config.network.rpc_bearer_file,
        &mut config.version_policy_file,
    ]
    .into_iter()
    .flatten()
    {
        if path.is_relative() {
            *path = base.join(&*path);
        }
    }
    config.network.validate()?;
    if config.bind.port() == 0 || (config.development && !config.bind.ip().is_loopback()) {
        return Err("relay requires a nonzero bind; development must bind loopback".into());
    }
    version::log_policy_key_status();
    let data = config.network.scoped_dir(&config.data_dir);
    let policy_directory = data.join("version-policy");
    version::restore_persisted_policy(&policy_directory)?;
    if let Some(path) = config.version_policy_file {
        if std::fs::metadata(&path)?.len() > 65536 {
            return Err("version policy exceeds size limit".into());
        }
        let policy: version::VersionPolicy = serde_json::from_slice(&std::fs::read(path)?)?;
        if !version::verify_policy(&policy)
            || !version::update_persisted_policy(&policy_directory, policy)?
        {
            return Err("relay version policy signature or monotonicity rejected".into());
        }
    } else if !config.development {
        return Err(
            "public relays require version_policy_file with an offline-signed policy".into(),
        );
    }
    version::compare_to_policy(version::CURRENT_VERSION, &version::effective_policy())?;
    let chain = Arc::new(ChainClient::new(
        config.network.clone(),
        config.data_dir.clone(),
    )?);
    chain.identity_check().await?;
    let registry = Arc::new(OfferRegistry::new(
        config.network.identity.clone(),
        config.development,
    ));
    let dht = DhtService::start(
        config.dht,
        config.network.identity.clone(),
        registry.clone(),
    )
    .await?;
    let rating = Arc::new(RatingState::new(
        chain.clone(),
        Arc::new(RatingStore::open(
            &data.join("feedback.sqlite"),
            &config.network.identity,
        )?),
    ));
    let state = Arc::new(RelayState {
        registry,
        dht: dht.clone(),
        chain: chain.clone(),
    });
    let router = Router::new()
        .route("/api/offers", get(catalog))
        .route("/api/health", get(health))
        .route("/api/ready", get(ready))
        .route(
            "/api/version-policy",
            get(|| async { Json(version::effective_policy()) }),
        )
        .with_state(state)
        .merge(rating_api::router(rating.clone()))
        .merge(chiral_network::chain_rpc_api::router(chain));
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let reconcile = tokio::spawn(rating_api::reconcile(rating, stop_rx.clone()));
    let discovery = dht.clone();
    let mut shutdown = stop_rx;
    let refresh = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            tokio::select! {
                _=shutdown.changed()=>{if *shutdown.borrow(){break;}},
                _=interval.tick()=>{if let Err(error)=discovery.discover(None).await{eprintln!("discovery refresh failed: {error}");}}
            }
        }
    });
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    println!(
        "Chiral testnet relay listening on {}; peer_id={}",
        config.bind,
        dht.peer_id()
    );
    let result = axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    let _ = stop_tx.send(true);
    dht.stop().await;
    let _ = refresh.await;
    let _ = reconcile.await;
    result?;
    Ok(())
}
