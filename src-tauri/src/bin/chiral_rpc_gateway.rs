use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chiral_network::{
    chain_client::ChainClient, chain_rpc_api, network::NetworkConfig, provider_config,
};
use clap::Parser;
use serde_json::json;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use subtle::ConstantTimeEq;

#[derive(Parser)]
#[command(
    version,
    about = "Bounded Chiral settlement RPC gateway; operator methods are never exposed"
)]
struct Args {
    #[arg(long)]
    config: PathBuf,
    #[arg(long, default_value = "127.0.0.1:8547")]
    bind: SocketAddr,
    #[arg(long)]
    journal_dir: PathBuf,
    /// Inbound bearer, separate from the configured upstream RPC bearer.
    #[arg(long)]
    bearer_file: Option<PathBuf>,
}

async fn auth(State(token): State<Arc<Option<String>>>, request: Request, next: Next) -> Response {
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    let authorized = token.as_ref().as_ref().map_or(true, |expected| {
        bool::from(supplied.as_bytes().ct_eq(expected.as_bytes()))
    });
    if !authorized || request.headers().contains_key(header::ORIGIN) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32003,"message":"RPC authentication required"}}))).into_response();
    }
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.bind.port() == 0 {
        return Err("gateway requires an explicit nonzero port".into());
    }
    if !args.bind.ip().is_loopback() && args.bearer_file.is_none() {
        return Err(
            "non-loopback RPC gateways require an inbound bearer file and TLS ingress".into(),
        );
    }
    let token = args
        .bearer_file
        .as_deref()
        .map(provider_config::read_secret)
        .transpose()?;
    if token.as_ref().is_some_and(|value| value.len() < 32) {
        return Err("RPC bearer must contain at least 32 random characters".into());
    }
    let mut config = NetworkConfig::load(&args.config)?;
    if let Some(path) = config.rpc_bearer_file.as_mut() {
        if path.is_relative() {
            *path = args
                .config
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join(&*path);
        }
    }
    let chain = Arc::new(ChainClient::new(config, args.journal_dir)?);
    chain.identity_check().await?;
    let ready_chain = chain.clone();
    let public =
        chain_rpc_api::router(chain).layer(middleware::from_fn_with_state(Arc::new(token), auth));
    let router = public.merge(
        Router::new()
            .route("/healthz", get(|| async { Json(json!({"status":"live"})) }))
            .route(
                "/readyz",
                get(move || {
                    let chain = ready_chain.clone();
                    async move {
                        let ready = chain.identity_check().await.is_ok();
                        (
                            if ready {
                                StatusCode::OK
                            } else {
                                StatusCode::SERVICE_UNAVAILABLE
                            },
                            Json(json!({"ready":ready})),
                        )
                    }
                }),
            ),
    );
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    println!("Chiral bounded RPC gateway listening on {}", args.bind);
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("install SIGTERM handler");
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
        })
        .await?;
    Ok(())
}
