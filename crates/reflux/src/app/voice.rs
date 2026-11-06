//! Voice runtime: op 4 Voice State Updates, the LiveKit call that a VOICE_SERVER_UPDATE grant
//! opens, active speakers, ping, screen share and the stream viewer. `shell::voice::Voice` holds
//! what the views show; this holds the call itself.

use std::sync::Arc;
use std::time::Duration;

use reflux_model::{GatewayError, GatewayPayload, OpCode, Permissions, Snowflake, UpdateVoiceState, VoiceServerUpdate, VoiceState};
use reflux_net::Command;
use reflux_voice::livekit::prelude::RoomEvent;
use reflux_voice::{Call, ScreenShare};

use super::{spawn, update};
use crate::State;
use crate::shell::Intent;

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
        _ => {}
    }
}

/// The op 4 payload for the shell's current voice state (`channel` `None` = leave).
fn payload(s: &State, channel: Option<(Option<Snowflake>, Snowflake)>) -> UpdateVoiceState {
    let v = &s.app.shell.voice;
    UpdateVoiceState {
        guild_id: channel.and_then(|(g, _)| g),
        channel_id: channel.map(|(_, c)| c),
        connection_id: s.app.voice.connection_id.clone().filter(|_| channel.is_some()),
        self_mute: v.self_mute,
        self_deaf: v.self_deaf,
        self_stream: v.screen && s.app.voice.share.is_some(),
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
    drop_call(s);
    s.app.voice.connection_id = None;
    send(s, payload(s, Some((guild, channel))));
}

/// Hangs up locally; `tell_gateway` also sends op 4 with a null channel.
pub(super) fn leave(s: &mut State, tell_gateway: bool) {
    let Some((guild, _)) = s.app.shell.voice.channel else { return };
    if tell_gateway {
        send(s, UpdateVoiceState { guild_id: guild, ..payload(s, None) });
    }
    drop_call(s);
    s.app.voice.connection_id = None;
    let v = &mut s.app.shell.voice;
    (v.channel, v.connected, v.screen, v.ping, v.watching) = (None, false, false, None, None);
    v.speaking.clear();
    v.pings.clear();
}

fn drop_call(s: &mut State) {
    let rt = &mut s.app.voice;
    rt.generation += 1;
    rt.share = None;
    if let Some(w) = rt.watch.take() {
        w.abort();
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
    // shortcut: peer-to-peer DM calls (`p2p` grants with ICE servers) aren't implemented.
    let (Some(endpoint), Some(token)) = (v.endpoint.clone(), v.token.clone()) else {
        eprintln!("reflux: voice: peer-to-peer grants aren't supported");
        return fail(s);
    };
    drop_call(s);
    s.app.voice.connection_id = Some(v.connection_id.clone());
    let generation = s.app.voice.generation;
    let endpoint = if endpoint.contains("://") { endpoint } else { format!("wss://{endpoint}") };
    let speak = super::super::shell::voice::can(&s.app.store, &s.app.shell.voice, Permissions::SPEAK);
    spawn(async move {
        let (call, mut events) = match Call::connect(&endpoint, &token, true).await {
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
            s.app.voice.call = Some(c);
            s.app.shell.voice.connected = true;
            apply_self(s);
            apply_devices(s);
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
                RoomEvent::TrackSubscribed { .. } => call.apply_deaf(),
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
        fail(s);
    }
}

/// Our own voice state went away (a moderator disconnected us, or another session took over).
// shortcut: being moved to another channel by a moderator isn't followed.
pub(super) fn on_state(s: &mut State, v: &VoiceState) {
    let me = s.app.store.my_id();
    let ours = v.connection_id.is_some() && v.connection_id == s.app.voice.connection_id;
    if v.user_id == Some(me) && ours && v.channel_id.is_none() {
        leave(s, false);
    }
}

/// Mic and speaker follow mute / deafen (deafened also mutes) and the SPEAK permission.
fn apply_self(s: &State) {
    let Some(call) = &s.app.voice.call else { return };
    let v = &s.app.shell.voice;
    call.set_mic_muted(v.self_mute || v.self_deaf);
    call.set_deafened(v.self_deaf);
}

fn apply_devices(s: &State) {
    let Some(call) = &s.app.voice.call else { return };
    let v = &s.app.shell.voice;
    if let Err(e) = call.set_devices(v.input.as_deref(), v.output.as_deref()) {
        eprintln!("reflux: audio devices: {e}");
    }
}

fn toggle_screen(s: &mut State) {
    let Some(call) = s.app.voice.call.clone() else { return };
    if let Some(share) = s.app.voice.share.take() {
        s.app.shell.voice.screen = false;
        send_state(s);
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
    spawn(async move {
        let r = call.start_screen().await;
        update(move |s| match r {
            Ok(share) if s.app.voice.generation == generation && s.app.shell.voice.screen => {
                s.app.voice.share = Some(share);
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
