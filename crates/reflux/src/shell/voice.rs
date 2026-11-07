//! Voice: the connection panel over the user area (`voice/components/VoiceConnectionStatus*`,
//! `voice_connection_status/LocalParticipantControls.tsx`), the signal icon
//! (`SignalStrengthIcon.tsx`, `VoiceLatencySignal.ts`), the local mute/deafen rules
//! (`LocalVoiceStateMachine.ts`) and the stream viewer (`WatchStreamOverlay`,
//! `StreamFocusHeaderInfo`, `StreamSpectatorsPopout`, `ScreenShareBufferingFrame`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use reflux_model::{Permissions, Snowflake};
use reflux_store::Store;
use rune::{Color, El, Icon, LoopSpec, Pixels, col, el, icon, row, text};

use super::{Intent, ShellAccess, ShellState};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{Menu, MenuItem, Placement, mix, rgb, rgba};
use crate::i18n::{t, tf};

/// What the user area and sidebar show about voice (`MediaEngine` + `LocalVoiceState`).
#[derive(Default)]
pub struct Voice {
    /// (guild, channel) joined or being joined.
    pub channel: Option<(Option<Snowflake>, Snowflake)>,
    /// LiveKit room connected (else "Connecting...").
    pub connected: bool,
    /// The last join that failed ("Connection failed", retry / dismiss).
    pub failed: Option<(Option<Snowflake>, Snowflake)>,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// Undeafening restores the mic (it was on when deafening muted it).
    pub unmute_on_undeafen: bool,
    /// Sharing the screen, or waiting for the portal picker.
    pub screen: bool,
    /// Users LiveKit reports as active speakers.
    pub speaking: HashSet<Snowflake>,
    /// Latest round-trip time and the history the signal icon compares it against.
    pub ping: Option<u32>,
    pub pings: Vec<u32>,
    /// The user whose screen share the viewer shows, and its frames.
    pub watching: Option<Snowflake>,
    /// The watched stream's viewer state (`selectVoiceMediaGraphStreamTileState` over the parts
    /// Reflux knows: frames, the streamer's voice state and time).
    pub watch_state: WatchState,
    /// The user `watch_state` belongs to, and when the watch attempt began.
    pub watch_for: Option<Snowflake>,
    pub watch_started: Option<Instant>,
    /// Frames the render task produced, and the count / time `watch_state` last saw.
    pub stream_frames: Arc<AtomicU64>,
    pub watch_frame: u64,
    pub watch_frame_at: Option<Instant>,
    /// Estimated frame rate of the watched stream (`StreamInfoPill` fps).
    pub watch_fps: f32,
    fps_frames: u64,
    fps_at: Option<Instant>,
    /// `applyStreamWatchFocus`: the call view keeps this stream focused (pin).
    pub stream_pinned: bool,
    /// The running share's source stopped on its own (`ScreenShareEndedModal`).
    pub share_ended: bool,
    /// "Watch stream" on a channel we weren't in: watch once its track arrives.
    pub watch_on_join: Option<Snowflake>,
    /// The entrance sound scope the settings show (`None`: the open community's, else `global`).
    pub entrance_scope: Option<String>,
    /// Active Now stream previews by stream key.
    pub previews: HashMap<String, super::active_now::Preview>,
    pub stream: Pixels,
    /// Microphone and speaker names (`None` until listed), and the chosen ones.
    pub devices: Option<(Vec<String>, Vec<String>)>,
    pub input: Option<String>,
    pub output: Option<String>,
    /// The persisted Voice & video settings (mirrored from `Saved::voice_settings`).
    pub settings: Settings,
    /// Publishing the camera (or starting it).
    pub camera: bool,
    /// Our own camera frames (mirrored per `mirror_camera`), for the preview and our tile.
    pub self_view: Pixels,
    /// The camera preview modal's capture runs.
    pub previewing: bool,
    /// Remote camera frames by user.
    pub videos: HashMap<Snowflake, Pixels>,
    /// Cameras GStreamer sees (`None` until listed).
    pub cameras: Option<Vec<String>>,
    /// `VoiceCallView` replaces the chat column.
    pub call_view: bool,
    /// The tile shown large (`focusedParticipant`), `None` = the grid.
    pub focus: Option<Snowflake>,
    /// Users muted locally (volumes live in `settings`; both feed our mixer).
    pub local_mutes: HashSet<Snowflake>,
    /// The room's media is encrypted (`e2ee_key`).
    pub encrypted: bool,
    /// LiveKit endpoint host and room name of the connection.
    pub server: Option<(String, String)>,
    /// LiveKit lost the connection and is reconnecting.
    pub reconnecting: bool,
    /// The mic test meter runs; its last level in dBFS.
    pub mic_test: bool,
    pub level: f32,
    /// The mic gate (voice activity / push-to-talk) is open.
    pub transmitting: bool,
    /// The push-to-talk key is held in the window.
    pub ptt_held: bool,
    /// The next key pressed becomes the push-to-talk key.
    pub recording_key: bool,
    pub modal: Option<Modal>,
    /// The voice message composer.
    pub voice_message: Option<VoiceMessage>,
    /// Incoming calls the user declined or ignored (until the call ends).
    pub dismissed_calls: HashSet<Snowflake>,
    /// The watched stream fills the window.
    pub stream_fullscreen: bool,
    pub entrance: Option<EntranceLibrary>,
    pub trim: Option<Trim>,
    /// The in-app screen picker's screens and windows (X11 / Windows), with thumbnails, and the
    /// one picked (`None` = the system picker).
    pub screen_sources: Option<Vec<(reflux_voice::ScreenSource, Pixels)>>,
    pub screen_pick: Option<reflux_voice::ScreenSource>,
    /// The source the running share started from, so a quality change can restart it.
    pub share_pick: Option<reflux_voice::ScreenSource>,
    /// The picker's tab (`apps`, `displays`, `devices`).
    pub screen_tab: String,
    /// The camera / capture device a `Devices` share uses.
    pub screen_device: Option<String>,
    /// Linux screen share audio: (PipeWire `object.serial`, application name).
    pub audio_sources: Option<Vec<(String, String)>>,
    /// `VoiceP2pRollout` (`GET /experiments`): peer-to-peer calls offered, and their cap.
    pub p2p_rollout: (bool, usize),
    /// `restoreVoiceSession`'s (camera, screen share), started once the call connects.
    pub restore_media: (bool, bool),
    /// Users in our peer-to-peer call whose direct connection failed.
    pub p2p_failed: Vec<Snowflake>,
}

/// What the stream viewer shows (`VoiceParticipantTileScreenShareStateValue` and
/// `useScreenShareWatchFailure`): the first frame hasn't arrived, it renders, the attempt failed,
/// the streamer stopped sharing, or the tile is our own paused preview.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WatchState {
    #[default]
    Idle,
    Buffering,
    Live,
    /// `Watching failed :(` with the diagnostic code (`ScreenShareWatchErrorCode`).
    Failed(i32),
    Ended,
}

/// `WATCH_ATTEMPT_TIMEOUT_MS`: no first frame ends the attempt.
pub const WATCH_FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(15);
/// A live stream that stops producing frames shows the buffering frame again.
pub const WATCH_STALL_AFTER: Duration = Duration::from_secs(5);
/// `VOICE_MEDIA_GRAPH_FIRST_FRAME_TIMEOUT_FAILURE` and `VOICE_MEDIA_GRAPH_REPUBLISH_TIMEOUT_FAILURE`.
pub const FIRST_FRAME_TIMEOUT_CODE: i32 = -2303;
pub const REPUBLISH_TIMEOUT_CODE: i32 = -2304;

impl Voice {
    /// Starts (or restarts) a watch: no subscription internals, so the blank frame is the
    /// "publication missing / attaching" state and the first rendered frame moves it to live.
    pub fn begin_watch(&mut self, user: Snowflake) {
        self.watching = Some(user);
        self.watch_for = Some(user);
        self.watch_started = Some(Instant::now());
        self.watch_frame = 0;
        self.watch_frame_at = None;
        self.watch_fps = 0.0;
        self.fps_frames = 0;
        self.fps_at = None;
        self.stream_frames.store(0, Ordering::Relaxed);
        self.stream = Pixels::default();
        self.watch_state = WatchState::Buffering;
        // `openAndWatchStream` -> `applyStreamWatchFocus`: watching pins the stream in the call.
        (self.stream_pinned, self.focus) = (true, Some(user));
    }

    pub fn end_watch(&mut self) {
        let watched = self.watching.or(self.watch_for);
        self.watching = None;
        self.watch_for = None;
        self.watch_started = None;
        self.watch_frame = 0;
        self.watch_frame_at = None;
        self.watch_fps = 0.0;
        self.fps_frames = 0;
        self.fps_at = None;
        self.watch_state = WatchState::Idle;
        self.stream_pinned = false;
        if watched.is_some() && self.focus == watched {
            self.focus = None;
        }
    }

    /// `applyStreamWatchFocus`: pinning the watched stream makes it the call view's focus.
    pub fn toggle_watch_pin(&mut self) {
        let Some(user) = self.watching else { return };
        self.stream_pinned = !self.stream_pinned;
        if self.stream_pinned {
            self.focus = Some(user);
        } else if self.focus == Some(user) {
            self.focus = None;
        }
    }

    /// `WatchStreamOverlay` retry: another 15 s attempt on the same stream.
    pub fn retry_watch(&mut self) {
        if self.watching.is_none() {
            return;
        }
        self.watch_started = Some(Instant::now());
        self.watch_frame = 0;
        self.watch_frame_at = None;
        self.watch_fps = 0.0;
        self.fps_frames = 0;
        self.fps_at = None;
        self.stream_frames.store(0, Ordering::Relaxed);
        self.stream = Pixels::default();
        self.watch_state = WatchState::Buffering;
    }

    /// One viewer tick: a rendered frame makes the stream live, silence stalls it and then fails
    /// it, and the streamer leaving the share (`self_stream` off) ends it. Returns whether the
    /// viewer needs to redraw.
    pub fn watch_step(&mut self, store: &Store, now: Instant) -> bool {
        let Some(user) = self.watching else {
            let changed = self.watch_state != WatchState::Idle;
            self.watch_state = WatchState::Idle;
            return changed;
        };
        if self.watch_for != Some(user) || self.watch_started.is_none() {
            self.begin_watch(user);
            return true;
        }
        let started = self.watch_started.unwrap_or(now);
        let live = if user == store.my_id() {
            self.screen
        } else {
            self.channel.and_then(|(g, c)| store.voice_states.get(&(g, user)).map(|s| s.self_stream && s.channel_id == Some(c))).unwrap_or(false)
        };
        if !live {
            if self.watch_state != WatchState::Ended {
                self.watch_state = WatchState::Ended;
                crate::popout::stream_ended();
                return true;
            }
            return false;
        }
        let frames = self.stream_frames.load(Ordering::Relaxed);
        let mut changed = false;
        if frames != self.watch_frame {
            self.watch_frame = frames;
            self.watch_frame_at = Some(now);
            if self.watch_state != WatchState::Live {
                self.watch_state = WatchState::Live;
                changed = true;
            }
        }
        // `StreamInfoPill`'s fps: frames per second over at least a second, rounded where shown.
        if let Some(at) = self.fps_at {
            let dt = now.saturating_duration_since(at);
            if dt >= Duration::from_secs(1) {
                let fps = frames.saturating_sub(self.fps_frames) as f32 / dt.as_secs_f32();
                if fps.round() != self.watch_fps.round() {
                    changed = true;
                }
                self.watch_fps = fps;
                self.fps_frames = frames;
                self.fps_at = Some(now);
            }
        } else {
            (self.fps_at, self.fps_frames) = (Some(now), frames);
        }
        let last = self.watch_frame_at.unwrap_or(started);
        let quiet = now.saturating_duration_since(last);
        changed
            || match self.watch_state {
                // The last frame stays up, dimmed, under the corner buffering frame (`last-frame`).
                WatchState::Live if quiet >= WATCH_STALL_AFTER => {
                    self.watch_state = WatchState::Buffering;
                    true
                }
                WatchState::Buffering if self.watch_frame == 0 && now.saturating_duration_since(started) >= WATCH_FIRST_FRAME_TIMEOUT => {
                    self.watch_state = WatchState::Failed(FIRST_FRAME_TIMEOUT_CODE);
                    true
                }
                WatchState::Buffering if self.watch_frame > 0 && quiet >= WATCH_FIRST_FRAME_TIMEOUT => {
                    self.watch_state = WatchState::Failed(REPUBLISH_TIMEOUT_CODE);
                    true
                }
                _ => false,
            }
    }

    /// A frame reached the viewer (demo fixtures and the render callback).
    pub fn stream_frame(&mut self) {
        self.stream_frames.fetch_add(1, Ordering::Relaxed);
    }
}

/// One person watching the stream (`SpectatorEntry`): their user, voice connection and device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spectator {
    pub user: Snowflake,
    pub connection: String,
    pub mobile: bool,
}

/// `useStreamSpectators`: every other connection whose `viewer_stream_keys` holds the streamer's
/// key, deduplicated per connection, never the streamer or the local connection.
pub fn spectators_for(store: &Store, guild: Option<Snowflake>, streamer: Snowflake) -> Vec<Spectator> {
    let Some(state) = store.voice_states.get(&(guild, streamer)) else { return Vec::new() };
    let (Some(channel), Some(connection)) = (state.channel_id, state.connection_id.as_deref()) else { return Vec::new() };
    let key = format!("{}:{channel}:{connection}", guild.map_or("dm".to_string(), |g| g.to_string()));
    let me = store.my_id();
    let mut seen = HashSet::new();
    let mut list: Vec<Spectator> = store
        .voice_states
        .iter()
        .filter(|((g, u), v)| {
            *g == guild
                && *u != streamer
                && *u != me
                && v.channel_id == Some(channel)
                && v.viewer_stream_keys.iter().any(|k| k == &key)
                && v.connection_id.as_deref().is_some_and(|c| seen.insert(c.to_owned()))
        })
        .map(|((_, u), v)| Spectator { user: *u, connection: v.connection_id.clone().unwrap_or_default(), mobile: v.is_mobile })
        .collect();
    list.sort_by_key(|s| store.display_name(guild, s.user).to_lowercase());
    list
}

/// `isChannelP2p`: someone is in the call and every connection sends `p2p: true`.
pub fn is_p2p(store: &Store, guild: Option<Snowflake>, channel: Snowflake) -> bool {
    let mut states = store.voice_states.iter().filter(|((g, _), v)| *g == guild && v.channel_id == Some(channel)).peekable();
    states.peek().is_some() && states.all(|(_, v)| v.p2p)
}

/// Voice & video settings (`VoiceSettings.ts`, `Keybind` transmit mode) kept on disk.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    pub input_volume: f32,
    pub output_volume: f32,
    /// `voice` (focused voice), `studio` (direct input) or `custom`.
    pub processing: String,
    pub echo_cancellation: bool,
    pub noise_suppression: bool,
    pub auto_gain_control: bool,
    pub stereo: bool,
    pub push_to_talk: bool,
    /// The in-window push-to-talk key (`rune::Key` debug form).
    pub ptt_key: Option<String>,
    pub ptt_release_ms: u64,
    pub vad_auto: bool,
    pub vad_threshold: f32,
    pub show_connection_ids: bool,
    pub camera: Option<String>,
    pub mirror_camera: bool,
    /// `low` (480p), `medium` (720p) or `high` (1080p).
    pub camera_quality: String,
    /// `streamingMode`: `gaming` (1440p60), `screenshare` (1080p30) or `custom` (the saved
    /// resolution and frame rate below). The free tier clamps presets to 720p30.
    pub streaming_mode: String,
    /// Tallest custom screen share frame (`None` = source).
    pub screen_height: Option<u32>,
    pub screen_fps: u32,
    /// Bits/s, `None` = automatic.
    pub screen_bitrate: Option<u64>,
    /// `screenShareEncoderMode`: `auto`, `hardware` or `software` (older settings may hold
    /// `nvenc` / `vaapi`, which also pick that factory first).
    pub encoder: String,
    /// `screenShareContentHint`: `auto`, `motion`, `detail` or `text`.
    pub screen_content_hint: String,
    /// `screenShareScalabilityMode`: `auto` (H.264 / hardware), `single_layer` or `temporal`
    /// (VP9 + software, so the layers apply).
    pub screen_svc: String,
    /// `preferredScreenShareCodec`: `auto`, `av1`, `h265`, `h264`, `vp9` or `vp8`.
    pub screen_codec: String,
    /// `screenShareAv1OptIn` / `screenShareHevcOptIn`: AV1 and H.265 are off unless opted in.
    pub screen_av1_opt_in: bool,
    pub screen_hevc_opt_in: bool,
    /// `ParticipantVolume`: per-user volume (0..200 %).
    pub user_volumes: HashMap<u64, f32>,
    /// `StreamAudioPrefs`: per-streamer volume (0..200 %) and mutes.
    pub stream_volumes: HashMap<u64, f32>,
    pub stream_mutes: HashSet<u64>,
    /// `EntranceSoundListenerPrefs`: per-user entrance sound mutes and volume (0..200 %).
    pub entrance_mutes: HashSet<u64>,
    pub entrance_volumes: HashMap<u64, f32>,
    /// Linux: the PipeWire application node (its `object.serial`) a screen share sends audio
    /// from; `None` = no audio.
    pub stream_audio: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_volume: 100.0,
            output_volume: 100.0,
            processing: "voice".into(),
            echo_cancellation: true,
            noise_suppression: true,
            auto_gain_control: true,
            stereo: false,
            push_to_talk: false,
            ptt_key: None,
            ptt_release_ms: 20,
            vad_auto: true,
            vad_threshold: -60.0,
            show_connection_ids: false,
            camera: None,
            mirror_camera: true,
            camera_quality: "medium".into(),
            streaming_mode: "screenshare".into(),
            screen_height: Some(720),
            screen_fps: 30,
            screen_bitrate: None,
            encoder: "auto".into(),
            screen_content_hint: "text".into(),
            screen_svc: "auto".into(),
            screen_codec: "auto".into(),
            screen_av1_opt_in: false,
            screen_hevc_opt_in: false,
            user_volumes: HashMap::new(),
            stream_volumes: HashMap::new(),
            stream_mutes: HashSet::new(),
            entrance_mutes: HashSet::new(),
            entrance_volumes: HashMap::new(),
            stream_audio: None,
        }
    }
}

impl Settings {
    /// (echo cancellation, noise suppression, auto gain) for the chosen profile.
    pub fn processing(&self) -> (bool, bool, bool) {
        match self.processing.as_str() {
            "studio" => (false, false, false),
            "custom" => (self.echo_cancellation, self.noise_suppression, self.auto_gain_control),
            _ => (true, true, self.auto_gain_control),
        }
    }

    /// What the mic path runs (RNNoise = noise suppression) at the input volume.
    pub fn mic_processing(&self) -> reflux_voice::Processing {
        let (echo_cancellation, noise_suppression, auto_gain_control) = self.processing();
        reflux_voice::Processing { echo_cancellation, noise_suppression, auto_gain_control, input_gain: self.input_volume / 100.0 }
    }

    /// Copies the playback preferences into the call's mixer.
    pub fn apply_mixer(&self, m: &mut reflux_voice::Mixer, local_mutes: &HashSet<Snowflake>, deaf: bool) {
        m.output = self.output_volume / 100.0;
        m.deaf = deaf;
        m.user_volume = self.user_volumes.iter().map(|(u, v)| (*u, v / 100.0)).collect();
        m.stream_volume = self.stream_volumes.iter().map(|(u, v)| (*u, v / 100.0)).collect();
        m.entrance_volume = self.entrance_volumes.iter().map(|(u, v)| (*u, v / 100.0)).collect();
        m.muted_users = local_mutes.iter().map(|u| u.0).collect();
        m.muted_streams = self.stream_mutes.clone();
    }

    /// Push-to-talk applies outside direct input only.
    pub fn ptt(&self) -> bool {
        self.push_to_talk && self.processing != "studio"
    }

    /// The camera's capture size, frame rate and bitrate (`CAMERA_RESOLUTION_PRESET_TABLE`;
    /// 1080p needs the instance's `feature_higher_video_quality`).
    pub fn camera_quality(&self, higher_video_quality: bool) -> reflux_voice::CameraQuality {
        let (width, height, fps, bitrate) = match self.camera_quality.as_str() {
            // 480p is 640×360 in Fluxer.
            "low" => (640, 360, 20, 450_000),
            "high" if higher_video_quality => (1920, 1080, 30, 3_000_000),
            _ => (1280, 720, 30, 1_700_000),
        };
        reflux_voice::CameraQuality { width, height, fps, bitrate }
    }

    /// `resolveScreenShareTarget` for a display share: the streaming mode's resolution and
    /// frame rate (`gaming` 1440p60, `screenshare` 1080p30, `custom` the saved values), clamped
    /// to 720p/30 without `feature_higher_video_quality` (a P2P call always has it), plus the
    /// instance's bitrate ceiling.
    pub fn screen_quality(&self, limits: &crate::shell::upload::Limits, higher_video_quality: bool) -> reflux_voice::ScreenQuality {
        use reflux_voice::{ContentHint, Svc};
        let higher = higher_video_quality;
        let custom_hint = match self.screen_content_hint.as_str() {
            "auto" => ContentHint::Auto,
            "motion" => ContentHint::Motion,
            "detail" => ContentHint::Detail,
            _ => ContentHint::Text,
        };
        let (max_height, fps, content_hint) = match self.streaming_mode.as_str() {
            "gaming" => (Some(1440), 60, ContentHint::Motion),
            // `resolveScreenShareContentHintForMode`: Custom keeps the saved hint.
            "custom" => (self.screen_height, self.screen_fps, custom_hint),
            _ => (Some(1080), 30, ContentHint::Text),
        };
        let (max_height, fps) = if higher { (max_height, fps.clamp(1, 60)) } else { (Some(max_height.unwrap_or(720).min(720)), fps.clamp(1, 30)) };
        let svc = match self.screen_svc.as_str() {
            "single_layer" => Svc::SingleLayer,
            "temporal" => Svc::Temporal,
            _ => Svc::Auto,
        };
        let codec = match self.screen_codec.as_str() {
            "av1" if self.screen_av1_opt_in => Some(reflux_voice::VideoCodec::AV1),
            "h265" if self.screen_hevc_opt_in => Some(reflux_voice::VideoCodec::H265),
            "h264" => Some(reflux_voice::VideoCodec::H264),
            "vp9" => Some(reflux_voice::VideoCodec::VP9),
            "vp8" => Some(reflux_voice::VideoCodec::VP8),
            _ => None,
        };
        reflux_voice::ScreenQuality {
            fps,
            max_height,
            bitrate: higher.then_some(self.screen_bitrate).flatten(),
            ceiling: limits.screen_share_bitrate,
            content_hint,
            svc,
            codec,
        }
    }

    /// `resolveUserVideoTabScreenShareState`: the effective (resolution, frame rate) the selects
    /// show, and whether a preset owns them.
    pub fn screen_preset(&self) -> Option<(&'static str, Option<u32>, u32)> {
        match self.streaming_mode.as_str() {
            "gaming" => Some(("Gaming", Some(1440), 60)),
            "screenshare" => Some(("Screen share", Some(1080), 30)),
            _ => None,
        }
    }
}

/// `OFFERED_SCREEN_SHARE_RESOLUTIONS`: 480p and 720p; 1080p, 1440p and Source need the
/// instance's `feature_higher_video_quality`.
pub fn screen_resolutions(higher: bool) -> Vec<(String, String)> {
    let mut v = vec![("480".to_string(), "480p".to_string()), ("720".to_string(), "720p".to_string())];
    if higher {
        v.extend([("1080", "1080p"), ("1440", "1440p")].map(|(a, b)| (a.to_string(), b.to_string())));
        v.push(("source".into(), t("Source").to_string()));
    }
    v
}

/// `SUPPORTED_SCREEN_SHARE_FRAME_RATES` as Fluxer offers them: 15 / 30, and 60 with the
/// higher-quality feature.
pub fn screen_frame_rates(higher: bool) -> Vec<(String, String)> {
    let rates = if higher { &[15, 30, 60][..] } else { &[15, 30][..] };
    rates.iter().map(|f| (f.to_string(), crate::i18n::tf("{frameRate} FPS", &[("frameRate", &f.to_string())]))).collect()
}

/// `buildBitrateOptions` + Automatic: Fluxer's steps (`SCREEN_SHARE_BITRATE_STEPS_BPS`) below
/// the instance ceiling, the ceiling itself, then the saved value if it is not in the list.
pub fn screen_bitrates(ceiling: u64, chosen: Option<u64>) -> Vec<(String, String)> {
    const STEPS: [u64; 11] = [2_000_000, 4_000_000, 6_000_000, 8_000_000, 10_000_000, 15_000_000, 20_000_000, 25_000_000, 30_000_000, 40_000_000, 50_000_000];
    let mut values: Vec<u64> = STEPS.into_iter().filter(|b| *b < ceiling).collect();
    values.push(ceiling);
    values.extend(chosen.filter(|c| *c <= ceiling));
    values.sort_unstable();
    values.dedup();
    let mbps = |b: u64| if b.is_multiple_of(1_000_000) { (b / 1_000_000).to_string() } else { format!("{:.1}", b as f64 / 1_000_000.0) };
    let mut out = vec![("auto".to_string(), t("Automatic").to_string())];
    out.extend(values.into_iter().map(|b| {
        let label = crate::i18n::tf("{bitrate} Mbps", &[("bitrate", &mbps(b))]);
        (b.to_string(), label)
    }));
    out
}

/// Voice dialogs.
#[derive(Clone, Debug, PartialEq)]
pub enum Modal {
    /// `CameraPreviewModal`: the self view before turning the camera on.
    CameraPreview,
    /// `ScreenSharePickerModal`: quality, then the system picker.
    ScreenShare,
    /// `VoiceChannelFullModal`.
    ChannelFull,
    /// `VoiceConnectionConfirmModal`: already in voice on another device/channel.
    JoinConfirm { guild: Option<Snowflake>, channel: Snowflake },
    /// A peer-to-peer call Reflux can't join, or another refusal.
    Refused(String),
    /// `CallNotRingableModal`.
    NotRingable,
    /// `requestStartCall`'s confirm, with "Confirm before starting calls" on.
    /// `tip`: opened from a header button, where Shift would have skipped it.
    StartCall { channel: Snowflake, video: bool, tip: bool },
    /// `AudioProcessingModal`: the processing profile and its custom toggles.
    AudioProcessing,
    /// `EntranceSoundTrimmerModal`.
    Trim,
    /// `VoiceP2pConsentModal`: `start` a P2P call (with "Start standard call" when
    /// `allow_standard`) or join one (with "Do not ask again"); `video` turns the camera on after.
    P2pConsent { guild: Option<Snowflake>, channel: Snowflake, start: bool, allow_standard: bool, video: bool, always: bool },
    /// `VoiceP2pPeerFailedModal`: direct connections to these users failed.
    P2pFailed { names: Vec<String>, allow_switch: bool },
    /// `NewDeviceMonitoring`'s prompt: switch to a newly plugged-in device?
    NewDevice(NewDevice),
}

/// A newly seen sound card, and its input / output to switch to.
#[derive(Clone, Debug, PartialEq)]
pub struct NewDevice {
    pub name: String,
    pub input: Option<String>,
    pub output: Option<String>,
}

/// The card a cpal device name belongs to: `"E1x2 OTG, USB Audio"` and `"E1x2 OTG"` are one
/// device, like `MediaDeviceInfo.groupId`.
pub fn card(device: &str) -> &str {
    device.split_once(", ").map_or(device, |(c, _)| c)
}

/// `getNewDevicePromptCandidates`: cards not seen before nor ignored, and not the selected
/// devices' own. `known` grows by every card listed.
pub fn new_devices(inputs: &[String], outputs: &[String], known: &mut Vec<String>, ignored: &[String], selected: [Option<&str>; 2]) -> Vec<NewDevice> {
    let fresh = known.is_empty();
    let mut found: Vec<NewDevice> = Vec::new();
    for (i, list) in [inputs, outputs].into_iter().enumerate() {
        for d in list {
            let c = card(d);
            if known.iter().any(|k| k == c) || ignored.iter().any(|k| k == c) || selected[i].is_some_and(|s| card(s) == c) {
                continue;
            }
            let at = match found.iter().position(|f| f.name == c) {
                Some(at) => at,
                None => {
                    found.push(NewDevice { name: c.to_owned(), input: None, output: None });
                    found.len() - 1
                }
            };
            let slot = if i == 0 { &mut found[at].input } else { &mut found[at].output };
            slot.get_or_insert_with(|| d.clone());
        }
    }
    for d in inputs.iter().chain(outputs) {
        if !known.iter().any(|k| k == card(d)) {
            known.push(card(d).to_owned());
        }
    }
    // The first look only learns what's there.
    if fresh { Vec::new() } else { found }
}

/// One clip in the entrance sound library (`EntranceSoundLibrary`).
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct EntranceSound {
    pub id: Snowflake,
    pub name: String,
    pub url: String,
    pub duration_ms: u32,
}

/// The library and its scope selections (`GET /users/@me/entrance-sounds`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntranceLibrary {
    pub sounds: Vec<EntranceSound>,
    /// scope → sound.
    pub selections: HashMap<String, Snowflake>,
}

/// `getEntranceSoundFallbackScopes`: where a scope's sound comes from when it has none.
pub fn entrance_fallbacks(scope: &str) -> Vec<String> {
    match scope {
        "global" => vec!["global".into()],
        "guilds" | "dms" => vec![scope.into(), "global".into()],
        _ => vec![scope.into(), "guilds".into(), "global".into()],
    }
}

impl EntranceLibrary {
    /// The sound `scope` plays and the scope it comes from (itself or one it inherits).
    pub fn resolve_scope(&self, scope: &str) -> Option<(Snowflake, String)> {
        entrance_fallbacks(scope).into_iter().find_map(|s| Some((*self.selections.get(&s)?, s)))
    }

    /// `resolveForScope`: `guild:{id}`, then `guilds` or `dms`, then `global`.
    #[cfg(feature = "voice-webrtc")]
    pub fn resolve(&self, guild: Option<Snowflake>) -> Option<Snowflake> {
        let scopes = match guild {
            Some(g) => vec![format!("guild:{g}"), "guilds".into(), "global".into()],
            None => vec!["dms".into(), "global".into()],
        };
        scopes.iter().find_map(|s| self.selections.get(s).copied())
    }
}

/// A file being trimmed for upload: mono samples at 48 kHz and the chosen span in seconds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trim {
    pub name: String,
    pub samples: std::sync::Arc<Vec<f32>>,
    pub start: f32,
    pub end: f32,
    pub uploading: bool,
    pub error: Option<String>,
}

impl Trim {
    pub const MAX_SECS: f32 = 5.2;
    pub const MIN_SECS: f32 = 0.1;

    pub fn secs(&self) -> f32 {
        self.samples.len() as f32 / reflux_voice::audio::RATE as f32
    }

    /// Moves one handle, keeping the span within 0.1..=5.2 s by pulling the other along.
    pub fn set(&mut self, start: bool, at: f32) {
        let total = self.secs();
        let at = at.clamp(0.0, total);
        if start {
            self.start = at.min((total - Self::MIN_SECS).max(0.0));
            self.end = self.end.clamp(self.start + Self::MIN_SECS, self.start + Self::MAX_SECS).min(total);
        } else {
            self.end = at.max(Self::MIN_SECS.min(total));
            self.start = self.start.clamp((self.end - Self::MAX_SECS).max(0.0), (self.end - Self::MIN_SECS).max(0.0));
        }
    }

    pub fn selection(&self) -> &[f32] {
        let r = reflux_voice::audio::RATE as f32;
        let (a, b) = ((self.start * r) as usize, (self.end * r) as usize);
        &self.samples[a.min(self.samples.len())..b.min(self.samples.len())]
    }
}

/// The voice message being recorded or reviewed (`VoiceMessageComposerModal`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoiceMessage {
    pub channel: Snowflake,
    pub recording: bool,
    pub secs: f32,
    /// Recent block peaks (0..1) while recording; the final waveform after.
    pub bars: Vec<f32>,
    pub sending: bool,
    pub error: Option<String>,
    /// The kept span in seconds while reviewing (`AudioWaveform` selection).
    pub start: f32,
    pub end: f32,
    /// When the selection preview started playing (Pause = stop, like Fluxer).
    pub playing: Option<std::time::Instant>,
}

impl VoiceMessage {
    /// The preview's position in seconds while it plays (`AudioWaveform` playhead).
    pub fn playhead(&self) -> Option<f32> {
        let at = self.start + self.playing?.elapsed().as_secs_f32();
        (at < self.end).then_some(at)
    }

    /// `VOICE_MESSAGE_MIN_SEND_DURATION_MS`.
    pub const MIN_SECS: f32 = 0.5;

    /// Moves the handle nearest `at`, keeping at least 0.5 s selected (when the clip allows).
    pub fn drag(&mut self, at: f32) {
        let at = at.clamp(0.0, self.secs);
        let min = Self::MIN_SECS.min(self.secs);
        if (at - self.start).abs() <= (at - self.end).abs() {
            self.start = at.min(self.end - min).max(0.0);
        } else {
            self.end = at.max(self.start + min).min(self.secs);
        }
    }

    /// The selected sample range of a `rate` Hz recording of `len` samples.
    pub fn range(&self, len: usize, rate: u32) -> std::ops::Range<usize> {
        let at = |s: f32| ((s * rate as f32) as usize).min(len);
        at(self.start)..at(self.end).max(at(self.start))
    }
}

/// What the voice views ask the runtime for.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// "Don't suggest {deviceName} again".
    IgnoreDevice(String),
    ToggleCamera,
    /// Start or stop the camera preview capture.
    Preview(bool),
    LoadCameras,
    /// `voice.settings` changed: save and apply.
    Settings,
    /// `Change stream`: stop the running share and start it from the picker's new source.
    ChangeScreen,
    LocalMute(Snowflake, bool),
    /// Check eligibility, join and ring (`CallCommands.startCall`).
    StartCall(Snowflake),
    /// The P2P consent dialog's answer: `Some(true)` agrees, `Some(false)` is "Start standard
    /// call" (start a LiveKit call instead); `start` rings like [`Op::StartCall`].
    P2pConsent { guild: Option<Snowflake>, channel: Snowflake, p2p: bool, start: bool },
    /// `switchToStandardCall`: move the whole call to LiveKit (`p2p: false`).
    SwitchToStandard,
    /// Join a ringing call (`joinCall`).
    AcceptCall(Snowflake),
    /// Stop ringing us (`rejectCall`).
    DeclineCall(Snowflake),
    /// Ring (`Some`) or stop ringing (`None`) one recipient of the call we're in.
    Ring(Snowflake, Snowflake, bool),
    MicTest(bool),
    /// Start recording a voice message into this channel, stop and keep it, or cancel.
    RecordStart(Snowflake),
    RecordStop,
    RecordCancel,
    /// Upload the recorded message.
    RecordSend,
    /// Play the selected span of the recording.
    RecordPlay,
    /// The push-to-talk key went down or up in the window.
    PushToTalk(bool),
    /// "Join" in the already-in-voice confirmation.
    ConfirmJoin(Option<Snowflake>, Snowflake),
    /// Load the entrance sound library.
    LoadEntrance,
    /// Assign a clip to a scope (`None` clears it).
    SelectEntrance(String, Option<Snowflake>),
    DeleteEntrance(Snowflake),
    /// Play a clip from the library, or the trimmer's selection.
    PreviewEntrance(Snowflake),
    PreviewTrim,
    /// Upload the trimmer's selection.
    UploadTrim,
    /// Pop the call view, a participant's camera or stream into its own window; `pip` keeps
    /// a small one on top.
    PopOut { user: Option<Snowflake>, screen: bool, pip: bool },
    /// Linux: list the PipeWire application streams for screen share audio.
    LoadAudioSources,
    /// List screens and windows with thumbnails for the in-app picker.
    LoadScreenSources,
}

impl ShellState {
    /// Clicking a voice channel: open its text chat and join it (switching from another one),
    /// like `ChannelItem.handleSelect` (`startVoiceConnection` selects the channel too).
    pub fn join_voice(&mut self, guild: Option<Snowflake>, channel: Snowflake) {
        if self.channel != Some(channel) {
            self.select_channel(channel);
        }
        let v = &mut self.voice;
        if v.channel == Some((guild, channel)) {
            return;
        }
        (v.channel, v.connected, v.failed, v.screen, v.ping, v.watching) = (Some((guild, channel)), false, None, false, None, None);
        (v.camera, v.encrypted, v.server, v.reconnecting, v.focus) = (false, false, None, false, None);
        v.speaking.clear();
        v.pings.clear();
        v.videos.clear();
        v.share_pick = None;
        v.screen_device = None;
        self.intents.push(Intent::JoinVoice { guild, channel });
    }

    pub fn leave_voice(&mut self) {
        self.voice.failed = None;
        if self.voice.channel.is_some() {
            self.intents.push(Intent::LeaveVoice);
        }
    }

    /// `toggleSelfMute`: unmuting while deafened undeafens too.
    pub fn toggle_mute(&mut self) {
        let v = &mut self.voice;
        if v.self_deaf && v.self_mute {
            (v.self_mute, v.self_deaf) = (false, false);
        } else {
            v.self_mute = !v.self_mute;
        }
        v.unmute_on_undeafen = false;
        self.intents.push(Intent::VoiceSelf);
    }

    /// `toggleSelfDeaf`: deafening mutes; undeafening unmutes only if deafening muted.
    pub fn toggle_deaf(&mut self) {
        let v = &mut self.voice;
        if v.self_deaf {
            v.self_deaf = false;
            if v.unmute_on_undeafen {
                v.self_mute = false;
            }
            v.unmute_on_undeafen = false;
        } else {
            v.unmute_on_undeafen = !v.self_mute;
            (v.self_deaf, v.self_mute) = (true, true);
        }
        self.intents.push(Intent::VoiceSelf);
    }
}

/// Whether the connected channel lets us speak (`Permissions.SPEAK`); true outside voice.
pub fn can(store: &Store, v: &Voice, perm: Permissions) -> bool {
    v.channel.is_none_or(|(g, c)| g.is_none() || store.permissions(c).contains(perm))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    Green,
    Yellow,
    Orange,
    Red,
}

/// `getLatencySignalState`: filled arcs and tone from how far `ping` exceeds the median of the
/// last 30 samples. `None` while measuring.
pub fn signal(ping: Option<u32>, history: &[u32]) -> Option<(usize, Tone)> {
    let ping = ping?;
    let mut recent: Vec<u32> = history[history.len().saturating_sub(30)..].iter().copied().filter(|&l| l > 0).collect();
    recent.sort_unstable();
    let n = recent.len();
    let baseline = match n {
        0 => ping,
        _ if n % 2 == 1 => recent[n / 2],
        _ => (recent[n / 2 - 1] + recent[n / 2]).div_ceil(2),
    };
    Some(match ping.saturating_sub(baseline) {
        e if e < 70 => (4, Tone::Green),
        e if e < 160 => (3, Tone::Yellow),
        e if e < 300 => (2, Tone::Orange),
        _ => (1, Tone::Red),
    })
}

// `SignalStrengthIcon` at size 16, stroke 2: a dot at (1, 15) and four quarter arcs around it.
static SIGNAL_DOT: Icon = Icon { name: "signal-dot", view: 16.0, body: r#"<circle cx="1" cy="15" r="1.5" fill="currentColor"/>"# };
static SIGNAL_ARCS: [Icon; 4] = [
    Icon { name: "signal-arc-1", view: 16.0, body: r#"<path d="M 1 11.5 A 3.5 3.5 0 0 1 4.5 15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"# },
    Icon { name: "signal-arc-2", view: 16.0, body: r#"<path d="M 1 8 A 7 7 0 0 1 8 15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"# },
    Icon { name: "signal-arc-3", view: 16.0, body: r#"<path d="M 1 4.5 A 10.5 10.5 0 0 1 11.5 15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"# },
    Icon { name: "signal-arc-4", view: 16.0, body: r#"<path d="M 1 1 A 14 14 0 0 1 15 15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"# },
];

fn signal_icon(v: &Voice, p: &Palette) -> El {
    let state = signal(v.ping, &v.pings);
    let tone = state.map(|(_, t)| match t {
        Tone::Green => p.status_online,
        Tone::Yellow | Tone::Orange => p.status_idle,
        Tone::Red => p.status_danger,
    });
    let filled = state.map_or(0, |(n, _)| n);
    let mut glyph = el().size(16.0, 16.0).child(icon(&SIGNAL_DOT, 16.0).color(tone.unwrap_or(p.text_tertiary)).absolute(0.0, 0.0));
    for (i, arc) in SIGNAL_ARCS.iter().enumerate() {
        let c = if i < filled { tone.unwrap_or(p.text_tertiary_muted) } else { p.text_tertiary_muted };
        glyph = glyph.child(icon(arc, 16.0).color(c).absolute(0.0, 0.0));
    }
    let label = match v.ping {
        Some(ms) => crate::i18n::tf("Ping: {currentLatency}ms", &[("currentLatency", &ms.to_string())]),
        None => t("Measuring latency...").into(),
    };
    row().size(24.0, 24.0).center().role("img").label(label).child(glyph)
}

/// Height the panel adds above the user row: padding 8 + status row 32 + gap 12 + source line 16
/// + gap 12 + media buttons 32 + padding 8, and the 1px divider under it.
pub const PANEL_HEIGHT: f32 = 8.0 + 32.0 + 12.0 + 16.0 + 12.0 + 32.0 + 8.0 + 1.0;
/// The failed variant has only the status row.
pub const FAILED_HEIGHT: f32 = 8.0 + 32.0 + 8.0 + 1.0;

pub fn panel_height(v: &Voice) -> f32 {
    if v.channel.is_some() {
        PANEL_HEIGHT
    } else if v.failed.is_some() {
        FAILED_HEIGHT
    } else {
        0.0
    }
}

/// `.controlButton`: 32px, transparent, hover tint.
fn control_button<S: 'static>(i: &'static Icon, label: &str, p: &Palette, on: impl Fn(&mut S) + 'static) -> El {
    row()
        .size(32.0, 32.0)
        .center()
        .radius(dim::RADIUS_MD)
        .hover(mix(p.control_button_normal_text, Color::TRANSPARENT, 10.0))
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.to_string())
        .on_press(on)
        .child(icon(i, 20.0).color(p.control_button_normal_text).group_color(p.control_button_hover_text))
}

/// `.mediaButton`: half the row, 32 tall, a 1px hover-coloured border; green while active.
fn media_button<S: 'static>(i: &'static Icon, label: &str, active: bool, disabled: bool, p: &Palette, on: impl Fn(&mut S) + 'static) -> El {
    let (bg, fg, border) = if active {
        (mix(p.status_online, Color::TRANSPARENT, 15.0), p.status_online, mix(p.status_online, Color::TRANSPARENT, 30.0))
    } else {
        (mix(p.background_modifier_hover, Color::TRANSPARENT, 70.0), p.control_button_normal_text, p.background_modifier_hover)
    };
    let mut b = row()
        .grow(1.0)
        .w(0.0)
        .h(32.0)
        .center()
        .radius(dim::RADIUS_MD)
        .fill(bg)
        .border(1.0, border)
        .role("button")
        .label(label.to_string())
        .hover_group()
        .child(icon(i, 20.0).color(fg).group_color(if active || disabled { fg } else { p.control_button_hover_text }));
    if disabled {
        b = b.opacity(0.5);
    } else {
        let hover = if active { mix(p.status_online, Color::TRANSPARENT, 20.0) } else { mix(p.control_button_normal_text, Color::TRANSPARENT, 10.0) };
        b = b.hover(hover).cursor(rune::Cursor::Pointer).on_press(on);
    }
    b
}

/// `VoiceConnectionStatusInner` (or its failed variant), with the divider under it.
pub fn panel<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let v = &st.voice;
    let status = |label: &str, c: Color| text(label.to_string()).font(font(14.0, 600.0).lh(18.0)).color(c).truncate().grow(1.0).shrink();
    let divider = el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.floating_surface_ring_color_strong);
    let shell = col().w_full().padding(8.0).gap(12.0).pb(9.0);
    let Some((guild, channel)) = v.channel else {
        let target = v.failed?;
        let retry = move |s: &mut S| acc(s).join_voice(target.0, target.1);
        let status_row = row()
            .w_full()
            .h(32.0)
            .gap(8.0)
            .center_y()
            .child(status("Connection failed", p.status_danger).cursor(rune::Cursor::Pointer).on_press(retry))
            .child(control_button(&icons::ARROW_CLOCKWISE_BOLD, t("Try again"), p, retry))
            .child(control_button(&icons::X_BOLD, t("Dismiss"), p, move |s: &mut S| acc(s).voice.failed = None));
        return Some(shell.child(status_row).child(divider));
    };
    let (label, color) = match (v.connected, v.reconnecting) {
        (_, true) => (t("Reconnecting..."), p.status_idle),
        (true, _) => (t("In voice chat"), p.status_online),
        _ => (t("Connecting..."), p.status_idle),
    };
    let mut status_row = row().w_full().h(32.0).gap(8.0).center_y();
    if v.connected {
        status_row = status_row.child(signal_icon(v, p));
    }
    // The status opens `VoiceDetailsPopout`; the lock is `VoiceE2EEIndicator`.
    let details = status(label, color).key(super::call::DETAILS).cursor(rune::Cursor::Pointer).role("button").on_press(move |s: &mut S| acc(s).popups.toggle(super::call::DETAILS));
    status_row = status_row.child(details);
    if v.encrypted {
        status_row = status_row.child(icon(&icons::LOCK_SIMPLE_FILL, 16.0).color(p.status_online).label("Call media is encrypted"));
    }
    // `AudioProcessingButton`: opens `AudioProcessingModal`.
    status_row = status_row
        .child(control_button(&icons::WAVEFORM_BOLD, t("Audio processing"), p, move |s: &mut S| acc(s).voice.modal = Some(Modal::AudioProcessing)))
        .child(control_button(&icons::PHONE_X_FILL, t("Disconnect"), p, move |s: &mut S| acc(s).leave_voice()));
    let channel_name = store.channels.get(&channel).map(|c| super::chat::channel_title(store, c)).unwrap_or_default();
    let source = match guild.and_then(|g| store.guilds.get(&g)).and_then(|g| g.info.as_ref()) {
        Some(g) => format!("{channel_name} / {}", g.name),
        None => channel_name,
    };
    let source_row = row().w_full().h(16.0).center_y().child(
        text(source)
            .font(font(12.0, 400.0).lh(16.0))
            .color(p.text_secondary)
            .truncate()
            .shrink()
            .cursor(rune::Cursor::Pointer)
            .role("link")
            .on_press(move |s: &mut S| {
                let sh = acc(s);
                if let Some(g) = guild {
                    sh.guild = Some(g);
                }
                sh.select_channel(channel);
            }),
    );
    let can_stream = can(store, v, Permissions::STREAM);
    let screen_label = match (v.connected, can_stream, v.screen) {
        (false, _, _) => t("Waiting for connection..."),
        (_, false, _) => "You don't have permission to stream in this channel",
        (_, _, true) => "Stop sharing your screen",
        _ => t("Share your screen"),
    };
    let cam_ok = crate::camera::available();
    let camera_label = match (v.connected, can_stream && cam_ok, v.camera) {
        (false, _, _) => t("Waiting for connection..."),
        (_, false, _) if !cam_ok => "Camera needs Reflux built with GStreamer",
        (_, false, _) => "You don't have permission to use video in this channel",
        (_, _, true) => t("Turn off camera"),
        _ => t("Turn on camera"),
    };
    let media = row()
        .w_full()
        .gap(4.0)
        .child(media_button(if v.camera { &icons::VIDEO_CAMERA_FILL } else { &icons::CAMERA_SLASH_FILL }, camera_label, v.camera, !v.connected || !can_stream || !cam_ok, p, move |s: &mut S| {
            acc(s).intents.push(Intent::Voice(Op::ToggleCamera))
        }))
        .child(media_button(&icons::MONITOR_PLAY_FILL, screen_label, v.screen, !v.connected || !can_stream, p, move |s: &mut S| {
            let sh = acc(s);
            if sh.voice.screen { sh.intents.push(Intent::ToggleScreen) } else { super::call::open_screen_picker(sh) }
        }));
    Some(shell.child(status_row).child(source_row).child(media).child(divider))
}

const SPECTATORS_BTN: &str = "stream-spectators-btn";
const STREAM_MENU: &str = "stream-settings";

/// The screen share being watched, in place of the chat column (`WatchStreamOverlay`,
/// `StreamFocusHeaderInfo`, `FocusedStreamVolumeControl`): streamer and stream info, the
/// spectators popout (`StreamSpectatorsPopout`), volume/mute + stream settings, pin, fullscreen,
/// pop-out and stop. The body follows `VoiceParticipantTileScreenShareStateValue` with
/// `ScreenShareBufferingFrame`, `Watching failed :(` and `Stream ended`.
pub fn viewer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let user = st.voice.watching?;
    let guild = st.voice.channel.and_then(|(g, _)| g);
    let name = store.display_name(guild, user);
    let me = store.my_id();
    let state = match st.voice.watch_state {
        WatchState::Idle => WatchState::Buffering,
        s => s,
    };
    let watched = user != me;
    let (vol, muted) = (st.voice.settings.stream_volumes.get(&user.0).copied().unwrap_or(100.0), st.voice.settings.stream_mutes.contains(&user.0));
    let mut header = row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::SPACING_4)
        .gap(dim::SPACING_2)
        .center_y()
        .child(super::avatar(store, st, user, 20.0, None, p.background_primary, p))
        .child(text(tf("{streamerDisplayName}'s screen", &[("streamerDisplayName", &name.to_string())])).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).truncate().shrink())
        .child(el().grow(1.0));
    if let Some(info) = stream_info(&st.voice.stream, st.voice.watch_fps).filter(|_| state == WatchState::Live || state == WatchState::Buffering) {
        header = header.child(stream_pill(info, p));
    }
    if watched && !spectators_for(store, guild, user).is_empty() {
        header = header.child(spectators_button(store, st, guild, user, p, acc));
    }
    header = header
        .child(viewer_button("stream-volume", if muted { &icons::SPEAKER_SLASH_FILL } else if vol > 100.0 { &icons::SPEAKER_HIGH_FILL } else { &icons::SPEAKER_LOW_FILL }, if muted { t("Unmute stream") } else { t("Mute stream") }, muted, p, move |s: &mut S| {
            let sh = acc(s);
            if !sh.voice.settings.stream_mutes.remove(&user.0) {
                sh.voice.settings.stream_mutes.insert(user.0);
            }
            sh.intents.push(Intent::Voice(Op::Settings));
        }))
        .child(viewer_button("stream-pin", if st.voice.stream_pinned { &icons::PUSH_PIN_FILL } else { &icons::PUSH_PIN }, if st.voice.stream_pinned { t("Unpin stream") } else { t("Pin stream") }, st.voice.stream_pinned, p, move |s: &mut S| acc(s).voice.toggle_watch_pin()))
        .child(viewer_button("stream-fullscreen", if st.voice.stream_fullscreen { &icons::CORNERS_IN } else { &icons::CORNERS_OUT }, if st.voice.stream_fullscreen { t("Exit fullscreen") } else { t("Enter fullscreen") }, st.voice.stream_fullscreen, p, move |s: &mut S| {
            acc(s).voice.stream_fullscreen ^= true;
        }))
        .child(viewer_button("stream-popout", &icons::ARROW_SQUARE_OUT, t("Pop out stream"), false, p, move |s: &mut S| {
            acc(s).intents.push(Intent::Voice(Op::PopOut { user: Some(user), screen: true, pip: false }))
        }))
        .child(viewer_button("stream-settings", &icons::DOTS_THREE_BOLD, t("Stream settings"), false, p, move |s: &mut S| {
            acc(s).popups.toggle(STREAM_MENU);
        }))
        .child(viewer_button("stream-stop", &icons::X_BOLD, t("Stop watching"), false, p, move |s: &mut S| stop_watching(s, acc)));
    let last_frame = st.voice.stream.size() != (0, 0);
    let mut body = el().w_full().grow(1.0).min_h(0.0).fill(Color::BLACK);
    let content = if !watched {
        // An own stream's tile (`isOwnScreenShare && isOwnStreamPreviewPaused`).
        self_preview(p)
    } else {
        match state {
            WatchState::Failed(code) => failure_frame(code, p),
            WatchState::Ended => {
                body = body.child(el().w_full().h_full().pixels_fit(&st.voice.stream, rune::SurfaceFit::Fit).opacity(0.5));
                ended_overlay(p, acc)
            }
            WatchState::Buffering if last_frame => {
                body = body.child(el().w_full().h_full().pixels_fit(&st.voice.stream, rune::SurfaceFit::Fit).opacity(0.5));
                buffering_frame(true, p)
            }
            WatchState::Buffering => buffering_frame(false, p),
            _ => {
                body = body.child(el().w_full().h_full().pixels_fit(&st.voice.stream, rune::SurfaceFit::Fit));
                el()
            }
        }
    };
    body = body.child(content);
    Some(col().grow(1.0).min_w(0.0).h_full().fill(Color::BLACK).child(header.fill(p.background_primary)).child(body))
}

/// The viewer's menus and popouts, into the screen's overlay layer: the spectators list and the
/// stream settings menu, plus the running share's `ScreenShareEndedModal`.
pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if let Some(user) = st.voice.watching {
        let guild = st.voice.channel.and_then(|(g, _)| g);
        if st.popups.is_open(SPECTATORS_BTN) {
            layer.push(crate::ui::overlay::anchor(spectators_card(store, st, guild, user, p, acc), SPECTATORS_BTN, Placement::BottomEnd, 8.0));
        }
        stream_menu(st, p, acc).el(&st.popups, super::pops(acc), p, layer);
    }
    if st.voice.share_ended {
        let close = move |s: &mut S| acc(s).voice.share_ended = false;
        layer.push(
            crate::ui::Modal::new(t("Screen share ended"), close)
                .body(text(t("The screen share source stopped sending video, so your screen share was stopped.")).font(font(14.0, 400.0).lh(18.0)).color(p.text_primary).wrap())
                .footer(crate::ui::Button::new(t("Okay")).key("share-ended-ok").on_press(move |s: &mut S| acc(s).voice.share_ended = false).el(p))
                .el(p),
        );
    }
}

/// Drives the viewer's clock from `poll_shell`: frames make the stream live, silence fails it
/// after `WATCH_FIRST_FRAME_TIMEOUT`, and a wake keeps the deadline reachable.
pub fn watch_poll(store: &Store, st: &mut ShellState, tx: &mut rune::Tx) -> bool {
    let now = Instant::now();
    let changed = st.voice.watch_step(store, now);
    if st.voice.watching.is_some() {
        tx.wake_after(Duration::from_millis(500));
    }
    changed
}

/// Stops the viewer and its fullscreen, menus and pin.
fn stop_watching<S: 'static>(s: &mut S, acc: ShellAccess<S>) {
    let sh = acc(s);
    sh.voice.end_watch();
    sh.voice.stream_fullscreen = false;
    sh.popups.close();
    sh.intents.push(Intent::Watch(None));
}

/// `.controlButton` with a key, for a popup anchored to it; `active` tints it like a media button.
fn viewer_button<S: 'static>(key: &'static str, i: &'static Icon, label: &str, active: bool, p: &Palette, on: impl Fn(&mut S) + 'static) -> El {
    let fg = if active { p.status_online } else { p.control_button_normal_text };
    row()
        .key(key)
        .size(32.0, 32.0)
        .center()
        .radius(dim::RADIUS_MD)
        .fill(if active { mix(p.status_online, Color::TRANSPARENT, 15.0) } else { Color::TRANSPARENT })
        .hover(mix(p.control_button_normal_text, Color::TRANSPARENT, 10.0))
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.to_string())
        .on_press(on)
        .child(icon(i, 20.0).color(fg).group_color(p.control_button_hover_text))
}

/// The stream's resolution (`StreamInfoPill`): the closest of Fluxer's heights, `4K` for 2160,
/// plus the measured frame rate when one is known.
pub fn stream_info(px: &Pixels, fps: f32) -> Option<String> {
    let (_, h) = px.size();
    if h == 0 {
        return None;
    }
    let resolution = resolution_label(h);
    let fps = fps.round();
    if fps.is_finite() && fps > 0.0 {
        Some(tf("{resolution} {fps} FPS", &[("resolution", &resolution), ("fps", &format!("{fps:.0}"))]))
    } else {
        Some(resolution)
    }
}

fn resolution_label(height: u32) -> String {
    // `getClosestResolutionHeight`: starts at 480 and keeps the first of equal distances.
    let mut closest = 480u32;
    let mut smallest = height.abs_diff(closest);
    for value in [480, 240, 720, 1080, 1440, 2160] {
        let diff = height.abs_diff(value);
        if diff < smallest {
            closest = value;
            smallest = diff;
        }
    }
    match closest {
        2160 => "4K".into(),
        h => format!("{h}p"),
    }
}

/// `StreamInfoPill` + `LiveBadge`: the resolution pill and the red LIVE badge.
fn stream_pill(info: String, p: &Palette) -> El {
    row()
        .gap(4.0)
        .center_y()
        .child(
            row()
                .h(20.0)
                .px(8.0)
                .center_y()
                .radius(999.0)
                .fill(p.background_tertiary)
                .child(text(info).font(font(11.0, 500.0).lh(13.2)).color(p.text_secondary)),
        )
        .child(
            row()
                .h(20.0)
                .px(6.0)
                .center_y()
                .radius(7.0)
                .fill(rgb(220, 38, 38))
                .role("img")
                .label(t("Screen sharing"))
                .child(text("LIVE").font(font(10.0, 600.0).lh(12.0)).color(Color::WHITE)),
        )
}

/// `StreamSpectatorsPopout`'s trigger: the eye, the count and up to five watcher avatars.
fn spectators_button<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, user: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let watchers = spectators_for(store, guild, user);
    let mut r = row()
        .key(SPECTATORS_BTN)
        .h(32.0)
        .px(4.0)
        .gap(4.0)
        .center_y()
        .radius(dim::RADIUS_MD)
        .hover(mix(p.control_button_normal_text, Color::TRANSPARENT, 10.0))
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(tf("Spectators - {count}", &[("count", &watchers.len().to_string())]))
        .on_press(move |s: &mut S| acc(s).popups.toggle(SPECTATORS_BTN))
        .child(icon(&icons::EYE_FILL, 16.0).color(p.text_tertiary).group_color(p.control_button_hover_text))
        .child(text(watchers.len().to_string()).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary).group_color(p.control_button_hover_text));
    for w in watchers.iter().take(5) {
        r = r.child(super::avatar(store, st, w.user, 20.0, None, p.background_primary, p));
    }
    r
}

/// `StreamSpectatorsPopout`: the spectator count, then a row per connection (avatar, name, the
/// device icon) that opens the profile popout.
fn spectators_card<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, user: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let watchers = spectators_for(store, guild, user);
    let card = col()
        .w(220.0)
        .max_h(320.0)
        .radius(4.0)
        .fill(p.background_primary)
        .border(1.0, p.background_modifier_accent)
        .box_shadow(crate::ui::overlay::popout_shadow(p, 0.24, 0.14))
        .modal(true, move |s: &mut S| acc(s).popups.close())
        .child(
            text(tf("Spectators - {count}", &[("count", &watchers.len().to_string())]))
                .padding(12.0)
                .pb(8.0)
                .font(font(12.0, 600.0).lh(14.4))
                .color(p.text_primary_muted),
        );
    let mut list = col().w_full().padding(4.0).gap(2.0).role("group").label(t("Spectators"));
    for w in watchers {
        let key = format!("spectator-{}-{}", w.user, w.connection);
        let anchor = SPECTATORS_BTN.to_string();
        let note = store.notes.get(&w.user).cloned().unwrap_or_default();
        let row = row()
            .key(key)
            .w_full()
            .h(32.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(4.0)
            .hover(p.background_modifier_hover)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(tf("View profile for {displayName}", &[("displayName", &store.display_name(guild, w.user))]))
            .on_press(move |s: &mut S| {
                let sh = acc(s);
                sh.open_profile(w.user, guild, note.clone(), anchor.clone(), Placement::LeftStart);
            })
            .child(super::avatar(store, st, w.user, 24.0, None, p.background_primary, p))
            .child(text(store.display_name(guild, w.user)).font(font(14.0, 500.0).lh(18.0)).color(p.text_primary).truncate().grow(1.0).shrink())
            .child(icon(if w.mobile { &icons::DEVICE_MOBILE } else { &icons::DESKTOP }, 14.0).color(p.text_primary_muted));
        list = list.child(row);
    }
    card.child(crate::ui::misc::scroller(list.max_h(272.0).scroll(rune::ScrollSpec { stick_bottom: false }), false, p))
}

/// `ScreenShareBufferingFrame`: nine dots orbiting the centre over black; `corner` is the small
/// pill over a dimmed last frame. `last-frame` dims the kept frame (`ScreenShareBufferingFrame`
/// `variant="corner"`).
fn buffering_frame(corner: bool, _p: &Palette) -> El {
    let mut dots = el().w_full().h_full();
    let orbit = if corner { 8.0 } else { 24.0 };
    let dot = if corner { 3.0 } else { 6.0 };
    for i in 0..9 {
        let angle = i as f32 * std::f32::consts::TAU / 9.0;
        // shortcut: the CSS steps each dot's opacity on its own keyframes; Rune's loop fades them
        // smoothly with a stagger. Upgrade if the stepped ring matters.
        let opacity = LoopSpec::opacity(0.75075, 0.75075 * i as f64 / 9.0, 0.44, 1.0);
        dots = dots.child(
            el()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .center()
                .child(
                    el()
                        .size(orbit * 2.0, orbit * 2.0)
                        .rotate(angle)
                        .child(row().w_full().h_full().justify_center().items_start().child(el().size(dot, dot).radius(dot / 2.0).fill(Color::WHITE).looping(opacity))),
                ),
        );
    }
    let frame = if corner {
        el().pin(None, None, Some(12.0), Some(12.0)).size(40.0, 40.0).radius(999.0).fill(rgba(0, 0, 0, 0.45)).child(dots)
    } else {
        el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(rgb(0x0d, 0x0d, 0x0f)).child(dots)
    };
    frame.role("status").label(t("Stream buffering")).inert()
}

/// `ScreenShareBufferingFrame status="failed"`: "Watching failed :(" and the error code.
fn failure_frame(code: i32, _p: &Palette) -> El {
    let subtext = tf("error code {code}", &[("code", &code.to_string())]);
    el()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(rgb(0x0d, 0x0d, 0x0f))
        .center()
        .child(
            col()
                .max_w(352.0)
                .gap(8.0)
                .items_center()
                .child(text(t("Watching failed :(")).font(font(18.0, 700.0).lh(21.6)).color(Color::WHITE).align(rune::Align::Center))
                .child(text(subtext).font(font(14.0, 500.0).lh(17.5)).color(rgba(255, 255, 255, 0.7)).align(rune::Align::Center)),
        )
        .role("alert")
        .label(t("Watching failed :("))
}

/// `streamEndedOverlay`: "Stream ended" and the secondary "Stop watching" button.
fn ended_overlay<S: 'static>(p: &Palette, acc: ShellAccess<S>) -> El {
    el()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .center()
        .fill(rgba(0, 0, 0, 0.72))
        .child(
            col()
                .gap(12.0)
                .items_center()
                .child(text(t("Stream ended")).font(font(16.0, 600.0).lh(19.2)).color(Color::WHITE))
                .child(
                    crate::ui::Button::new(t("Stop watching"))
                        .variant(crate::ui::ButtonVariant::Secondary)
                        .size(crate::ui::ButtonSize::Compact)
                        .key("stream-ended-stop")
                        .on_press(move |s: &mut S| stop_watching(s, acc))
                        .el(p),
                ),
        )
        .role("alert")
        .label(t("Stream ended"))
}

/// `PREVIEW_PAUSED_TO_SAVE_RESOURCES_DESCRIPTOR` + `YOUR_STREAM_IS_STILL_LIVE_DESCRIPTOR` on our
/// own stream: Reflux doesn't render a local copy, so the preview is paused while the published
/// stream stays live for everyone else.
fn self_preview(_p: &Palette) -> El {
    el()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(rgb(0x0d, 0x0d, 0x0f))
        .center()
        .child(
            col()
                .gap(8.0)
                .items_center()
                .padding(16.0)
                .child(icon(&icons::PAUSE_FILL, 48.0).color(Color::WHITE))
                .child(text(t("Preview paused to save resources")).font(font(18.0, 600.0).lh(21.6)).color(Color::WHITE).align(rune::Align::Center))
                .child(text(t("Your stream is still live")).font(font(14.0, 400.0).lh(17.5)).color(rgba(255, 255, 255, 0.7)).align(rune::Align::Center)),
        )
        .role("status")
        .label(t("Your stream is still live"))
}

/// `FocusedStreamVolumeControl` + the viewer's actions: stream volume and mute, pin, fullscreen,
/// pop-out, picture-in-picture, retry and stop.
fn stream_menu<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Menu<S> {
    let Some(user) = st.voice.watching else { return Menu::new(STREAM_MENU) };
    let set = &st.voice.settings;
    let vol = set.stream_volumes.get(&user.0).copied().unwrap_or(100.0);
    let muted = set.stream_mutes.contains(&user.0);
    let pinned = st.voice.stream_pinned;
    let full = st.voice.stream_fullscreen;
    let failed = matches!(st.voice.watch_state, WatchState::Failed(_));
    let tweak = move |s: &mut S, f: &dyn Fn(&mut Settings)| {
        let sh = acc(s);
        f(&mut sh.voice.settings);
        sh.intents.push(Intent::Voice(Op::Settings));
    };
    let mut m = Menu::new(STREAM_MENU)
        .min_w(240.0)
        .group(t("Stream info"))
        .item(MenuItem::new("stream-info", stream_info(&st.voice.stream, st.voice.watch_fps).unwrap_or_else(|| t("Waiting for video").to_string())).icon(&icons::MONITOR_PLAY_FILL).disabled(true))
        .separator()
        .entry(crate::ui::menu_slider("stream-volume".into(), t("Stream volume"), vol, 0.0, 200.0, format!("{vol:.0}%"), p, move |s: &mut S, x| {
            tweak(s, &|st| {
                st.stream_volumes.insert(user.0, x);
            })
        }))
        .item(MenuItem::new("stream-mute", t("Mute stream")).checkbox(muted).keep_open().on_select(move |s: &mut S| {
            tweak(s, &|st| {
                if !st.stream_mutes.remove(&user.0) {
                    st.stream_mutes.insert(user.0);
                }
            })
        }))
        .separator()
        .item(MenuItem::new("stream-pin", if pinned { t("Unpin stream") } else { t("Pin stream") }).icon(if pinned { &icons::PUSH_PIN_FILL } else { &icons::PUSH_PIN }).checkbox(pinned).on_select(move |s: &mut S| acc(s).voice.toggle_watch_pin()))
        .item(MenuItem::new("stream-fullscreen", if full { t("Exit fullscreen") } else { t("Enter fullscreen") }).icon(if full { &icons::CORNERS_IN } else { &icons::CORNERS_OUT }).on_select(move |s: &mut S| {
            acc(s).voice.stream_fullscreen ^= true;
        }))
        .item(MenuItem::new("stream-popout", t("Pop out stream")).icon(&icons::ARROW_SQUARE_OUT).on_select(move |s: &mut S| {
            acc(s).intents.push(Intent::Voice(Op::PopOut { user: Some(user), screen: true, pip: false }))
        }))
        .item(MenuItem::new("stream-pip", t("Picture-in-picture")).icon(&icons::PICTURE_IN_PICTURE).on_select(move |s: &mut S| {
            acc(s).intents.push(Intent::Voice(Op::PopOut { user: Some(user), screen: true, pip: true }))
        }));
    if failed {
        m = m.item(MenuItem::new("stream-retry", t("Try again")).icon(&icons::ARROW_CLOCKWISE_BOLD).on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.retry_watch();
            sh.intents.push(Intent::Watch(Some(user)));
        }));
    }
    m.separator().item(MenuItem::new("stream-stop", t("Stop watching")).icon(&icons::X_BOLD).danger().on_select(move |s: &mut S| stop_watching(s, acc)))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_devices_group_by_card() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut known = Vec::new();
        assert!(new_devices(&v(&["PCH, Analog"]), &v(&["PCH, Analog", "PCH"]), &mut known, &[], [None, None]).is_empty(), "the first look only learns");
        assert_eq!(known, ["PCH"]);
        let (ins, outs) = (v(&["PCH, Analog", "Headset, USB Audio"]), v(&["PCH", "Headset, USB Audio", "Headset"]));
        let got = new_devices(&ins, &outs, &mut known.clone(), &[], [None, None]);
        assert_eq!(got, [NewDevice { name: "Headset".into(), input: Some("Headset, USB Audio".into()), output: Some("Headset, USB Audio".into()) }]);
        assert!(new_devices(&ins, &outs, &mut known.clone(), &v(&["Headset"]), [None, None]).is_empty(), "ignored");
        assert!(new_devices(&ins, &outs, &mut known.clone(), &[], [Some("Headset"), Some("Headset")]).is_empty(), "already selected");
        new_devices(&ins, &outs, &mut known, &[], [None, None]);
        assert!(new_devices(&ins, &outs, &mut known, &[], [None, None]).is_empty(), "seen now");
    }

    #[test]
    fn signal_follows_excess_over_median() {
        assert_eq!(signal(None, &[40]), None);
        assert_eq!(signal(Some(40), &[]), Some((4, Tone::Green)));
        let h = [40, 42, 38, 41];
        assert_eq!(signal(Some(100), &h), Some((4, Tone::Green)));
        assert_eq!(signal(Some(150), &h), Some((3, Tone::Yellow)));
        assert_eq!(signal(Some(300), &h), Some((2, Tone::Orange)));
        assert_eq!(signal(Some(400), &h), Some((1, Tone::Red)));
    }

    #[test]
    fn mute_and_deafen_follow_fluxer() {
        let mut st = ShellState::default();
        st.toggle_deaf();
        assert!(st.voice.self_deaf && st.voice.self_mute);
        st.toggle_deaf();
        assert!(!st.voice.self_deaf && !st.voice.self_mute, "undeafen restores the mic deafening muted");
        st.toggle_mute();
        st.toggle_deaf();
        st.toggle_deaf();
        assert!(st.voice.self_mute, "stays muted when it was muted before deafening");
        st.toggle_deaf();
        st.toggle_mute();
        assert!(!st.voice.self_deaf && !st.voice.self_mute, "unmuting while deafened undeafens");
        assert_eq!(st.intents.iter().filter(|i| **i == Intent::VoiceSelf).count(), 7);
    }

    #[test]
    fn trim_keeps_0_1_to_5_2_seconds() {
        let mut t = Trim { samples: std::sync::Arc::new(vec![0.0; 48_000 * 10]), ..Default::default() };
        t.set(false, 9.0);
        assert_eq!((t.start, t.end), (9.0 - Trim::MAX_SECS, 9.0), "the start follows a far end");
        t.set(true, 8.95);
        assert!((t.end - (8.95 + Trim::MIN_SECS)).abs() < 1e-4, "the end stays 0.1 s after the start");
        t.set(true, 0.0);
        assert_eq!((t.start, t.end), (0.0, Trim::MAX_SECS));
        assert_eq!(t.selection().len(), (Trim::MAX_SECS * 48_000.0) as usize);
    }

    #[test]
    fn entrance_scope_falls_back_to_global() {
        let mut lib = EntranceLibrary::default();
        lib.selections.insert("global".into(), Snowflake(1));
        lib.selections.insert("guilds".into(), Snowflake(2));
        lib.selections.insert("guild:7".into(), Snowflake(3));
        assert_eq!(lib.resolve_scope("guild:8"), Some((Snowflake(2), "guilds".into())), "inherits the community default");
        assert_eq!(lib.resolve_scope("dms"), Some((Snowflake(1), "global".into())));
        assert_eq!(lib.resolve_scope("guild:7"), Some((Snowflake(3), "guild:7".into())));
        #[cfg(feature = "voice-webrtc")]
        assert_eq!(lib.resolve(Some(Snowflake(7))), Some(Snowflake(3)));
        #[cfg(feature = "voice-webrtc")]
        assert_eq!(lib.resolve(Some(Snowflake(8))), Some(Snowflake(2)));
        #[cfg(feature = "voice-webrtc")]
        assert_eq!(lib.resolve(None), Some(Snowflake(1)), "no dms selection: global");
    }

    #[test]
    fn joining_twice_is_one_join() {
        let mut st = ShellState::default();
        st.join_voice(Some(Snowflake(1)), Snowflake(2));
        st.join_voice(Some(Snowflake(1)), Snowflake(2));
        // Clicking a voice channel also opens its text chat (`selectChannel`).
        assert_eq!(st.intents, [Intent::SelectChannel(Snowflake(2)), Intent::JoinVoice { guild: Some(Snowflake(1)), channel: Snowflake(2) }]);
        st.leave_voice();
        assert_eq!(st.intents.last(), Some(&Intent::LeaveVoice));
    }

    #[test]
    fn screen_codec_preference_maps_and_needs_its_opt_in() {
        use reflux_voice::VideoCodec;
        let limits = crate::shell::upload::Limits { higher_video_quality: true, screen_share_bitrate: 16_000_000, ..Default::default() };
        let mut set = Settings::default();
        assert_eq!(set.screen_quality(&limits, true).codec, None, "automatic");
        set.screen_codec = "vp9".into();
        assert_eq!(set.screen_quality(&limits, true).codec, Some(VideoCodec::VP9));
        set.screen_codec = "h264".into();
        assert_eq!(set.screen_quality(&limits, true).codec, Some(VideoCodec::H264));
        set.screen_codec = "av1".into();
        assert_eq!(set.screen_quality(&limits, true).codec, None, "AV1 is off until opted in");
        set.screen_av1_opt_in = true;
        assert_eq!(set.screen_quality(&limits, true).codec, Some(VideoCodec::AV1));
        set.screen_codec = "h265".into();
        assert_eq!(set.screen_quality(&limits, true).codec, None);
        set.screen_hevc_opt_in = true;
        assert_eq!(set.screen_quality(&limits, true).codec, Some(VideoCodec::H265));
    }

    #[test]
    fn screen_quality_follows_the_instance_tier() {
        let mut set = Settings { streaming_mode: "custom".into(), screen_height: Some(1440), screen_fps: 60, screen_bitrate: Some(8_000_000), ..Default::default() };
        let free = crate::shell::upload::Limits { screen_share_bitrate: 16_000_000, ..Default::default() };
        let q = set.screen_quality(&free, false);
        assert_eq!((q.max_height, q.fps, q.bitrate, q.ceiling), (Some(720), 30, None, 16_000_000), "720p/30 without higher video quality");
        set.screen_height = Some(480);
        set.screen_fps = 15;
        assert_eq!(set.screen_quality(&free, false).max_height, Some(480), "480p is part of the free tier");
        let paid = crate::shell::upload::Limits { higher_video_quality: true, screen_share_bitrate: 16_000_000, ..Default::default() };
        set.screen_height = Some(1440);
        set.screen_fps = 60;
        let q = set.screen_quality(&paid, true);
        assert_eq!((q.max_height, q.fps, q.bitrate), (Some(1440), 60, Some(8_000_000)));
        // Presets own the values until a resolution/frame rate pick switches to Custom.
        set.streaming_mode = "gaming".into();
        let q = set.screen_quality(&paid, true);
        assert_eq!((q.max_height, q.fps, q.bitrate), (Some(1440), 60, Some(8_000_000)), "gaming: 1440p60");
        set.streaming_mode = "screenshare".into();
        let q = set.screen_quality(&paid, true);
        assert_eq!((q.max_height, q.fps), (Some(1080), 30), "screen share: 1080p30");
        set.streaming_mode = "gaming".into();
        let q = set.screen_quality(&free, false);
        assert_eq!((q.max_height, q.fps), (Some(720), 30), "presets clamp to 720p30 without higher video quality");
        assert_eq!(set.screen_preset().map(|p| p.0), Some("Gaming"));
        set.streaming_mode = "custom".into();
        assert_eq!(set.screen_preset(), None);
        assert_eq!(screen_resolutions(false).iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>(), ["480", "720"]);
        assert_eq!(screen_resolutions(true).len(), 5);
        assert_eq!(screen_frame_rates(false).len(), 2);
        assert_eq!(screen_frame_rates(true).len(), 3);
        let all = screen_bitrates(16_000_000, None);
        let rates: Vec<&str> = all.iter().map(|(v, _)| v.as_str()).collect();
        assert_eq!(rates, ["auto", "2000000", "4000000", "6000000", "8000000", "10000000", "15000000", "16000000"]);
        assert_eq!(screen_bitrates(16_000_000, Some(12_000_000))[1].0, "2000000");
        assert!(screen_bitrates(16_000_000, Some(12_000_000)).iter().any(|(v, _)| v == "12000000"), "a saved custom value stays selectable");
    }

    #[test]
    fn screen_settings_round_trip_through_session_json() {
        let s = Settings { streaming_mode: "custom".into(), screen_height: Some(480), screen_fps: 60, screen_bitrate: Some(4_000_000), screen_content_hint: "detail".into(), ..Default::default() };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.screen_height, Some(480));
        assert_eq!(back.screen_fps, 60);
        assert_eq!(back.screen_bitrate, Some(4_000_000));
        assert_eq!(back.screen_content_hint, "detail");
        assert_eq!(back.streaming_mode, "custom");
    }

    #[test]
    fn camera_quality_follows_fluxer_presets() {
        let mut set = Settings { camera_quality: "low".into(), ..Default::default() };
        let q = set.camera_quality(false);
        assert_eq!((q.width, q.height, q.fps, q.bitrate), (640, 360, 20, 450_000));
        set.camera_quality = "medium".into();
        let q = set.camera_quality(false);
        assert_eq!((q.width, q.height, q.fps, q.bitrate), (1280, 720, 30, 1_700_000));
        set.camera_quality = "high".into();
        let q = set.camera_quality(false);
        assert_eq!((q.width, q.height, q.fps), (1280, 720, 30), "1080p is gated");
        let q = set.camera_quality(true);
        assert_eq!((q.width, q.height, q.fps, q.bitrate), (1920, 1080, 30, 3_000_000));
    }

    fn watch_store(live: bool) -> Store {
        let mut store = Store { me: Some(reflux_model::User { base: reflux_model::PartialUser { id: Snowflake(1), ..Default::default() }, ..Default::default() }), ..Default::default() };
        store.voice_states.insert(
            (Some(Snowflake(9)), Snowflake(2)),
            reflux_model::VoiceState { user_id: Some(Snowflake(2)), channel_id: Some(Snowflake(9)), connection_id: Some("c2".into()), self_stream: live, ..Default::default() },
        );
        store
    }

    #[test]
    fn watch_buffers_lives_stalls_fails_and_ends() {
        let store = watch_store(true);
        let mut v = Voice { channel: Some((Some(Snowflake(9)), Snowflake(9))), ..Default::default() };
        let t0 = Instant::now();
        v.begin_watch(Snowflake(2));
        assert_eq!(v.watch_state, WatchState::Buffering, "publication missing / attaching");
        v.stream_frame();
        assert!(v.watch_step(&store, t0 + Duration::from_millis(100)));
        assert_eq!(v.watch_state, WatchState::Live);
        assert!(!v.watch_step(&store, t0 + Duration::from_secs(1)), "a quiet live stream keeps its last frame");
        assert!(v.watch_step(&store, t0 + Duration::from_secs(6)));
        assert_eq!(v.watch_state, WatchState::Buffering, "5 s without frames shows the last-frame buffering state");
        assert!(v.watch_step(&store, t0 + Duration::from_secs(20)));
        assert_eq!(v.watch_state, WatchState::Failed(REPUBLISH_TIMEOUT_CODE));
        v.retry_watch();
        assert_eq!(v.watch_state, WatchState::Buffering);
        assert_eq!(v.watch_frame, 0);

        // No first frame: `Watching failed :(` after 15 s, error -2303.
        let mut v = Voice { channel: Some((Some(Snowflake(9)), Snowflake(9))), ..Default::default() };
        v.begin_watch(Snowflake(2));
        let t1 = Instant::now();
        assert!(!v.watch_step(&store, t1 + Duration::from_secs(14)));
        assert_eq!(v.watch_state, WatchState::Buffering);
        assert!(v.watch_step(&store, t1 + WATCH_FIRST_FRAME_TIMEOUT));
        assert_eq!(v.watch_state, WatchState::Failed(FIRST_FRAME_TIMEOUT_CODE));

        // The streamer stops sharing: `Stream ended`, even after a failure.
        let ended = watch_store(false);
        let mut v = Voice { channel: Some((Some(Snowflake(9)), Snowflake(9))), ..Default::default() };
        v.begin_watch(Snowflake(2));
        v.stream_frame();
        v.watch_step(&store, t0 + Duration::from_millis(100));
        assert!(v.watch_step(&ended, t0 + Duration::from_secs(1)));
        assert_eq!(v.watch_state, WatchState::Ended);
        assert_eq!(v.watching, Some(Snowflake(2)), "the tile stays up behind the ended overlay");
        v.end_watch();
        assert_eq!((v.watching, v.watch_state, v.watch_frame), (None, WatchState::Idle, 0));
    }

    #[test]
    fn watch_pin_focuses_the_call_view_and_unfocuses_it() {
        let mut v = Voice::default();
        v.begin_watch(Snowflake(2));
        assert!(v.stream_pinned, "applyStreamWatchFocus pins the stream when the watch starts");
        assert_eq!(v.focus, Some(Snowflake(2)), "the watched stream is the call view's focused tile");
        v.toggle_watch_pin();
        assert!(!v.stream_pinned);
        assert_eq!(v.focus, None);
        v.toggle_watch_pin();
        v.toggle_watch_pin();
        v.end_watch();
        assert!(!v.stream_pinned, "stopping the watch releases the pin");
        assert_eq!(v.focus, None);
    }

    #[test]
    fn stream_info_uses_fluxers_closest_resolution() {
        assert_eq!(resolution_label(180), "240p");
        assert_eq!(resolution_label(360), "480p", "equal distances keep the first (480)");
        assert_eq!(resolution_label(720), "720p");
        assert_eq!(resolution_label(1080), "1080p");
        assert_eq!(resolution_label(1400), "1440p");
        assert_eq!(resolution_label(2160), "4K");
        let px = Pixels::default();
        assert_eq!(stream_info(&px, 0.0), None);
        px.set(2, 2, vec![0; 16]);
        assert_eq!(stream_info(&px, 0.0), Some("240p".into()));
        assert_eq!(stream_info(&px, 29.6), Some("240p 30 FPS".into()));
    }

    #[test]
    fn spectators_exclude_self_streamer_and_duplicate_connections() {
        use reflux_model::PartialUser;
        let mut store = watch_store(true);
        store.users.insert(Snowflake(3), PartialUser { id: Snowflake(3), username: "zed".into(), ..Default::default() });
        store.users.insert(Snowflake(4), PartialUser { id: Snowflake(4), username: "amy".into(), ..Default::default() });
        store.users.insert(Snowflake(5), PartialUser { id: Snowflake(5), username: "nope".into(), ..Default::default() });
        let key = "9:9:c2".to_string();
        let viewer = |u: u64, conn: &str, mobile: bool, keys: Vec<String>| reflux_model::VoiceState {
            user_id: Some(Snowflake(u)),
            channel_id: Some(Snowflake(9)),
            connection_id: Some(conn.into()),
            is_mobile: mobile,
            viewer_stream_keys: keys,
            ..Default::default()
        };
        store.voice_states.insert((Some(Snowflake(9)), Snowflake(3)), viewer(3, "c3", true, vec![key.clone()]));
        // The same connection cannot watch twice; another connection of the streamer is not a viewer.
        store.voice_states.insert((Some(Snowflake(9)), Snowflake(4)), viewer(4, "c4", false, vec![key.clone()]));
        store.voice_states.insert((Some(Snowflake(9)), Snowflake(5)), viewer(5, "c5", false, vec![]));
        store.users.insert(Snowflake(6), PartialUser { id: Snowflake(6), username: "bob".into(), ..Default::default() });
        store.voice_states.insert((Some(Snowflake(9)), Snowflake(6)), viewer(6, "c3", false, vec![key.clone()]));
        store.voice_states.insert((Some(Snowflake(9)), Snowflake(2)), reflux_model::VoiceState { channel_id: Some(Snowflake(9)), connection_id: Some("c2".into()), self_stream: true, viewer_stream_keys: vec![key.clone()], ..Default::default() });
        store.voice_states.insert((Some(Snowflake(9)), Snowflake(1)), viewer(1, "c1", false, vec![key]));
        let got = spectators_for(&store, Some(Snowflake(9)), Snowflake(2));
        assert_eq!(got.len(), 2, "self, the streamer, non-watchers and one of the two c3 rows are left out");
        assert_eq!(got.iter().map(|s| s.connection.as_str()).collect::<Vec<_>>(), ["c4", "c3"], "one row per connection, sorted by name");
        assert_eq!(got[0].user, Snowflake(4), "amy comes first");
        let c3 = got.iter().find(|s| s.connection == "c3").unwrap();
        assert!(c3.user == Snowflake(3) || c3.user == Snowflake(6), "one of the c3 rows, whichever the map yields first");
        assert_eq!(c3.mobile, c3.user == Snowflake(3));
    }
}
