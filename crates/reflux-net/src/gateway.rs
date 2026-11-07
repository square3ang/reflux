//! Gateway connection (`gateway/overview.md`, `opcodes-and-close-codes.md`). Timings follow the
//! reference client (`GatewaySocket.ts`).

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use reflux_model::{
    CloseCode, DispatchEvent, GatewayError, GatewayPayload, Hello, Identify, IdentifyProperties, InitialPresence,
    OpCode, Resume, SessionFlags,
};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep, sleep_until};
use tokio_tungstenite::tungstenite::{
    self, ClientRequestBuilder,
    protocol::{CloseFrame, frame::coding::CloseCode as WsCloseCode},
};
use tracing::{debug, info, warn};
use url::Url;

use crate::zstd_stream::ZstdStream;

const HELLO_TIMEOUT: Duration = Duration::from_secs(20);
const HEARTBEAT_ACK_TIMEOUT: Duration = Duration::from_secs(15);
/// RESUMED must arrive within this long.
const RESUME_TIMEOUT: Duration = Duration::from_secs(15);
/// A session can be resumed this long after its socket closed. The Gateway keeps it for 60 s;
/// `GatewaySocket` tries for 180 s, and a late Resume is answered with opcode 9 (Identify).
const RESUME_WINDOW: Duration = Duration::from_secs(180);
const MIN_RECONNECT: Duration = Duration::from_secs(1);
const MAX_RECONNECT: Duration = Duration::from_secs(10);
const RECONNECT_SPREAD_MS: u64 = 2000;
const INVALID_SESSION_IDENTIFY_SPREAD_MS: u64 = 1000;

pub struct GatewayConfig {
    /// `endpoints.gateway` from discovery; its query is kept.
    pub url: String,
    /// Raw user token, no prefix.
    pub token: String,
    pub properties: IdentifyProperties,
    pub flags: SessionFlags,
    pub presence: Option<InitialPresence>,
    /// `compress=zstd-stream&stream=1`.
    pub compress: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConnectionState {
    Connecting,
    /// After Ready or Resumed.
    Connected,
    Reconnecting(Duration),
    /// Final: closed by [`Command::Close`] or by a close code that forbids reconnecting.
    Disconnected(Option<CloseCode>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum GatewayEvent {
    State(ConnectionState),
    Dispatch(DispatchEvent),
    /// Opcode 12, a refused voice join.
    Error(GatewayError),
    /// A dispatch whose payload didn't parse (skipped); `error` has the JSON path.
    Malformed { t: String, d: Value, error: String },
}

#[derive(Clone, Debug)]
pub enum Command {
    /// Any client opcode (presence, voice state, lazy request, ...). Dropped while disconnected.
    Send(GatewayPayload),
    Close,
    /// `GatewaySocket.reset()`: drop the session and connect again with a fresh IDENTIFY.
    Reset,
    /// `simulateNetworkDisconnect()`: close 4000 and reconnect (RESUME) as after a network drop.
    Disconnect,
}

/// A running Gateway task. Dropping `commands` closes it.
pub struct Gateway {
    pub commands: mpsc::UnboundedSender<Command>,
    pub events: mpsc::UnboundedReceiver<GatewayEvent>,
}

impl Gateway {
    /// Spawns the connection task on the current Tokio runtime.
    pub fn connect(config: GatewayConfig) -> crate::Result<Self> {
        gateway_url(&config.url, config.compress)?;
        let (commands, cmd_rx) = mpsc::unbounded_channel();
        let (ev_tx, events) = mpsc::unbounded_channel();
        let session = Session { config, id: None, seq: None, closed_at: None, attempts: 0, events: ev_tx };
        tokio::spawn(session.run(cmd_rx));
        Ok(Self { commands, events })
    }
}

/// Adds `v=1&encoding=json` (+ `compress=zstd-stream&stream=1`), keeping any other query.
pub fn gateway_url(base: &str, compress: bool) -> crate::Result<String> {
    let mut url = Url::parse(base).map_err(|e| crate::Error::Url(format!("{base}: {e}")))?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err(crate::Error::Url(format!("{base}: not a ws(s) URL")));
    }
    let ours = ["v", "encoding", "compress", "stream"];
    let kept: Vec<(String, String)> =
        url.query_pairs().into_owned().filter(|(k, _)| !ours.contains(&k.as_str())).collect();
    let mut q = url.query_pairs_mut();
    q.clear().extend_pairs(kept).append_pair("v", "1").append_pair("encoding", "json");
    if compress {
        q.append_pair("compress", "zstd-stream").append_pair("stream", "1");
    }
    drop(q);
    Ok(url.into())
}

enum Exit {
    Shutdown,
    Fatal(CloseCode),
    Reconnect { immediate: bool },
}

struct Session {
    config: GatewayConfig,
    id: Option<String>,
    /// Last Dispatch sequence received.
    seq: Option<u64>,
    closed_at: Option<Instant>,
    attempts: u32,
    events: mpsc::UnboundedSender<GatewayEvent>,
}

impl Session {
    fn emit(&self, e: GatewayEvent) {
        let _ = self.events.send(e);
    }

    fn clear(&mut self) {
        self.id = None;
        self.seq = None;
    }

    async fn run(mut self, mut cmds: mpsc::UnboundedReceiver<Command>) {
        loop {
            if self.closed_at.is_some_and(|t| t.elapsed() > RESUME_WINDOW) {
                self.clear();
            }
            self.emit(GatewayEvent::State(ConnectionState::Connecting));
            let exit = self.connection(&mut cmds).await;
            self.closed_at = Some(Instant::now());
            let delay = match exit {
                Exit::Shutdown => return self.emit(GatewayEvent::State(ConnectionState::Disconnected(None))),
                Exit::Fatal(code) => return self.emit(GatewayEvent::State(ConnectionState::Disconnected(Some(code)))),
                Exit::Reconnect { immediate: true } => Duration::from_millis(fastrand::u64(..RECONNECT_SPREAD_MS)),
                Exit::Reconnect { immediate: false } => {
                    self.attempts += 1;
                    backoff(self.attempts)
                }
            };
            info!(?delay, "gateway reconnecting");
            self.emit(GatewayEvent::State(ConnectionState::Reconnecting(delay)));
            let wake = sleep(delay);
            tokio::pin!(wake);
            loop {
                tokio::select! {
                    _ = &mut wake => break,
                    cmd = cmds.recv() => match cmd {
                        Some(Command::Send(p)) => debug!(op = p.op.0, "gateway offline, dropping command"),
                        Some(Command::Reset) => {
                            self.clear();
                            break;
                        }
                        Some(Command::Disconnect) => debug!("gateway offline, nothing to disconnect"),
                        Some(Command::Close) | None => {
                            return self.emit(GatewayEvent::State(ConnectionState::Disconnected(None)));
                        }
                    },
                }
            }
        }
    }

    async fn connection(&mut self, cmds: &mut mpsc::UnboundedReceiver<Command>) -> Exit {
        // Rebuilt per socket: a compression decode failure turns compression off for good.
        let url = match gateway_url(&self.config.url, self.config.compress) {
            Ok(url) => url,
            Err(e) => {
                warn!(%e, "invalid gateway url");
                return Exit::Fatal(CloseCode::DECODE_ERROR);
            }
        };
        let request = match url.parse() {
            Ok(uri) => ClientRequestBuilder::new(uri).with_header("User-Agent", crate::USER_AGENT),
            Err(e) => {
                warn!(%e, "invalid gateway url");
                return Exit::Fatal(CloseCode::DECODE_ERROR);
            }
        };
        let ws = match tokio::time::timeout(HELLO_TIMEOUT, tokio_tungstenite::connect_async(request)).await {
            Ok(Ok((ws, _))) => ws,
            Ok(Err(e)) => {
                warn!(%e, "gateway connect failed");
                return Exit::Reconnect { immediate: false };
            }
            Err(_) => {
                warn!("gateway connect timed out");
                return Exit::Reconnect { immediate: false };
            }
        };
        let (sink, mut stream) = ws.split();
        let zstd = match self.config.compress.then(ZstdStream::new).transpose() {
            Ok(z) => z,
            Err(e) => {
                warn!(%e, "zstd init failed");
                return Exit::Fatal(CloseCode::DECODE_ERROR);
            }
        };
        let mut conn = Conn {
            s: self,
            sink,
            zstd,
            interval: None,
            next_heartbeat: None,
            ack_deadline: None,
            identify_at: None,
            hello_deadline: Some(Instant::now() + HELLO_TIMEOUT),
            resume_deadline: None,
            resuming: false,
        };
        // A disabled `select!` branch still evaluates its future; this is never awaited.
        let never = || Instant::now() + Duration::from_secs(86_400);
        loop {
            let exit = tokio::select! {
                msg = stream.next() => match msg {
                    Some(Ok(tungstenite::Message::Text(t))) => conn.on_json(t.as_bytes()).await,
                    Some(Ok(tungstenite::Message::Binary(b))) => conn.on_binary(&b).await,
                    Some(Ok(tungstenite::Message::Close(frame))) => Some(conn.on_close(frame)),
                    Some(Ok(_)) => None,
                    Some(Err(e)) => {
                        warn!(%e, "gateway socket error");
                        Some(Exit::Reconnect { immediate: false })
                    }
                    None => Some(conn.on_close(None)),
                },
                _ = sleep_until(conn.next_heartbeat.unwrap_or_else(never)), if conn.next_heartbeat.is_some() => {
                    conn.next_heartbeat = conn.interval.map(|i| Instant::now() + heartbeat_delay(i));
                    conn.heartbeat().await
                }
                _ = sleep_until(conn.ack_deadline.unwrap_or_else(never)), if conn.ack_deadline.is_some() => {
                    warn!("heartbeat ACK timeout");
                    conn.close(4000, "Heartbeat ACK timeout").await;
                    Some(Exit::Reconnect { immediate: true })
                }
                _ = sleep_until(conn.hello_deadline.unwrap_or_else(never)), if conn.hello_deadline.is_some() => {
                    warn!("no HELLO from gateway");
                    conn.close(4000, "Hello timeout").await;
                    Some(Exit::Reconnect { immediate: false })
                }
                _ = sleep_until(conn.resume_deadline.unwrap_or_else(never)), if conn.resume_deadline.is_some() => {
                    warn!("gateway did not answer RESUME");
                    conn.s.clear();
                    conn.close(4000, "Resume timeout").await;
                    Some(Exit::Reconnect { immediate: false })
                }
                _ = sleep_until(conn.identify_at.unwrap_or_else(never)), if conn.identify_at.is_some() => {
                    conn.identify_at = None;
                    conn.identify().await
                }
                cmd = cmds.recv() => match cmd {
                    Some(Command::Send(p))
                        if crate::readonly()
                            && !read_only_op(p.op)
                            && !(p.op == OpCode::VOICE_STATE_UPDATE && crate::allows_voice_state(&p.d, crate::voice_channel())) =>
                    {
                        warn!(op = p.op.0, "read-only mode: gateway command dropped");
                        None
                    }
                    Some(Command::Send(p)) => conn.send(&p).await,
                    Some(Command::Reset) => {
                        info!("gateway reset");
                        conn.close(1000, "Reset").await;
                        conn.s.clear();
                        Some(Exit::Reconnect { immediate: true })
                    }
                    Some(Command::Disconnect) => {
                        info!("simulating network disconnect with resumable close");
                        conn.close(4000, "Simulated network disconnect").await;
                        Some(Exit::Reconnect { immediate: false })
                    }
                    Some(Command::Close) | None => {
                        conn.close(1000, "").await;
                        Some(Exit::Shutdown)
                    }
                },
            };
            if let Some(exit) = exit {
                return exit;
            }
        }
    }
}

type Sink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tungstenite::Message,
>;

/// One socket of a session. Handlers return `Some(exit)` to end the socket.
struct Conn<'a> {
    s: &'a mut Session,
    sink: Sink,
    zstd: Option<ZstdStream>,
    interval: Option<Duration>,
    next_heartbeat: Option<Instant>,
    ack_deadline: Option<Instant>,
    identify_at: Option<Instant>,
    hello_deadline: Option<Instant>,
    /// Set while a RESUME waits for READY/RESUMED/INVALID_SESSION.
    resume_deadline: Option<Instant>,
    resuming: bool,
}

impl Conn<'_> {
    async fn send(&mut self, p: &GatewayPayload) -> Option<Exit> {
        let json = serde_json::to_vec(p).expect("payload serializes");
        let msg = match &mut self.zstd {
            Some(z) => match z.compress(&json) {
                Ok(b) => tungstenite::Message::Binary(b.into()),
                Err(e) => {
                    warn!(%e, "gateway compression failed");
                    return Some(self.without_compression());
                }
            },
            None => tungstenite::Message::Text(String::from_utf8(json).expect("JSON is UTF-8").into()),
        };
        if let Err(e) = self.sink.send(msg).await {
            warn!(%e, "gateway send failed");
            return Some(Exit::Reconnect { immediate: false });
        }
        None
    }

    async fn close(&mut self, code: u16, reason: &str) {
        let frame = CloseFrame { code: WsCloseCode::from(code), reason: reason.into() };
        let _ = self.sink.send(tungstenite::Message::Close(Some(frame))).await;
    }

    async fn heartbeat(&mut self) -> Option<Exit> {
        // Only the first unanswered heartbeat starts the ACK timer.
        self.ack_deadline.get_or_insert_with(|| Instant::now() + HEARTBEAT_ACK_TIMEOUT);
        self.send(&GatewayPayload::command(OpCode::HEARTBEAT, self.s.seq)).await
    }

    async fn identify(&mut self) -> Option<Exit> {
        self.s.clear();
        let c = &self.s.config;
        let d = Identify {
            token: c.token.clone(),
            properties: c.properties.clone(),
            presence: c.presence.clone(),
            flags: c.flags,
            ..Default::default()
        };
        info!("gateway IDENTIFY");
        self.send(&GatewayPayload::command(OpCode::IDENTIFY, d)).await
    }

    async fn on_binary(&mut self, frame: &[u8]) -> Option<Exit> {
        let Some(z) = &mut self.zstd else { return self.on_json(frame).await };
        match z.decompress(frame) {
            Ok(values) => {
                for v in values {
                    if let Some(exit) = self.on_value(v).await {
                        return Some(exit);
                    }
                }
                None
            }
            Err(e) => {
                warn!(%e, "gateway zstd decode failed");
                self.close(4002, "Decompression failed").await;
                Some(self.without_compression())
            }
        }
    }

    /// `GatewaySocket.handleRecoverableCompressionDecodeError`: the codec is unusable, so the
    /// next socket (same session) runs without compression instead of failing forever.
    fn without_compression(&mut self) -> Exit {
        warn!("gateway compression failed; reconnecting without compression");
        self.s.config.compress = false;
        Exit::Reconnect { immediate: true }
    }

    async fn on_json(&mut self, bytes: &[u8]) -> Option<Exit> {
        match serde_json::from_slice(bytes) {
            Ok(v) => self.on_value(v).await,
            Err(e) => {
                warn!(%e, "gateway sent invalid JSON");
                None
            }
        }
    }

    async fn on_value(&mut self, v: Value) -> Option<Exit> {
        let p: GatewayPayload = match serde_json::from_value(v) {
            Ok(p) => p,
            Err(e) => {
                warn!(%e, "malformed gateway payload");
                return None;
            }
        };
        match p.op {
            OpCode::DISPATCH => self.on_dispatch(p),
            OpCode::HEARTBEAT => return self.heartbeat().await,
            OpCode::HEARTBEAT_ACK => self.ack_deadline = None,
            OpCode::HELLO => {
                let interval = serde_json::from_value::<Hello>(p.d).map_or(0, |h| h.heartbeat_interval);
                let interval = Duration::from_millis(interval.max(1000));
                self.hello_deadline = None;
                self.interval = Some(interval);
                self.next_heartbeat = Some(Instant::now() + heartbeat_delay(interval));
                let (Some(id), Some(seq)) = (self.s.id.clone(), self.s.seq) else { return self.identify().await };
                info!(seq, "gateway RESUME");
                self.resuming = true;
                self.resume_deadline = Some(Instant::now() + RESUME_TIMEOUT);
                let d = Resume { token: self.s.config.token.clone(), session_id: id, seq };
                return self.send(&GatewayPayload::command(OpCode::RESUME, d)).await;
            }
            OpCode::INVALID_SESSION => {
                // Always `d: false` (`handleInvalidSessionPayload`). A rejected Resume still has
                // its socket: Identify on it after a short spread. Anything else got its session
                // invalidated: reconnect with backoff instead of hammering Identify.
                info!("gateway INVALID_SESSION");
                self.s.clear();
                self.resume_deadline = None;
                if self.resuming {
                    self.resuming = false;
                    let spread = Duration::from_millis(fastrand::u64(..INVALID_SESSION_IDENTIFY_SPREAD_MS));
                    self.identify_at = Some(Instant::now() + spread);
                } else {
                    self.close(4000, "Invalid session").await;
                    return Some(Exit::Reconnect { immediate: false });
                }
            }
            OpCode::RECONNECT => {
                info!("gateway requested reconnect");
                self.close(4000, "Server requested reconnect").await;
                return Some(Exit::Reconnect { immediate: true });
            }
            OpCode::GATEWAY_ERROR => match serde_json::from_value(p.d) {
                Ok(e) => self.s.emit(GatewayEvent::Error(e)),
                Err(e) => warn!(%e, "malformed gateway error"),
            },
            op => debug!(op = op.0, "ignoring gateway opcode"),
        }
        None
    }

    fn on_dispatch(&mut self, p: GatewayPayload) {
        if let Some(s) = p.s {
            self.s.seq = Some(self.s.seq.map_or(s, |old| old.max(s)));
        }
        let Some(t) = p.t else { return };
        if t == "READY" || t == "RESUMED" {
            if t == "READY" {
                self.s.id = p.d.get("session_id").and_then(Value::as_str).map(str::to_owned);
            }
            info!(event = %t, "gateway connected");
            self.resuming = false;
            self.resume_deadline = None;
            self.s.attempts = 0;
            self.s.emit(GatewayEvent::State(ConnectionState::Connected));
        }
        match DispatchEvent::parse(&t, &p.d) {
            Ok(e) => self.s.emit(GatewayEvent::Dispatch(e)),
            Err(e) => {
                warn!(event = %t, %e, "malformed dispatch, skipped");
                self.s.emit(GatewayEvent::Malformed { t, d: p.d, error: e.to_string() });
            }
        }
    }

    fn on_close(&mut self, frame: Option<CloseFrame>) -> Exit {
        let (code, reason) = frame.map_or((1006, String::new()), |f| (u16::from(f.code), f.reason.to_string()));
        let code = CloseCode(code);
        warn!(code = code.0, %reason, "gateway closed");
        if !code.can_reconnect() {
            self.s.clear();
            return Exit::Fatal(code);
        }
        // The resume seq was rejected, the session timed out, or the resumed session is gone.
        if matches!(code, CloseCode::INVALID_SEQUENCE | CloseCode::SESSION_TIMEOUT)
            || (self.resuming && code == CloseCode::UNKNOWN_ERROR)
        {
            self.s.clear();
        }
        Exit::Reconnect { immediate: false }
    }
}

/// 80% of the interval plus up to 5% (max 1.5 s) jitter, at least 1 s.
/// Client opcodes that change nothing others can see (read-only mode allows only these).
fn read_only_op(op: OpCode) -> bool {
    matches!(
        op,
        OpCode::HEARTBEAT
            | OpCode::IDENTIFY
            | OpCode::RESUME
            | OpCode::REQUEST_GUILD_MEMBERS
            | OpCode::LAZY_REQUEST
            | OpCode::REQUEST_GUILD_COUNTS
            | OpCode::REQUEST_CHANNEL_MEMBER_COUNTS
            | OpCode::REQUEST_FORUM_UNREADS
    )
}

fn heartbeat_delay(interval: Duration) -> Duration {
    let ms = interval.as_millis() as u64;
    let jitter = (ms / 20).min(1500);
    Duration::from_millis((ms * 4 / 5).max(1000) + fastrand::u64(..=jitter))
}

/// 1 s doubling to 10 s, plus up to 1 s of spread.
fn backoff(attempt: u32) -> Duration {
    let base = MIN_RECONNECT.saturating_mul(1 << attempt.saturating_sub(1).min(4)).min(MAX_RECONNECT);
    base + Duration::from_millis(fastrand::u64(..1000))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message as Ws;

    #[test]
    fn url_params() {
        assert_eq!(
            gateway_url("wss://gateway.fluxer.app", true).unwrap(),
            "wss://gateway.fluxer.app/?v=1&encoding=json&compress=zstd-stream&stream=1"
        );
        assert_eq!(gateway_url("ws://h:1/gw?x=y&v=9", false).unwrap(), "ws://h:1/gw?x=y&v=1&encoding=json");
        assert!(gateway_url("https://h", false).is_err());
    }

    #[test]
    fn timings() {
        for _ in 0..100 {
            let d = heartbeat_delay(Duration::from_millis(41_250));
            assert!((33_000..=34_500).contains(&(d.as_millis() as u64)));
            assert!(backoff(1) >= MIN_RECONNECT && backoff(1) < MIN_RECONNECT * 2);
            assert!(backoff(30) >= MAX_RECONNECT && backoff(30) < MAX_RECONNECT + MIN_RECONNECT);
        }
        assert!(heartbeat_delay(Duration::from_millis(100)) >= Duration::from_secs(1));
    }

    /// Mock Gateway: one zstd stream per socket in each direction, or plain JSON text.
    struct Server {
        ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        z: Option<ZstdStream>,
    }

    impl Server {
        async fn accept(l: &TcpListener) -> Self {
            Self::accept_with(l, true).await
        }
        async fn accept_with(l: &TcpListener, compress: bool) -> Self {
            let (tcp, _) = l.accept().await.unwrap();
            Self { ws: tokio_tungstenite::accept_async(tcp).await.unwrap(), z: compress.then(|| ZstdStream::new().unwrap()) }
        }
        async fn send(&mut self, v: Value) {
            let msg = match &mut self.z {
                Some(z) => Ws::Binary(z.compress(v.to_string().as_bytes()).unwrap().into()),
                None => Ws::Text(v.to_string().into()),
            };
            self.ws.send(msg).await.unwrap();
        }
        async fn recv(&mut self) -> Value {
            match self.ws.next().await.unwrap().unwrap() {
                Ws::Binary(b) => self.z.as_mut().expect("compressed frame on a plain socket").decompress(&b).unwrap().remove(0),
                Ws::Text(t) => serde_json::from_str(&t).unwrap(),
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    async fn next_state(gw: &mut Gateway) -> ConnectionState {
        loop {
            if let GatewayEvent::State(s) = gw.events.recv().await.unwrap() {
                return s;
            }
        }
    }

    /// The next close frame, skipping heartbeats still in flight (`ack_deadline` is 15 s, the
    /// test interval 1 s).
    async fn close_code(srv: &mut Server) -> u16 {
        loop {
            match srv.ws.next().await {
                Some(Ok(Ws::Close(Some(f)))) => return u16::from(f.code),
                Some(Ok(Ws::Binary(_) | Ws::Text(_) | Ws::Ping(_) | Ws::Pong(_))) => {}
                other => panic!("expected a close frame, got {other:?}"),
            }
        }
    }

    #[test]
    fn voice_test_channel_only() {
        use serde_json::json;
        let ok = |d| crate::allows_voice_state(&d, Some("15"));
        assert!(ok(json!({"guild_id": "1", "channel_id": "15"})));
        assert!(ok(json!({"guild_id": "1", "channel_id": null})), "leaving is allowed");
        assert!(!ok(json!({"guild_id": "1", "channel_id": "16"})));
        assert!(!ok(json!({"guild_id": "1"})), "a missing channel is not a leave");
        assert!(!crate::allows_voice_state(&json!({"channel_id": "15"}), None));
    }

    #[test]
    fn read_only_ops() {
        assert!([OpCode::HEARTBEAT, OpCode::IDENTIFY, OpCode::RESUME, OpCode::LAZY_REQUEST].into_iter().all(read_only_op));
        assert!(![OpCode::PRESENCE_UPDATE, OpCode::VOICE_STATE_UPDATE, OpCode::VOICE_SIGNAL].into_iter().any(read_only_op));
    }

    #[tokio::test]
    async fn hello_identify_heartbeat_resume() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut gw = Gateway::connect(GatewayConfig {
            url: format!("ws://{}", l.local_addr().unwrap()),
            token: "tok".into(),
            properties: IdentifyProperties { os: "Linux".into(), browser: "Reflux".into(), device: "desktop".into(), ..Default::default() },
            flags: SessionFlags::empty(),
            presence: None,
            compress: true,
        })
        .unwrap();

        let mut srv = Server::accept(&l).await;
        assert_eq!(next_state(&mut gw).await, ConnectionState::Connecting);
        srv.send(serde_json::json!({"op": 10, "d": {"heartbeat_interval": 1000}})).await;
        let identify = srv.recv().await;
        assert_eq!(identify["op"], 2);
        assert_eq!(identify["d"]["token"], "tok");
        assert_eq!(identify["d"]["properties"]["os"], "Linux");

        srv.send(serde_json::json!({"op": 0, "s": 1, "t": "READY", "d": {"session_id": "abc", "v": 1}})).await;
        assert_eq!(next_state(&mut gw).await, ConnectionState::Connected);
        assert!(matches!(gw.events.recv().await, Some(GatewayEvent::Dispatch(DispatchEvent::Ready(r))) if r.session_id == "abc"));
        // Malformed dispatch is reported with its JSON path and skipped; the next one still arrives.
        srv.send(serde_json::json!({"op": 0, "s": 2, "t": "MESSAGE_CREATE", "d": {"id": []}})).await;
        srv.send(serde_json::json!({"op": 0, "s": 3, "t": "SOMETHING_NEW", "d": {"x": 1}})).await;
        assert!(matches!(gw.events.recv().await, Some(GatewayEvent::Malformed { t, error, .. }) if t == "MESSAGE_CREATE" && error.starts_with("id: ")));
        assert!(matches!(gw.events.recv().await, Some(GatewayEvent::Dispatch(DispatchEvent::Unknown { t, .. })) if t == "SOMETHING_NEW"));

        // Server-requested heartbeat is answered at once with the last sequence.
        srv.send(serde_json::json!({"op": 1, "d": null})).await;
        assert_eq!(srv.recv().await, serde_json::json!({"op": 1, "d": 3}));
        srv.send(serde_json::json!({"op": 11})).await;
        // Scheduled heartbeat about 1 s after Hello.
        assert_eq!(srv.recv().await, serde_json::json!({"op": 1, "d": 3}));
        srv.send(serde_json::json!({"op": 11})).await;

        // A resumable close: the client reconnects and resumes on a fresh zstd stream.
        srv.ws.close(Some(CloseFrame { code: WsCloseCode::from(4000), reason: "drain".into() })).await.unwrap();
        assert!(matches!(next_state(&mut gw).await, ConnectionState::Reconnecting(_)));
        let mut srv = Server::accept(&l).await;
        srv.send(serde_json::json!({"op": 10, "d": {"heartbeat_interval": 41250}})).await;
        let resume = srv.recv().await;
        assert_eq!(resume, serde_json::json!({"op": 6, "d": {"token": "tok", "session_id": "abc", "seq": 3}}));

        // Invalid session: identify again on the same socket.
        srv.send(serde_json::json!({"op": 9, "d": false})).await;
        assert_eq!(srv.recv().await["op"], 2);

        // Authentication failure is final.
        srv.ws.close(Some(CloseFrame { code: WsCloseCode::from(4004), reason: "Invalid token".into() })).await.unwrap();
        loop {
            if let ConnectionState::Disconnected(code) = next_state(&mut gw).await {
                assert_eq!(code, Some(CloseCode::AUTHENTICATION_FAILED));
                break;
            }
        }
    }

    #[tokio::test]
    async fn developer_disconnect_resumes_and_reset_identifies() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut gw = Gateway::connect(GatewayConfig {
            url: format!("ws://{}", l.local_addr().unwrap()),
            token: "tok".into(),
            properties: Default::default(),
            flags: SessionFlags::empty(),
            presence: None,
            compress: true,
        })
        .unwrap();
        let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 41250}});
        let mut srv = Server::accept(&l).await;
        srv.send(hello.clone()).await;
        assert_eq!(srv.recv().await["op"], 2);
        srv.send(serde_json::json!({"op": 0, "s": 5, "t": "READY", "d": {"session_id": "abc", "v": 1}})).await;
        while !matches!(gw.events.recv().await, Some(GatewayEvent::Dispatch(DispatchEvent::Ready(_)))) {}
        let closed = |m: Option<Result<Ws, _>>| match m {
            Some(Ok(Ws::Close(Some(f)))) => u16::from(f.code),
            other => panic!("expected a close frame, got {other:?}"),
        };
        gw.commands.send(Command::Disconnect).unwrap();
        assert_eq!(closed(srv.ws.next().await), 4000);
        let mut srv = Server::accept(&l).await;
        srv.send(hello.clone()).await;
        assert_eq!(srv.recv().await, serde_json::json!({"op": 6, "d": {"token": "tok", "session_id": "abc", "seq": 5}}));
        gw.commands.send(Command::Reset).unwrap();
        assert_eq!(closed(srv.ws.next().await), 1000);
        let mut srv = Server::accept(&l).await;
        srv.send(hello).await;
        assert_eq!(srv.recv().await["op"], 2, "a reset session identifies again");
    }

    fn test_config(l: &TcpListener, compress: bool) -> GatewayConfig {
        GatewayConfig {
            url: format!("ws://{}", l.local_addr().unwrap()),
            token: "tok".into(),
            properties: Default::default(),
            flags: SessionFlags::empty(),
            presence: None,
            compress,
        }
    }

    /// Paused-clock helper: advance until the Gateway reports `want`, returning what it saw.
    /// (Without `start_paused` the clock never advances on its own, so this is deterministic.)
    async fn state_while_paused(gw: &mut Gateway, want: impl Fn(&ConnectionState) -> bool) -> ConnectionState {
        for _ in 0..80 {
            while let Ok(ev) = gw.events.try_recv() {
                if let GatewayEvent::State(s) = ev
                    && want(&s)
                {
                    return s;
                }
            }
            tokio::time::advance(Duration::from_millis(500)).await;
            tokio::task::yield_now().await;
        }
        panic!("gateway never reached the expected state");
    }

    #[tokio::test]
    async fn resume_without_an_answer_times_out_and_identifies() {
        tokio::time::pause();
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut gw = Gateway::connect(test_config(&l, true)).unwrap();
        let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 41250}});
        let mut srv = Server::accept(&l).await;
        srv.send(hello.clone()).await;
        assert_eq!(srv.recv().await["op"], 2);
        srv.send(serde_json::json!({"op": 0, "s": 1, "t": "READY", "d": {"session_id": "abc", "v": 1}})).await;
        state_while_paused(&mut gw, |s| *s == ConnectionState::Connected).await;

        // The socket dies; the next connection resumes, but the server never answers.
        drop(srv);
        state_while_paused(&mut gw, |s| matches!(s, ConnectionState::Reconnecting(_))).await;
        state_while_paused(&mut gw, |s| *s == ConnectionState::Connecting).await;
        let mut srv = Server::accept(&l).await;
        srv.send(hello.clone()).await;
        assert_eq!(srv.recv().await, serde_json::json!({"op": 6, "d": {"token": "tok", "session_id": "abc", "seq": 1}}));

        state_while_paused(&mut gw, |s| matches!(s, ConnectionState::Reconnecting(_))).await;
        assert_eq!(
            match srv.ws.next().await {
                Some(Ok(Ws::Close(Some(f)))) => u16::from(f.code),
                other => panic!("expected a 4000 close, got {other:?}"),
            },
            4000
        );
        // The session was cleared, so the next socket identifies again.
        state_while_paused(&mut gw, |s| *s == ConnectionState::Connecting).await;
        let mut srv = Server::accept(&l).await;
        srv.send(hello).await;
        assert_eq!(srv.recv().await["op"], 2);
    }

    #[tokio::test]
    async fn missing_heartbeat_ack_reconnects() {
        tokio::time::pause();
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut gw = Gateway::connect(test_config(&l, true)).unwrap();
        let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 1000}});
        let mut srv = Server::accept(&l).await;
        srv.send(hello).await;
        assert_eq!(srv.recv().await["op"], 2);
        srv.send(serde_json::json!({"op": 0, "s": 2, "t": "READY", "d": {"session_id": "abc", "v": 1}})).await;
        state_while_paused(&mut gw, |s| *s == ConnectionState::Connected).await;

        // The scheduled heartbeat goes out; no ACK follows, so the socket is declared dead.
        // Its timer was registered when HELLO arrived, so advancing the paused clock fires it.
        tokio::time::advance(Duration::from_millis(1000)).await;
        tokio::task::yield_now().await;
        assert_eq!(srv.recv().await, serde_json::json!({"op": 1, "d": 2}));
        state_while_paused(&mut gw, |s| matches!(s, ConnectionState::Reconnecting(_))).await;
        assert_eq!(close_code(&mut srv).await, 4000);
        // The session was still resumable, so the reconnect resumes it.
        state_while_paused(&mut gw, |s| *s == ConnectionState::Connecting).await;
        let mut srv = Server::accept(&l).await;
        srv.send(serde_json::json!({"op": 10, "d": {"heartbeat_interval": 41250}})).await;
        assert_eq!(srv.recv().await["op"], 6);
    }

    #[tokio::test]
    async fn compression_failure_falls_back_to_plain_json() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut gw = Gateway::connect(test_config(&l, true)).unwrap();
        let mut srv = Server::accept(&l).await;
        assert_eq!(next_state(&mut gw).await, ConnectionState::Connecting);
        srv.send(serde_json::json!({"op": 10, "d": {"heartbeat_interval": 41250}})).await;
        assert_eq!(srv.recv().await["op"], 2);
        srv.send(serde_json::json!({"op": 0, "s": 4, "t": "READY", "d": {"session_id": "abc", "v": 1}})).await;
        assert_eq!(next_state(&mut gw).await, ConnectionState::Connected);
        while !matches!(gw.events.recv().await, Some(GatewayEvent::Dispatch(DispatchEvent::Ready(_)))) {}

        // A binary frame that isn't a zstd stream: close 4002, then reconnect without compression.
        srv.ws.send(Ws::Binary(b"not zstd".to_vec().into())).await.unwrap();
        assert_eq!(
            match srv.ws.next().await {
                Some(Ok(Ws::Close(Some(f)))) => u16::from(f.code),
                other => panic!("expected a 4002 close, got {other:?}"),
            },
            4002
        );
        let mut srv = Server::accept_with(&l, false).await;
        srv.send(serde_json::json!({"op": 10, "d": {"heartbeat_interval": 41250}})).await;
        assert_eq!(srv.recv().await, serde_json::json!({"op": 6, "d": {"token": "tok", "session_id": "abc", "seq": 4}}));
        srv.send(serde_json::json!({"op": 0, "s": 5, "t": "RESUMED", "d": {}})).await;
        loop {
            if let GatewayEvent::State(ConnectionState::Connected) = gw.events.recv().await.unwrap() {
                break;
            }
        }
    }

    #[tokio::test]
    async fn an_invalid_session_without_a_resume_reconnects_with_backoff() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut gw = Gateway::connect(test_config(&l, true)).unwrap();
        let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 41250}});
        let mut srv = Server::accept(&l).await;
        assert_eq!(next_state(&mut gw).await, ConnectionState::Connecting);
        srv.send(hello.clone()).await;
        assert_eq!(srv.recv().await["op"], 2);
        srv.send(serde_json::json!({"op": 0, "s": 1, "t": "READY", "d": {"session_id": "abc", "v": 1}})).await;
        assert_eq!(next_state(&mut gw).await, ConnectionState::Connected);
        while !matches!(gw.events.recv().await, Some(GatewayEvent::Dispatch(DispatchEvent::Ready(_)))) {}

        srv.send(serde_json::json!({"op": 9, "d": false})).await;
        assert_eq!(close_code(&mut srv).await, 4000);
        assert!(matches!(next_state(&mut gw).await, ConnectionState::Reconnecting(_)), "backoff, not another Identify on this socket");
        let mut srv = Server::accept(&l).await;
        srv.send(hello).await;
        assert_eq!(srv.recv().await["op"], 2, "the session is gone: identify again");
    }

    #[tokio::test]
    async fn a_session_older_than_the_resume_window_identifies() {
        tokio::time::pause();
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut gw = Gateway::connect(test_config(&l, true)).unwrap();
        let hello = serde_json::json!({"op": 10, "d": {"heartbeat_interval": 41250}});
        let mut srv = Server::accept(&l).await;
        srv.send(hello.clone()).await;
        assert_eq!(srv.recv().await["op"], 2);
        srv.send(serde_json::json!({"op": 0, "s": 1, "t": "READY", "d": {"session_id": "abc", "v": 1}})).await;
        state_while_paused(&mut gw, |s| *s == ConnectionState::Connected).await;
        drop(srv);
        state_while_paused(&mut gw, |s| matches!(s, ConnectionState::Reconnecting(_))).await;
        // Stay offline past the window, then reconnect.
        tokio::time::advance(RESUME_WINDOW + Duration::from_secs(1)).await;
        state_while_paused(&mut gw, |s| *s == ConnectionState::Connecting).await;
        let mut srv = Server::accept(&l).await;
        srv.send(hello).await;
        assert_eq!(srv.recv().await["op"], 2, "no RESUME after the window");
    }
}
