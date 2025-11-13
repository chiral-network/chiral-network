#[derive(Debug, thiserror::Error)]
pub enum ChiralError {
    #[error("Protocol error: {0}")]
    Protocol(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(String),
}