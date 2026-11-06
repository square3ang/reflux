//! HTTP API client (`http-api/index.md`, `topics/rate-limits.md`, `topics/captcha.md`).

use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use reflux_model::{
    ApiError, AuthResponse, AuthToken, Channel, CreateMessage, InstanceDiscovery, IpAuthorizationPoll, LoginRequest, Message,
    MfaTicket, ReactionEmoji, RegisterRequest, Relationship, Snowflake, TotpLoginRequest, User, UserGuildSettings, UserSettings,
    WebAuthnAuthenticationOptions, WebAuthnLoginRequest,
};
use reqwest::{Method, StatusCode, header};
use serde::{Serialize, de::DeserializeOwned};

use crate::{Error, Result, altcha};

/// Longest rate-limit wait done silently; longer ones (login limits are minutes) fail with
/// `RATE_LIMITED` and `retry_after` so the UI can tell the user.
const MAX_SILENT_WAIT: Duration = Duration::from_secs(10);
const MAX_429_RETRIES: u32 = 3;
/// Like the reference client: a second challenge after a solved one is solved once more.
const MAX_CAPTCHA_SOLVES: u32 = 2;
/// Sent by the official client on same-origin requests; the server gates threads on them.
const FEATURES: &str = "view_channel_members_permission,channel_threads";
const CAPABILITIES: &str = "guild_visits";

/// Which page of `GET /channels/{id}/messages`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessagePage {
    Latest,
    Before(Snowflake),
    After(Snowflake),
    Around(Snowflake),
}

pub struct Client {
    http: reqwest::Client,
    /// `{api}/v1`.
    base: String,
    token: RwLock<Option<String>>,
    /// `X-Fluxer-Client-Installation-ID`: 32 hex chars, stable per install.
    installation_id: Option<String>,
    limits: Mutex<RateLimits>,
}

impl Client {
    /// `api` is the discovered API base URL (without `/v1`).
    pub fn new(http: reqwest::Client, api: &str) -> Self {
        Self {
            http,
            base: format!("{}/v1", api.trim_end_matches('/')),
            token: RwLock::new(None),
            installation_id: None,
            limits: Mutex::default(),
        }
    }

    pub fn with_installation_id(mut self, id: String) -> Self {
        self.installation_id = Some(id);
        self
    }

    /// Picks `api_transport`, then `api_public`, then `api_client`, then `api`. fluxer.app's
    /// `api_client` (web.fluxer.app) refuses non-GET requests without a browser `Origin`
    /// (`INVALID_API_ORIGIN`); its desktop app and other clients go through fluxer.com/api.
    pub fn from_discovery(http: reqwest::Client, d: &InstanceDiscovery) -> Self {
        let e = &d.endpoints;
        let api = [e.api_transport.as_deref().unwrap_or(""), &e.api_public, &e.api_client, &e.api]
            .into_iter()
            .find(|u| !u.is_empty())
            .unwrap_or_default();
        Self::new(http, api)
    }

    /// Sets the user session token, sent raw in `Authorization`.
    pub fn set_token(&self, token: Option<String>) {
        *self.token.write().unwrap() = token;
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.request(Method::GET, path, None::<&()>).await
    }

    /// Sends `method {base}/v1{path}` with an optional JSON body. Waits out known rate limits,
    /// retries 429s and solves ALTCHA challenges. An empty response body parses as `null`.
    pub async fn request<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T> {
        let body = body.map(serde_json::to_vec).transpose()?;
        let key = format!("{method} {}", path.split('?').next().unwrap_or(path));
        let (mut captcha, mut solves, mut retries) = (None::<String>, 0, 0);
        loop {
            let wait = self.limits.lock().unwrap().wait(&key, Instant::now());
            if let Some(wait) = wait {
                if wait > MAX_SILENT_WAIT {
                    return Err(rate_limited(wait));
                }
                tokio::time::sleep(wait).await;
            }

            let mut req = self
                .http
                .request(method.clone(), format!("{}{path}", self.base))
                .header("X-Fluxer-Features", FEATURES)
                .header("X-Fluxer-Capabilities", CAPABILITIES);
            if let Some(token) = self.token.read().unwrap().as_deref() {
                req = req.header(header::AUTHORIZATION, token);
            }
            if let Some(id) = &self.installation_id {
                req = req.header("X-Fluxer-Client-Installation-ID", id);
            }
            if let Some(token) = &captcha {
                req = req.header("X-Captcha-Token", token);
            }
            if let Some(body) = &body {
                req = req.header(header::CONTENT_TYPE, "application/json").body(body.clone());
            }
            let res = req.send().await?;
            let status = res.status();
            let h = res.headers();
            let num = |name: &str| h.get(name).and_then(|v| v.to_str().ok()?.parse::<f64>().ok());
            let (remaining, reset_after, retry_header) =
                (num("x-ratelimit-remaining"), num("x-ratelimit-reset-after"), num("retry-after"));
            let global_header = h.get("x-ratelimit-global").is_some_and(|v| v == "true");
            let bytes = res.bytes().await?;

            if status.is_success() {
                // Route headers come only for bot tokens: pause before the bucket denies.
                if let (Some(0.0), Some(reset)) = (remaining, reset_after) {
                    self.limits.lock().unwrap().record(&key, false, secs(reset), Instant::now());
                }
                return Ok(serde_json::from_slice(if bytes.is_empty() { b"null" } else { &bytes })?);
            }

            let error = serde_json::from_slice::<ApiError>(&bytes).unwrap_or_else(|_| ApiError {
                code: status.as_str().to_owned(),
                message: String::from_utf8_lossy(&bytes).into_owned(),
                ..Default::default()
            });
            if status == StatusCode::TOO_MANY_REQUESTS {
                let global = global_header || error.extra.get("global").and_then(|v| v.as_bool()) == Some(true);
                // The body value is exact; the header is rounded up to whole seconds.
                let after = secs(error.retry_after.or(retry_header).unwrap_or(1.0));
                self.limits.lock().unwrap().record(&key, global, after, Instant::now());
                if retries < MAX_429_RETRIES && after <= MAX_SILENT_WAIT {
                    retries += 1;
                    continue;
                }
            } else if status == StatusCode::BAD_REQUEST
                && matches!(error.code.as_str(), "CAPTCHA_REQUIRED" | "INVALID_CAPTCHA")
                && error.captcha_provider.as_deref() == Some("altcha")
                && solves < MAX_CAPTCHA_SOLVES
                && let Some(challenge) = error.altcha_challenge.clone()
            {
                solves += 1;
                let token = tokio::task::spawn_blocking(move || altcha::token(&challenge))
                    .await
                    .map_err(|e| Error::Captcha(e.to_string()))??;
                captcha = Some(token);
                continue;
            }
            return Err(Error::Api { status, error: Box::new(error) });
        }
    }

    // --- Authentication (`http-api/authentication.mdx`) ---

    pub async fn login(&self, req: &LoginRequest) -> Result<AuthResponse> {
        self.request(Method::POST, "/auth/login", Some(req)).await
    }

    /// TOTP or backup code for an MFA ticket.
    pub async fn login_totp(&self, req: &TotpLoginRequest) -> Result<AuthToken> {
        self.request(Method::POST, "/auth/login/mfa/totp", Some(req)).await
    }

    pub async fn login_webauthn_options(&self, ticket: &str) -> Result<WebAuthnAuthenticationOptions> {
        let body = MfaTicket { ticket: ticket.to_owned() };
        self.request(Method::POST, "/auth/login/mfa/webauthn/authentication-options", Some(&body)).await
    }

    pub async fn login_webauthn(&self, req: &WebAuthnLoginRequest) -> Result<AuthToken> {
        self.request(Method::POST, "/auth/login/mfa/webauthn", Some(req)).await
    }

    /// State of a sign-in waiting on `IP_AUTHORIZATION_REQUIRED` (poll every 2 s, like the app).
    pub async fn poll_ip_authorization(&self, ticket: &str) -> Result<IpAuthorizationPoll> {
        let ticket: String = url::form_urlencoded::byte_serialize(ticket.as_bytes()).collect();
        self.get(&format!("/auth/ip-authorization/poll?ticket={ticket}")).await
    }

    pub async fn register(&self, req: &RegisterRequest) -> Result<AuthResponse> {
        self.request(Method::POST, "/auth/register", Some(req)).await
    }

    /// Revokes the current session token server-side and forgets it locally.
    pub async fn logout(&self) -> Result<()> {
        self.request::<(), ()>(Method::POST, "/auth/logout", None).await?;
        self.set_token(None);
        Ok(())
    }

    // --- Users and messages ---

    pub async fn current_user(&self) -> Result<User> {
        self.get("/users/@me").await
    }

    /// `PATCH /users/@me/settings` with a partial settings object; returns the full settings.
    pub async fn update_settings(&self, patch: &serde_json::Value) -> Result<UserSettings> {
        self.request(Method::PATCH, "/users/@me/settings", Some(patch)).await
    }

    /// Newest first; `limit` is 1-100.
    pub async fn messages(&self, channel: Snowflake, page: MessagePage, limit: u8) -> Result<Vec<Message>> {
        let anchor = match page {
            MessagePage::Latest => String::new(),
            MessagePage::Before(id) => format!("&before={id}"),
            MessagePage::After(id) => format!("&after={id}"),
            MessagePage::Around(id) => format!("&around={id}"),
        };
        self.get(&format!("/channels/{channel}/messages?limit={limit}{anchor}")).await
    }

    /// JSON body only; reuse the `nonce` when retrying a send.
    pub async fn send_message(&self, channel: Snowflake, msg: &CreateMessage) -> Result<Message> {
        self.request(Method::POST, &format!("/channels/{channel}/messages"), Some(msg)).await
    }

    // --- Message actions (`http-api/messages.mdx`) ---

    /// Changes only the text; other fields stay.
    pub async fn edit_message(&self, channel: Snowflake, id: Snowflake, content: &str) -> Result<Message> {
        let body = serde_json::json!({ "content": content });
        self.request(Method::PATCH, &format!("/channels/{channel}/messages/{id}"), Some(&body)).await
    }

    pub async fn delete_message(&self, channel: Snowflake, id: Snowflake) -> Result<()> {
        self.empty(Method::DELETE, &format!("/channels/{channel}/messages/{id}"), None).await
    }

    pub async fn set_pinned(&self, channel: Snowflake, id: Snowflake, pinned: bool) -> Result<()> {
        let m = if pinned { Method::PUT } else { Method::DELETE };
        self.empty(m, &format!("/channels/{channel}/pins/{id}"), None).await
    }

    /// Adds or removes the current user's reaction.
    pub async fn set_reaction(&self, channel: Snowflake, id: Snowflake, emoji: &ReactionEmoji, add: bool) -> Result<()> {
        let m = if add { Method::PUT } else { Method::DELETE };
        let e = reaction_path(emoji);
        self.empty(m, &format!("/channels/{channel}/messages/{id}/reactions/{e}/@me"), None).await
    }

    /// 204, or 200 with slowmode details we don't use.
    pub async fn typing(&self, channel: Snowflake) -> Result<()> {
        self.empty(Method::POST, &format!("/channels/{channel}/typing"), None).await
    }

    /// Marks the channel read up to `id`; `manual` moves the marker back (mark as unread).
    pub async fn ack_message(&self, channel: Snowflake, id: Snowflake, manual: bool) -> Result<()> {
        let body = serde_json::json!({ "mention_count": 0, "manual": manual });
        self.empty(Method::POST, &format!("/channels/{channel}/messages/{id}/ack"), Some(&body)).await
    }

    /// `POST /read-states/ack-bulk`: marks each channel read through its message (≤ 100 entries).
    pub async fn ack_bulk(&self, acks: &[(Snowflake, Snowflake)]) -> Result<()> {
        let read_states: Vec<_> = acks.iter().map(|(c, m)| serde_json::json!({ "channel_id": c, "message_id": m })).collect();
        self.empty(Method::POST, "/read-states/ack-bulk", Some(&serde_json::json!({ "read_states": read_states }))).await
    }

    // --- Relationships, private channels, notes, guild membership (`http-api/users/*`) ---

    /// Friend request by tag; the official client sends `0000` for a bare username.
    pub async fn send_friend_request(&self, username: &str, discriminator: &str) -> Result<Relationship> {
        let body = serde_json::json!({ "username": username, "discriminator": discriminator });
        self.request(Method::POST, "/users/@me/relationships", Some(&body)).await
    }

    /// Sends a friend request to the user, or accepts theirs.
    pub async fn add_friend(&self, user: Snowflake) -> Result<Relationship> {
        self.request::<_, ()>(Method::POST, &format!("/users/@me/relationships/{user}"), None).await
    }

    pub async fn block(&self, user: Snowflake) -> Result<Relationship> {
        let body = serde_json::json!({ "type": 2 });
        self.request(Method::PUT, &format!("/users/@me/relationships/{user}"), Some(&body)).await
    }

    /// Removes a friend, cancels or ignores a request, or unblocks.
    pub async fn remove_relationship(&self, user: Snowflake) -> Result<()> {
        self.empty(Method::DELETE, &format!("/users/@me/relationships/{user}"), None).await
    }

    /// Opens (or returns) the DM with a user.
    pub async fn open_dm(&self, user: Snowflake) -> Result<Channel> {
        let body = serde_json::json!({ "recipient_id": user });
        self.request(Method::POST, "/users/@me/channels", Some(&body)).await
    }

    /// Closes a DM or leaves a group DM.
    pub async fn close_channel(&self, channel: Snowflake) -> Result<()> {
        self.empty(Method::DELETE, &format!("/channels/{channel}"), None).await
    }

    /// An empty note deletes it.
    pub async fn set_note(&self, user: Snowflake, note: &str) -> Result<()> {
        let body = serde_json::json!({ "note": note });
        self.empty(Method::PUT, &format!("/users/@me/notes/{user}"), Some(&body)).await
    }

    pub async fn leave_guild(&self, guild: Snowflake) -> Result<()> {
        self.empty(Method::DELETE, &format!("/users/@me/guilds/{guild}"), None).await
    }

    /// `PATCH /users/@me/guilds/{guild or @me}/settings` with update fields.
    pub async fn update_guild_settings(&self, guild: Option<Snowflake>, patch: &serde_json::Value) -> Result<UserGuildSettings> {
        let g = guild.map_or_else(|| "@me".to_string(), |g| g.to_string());
        self.request(Method::PATCH, &format!("/users/@me/guilds/{g}/settings"), Some(patch)).await
    }

    /// A request whose response body (if any) is ignored.
    async fn empty(&self, method: Method, path: &str, body: Option<&serde_json::Value>) -> Result<()> {
        self.request::<serde_json::Value, _>(method, path, body).await.map(drop)
    }
}

/// The `{emoji}` path value: `name:id` for a custom emoji, else the URL-encoded character.
fn reaction_path(e: &ReactionEmoji) -> String {
    let raw = match e.id {
        Some(id) => format!("{}:{id}", e.name),
        None => e.name.clone(),
    };
    url::form_urlencoded::byte_serialize(raw.as_bytes()).collect()
}

fn secs(s: f64) -> Duration {
    Duration::try_from_secs_f64(s).unwrap_or(Duration::from_secs(1))
}

fn rate_limited(wait: Duration) -> Error {
    Error::Api {
        status: StatusCode::TOO_MANY_REQUESTS,
        error: Box::new(ApiError {
            code: "RATE_LIMITED".into(),
            message: "You're being rate limited.".into(),
            retry_after: Some(wait.as_secs_f64()),
            ..Default::default()
        }),
    }
}

/// Pauses learned from 429s (and bot route headers): one per `METHOD path`, like the reference
/// client, plus the global bucket, which pauses every request.
#[derive(Default)]
struct RateLimits {
    global: Option<Instant>,
    routes: HashMap<String, Instant>,
}

impl RateLimits {
    /// How long to wait before sending on `key`, if at all. Drops expired entries.
    fn wait(&mut self, key: &str, now: Instant) -> Option<Duration> {
        self.routes.retain(|_, until| *until > now);
        self.global = self.global.filter(|until| *until > now);
        let until = self.global.into_iter().chain(self.routes.get(key).copied()).max()?;
        Some(until - now)
    }

    fn record(&mut self, key: &str, global: bool, after: Duration, now: Instant) {
        // A shorter record never shortens a known pause.
        let until = now + after;
        if global {
            self.global = self.global.max(Some(until));
        } else {
            let e = self.routes.entry(key.to_owned()).or_insert(until);
            *e = (*e).max(until);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaction_paths() {
        let thumbs = ReactionEmoji { name: "👍".into(), ..Default::default() };
        assert_eq!(reaction_path(&thumbs), "%F0%9F%91%8D");
        let custom = ReactionEmoji { name: "fluxer".into(), id: Some(Snowflake(77)), ..Default::default() };
        assert_eq!(reaction_path(&custom), "fluxer%3A77");
    }

    #[test]
    fn route_and_global_buckets() {
        let mut rl = RateLimits::default();
        let t = Instant::now();
        let ms = Duration::from_millis;
        assert_eq!(rl.wait("GET /a", t), None);

        rl.record("GET /a", false, ms(428), t);
        assert_eq!(rl.wait("GET /a", t), Some(ms(428)));
        assert_eq!(rl.wait("POST /a", t), None, "buckets are per method and path");
        assert_eq!(rl.wait("GET /a", t + ms(100)), Some(ms(328)));
        rl.record("GET /a", false, ms(10), t);
        assert_eq!(rl.wait("GET /a", t + ms(100)), Some(ms(328)), "shorter record keeps the pause");

        rl.record("GET /b", true, ms(1000), t);
        assert_eq!(rl.wait("POST /c", t), Some(ms(1000)), "global pauses every route");
        assert_eq!(rl.wait("GET /a", t + ms(500)), Some(ms(500)));

        assert_eq!(rl.wait("GET /a", t + ms(1000)), None);
        assert!(rl.routes.is_empty() && rl.global.is_none(), "expired entries are dropped");
    }
}
