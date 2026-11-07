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

static READONLY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Read-only mode (`REFLUX_READONLY=1`, or [`force_readonly`]): every non-GET request (but message
/// search and [`write_channel`] writes) and every gateway command but heartbeat/identify/resume/
/// member and count requests is refused locally.
/// For testing against real accounts without changing anything others can see.
pub fn readonly() -> bool {
    *READONLY.get_or_init(|| std::env::var_os("REFLUX_READONLY").is_some_and(|v| !v.is_empty() && v != "0"))
}

/// Turns read-only mode on for this process. Panics if it was already read as off.
pub fn force_readonly() {
    assert!(*READONLY.get_or_init(|| true), "read-only mode was already off");
}

/// `REFLUX_WRITE_CHANNEL=<id>`: in read-only mode, writes under `/channels/<id>/` (and the
/// upload PUTs of attachments planned there) are let through, for tests in one channel.
pub fn write_channel() -> Option<&'static str> {
    static ID: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    ID.get_or_init(|| std::env::var("REFLUX_WRITE_CHANNEL").ok().filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())))
        .as_deref()
}

/// `REFLUX_VOICE_CHANNEL=<id>`: in read-only mode, Voice State Updates (op 4) that join this
/// channel or leave voice are let through, for live voice tests in one channel.
pub fn voice_channel() -> Option<&'static str> {
    static ID: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    ID.get_or_init(|| std::env::var("REFLUX_VOICE_CHANNEL").ok().filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())))
        .as_deref()
}

/// Whether read-only mode lets this op 4 payload through: a leave (`channel_id` null), or the
/// voice channel.
pub(crate) fn allows_voice_state(d: &serde_json::Value, voice_channel: Option<&str>) -> bool {
    let Some(ch) = voice_channel else { return false };
    match d.get("channel_id") {
        None => false,
        Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(c)) => c == ch,
        Some(serde_json::Value::Number(n)) => n.to_string() == ch,
        _ => false,
    }
}

/// Whether read-only mode lets `method path` (an API path, query allowed) through, given the
/// write channel.
pub(crate) fn allows(method: &reqwest::Method, path: &str, write_channel: Option<&str>) -> bool {
    use reqwest::Method;
    let path = path.split('?').next().unwrap_or(path);
    // Message search is a POST but only reads (it may start indexing, which nobody sees).
    if *method == Method::GET || (*method == Method::POST && path == "/search/messages") {
        return true;
    }
    // No dot segments: the URL parser would resolve them out of the channel.
    let dots = path.contains("..") || path.to_ascii_lowercase().contains("%2e");
    !dots && write_channel.is_some_and(|c| path.strip_prefix("/channels/").and_then(|p| p.strip_prefix(c)).is_some_and(|p| p.starts_with('/')))
}

/// `User-Agent` for every HTTP and WebSocket request.
pub const USER_AGENT: &str = concat!("Reflux/", env!("CARGO_PKG_VERSION"));

/// A `reqwest` client with the crate's user agent, for [`discover`] and [`Client`].
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder().user_agent(USER_AGENT).build().expect("TLS backend")
}
