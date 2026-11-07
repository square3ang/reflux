//! `--live=voice`: a short real voice session in the one channel `REFLUX_VOICE_CHANNEL` names
//! (read-only mode lets op 4 through only for it, and for leaving). Joins through the app's own
//! intents, then checks: VOICE_SERVER_UPDATE → LiveKit connect (and whether the grant carried an
//! E2EE key), the gateway's voice state echo, the mic published (`REFLUX_TEST_TONE=1` for a
//! generated tone), ping, our own speaking, self mute / deafen echoes, a screen share track's
//! encoder (`encoderImplementation`), screen share audio from another app, the camera if one
//! exists (each track: server sid, RTP sent, SFU receiver reports), and a clean leave.

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    StreamAudio,
    Camera,
    Leave,
    Done,
}

const ORDER: [Step; 14] = [
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
    Step::StreamAudio,
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
    /// The camera step's SFU check is running.
    pending: bool,
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
            pending: false,
        })
    }

    /// Runs a check on the network thread; [`Run::result`] picks up its outcome.
    fn spawn(&self, f: impl Future<Output = Result<String, String>> + Send + 'static) {
        let got = self.got.clone();
        *got.lock().unwrap() = None;
        super::super::spawn(async move {
            let r = f.await;
            *got.lock().unwrap() = Some(r);
            super::super::update(|_| {});
        });
    }

    fn result(&self, what: &str) -> Option<bool> {
        let r = self.got.lock().unwrap().take()?;
        match &r {
            Ok(m) => say(format!("{what}: {m}")),
            Err(e) => say(format!("{what} failed: {e}")),
        }
        Some(r.is_ok())
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
                // The gateway grants an `e2ee_key` only to e2ee_capable sessions in a guild with
                // this feature (or when its rollout is platform-wide).
                let e2ee = c.guild_id.and_then(|g| s.app.store.guilds.get(&g)?.info.as_ref()).is_some_and(|g| g.features.iter().any(|f| f == "VOICE_E2EE"));
                say(format!("guild feature VOICE_E2EE: {e2ee}"));
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
                    let stop = self.stop.clone();
                    self.spawn(screen(call?, stop));
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
            Step::StreamAudio => {
                if first {
                    self.spawn(stream_audio(call?));
                }
                self.result("stream audio")
            }
            Step::Camera => {
                if first {
                    self.pending = false;
                    let cams = crate::gst::cameras();
                    if cams.is_empty() {
                        say("no camera; skipped");
                        return Some(true);
                    }
                    say(format!("cameras: {cams:?}"));
                    sh.intents.push(crate::shell::Intent::Voice(crate::shell::voice::Op::ToggleCamera));
                }
                if !self.pending {
                    if !self.echo(s)?.self_video {
                        return None;
                    }
                    say("echo self_video true; checking what the SFU receives");
                    self.pending = true;
                    self.spawn(async move {
                        let call = call.ok_or_else(|| "no call".to_string())?;
                        tokio::time::sleep(Duration::from_secs(4)).await;
                        sent(&call, "Camera").await
                    });
                    return None;
                }
                let r = self.result("camera")?;
                say("turning the camera off");
                s.app.shell.intents.push(crate::shell::Intent::Voice(crate::shell::voice::Op::ToggleCamera));
                Some(r)
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
    let sfu = sent(&call, "Screenshare").await;
    let _ = call.unpublish_camera(&track).await;
    stop.store(true, Ordering::Relaxed);
    sfu?;
    Ok(format!("{enc:?}"))
}

/// `source`'s publication was accepted by the server (a `TR_` sid), has sent RTP (and encoded
/// frames, for video) and the SFU's receiver reports came back for it.
async fn sent(call: &reflux_voice::Call, source: &str) -> Result<String, String> {
    let all = call.published().await;
    let p = all.iter().find(|p| p.source == source).ok_or_else(|| format!("no {source} publication in {all:?}"))?;
    let video = matches!(source, "Camera" | "Screenshare");
    let ok = p.sid.starts_with("TR_") && p.packets_sent > 0 && p.sfu_reports > 0 && (!video || p.frames_encoded > 0);
    let line = format!("{p:?}");
    say(format!("published {line}"));
    if ok { Ok(line) } else { Err(line) }
}

/// Screen share audio from another application: a quiet 440 Hz tone played by `pw-play`, found
/// in `pw-dump` like the app's picker does, captured with `pipewiresrc` and published as
/// `ScreenshareAudio` for ~5 s. (Not gst's `pipewiresink`: its node can't be targeted, like
/// with `pw-record --target`.)
async fn stream_audio(call: Arc<reflux_voice::Call>) -> Result<String, String> {
    use std::io::Write;
    let mut tone = std::process::Command::new("pw-play")
        .args(["--raw", "--format", "s16", "--rate", "48000", "--channels", "2", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("pw-play: {e}"))?;
    let mut stdin = tone.stdin.take().ok_or("pw-play: no stdin")?;
    std::thread::spawn(move || {
        // One second of tone, 15 times at most; stops when pw-play is killed (broken pipe).
        let mut pcm = Vec::with_capacity(48_000 * 4);
        for i in 0..48_000 {
            let v = ((i as f32 * 440.0 / 48_000.0 * std::f32::consts::TAU).sin() * 1500.0) as i16;
            pcm.extend_from_slice(&v.to_le_bytes());
            pcm.extend_from_slice(&v.to_le_bytes());
        }
        for _ in 0..15 {
            if stdin.write_all(&pcm).is_err() {
                break;
            }
        }
    });
    let r = capture_app(&call).await;
    let _ = tone.kill();
    let _ = tone.wait();
    r
}

async fn capture_app(call: &reflux_voice::Call) -> Result<String, String> {
    let mut found = None;
    for _ in 0..20 {
        let apps = tokio::task::spawn_blocking(crate::gst::audio_apps).await.unwrap_or_default();
        found = apps.into_iter().find(|(_, name)| name.starts_with("pw-play"));
        if found.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let (node, name) = found.ok_or("the test tone never showed up in pw-dump")?;
    say(format!("capturing PipeWire node {node} ({name})"));
    let audio = Arc::new(call.publish_stream_audio().await.map_err(|e| e.to_string())?);
    let (total, loud) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (sink, t, l) = (audio.clone(), total.clone(), loud.clone());
    let capture = crate::gst::AudioCapture::start(&node, move |pcm| {
        t.fetch_add(pcm.len(), Ordering::Relaxed);
        l.fetch_add(pcm.iter().filter(|s| s.abs() > 1e-3).count(), Ordering::Relaxed);
        sink.push(pcm);
    });
    let r = match capture {
        Some(c) => {
            tokio::time::sleep(Duration::from_secs(5)).await;
            drop(c);
            sent(call, "ScreenshareAudio").await
        }
        None => Err("pipewiresrc didn't start".into()),
    };
    let _ = call.unpublish_stream_audio(&audio).await;
    let (total, loud) = (total.load(Ordering::Relaxed), loud.load(Ordering::Relaxed));
    say(format!("captured {total} samples, {loud} non-silent"));
    if loud == 0 {
        return Err(format!("silence from node {node}"));
    }
    r
}
