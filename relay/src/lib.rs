pub mod relay_auth;
pub mod http_server {
    use axum::{Router, routing::get};
    use std::net::SocketAddr;
    use tokio::net::TcpListener;
    
    pub async fn create_listener() -> Result<(TcpListener, u16), std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        Ok((listener, port))
    }
    
    pub fn create_router() -> Router {
        Router::new()
            .route("/health", get(|| async { "OK" }))
            .route("/metrics", get(|| async { 
                serde_json::json!({
                    "status": "running"
                }).to_string()
            }))
    }
}