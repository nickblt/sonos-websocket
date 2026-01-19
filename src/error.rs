use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),

    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid server name: {0}")]
    InvalidServerName(#[from] rustls::pki_types::InvalidDnsNameError),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Player not found: {0}")]
    PlayerNotFound(String),

    #[error("Player not connected: {0}")]
    NotConnected(String),

    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
}

pub type Result<T> = std::result::Result<T, Error>;
