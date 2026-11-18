use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chiral_network::{
    chain_client::ChainClient, chain_rpc_api, network::NetworkConfig, provider_config, wallet,
};
use clap::Parser;
use serde_json::json;
use std::{
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

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
    /// Protected inbound bearer file, required on every bind address including loopback,
    /// because TLS ingress may forward public requests to a loopback port. At least 32
    /// visible ASCII characters, distinct from the configured upstream RPC bearer.
    #[arg(long)]
    bearer_file: PathBuf,
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn auth(State(token): State<Arc<String>>, request: Request, next: Next) -> Response {
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    let authorized = bool::from(supplied.as_bytes().ct_eq(token.as_bytes()));
    // A browser page must not use the gateway, even with a valid bearer.
    if !authorized || request.headers().contains_key(header::ORIGIN) {
        return no_store((StatusCode::UNAUTHORIZED, Json(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32003,"message":"RPC authentication required"}}))).into_response());
    }
    no_store(next.run(request).await)
}

fn inbound_bearer(path: &Path) -> Result<String, String> {
    // Owner-only regular file, no symbolic link, no whitespace or control characters.
    let token = provider_config::read_secret(path)?;
    // An HTTP Authorization header carries only visible ASCII.
    if token.len() < 32 || !token.is_ascii() {
        return Err("RPC gateway bearer must contain at least 32 visible ASCII characters".into());
    }
    Ok(token)
}

/// The upstream bearer as `rpc_client` sends it, read only to prove the two differ.
fn upstream_bearer(path: &Path) -> Result<Zeroizing<String>, String> {
    let mut text = Zeroizing::new(String::new());
    wallet::open_secret_file(path)?
        .take(4097)
        .read_to_string(&mut text)
        .map_err(|_| "cannot read the configured upstream RPC bearer")?;
    Ok(Zeroizing::new(text.trim().to_owned()))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.bind.port() == 0 {
        return Err("gateway requires an explicit nonzero port".into());
    }
    let token = inbound_bearer(&args.bearer_file)?;
    let mut config = NetworkConfig::load(&args.config)?;
    if let Some(path) = config.rpc_bearer_file.as_mut() {
        if path.is_relative() {
            *path = args
                .config
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(&*path);
        }
    }
    // A caller holding the upstream credential could bypass the method allowlist.
    if let Some(path) = config.rpc_bearer_file.as_deref() {
        if bool::from(upstream_bearer(path)?.as_bytes().ct_eq(token.as_bytes())) {
            return Err("the inbound RPC gateway bearer must differ from the upstream RPC bearer".into());
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
