//! No-WebRTC implementation: the `webrtc` feature is off, so libwebrtc is not compiled.
//!
//! Every public item `reflux` uses exists with the same shape, but behaves like "voice
//! unavailable": calls fail, there are no devices, tracks or screen sources, and nothing
//! touches the network. UI work can build in a fraction of the time.

use std::sync::{Arc, Mutex};

pub mod audio {
    //! The real audio path's public API, with every operation unavailable.

    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    pub use cpal;
    pub use crate::resample::Resampler;

    /// The rate everything inside runs at (WebRTC's and RNNoise's).
    pub const RATE: u32 = 48_000;
    /// 10 ms at [`RATE`]: one APM / RNNoise / WebRTC frame.
    pub const FRAME: usize = 480;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum Kind {
        /// A microphone track.
        Voice,
        /// Screen share audio.
        Stream,
        /// A one-shot entrance sound clip.
        Entrance,
        /// The mic test's playback of our own mic.
        Monitor,
    }

    /// One remote source: what it is, whose it is, and which connection.
    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    pub struct Source {
        pub kind: Kind,
        pub user: u64,
        pub conn: String,
    }

    /// The remote mix's settings. Nothing plays, but the app still writes these.
    #[derive(Debug)]
    pub struct Mixer {
        pub user_volume: HashMap<u64, f32>,
        pub stream_volume: HashMap<u64, f32>,
        pub entrance_volume: HashMap<u64, f32>,
        pub muted_users: HashSet<u64>,
        pub muted_streams: HashSet<u64>,
        pub deaf: bool,
        pub output: f32,
    }

    impl Default for Mixer {
        fn default() -> Self {
            Self {
                user_volume: HashMap::new(),
                stream_volume: HashMap::new(),
                entrance_volume: HashMap::new(),
                muted_users: HashSet::new(),
                muted_streams: HashSet::new(),
                deaf: false,
                output: 1.0,
            }
        }
    }

    impl Mixer {
        pub fn push(&mut self, _src: Source, _samples: &[f32]) {}
        pub fn remove(&mut self, _src: &Source) {}
        pub fn mix(&mut self, out: &mut [f32]) {
            out.fill(0.0);
        }
    }

    /// The mic processing settings. Kept so the settings UI and `Call::set_processing` compile.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Processing {
        pub echo_cancellation: bool,
        pub noise: crate::Noise,
        pub auto_gain_control: bool,
        pub input_gain: f32,
    }

    impl Default for Processing {
        fn default() -> Self {
            Self { echo_cancellation: true, noise: crate::Noise::default(), auto_gain_control: true, input_gain: 1.0 }
        }
    }

    /// The call's audio device. Never opens a stream.
    #[derive(Default)]
    pub struct Engine {
        pub mixer: Arc<std::sync::Mutex<Mixer>>,
    }

    impl Engine {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn level(&self) -> f32 {
            0.0
        }
        pub fn set_processing(&self, _p: Processing) {}
        pub fn open_output(&self, _name: Option<&str>) -> bool {
            false
        }
        pub fn open_input(&self, _name: Option<&str>) -> bool {
            false
        }
        pub fn set_monitor(&self, _on: bool) {}
        pub fn close_input(&self) {}
    }

    /// A running raw microphone capture: the mic test meter, voice-activity outside calls and
    /// voice-message recording. Never opens a device here.
    pub struct Capture {
        pub rate: u32,
    }

    /// Opens the selected microphone. Unavailable without the WebRTC build.
    pub fn capture(_device: Option<&str>, _on_block: impl FnMut(&[f32]) + Send + 'static) -> Option<Capture> {
        None
    }

    /// The input device called `name`: unavailable without the WebRTC build.
    pub fn input_device(_name: Option<&str>) -> Option<cpal::Device> {
        None
    }
}

pub mod mesh {
    //! The P2P mesh's public API. A stub call never has a mesh.

    use serde_json::Value;

    /// `VOICE_MESH_SOURCES`, in transceiver order.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

        /// The source a `voiceMeshTrackSid` names.
        pub fn of_sid(sid: &str) -> Option<Src> {
            Src::ALL.into_iter().find(|s| sid.ends_with(&format!("_{}", s.name())))
        }
    }

    /// `voiceMeshTrackSid`.
    pub fn track_sid(connection_id: &str, src: Src) -> String {
        format!("TR_{connection_id}_{}", src.name())
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
        pub ice_servers: Vec<Vec<String>>,
    }

    /// A remote track. There are none without the WebRTC build.
    #[derive(Clone, Debug)]
    pub enum Track {
        Audio,
        Video,
    }

    /// What the mesh asks of the app.
    #[derive(Clone)]
    pub enum Event {
        Signal { to: String, data: Value },
        Status { conn: String, user: u64, status: Status },
        Remote { conn: String, user: u64, src: Src, track: Option<Track> },
        Report(Value),
    }

    /// A peer-to-peer call. The stub never has one.
    pub struct Mesh;

    impl Mesh {
        pub fn connection_id(&self) -> &str {
            ""
        }
        pub fn has(&self, _conn: &str) -> bool {
            false
        }
        pub fn set_roster(&self, _roster: &[(String, u64)]) {}
        pub fn signal(&self, _from: &str, _user: u64, _data: &Value) {}
        pub fn fail_peer(&self, _conn: &str) {}
        pub fn server_mute(&self, _conn: &str, _muted: bool) {}
        pub fn set_muted(&self, _src: Src, _muted: bool) {}
        pub fn set_watching(&self, _user: Option<u64>) {}
        pub fn resume(&self) {}
        pub fn close(&self) {}
    }
}

pub use audio::{Engine, Kind, Mixer, Processing, Source};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("voice calls are unavailable in this build (no WebRTC); rebuild with the default features")]
    Unavailable,
    #[error("screen capture is unavailable or was cancelled")]
    Capture,
}

/// A stub room event; a stub [`Call::connect`] always fails, so none is ever produced.
pub enum RoomEvent {}

/// A stub remote track type for [`Call::play`] / [`Call::video_track`].
pub struct RemoteAudioTrack;
pub struct RemoteParticipant;
pub struct RtcAudioTrack;
pub struct RtcVideoTrack;

/// The LiveKit track source, kept for the public signatures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackSource {
    Microphone,
    Camera,
    Screenshare,
    ScreenshareAudio,
}

pub struct Call {
    pub encrypted: bool,
    /// Kept so `connect_mesh`'s receiver does not end immediately.
    _events: Option<tokio::sync::mpsc::UnboundedSender<mesh::Event>>,
}

impl Call {
    pub async fn connect(_endpoint: &str, _token: &str, _with_audio: bool, _e2ee_key: Option<&str>) -> Result<(Self, tokio::sync::mpsc::UnboundedReceiver<RoomEvent>), Error> {
        Err(Error::Unavailable)
    }

    pub fn connect_mesh(_target: mesh::Target) -> (Self, tokio::sync::mpsc::UnboundedReceiver<mesh::Event>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Self { encrypted: false, _events: Some(tx) }, rx)
    }

    pub fn mesh(&self) -> Option<&mesh::Mesh> {
        None
    }

    pub fn mic_level(&self) -> f32 {
        0.0
    }

    pub fn noise_state(&self) -> crate::NoiseState {
        crate::NoiseState::Idle
    }

    pub fn set_processing(&self, _p: Processing) {}

    pub fn mixer(&self) -> Option<Arc<Mutex<Mixer>>> {
        None
    }

    pub fn set_mic_gate(&self, _open: bool) {}

    pub async fn publish_mic(&self, _muted: bool) -> Result<(), Error> {
        Err(Error::Unavailable)
    }

    pub fn set_mic_muted(&self, _muted: bool) {}

    pub fn play(&self, _track: RemoteAudioTrack, _participant: &RemoteParticipant, _source: TrackSource) -> Option<impl Future<Output = ()> + Send + 'static> {
        None::<std::future::Ready<()>>
    }

    pub fn play_rtc(&self, _track: RtcAudioTrack, _src: Source, _speaking: impl FnMut(bool) + Send + 'static) -> Option<impl Future<Output = ()> + Send + 'static> {
        None::<std::future::Ready<()>>
    }

    pub fn set_devices(&self, _input: Option<&str>, _output: Option<&str>) {}

    pub async fn publish_stream_audio(&self) -> Result<StreamAudio, Error> {
        Err(Error::Unavailable)
    }

    pub async fn unpublish_stream_audio(&self, _audio: &StreamAudio) -> Result<(), Error> {
        Ok(())
    }

    pub async fn rtt_ms(&self) -> Option<u32> {
        None
    }

    pub async fn start_screen(&self, _quality: ScreenQuality, _pick: Option<ScreenSource>) -> Result<ScreenShare, Error> {
        Err(Error::Unavailable)
    }

    pub async fn stop_screen(&self, _share: ScreenShare) -> Result<(), Error> {
        Ok(())
    }

    pub fn screen_track(&self, _user: u64) -> Option<RtcVideoTrack> {
        None
    }

    pub fn video_track(&self, _user: u64, _source: TrackSource) -> Option<RtcVideoTrack> {
        None
    }

    pub async fn publish_camera(&self, _quality: CameraQuality) -> Result<Camera, Error> {
        Err(Error::Unavailable)
    }

    pub async fn publish_test_screen(&self, _quality: ScreenQuality, _width: u32, _height: u32) -> Result<Camera, Error> {
        Err(Error::Unavailable)
    }

    pub async fn unpublish_camera(&self, _camera: &Camera) -> Result<(), Error> {
        Ok(())
    }

    pub async fn video_encoders(&self) -> Vec<(String, u32, String)> {
        Vec::new()
    }

    pub fn server_info(&self) -> (String, String) {
        (String::new(), String::new())
    }

    pub async fn leave(&self) -> Result<(), Error> {
        Ok(())
    }

    /// Our server-acknowledged publications. None without a room.
    pub async fn published(&self) -> Vec<Published> {
        Vec::new()
    }
}

/// Published screen share audio. Never published, but the app still calls [`StreamAudio::push`].
pub struct StreamAudio;

impl StreamAudio {
    pub fn push(&self, _samples: &[f32]) {}
}

/// A published camera track. Never published; frames are dropped.
pub struct Camera;

impl Camera {
    pub fn push_rgba(&self, _width: u32, _height: u32, _rgba: &[u8]) {}
}

/// A running screen share. There are none.
pub struct ScreenShare;

impl ScreenShare {
    /// No capture in the stub build, so a share never ends on its own.
    pub fn ended(&self) -> bool {
        false
    }

    pub fn track(&self) -> Option<RtcVideoTrack> {
        None
    }
}

impl Drop for ScreenShare {
    fn drop(&mut self) {}
}

/// What left this client for a published track. Never published, so the list is empty.
#[derive(Debug, Default)]
pub struct Published {
    pub source: String,
    pub sid: String,
    pub encoder: String,
    pub frames_encoded: u32,
    pub packets_sent: u64,
    pub bytes_sent: u64,
    pub sfu_reports: u64,
    pub sfu_lost: i64,
    pub frame_width: u32,
    pub frame_height: u32,
    pub frames_per_second: f64,
    pub target_bitrate: f64,
    pub limitation: String,
}

/// `ScreenShareContentHint`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContentHint {
    Auto,
    Motion,
    Detail,
    #[default]
    Text,
}

/// `ScreenShareScalabilityModePreference`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Svc {
    #[default]
    Auto,
    SingleLayer,
    Temporal,
}

/// `MAX_SCREEN_SHARE_BITRATE`: Fluxer's fallback ceiling (bits/s).
pub const MAX_SCREEN_SHARE_BITRATE: u64 = 9_000_000;

const SCREEN_SHARE_RUNGS: [(u32, u32); 6] = [(426, 240), (854, 480), (1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)];
const SCREEN_SHARE_KBPS: [[u64; 3]; 6] = [[300, 500, 700], [1200, 2000, 3000], [2000, 3000, 4500], [3000, 4500, 6000], [4000, 5500, 6000], [4500, 6000, 6000]];

fn floor_even(value: f64) -> u32 {
    ((value / 2.0).floor() as u32 * 2).max(2)
}

fn fps_bucket(fps: u32) -> u32 {
    if fps >= 60 { 60 } else if fps >= 30 { 30 } else { 15 }
}

fn fps_column(fps: u32) -> usize {
    if fps >= 60 { 2 } else if fps >= 30 { 1 } else { 0 }
}

fn bitrate_rung(pixels: u64) -> usize {
    let mut rung = 0;
    for (i, (w, h)) in SCREEN_SHARE_RUNGS.iter().enumerate() {
        if *w as u64 * *h as u64 > pixels {
            break;
        }
        rung = i;
    }
    rung
}

/// A camera track's capture size, frame rate and bitrate cap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraQuality {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u64,
}

/// Mirrors `livekit::options::VideoCodec` for the no-WebRTC build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoCodec { VP8, H264, VP9, AV1, H265 }

/// Screen share quality (`StreamSettings`). Kept so the picker's settings compile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenQuality {
    pub fps: u32,
    pub max_height: Option<u32>,
    pub bitrate: Option<u64>,
    pub ceiling: u64,
    pub content_hint: ContentHint,
    pub svc: Svc,
    pub codec: Option<VideoCodec>,
}

impl Default for ScreenQuality {
    fn default() -> Self {
        Self { fps: 30, max_height: Some(720), bitrate: None, ceiling: MAX_SCREEN_SHARE_BITRATE, content_hint: ContentHint::default(), svc: Svc::default(), codec: None }
    }
}

impl ScreenQuality {
    /// The frame size sent for a `w`×`h` capture (the preset's pixel budget), even sides.
    pub fn fit(&self, w: u32, h: u32) -> (u32, u32) {
        let (bw, bh) = match self.max_height {
            None => SCREEN_SHARE_RUNGS[SCREEN_SHARE_RUNGS.len() - 1],
            Some(m) => SCREEN_SHARE_RUNGS.iter().copied().find(|(_, h)| *h == m).unwrap_or_else(|| ((m as u64 * 16).div_ceil(9) as u32, m)),
        };
        let budget = bw as u64 * bh as u64;
        let pixels = w.max(1) as u64 * h.max(1) as u64;
        let (w, h) = if pixels <= budget {
            (w, h)
        } else {
            let scale = (budget as f64 / pixels as f64).sqrt();
            (floor_even(w as f64 * scale), floor_even(h as f64 * scale))
        };
        (w.max(2) & !1, h.max(2) & !1)
    }

    /// The encoder's bitrate cap (Fluxer's automatic table or the chosen value).
    pub fn max_bitrate(&self, width: u32, height: u32) -> u64 {
        let pixels = width as u64 * height as u64;
        let chosen = match self.bitrate {
            Some(b) => b,
            None => {
                let scale = (self.ceiling as f64 / MAX_SCREEN_SHARE_BITRATE as f64).max(1.0);
                let floor = SCREEN_SHARE_KBPS[bitrate_rung(pixels)][fps_column(self.fps)] as f64 * 1000.0 * scale;
                let computed = 0.02 * pixels as f64 * fps_bucket(self.fps) as f64 * scale;
                floor.max(computed).round() as u64
            }
        };
        chosen.min(self.ceiling)
    }
}

/// A screen or window for the in-app picker (`ScreenSharePickerModal`).
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenSource {
    pub window: bool,
    pub id: u64,
    pub title: String,
    /// A small straight-RGBA preview: (width, height, pixels).
    pub thumbnail: Option<(u32, u32, Vec<u8>)>,
}

impl ScreenSource {
    /// The id of [`ScreenSource::portal`]'s pick.
    pub const PORTAL: u64 = u64::MAX;

    pub fn portal(window: bool) -> ScreenSource {
        ScreenSource { window, id: Self::PORTAL, title: String::new(), thumbnail: None }
    }
}

/// Encoder preference: meaningless without encoders.
pub fn set_software_encoding(_on: bool) {}

/// Names of the microphones and speakers. There are none.
pub fn devices() -> (Vec<String>, Vec<String>) {
    (Vec::new(), Vec::new())
}

/// Whether the platform lets us list screens and windows ourselves.
pub fn can_list_sources() -> bool {
    false
}

/// Screens and windows for the in-app picker. There are none.
pub fn screen_sources(_thumb_w: u32) -> Vec<ScreenSource> {
    Vec::new()
}

/// Feeds a remote track's frames to `sink`. Without WebRTC there are no tracks to render.
pub async fn render_video(_track: RtcVideoTrack, _sink: impl FnMut(u32, u32, Vec<u8>)) {}

/// The user ID in a Fluxer participant identity (`user_{id}_{connection}`).
pub fn user_of(identity: &str) -> Option<u64> {
    let rest = identity.strip_prefix("user_")?;
    let (id, conn) = rest.split_once('_')?;
    (!conn.is_empty()).then_some(())?;
    id.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities() {
        assert_eq!(user_of("user_123_abc"), Some(123));
        assert_eq!(user_of("user_123_a_b"), Some(123));
        assert_eq!(user_of("user_123_"), None);
        assert_eq!(user_of("bot_1_x"), None);
    }

    #[test]
    fn screen_quality_fits() {
        let q = ScreenQuality::default();
        assert_eq!(q.fit(2560, 1440), (1280, 720));
        assert_eq!(q.fit(1280, 600), (1280, 600));
        assert_eq!(ScreenQuality { max_height: None, ..q }.fit(1921, 1081), (1920, 1080));
        assert_eq!(q.fit(3440, 1440), (1482, 620), "area budget, not just the height");
        assert_eq!(ScreenQuality { max_height: Some(480), ..q }.fit(1920, 1080), (852, 480));
        assert_eq!(ScreenQuality { max_height: Some(1440), ..q }.fit(3840, 2160), (2560, 1440));
        assert_eq!(q.max_bitrate(1280, 720), 3_000_000);
        assert_eq!(ScreenQuality { fps: 15, ..q }.max_bitrate(1280, 720), 2_000_000);
    }

    #[tokio::test]
    async fn calls_are_unavailable() {
        assert!(Call::connect("wss://example.invalid", "token", true, None).await.is_err());
    }
}
