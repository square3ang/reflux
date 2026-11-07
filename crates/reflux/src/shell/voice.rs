//! Voice: the connection panel over the user area (`voice/components/VoiceConnectionStatus*`,
//! `voice_connection_status/LocalParticipantControls.tsx`), the signal icon
//! (`SignalStrengthIcon.tsx`, `VoiceLatencySignal.ts`), the local mute/deafen rules
//! (`LocalVoiceStateMachine.ts`) and a minimal stream viewer.

use std::collections::{HashMap, HashSet};


use reflux_model::{Permissions, Snowflake};
use reflux_store::Store;
use rune::{Color, El, Icon, Pixels, col, el, icon, row, text};

use super::{Intent, ShellAccess, ShellState};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::mix;
use crate::i18n::t;

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
    /// Users muted locally.
    // shortcut: no per-user volume slider; the Rust SDK has no remote track volume (WebRTC's
    // AudioSourceInterface::SetVolume isn't bound). Add it when webrtc-sys exposes it.
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
    /// Linux screen share audio: (PipeWire `object.serial`, application name).
    pub audio_sources: Option<Vec<(String, String)>>,
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
    /// Tallest screen share frame (`None` = source).
    pub screen_height: Option<u32>,
    pub screen_fps: u32,
    /// Bits/s, `None` = automatic.
    pub screen_bitrate: Option<u64>,
    /// `auto`, `nvenc`, `vaapi` or `software`.
    pub encoder: String,
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
            screen_height: Some(720),
            screen_fps: 30,
            screen_bitrate: None,
            encoder: "auto".into(),
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

    pub fn camera_size(&self) -> (u32, u32) {
        match self.camera_quality.as_str() {
            "low" => (854, 480),
            "high" => (1920, 1080),
            _ => (1280, 720),
        }
    }

    pub fn screen_quality(&self) -> reflux_voice::ScreenQuality {
        reflux_voice::ScreenQuality { fps: self.screen_fps, max_height: self.screen_height, bitrate: self.screen_bitrate }
    }
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
    StartCall { channel: Snowflake, video: bool },
    /// `AudioProcessingModal`: the processing profile and its custom toggles.
    AudioProcessing,
    /// `EntranceSoundTrimmerModal`.
    Trim,
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

impl EntranceLibrary {
    /// `resolveForScope`: `guild:{id}`, then `guilds` or `dms`, then `global`.
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
}

/// What the voice views ask the runtime for.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    ToggleCamera,
    /// Start or stop the camera preview capture.
    Preview(bool),
    LoadCameras,
    /// `voice.settings` changed: save and apply.
    Settings,
    LocalMute(Snowflake, bool),
    /// Check eligibility, join and ring (`CallCommands.startCall`).
    StartCall(Snowflake),
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
    /// Clicking a voice channel: join it (switching from another one).
    pub fn join_voice(&mut self, guild: Option<Snowflake>, channel: Snowflake) {
        let v = &mut self.voice;
        // Clicking the channel we're in opens its call view (`VoiceCallView`).
        v.call_view = guild.is_some();
        if v.channel == Some((guild, channel)) {
            return;
        }
        (v.channel, v.connected, v.failed, v.screen, v.ping, v.watching) = (Some((guild, channel)), false, None, false, None, None);
        (v.camera, v.encrypted, v.server, v.reconnecting, v.focus) = (false, false, None, false, None);
        v.speaking.clear();
        v.pings.clear();
        v.videos.clear();
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
    let cam_ok = crate::gst::available();
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

/// The screen share being watched, in place of the chat column (`WatchStreamOverlay`): full
/// window toggle and the spectators (`StreamSpectatorsPopout`: voice states whose
/// `viewer_stream_keys` hold this stream).
// shortcut: no stream volume; the Rust SDK has no per-track volume and stream audio isn't
// published by anyone on Linux yet.
pub fn viewer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let user = st.voice.watching?;
    let guild = st.voice.channel.and_then(|(g, _)| g);
    let name = store.display_name(guild, user);
    let header = row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::SPACING_4)
        .gap(dim::SPACING_2)
        .center_y()
        .child(icon(&icons::MONITOR_PLAY_FILL, 20.0).color(p.text_tertiary))
        .child(text(crate::i18n::tf("{streamerDisplayName}'s screen", &[("streamerDisplayName", &name.to_string())])).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).truncate().grow(1.0).shrink())
        .child(spectators(store, st, guild, user, p))
        .child(control_button(if st.voice.stream_fullscreen { &icons::CORNERS_IN } else { &icons::CORNERS_OUT }, "Full screen", p, move |s: &mut S| {
            acc(s).voice.stream_fullscreen ^= true;
        }))
        .child(control_button(&icons::X_BOLD, t("Stop watching"), p, move |s: &mut S| {
            let sh = acc(s);
            (sh.voice.watching, sh.voice.stream_fullscreen) = (None, false);
            sh.intents.push(Intent::Watch(None));
        }));
    let video = el().w_full().grow(1.0).min_h(0.0).pixels_fit(&st.voice.stream, rune::SurfaceFit::Fit);
    Some(col().grow(1.0).min_w(0.0).h_full().fill(Color::BLACK).child(header.fill(p.background_primary)).child(video))
}

/// Avatars of who else watches `user`'s stream.
fn spectators(store: &Store, st: &ShellState, guild: Option<Snowflake>, user: Snowflake, p: &Palette) -> El {
    let Some(key) = store.voice_states.get(&(guild, user)).and_then(|v| {
        let (_, c) = st.voice.channel?;
        Some(format!("{}:{c}:{}", guild.map_or("dm".to_string(), |g| g.to_string()), v.connection_id.as_deref()?))
    }) else {
        return el();
    };
    let watchers: Vec<Snowflake> = store.voice_states.iter().filter(|((g, _), v)| *g == guild && v.viewer_stream_keys.contains(&key)).map(|((_, u), _)| *u).collect();
    let mut r = row().gap(4.0).center_y().role("group").label(format!("{} watching", watchers.len()));
    r = r.child(icon(&icons::EYE_FILL, 16.0).color(p.text_tertiary)).child(text(watchers.len().to_string()).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary));
    for u in watchers.into_iter().take(5) {
        r = r.child(super::avatar(store, st, u, 20.0, None, p.background_primary, p));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(lib.resolve(Some(Snowflake(7))), Some(Snowflake(3)));
        assert_eq!(lib.resolve(Some(Snowflake(8))), Some(Snowflake(2)));
        assert_eq!(lib.resolve(None), Some(Snowflake(1)), "no dms selection: global");
    }

    #[test]
    fn joining_twice_is_one_join() {
        let mut st = ShellState::default();
        st.join_voice(Some(Snowflake(1)), Snowflake(2));
        st.join_voice(Some(Snowflake(1)), Snowflake(2));
        assert_eq!(st.intents, [Intent::JoinVoice { guild: Some(Snowflake(1)), channel: Snowflake(2) }]);
        st.leave_voice();
        assert_eq!(st.intents.last(), Some(&Intent::LeaveVoice));
    }
}
