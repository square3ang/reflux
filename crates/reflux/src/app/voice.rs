//! Voice runtime: op 4 Voice State Updates, the LiveKit call that a VOICE_SERVER_UPDATE grant
//! opens, active speakers, ping, screen share, camera, the stream and camera viewers, the mic
//! gate (voice activity / push-to-talk), DM call ringing and voice message recording.
//! `shell::voice::Voice` holds what the views show; this holds the call and devices.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reflux_model::{GatewayError, GatewayPayload, OpCode, Permissions, Snowflake, UpdateVoiceState, VoiceServerUpdate, VoiceSignal, VoiceState};
use reflux_net::Command;
#[cfg(feature = "voice-webrtc")]
use reflux_voice::livekit::prelude::{RemoteTrack, RoomEvent, TrackSource};
#[cfg(feature = "voice-webrtc")]
use reflux_voice::mesh::{self, Src};
use reflux_voice::{Call, Camera, ScreenShare, StreamAudio};
use reqwest::Method;
use serde_json::{Value, json};

use super::{spawn, update};
use crate::State;
use crate::shell::Intent;
use crate::shell::voice::{Modal, Op, Settings, VoiceMessage};
use crate::voice_input::{Gate, Mic, Mode};
use crate::i18n::t;

#[cfg(feature = "voice-webrtc")]
const PING_EVERY: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct VoiceRt {
    call: Option<Arc<Call>>,
    /// Bumped per join / leave, so a stale connect or event loop drops its results.
    generation: u64,
    /// Our connection ID from VOICE_SERVER_UPDATE (tells our voice state from other sessions').
    connection_id: Option<String>,
    pub(super) share: Option<ScreenShare>,
    /// A camera / capture device shared as a screen share (`Devices` in the picker), and the
    /// capture feeding it.
    pub(super) device_share: Option<Arc<Camera>>,
    device_capture: Option<crate::camera::CameraCapture>,
    /// A quality change from the in-call menu restarts the share once the old track is
    /// unpublished.
    pub(super) restart_screen: bool,
    /// The picker's source list is being refreshed, and when it last was.
    screen_sources_loading: bool,
    screen_sources_at: Option<std::time::Instant>,
    watch: Option<tokio::task::AbortHandle>,
    /// Draws our own share into `Voice::own_share` for its call tile.
    own_render: Option<tokio::task::AbortHandle>,
    pub(super) devices_loading: bool,
    /// The published camera track, and the slot the capture pushes frames into.
    pub(super) camera: Option<Arc<Camera>>,
    feed: Arc<Mutex<Option<Arc<Camera>>>>,
    capture: Option<crate::camera::CameraCapture>,
    /// Remote camera renderers by user.
    videos: HashMap<Snowflake, tokio::task::AbortHandle>,
    /// The second microphone stream (meter, voice activity, voice messages), and the
    /// voice-activity gate it drives on the audio thread: (mode, state, open).
    mic: Option<Mic>,
    /// `useMicTest`: its own processed mic and speakers, the mic playing back 0.9 s late.
    /// Keyed by what it was opened with, so a settings or device change restarts it.
    mic_test: Option<(reflux_voice::audio::Engine, MicTestKey)>,
    vad: Arc<Mutex<(Option<Mode>, Gate, bool)>>,
    ptt: Gate,
    /// Whether the mic track is enabled: push-to-talk switches it; voice activity leaves it on
    /// for the gate in the mic path (`Processing::gate`).
    track_open: bool,
    /// A DM call we started: ring these once connected; stop ringing them when leaving.
    pending_ring: Option<(Snowflake, Vec<Snowflake>)>,
    rang: Option<(Snowflake, Vec<Snowflake>)>,
    /// The recorded voice message (samples, rate).
    recorded: Option<(Vec<f32>, u32)>,
    /// A join the confirm modal let through.
    confirmed: Option<Snowflake>,
    /// Remote audio tracks playing into the mixer, by track SID.
    audio: HashMap<String, tokio::task::AbortHandle>,
    /// Screen share audio (Linux: a PipeWire application stream; Windows/macOS: the system mix)
    /// and its capture.
    stream_audio: Option<(Arc<StreamAudio>, crate::audio_share::Capture)>,
    /// `VoiceP2pConsent`: the channel whose peer-to-peer call the user agreed to.
    p2p_agreed: Option<Snowflake>,
    /// The `/experiments` poll loop of this session.
    pub(super) rollout: Option<tokio::task::AbortHandle>,
    /// `useScreensharePreviewUploader`'s uploaded still.
    pub(super) preview_upload: super::share_preview::Upload,
    /// The latest frame of our share, kept only when `want_own_frame` asks for one.
    pub(super) own_frame: Arc<Mutex<Option<super::share_preview::Still>>>,
    pub(super) want_own_frame: Arc<std::sync::atomic::AtomicBool>,
    /// `syncConversionWatchdog`: the armed timer's token, while our state says the server took the
    /// call off P2P and no standard grant has arrived yet.
    conversion: Option<u64>,
    conversion_seq: u64,
}

/// Our voice connection ID (live tests compare the gateway's echo with it).
pub(super) fn connection_id(s: &State) -> Option<&str> {
    s.app.voice.connection_id.as_deref()
}

pub(super) fn call(s: &State) -> Option<Arc<Call>> {
    s.app.voice.call.clone()
}

pub(super) fn run(s: &mut State, i: Intent) {
    match i {
        Intent::JoinVoice { guild, channel } => join(s, guild, channel),
        Intent::LeaveVoice => {
            s.app.voice.p2p_agreed = None;
            leave(s, true);
        }
        Intent::VoiceSelf => {
            send_state(s);
            apply_self(s);
        }
        Intent::ToggleScreen => toggle_screen(s),
        Intent::Watch(user) => watch(s, user),
        Intent::LoadDevices => spawn(async move {
            if let Ok(d) = tokio::task::spawn_blocking(reflux_voice::devices).await {
                update(move |s| s.app.shell.voice.devices = Some(d));
            }
        }),
        Intent::VoiceDevices => {
            let v = &s.app.shell.voice;
            (s.app.saved.voice_input, s.app.saved.voice_output) = (v.input.clone(), v.output.clone());
            crate::session::save(&s.app.saved);
            s.app.voice.mic = None; // the next tick reopens it on the new input
            apply_devices(s);
        }
        Intent::Voice(op) => run_op(s, op),
        _ => {}
    }
}

fn run_op(s: &mut State, op: Op) {
    match op {
        Op::IgnoreDevice(card) => {
            if !s.app.saved.ignored_devices.contains(&card) {
                s.app.saved.ignored_devices.push(card);
                crate::session::save(&s.app.saved);
            }
        }
        Op::ToggleCamera => toggle_camera(s),
        Op::Preview(on) => {
            s.app.shell.voice.previewing = on;
            if on { ensure_capture(s) } else { release_capture(s) }
        }
        Op::LoadCameras => spawn(async move {
            let names = tokio::task::spawn_blocking(crate::camera::cameras).await.unwrap_or_default();
            update(move |s| s.app.shell.voice.cameras = Some(names));
        }),
        Op::Settings => {
            let before = s.app.saved.voice_settings.clone();
            s.app.saved.voice_settings = s.app.shell.voice.settings.clone();
            crate::session::save(&s.app.saved);
            apply_encoder(&s.app.saved.voice_settings);
            apply_self(s);
            // `StreamSettingsMenuContent` with applyToLiveStream: a running share picks up
            // resolution / frame rate / bitrate / audio / encoder changes at once.
            if (s.app.voice.share.is_some() || s.app.voice.device_share.is_some()) && screen_settings_changed(&before, &s.app.saved.voice_settings) {
                s.app.voice.restart_screen = true;
                s.app.shell.intents.push(crate::shell::Intent::ToggleScreen);
            }
        }
        Op::ChangeScreen => {
            s.app.voice.restart_screen = true;
            s.app.shell.intents.push(crate::shell::Intent::ToggleScreen);
        }
        Op::LocalMute(user, muted) => {
            let v = &mut s.app.shell.voice;
            if muted { v.local_mutes.insert(user) } else { v.local_mutes.remove(&user) };
            apply_self(s);
        }
        Op::StartCall(channel) => start_call(s, channel),
        Op::P2pConsent { guild, channel, p2p, start } => {
            s.app.voice.p2p_agreed = p2p.then_some(channel);
            if start { start_call(s, channel) } else { s.app.shell.join_voice(guild, channel) }
        }
        Op::SwitchToStandard => {
            if let Some(ch) = s.app.shell.voice.channel {
                send(s, UpdateVoiceState { p2p: Some(false), ..payload(s, Some(ch)) });
            }
        }
        Op::AcceptCall(channel) => {
            s.app.shell.voice.dismissed_calls.remove(&channel);
            s.app.shell.join_voice(None, channel);
        }
        Op::DeclineCall(channel) => {
            s.app.shell.voice.dismissed_calls.insert(channel);
            if s.app.shell.voice.channel.is_some_and(|(_, c)| c == channel) {
                leave(s, true);
            }
            api(s, Method::POST, format!("/channels/{channel}/call/stop-ringing"), json!({}));
        }
        Op::Ring(channel, user, ring) => {
            let path = if ring { "ring" } else { "stop-ringing" };
            api(s, Method::POST, format!("/channels/{channel}/call/{path}"), json!({ "recipients": [user] }));
        }
        Op::MicTest(on) => s.app.shell.voice.mic_test = on,
        Op::RecordStart(channel) => {
            crate::desktop::sound::stop_preview();
            s.app.voice.recorded = None;
            s.app.shell.voice.voice_message = Some(VoiceMessage { channel, recording: true, ..Default::default() });
            ensure_mic(s);
            match &s.app.voice.mic {
                Some(m) => m.start_recording(),
                None => {
                    if let Some(vm) = &mut s.app.shell.voice.voice_message {
                        (vm.recording, vm.error) = (false, Some(t("No microphone found").into()));
                    }
                }
            }
        }
        Op::RecordStop => {
            let rec = s.app.voice.mic.as_ref().and_then(|m| m.stop_recording());
            if let (Some((samples, rate)), Some(vm)) = (rec, &mut s.app.shell.voice.voice_message) {
                let (_, b64) = crate::voice_input::waveform(&samples, rate);
                vm.bars = crate::voice_input::decode_waveform(&b64);
                vm.secs = samples.len() as f32 / rate as f32;
                (vm.start, vm.end) = (0.0, vm.secs);
                vm.recording = false;
                s.app.voice.recorded = Some((samples, rate));
            }
        }
        Op::RecordCancel => {
            crate::desktop::sound::stop_preview();
            if let Some(m) = &s.app.voice.mic {
                m.stop_recording();
            }
            s.app.voice.recorded = None;
            s.app.shell.voice.voice_message = None;
        }
        Op::RecordSend => send_voice_message(s),
        Op::RecordPlay => {
            crate::desktop::sound::stop_preview();
            if let (Some((samples, rate)), Some(vm)) = (&s.app.voice.recorded, &mut s.app.shell.voice.voice_message) {
                vm.playing = match vm.playhead() {
                    Some(_) => None,
                    None => {
                        crate::desktop::sound::preview(crate::voice_input::wav(&samples[vm.range(samples.len(), *rate)], *rate));
                        Some(std::time::Instant::now())
                    }
                };
            }
        }
        Op::PushToTalk(held) => s.app.shell.voice.ptt_held = held,
        Op::ConfirmJoin(guild, channel) => confirm_join(s, guild, channel),
        Op::LoadEntrance => load_entrance(s),
        Op::SelectEntrance(scope, sound) => {
            api_then(s, Method::PUT, "/users/@me/entrance-sound-selections".into(), json!({ "scope_id": scope, "sound_id": sound }), load_entrance);
        }
        Op::DeleteEntrance(id) => api_then(s, Method::DELETE, format!("/users/@me/entrance-sounds/{id}"), Value::Null, load_entrance),
        Op::PreviewEntrance(id) => {
            let url = s.app.shell.voice.entrance.as_ref().and_then(|l| l.sounds.iter().find(|x| x.id == id)).map(|x| x.url.clone());
            if let Some(url) = url {
                spawn(async move {
                    if let Some(b) = fetch(&url).await {
                        crate::desktop::sound::play_bytes(b, 1.0);
                    }
                });
            }
        }
        Op::PreviewTrim => {
            if let Some(tr) = &s.app.shell.voice.trim {
                crate::desktop::sound::play_bytes(crate::voice_input::wav(tr.selection(), reflux_voice::audio::RATE), 1.0);
            }
        }
        Op::UploadTrim => upload_trim(s),
        Op::PopOut { user, screen, pip } => crate::popout::open(s, user, screen, pip),
        // `pw-dump` on Linux runs off the UI thread.
        Op::LoadAudioSources => spawn(async move {
            let apps = tokio::task::spawn_blocking(crate::audio_share::sources).await.unwrap_or_default();
            update(move |s| s.app.shell.voice.audio_sources = Some(apps));
        }),
        Op::LoadScreenSources => {
            if reflux_voice::can_list_sources() && !rune::is_headless() && !s.app.voice.screen_sources_loading {
                s.app.voice.screen_sources_loading = true;
                spawn(async move {
                    let mut list = tokio::task::spawn_blocking(|| reflux_voice::screen_sources(320)).await.unwrap_or_default();
                    // `isOwnWindow`: our own windows go last.
                    list.sort_by_key(|src| src.window && crate::desktop::window::is_own(src.id));
                    update(move |s| {
                        s.app.voice.screen_sources_loading = false;
                        s.app.voice.screen_sources_at = Some(std::time::Instant::now());
                        s.app.shell.voice.screen_sources = Some(
                            list.into_iter()
                                .map(|mut src| {
                                    let px = rune::Pixels::default();
                                    if let Some((w, h, rgba)) = src.thumbnail.take() {
                                        px.set(w, h, rgba);
                                    }
                                    (src, px)
                                })
                                .collect(),
                        )
                    });
                });
            }
        }
    }
}

/// A REST call, then `then` on the UI thread when it succeeds.
fn api_then(s: &State, method: Method, path: String, body: Value, then: fn(&mut State)) {
    let Some(inst) = s.app.instance.clone() else { return };
    spawn(async move {
        let body = (!body.is_null()).then_some(body);
        match inst.rest.request::<Value, Value>(method, &path, body.as_ref()).await {
            Ok(_) => update(then),
            Err(e) => eprintln!("reflux: {path}: {e}"),
        }
    });
}

/// GETs a public URL (entrance sound clips live on the CDN, no auth).
async fn fetch(url: &str) -> Option<Vec<u8>> {
    let r = async { reflux_net::http_client().get(url).send().await?.error_for_status()?.bytes().await }.await;
    r.map(|b| b.to_vec()).map_err(|e| eprintln!("reflux: {url}: {e}")).ok()
}

// ---- Entrance sounds ---------------------------------------------------------------------------

fn load_entrance(s: &mut State) {
    let Some(inst) = s.app.instance.clone() else { return };
    spawn(async move {
        match inst.rest.get::<Value>("/users/@me/entrance-sounds").await {
            Ok(v) => {
                let lib = parse_library(&v);
                update(move |s| s.app.shell.voice.entrance = Some(lib));
            }
            Err(e) => eprintln!("reflux: entrance sounds: {e}"),
        }
    });
}

fn parse_library(v: &Value) -> crate::shell::voice::EntranceLibrary {
    let sounds = serde_json::from_value(v["sounds"].clone()).unwrap_or_default();
    let selections = v["selections"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| Some((x["scope_id"].as_str()?.to_string(), x["sound_id"].as_str()?.parse().ok().map(Snowflake)?)))
        .collect();
    crate::shell::voice::EntranceLibrary { sounds, selections }
}

/// `EntranceSoundTrimmerModal` → `POST /users/@me/entrance-sounds` as WAV, then select it for
/// every scope (`global`) when nothing is selected yet.
fn upload_trim(s: &mut State) {
    let Some(inst) = s.app.instance.clone() else { return };
    let Some(tr) = &mut s.app.shell.voice.trim else { return };
    let bytes = crate::voice_input::wav(tr.selection(), reflux_voice::audio::RATE);
    // `ENTRANCE_SOUND_MAX_BYTES`.
    if bytes.len() > 1_048_576 {
        tr.error = Some(t("Trimmed clip is over the size limit. Try a shorter selection.").into());
        return;
    }
    (tr.uploading, tr.error) = (true, None);
    let name: String = tr.name.chars().take(32).collect();
    let first = s.app.shell.voice.entrance.as_ref().is_none_or(|l| l.selections.is_empty());
    let body = json!({ "name": if name.trim().is_empty() { "Entrance sound".into() } else { name }, "audio": crate::guild::base64(&bytes) });
    spawn(async move {
        let r = inst.rest.request::<Value, Value>(Method::POST, "/users/@me/entrance-sounds", Some(&body)).await;
        if let (Ok(v), true) = (&r, first)
            && let Some(id) = v["id"].as_str()
        {
            let sel = json!({ "scope_id": "global", "sound_id": id });
            let _ = inst.rest.request::<Value, Value>(Method::PUT, "/users/@me/entrance-sound-selections", Some(&sel)).await;
        }
        update(move |s| match r {
            Ok(_) => {
                s.app.shell.voice.trim = None;
                s.app.shell.voice.modal = None;
                load_entrance(s);
            }
            Err(e) => {
                if let Some(tr) = &mut s.app.shell.voice.trim {
                    (tr.uploading, tr.error) = (false, Some(e.to_string()));
                }
            }
        });
    });
}

/// After connecting: `POST /voice/channels/{id}/entrance-sound` with the clip that fits.
/// `restoreLocalMedia`: the camera, then the screen share (its picker; nothing to reuse after a restart).
#[cfg(feature = "voice-webrtc")]
fn restore_media(s: &mut State) {
    let (video, stream) = std::mem::take(&mut s.app.shell.voice.restore_media);
    if video && !s.app.shell.voice.camera {
        toggle_camera(s);
    }
    if stream && s.app.voice.share.is_none() {
        crate::shell::call::open_screen_picker(&mut s.app.shell);
    }
}

#[cfg(feature = "voice-webrtc")]
fn play_own_entrance(s: &State) {
    let (Some(inst), Some((guild, channel))) = (s.app.instance.clone(), s.app.shell.voice.channel) else { return };
    spawn(async move {
        let Ok(v) = inst.rest.get::<Value>("/users/@me/entrance-sounds").await else { return };
        let lib = parse_library(&v);
        if let Some(sound) = lib.resolve(guild) {
            let path = format!("/voice/channels/{channel}/entrance-sound");
            if let Err(e) = inst.rest.request::<Value, Value>(Method::POST, &path, Some(&json!({ "sound_id": sound }))).await {
                eprintln!("reflux: {path}: {e}");
            }
        }
    });
}

/// ENTRANCE_SOUND_PLAY: someone in our channel joined with a clip; play it through the mix (so
/// deafen, output device and their entrance volume apply).
pub(super) fn on_entrance(s: &State, e: &reflux_model::EntranceSoundPlay) {
    let v = &s.app.shell.voice;
    let here = v.channel.is_some_and(|(_, c)| c == e.channel_id);
    let Some(mixer) = s.app.voice.call.as_ref().and_then(|c| c.mixer()) else { return };
    if !here || e.user_id == s.app.store.my_id() || v.settings.entrance_mutes.contains(&e.user_id.0) {
        return;
    }
    let (url, user) = (e.url.clone(), e.user_id.0);
    spawn(async move {
        let Some(bytes) = fetch(&url).await else { return };
        let Ok(Some(pcm)) = tokio::task::spawn_blocking(move || crate::voice_input::decode(bytes)).await else { return };
        let src = reflux_voice::Source { kind: reflux_voice::Kind::Entrance, user, conn: String::new() };
        mixer.lock().unwrap().push(src, &pcm);
    });
}

/// A REST call whose effects come back over the gateway; failures are logged.
fn api(s: &State, method: Method, path: String, body: Value) {
    let Some(inst) = s.app.instance.clone() else { return };
    spawn(async move {
        if let Err(e) = inst.rest.request::<Value, Value>(method, &path, Some(&body)).await {
            eprintln!("reflux: {path}: {e}");
        }
    });
}

/// `CallCommands.startCall` after `checkCallEligibility`: join, then ring once connected.
fn start_call(s: &mut State, channel: Snowflake) {
    let Some(inst) = s.app.instance.clone() else { return };
    let me = s.app.store.my_id();
    let recipients: Vec<Snowflake> = s.app.store.channels.get(&channel).map(|c| c.recipients.iter().map(|u| u.id).filter(|u| *u != me).collect()).unwrap_or_default();
    spawn(async move {
        let r = inst.rest.get::<Value>(&format!("/channels/{channel}/call")).await;
        update(move |s| {
            let ringable = match r {
                Ok(v) => v["ringable"].as_bool().unwrap_or(false),
                Err(e) => return eprintln!("reflux: call eligibility: {e}"),
            };
            if !ringable {
                s.app.shell.voice.modal = Some(Modal::NotRingable);
                return;
            }
            s.app.voice.pending_ring = Some((channel, recipients));
            s.app.shell.join_voice(None, channel);
        });
    });
}

/// The op 4 payload for the shell's current voice state (`channel` `None` = leave).
fn payload(s: &State, channel: Option<(Option<Snowflake>, Snowflake)>) -> UpdateVoiceState {
    let v = &s.app.shell.voice;
    // `viewer_stream_keys`: `{guild or "dm"}:{channel}:{connection}` of the stream we watch.
    let watching = v.watching.zip(channel).and_then(|(u, (g, c))| {
        let state = s.app.store.voice_states.get(&(g, u))?;
        Some(format!("{}:{c}:{}", g.map_or("dm".to_string(), |g| g.to_string()), state.connection_id.as_deref()?))
    });
    UpdateVoiceState {
        guild_id: channel.and_then(|(g, _)| g),
        channel_id: channel.map(|(_, c)| c),
        connection_id: s.app.voice.connection_id.clone().filter(|_| channel.is_some()),
        self_mute: v.self_mute,
        self_deaf: v.self_deaf,
        self_video: v.camera && s.app.voice.camera.is_some(),
        // A camera/device share publishes as a screen share too (`Devices` in the picker), so
        // the server and other clients see us streaming.
        self_stream: v.screen && (s.app.voice.share.is_some() || s.app.voice.device_share.is_some()),
        viewer_stream_keys: channel.map(|_| watching.map(|k| vec![k])),
        // Only `true` agrees; leaving it out never switches a call.
        p2p: channel.filter(|(_, c)| s.app.voice.p2p_agreed == Some(*c)).map(|_| true),
        ..Default::default()
    }
}

fn send(s: &State, d: UpdateVoiceState) {
    if let Some(gw) = &s.app.gateway {
        let _ = gw.send(Command::Send(GatewayPayload::command(OpCode::VOICE_STATE_UPDATE, d)));
    }
}

fn send_state(s: &State) {
    if let Some(ch) = s.app.shell.voice.channel {
        send(s, payload(s, Some(ch)));
    }
}

fn join(s: &mut State, guild: Option<Snowflake>, channel: Snowflake) {
    if guild.is_some() && !s.app.store.permissions(channel).contains(Permissions::CONNECT) {
        s.app.shell.voice.channel = None;
        return;
    }
    // `VoiceConnectionConfirmModal`: already connected to *this channel* from another device
    // (Fluxer allows the account in another channel from another device; that is "Just join").
    let me = s.app.store.my_id();
    let elsewhere = s.app.voice.call.is_none()
        && s.app.store.voice_states.iter().any(|((g, u), v)| *g == guild && *u == me && v.channel_id == Some(channel) && v.connection_id != s.app.voice.connection_id);
    if elsewhere && s.app.voice.confirmed != Some(channel) {
        let v = &mut s.app.shell.voice;
        v.channel = None;
        v.modal = Some(Modal::JoinConfirm { guild, channel });
        return;
    }
    s.app.voice.confirmed = None;
    // `checkP2pConsent`: a P2P call (or an empty "P2P calls only" channel) needs agreement.
    let others = s.app.store.voice_states.iter().any(|((g, u), v)| *g == guild && *u != me && v.channel_id == Some(channel));
    let pinned = guild.is_some() && !others && s.app.shell.voice.p2p_rollout.0 && s.app.store.channels.get(&channel).is_some_and(|c| c.rtc_p2p);
    if (pinned || crate::shell::voice::is_p2p(&s.app.store, guild, channel)) && !p2p_consent(s, guild, channel, pinned) {
        return;
    }
    drop_call(s);
    s.app.voice.connection_id = None;
    send(s, payload(s, Some((guild, channel))));
}

/// `requestP2pConsent`: true when already agreed (or "Join P2P calls without asking"); else
/// asks and abandons this join (the dialog's answer joins again).
fn p2p_consent(s: &mut State, guild: Option<Snowflake>, channel: Snowflake, start: bool) -> bool {
    if s.app.voice.p2p_agreed == Some(channel) {
        return true;
    }
    if crate::look::with(|l| l.skip_p2p_join) {
        s.app.voice.p2p_agreed = Some(channel);
        return true;
    }
    let v = &mut s.app.shell.voice;
    v.channel = None;
    v.modal = Some(Modal::P2pConsent { guild, channel, start, allow_standard: false, video: false, always: false });
    false
}

/// The confirm modal's "Join anyway".
fn confirm_join(s: &mut State, guild: Option<Snowflake>, channel: Snowflake) {
    s.app.voice.confirmed = Some(channel);
    s.app.shell.join_voice(guild, channel);
}

/// Hangs up locally; `tell_gateway` also sends op 4 with a null channel.
pub(super) fn leave(s: &mut State, tell_gateway: bool) {
    let Some((guild, _)) = s.app.shell.voice.channel else { return };
    if tell_gateway {
        send(s, UpdateVoiceState { guild_id: guild, ..payload(s, None) });
    }
    // `leaveCall`: stop ringing whoever we rang and nobody answered.
    if let Some((channel, users)) = s.app.voice.rang.take() {
        let ringing = s.app.store.calls.get(&channel).map(|c| c.ringing.clone()).unwrap_or_default();
        let left: Vec<_> = users.into_iter().filter(|u| ringing.contains(u)).collect();
        if !left.is_empty() {
            api(s, Method::POST, format!("/channels/{channel}/call/stop-ringing"), json!({ "recipients": left }));
        }
    }
    s.app.voice.pending_ring = None;
    drop_call(s);
    s.app.voice.connection_id = None;
    let v = &mut s.app.shell.voice;
    v.p2p_failed.clear();
    if matches!(v.modal, Some(Modal::P2pFailed { .. })) {
        v.modal = None;
    }
    (v.channel, v.connected, v.screen, v.ping, v.watching, v.camera, v.call_view) = (None, false, false, None, None, false, false);
    (v.encrypted, v.server, v.reconnecting, v.focus, v.transmitting) = (false, None, false, None, false);
    v.speaking.clear();
    v.pings.clear();
    v.videos.clear();
    release_capture(s);
    crate::popout::close();
}

fn drop_call(s: &mut State) {
    let rt = &mut s.app.voice;
    rt.generation += 1;
    rt.share = None;
    if let Some(r) = rt.own_render.take() {
        r.abort();
    }
    rt.device_share = None;
    rt.device_capture = None;
    rt.restart_screen = false;
    rt.camera = None;
    *rt.feed.lock().unwrap() = None;
    if let Some(w) = rt.watch.take() {
        w.abort();
    }
    for h in rt.videos.drain().map(|(_, h)| h).chain(rt.audio.drain().map(|(_, h)| h)) {
        h.abort();
    }
    rt.stream_audio = None;
    if let Some(call) = rt.call.take() {
        spawn(async move {
            let _ = call.leave().await;
        });
    }
}

#[cfg(feature = "voice-webrtc")]
pub(super) fn on_server_update(s: &mut State, v: &VoiceServerUpdate) {
    if s.app.shell.voice.channel != Some((v.guild_id, v.channel_id)) {
        return;
    }
    if v.p2p {
        return start_mesh(s, v);
    }
    // A LiveKit grant (also when a P2P call switched to LiveKit) ends any P2P agreement and
    // satisfies the conversion watchdog.
    s.app.voice.p2p_agreed = None;
    s.app.voice.conversion = None;
    let (Some(endpoint), Some(token)) = (v.endpoint.clone(), v.token.clone()) else {
        eprintln!("reflux: voice: a grant without a LiveKit endpoint or token");
        return fail(s);
    };
    drop_call(s);
    s.app.voice.connection_id = Some(v.connection_id.clone());
    let generation = s.app.voice.generation;
    let endpoint = if endpoint.contains("://") { endpoint } else { format!("wss://{endpoint}") };
    let host = endpoint.split("://").nth(1).unwrap_or(&endpoint).split('/').next().unwrap_or_default().to_string();
    let speak = crate::shell::voice::can(&s.app.store, &s.app.shell.voice, Permissions::SPEAK);
    let bitrate = crate::guild::channel::mic_bitrate(&s.app.store, v.channel_id);
    let key = v.e2ee_key.clone();
    spawn(async move {
        let (call, mut events) = match Call::connect(&endpoint, &token, true, key.as_deref()).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("reflux: voice connect: {e}");
                return update(move |s| {
                    if s.app.voice.generation == generation {
                        fail(s);
                    }
                });
            }
        };
        let call = Arc::new(call);
        if speak && let Err(e) = call.publish_mic(true, bitrate).await {
            eprintln!("reflux: publish mic: {e}");
        }
        let c = call.clone();
        update(move |s| {
            if s.app.voice.generation != generation {
                return spawn(async move {
                    let _ = c.leave().await;
                });
            }
            let (_, room) = c.server_info();
            let v = &mut s.app.shell.voice;
            (v.connected, v.encrypted, v.server) = (true, c.encrypted, Some((host, room)));
            c.set_mic_gate(false); // the next tick opens it per voice activity / push-to-talk
            s.app.voice.track_open = false;
            s.app.shell.voice.transmitting = false;
            s.app.voice.call = Some(c);
            apply_self(s);
            apply_devices(s);
            play_own_entrance(s);
            restore_media(s);
            if let Some((channel, users)) = s.app.voice.pending_ring.take().filter(|(ch, _)| s.app.shell.voice.channel.is_some_and(|(_, c)| c == *ch)) {
                api(s, Method::POST, format!("/channels/{channel}/call/ring"), json!({ "recipients": users }));
                s.app.voice.rang = Some((channel, users));
            }
        });
        let weak = Arc::downgrade(&call);
        spawn(ping_loop(weak.clone(), generation));
        spawn(self_speaking(weak, generation));
        while let Some(ev) = events.recv().await {
            // A share published or auto-subscribed while not watched: unsubscribe it.
            if let RoomEvent::TrackPublished { publication, .. } | RoomEvent::TrackSubscribed { publication, .. } = &ev
                && matches!(publication.source(), TrackSource::Screenshare | TrackSource::ScreenshareAudio)
            {
                update(move |s| {
                    if s.app.voice.generation == generation
                        && let Some(call) = &s.app.voice.call
                    {
                        call.sync_streams(s.app.shell.voice.watching.map(|u| u.0), None);
                    }
                });
            }
            match ev {
                RoomEvent::ActiveSpeakersChanged { speakers } => {
                    // Others only: our own ring is the local gate's (`self_speaking`), so a server
                    // update can't flicker it (Fluxer's `VoiceSpeakingDetection` does the same).
                    let mut ids: std::collections::HashSet<Snowflake> = speakers
                        .iter()
                        .filter_map(|p| reflux_voice::user_of(&p.identity().0))
                        .map(Snowflake)
                        .collect();
                    update(move |s| {
                        if s.app.voice.generation == generation {
                            let me = s.app.store.my_id();
                            ids.remove(&me);
                            if s.app.shell.voice.speaking.contains(&me) {
                                ids.insert(me);
                            }
                            s.app.shell.voice.speaking = ids;
                        }
                    });
                }
                RoomEvent::TrackSubscribed { track, publication, participant } => {
                    let user = reflux_voice::user_of(&participant.identity().0).map(Snowflake);
                    match (track, user, publication.source()) {
                        (RemoteTrack::Video(t), Some(user), TrackSource::Camera) => update(move |s| {
                            if s.app.voice.generation == generation {
                                render_camera(s, user, t.rtc_track());
                            }
                        }),
                        // The watched stream's publication arrived after the click: start rendering.
                        (RemoteTrack::Video(_), Some(user), TrackSource::Screenshare) => update(move |s| {
                            if s.app.voice.generation != generation {
                                return;
                            }
                            if s.app.shell.voice.watching == Some(user) {
                                watch(s, Some(user));
                            }
                        }),
                        (RemoteTrack::Audio(t), _, source) => {
                            if let Some(play) = call.play(t, &participant, source)
                                && let Some(b) = super::BRIDGE.get()
                            {
                                let h = b.rt.spawn(play).abort_handle();
                                let sid = publication.sid().to_string();
                                update(move |s| {
                                    if s.app.voice.generation == generation { s.app.voice.audio.insert(sid, h); } else { h.abort() }
                                });
                            }
                        }
                        _ => {}
                    }
                }
                RoomEvent::TrackUnsubscribed { publication, .. } if publication.kind() == reflux_voice::livekit::prelude::TrackKind::Audio => {
                    let sid = publication.sid().to_string();
                    update(move |s| {
                        if let Some(h) = s.app.voice.audio.remove(&sid) {
                            h.abort();
                        }
                    });
                }
                RoomEvent::TrackUnsubscribed { publication, participant, .. } if publication.source() == TrackSource::Camera => {
                    if let Some(user) = reflux_voice::user_of(&participant.identity().0).map(Snowflake) {
                        update(move |s| {
                            if let Some(h) = s.app.voice.videos.remove(&user) {
                                h.abort();
                            }
                            s.app.shell.voice.videos.remove(&user);
                        });
                    }
                }
                // `VoiceReconnectionManager`: LiveKit resumes by itself; show it meanwhile.
                RoomEvent::Reconnecting | RoomEvent::Reconnected => {
                    let on = matches!(ev, RoomEvent::Reconnecting);
                    update(move |s| {
                        if s.app.voice.generation == generation {
                            s.app.shell.voice.reconnecting = on;
                        }
                    });
                }
                RoomEvent::Disconnected { reason } => {
                    eprintln!("reflux: voice disconnected: {reason:?}");
                    return update(move |s| {
                        if s.app.voice.generation == generation {
                            fail(s);
                        }
                    });
                }
                _ => {}
            }
        }
    });
}

/// No-WebRTC build: there is no room to connect to; show "Connection failed" like a refused join.
#[cfg(not(feature = "voice-webrtc"))]
pub(super) fn on_server_update(s: &mut State, _v: &VoiceServerUpdate) {
    eprintln!("reflux: voice: this build has no WebRTC support; rebuild with the default features for calls");
    fail(s);
}

// ---- Peer-to-peer calls -----------------------------------------------------------------------

/// A peer-to-peer grant: our audio path and a [`mesh::Mesh`] instead of a LiveKit room. Peers
/// come from the voice states ([`sync_roster`]) and talk over op 13.
#[cfg(feature = "voice-webrtc")]
fn start_mesh(s: &mut State, v: &VoiceServerUpdate) {
    drop_call(s);
    s.app.voice.connection_id = Some(v.connection_id.clone());
    let generation = s.app.voice.generation;
    let target = mesh::Target {
        guild_id: v.guild_id.map(|g| g.0),
        channel_id: v.channel_id.0,
        connection_id: v.connection_id.clone(),
        ice_servers: v.ice_servers.iter().map(|i| i.urls.clone()).collect(),
    };
    let speak = crate::shell::voice::can(&s.app.store, &s.app.shell.voice, Permissions::SPEAK);
    let bitrate = crate::guild::channel::mic_bitrate(&s.app.store, v.channel_id);
    spawn(async move {
        let (call, mut events) = Call::connect_mesh(target);
        let call = Arc::new(call);
        if speak && let Err(e) = call.publish_mic(true, bitrate).await {
            eprintln!("reflux: publish mic: {e}");
        }
        let c = call.clone();
        update(move |s| {
            if s.app.voice.generation != generation {
                return spawn(async move {
                    let _ = c.leave().await;
                });
            }
            let v = &mut s.app.shell.voice;
            (v.connected, v.encrypted, v.server) = (true, false, None);
            c.set_mic_gate(false);
            s.app.voice.track_open = false;
            s.app.shell.voice.transmitting = false;
            s.app.voice.call = Some(c);
            apply_self(s);
            apply_devices(s);
            sync_roster(s);
            play_own_entrance(s);
            restore_media(s);
            if let Some((channel, users)) = s.app.voice.pending_ring.take().filter(|(ch, _)| s.app.shell.voice.channel.is_some_and(|(_, c)| c == *ch)) {
                api(s, Method::POST, format!("/channels/{channel}/call/ring"), json!({ "recipients": users }));
                s.app.voice.rang = Some((channel, users));
            }
        });
        spawn(ping_loop(Arc::downgrade(&call), generation));
        spawn(self_speaking(Arc::downgrade(&call), generation));
        while let Some(ev) = events.recv().await {
            on_mesh_event(&call, generation, ev);
        }
    });
}

/// Our own ring, from the local gate (a P2P call has no server to report it, and LiveKit's active
/// speakers lag): we're speaking while the gate is open and the mic is loud (300 ms hang).
#[cfg(feature = "voice-webrtc")]
async fn self_speaking(call: std::sync::Weak<Call>, generation: u64) {
    let (mut on, mut quiet, mut noise) = (false, u32::MAX, reflux_voice::NoiseState::Idle);
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let Some(call) = call.upgrade() else { return };
        quiet = if call.mic_level() > 0.01 { 0 } else { quiet.saturating_add(1) };
        // `VoiceSpeakingDetection`: with a gate, its state (5 ms attack, 180 ms release) is our
        // ring; without one (push-to-talk, studio) the level and its 300 ms hang stands in.
        let now_on = call.mic_speaking().unwrap_or(quiet < 6);
        let ns = call.noise_state();
        if ns != noise {
            noise = ns;
            update(move |s| {
                if s.app.voice.generation == generation {
                    s.app.shell.voice.noise_state = ns;
                }
            });
        }
        if on != now_on {
            on = now_on;
            update(move |s| {
                if s.app.voice.generation == generation {
                    let me = s.app.store.my_id();
                    let v = &mut s.app.shell.voice;
                    if on && v.transmitting { v.speaking.insert(me) } else { v.speaking.remove(&me) };
                }
            });
        }
    }
}

#[cfg(feature = "voice-webrtc")]
fn on_mesh_event(call: &Arc<Call>, generation: u64, ev: mesh::Event) {
    let current = move |s: &State| s.app.voice.generation == generation;
    match ev {
        mesh::Event::Signal { to, data } => update(move |s| {
            let Some((guild, channel)) = s.app.shell.voice.channel.filter(|_| current(s)) else { return };
            let sig = VoiceSignal { guild_id: guild, channel_id: channel, to: Some(to.clone()), from: None, user_id: None, data };
            // `VoiceMeshPeer.send`: an oversized signal fails the peer (its status event opens the dialog).
            if signal_size(&sig) > VOICE_SIGNAL_MAX_JSON_BYTES {
                eprintln!("reflux: voice: a signal to {to} is over {VOICE_SIGNAL_MAX_JSON_BYTES} bytes; failing the peer");
                if let Some(m) = s.app.voice.call.as_ref().and_then(|c| c.mesh()) {
                    m.fail_peer(&to);
                }
                return;
            }
            if let Some(gw) = &s.app.gateway {
                let _ = gw.send(Command::Send(GatewayPayload::command(OpCode::VOICE_SIGNAL, sig)));
            }
        }),
        mesh::Event::Status { user, status, .. } => update(move |s| {
            if current(s) {
                on_peer_status(s, Snowflake(user), status);
            }
        }),
        mesh::Event::Remote { conn, user, src, track } => {
            let key = format!("{conn}:{}", src.name());
            match (src, track) {
                (Src::Microphone | Src::ScreenShareAudio, Some(reflux_voice::livekit::webrtc::prelude::MediaStreamTrack::Audio(t))) => {
                    let kind = if src == Src::Microphone { reflux_voice::Kind::Voice } else { reflux_voice::Kind::Stream };
                    let source = reflux_voice::Source { kind, user, conn: conn.clone() };
                    let speaking = move |on: bool| {
                        if src == Src::Microphone {
                            update(move |s| {
                                if current(s) {
                                    let v = &mut s.app.shell.voice;
                                    if on { v.speaking.insert(Snowflake(user)) } else { v.speaking.remove(&Snowflake(user)) };
                                }
                            });
                        }
                    };
                    if let (Some(play), Some(b)) = (call.play_rtc(t, source, speaking), super::BRIDGE.get()) {
                        let h = b.rt.spawn(play).abort_handle();
                        update(move |s| {
                            if !current(s) {
                                return h.abort();
                            }
                            if let Some(old) = s.app.voice.audio.insert(key, h) {
                                old.abort();
                            }
                        });
                    }
                }
                (Src::Camera, Some(reflux_voice::livekit::webrtc::prelude::MediaStreamTrack::Video(t))) => update(move |s| {
                    if current(s) {
                        render_camera(s, Snowflake(user), t);
                    }
                }),
                (Src::Camera, None) => update(move |s| {
                    if let Some(h) = s.app.voice.videos.remove(&Snowflake(user)) {
                        h.abort();
                    }
                    s.app.shell.voice.videos.remove(&Snowflake(user));
                }),
                (Src::ScreenShare, Some(_)) => update(move |s| {
                    if current(s) && s.app.shell.voice.watching == Some(Snowflake(user)) {
                        watch(s, Some(Snowflake(user)));
                    }
                }),
                (_, None) => update(move |s| {
                    if let Some(h) = s.app.voice.audio.remove(&key) {
                        h.abort();
                    }
                }),
                _ => {}
            }
        }
        mesh::Event::Report(r) => update(move |s| {
            if current(s) {
                api(s, Method::POST, "/voice/p2p/connection-reports".into(), json!({ "reports": [r] }));
            }
        }),
    }
}

/// `VoiceMeshPeers` + `showP2pPeerFailedModal`: a new failure opens the dialog; it closes once
/// nobody's failed.
#[cfg(feature = "voice-webrtc")]
fn on_peer_status(s: &mut State, user: Snowflake, status: mesh::Status) {
    let v = &mut s.app.shell.voice;
    let was = v.p2p_failed.contains(&user);
    v.p2p_failed.retain(|u| *u != user);
    if status != mesh::Status::Failed {
        if v.p2p_failed.is_empty() && matches!(v.modal, Some(Modal::P2pFailed { .. })) {
            v.modal = None;
        }
        return;
    }
    v.p2p_failed.push(user);
    if was {
        return;
    }
    let Some((guild, channel)) = v.channel else { return };
    let names = v.p2p_failed.iter().map(|u| s.app.store.display_name(guild, *u)).collect();
    let allow_switch = guild.is_none() || s.app.store.channels.get(&channel).is_none_or(|c| !c.rtc_p2p);
    s.app.shell.voice.modal = Some(Modal::P2pFailed { names, allow_switch });
}

/// `syncRoster`: the other connections in our channel that send peer-to-peer (and ones already
/// connected), once our own state says we're in the mesh; and their server mutes.
fn sync_roster(s: &mut State) {
    let Some((guild, channel)) = s.app.shell.voice.channel else { return };
    let own = s.app.voice.connection_id.clone();
    let own_p2p = s
        .app
        .store
        .voice_states
        .iter()
        .filter(|((g, _), v)| *g == guild && v.channel_id == Some(channel))
        .find(|(_, v)| v.connection_id.is_some() && v.connection_id == own)
        .map(|(_, v)| v.p2p);
    // The server converts a P2P call to the SFU by flipping our own state's `p2p`; a standard
    // grant follows. If it doesn't, the call is gone (VoiceMeshEngine.ts `syncConversionWatchdog`).
    // Only a mesh call can be converted; a plain LiveKit call always reports `p2p: false`.
    let call = s.app.voice.call.clone();
    let mesh = call.as_deref().and_then(|c| c.mesh());
    sync_conversion_watchdog(s, mesh.is_some() && own_p2p == Some(false));
    let Some(mesh) = mesh else { return };
    let own = own.as_deref();
    let states = || s.app.store.voice_states.iter().filter(|((g, _), v)| *g == guild && v.channel_id == Some(channel));
    let mut roster = Vec::new();
    for ((_, user), v) in states() {
        let Some(conn) = v.connection_id.clone().filter(|c| Some(c.as_str()) != own) else { continue };
        if own_p2p.is_some() && (mesh.has(&conn) || (own_p2p == Some(true) && v.p2p)) {
            mesh.server_mute(&conn, v.mute);
            roster.push((conn, user.0));
        }
    }
    mesh.set_roster(&roster);
}

/// `CONVERSION_GRACE_MS`: how long a converted call may go without a standard grant.
const CONVERSION_GRACE: Duration = Duration::from_secs(10);
/// `VOICE_SIGNAL_MAX_JSON_BYTES`: the gateway refuses larger payloads; Fluxer fails the peer first.
#[cfg(feature = "voice-webrtc")]
const VOICE_SIGNAL_MAX_JSON_BYTES: usize = 16_000;

/// `syncConversionWatchdog`: arms (or clears) the timer started when our own voice state left
/// P2P mode. A standard grant clears it in [`on_server_update`]; expiry ends the call.
fn sync_conversion_watchdog(s: &mut State, converting: bool) {
    if !converting {
        s.app.voice.conversion = None;
        return;
    }
    if s.app.voice.conversion.is_some() {
        return;
    }
    let (token, generation) = {
        let v = &mut s.app.voice;
        v.conversion_seq += 1;
        (v.conversion_seq, v.generation)
    };
    s.app.voice.conversion = Some(token);
    spawn(async move {
        tokio::time::sleep(CONVERSION_GRACE).await;
        update(move |s| {
            if s.app.voice.conversion != Some(token) || s.app.voice.generation != generation {
                return;
            }
            s.app.voice.conversion = None;
            eprintln!("reflux: voice: the call left P2P mode without a standard grant");
            leave(s, true);
        });
    });
}

/// The op 13 payload's size, as the gateway measures it (`GatewaySocket.sendVoiceSignal`).
#[cfg(feature = "voice-webrtc")]
fn signal_size(sig: &VoiceSignal) -> usize {
    serde_json::to_vec(&GatewayPayload::command(OpCode::VOICE_SIGNAL, sig)).map_or(0, |b| b.len())
}

/// VOICE_SIGNAL for our peer-to-peer call.
pub(super) fn on_signal(s: &State, sig: &VoiceSignal) {
    let Some(mesh) = s.app.voice.call.as_ref().and_then(|c| c.mesh()) else { return };
    if s.app.shell.voice.channel != Some((sig.guild_id, sig.channel_id)) {
        return;
    }
    if let (Some(from), Some(user)) = (&sig.from, sig.user_id) {
        mesh.signal(from, user.0, &sig.data);
    }
}

/// The gateway is back: re-send what peers may have missed.
pub(super) fn on_reconnected(s: &State) {
    if let Some(mesh) = s.app.voice.call.as_ref().and_then(|c| c.mesh()) {
        mesh.resume();
    }
}

/// `NewDeviceMonitoring`: lists audio devices every 10 s while signed in and offers to switch to
/// a newly plugged-in one. Started once, on the first READY.
pub(super) fn watch_devices() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    spawn(async move {
        loop {
            if let Ok((inputs, outputs)) = tokio::task::spawn_blocking(reflux_voice::devices).await {
                update(move |s| on_devices(s, inputs, outputs));
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });
}

fn on_devices(s: &mut State, inputs: Vec<String>, outputs: Vec<String>) {
    if s.app.route != super::Route::Main {
        return;
    }
    let v = &s.app.shell.voice;
    let saved = &mut s.app.saved;
    let before = saved.known_devices.len();
    let found = crate::shell::voice::new_devices(&inputs, &outputs, &mut saved.known_devices, &saved.ignored_devices, [v.input.as_deref(), v.output.as_deref()]);
    if saved.known_devices.len() != before {
        crate::session::save(saved);
    }
    // Several at once is a new machine or a driver reload, not a plug-in: don't ask.
    if (1..=2).contains(&found.len()) && v.modal.is_none() {
        let pick = found.iter().find(|d| d.input.is_some()).unwrap_or(&found[0]).clone();
        s.app.shell.voice.modal = Some(Modal::NewDevice(pick));
    }
    s.app.shell.voice.devices = Some((inputs, outputs));
}

/// `ExperimentAssignments` poll delay: `poll_interval_seconds` clamped to 60 s..24 h, doubled
/// per failure (at most twice), ± `poll_jitter_percent` (≤ 50 %), never under 30 s. `r` in 0..1.
fn rollout_delay(interval: Option<f64>, jitter_percent: Option<f64>, failures: u32, r: f64) -> Duration {
    let secs = interval.filter(|s| s.is_finite()).map_or(60.0, |s| s.round().clamp(60.0, 86_400.0));
    let jitter = jitter_percent.filter(|p| p.is_finite()).map_or(0.0, |p| p.round().clamp(0.0, 50.0) / 100.0);
    let target = secs * 1000.0 * 2f64.powi(failures as i32);
    Duration::from_millis((target + target * jitter * (r * 2.0 - 1.0)).round().max(30_000.0) as u64)
}

/// `VoiceP2pRollout` from `GET /experiments`, polled like `ExperimentAssignments` for the
/// session; three 404s in a row stop it.
// shortcut: no ETag / 304 and no ×4 slower polling while the window is hidden.
pub(super) fn load_rollout(s: &mut State) {
    let Some(inst) = s.app.instance.clone() else { return };
    let Some(b) = super::BRIDGE.get() else { return };
    let task = b.rt.spawn(async move {
        let (mut failures, mut not_found) = (0u32, 0u32);
        let mut poll = (None, None);
        loop {
            match inst.rest.get::<Value>("/experiments").await {
                Ok(v) => {
                    (failures, not_found) = (0, 0);
                    poll = (v["poll_interval_seconds"].as_f64(), v["poll_jitter_percent"].as_f64());
                    let a = &v["assignments"]["voice_p2p"];
                    let rollout = (a["enabled"].as_bool().unwrap_or(false), a["max_participants"].as_u64().unwrap_or(2) as usize);
                    update(move |s| s.app.shell.voice.p2p_rollout = rollout);
                }
                Err(e) => {
                    eprintln!("reflux: experiments: {e}");
                    failures = (failures + 1).min(2);
                    let status = match &e {
                        reflux_net::Error::Api { status, .. } => Some(status.as_u16()),
                        reflux_net::Error::Http(h) => h.status().map(|s| s.as_u16()),
                        _ => None,
                    };
                    not_found = if status == Some(404) { not_found + 1 } else { 0 };
                    if not_found >= 3 {
                        return;
                    }
                }
            }
            tokio::time::sleep(rollout_delay(poll.0, poll.1, failures, fastrand::f64())).await;
        }
    });
    if let Some(old) = s.app.voice.rollout.replace(task.abort_handle()) {
        old.abort();
    }
}

/// Draws a remote camera into `voice.videos[user]`.
#[cfg(feature = "voice-webrtc")]
fn render_camera(s: &mut State, user: Snowflake, track: reflux_voice::livekit::webrtc::prelude::RtcVideoTrack) {
    let pixels = s.app.shell.voice.videos.entry(user).or_default().clone();
    if let Some(b) = super::BRIDGE.get() {
        let task = b.rt.spawn(reflux_voice::render_video(track, move |w, h, rgba| pixels.set(w, h, rgba)));
        if let Some(old) = s.app.voice.videos.insert(user, task.abort_handle()) {
            old.abort();
        }
    }
}

#[cfg(feature = "voice-webrtc")]
async fn ping_loop(call: std::sync::Weak<Call>, generation: u64) {
    loop {
        tokio::time::sleep(PING_EVERY).await;
        let Some(call) = call.upgrade() else { return };
        if call.mic_missing() {
            republish_mic(call.clone(), generation);
        }
        let Some(ms) = call.rtt_ms().await else { continue };
        update(move |s| {
            if s.app.voice.generation == generation {
                let v = &mut s.app.shell.voice;
                v.ping = Some(ms);
                v.pings.push(ms);
                if v.pings.len() > 60 {
                    v.pings.remove(0);
                }
            }
        });
    }
}

/// Others stopped hearing us after an hour or two while we still heard them, until a rejoin:
/// a full reconnect's republish of the mic had failed. Publish it again, as the rejoin did.
#[cfg(feature = "voice-webrtc")]
fn republish_mic(call: Arc<Call>, generation: u64) {
    static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if BUSY.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    update(move |s| {
        let v = &s.app.shell.voice;
        let Some((_, channel)) = v.channel.filter(|_| s.app.voice.generation == generation) else {
            return BUSY.store(false, std::sync::atomic::Ordering::Release);
        };
        let muted = v.self_mute || v.self_deaf;
        let bitrate = crate::guild::channel::mic_bitrate(&s.app.store, channel);
        eprintln!("reflux: voice: our microphone is no longer published (a reconnect lost it); publishing it again");
        spawn(async move {
            let result = call.publish_mic(muted, bitrate).await;
            BUSY.store(false, std::sync::atomic::Ordering::Release);
            if let Err(e) = result {
                return eprintln!("reflux: voice: republishing the microphone failed: {e}");
            }
            update(move |s| {
                if s.app.voice.generation == generation {
                    // The new track starts gated; the next tick opens it per voice activity.
                    call.set_mic_gate(false);
                    s.app.voice.track_open = false;
                    apply_self(s);
                }
            });
        });
    });
}

/// The join failed or the room dropped: show "Connection failed" with retry.
fn fail(s: &mut State) {
    let target = s.app.shell.voice.channel;
    leave(s, true);
    s.app.shell.voice.failed = target;
}

/// Op 12 refused the join (`VOICE_CHANNEL_FULL`, P2P consent, ...).
pub(super) fn on_error(s: &mut State, e: &GatewayError) {
    let Some((guild, channel)) = s.app.shell.voice.channel.filter(|_| !s.app.shell.voice.connected) else { return };
    eprintln!("reflux: voice join refused: {} {}", e.code, e.message);
    let agreed = s.app.voice.p2p_agreed == Some(channel);
    leave(s, false);
    // `handleP2pRefusal`: a P2P call we hadn't agreed to asks, then joins again.
    if e.code == "VOICE_P2P_CONSENT_REQUIRED" && !agreed {
        if p2p_consent(s, guild, channel, false) {
            s.app.shell.join_voice(guild, channel);
        }
        return;
    }
    if e.code.starts_with("VOICE_P2P_") {
        s.app.voice.p2p_agreed = None;
    }
    s.app.shell.voice.modal = Some(match e.code.as_str() {
        "VOICE_CHANNEL_FULL" => Modal::ChannelFull,
        "VOICE_P2P_CONSENT_REQUIRED" => Modal::Refused(t("Couldn't join the P2P call. Try again.").into()),
        "VOICE_P2P_UNAVAILABLE" => Modal::Refused(t("P2P calls are unavailable right now.").into()),
        _ => Modal::Refused(e.message.clone()),
    });
}

/// Our own voice state changed: a moderator moved us (follow it; the new grant arrives as a
/// VOICE_SERVER_UPDATE) or disconnected us, or another session took over.
pub(super) fn on_state(s: &mut State, v: &VoiceState) {
    sync_roster(s);
    let me = s.app.store.my_id();
    let ours = v.connection_id.is_some() && v.connection_id == s.app.voice.connection_id;
    if v.user_id != Some(me) || !ours {
        return;
    }
    match v.channel_id {
        None => leave(s, false),
        Some(c) if s.app.shell.voice.channel.is_some_and(|(_, cur)| cur != c) => {
            crate::desktop::play(s, crate::desktop::Sound::UserMove);
            s.app.shell.voice.channel = Some((v.guild_id, c));
        }
        _ => {}
    }
}

/// Mic and speaker follow mute / deafen (deafened also mutes) and the SPEAK permission, and
/// the voice processing settings.
fn apply_self(s: &State) {
    let Some(call) = &s.app.voice.call else { return };
    let v = &s.app.shell.voice;
    call.set_mic_muted(v.self_mute || v.self_deaf);
    call.set_processing(v.settings.mic_processing());
    if let Some(m) = call.mixer() {
        v.settings.apply_mixer(&mut m.lock().unwrap(), &v.local_mutes, v.self_deaf);
    }
}

fn apply_devices(s: &State) {
    let Some(call) = &s.app.voice.call else { return };
    let v = &s.app.shell.voice;
    call.set_devices(v.input.as_deref(), v.output.as_deref());
}

/// The encoder preference: software, or which hardware factory webrtc-sys tries first
/// (`LIVEKIT_PREFERRED_HW_ENCODER`, read when an encoder is created).
pub(super) fn apply_encoder(settings: &Settings) {
    reflux_voice::set_software_encoding(settings.encoder == "software");
    match settings.encoder.as_str() {
        // SAFETY: set on the UI thread; libwebrtc reads it when it creates an encoder.
        e @ ("nvenc" | "vaapi") => unsafe { std::env::set_var("LIVEKIT_PREFERRED_HW_ENCODER", e) },
        _ => unsafe { std::env::remove_var("LIVEKIT_PREFERRED_HW_ENCODER") },
    }
}

/// `CODEC_DISPLAY_LABEL`-style name of what a share publishes in.
fn codec_label(q: &reflux_voice::ScreenQuality) -> &'static str {
    use reflux_voice::{Svc, VideoCodec};
    match q.codec.unwrap_or(if q.svc != Svc::Auto { VideoCodec::VP9 } else { VideoCodec::H264 }) {
        VideoCodec::VP8 => "VP8",
        VideoCodec::H264 => "H264",
        VideoCodec::VP9 => "VP9",
        VideoCodec::AV1 => "AV1",
        VideoCodec::H265 => "H265",
    }
}

/// `VideoAccelerationClassification.isSoftwareVideoImplementation`.
pub(crate) fn software_implementation(name: &str) -> bool {
    const PREFIXES: [&str; 11] = ["libaom", "libvpx", "openh264", "ffmpeg", "dav1d", "libdav1d", "libgav1", "svt", "rav1e", "x264", "x265"];
    let lower = name.to_lowercase();
    PREFIXES.iter().any(|p| lower.starts_with(p)) || lower.contains("software")
}

/// `verifyScreenShareEncoderStart` + `shouldTriggerSoftwareEncoderWarning`: once the share has
/// encoded frames, a software encoder raises `SoftwareEncoderNagbar` unless software was asked
/// for (the Software encoder path, or SVC, which shares in VP9 on the software encoder).
// shortcut: Fluxer's Automatic mode warns only when its GPU probe expected hardware; webrtc-sys
// exposes no such probe, so Automatic warns like Prefer hardware.
fn verify_encoder(s: &mut State, codec: &'static str) {
    let set = &s.app.shell.voice.settings;
    if set.encoder == "software" || set.screen_svc != "auto" || s.app.shell.prefs.dismissed_nagbars.iter().any(|k| k == "software-encoder") {
        return;
    }
    let Some(call) = s.app.voice.call.clone() else { return };
    let generation = s.app.voice.generation;
    spawn(async move {
        for _ in 0..15 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let stats = call.published().await;
            let Some(p) = stats.iter().find(|p| p.source == "Screenshare" && p.frames_encoded > 0) else { continue };
            if software_implementation(&p.encoder) {
                let implementation = p.encoder.clone();
                update(move |s| {
                    if s.app.voice.generation == generation && s.app.voice.share.is_some() {
                        eprintln!("reflux: screen share is using a software encoder ({implementation})");
                        s.app.shell.voice.software_encoder = Some(codec);
                    }
                });
            }
            return;
        }
    });
}

/// Whether a settings change affects what a live screen share publishes.
fn screen_settings_changed(a: &Settings, b: &Settings) -> bool {
    a.streaming_mode != b.streaming_mode
        || a.screen_height != b.screen_height
        || a.screen_fps != b.screen_fps
        || a.screen_bitrate != b.screen_bitrate
        || a.screen_content_hint != b.screen_content_hint
        || a.screen_svc != b.screen_svc
        || a.screen_codec != b.screen_codec
        || a.screen_av1_opt_in != b.screen_av1_opt_in
        || a.screen_hevc_opt_in != b.screen_hevc_opt_in
        || a.encoder != b.encoder
        || a.stream_audio != b.stream_audio
}

fn toggle_screen(s: &mut State) {
    // `SoftwareEncoderWarning.reset()` on every start and stop.
    s.app.shell.voice.software_encoder = None;
    let Some(call) = s.app.voice.call.clone() else { return };
    if let Some(share) = s.app.voice.share.take() {
        if let Some(r) = s.app.voice.own_render.take() {
            r.abort();
        }
        s.app.shell.voice.screen = false;
        send_state(s);
        crate::desktop::play(s, crate::desktop::Sound::ScreenShareStop);
        let audio = s.app.voice.stream_audio.take().map(|(a, _capture)| a);
        let restart = std::mem::take(&mut s.app.voice.restart_screen);
        let generation = s.app.voice.generation;
        return spawn(async move {
            if let Err(e) = call.stop_screen(share).await {
                eprintln!("reflux: stop screen share: {e}");
            }
            if let Some(a) = audio {
                let _ = call.unpublish_stream_audio(&a).await;
            }
            if restart {
                update(move |s| {
                    if s.app.voice.generation == generation && !s.app.shell.voice.screen && s.app.voice.share.is_none() {
                        s.app.shell.intents.push(crate::shell::Intent::ToggleScreen);
                    }
                });
            }
        });
    }
    if let Some(cam) = s.app.voice.device_share.take() {
        s.app.shell.voice.screen = false;
        s.app.voice.device_capture = None;
        send_state(s);
        crate::desktop::play(s, crate::desktop::Sound::ScreenShareStop);
        let audio = s.app.voice.stream_audio.take().map(|(a, _capture)| a);
        let restart = std::mem::take(&mut s.app.voice.restart_screen);
        let generation = s.app.voice.generation;
        return spawn(async move {
            if let Err(e) = call.unpublish_camera(&cam).await {
                eprintln!("reflux: stop device share: {e}");
            }
            if let Some(a) = audio {
                let _ = call.unpublish_stream_audio(&a).await;
            }
            if restart {
                // Let the capture device go before the new share opens it.
                tokio::time::sleep(Duration::from_millis(300)).await;
                update(move |s| {
                    if s.app.voice.generation == generation && !s.app.shell.voice.screen && s.app.voice.device_share.is_none() {
                        s.app.shell.intents.push(crate::shell::Intent::ToggleScreen);
                    }
                });
            }
        });
    }
    if s.app.shell.voice.screen {
        return; // the picker is already open
    }
    s.app.shell.voice.screen = true;
    let generation = s.app.voice.generation;
    // `hasHigherVideoQuality`: P2P calls get it for free.
    let higher = s.app.shell.limits.higher_video_quality || s.app.voice.call.as_ref().is_some_and(|c| c.mesh().is_some());
    let quality = s.app.shell.voice.settings.screen_quality(&s.app.shell.limits, higher);
    let app_audio = s.app.shell.voice.settings.stream_audio.clone().filter(|_| crate::audio_share::available());
    let codec = codec_label(&quality);
    // `Devices` in the picker: a camera / capture device published as the screen share.
    if let Some(device) = s.app.shell.voice.screen_device.clone() {
        let (w, h) = quality.fit(3840, 2160);
        s.app.shell.voice.share_pick = None;
        let fps = quality.fps;
        // A device share is a screen share to the server: the preview uploader gets a frame
        // from the same feed (kept only when it asks).
        let (frame, want) = (s.app.voice.own_frame.clone(), s.app.voice.want_own_frame.clone());
        spawn(async move {
            let cam = match call.publish_test_screen(quality, w, h).await {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    eprintln!("reflux: device share: {e}");
                    update(move |s| {
                        if s.app.voice.generation == generation {
                            s.app.shell.voice.screen = false;
                        }
                    });
                    return;
                }
            };
            let feed = cam.clone();
            // A restart may open the device before the old pipeline has released it; retry once.
            let start = |feed: Arc<Camera>| {
                let (frame, want) = (frame.clone(), want.clone());
                crate::camera::CameraCapture::start(Some(&device), w, h, fps, move |w, h, rgba| {
                    if want.swap(false, std::sync::atomic::Ordering::Relaxed) {
                        *frame.lock().unwrap() = Some((w, h, rgba.clone()));
                    }
                    feed.push_rgba(w, h, &rgba)
                })
            };
            let capture = start(feed.clone()).or_else(|| {
                std::thread::sleep(Duration::from_millis(300));
                start(feed)
            });
            let Some(capture) = capture else {
                let _ = call.unpublish_camera(&cam).await;
                update(move |s| {
                    if s.app.voice.generation == generation && s.app.shell.voice.screen {
                        s.app.shell.voice.screen = false;
                        s.app.shell.notice = Some((t("Couldn't share this device").to_string(), t("Reflux couldn't open the capture device.").to_string()));
                    }
                });
                return;
            };
            let audio = match app_audio {
                Some(node) => match call.publish_stream_audio().await {
                    Ok(a) => Some((Arc::new(a), node)),
                    Err(e) => {
                        eprintln!("reflux: stream audio: {e}");
                        None
                    }
                },
                None => None,
            };
            // Start the capture on this worker thread (module startup can block on a device or
            // permission prompt); a failed start unpublishes the silent track again.
            let audio = match audio {
                Some((a, node)) => {
                    let sink = a.clone();
                    match crate::audio_share::start(&node, move |pcm| sink.push(pcm)) {
                        Some(capture) => Some((a, capture)),
                        None => {
                            eprintln!("reflux: couldn't start stream audio ({node})");
                            let _ = call.unpublish_stream_audio(&a).await;
                            None
                        }
                    }
                }
                None => None,
            };
            update(move |s| {
                if s.app.voice.generation != generation || !s.app.shell.voice.screen {
                    return;
                }
                s.app.voice.device_share = Some(cam);
                s.app.voice.device_capture = Some(capture);
                if let Some((a, capture)) = audio {
                    s.app.voice.stream_audio = Some((a, capture));
                }
                crate::desktop::play(s, crate::desktop::Sound::ScreenShareStart);
                send_state(s);
            });
        });
        return;
    }
    // A live quality change restarts from the source the share already used.
    let pick = s.app.shell.voice.screen_pick.take().or_else(|| s.app.shell.voice.share_pick.clone());
    s.app.shell.voice.share_pick = pick.clone();
    spawn(async move {
        let r = call.start_screen(quality, pick).await;
        // `AudioSourcePickerLinux`: the chosen application's audio goes with the stream.
        let audio = match (&r, app_audio) {
            (Ok(_), Some(node)) => match call.publish_stream_audio().await {
                Ok(a) => Some((Arc::new(a), node)),
                Err(e) => {
                    eprintln!("reflux: stream audio: {e}");
                    None
                }
            },
            _ => None,
        };
        let audio = match audio {
            Some((a, node)) => {
                let sink = a.clone();
                match crate::audio_share::start(&node, move |pcm| sink.push(pcm)) {
                    Some(capture) => Some((a, capture)),
                    None => {
                        eprintln!("reflux: couldn't start stream audio ({node})");
                        let _ = call.unpublish_stream_audio(&a).await;
                        None
                    }
                }
            }
            None => None,
        };
        update(move |s| match r {
            Ok(share) if s.app.voice.generation == generation && s.app.shell.voice.screen => {
                s.app.voice.share = Some(share);
                verify_encoder(s, codec);
                render_own_share(s);
                // A restarted share is a new track: the self view follows it.
                let me = s.app.store.my_id();
                if s.app.shell.voice.watching == Some(me) {
                    watch(s, Some(me));
                }
                if let Some((a, capture)) = audio {
                    s.app.voice.stream_audio = Some((a, capture));
                }
                crate::desktop::play(s, crate::desktop::Sound::ScreenShareStart);
                send_state(s);
            }
            Ok(share) => drop(share),
            Err(e) => {
                eprintln!("reflux: screen share: {e}");
                s.app.shell.voice.screen = false;
            }
        });
    });
}

fn watch(s: &mut State, user: Option<Snowflake>) {
    if let Some(w) = s.app.voice.watch.take() {
        w.abort();
    }
    match user {
        Some(u) if s.app.shell.voice.watch_for != Some(u) => {
            let sh = &mut s.app.shell;
            sh.voice.begin_watch(u);
            // `applyStreamWatchFocus`: the stream becomes the pinned tile and the call's channel is
            // selected; whether its chat or the expanded call shows stays as it was.
            if let Some((Some(_), c)) = sh.voice.channel {
                sh.voice.focus = Some(u);
                sh.call.pin(Some(u));
                if sh.channel != Some(c) && !sh.voice.call_view {
                    sh.select_channel(c);
                }
            }
        }
        None => s.app.shell.voice.end_watch(),
        _ => {}
    }
    send_state(s);
    if user.is_none() {
        crate::popout::stream_ended();
    }
    // A P2P peer only sends its screen share to those who want it.
    if let Some(mesh) = s.app.voice.call.as_ref().and_then(|c| c.mesh()) {
        mesh.set_watching(user.map(|u| u.0));
    }
    // Only the watched stream is subscribed (its arrival watches again via `TrackSubscribed`),
    // and only its audio plays: the mixer also drops a share's audio that arrives before an
    // unsubscribe lands (auto-subscribe on join).
    if let Some(call) = &s.app.voice.call {
        call.sync_streams(user.map(|u| u.0), None);
        if let Some(m) = call.mixer() {
            m.lock().unwrap().watching = user.map(|u| u.0);
        }
    }
    s.app.shell.voice.self_stream = false;
    let (Some(user), Some(call)) = (user, &s.app.voice.call) else { return };
    // Our own stream: Fluxer's tile draws the local track, so the viewer shows it as-is.
    let own = (user == s.app.store.my_id()).then(|| s.app.voice.share.as_ref().and_then(|sh| sh.track())).flatten();
    s.app.shell.voice.self_stream = own.is_some();
    let Some(track) = own.or_else(|| call.screen_track(user.0)) else {
        // Publication missing / attaching: `TrackSubscribed` starts the render once the share
        // arrives, and `watch_step` fails the attempt after 15 s without a frame. In a P2P call
        // the track also arrives later (`Event::Remote` watches again).
        return;
    };
    let pixels = s.app.shell.voice.stream.clone();
    let frames = s.app.shell.voice.stream_frames.clone();
    if let Some(b) = super::BRIDGE.get() {
        let task = b.rt.spawn(reflux_voice::render_video(track, move |w, h, rgba| {
            pixels.set(w, h, rgba);
            frames.fetch_add(1, Ordering::Relaxed);
        }));
        s.app.voice.watch = Some(task.abort_handle());
    }
}

/// Our own screen share tile draws the local track, never a "Watch stream" prompt
/// (`isRemoteVisibleScreenShare` is false for `isOwnScreenShare`).
fn render_own_share(s: &mut State) {
    if let Some(r) = s.app.voice.own_render.take() {
        r.abort();
    }
    let (Some(track), Some(b)) = (s.app.voice.share.as_ref().and_then(|sh| sh.track()), super::BRIDGE.get()) else { return };
    let pixels = s.app.shell.voice.own_share.clone();
    // The uploader keeps one frame when it asks, rather than copying every one.
    let (frame, want) = (s.app.voice.own_frame.clone(), s.app.voice.want_own_frame.clone());
    let task = b.rt.spawn(reflux_voice::render_video(track, move |w, h, rgba| {
        if want.swap(false, std::sync::atomic::Ordering::Relaxed) {
            *frame.lock().unwrap() = Some((w, h, rgba.clone()));
        }
        pixels.set(w, h, rgba);
    }));
    s.app.voice.own_render = Some(task.abort_handle());
}

// ---- Camera ----------------------------------------------------------------------------------

/// Starts the camera capture once (for the preview, our tile and the published track).
fn ensure_capture(s: &mut State) {
    if s.app.voice.capture.is_some() {
        return;
    }
    let st = &s.app.shell.voice.settings;
    let higher = s.app.shell.limits.higher_video_quality || s.app.voice.call.as_ref().is_some_and(|c| c.mesh().is_some());
    let q = st.camera_quality(higher);
    let (w, h, fps) = (q.width, q.height, q.fps);
    let mirror = st.mirror_camera;
    let feed = s.app.voice.feed.clone();
    let view = s.app.shell.voice.self_view.clone();
    let device = st.camera.clone();
    s.app.voice.capture = crate::camera::CameraCapture::start(device.as_deref(), w, h, fps, move |w, h, mut rgba| {
        if let Some(c) = feed.lock().unwrap().clone() {
            c.push_rgba(w, h, &rgba);
        }
        if mirror {
            crate::gst::mirror(&mut rgba, w as usize);
        }
        view.set(w, h, rgba);
    });
    if s.app.voice.capture.is_none() {
        s.app.shell.notice = Some(("Camera unavailable".into(), "Reflux couldn't open a camera.".into()));
        s.app.shell.voice.previewing = false;
    }
}

fn release_capture(s: &mut State) {
    let v = &s.app.shell.voice;
    if !v.camera && !v.previewing {
        s.app.voice.capture = None;
    }
}

fn toggle_camera(s: &mut State) {
    let Some(call) = s.app.voice.call.clone() else { return };
    if s.app.shell.voice.camera {
        s.app.shell.voice.camera = false;
        *s.app.voice.feed.lock().unwrap() = None;
        let cam = s.app.voice.camera.take();
        release_capture(s);
        send_state(s);
        crate::desktop::play(s, crate::desktop::Sound::CameraOff);
        if let Some(cam) = cam {
            spawn(async move {
                if let Err(e) = call.unpublish_camera(&cam).await {
                    eprintln!("reflux: stop camera: {e}");
                }
            });
        }
        return;
    }
    s.app.shell.voice.camera = true;
    ensure_capture(s);
    if s.app.voice.capture.is_none() {
        s.app.shell.voice.camera = false;
        return;
    }
    let generation = s.app.voice.generation;
    let higher = s.app.shell.limits.higher_video_quality || s.app.voice.call.as_ref().is_some_and(|c| c.mesh().is_some());
    let q = s.app.shell.voice.settings.camera_quality(higher);
    spawn(async move {
        let r = call.publish_camera(q).await;
        update(move |s| match r {
            Ok(cam) if s.app.voice.generation == generation && s.app.shell.voice.camera => {
                let cam = Arc::new(cam);
                *s.app.voice.feed.lock().unwrap() = Some(cam.clone());
                s.app.voice.camera = Some(cam);
                crate::desktop::play(s, crate::desktop::Sound::CameraOn);
                send_state(s);
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("reflux: camera: {e}");
                s.app.shell.voice.camera = false;
                release_capture(s);
            }
        });
    });
}

// ---- Microphone gate, mic test, voice messages --------------------------------------------------

/// `useMicTest`: while testing, the selected mic through the voice processing to the selected
/// speakers (at the output volume), and the meter on the processed level.
fn mic_test(s: &mut State) {
    let v = &s.app.shell.voice;
    if !v.mic_test || rune::is_headless() {
        s.app.voice.mic_test = None;
        return;
    }
    let key = (v.settings.mic_processing(), v.input.clone(), v.output.clone());
    if s.app.voice.mic_test.as_ref().is_none_or(|(_, k)| *k != key) {
        s.app.voice.mic_test = None; // close the old streams first
        let e = reflux_voice::audio::Engine::new();
        e.set_processing(key.0);
        e.open_output(key.2.as_deref());
        e.open_input(key.1.as_deref());
        e.set_monitor(true);
        s.app.voice.mic_test = Some((e, key));
    }
    if let Some((e, _)) = &s.app.voice.mic_test {
        e.mixer.lock().unwrap().output = v.settings.output_volume / 100.0;
    }
}

type MicTestKey = (reflux_voice::audio::Processing, Option<String>, Option<String>);

fn ensure_mic(s: &mut State) {
    if s.app.voice.mic.is_some() || rune::is_headless() {
        return;
    }
    let vad = s.app.voice.vad.clone();
    s.app.voice.mic = Mic::start(s.app.shell.voice.input.clone(), move |db| {
        let now = super::now_ms();
        let mut g = vad.lock().unwrap();
        let Some(mode) = g.0 else { return };
        let open = g.1.step(mode, db, now);
        if open != g.2 {
            g.2 = open;
            update(|_| {}); // the next frame's tick applies it
        }
    });
}

/// Once a frame: keeps the mic stream open while something needs it, applies the gate, and
/// refreshes the meter and the recording.
pub(super) fn tick(s: &mut State, tx: &mut rune::Tx) {
    crate::popout::sync(s);
    // `ScreenShareEndedModal`: the capture stopped on its own (the portal/window source went
    // away). Stop the share and tell the user; an intentional stop disarms `ended`.
    // shortcut: only display/window shares report this; camera-device shares can't tell.
    if s.app.shell.voice.screen
        && !s.app.shell.voice.share_ended
        && s.app.voice.share.as_ref().is_some_and(|share| share.ended())
    {
        s.app.shell.voice.share_ended = true;
        s.app.shell.intents.push(Intent::ToggleScreen);
    }
    // `DESKTOP_SOURCE_LIST_POLL_INTERVAL_MS`: refresh the picker's thumbnails while it is open.
    // shortcut: 2 s, not Fluxer's 1 s — each refresh re-captures every source's thumbnail.
    if s.app.shell.voice.modal == Some(Modal::ScreenShare)
        && reflux_voice::can_list_sources()
        && !rune::is_headless()
        && !s.app.voice.screen_sources_loading
        && s.app.voice.screen_sources_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
    {
        s.app.shell.intents.push(Intent::Voice(Op::LoadScreenSources));
    }
    // Active Now's "Watch stream" joined first: watch once the share arrives.
    if let Some(u) = s.app.shell.voice.watch_on_join {
        if s.app.shell.voice.channel.is_none() {
            s.app.shell.voice.watch_on_join = None;
        } else if s.app.voice.call.as_ref().is_some_and(|c| c.has_stream(u.0)) {
            s.app.shell.voice.watch_on_join = None;
            s.app.shell.voice.watching = Some(u);
            watch(s, Some(u));
        }
    }
    let audio = s.app.shell.voice.watching.is_some_and(|u| s.app.voice.call.as_ref().is_some_and(|c| c.has_stream_audio(u.0)));
    if s.app.shell.voice.watch_audio != audio {
        s.app.shell.voice.watch_audio = audio;
    }
    let now = super::now_ms();
    let v = &s.app.shell.voice;
    let in_call = s.app.voice.call.is_some() && !(v.self_mute || v.self_deaf);
    let vad_mode = (in_call && !v.settings.ptt() && v.settings.processing != "studio")
        .then_some(Mode::Activity { threshold: 20.0 * v.settings.vad_rms().log10(), auto: v.settings.vad_auto });
    let recording = v.voice_message.as_ref().is_some_and(|m| m.recording);
    let need = vad_mode.is_some() || recording;
    mic_test(s);
    if need { ensure_mic(s) } else { s.app.voice.mic = None }
    s.app.voice.vad.lock().unwrap().0 = vad_mode;

    let v = &s.app.shell.voice;
    let open = if !in_call {
        false
    } else if v.settings.ptt() {
        // The in-window key, or the global shortcut (xdg GlobalShortcuts portal).
        let held = v.ptt_held || crate::desktop::shortcuts::ptt_held();
        let release = v.settings.ptt_release_ms;
        let open = s.app.voice.ptt.step(Mode::PushToTalk { held, release_ms: release }, 0.0, now);
        if open && !held {
            tx.wake_after(Duration::from_millis(release.max(1)));
        }
        open
    } else if vad_mode.is_some() && s.app.voice.mic.is_some() {
        s.app.voice.vad.lock().unwrap().2
    } else {
        true
    };
    s.app.shell.voice.transmitting = open;
    // Voice activity fades the voice in and out in the mic path (`fluxer-voice-gate`); turning
    // the track on and off a tick after speech starts cut the start of words.
    let track = open || (in_call && vad_mode.is_some());
    if track != s.app.voice.track_open {
        s.app.voice.track_open = track;
        if let Some(call) = &s.app.voice.call {
            call.set_mic_gate(track);
        }
    }

    if let Some((e, _)) = &s.app.voice.mic_test {
        s.app.shell.voice.level = crate::voice_input::to_db(e.level());
        tx.wake_after(Duration::from_millis(50));
    }
    let Some(mic) = &s.app.voice.mic else { return };
    let v = &mut s.app.shell.voice;
    if let Some(vm) = v.voice_message.as_mut().filter(|m| m.recording) {
        vm.secs = mic.recorded_secs().unwrap_or(0.0);
        // `VOICE_MESSAGE_WAVEFORM_BAR_COUNT` bars, one per 70 ms tick.
        vm.bars = mic.recent_peaks(24, 48_000 * 70 / 1000);
        tx.wake_after(Duration::from_millis(70));
    }
}

/// `sendVoiceMessage`: the recording as `voice-message.wav` with its waveform and duration.
fn send_voice_message(s: &mut State) {
    crate::desktop::sound::stop_preview();
    let Some(vm) = s.app.shell.voice.voice_message.clone() else { return };
    let Some((all, rate)) = s.app.voice.recorded.take() else { return };
    if vm.end - vm.start < VoiceMessage::MIN_SECS {
        if let Some(m) = &mut s.app.shell.voice.voice_message {
            m.error = Some(t("Selection must be at least {seconds}s.").replace("{seconds}", "0.5"));
        }
        s.app.voice.recorded = Some((all, rate));
        return;
    }
    let samples = &all[vm.range(all.len(), rate)];
    let (secs, waveform) = crate::voice_input::waveform(samples, rate);
    let bytes = crate::voice_input::wav(samples, rate);
    let sh = &mut s.app.shell;
    sh.upload_seq += 1;
    let upload = crate::shell::upload::Upload {
        id: sh.upload_seq,
        name: "voice-message.wav".into(),
        size: bytes.len() as u64,
        source: crate::shell::upload::Source::Bytes(Arc::new(bytes)),
        spoiler: false,
        description: String::new(),
        voice: Some((secs, waveform)),
    };
    sh.voice.voice_message = None;
    sh.intents.push(Intent::SendMessage { channel: vm.channel, content: String::new(), reply: None, attachments: vec![upload] });
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_live_stream_quality_pick_restarts_the_share_with_it() {
        // The "Stream quality" radios switch to Custom and set the value (`stream_quality_entries`).
        let before = super::Settings::default();
        let mut after = before.clone();
        (after.screen_fps, after.streaming_mode) = (15, "custom".into());
        assert!(super::screen_settings_changed(&before, &after));
        let limits = crate::shell::upload::Limits::default();
        assert_eq!(after.screen_quality(&limits, true).fps, 15);
        after.screen_height = Some(480);
        assert_eq!(after.screen_quality(&limits, true).max_height, Some(480));
    }

    #[test]
    fn rollout_polls_like_experiment_assignments() {
        use std::time::Duration;
        assert_eq!(super::rollout_delay(None, None, 0, 0.5), Duration::from_secs(60));
        assert_eq!(super::rollout_delay(Some(10.0), None, 0, 0.5), Duration::from_secs(60), "at least a minute");
        assert_eq!(super::rollout_delay(Some(300.0), Some(20.0), 0, 1.0), Duration::from_secs(360));
        assert_eq!(super::rollout_delay(Some(300.0), Some(90.0), 2, 0.0), Duration::from_secs(600), "jitter ≤ 50 %, two doublings");
    }

    #[cfg(feature = "voice-webrtc")]
    #[test]
    fn oversized_signals_are_refused() {
        use reflux_model::{OpCode, Snowflake, VoiceSignal};
        use serde_json::json;
        let sig = |data| VoiceSignal { guild_id: None, channel_id: Snowflake(1), to: Some("c".into()), from: None, user_id: None, data };
        assert!(super::signal_size(&sig(json!({ "type": "offer", "sdp": "v=0" }))) <= super::VOICE_SIGNAL_MAX_JSON_BYTES);
        // A session description near the gateway's limit is refused like Fluxer's `sendVoiceSignal`.
        let big = sig(json!({ "type": "offer", "sdp": "x".repeat(super::VOICE_SIGNAL_MAX_JSON_BYTES) }));
        assert!(super::signal_size(&big) > super::VOICE_SIGNAL_MAX_JSON_BYTES);
        assert_eq!(OpCode::VOICE_SIGNAL.0, 13);
    }

    #[test]
    fn encoder_preference_sets_the_env() {
        let mut st = super::Settings { encoder: "nvenc".into(), ..Default::default() };
        super::apply_encoder(&st);
        assert_eq!(std::env::var("LIVEKIT_PREFERRED_HW_ENCODER").as_deref(), Ok("nvenc"));
        st.encoder = "auto".into();
        super::apply_encoder(&st);
        assert!(std::env::var("LIVEKIT_PREFERRED_HW_ENCODER").is_err());
    }
}
