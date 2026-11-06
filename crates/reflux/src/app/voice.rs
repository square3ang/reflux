//! Voice runtime: op 4 Voice State Updates, the LiveKit call that a VOICE_SERVER_UPDATE grant
//! opens, active speakers, ping, screen share, camera, the stream and camera viewers, the mic
//! gate (voice activity / push-to-talk), DM call ringing and voice message recording.
//! `shell::voice::Voice` holds what the views show; this holds the call and devices.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reflux_model::{GatewayError, GatewayPayload, OpCode, Permissions, Snowflake, UpdateVoiceState, VoiceServerUpdate, VoiceState};
use reflux_net::Command;
use reflux_voice::livekit::prelude::{RemoteTrack, RoomEvent, TrackSource};
use reflux_voice::{Call, Camera, ScreenShare};
use reqwest::Method;
use serde_json::{Value, json};

use super::{spawn, update};
use crate::State;
use crate::shell::Intent;
use crate::shell::voice::{Modal, Op, Settings, VoiceMessage};
use crate::voice_input::{Gate, Mic, Mode};
use crate::i18n::t;

const PING_EVERY: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct VoiceRt {
    call: Option<Arc<Call>>,
    /// Bumped per join / leave, so a stale connect or event loop drops its results.
    generation: u64,
    /// Our connection ID from VOICE_SERVER_UPDATE (tells our voice state from other sessions').
    connection_id: Option<String>,
    share: Option<ScreenShare>,
    watch: Option<tokio::task::AbortHandle>,
    pub(super) devices_loading: bool,
    /// The published camera track, and the slot the capture pushes frames into.
    camera: Option<Arc<Camera>>,
    feed: Arc<Mutex<Option<Arc<Camera>>>>,
    capture: Option<crate::gst::CameraCapture>,
    /// Remote camera renderers by user.
    videos: HashMap<Snowflake, tokio::task::AbortHandle>,
    /// The second microphone stream (meter, voice activity, voice messages), and the
    /// voice-activity gate it drives on the audio thread: (mode, state, open).
    mic: Option<Mic>,
    vad: Arc<Mutex<(Option<Mode>, Gate, bool)>>,
    ptt: Gate,
    /// A DM call we started: ring these once connected; stop ringing them when leaving.
    pending_ring: Option<(Snowflake, Vec<Snowflake>)>,
    rang: Option<(Snowflake, Vec<Snowflake>)>,
    /// The recorded voice message (samples, rate).
    recorded: Option<(Vec<f32>, u32)>,
    /// A join the confirm modal let through.
    confirmed: Option<Snowflake>,
}

pub(super) fn run(s: &mut State, i: Intent) {
    match i {
        Intent::JoinVoice { guild, channel } => join(s, guild, channel),
        Intent::LeaveVoice => leave(s, true),
        Intent::VoiceSelf => {
            send_state(s);
            apply_self(s);
        }
        Intent::ToggleScreen => toggle_screen(s),
        Intent::Watch(user) => watch(s, user),
        Intent::LoadDevices => spawn(async move {
            let r = tokio::task::spawn_blocking(reflux_voice::devices).await;
            update(move |s| match r {
                Ok(Ok(d)) => s.app.shell.voice.devices = Some(d),
                Ok(Err(e)) => {
                    eprintln!("reflux: audio devices: {e}");
                    s.app.shell.voice.devices = Some(Default::default());
                }
                Err(_) => {}
            });
        }),
        Intent::VoiceDevices => {
            let v = &s.app.shell.voice;
            (s.app.saved.voice_input, s.app.saved.voice_output) = (v.input.clone(), v.output.clone());
            crate::session::save(&s.app.saved);
            apply_devices(s);
        }
        Intent::Voice(op) => run_op(s, op),
        _ => {}
    }
}

fn run_op(s: &mut State, op: Op) {
    match op {
        Op::ToggleCamera => toggle_camera(s),
        Op::Preview(on) => {
            s.app.shell.voice.previewing = on;
            if on { ensure_capture(s) } else { release_capture(s) }
        }
        Op::LoadCameras => spawn(async move {
            let names = tokio::task::spawn_blocking(crate::gst::cameras).await.unwrap_or_default();
            update(move |s| s.app.shell.voice.cameras = Some(names));
        }),
        Op::Settings => {
            s.app.saved.voice_settings = s.app.shell.voice.settings.clone();
            crate::session::save(&s.app.saved);
            apply_encoder(&s.app.saved.voice_settings);
            apply_self(s);
        }
        Op::LocalMute(user, muted) => {
            let v = &mut s.app.shell.voice;
            if muted { v.local_mutes.insert(user) } else { v.local_mutes.remove(&user) };
            if let Some(call) = &s.app.voice.call {
                call.set_user_muted(user.0, muted);
            }
        }
        Op::StartCall(channel) => start_call(s, channel),
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
                vm.recording = false;
                s.app.voice.recorded = Some((samples, rate));
            }
        }
        Op::RecordCancel => {
            if let Some(m) = &s.app.voice.mic {
                m.stop_recording();
            }
            s.app.voice.recorded = None;
            s.app.shell.voice.voice_message = None;
        }
        Op::RecordSend => send_voice_message(s),
        Op::PushToTalk(held) => s.app.shell.voice.ptt_held = held,
        Op::ConfirmJoin(guild, channel) => confirm_join(s, guild, channel),
    }
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
        self_stream: v.screen && s.app.voice.share.is_some(),
        viewer_stream_keys: channel.map(|_| watching.map(|k| vec![k])),
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
    // `VoiceConnectionConfirmModal`: already connected from another device.
    let me = s.app.store.my_id();
    let elsewhere = s.app.voice.call.is_none() && s.app.store.voice_states.iter().any(|((_, u), v)| *u == me && v.channel_id.is_some() && v.connection_id != s.app.voice.connection_id);
    if elsewhere && s.app.voice.confirmed != Some(channel) {
        let v = &mut s.app.shell.voice;
        v.channel = None;
        v.modal = Some(Modal::JoinConfirm { guild, channel });
        return;
    }
    s.app.voice.confirmed = None;
    drop_call(s);
    s.app.voice.connection_id = None;
    send(s, payload(s, Some((guild, channel))));
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
    (v.channel, v.connected, v.screen, v.ping, v.watching, v.camera, v.call_view) = (None, false, false, None, None, false, false);
    (v.encrypted, v.server, v.reconnecting, v.focus, v.transmitting) = (false, None, false, None, false);
    v.speaking.clear();
    v.pings.clear();
    v.videos.clear();
    release_capture(s);
}

fn drop_call(s: &mut State) {
    let rt = &mut s.app.voice;
    rt.generation += 1;
    rt.share = None;
    rt.camera = None;
    *rt.feed.lock().unwrap() = None;
    if let Some(w) = rt.watch.take() {
        w.abort();
    }
    for (_, h) in rt.videos.drain() {
        h.abort();
    }
    if let Some(call) = rt.call.take() {
        spawn(async move {
            let _ = call.leave().await;
        });
    }
}

pub(super) fn on_server_update(s: &mut State, v: &VoiceServerUpdate) {
    if s.app.shell.voice.channel != Some((v.guild_id, v.channel_id)) {
        return;
    }
    // shortcut: peer-to-peer calls (`p2p` grants: one WebRTC peer connection per pair, signalled
    // over op 13) aren't implemented; we never send `p2p: true`, so this only comes from a
    // misbehaving server. A LiveKit implementation would need webrtc-sys's raw PeerConnection.
    let (Some(endpoint), Some(token)) = (v.endpoint.clone(), v.token.clone()) else {
        eprintln!("reflux: voice: peer-to-peer grants aren't supported");
        fail(s);
        s.app.shell.voice.modal = Some(Modal::Refused("This call is peer-to-peer, which Reflux can't join yet.".into()));
        return;
    };
    drop_call(s);
    s.app.voice.connection_id = Some(v.connection_id.clone());
    let generation = s.app.voice.generation;
    let endpoint = if endpoint.contains("://") { endpoint } else { format!("wss://{endpoint}") };
    let host = endpoint.split("://").nth(1).unwrap_or(&endpoint).split('/').next().unwrap_or_default().to_string();
    let speak = crate::shell::voice::can(&s.app.store, &s.app.shell.voice, Permissions::SPEAK);
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
        if speak && let Err(e) = call.publish_mic(true).await {
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
            for u in v.local_mutes.clone() {
                c.set_user_muted(u.0, true);
            }
            c.set_mic_gate(false); // the next tick opens it per voice activity / push-to-talk
            s.app.shell.voice.transmitting = false;
            s.app.voice.call = Some(c);
            apply_self(s);
            apply_devices(s);
            if let Some((channel, users)) = s.app.voice.pending_ring.take().filter(|(ch, _)| s.app.shell.voice.channel.is_some_and(|(_, c)| c == *ch)) {
                api(s, Method::POST, format!("/channels/{channel}/call/ring"), json!({ "recipients": users }));
                s.app.voice.rang = Some((channel, users));
            }
        });
        let weak = Arc::downgrade(&call);
        spawn(ping_loop(weak, generation));
        while let Some(ev) = events.recv().await {
            match ev {
                RoomEvent::ActiveSpeakersChanged { speakers } => {
                    let ids = speakers.iter().filter_map(|p| reflux_voice::user_of(&p.identity().0)).map(Snowflake).collect();
                    update(move |s| {
                        if s.app.voice.generation == generation {
                            s.app.shell.voice.speaking = ids;
                        }
                    });
                }
                RoomEvent::TrackSubscribed { track, publication, participant } => {
                    call.apply_deaf();
                    let user = reflux_voice::user_of(&participant.identity().0).map(Snowflake);
                    if let (RemoteTrack::Video(t), Some(user), TrackSource::Camera) = (track, user, publication.source()) {
                        update(move |s| {
                            if s.app.voice.generation == generation {
                                render_camera(s, user, t);
                            }
                        });
                    }
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

/// Draws a remote camera into `voice.videos[user]`.
fn render_camera(s: &mut State, user: Snowflake, track: reflux_voice::livekit::prelude::RemoteVideoTrack) {
    let pixels = s.app.shell.voice.videos.entry(user).or_default().clone();
    if let Some(b) = super::BRIDGE.get() {
        let task = b.rt.spawn(reflux_voice::render_video(track, move |w, h, rgba| pixels.set(w, h, rgba)));
        if let Some(old) = s.app.voice.videos.insert(user, task.abort_handle()) {
            old.abort();
        }
    }
}

async fn ping_loop(call: std::sync::Weak<Call>, generation: u64) {
    loop {
        tokio::time::sleep(PING_EVERY).await;
        let Some(call) = call.upgrade() else { return };
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

/// The join failed or the room dropped: show "Connection failed" with retry.
fn fail(s: &mut State) {
    let target = s.app.shell.voice.channel;
    leave(s, true);
    s.app.shell.voice.failed = target;
}

/// Op 12 refused the join (`VOICE_CHANNEL_FULL`, P2P consent, ...).
pub(super) fn on_error(s: &mut State, e: &GatewayError) {
    if s.app.shell.voice.channel.is_some() && !s.app.shell.voice.connected {
        eprintln!("reflux: voice join refused: {} {}", e.code, e.message);
        leave(s, false);
        s.app.shell.voice.modal = Some(match e.code.as_str() {
            "VOICE_CHANNEL_FULL" => Modal::ChannelFull,
            "VOICE_P2P_CONSENT_REQUIRED" | "VOICE_P2P_UNAVAILABLE" => Modal::Refused("This call is peer-to-peer, which Reflux can't join yet.".into()),
            _ => Modal::Refused(e.message.clone()),
        });
    }
}

/// Our own voice state changed: a moderator moved us (follow it; the new grant arrives as a
/// VOICE_SERVER_UPDATE) or disconnected us, or another session took over.
pub(super) fn on_state(s: &mut State, v: &VoiceState) {
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
    call.set_deafened(v.self_deaf);
    let (ec, ns, agc) = v.settings.processing();
    call.set_processing(ec, ns, agc);
}

fn apply_devices(s: &State) {
    let Some(call) = &s.app.voice.call else { return };
    let v = &s.app.shell.voice;
    if let Err(e) = call.set_devices(v.input.as_deref(), v.output.as_deref()) {
        eprintln!("reflux: audio devices: {e}");
    }
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

fn toggle_screen(s: &mut State) {
    let Some(call) = s.app.voice.call.clone() else { return };
    if let Some(share) = s.app.voice.share.take() {
        s.app.shell.voice.screen = false;
        send_state(s);
        crate::desktop::play(s, crate::desktop::Sound::ScreenShareStop);
        return spawn(async move {
            if let Err(e) = call.stop_screen(share).await {
                eprintln!("reflux: stop screen share: {e}");
            }
        });
    }
    if s.app.shell.voice.screen {
        return; // the picker is already open
    }
    s.app.shell.voice.screen = true;
    let generation = s.app.voice.generation;
    let quality = s.app.shell.voice.settings.screen_quality();
    spawn(async move {
        let r = call.start_screen(quality).await;
        update(move |s| match r {
            Ok(share) if s.app.voice.generation == generation && s.app.shell.voice.screen => {
                s.app.voice.share = Some(share);
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
    send_state(s);
    let (Some(user), Some(call)) = (user, &s.app.voice.call) else { return };
    let Some(track) = call.screen_track(user.0) else {
        eprintln!("reflux: no screen share from {user} to watch");
        s.app.shell.voice.watching = None;
        return;
    };
    let pixels = s.app.shell.voice.stream.clone();
    if let Some(b) = super::BRIDGE.get() {
        let task = b.rt.spawn(reflux_voice::render_video(track, move |w, h, rgba| pixels.set(w, h, rgba)));
        s.app.voice.watch = Some(task.abort_handle());
    }
}

// ---- Camera ----------------------------------------------------------------------------------

/// Starts the camera capture once (for the preview, our tile and the published track).
fn ensure_capture(s: &mut State) {
    if s.app.voice.capture.is_some() {
        return;
    }
    let st = &s.app.shell.voice.settings;
    let (w, h) = st.camera_size();
    let mirror = st.mirror_camera;
    let feed = s.app.voice.feed.clone();
    let view = s.app.shell.voice.self_view.clone();
    let device = st.camera.clone();
    s.app.voice.capture = crate::gst::CameraCapture::start(device.as_deref(), w, h, move |w, h, mut rgba| {
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
    let (w, h) = s.app.shell.voice.settings.camera_size();
    spawn(async move {
        let r = call.publish_camera(w, h).await;
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

fn ensure_mic(s: &mut State) {
    if s.app.voice.mic.is_some() || rune::is_headless() {
        return;
    }
    let vad = s.app.voice.vad.clone();
    s.app.voice.mic = Mic::start(move |db| {
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
    let now = super::now_ms();
    let v = &s.app.shell.voice;
    let in_call = s.app.voice.call.is_some() && !(v.self_mute || v.self_deaf);
    let vad_mode = (in_call && !v.settings.ptt() && v.settings.processing != "studio")
        .then_some(Mode::Activity { threshold: v.settings.vad_threshold, auto: v.settings.vad_auto });
    let recording = v.voice_message.as_ref().is_some_and(|m| m.recording);
    let need = vad_mode.is_some() || v.mic_test || recording;
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
    if open != s.app.shell.voice.transmitting {
        s.app.shell.voice.transmitting = open;
        if let Some(call) = &s.app.voice.call {
            call.set_mic_gate(open);
        }
    }

    let Some(mic) = &s.app.voice.mic else { return };
    let v = &mut s.app.shell.voice;
    if v.mic_test {
        v.level = mic.level_db();
        tx.wake_after(Duration::from_millis(50));
    }
    if let Some(vm) = v.voice_message.as_mut().filter(|m| m.recording) {
        vm.secs = mic.recorded_secs().unwrap_or(0.0);
        // `VOICE_MESSAGE_WAVEFORM_BAR_COUNT` bars, one per 70 ms tick.
        vm.bars = mic.recent_peaks(24, 48_000 * 70 / 1000);
        tx.wake_after(Duration::from_millis(70));
    }
}

/// `sendVoiceMessage`: the recording as `voice-message.wav` with its waveform and duration.
fn send_voice_message(s: &mut State) {
    let Some(vm) = s.app.shell.voice.voice_message.clone() else { return };
    let Some((samples, rate)) = s.app.voice.recorded.take() else { return };
    // `VOICE_MESSAGE_MIN_SEND_DURATION_MS`.
    if (samples.len() as f32 / rate as f32) < 0.5 {
        if let Some(m) = &mut s.app.shell.voice.voice_message {
            m.error = Some("Recording is too short.".into());
        }
        s.app.voice.recorded = Some((samples, rate));
        return;
    }
    let (secs, waveform) = crate::voice_input::waveform(&samples, rate);
    let bytes = crate::voice_input::wav(&samples, rate);
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
    fn encoder_preference_sets_the_env() {
        let mut st = super::Settings { encoder: "nvenc".into(), ..Default::default() };
        super::apply_encoder(&st);
        assert_eq!(std::env::var("LIVEKIT_PREFERRED_HW_ENCODER").as_deref(), Ok("nvenc"));
        st.encoder = "auto".into();
        super::apply_encoder(&st);
        assert!(std::env::var("LIVEKIT_PREFERRED_HW_ENCODER").is_err());
    }
}
