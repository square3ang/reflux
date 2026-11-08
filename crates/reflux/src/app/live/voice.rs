//! `--live=voice`: a short real voice session in the one channel `REFLUX_VOICE_CHANNEL` names
//! (read-only mode lets op 4 through only for it, and for leaving). Joins through the app's own
//! intents, then checks: VOICE_SERVER_UPDATE → LiveKit connect (and whether the grant carried an
//! E2EE key), the gateway's voice state echo, the mic published (a generated tone by default, as
//! a headless run has no one speaking; `REFLUX_TEST_TONE=0` uses the real microphone), ping, our
//! own speaking, self mute / deafen echoes, six screen-share quality
//! cases (resolution, frame rate, bitrate, the Screen share preset, the VP9 SVC mode; each one's
//! published frame size, encoded fps, bytes sent, target bitrate and encoder), a device share
//! (`Devices` in the picker), screen share audio from another app, the camera quality presets if
//! one exists (each track: server sid, RTP sent, SFU receiver reports), and a clean leave. The
//! SVC case is skipped, not failed, in a build without a VP9 encoder.
//! `REFLUX_LIVE_SCREEN_CAPTURE=1` uses the real X11 capturer instead of the test pattern.

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
    Device,
    StreamAudio,
    Camera,
    Leave,
    Done,
}

const ORDER: [Step; 15] = [
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
    Step::Device,
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
    /// The screen step's result from the network thread.
    got: Arc<Mutex<Option<Result<Payload, String>>>>,
    /// The camera step (or screen quality case) is running on the network thread.
    pending: bool,
    /// The display can be captured headless (X11); else the test pattern runs.
    capture: bool,
    /// The screen-quality cases: which one is running, when its share started, and whether the
    /// previous share is still being stopped.
    screen_case: usize,
    screen_wait: Option<Instant>,
    screen_stopping: bool,
    /// What the screen cases measured, for the comparison at the end.
    screens: Vec<ScreenStats>,
    /// The user's saved Voice & video settings, restored when the run ends (`Op::Settings`
    /// saves during the live re-configure check).
    orig_settings: Option<crate::shell::voice::Settings>,
    /// The device share: case (720p then a live 480p change), start wait, restart tracking,
    /// stopping and whether it measured right.
    device_case: usize,
    device_wait: Option<Instant>,
    device_change_at: Option<Instant>,
    device_restarted: bool,
    device_stopping: bool,
    device_ok: bool,
    /// The last whole second the Speaking step logged the mic level.
    mic_logged: u64,
    /// `useScreensharePreviewUploader` checked during the first device-share case.
    preview_checked: bool,
    preview_due: bool,
    /// The camera cases: which one is running and whether it is being turned off.
    camera_case: usize,
    camera_on: bool,
    cameras: Vec<ScreenStats>,
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
        // The mic check needs sound, and a headless run has no one speaking into the real
        // microphone: use the generated tone unless the caller asked for the real one.
        if std::env::var_os("REFLUX_TEST_TONE").is_none() {
            // SAFETY: setup, before the audio engine or any capture thread is started.
            unsafe { std::env::set_var("REFLUX_TEST_TONE", "1") };
            say("no REFLUX_TEST_TONE: the mic check uses a generated tone");
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
            capture: false,
            screen_case: 0,
            screen_wait: None,
            screen_stopping: false,
            screens: Vec::new(),
            orig_settings: None,
            device_case: 0,
            device_wait: None,
            device_change_at: None,
            device_restarted: false,
            device_stopping: false,
            device_ok: false,
            mic_logged: u64::MAX,
            preview_checked: false,
            preview_due: false,
            camera_case: 0,
            camera_on: false,
            cameras: Vec::new(),
        })
    }

    /// Our own stream key, and whether the uploader stored a still a viewer can fetch. The
    /// uploader retries every second, so wait up to three for the first attempt.
    fn check_preview(&mut self, s: &State) {
        for _ in 0..6 {
            if s.app.voice.preview_upload.uploaded {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if !s.app.voice.preview_upload.uploaded {
            say("preview: uploader has not uploaded");
            self.failures.push("preview not uploaded".into());
            return;
        }
        let Some(conn) = self.mine(s).and_then(|v| v.connection_id.clone()) else {
            self.failures.push("preview: no connection id".into());
            return;
        };
        let key = format!("{}:{}:{conn}", self.guild.map_or("dm".to_string(), |g| g.to_string()), self.ch);
        let Some(inst) = s.app.instance.as_ref() else { return };
        let Some(bridge) = super::super::BRIDGE.get() else { return };
        let got = bridge.rt.block_on(async { inst.rest.get_bytes(&format!("/streams/{key}/preview")).await });
        match got {
            Ok(Some(b)) if !b.is_empty() => say(format!("preview: server still {} bytes for {key}", b.len())),
            Ok(_) => self.failures.push("preview: GET returned none".into()),
            Err(e) => self.failures.push(format!("preview: GET {e}")),
        }
    }

    /// Runs a check on the network thread; [`Run::result`] picks up its outcome.
    fn spawn(&self, f: impl Future<Output = Result<String, String>> + Send + 'static) {
        self.spawn_payload(async move { f.await.map(Payload::Line) });
    }

    /// Like [`Run::spawn`], for a measurement with numbers.
    fn spawn_stats(&self, f: impl Future<Output = Result<ScreenStats, String>> + Send + 'static) {
        self.spawn_payload(async move { f.await.map(Payload::Stats) });
    }

    fn spawn_payload(&self, f: impl Future<Output = Result<Payload, String>> + Send + 'static) {
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
            Ok(Payload::Line(m)) => say(format!("{what}: {m}")),
            Ok(Payload::Stats(m)) => say(format!("{what}: {m}")),
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
            if let Some(orig) = self.orig_settings.take() {
                s.app.saved.voice_settings = orig;
                crate::session::save(&s.app.saved);
            }
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
        let limit = match self.step {
            Step::Speaking => Duration::from_secs(25),
            Step::Screen => Duration::from_secs(70),
            Step::Device => Duration::from_secs(45),
            Step::Camera => Duration::from_secs(45),
            _ => STEP_TIMEOUT,
        };
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
        if self.preview_due {
            self.preview_due = false;
            self.check_preview(s);
        }
        let call = super::super::voice::call(s);
        let sh = &mut s.app.shell;
        match self.step {
            Step::Preflight => {
                // Start from known quality settings and put the user's ones back when the run ends.
                if first {
                    let vs = &mut s.app.saved.voice_settings;
                    self.orig_settings = Some(vs.clone());
                    (vs.streaming_mode, vs.screen_height, vs.screen_fps, vs.screen_bitrate, vs.screen_content_hint, vs.screen_svc) =
                        ("screenshare".into(), Some(720), 30, None, "text".into(), "auto".into());
                }
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
                if let Some(crate::shell::voice::Modal::JoinConfirm { guild, channel }) = sh.voice.modal {
                    // A previous connection (another device or a run that never left): the same
                    // confirmation a user gets moves this connection here.
                    say("already connected elsewhere; taking the voice connection over");
                    sh.voice.modal = None;
                    sh.intents.push(crate::shell::Intent::Voice(crate::shell::voice::Op::ConfirmJoin(guild, channel)));
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
                // The mic chain's level, so a silent run shows whether the device is delivering.
                if since.as_secs() != self.mic_logged {
                    self.mic_logged = since.as_secs();
                    say(format!("mic peak {:.3} (0..1), transmitting {}, speaking {}", call.as_ref().map_or(0.0, |c| c.mic_level()), sh.voice.transmitting, sh.voice.speaking.contains(&me)));
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
                    // X11 capture lists its sources, so the real `start_screen` path runs without
                    // a portal dialog. Wayland's portal would need a pick we can't make headless;
                    // there the test-pattern track goes through the same publish options.
                    self.capture = prepare_capture();
                    say(if self.capture { "screen capture: X11 (real start_screen)" } else { "screen capture: test pattern (portal/picker unavailable)" });
                }
                if self.screen_case >= SCREEN_CASES.len() {
                    let ok = compare_screens(&self.screens);
                    for f in ok.iter().filter(|(_, ok)| !*ok).map(|(f, _)| f) {
                        self.failures.push(f.clone());
                    }
                    return Some(ok.iter().all(|(_, ok)| *ok));
                }
                let case = SCREEN_CASES[self.screen_case];
                if !self.capture {
                    // No window system to capture: publish the pattern through the same options.
                    if self.pending {
                        let done = self.got.lock().unwrap().take()?;
                        match done {
                            Ok(Payload::Stats(stats)) => {
                                say(format!("screen case {}: {stats}", self.screen_case));
                                self.screens.push(stats);
                                // `.screensharePreviewBackground`: our still reached the server and
                                // a viewer can fetch it (`GET /streams/{key}/preview`).

                            }
                            Ok(Payload::Line(m)) => say(format!("screen case {}: {m}", self.screen_case)),
                            Err(e) => {
                                say(format!("screen case {}: {e}", self.screen_case));
                                self.failures.push(format!("screen case {}: {e}", self.screen_case));
                            }
                        }
                        self.pending = false;
                        self.screen_case += 1;
                        return None;
                    }
                    let st = &mut sh.voice.settings;
                    (st.streaming_mode, st.screen_fps, st.screen_height, st.screen_bitrate, st.screen_content_hint, st.screen_svc) =
                        (case.mode.into(), case.fps, case.max_height, case.bitrate, case.hint.into(), case.svc.into());
                    say(format!("screen case {}: {} {} fps, {:?}p, {:?} bitrate, {} hint, SVC {}", self.screen_case, case.mode, case.fps, case.max_height, case.bitrate, case.hint, case.svc));
                    let quality = st.screen_quality(&sh.limits, sh.limits.higher_video_quality);
                    self.stop.store(false, Ordering::Relaxed);
                    self.spawn_stats(test_pattern(call?, quality, self.stop.clone()));
                    self.pending = true;
                    return None;
                }
                if self.screen_stopping {
                    // The player stopped: move on to the next quality once the track is gone.
                    if s.app.voice.share.is_none() {
                        self.screen_stopping = false;
                        self.screen_case += 1;
                    }
                    return None;
                }
                if self.pending {
                    match self.got.lock().unwrap().take()? {
                        Ok(Payload::Stats(stats)) => {
                            say(format!("screen case {}: {stats}", self.screen_case));
                            self.screens.push(stats);
                        }
                        Ok(Payload::Line(m)) => say(format!("screen case {}: {m}", self.screen_case)),
                        Err(e) => {
                            say(format!("screen case {}: {e}", self.screen_case));
                            self.failures.push(format!("screen case {}: {e}", self.screen_case));
                        }
                    }
                    self.pending = false;
                    self.screen_stopping = true;
                    sh.intents.push(crate::shell::Intent::ToggleScreen);
                    return None;
                }
                if s.app.voice.share.is_none() {
                    if let Some(started) = self.screen_wait {
                        // The share never appeared (capture refused) — record it and move on.
                        if !sh.voice.screen || started.elapsed() > Duration::from_secs(15) {
                            say(format!("screen case {}: share did not start", self.screen_case));
                            self.failures.push(format!("screen case {}: share did not start", self.screen_case));
                            self.screen_wait = None;
                            self.screen_case += 1;
                        }
                        return None;
                    }
                    let st = &mut sh.voice.settings;
                    (st.streaming_mode, st.screen_fps, st.screen_height, st.screen_bitrate, st.screen_content_hint, st.screen_svc) =
                        (case.mode.into(), case.fps, case.max_height, case.bitrate, case.hint.into(), case.svc.into());
                    say(format!("screen case {}: {} {} fps, {:?}p, {:?} bitrate, {} hint, SVC {}", self.screen_case, case.mode, case.fps, case.max_height, case.bitrate, case.hint, case.svc));
                    sh.intents.push(crate::shell::Intent::ToggleScreen);
                    self.screen_wait = Some(Instant::now());
                    return None;
                }
                // The share is up; sample it after the encoder settles.
                self.screen_wait = None;
                self.pending = true;
                let c = call?;
                self.spawn_stats(async move { measure_track(&c, "Screenshare", 2).await });
                None
            }
            Step::Device => {
                // `Devices` in the picker: a camera published as the screen share, then a live
                // 720p → 480p change through `Op::Settings` (the in-call Stream quality menu).
                if first {
                    let cams = crate::camera::cameras();
                    if cams.is_empty() {
                        say("no capture device; skipped");
                        return Some(true);
                    }
                    self.device_ok = true;
                    let st = &mut sh.voice.settings;
                    (st.streaming_mode, st.screen_height, st.screen_fps, st.screen_bitrate, st.screen_content_hint, st.screen_svc) =
                        ("custom".into(), Some(720), 15, None, "text".into(), "auto".into());
                    say(format!("device share: {} at 1280×720/15", cams[0]));
                    sh.voice.screen_device = Some(cams[0].clone());
                    sh.voice.screen_pick = None;
                    sh.intents.push(crate::shell::Intent::ToggleScreen);
                    self.device_wait = Some(Instant::now());
                    return None;
                }
                if self.device_stopping {
                    return if s.app.voice.device_share.is_none() { Some(self.device_ok) } else { None };
                }
                if self.pending {
                    let done = self.got.lock().unwrap().take()?;
                    let check = self.device_case == 0 && !self.preview_checked;
                    if check {
                        self.preview_checked = true;
                    }
                    match done {
                        Ok(Payload::Stats(stats)) => {
                            say(format!("device share case {}: {stats}", self.device_case));
                            // `.screensharePreviewBackground`: our still reached the server and a
                            // viewer can fetch it (`GET /streams/{key}/preview`).
                            if check {
                                self.preview_due = true;
                            }
                            let want = if self.device_case == 0 { (1280, 720) } else { (852, 480) };
                            let ok = (stats.width, stats.height) == want && stats.fps >= 8.0 && !stats.encoder.is_empty();
                            self.device_ok &= ok;
                            if !ok {
                                self.failures.push(format!("device share case {}: {stats}", self.device_case));
                            }
                        }
                        Ok(Payload::Line(m)) => say(format!("device share: {m}")),
                        Err(e) => {
                            say(format!("device share: {e}"));
                            self.failures.push(format!("device share: {e}"));
                        }
                    }
                    self.pending = false;
                    if self.device_case == 0 {
                        // The share is still live: change the resolution from the menu path.
                        let st = &mut sh.voice.settings;
                        (st.screen_height, st.screen_fps) = (Some(480), 15);
                        say("device share: changing to 480p while live");
                        sh.intents.push(crate::shell::Intent::Voice(crate::shell::voice::Op::Settings));
                        self.device_case = 1;
                        self.device_change_at = Some(Instant::now());
                        self.device_restarted = false;
                    } else {
                        self.device_stopping = true;
                        sh.intents.push(crate::shell::Intent::ToggleScreen);
                    }
                    return None;
                }
                if let Some(changed) = self.device_change_at {
                    // `Op::Settings` stops the old track, then the restart publishes a new one.
                    if s.app.voice.device_share.is_none() {
                        self.device_restarted = true;
                    }
                    if self.device_restarted && s.app.voice.device_share.is_some() {
                        self.device_change_at = None;
                        self.device_wait = None;
                    } else if changed.elapsed() > Duration::from_secs(15) {
                        say("device share: the live change did not restart");
                        self.failures.push("device share: the live change did not restart".into());
                        self.device_change_at = None;
                        return Some(false);
                    } else {
                        return None;
                    }
                }
                if s.app.voice.device_share.is_none() {
                    if let Some(started) = self.device_wait
                        && (!sh.voice.screen || started.elapsed() > Duration::from_secs(15))
                    {
                        say("device share: did not start");
                        self.failures.push("device share: did not start".into());
                        self.device_wait = None;
                        return Some(false);
                    }
                    return None;
                }
                self.device_wait = None;
                self.pending = true;
                let c = call?;
                self.spawn_stats(async move { measure_track(&c, "Screenshare", 1).await });
                None
            }
            Step::StreamAudio => {
                if first {
                    self.spawn(stream_audio(call?));
                }
                self.result("stream audio")
            }
            Step::Camera => {
                if first && self.camera_case == 0 {
                    self.pending = false;
                    let cams = crate::camera::cameras();
                    if cams.is_empty() {
                        say("no camera; skipped");
                        return Some(true);
                    }
                    say(format!("cameras: {cams:?}"));
                }
                if self.camera_case >= CAMERA_CASES.len() {
                    let ok = compare_cameras(&self.cameras);
                    for f in ok.iter().filter(|(_, ok)| !*ok).map(|(f, _)| f) {
                        self.failures.push(f.clone());
                    }
                    return Some(ok.iter().all(|(_, ok)| *ok));
                }
                if !self.camera_on {
                    if self.pending || s.app.voice.camera.is_some() {
                        return None;
                    }
                    let case = CAMERA_CASES[self.camera_case];
                    sh.voice.settings.camera_quality = case.quality.into();
                    say(format!("camera case {}: {} {}×{}@{}", self.camera_case, case.quality, case.width, case.height, case.fps));
                    sh.intents.push(crate::shell::Intent::Voice(crate::shell::voice::Op::ToggleCamera));
                    self.camera_on = true;
                    return None;
                }
                if !self.pending {
                    if !self.echo(s)?.self_video {
                        return None;
                    }
                    say("echo self_video true; checking what the SFU receives");
                    self.pending = true;
                    self.spawn_stats(async move {
                        let call = call.ok_or_else(|| "no call".to_string())?;
                        measure_track(&call, "Camera", 1).await
                    });
                    return None;
                }
                match self.got.lock().unwrap().take()? {
                    Ok(Payload::Stats(stats)) => {
                        say(format!("camera case {}: {stats}", self.camera_case));
                        self.cameras.push(stats);
                    }
                    Ok(Payload::Line(m)) => say(format!("camera case {}: {m}", self.camera_case)),
                    Err(e) => {
                        say(format!("camera case {}: {e}", self.camera_case));
                        self.failures.push(format!("camera case {}: {e}", self.camera_case));
                    }
                }
                self.pending = false;
                s.app.shell.intents.push(crate::shell::Intent::Voice(crate::shell::voice::Op::ToggleCamera));
                self.camera_on = false;
                self.camera_case += 1;
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

/// The quality cases `--live=voice` publishes one after another (screen share fps, resolution
/// preset, manual bitrate or `None` = Fluxer's automatic table, content hint).
#[derive(Clone, Copy, Debug)]
struct ScreenCase {
    /// `custom`, or a preset (`screenshare` / `gaming`) whose own values win.
    mode: &'static str,
    /// The values the case expects the encoder to use.
    fps: u32,
    max_height: Option<u32>,
    bitrate: Option<u64>,
    hint: &'static str,
    svc: &'static str,
}

const SCREEN_CASES: [ScreenCase; 6] = [
    ScreenCase { mode: "custom", fps: 30, max_height: Some(720), bitrate: None, hint: "text", svc: "auto" },
    ScreenCase { mode: "custom", fps: 15, max_height: Some(480), bitrate: None, hint: "text", svc: "auto" },
    ScreenCase { mode: "custom", fps: 60, max_height: Some(1080), bitrate: None, hint: "text", svc: "auto" },
    ScreenCase { mode: "custom", fps: 30, max_height: None, bitrate: Some(8_000_000), hint: "motion", svc: "auto" },
    // The Screen share preset (1080p30 with the instance's higher-quality limit).
    ScreenCase { mode: "screenshare", fps: 30, max_height: Some(1080), bitrate: None, hint: "text", svc: "auto" },
    // SVC needs VP9, so this publishes with the software libvpx encoder.
    ScreenCase { mode: "custom", fps: 15, max_height: Some(720), bitrate: None, hint: "text", svc: "single_layer" },
];

/// Camera quality presets (`CAMERA_RESOLUTION_PRESET_TABLE`).
#[derive(Clone, Copy, Debug)]
struct CameraCase {
    quality: &'static str,
    width: u32,
    height: u32,
    fps: u32,
}

const CAMERA_CASES: [CameraCase; 2] =
    [CameraCase { quality: "low", width: 640, height: 360, fps: 20 }, CameraCase { quality: "medium", width: 1280, height: 720, fps: 30 }];

/// What a network-thread check produced.
enum Payload {
    Line(String),
    Stats(ScreenStats),
}

/// One measurement of a published video track: what the encoder produced over two samples.
#[derive(Clone, Debug, Default)]
struct ScreenStats {
    width: u32,
    height: u32,
    fps: f64,
    /// Actual RTP payload rate while sampling.
    kbps: f64,
    /// The pacer's target bitrate (`outbound-rtp.target_bitrate`).
    target_kbps: f64,
    frames: u32,
    bytes: u64,
    encoder: String,
    limitation: String,
}

impl std::fmt::Display for ScreenStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}×{} {:.1} fps, {:.0} kbps sent (target {:.0} kbps), {} frames, {} bytes, encoder {}, quality limitation {}",
            self.width, self.height, self.fps, self.kbps, self.target_kbps, self.frames, self.bytes, self.encoder, self.limitation
        )
    }
}

/// Removes `WAYLAND_DISPLAY` so libwebrtc's capturer uses X11, where sources can be listed
/// without the xdg-desktop-portal's own picker. Returns whether a screen share can be captured.
fn prepare_capture() -> bool {
    // Opt-in: PipeWire-backed Wayland portals still open a picker even without WAYLAND_DISPLAY.
    if std::env::var_os("REFLUX_LIVE_SCREEN_CAPTURE").is_none() {
        return false;
    }
    // SAFETY: before the capture thread starts; only this scene reads these variables.
    unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    std::env::var_os("DISPLAY").is_some()
}

/// The test-pattern capture size when X11 isn't available (a real 1080p screen).
fn capture_size() -> (u32, u32) {
    (1920, 1080)
}

/// Publishes a test-pattern screen share through the same publish options as a real one, feeds
/// it at the requested rate for ~5 s and reports what the SFU's stats say. Fallback when the
/// display can't be captured headless.
async fn test_pattern(call: Arc<reflux_voice::Call>, quality: reflux_voice::ScreenQuality, stop: Arc<AtomicBool>) -> Result<ScreenStats, String> {
    let (sw, sh) = capture_size();
    let (w, h) = quality.fit(sw, sh);
    // Precompute a few high-frequency frames: generating them per frame in a debug build costs
    // more than the encoder and hides the frame-rate setting.
    let frames: Vec<Vec<u8>> = (0..4u32)
        .map(|n| {
            let mut rgba = vec![0u8; (w * h * 4) as usize];
            for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let v = (i as u32).wrapping_mul(2654435761).wrapping_add(n.wrapping_mul(40503));
                px.copy_from_slice(&[(v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, 255]);
            }
            rgba
        })
        .collect();
    let track = Arc::new(call.publish_test_screen(quality, sw, sh).await.map_err(|e| e.to_string())?);
    let feed = track.clone();
    let st = stop.clone();
    let fps = quality.fps.clamp(1, 120);
    std::thread::spawn(move || {
        let mut n = 0usize;
        while !st.load(Ordering::Relaxed) {
            feed.push_rgba(w, h, &frames[n % frames.len()]);
            n += 1;
            std::thread::sleep(Duration::from_millis(1000 / fps as u64));
        }
    });
    let stats = measure_track(&call, "Screenshare", 3).await;
    stop.store(true, Ordering::Relaxed);
    let _ = call.unpublish_camera(&track).await;
    stats
}

/// Samples `source`'s send stats over two seconds, after `settle` seconds to let the encoder
/// pick its rate.
async fn measure_track(call: &reflux_voice::Call, source: &str, settle: u64) -> Result<ScreenStats, String> {
    tokio::time::sleep(Duration::from_secs(settle)).await;
    let a = find(call, source).await?;
    let t = std::time::Instant::now();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let b = find(call, source).await?;
    let secs = t.elapsed().as_secs_f64();
    let frames = b.frames_encoded.saturating_sub(a.frames_encoded);
    let bytes = b.bytes_sent.saturating_sub(a.bytes_sent);
    Ok(ScreenStats {
        width: b.frame_width.max(a.frame_width),
        height: b.frame_height.max(a.frame_height),
        fps: frames as f64 / secs,
        kbps: bytes as f64 * 8.0 / 1000.0 / secs,
        target_kbps: b.target_bitrate / 1000.0,
        frames: b.frames_encoded,
        bytes: b.bytes_sent,
        encoder: b.encoder,
        limitation: b.limitation,
    })
}

async fn find(call: &reflux_voice::Call, source: &str) -> Result<reflux_voice::Published, String> {
    let all = call.published().await;
    all.into_iter().find(|p| p.source == source).ok_or_else(|| format!("no {source} publication"))
}

/// The case checks: resolution follows the preset, the frame rate follows the setting, and the
/// higher-bitrate case sends more than the low one.
fn compare_screens(cases: &[ScreenStats]) -> Vec<(String, bool)> {
    if cases.len() != SCREEN_CASES.len() {
        return vec![(format!("screen: {} of {} cases measured", cases.len(), SCREEN_CASES.len()), false)];
    }
    let mut out = Vec::new();
    let expect = [(1280, 720), (852, 480), (1920, 1080), (1920, 1080), (1920, 1080), (1280, 720)];
    for (i, (s, (w, h))) in cases.iter().zip(expect).enumerate() {
        if SCREEN_CASES[i].svc != "auto" && !s.encoder.to_lowercase().contains("libvpx") {
            // SVC needs VP9 (libvpx); a build without it publishes H.264, at a low rate.
            out.push((format!("screen case {i} skipped: no VP9 encoder in this build ({})", s.encoder), true));
            continue;
        }
        out.push((format!("screen case {i}: {}×{}, wanted {w}×{h}", s.width, s.height), (s.width, s.height) == (w, h)));
        let want = SCREEN_CASES[i].fps as f64;
        out.push((format!("screen case {i}: {:.1} fps, wanted about {want}", s.fps), s.fps >= want * 0.6 && s.fps <= want * 1.2));
        let encoder_ok = if SCREEN_CASES[i].svc == "auto" { !s.encoder.is_empty() } else { s.encoder.to_lowercase().contains("libvpx") };
        out.push((format!("screen case {i}: {} (target {:.0} kbps)", s.encoder, s.target_kbps), encoder_ok && s.target_kbps > 0.0));
    }
    // The settings must change what is published, not just what is stored.
    out.push((format!("1080p60 sends more than 480p15 ({:.0} vs {:.0} kbps)", cases[2].kbps, cases[1].kbps), cases[2].kbps > cases[1].kbps));
    out.push((format!("8 Mbps case sends more than automatic 480p15 ({:.0} vs {:.0} kbps)", cases[3].kbps, cases[1].kbps), cases[3].kbps > cases[1].kbps));
    out.push((format!("frame rate changes across 15/30/60 ({:.1}/{:.1}/{:.1})", cases[1].fps, cases[0].fps, cases[2].fps), cases[1].fps < cases[0].fps && cases[0].fps < cases[2].fps));
    out.push(("the SVC case runs at 15 fps with VP9, not the H.264 rate".to_string(), cases[5].fps < cases[0].fps));
    out
}

fn compare_cameras(cases: &[ScreenStats]) -> Vec<(String, bool)> {
    if cases.len() != CAMERA_CASES.len() {
        return vec![(format!("camera: {} of {} cases measured", cases.len(), CAMERA_CASES.len()), false)];
    }
    let mut out = Vec::new();
    for (i, (s, c)) in cases.iter().zip(&CAMERA_CASES).enumerate() {
        out.push((format!("camera case {i} ({}): {}×{}, wanted {}×{}", c.quality, s.width, s.height, c.width, c.height), (s.width, s.height) == (c.width, c.height)));
        // A debug build can't convert 720p RGBA fast enough for 30 fps (the low 360p case
        // reaches its 20 fps pacer); still require frames, not a stalled source.
        let want = c.fps as f64;
        let fps_ok = if i == 0 { s.fps >= want * 0.5 && s.fps <= want * 1.3 } else { s.fps >= 3.0 && s.fps <= want * 1.3 };
        out.push((format!("camera case {i}: {:.1} fps, wanted about {want}", s.fps), fps_ok));
    }
    out.push((format!("camera quality changes the frame size ({}×{} vs {}×{})", cases[0].width, cases[0].height, cases[1].width, cases[1].height), cases[0].width < cases[1].width));
    out
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
