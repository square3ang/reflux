//! Peer-to-peer calls: Fluxer's mesh engine (`features/voice/engine/mesh`) on libwebrtc.
//!
//! A peer-to-peer grant (VOICE_SERVER_UPDATE with `p2p: true` and STUN-only `ice_servers`)
//! means every pair of connections in the call runs its own `RTCPeerConnection`; nothing goes
//! through LiveKit. Signalling rides gateway op 13 (Voice Signal) as the JSON [`Signal`]s below,
//! exactly as Fluxer's `VoiceMeshSignal.ts` writes them, so we interoperate with the web app.
//!
//! Each connection has four `sendrecv` transceivers in a fixed order ([`Src::ALL`]). The one
//! with the smaller connection ID offers. Each side advertises what it publishes (`tracks`) and
//! asks for what it wants of the other's (`want`); a sender only carries a track the peer wants.
//! A peer gets 15 s to connect, an ICE restart after 5 s disconnected, and 10 s for the restart.
//!
//! [`Mesh`] is driven from the app: [`Mesh::set_roster`] from voice states, [`Mesh::signal`] from
//! VOICE_SIGNAL, [`Mesh::publish`] for local tracks; it reports back through [`Event`]s. Each peer
//! is a task that owns its peer connection, so negotiation steps never interleave (Fluxer's
//! per-peer promise queue).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use livekit::rtc_engine::lk_runtime::LkRuntime;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::peer_connection_factory::native::PeerConnectionFactoryExt;
use livekit::webrtc::prelude::*;
use livekit::webrtc::stats::{IceCandidateType, RtcStats};
use livekit::webrtc::video_source::native::NativeVideoSource;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::Instant;

const SETUP_DEADLINE: Duration = Duration::from_secs(15);
const DISCONNECTED_GRACE: Duration = Duration::from_secs(5);
const RESTART_DEADLINE: Duration = Duration::from_secs(10);
/// Signals from a connection not in the roster yet, kept until it shows up.
const INBOX_LIMIT: usize = 64;
const SCREEN_SHARE_BUDGET_BPS: u64 = 12_000_000;
const CAMERA_BUDGET_BPS: u64 = 4_000_000;

/// `VOICE_MESH_SOURCES`, in transceiver order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Src {
    Microphone,
    Camera,
    ScreenShare,
    ScreenShareAudio,
}

impl Src {
    pub const ALL: [Src; 4] = [Src::Microphone, Src::Camera, Src::ScreenShare, Src::ScreenShareAudio];

    pub fn name(self) -> &'static str {
        match self {
            Src::Microphone => "microphone",
            Src::Camera => "camera",
            Src::ScreenShare => "screen_share",
            Src::ScreenShareAudio => "screen_share_audio",
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    fn media(self) -> MediaType {
        match self {
            Src::Microphone | Src::ScreenShareAudio => MediaType::Audio,
            Src::Camera | Src::ScreenShare => MediaType::Video,
        }
    }

    /// The source a `voiceMeshTrackSid` names.
    pub fn of_sid(sid: &str) -> Option<Src> {
        Src::ALL.into_iter().find(|s| sid.ends_with(&format!("_{}", s.name())))
    }
}

/// `voiceMeshTrackSid`.
pub fn track_sid(connection_id: &str, src: Src) -> String {
    format!("TR_{connection_id}_{}", src.name())
}

/// `VoiceMeshTrackInfo`: a published source, muted or not, with its video size.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackInfo {
    pub src: Src,
    pub sid: String,
    pub m: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub w: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h: Option<f64>,
}

/// `RTCIceCandidateInit`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub candidate: String,
    pub sdp_mid: Option<String>,
    #[serde(rename = "sdpMLineIndex")]
    pub sdp_mline_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username_fragment: Option<String>,
}

/// `VoiceMeshSignalData`: the `data` of a Voice Signal. `g` is the negotiation generation,
/// `v` a version that only goes up.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum Signal {
    Offer { g: u64, sdp: String },
    Answer { g: u64, sdp: String },
    Ice { g: u64, c: Option<Candidate> },
    NeedOffer {
        g: u64,
        #[serde(default, skip_serializing_if = "std::ops::Not::not", deserialize_with = "only_true")]
        restart: bool,
    },
    Tracks { v: u64, tracks: Vec<TrackInfo> },
    Want { v: u64, sids: Vec<String> },
}

/// `restart` is either absent or `true` (`parseVoiceMeshSignalData`).
fn only_true<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    match bool::deserialize(d)? {
        true => Ok(true),
        false => Err(serde::de::Error::custom("restart must be true")),
    }
}

impl Signal {
    /// `parseVoiceMeshSignalData`: `None` for anything malformed (dropped).
    pub fn parse(v: &Value) -> Option<Signal> {
        serde_json::from_value(v.clone()).ok()
    }
}

/// `VoiceMeshPeerStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    Failed,
}

/// What the grant says about our side of the mesh.
#[derive(Clone, Debug)]
pub struct Target {
    pub guild_id: Option<u64>,
    pub channel_id: u64,
    pub connection_id: String,
    /// `stun:` URLs per ICE server.
    pub ice_servers: Vec<Vec<String>>,
}

/// What the mesh asks of the app.
#[derive(Clone)]
pub enum Event {
    /// Send `data` to connection `to` with op 13.
    Signal { to: String, data: Value },
    Status { conn: String, user: u64, status: Status },
    /// A remote source started reaching us (`Some`) or stopped.
    Remote { conn: String, user: u64, src: Src, track: Option<MediaStreamTrack> },
    /// A connection report for `POST /voice/p2p/connection-reports`.
    Report(Value),
}

#[derive(Clone, Default)]
struct Slot {
    track: Option<MediaStreamTrack>,
    /// (muted, width, height, bitrate cap) while published.
    published: Option<(bool, u32, u32, Option<u64>)>,
}

struct Local {
    slots: [Slot; 4],
    tracks: (u64, Vec<TrackInfo>),
    backend: VideoEncoderBackend,
}

/// A peer's remote track per source.
type Tracks = [Option<MediaStreamTrack>; 4];

struct Shared {
    target: Target,
    runtime: Arc<LkRuntime>,
    events: UnboundedSender<Event>,
    local: Mutex<Local>,
    version: AtomicU64,
    /// conn → (user, task).
    peers: Mutex<HashMap<String, (u64, UnboundedSender<Cmd>)>>,
    peer_count: AtomicUsize,
    /// What each peer wants of ours (screen share bitrate split).
    wants: Mutex<HashMap<String, HashSet<String>>>,
    /// Whose screen share we watch: only then we want it.
    watching: Mutex<Option<u64>>,
    /// Remote tracks reaching us: conn → (user, per source).
    receivers: Mutex<HashMap<String, (u64, Tracks)>>,
    pcs: Mutex<HashMap<String, PeerConnection>>,
    reported: Mutex<HashSet<String>>,
}

impl Shared {
    fn next_version(&self) -> u64 {
        self.version.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    fn broadcast(&self, cmd: impl Fn() -> Cmd) {
        for (_, tx) in self.peers.lock().unwrap().values() {
            let _ = tx.send(cmd());
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1)
}

pub struct Mesh {
    sh: Arc<Shared>,
    inbox: Mutex<HashMap<String, Vec<(u64, Signal)>>>,
    rt: tokio::runtime::Handle,
}

impl Mesh {
    /// Starts an empty mesh; peers come from [`Mesh::set_roster`]. Peer tasks run on `rt`.
    pub fn new(target: Target, rt: tokio::runtime::Handle) -> (Mesh, UnboundedReceiver<Event>) {
        let (events, rx) = unbounded_channel();
        let version = now_ms();
        let sh = Arc::new(Shared {
            target,
            runtime: LkRuntime::instance(),
            events,
            local: Mutex::new(Local { slots: Default::default(), tracks: (version, Vec::new()), backend: VideoEncoderBackend::Auto }),
            version: AtomicU64::new(version),
            peers: Mutex::default(),
            peer_count: AtomicUsize::new(0),
            wants: Mutex::default(),
            watching: Mutex::default(),
            receivers: Mutex::default(),
            pcs: Mutex::default(),
            reported: Mutex::default(),
        });
        (Mesh { sh, inbox: Mutex::default(), rt }, rx)
    }

    pub fn connection_id(&self) -> &str {
        &self.sh.target.connection_id
    }

    pub fn has(&self, conn: &str) -> bool {
        self.sh.peers.lock().unwrap().contains_key(conn)
    }

    /// The other connections in the mesh: (connection ID, user ID). Peers not in it are closed,
    /// new ones started (with any signals they sent before showing up).
    pub fn set_roster(&self, roster: &[(String, u64)]) {
        let mut peers = self.sh.peers.lock().unwrap();
        let before = peers.len();
        peers.retain(|conn, (user, tx)| {
            let keep = roster.iter().any(|(c, u)| c == conn && u == user);
            if !keep {
                let _ = tx.send(Cmd::Close);
                self.inbox.lock().unwrap().remove(conn);
                self.sh.wants.lock().unwrap().remove(conn);
            }
            keep
        });
        let kept = peers.len();
        for (conn, user) in roster {
            if *conn == self.sh.target.connection_id || peers.contains_key(conn) {
                continue;
            }
            let (tx, rx) = unbounded_channel();
            for (from_user, sig) in self.inbox.lock().unwrap().remove(conn).unwrap_or_default() {
                if from_user == *user {
                    let _ = tx.send(Cmd::Signal(sig));
                }
            }
            let _ = tx.send(Cmd::Start);
            peers.insert(conn.clone(), (*user, tx.clone()));
            self.rt.spawn(Peer::new(self.sh.clone(), conn.clone(), *user, tx).run(rx));
        }
        self.sh.peer_count.store(peers.len(), Ordering::Relaxed);
        let changed = kept != before || peers.len() != kept;
        drop(peers);
        if changed {
            self.sh.broadcast(|| Cmd::SyncAll);
        }
    }

    /// A VOICE_SIGNAL from `from` (owned by `user`).
    pub fn signal(&self, from: &str, user: u64, data: &Value) {
        let Some(sig) = Signal::parse(data) else { return };
        if let Some((owner, tx)) = self.sh.peers.lock().unwrap().get(from) {
            if *owner == user {
                let _ = tx.send(Cmd::Signal(sig));
            }
            return;
        }
        if from == self.sh.target.connection_id {
            return;
        }
        let mut inbox = self.inbox.lock().unwrap();
        let pending = inbox.entry(from.to_string()).or_default();
        if pending.len() < INBOX_LIMIT {
            pending.push((user, sig));
        }
    }

    /// `VoiceMeshPeer.fail`: gives up on `conn` (e.g. a signal to it was too big to send).
    pub fn fail_peer(&self, conn: &str) {
        if let Some((_, tx)) = self.sh.peers.lock().unwrap().get(conn) {
            let _ = tx.send(Cmd::Fail);
        }
    }

    /// A participant's server mute: their microphone stops playing here.
    pub fn server_mute(&self, conn: &str, muted: bool) {
        if let Some((_, tx)) = self.sh.peers.lock().unwrap().get(conn) {
            let _ = tx.send(Cmd::ServerMute(muted));
        }
    }

    /// Publishes `track` as `src` (`None` unpublishes). `size` is a video's frame size,
    /// `max_bitrate` a cap in bits/s.
    pub fn publish(&self, src: Src, track: Option<MediaStreamTrack>, muted: bool, size: (u32, u32), max_bitrate: Option<u64>) {
        {
            let mut local = self.sh.local.lock().unwrap();
            let published = track.is_some().then_some((muted, size.0, size.1, max_bitrate));
            local.slots[src.index()] = Slot { track, published };
        }
        self.refresh(src);
    }

    pub fn set_muted(&self, src: Src, muted: bool) {
        {
            let mut local = self.sh.local.lock().unwrap();
            match &mut local.slots[src.index()].published {
                Some(p) if p.0 != muted => p.0 = muted,
                _ => return,
            }
        }
        self.refresh(src);
    }

    /// Hardware or software video encoding for tracks attached from now on.
    pub fn set_encoder(&self, backend: VideoEncoderBackend) {
        self.sh.local.lock().unwrap().backend = backend;
    }

    /// `refreshTracks` + `syncSender`.
    fn refresh(&self, src: Src) {
        let tracks = {
            let mut local = self.sh.local.lock().unwrap();
            let conn = &self.sh.target.connection_id;
            let list: Vec<TrackInfo> = Src::ALL
                .iter()
                .filter_map(|&s| {
                    let (m, w, h, _) = local.slots[s.index()].published?;
                    Some(TrackInfo { src: s, sid: track_sid(conn, s), m, w: (w > 0).then_some(w as f64), h: (h > 0).then_some(h as f64) })
                })
                .collect();
            if list != local.tracks.1 {
                local.tracks = (self.sh.next_version(), list);
                Some(local.tracks.clone())
            } else {
                None
            }
        };
        if let Some((v, tracks)) = tracks {
            self.sh.broadcast(|| Cmd::Send(Signal::Tracks { v, tracks: tracks.clone() }));
        }
        self.sh.broadcast(|| Cmd::Sync(src));
    }

    /// Watch `user`'s screen share (and its audio), or nobody's.
    pub fn set_watching(&self, user: Option<u64>) {
        *self.sh.watching.lock().unwrap() = user;
        self.sh.broadcast(|| Cmd::Interest);
    }

    /// The gateway reconnected: signals may have been lost, so renegotiate whatever hasn't
    /// connected and re-send our tracks and wants.
    pub fn resume(&self) {
        self.sh.broadcast(|| Cmd::Resume);
    }

    /// A remote video (camera or screen share) of `user` that reaches us.
    pub fn remote_video(&self, user: u64, src: Src) -> Option<RtcVideoTrack> {
        self.sh.receivers.lock().unwrap().values().filter(|(u, _)| *u == user).find_map(|(_, t)| match &t[src.index()] {
            Some(MediaStreamTrack::Video(v)) => Some(v.clone()),
            _ => None,
        })
    }

    /// Whether a remote track of `user` from `src` reaches us (e.g. a screen share's audio).
    pub fn has_remote(&self, user: u64, src: Src) -> bool {
        self.sh.receivers.lock().unwrap().values().any(|(u, t)| *u == user && t[src.index()].is_some())
    }

    /// Each peer's status.
    pub fn connections(&self) -> Vec<(String, u64)> {
        self.sh.peers.lock().unwrap().iter().map(|(c, (u, _))| (c.clone(), *u)).collect()
    }

    /// The lowest round-trip time over the peers' selected candidate pairs.
    pub async fn rtt_ms(&self) -> Option<u32> {
        let pcs: Vec<PeerConnection> = self.sh.pcs.lock().unwrap().values().cloned().collect();
        let mut best = None;
        for pc in pcs {
            let Ok(stats) = pc.get_stats().await else { continue };
            for s in stats {
                if let RtcStats::CandidatePair(c) = s
                    && c.candidate_pair.nominated
                    && c.candidate_pair.responses_received > 0
                {
                    let ms = (c.candidate_pair.current_round_trip_time * 1000.0).round() as u32;
                    best = Some(best.map_or(ms, |b: u32| b.min(ms)));
                }
            }
        }
        best
    }

    /// Closes every peer connection.
    pub fn close(&self) {
        let peers = std::mem::take(&mut *self.sh.peers.lock().unwrap());
        for (_, tx) in peers.into_values() {
            let _ = tx.send(Cmd::Close);
        }
        self.inbox.lock().unwrap().clear();
    }
}

impl Drop for Mesh {
    fn drop(&mut self) {
        self.close();
    }
}

enum Cmd {
    Start,
    Resume,
    Signal(Signal),
    Send(Signal),
    Sync(Src),
    SyncAll,
    Interest,
    ServerMute(bool),
    Fail,
    Close,
    /// From the peer connection of generation `g`.
    Ice(u64, Candidate),
    State(u64, PeerConnectionState),
}

/// `VoiceMeshPeer`.
struct Peer {
    sh: Arc<Shared>,
    tx: UnboundedSender<Cmd>,
    conn: String,
    user: u64,
    offerer: bool,
    status: Status,
    remote_tracks: Vec<TrackInfo>,
    remote_tracks_v: u64,
    remote_want: HashSet<String>,
    remote_want_v: u64,
    local_want: Option<(u64, Vec<String>)>,
    server_muted: bool,
    pc: Option<PeerConnection>,
    transceivers: Vec<RtpTransceiver>,
    generation: u64,
    answered: bool,
    restarting: bool,
    /// Our offer of this generation, re-sent while an ICE restart waits.
    last_offer: Option<String>,
    created: Instant,
    setup_ms: Option<u64>,
    ice_restarted: bool,
    setup_deadline: Option<Instant>,
    disconnected_deadline: Option<Instant>,
    restart_deadline: Option<Instant>,
    emitted: [Option<String>; 4],
    /// Never-fed tracks, one per source: what a sender carries while the peer doesn't want
    /// ours (webrtc-sys crashes on `set_track(None)`), and what makes an answer sendrecv.
    idle: [MediaStreamTrack; 4],
}

type R = Result<(), RtcError>;

impl Peer {
    fn new(sh: Arc<Shared>, conn: String, user: u64, tx: UnboundedSender<Cmd>) -> Self {
        let offerer = sh.target.connection_id < conn;
        let factory = sh.runtime.pc_factory();
        let idle = Src::ALL.map(|src| -> MediaStreamTrack {
            match src.media() {
                MediaType::Audio => factory.create_audio_track(src.name(), NativeAudioSource::new(Default::default(), 48_000, 1, 0)).into(),
                _ => factory.create_video_track(src.name(), NativeVideoSource::new(VideoResolution { width: 2, height: 2 }, false)).into(),
            }
        });
        Self {
            sh,
            tx,
            conn,
            user,
            offerer,
            status: Status::Connecting,
            remote_tracks: Vec::new(),
            remote_tracks_v: 0,
            remote_want: HashSet::new(),
            remote_want_v: 0,
            local_want: None,
            server_muted: false,
            pc: None,
            transceivers: Vec::new(),
            generation: 0,
            answered: false,
            restarting: false,
            last_offer: None,
            created: Instant::now(),
            setup_ms: None,
            ice_restarted: false,
            setup_deadline: None,
            disconnected_deadline: None,
            restart_deadline: None,
            emitted: Default::default(),
            idle,
        }
    }

    async fn run(mut self, mut rx: UnboundedReceiver<Cmd>) {
        self.sh.emit(Event::Status { conn: self.conn.clone(), user: self.user, status: Status::Connecting });
        loop {
            let next = [self.setup_deadline, self.disconnected_deadline, self.restart_deadline].into_iter().flatten().min();
            let far = Instant::now() + Duration::from_secs(3600);
            tokio::select! {
                cmd = rx.recv() => match cmd {
                    None | Some(Cmd::Close) => break,
                    Some(cmd) => {
                        if let Err(e) = self.handle(cmd).await {
                            eprintln!("reflux-voice: mesh negotiation with {} failed: {e}", self.conn);
                            self.fail();
                        }
                    }
                },
                _ = tokio::time::sleep_until(next.unwrap_or(far)), if next.is_some() => {
                    if let Err(e) = self.timers().await {
                        eprintln!("reflux-voice: mesh recovery with {} failed: {e}", self.conn);
                        self.fail();
                    }
                }
            }
        }
        self.reset_transport();
        for src in Src::ALL {
            self.emit_receiver(src, None);
        }
        self.sh.receivers.lock().unwrap().remove(&self.conn);
    }

    async fn handle(&mut self, cmd: Cmd) -> R {
        if self.status == Status::Failed && !matches!(cmd, Cmd::Sync(_) | Cmd::SyncAll | Cmd::Interest | Cmd::ServerMute(_)) {
            return Ok(());
        }
        match cmd {
            Cmd::Start => {
                self.arm_setup();
                if self.offerer {
                    if self.pc.is_none() {
                        self.start_generation().await?;
                    }
                } else if self.generation == 0 {
                    self.send(Signal::NeedOffer { g: 0, restart: false });
                }
            }
            Cmd::Resume => {
                match self.status {
                    Status::Connected if self.restarting => self.recover().await?,
                    Status::Connected => {}
                    _ if self.offerer => self.start_generation().await?,
                    _ => {
                        self.arm_setup();
                        self.send(Signal::NeedOffer { g: 0, restart: false });
                    }
                }
                let (v, tracks) = self.sh.local.lock().unwrap().tracks.clone();
                self.send(Signal::Tracks { v, tracks });
                if let Some((v, sids)) = self.local_want.clone() {
                    self.send(Signal::Want { v, sids });
                }
            }
            Cmd::Signal(Signal::Tracks { v, tracks }) => self.receive_tracks(v, tracks),
            Cmd::Signal(Signal::Want { v, sids }) => self.receive_want(v, sids),
            Cmd::Signal(sig) => self.negotiate(sig).await?,
            Cmd::Send(sig) => self.send(sig),
            Cmd::Sync(src) => self.sync_sender(src),
            Cmd::SyncAll => Src::ALL.into_iter().for_each(|s| self.sync_sender(s)),
            Cmd::Interest => self.sync_interest(),
            Cmd::ServerMute(m) => {
                self.server_muted = m;
                self.sync_receiver(Src::Microphone);
            }
            Cmd::Ice(g, c) => {
                if self.pc.is_some() && g == self.generation {
                    self.send(Signal::Ice { g, c: Some(c) });
                }
            }
            Cmd::State(g, state) => {
                if self.pc.is_some() && g == self.generation {
                    self.connection_state(state).await?;
                }
            }
            Cmd::Fail => self.fail(),
            Cmd::Close => {}
        }
        Ok(())
    }

    fn send(&self, sig: Signal) {
        if self.status == Status::Failed {
            return;
        }
        if let Ok(data) = serde_json::to_value(&sig) {
            self.sh.emit(Event::Signal { to: self.conn.clone(), data });
        }
    }

    fn receive_tracks(&mut self, v: u64, tracks: Vec<TrackInfo>) {
        if v <= self.remote_tracks_v {
            return;
        }
        let mut seen = HashSet::new();
        if !tracks.iter().all(|t| seen.insert(t.src) && t.sid == track_sid(&self.conn, t.src)) {
            return;
        }
        self.remote_tracks_v = v;
        self.remote_tracks = tracks;
        self.sync_interest();
    }

    fn receive_want(&mut self, v: u64, sids: Vec<String>) {
        if v <= self.remote_want_v {
            return;
        }
        self.remote_want_v = v;
        self.remote_want = sids.into_iter().collect();
        self.sh.wants.lock().unwrap().insert(self.conn.clone(), self.remote_want.clone());
        // `handlePeerWant`: the screen share bitrate split may change for everyone.
        self.sh.broadcast(|| Cmd::SyncAll);
    }

    async fn negotiate(&mut self, sig: Signal) -> R {
        match sig {
            Signal::Offer { g, sdp } => {
                if self.offerer || g < self.generation {
                    return Ok(());
                }
                if g > self.generation {
                    return self.accept_generation(g, &sdp).await;
                }
                // An ICE restart of the current generation.
                let Some(pc) = self.pc.clone() else { return Ok(()) };
                self.ice_restarted = true;
                pc.set_remote_description(sdp_of(&sdp, SdpType::Offer)?).await?;
                self.send_answer(&pc, g).await
            }
            Signal::Answer { g, sdp } => {
                let Some(pc) = self.pc.clone() else { return Ok(()) };
                if !self.offerer || g != self.generation || pc.signaling_state() != SignalingState::HaveLocalOffer {
                    return Ok(());
                }
                pc.set_remote_description(sdp_of(&sdp, SdpType::Answer)?).await?;
                self.answered = true;
                Ok(())
            }
            Signal::Ice { g, c } => {
                let (Some(pc), Some(c)) = (self.pc.clone(), c) else { return Ok(()) };
                if g != self.generation {
                    return Ok(());
                }
                match IceCandidate::parse(c.sdp_mid.as_deref().unwrap_or(""), c.sdp_mline_index.unwrap_or(0) as i32, &c.candidate) {
                    Ok(cand) => {
                        if let Err(e) = pc.add_ice_candidate(cand).await {
                            eprintln!("reflux-voice: dropping ICE candidate from {}: {e}", self.conn);
                        }
                    }
                    Err(e) => eprintln!("reflux-voice: bad ICE candidate from {}: {e:?}", self.conn),
                }
                Ok(())
            }
            Signal::NeedOffer { g, restart } => {
                if !self.offerer {
                    return Ok(());
                }
                if restart {
                    if g == self.generation {
                        self.recover().await?;
                    }
                    return Ok(());
                }
                if g == self.generation && self.answered {
                    return Ok(());
                }
                self.start_generation().await
            }
            Signal::Tracks { .. } | Signal::Want { .. } => Ok(()),
        }
    }

    async fn start_generation(&mut self) -> R {
        let g = if self.generation == 0 { now_ms() } else { self.generation + 1 };
        self.begin_generation(g);
        let pc = self.create_transport(g)?;
        let init = || RtpTransceiverInit { direction: RtpTransceiverDirection::SendRecv, stream_ids: Vec::new(), send_encodings: Vec::new() };
        self.transceivers = Src::ALL.into_iter().map(|src| pc.add_transceiver_for_media(src.media(), init())).collect::<Result<_, _>>()?;
        self.set_codec_preferences();
        self.transport_changed();
        self.send_offer(&pc, g, false).await
    }

    async fn accept_generation(&mut self, g: u64, sdp: &str) -> R {
        self.begin_generation(g);
        let pc = self.create_transport(g)?;
        // Transceivers made by setRemoteDescription are receive-only, and libwebrtc doesn't let
        // us change their direction. It reuses ones made by addTrack (in order, per kind)
        // instead, which stay sendrecv: so add the idle track of each source first.
        for track in &self.idle {
            pc.add_track(track.clone(), &[] as &[&str])?;
        }
        pc.set_remote_description(sdp_of(sdp, SdpType::Offer)?).await?;
        let transceivers = pc.transceivers();
        let layout = transceivers.len() == Src::ALL.len()
            && Src::ALL.iter().zip(&transceivers).all(|(src, t)| match t.receiver().track() {
                Some(MediaStreamTrack::Audio(_)) => src.media() == MediaType::Audio,
                Some(MediaStreamTrack::Video(_)) => src.media() == MediaType::Video,
                None => false,
            });
        if !layout {
            eprintln!("reflux-voice: offer from {} doesn't carry the mesh transceiver layout", self.conn);
            self.fail();
            return Ok(());
        }
        self.transceivers = transceivers;
        self.set_codec_preferences();
        self.transport_changed();
        self.send_answer(&pc, g).await
    }

    async fn send_offer(&mut self, pc: &PeerConnection, g: u64, ice_restart: bool) -> R {
        // `None`: keep the transceivers sendrecv (the default would make them send-only).
        let opts = OfferOptions { ice_restart, offer_to_receive_audio: None, offer_to_receive_video: None };
        let offer = pc.create_offer(opts).await?;
        let sdp = offer.to_string();
        pc.set_local_description(offer).await?;
        self.last_offer = Some(sdp.clone());
        self.send(Signal::Offer { g, sdp });
        Ok(())
    }

    async fn send_answer(&mut self, pc: &PeerConnection, g: u64) -> R {
        let answer = pc.create_answer(AnswerOptions::default()).await?;
        let sdp = answer.to_string();
        pc.set_local_description(answer).await?;
        self.send(Signal::Answer { g, sdp });
        Ok(())
    }

    /// ICE restart: the offerer sends a restart offer, the answerer asks for one.
    async fn recover(&mut self) -> R {
        let Some(pc) = self.pc.clone() else { return Ok(()) };
        if self.status == Status::Failed {
            return Ok(());
        }
        if self.restarting {
            if !self.offerer {
                self.send(Signal::NeedOffer { g: self.generation, restart: true });
            } else if pc.signaling_state() == SignalingState::HaveLocalOffer
                && let Some(sdp) = self.last_offer.clone()
            {
                self.send(Signal::Offer { g: self.generation, sdp });
            }
            return Ok(());
        }
        self.restarting = true;
        self.ice_restarted = true;
        self.restart_deadline = Some(Instant::now() + RESTART_DEADLINE);
        if self.offerer {
            return self.send_offer(&pc, self.generation, true).await;
        }
        self.send(Signal::NeedOffer { g: self.generation, restart: true });
        Ok(())
    }

    async fn timers(&mut self) -> R {
        let now = Instant::now();
        if self.setup_deadline.is_some_and(|d| d <= now) {
            self.setup_deadline = None;
            self.fail();
            return Ok(());
        }
        if self.disconnected_deadline.is_some_and(|d| d <= now) {
            self.disconnected_deadline = None;
            self.recover().await?;
        }
        if self.restart_deadline.is_some_and(|d| d <= now) {
            self.restart_deadline = None;
            if self.pc.as_ref().is_none_or(|pc| pc.connection_state() != PeerConnectionState::Connected) {
                self.fail();
            } else {
                self.restarting = false;
            }
        }
        Ok(())
    }

    fn begin_generation(&mut self, g: u64) {
        self.reset_transport();
        self.generation = g;
        self.answered = false;
        self.restarting = false;
        self.last_offer = None;
        self.set_status(Status::Connecting);
        self.arm_setup();
    }

    fn create_transport(&mut self, g: u64) -> Result<PeerConnection, RtcError> {
        let mut config = RtcConfiguration::default();
        config.ice_servers = self.sh.target.ice_servers.iter().map(|urls| IceServer { urls: urls.clone(), username: String::new(), password: String::new() }).collect();
        let pc = self.sh.runtime.pc_factory().create_peer_connection(config)?;
        let tx = self.tx.clone();
        pc.on_ice_candidate(Some(Box::new(move |c: IceCandidate| {
            let c = Candidate { candidate: c.candidate(), sdp_mid: Some(c.sdp_mid()), sdp_mline_index: Some(c.sdp_mline_index().max(0) as u32), username_fragment: None };
            let _ = tx.send(Cmd::Ice(g, c));
        })));
        let tx = self.tx.clone();
        pc.on_connection_state_change(Some(Box::new(move |state| {
            let _ = tx.send(Cmd::State(g, state));
        })));
        self.sh.pcs.lock().unwrap().insert(self.conn.clone(), pc.clone());
        self.pc = Some(pc.clone());
        Ok(pc)
    }

    /// Opus (with RED first) for audio; H.264 (Constrained Baseline first, the profile NVENC /
    /// VA-API encode), then VP8, for video.
    fn set_codec_preferences(&self) {
        let factory = self.sh.runtime.pc_factory();
        for (src, t) in Src::ALL.iter().zip(&self.transceivers) {
            let codecs = factory.get_rtp_receiver_capabilities(src.media()).codecs;
            let pick = |mime: &str| codecs.iter().filter(|c| c.mime_type.eq_ignore_ascii_case(mime)).cloned().collect::<Vec<_>>();
            let list = match src.media() {
                MediaType::Audio => [pick("audio/red"), pick("audio/opus")].concat(),
                _ => {
                    let mut h264 = pick("video/H264");
                    h264.sort_by_key(|c| !c.sdp_fmtp_line.as_deref().is_some_and(|f| f.contains("profile-level-id=42e01f")));
                    [h264, pick("video/VP8"), pick("video/rtx")].concat()
                }
            };
            if let Err(e) = t.set_codec_preferences(list) {
                eprintln!("reflux-voice: codec preferences for {}: {e}", src.name());
            }
        }
    }

    async fn connection_state(&mut self, state: PeerConnectionState) -> R {
        match state {
            PeerConnectionState::Connected => {
                self.clear_timers();
                self.restarting = false;
                self.setup_ms.get_or_insert(self.created.elapsed().as_millis() as u64);
                self.set_status(Status::Connected);
            }
            PeerConnectionState::Disconnected => {
                self.disconnected_deadline.get_or_insert(Instant::now() + DISCONNECTED_GRACE);
            }
            PeerConnectionState::Failed if self.restarting => self.fail(),
            PeerConnectionState::Failed => self.recover().await?,
            _ => {}
        }
        Ok(())
    }

    fn fail(&mut self) {
        if self.status == Status::Failed {
            return;
        }
        self.reset_transport();
        self.set_status(Status::Failed);
        self.transport_changed();
    }

    fn set_status(&mut self, status: Status) {
        if self.status == status {
            return;
        }
        self.status = status;
        self.sh.emit(Event::Status { conn: self.conn.clone(), user: self.user, status });
        if status == Status::Connecting {
            return;
        }
        self.report(status == Status::Connected);
        if status == Status::Connected {
            Src::ALL.into_iter().for_each(|s| self.sync_sender(s));
        }
    }

    /// `reportPeer`: once per connection per call.
    fn report(&self, connected: bool) {
        if !self.sh.reported.lock().unwrap().insert(self.conn.clone()) {
            return;
        }
        let t = &self.sh.target;
        let mut report = serde_json::json!({
            "channel_id": t.channel_id.to_string(),
            "guild_id": t.guild_id.map(|g| g.to_string()),
            "participant_count": self.sh.peer_count.load(Ordering::Relaxed) + 1,
            "outcome": if connected { "connected" } else { "failed" },
            "setup_ms": self.setup_ms,
            "ice_restarted": self.ice_restarted,
            "local_candidate_type": null, "remote_candidate_type": null, "ip_family": null, "protocol": null,
        });
        let (pc, sh) = (self.pc.clone(), self.sh.clone());
        tokio::spawn(async move {
            if let Some(stats) = match pc {
                Some(pc) => pc.get_stats().await.ok(),
                None => None,
            } {
                connection_path(&stats, &mut report);
            }
            sh.emit(Event::Report(report));
        });
    }

    fn transport_changed(&mut self) {
        for src in Src::ALL {
            self.sync_sender(src);
            self.sync_receiver(src);
        }
        if self.status == Status::Failed {
            return;
        }
        let (v, tracks) = self.sh.local.lock().unwrap().tracks.clone();
        self.send(Signal::Tracks { v, tracks });
        if let Some((v, sids)) = self.local_want.clone() {
            self.send(Signal::Want { v, sids });
        }
    }

    /// `syncPeerSender`: the sender carries our track only while the peer wants it.
    fn sync_sender(&self, src: Src) {
        let Some(t) = self.transceivers.get(src.index()) else { return };
        let sender = t.sender();
        let own = track_sid(&self.sh.target.connection_id, src);
        let local = self.sh.local.lock().unwrap();
        let slot = &local.slots[src.index()];
        let track = slot.published.and(slot.track.clone()).filter(|_| self.remote_want.contains(&own)).unwrap_or_else(|| self.idle[src.index()].clone());
        if sender.track().map(|t| t.id()) != Some(track.id()) {
            if src.media() == MediaType::Video {
                sender.set_video_encoder_backend(local.backend);
            }
            if let Err(e) = sender.set_track(Some(track)) {
                eprintln!("reflux-voice: mesh sender {}: {e}", src.name());
            }
        }
        if src.media() != MediaType::Video {
            return;
        }
        let cap = match src {
            Src::ScreenShare => {
                let watchers = self.sh.wants.lock().unwrap().values().filter(|w| w.contains(&own)).count();
                SCREEN_SHARE_BUDGET_BPS / watchers.max(1) as u64
            }
            _ => CAMERA_BUDGET_BPS / self.sh.peer_count.load(Ordering::Relaxed).max(1) as u64,
        };
        let cap = slot.published.and_then(|p| p.3).map_or(cap, |b| b.min(cap));
        let mut params = sender.parameters();
        if let Some(e) = params.encodings.first_mut()
            && e.max_bitrate != Some(cap)
        {
            e.max_bitrate = Some(cap);
            if let Err(e) = sender.set_parameters(params) {
                eprintln!("reflux-voice: mesh bitrate for {}: {e}", src.name());
            }
        }
    }

    /// `syncPeerInterest`: want their microphone and camera, and their screen share while we
    /// watch it.
    fn sync_interest(&mut self) {
        let watching = *self.sh.watching.lock().unwrap() == Some(self.user);
        let mut sids: Vec<String> = self
            .remote_tracks
            .iter()
            .filter(|t| watching || matches!(t.src, Src::Microphone | Src::Camera))
            .map(|t| t.sid.clone())
            .collect();
        sids.sort();
        if self.local_want.as_ref().map_or(&[][..], |w| &w.1[..]) != &sids[..] {
            let v = self.sh.next_version();
            self.local_want = Some((v, sids.clone()));
            self.send(Signal::Want { v, sids });
        }
        Src::ALL.into_iter().for_each(|s| self.sync_receiver(s));
    }

    /// `syncPeerReceiver`: a remote source plays once advertised and wanted (a server-muted
    /// microphone doesn't).
    fn sync_receiver(&mut self, src: Src) {
        let sid = track_sid(&self.conn, src);
        let advertised = self.remote_tracks.iter().any(|t| t.src == src);
        let wanted = self.local_want.as_ref().is_some_and(|w| w.1.contains(&sid));
        let muted = src == Src::Microphone && self.server_muted;
        let track = self.transceivers.get(src.index()).and_then(|t| t.receiver().track()).filter(|_| advertised && wanted && !muted);
        self.emit_receiver(src, track);
    }

    fn emit_receiver(&mut self, src: Src, track: Option<MediaStreamTrack>) {
        let id = track.as_ref().map(|t| t.id());
        if self.emitted[src.index()] == id {
            return;
        }
        self.emitted[src.index()] = id;
        let mut receivers = self.sh.receivers.lock().unwrap();
        receivers.entry(self.conn.clone()).or_insert((self.user, Default::default())).1[src.index()] = track.clone();
        drop(receivers);
        self.sh.emit(Event::Remote { conn: self.conn.clone(), user: self.user, src, track });
    }

    fn arm_setup(&mut self) {
        self.setup_deadline = Some(Instant::now() + SETUP_DEADLINE);
    }

    fn clear_timers(&mut self) {
        (self.setup_deadline, self.disconnected_deadline, self.restart_deadline) = (None, None, None);
    }

    fn reset_transport(&mut self) {
        self.clear_timers();
        self.transceivers.clear();
        if let Some(pc) = self.pc.take() {
            pc.on_ice_candidate(None);
            pc.on_connection_state_change(None);
            pc.close();
            self.sh.pcs.lock().unwrap().remove(&self.conn);
        }
    }
}

fn sdp_of(sdp: &str, kind: SdpType) -> Result<SessionDescription, RtcError> {
    SessionDescription::parse(sdp, kind).map_err(|e| RtcError { error_type: RtcErrorType::InvalidSdp, message: format!("{e:?}") })
}

/// `readVoiceMeshConnectionPath`: candidate types, IP family and protocol of the selected pair.
fn connection_path(stats: &[RtcStats], report: &mut Value) {
    let by_id = |id: &str| {
        stats.iter().find_map(|s| match s {
            RtcStats::LocalCandidate(c) if c.rtc.id == id => Some(&c.local_candidate),
            RtcStats::RemoteCandidate(c) if c.rtc.id == id => Some(&c.remote_candidate),
            _ => None,
        })
    };
    let selected = stats.iter().find_map(|s| match s {
        RtcStats::Transport(t) if !t.transport.selected_candidate_pair_id.is_empty() => Some(t.transport.selected_candidate_pair_id.clone()),
        _ => None,
    });
    let pair = stats.iter().find_map(|s| match s {
        RtcStats::CandidatePair(p) if selected.as_deref().map_or(p.candidate_pair.nominated, |id| p.rtc.id == id) => Some(&p.candidate_pair),
        _ => None,
    });
    let Some(pair) = pair else { return };
    let kind = |c: Option<&livekit::webrtc::stats::dictionaries::IceCandidateStats>| {
        c.and_then(|c| c.candidate_type).map(|t| match t {
            IceCandidateType::Host => "host",
            IceCandidateType::Srflx => "srflx",
            IceCandidateType::Prflx => "prflx",
            IceCandidateType::Relay => "relay",
        })
    };
    let (local, remote) = (by_id(&pair.local_candidate_id), by_id(&pair.remote_candidate_id));
    let family = |c: Option<&livekit::webrtc::stats::dictionaries::IceCandidateStats>| {
        let a = &c?.address;
        if a.parse::<std::net::Ipv4Addr>().is_ok() {
            Some("ipv4")
        } else {
            a.contains(':').then_some("ipv6")
        }
    };
    report["local_candidate_type"] = kind(local).into();
    report["remote_candidate_type"] = kind(remote).into();
    report["ip_family"] = family(local).or(family(remote)).into();
    report["protocol"] = local.map(|c| c.protocol.as_str()).filter(|p| matches!(*p, "udp" | "tcp")).into();
}

#[cfg(test)]
#[path = "mesh/tests.rs"]
mod tests;
