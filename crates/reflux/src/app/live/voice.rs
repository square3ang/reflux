//! `--live=voice`: a short real voice session in the one channel `REFLUX_VOICE_CHANNEL` names
//! (read-only mode lets op 4 through only for it, and for leaving). Joins through the app's own
//! intents, then checks: VOICE_SERVER_UPDATE → LiveKit connect (and whether the grant carried an
//! E2EE key), the gateway's voice state echo, the mic published (`REFLUX_TEST_TONE=1` for a
//! generated tone), ping, our own speaking, self mute / deafen echoes, a screen share track's
//! encoder (`encoderImplementation`), the camera if one exists, and a clean leave.

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use reflux_model::{ChannelType, Permissions, Snowflake, VoiceState};

use crate::State;

const STEP_TIMEOUT: Duration = Duration::from_secs(20);
/// The whole session stays under two minutes.
pub const TIMEOUT: Duration = Duration::from_secs(110);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Preflight,
    Join,
    Echo,
    Ping,
    Speaking,
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    Screen,
    Camera,
    Leave,
    Done,
}

const ORDER: [Step; 13] = [
    Step::Preflight,
    Step::Join,
    Step::Echo,
    Step::Ping,
    Step::Speaking,
    Step::Mute,
    Step::Unmute,
    Step::Deafen,
    Step::Undeafen,
    Step::Screen,
    Step::Camera,
    Step::Leave,
    Step::Done,
];

pub struct Run {
    step: Step,
    started: Option<Instant>,
    ch: Snowflake,
    guild: Option<Snowflake>,
    failures: Vec<String>,
    /// The test screen track's feeder stops with this.
    stop: Arc<AtomicBool>,
    /// The screen step's result from the network thread: encoders, or an error.
    got: Arc<Mutex<Option<Result<String, String>>>>,
}

fn say(msg: impl std::fmt::Display) {
    eprintln!("reflux --live=voice: {msg}");
}

impl Run {
    pub fn new(scene: &str) -> Option<Run> {
        if scene != "voice" {
            return None;
        }
        let ch = reflux_net::voice_channel().and_then(|c| c.parse().ok()).map(Snowflake);
        if ch.is_none() {
            say("REFLUX_VOICE_CHANNEL is not set; nothing to do");
        }
        Some(Run {
            step: if ch.is_some() { Step::Preflight } else { Step::Done },
            started: None,
            ch: ch.unwrap_or(Snowflake(0)),
            guild: None,
            failures: Vec::new(),
            stop: Arc::default(),
            got: Arc::default(),
        })
    }

    fn mine<'a>(&self, s: &'a State) -> Option<&'a VoiceState> {
        s.app.store.voice_states.get(&(self.guild, s.app.store.my_id()))
    }

    /// Our echoed voice state in the test channel, from this connection.
    fn echo<'a>(&self, s: &'a State) -> Option<&'a VoiceState> {
        let conn = super::super::voice::connection_id(s)?;
        self.mine(s).filter(|v| v.channel_id == Some(self.ch) && v.connection_id.as_deref() == Some(conn))
    }

    /// One poll; true once finished.
    pub fn step(&mut self, s: &mut State) -> bool {
        if self.step == Step::Done {
            self.stop.store(true, Ordering::Relaxed);
            for f in &self.failures {
                say(format!("FAILED {f}"));
            }
            let left = self.mine(s).is_none_or(|v| v.channel_id.is_none());
            say(format!("{} failure(s); left the channel: {left}", self.failures.len()));
            return true;
        }
        let now = Instant::now();
        let first = self.started.is_none();
        let since = now - *self.started.get_or_insert(now);
        let done = self.advance(s, first, since);
        let limit = if self.step == Step::Speaking { Duration::from_secs(25) } else { STEP_TIMEOUT };
        if done == Some(true) {
            say(format!("{:?} ok ({:.1}s)", self.step, since.as_secs_f32()));
        } else if done == Some(false) || since > limit {
            let why = if done.is_none() { "timed out" } else { "failed" };
            self.failures.push(format!("{:?} {why}", self.step));
            say(format!("{:?} {why}", self.step));
            if matches!(self.step, Step::Preflight) {
                self.step = Step::Done;
                return false;
            }
            if matches!(self.step, Step::Join) {
                self.step = Step::Leave;
                self.started = None;
                return false;
            }
        } else {
            return false;
        }
        self.started = None;
        self.step = ORDER.iter().position(|x| *x == self.step).and_then(|i| ORDER.get(i + 1)).copied().unwrap_or(Step::Done);
        false
    }

    /// The whole run hit [`TIMEOUT`]: skip to leaving (once).
    pub fn abort(&mut self, s: &State) {
        if !matches!(self.step, Step::Leave | Step::Done) {
            say("run timed out; leaving");
            self.failures.push(format!("{:?} cut short", self.step));
            self.step = if self.mine(s).is_some_and(|v| v.channel_id.is_some()) || s.app.shell.voice.channel.is_some() { Step::Leave } else { Step::Done };
            self.started = None;
        }
    }

    /// `Some(true)` done, `Some(false)` failed, `None` still waiting.
    fn advance(&mut self, s: &mut State, first: bool, since: Duration) -> Option<bool> {
        let call = super::super::voice::call(s);
        let sh = &mut s.app.shell;
        match self.step {
            Step::Preflight => {
                // `REFLUX_LIVE_ENCODER=nvenc|vaapi|software|auto` tries another preference (not
                // saved). Before joining: the encoder factory reads it once, at connect.
                if let Ok(e) = std::env::var("REFLUX_LIVE_ENCODER") {
                    super::super::voice::apply_encoder(&crate::shell::voice::Settings { encoder: e, ..Default::default() });
                }
                let c = s.app.store.channels.get(&self.ch)?;
                self.guild = c.guild_id;
                let perms = s.app.store.permissions(self.ch);
                say(format!(
                    "channel {:?} kind {:?}, guild {:?}; connect {} speak {} stream {}",
                    c.name,
                    c.kind,
                    c.guild_id,
                    perms.contains(Permissions::CONNECT),
                    perms.contains(Permissions::SPEAK),
                    perms.contains(Permissions::STREAM)
                ));
                Some(c.kind == ChannelType::GUILD_VOICE && perms.contains(Permissions::CONNECT))
            }
            Step::Join => {
                if first {
                    sh.join_voice(self.guild, self.ch);
                    sh.voice.call_view = false;
                }
                if let Some(m) = &sh.voice.modal {
                    say(format!("refused: {m:?}"));
                    return Some(false);
                }
                if sh.voice.failed.is_some() {
                    return Some(false);
                }
                sh.voice.connected.then(|| {
                    let v = &sh.voice;
                    say(format!("connected: server {:?}, e2ee {} (the grant {} e2ee_key)", v.server, v.encrypted, if v.encrypted { "carried" } else { "had no" }));
                    true
                })
            }
            Step::Echo => {
                let e = self.echo(s)?;
                say(format!("gateway voice state echo: {e:?}"));
                Some(true)
            }
            Step::Ping => sh.voice.ping.map(|ms| {
                say(format!("ping {ms} ms"));
                true
            }),
            Step::Speaking => {
                let me = s.app.store.my_id();
                if first {
                    say(format!("transmitting: {}, self_mute {}", sh.voice.transmitting, sh.voice.self_mute));
                }
                // RNNoise may eat the tone; after 12 s try direct input.
                if since > Duration::from_secs(12) && sh.voice.settings.processing != "studio" {
                    say("no speaking yet with focused voice; switching to direct input");
                    // Applied, not saved: live tests leave the user's settings alone.
                    sh.voice.settings.processing = "studio".into();
                    let p = sh.voice.settings.mic_processing();
                    if let Some(c) = &call {
                        c.set_processing(p);
                    }
                }
                sh.voice.speaking.contains(&me).then_some(true)
            }
            Step::Mute | Step::Unmute | Step::Deafen | Step::Undeafen => {
                if first {
                    match self.step {
                        Step::Mute | Step::Unmute => sh.toggle_mute(),
                        _ => sh.toggle_deaf(),
                    }
                }
                let e = self.echo(s)?;
                let want = match self.step {
                    Step::Mute => e.self_mute,
                    Step::Unmute => !e.self_mute,
                    Step::Deafen => e.self_deaf && e.self_mute,
                    _ => !e.self_deaf,
                };
                want.then(|| {
                    say(format!("echo self_mute {} self_deaf {}", e.self_mute, e.self_deaf));
                    true
                })
            }
            Step::Screen => {
                if first {
                    say(format!("LIVEKIT_PREFERRED_HW_ENCODER={:?}", std::env::var("LIVEKIT_PREFERRED_HW_ENCODER").ok()));
                    let call = call?;
                    let (stop, got) = (self.stop.clone(), self.got.clone());
                    *got.lock().unwrap() = None;
                    super::super::spawn(async move {
                        let r = screen(call, stop).await;
                        *got.lock().unwrap() = Some(r);
                        super::super::update(|_| {});
                    });
                }
                match self.got.lock().unwrap().take()? {
                    Ok(enc) => {
                        say(format!("screen share encoders: {enc}"));
                        Some(enc.to_lowercase().contains("nvidia"))
                    }
                    Err(e) => {
                        say(format!("screen share: {e}"));
                        Some(false)
                    }
                }
            }
            Step::Camera => {
                if first {
                    let cams = crate::gst::cameras();
                    if cams.is_empty() {
                        say("no camera; skipped");
                        return Some(true);
                    }
                    say(format!("cameras: {cams:?}"));
                    sh.intents.push(crate::shell::Intent::Voice(crate::shell::voice::Op::ToggleCamera));
                }
                if !sh.voice.camera && since > Duration::from_secs(2) && self.echo(s).is_none_or(|e| !e.self_video) {
                    return None;
                }
                let e = self.echo(s)?;
                if e.self_video {
                    say("echo self_video true; turning the camera off");
                    s.app.shell.intents.push(crate::shell::Intent::Voice(crate::shell::voice::Op::ToggleCamera));
                    return Some(true);
                }
                None
            }
            Step::Leave => {
                if first {
                    self.stop.store(true, Ordering::Relaxed);
                    sh.leave_voice();
                }
                let local = sh.voice.channel.is_none();
                let gone = self.mine(s).is_none_or(|v| v.channel_id.is_none());
                (gone && local).then(|| {
                    say("left: our voice state is gone");
                    true
                })
            }
            Step::Done => Some(true),
        }
    }
}

/// Publishes a 1280×720 test-pattern screen share for ~6 s and reads back its encoder.
async fn screen(call: Arc<reflux_voice::Call>, stop: Arc<AtomicBool>) -> Result<String, String> {
    let (w, h) = (1280u32, 720u32);
    let track = Arc::new(call.publish_test_screen(w, h).await.map_err(|e| e.to_string())?);
    let feed = track.clone();
    let st = stop.clone();
    std::thread::spawn(move || {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let mut n = 0u32;
        while !st.load(Ordering::Relaxed) && n < 30 * 8 {
            for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let (x, y) = (i as u32 % w, i as u32 / w);
                px.copy_from_slice(&[(x + n * 4) as u8, (y + n * 2) as u8, (x ^ y) as u8, 255]);
            }
            feed.push_rgba(w, h, &rgba);
            n += 1;
            std::thread::sleep(Duration::from_millis(33));
        }
    });
    tokio::time::sleep(Duration::from_secs(6)).await;
    let enc = call.video_encoders().await;
    let _ = call.unpublish_camera(&track).await;
    stop.store(true, Ordering::Relaxed);
    Ok(format!("{enc:?}"))
}
