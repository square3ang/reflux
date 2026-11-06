//! Networking for Fluxer: instance discovery, the HTTP API client (rate limits, ALTCHA) and the
//! Gateway connection.

pub mod altcha;
pub mod discovery;
pub mod gateway;
pub mod rest;
mod zstd_stream;

pub use discovery::discover;
pub use gateway::{Command, ConnectionState, Gateway, GatewayConfig, GatewayEvent};
pub use rest::{ChannelPin, Client, MessagePage, PinsPage, SearchResponse, UploadPlan};

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{status}: {} ({})", .error.message, .error.code)]
    Api { status: reqwest::StatusCode, error: Box<reflux_model::ApiError> },
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("captcha: {0}")]
    Captcha(String),
    #[error("instance discovery: {0}")]
    Discovery(String),
    #[error("invalid url: {0}")]
    Url(String),
    #[error("{0}")]
    Upload(String),
}

impl Error {
    /// The API error code, if the server returned one.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Api { error, .. } => Some(&error.code),
            _ => None,
        }
    }
}

/// `User-Agent` for every HTTP and WebSocket request.
pub const USER_AGENT: &str = concat!("Reflux/", env!("CARGO_PKG_VERSION"));

/// A `reqwest` client with the crate's user agent, for [`discover`] and [`Client`].
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder().user_agent(USER_AGENT).build().expect("TLS backend")
}
