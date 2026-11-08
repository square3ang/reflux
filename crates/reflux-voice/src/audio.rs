//! Our own audio path. LiveKit's Rust SDK has no per-track volume, so the call never opens
//! WebRTC's platform audio module: remote tracks are read as PCM ([`Mixer::push`] from a
//! `NativeAudioStream`), mixed with per-user / per-stream volume, local mutes, deafen and the
//! output volume, and played through cpal on the chosen output device. The microphone is
//! captured through cpal too, scaled by the input volume, run through WebRTC's audio processing
//! module (echo cancellation against our own mix, gain control, high-pass) and RNNoise, and fed
//! to a `NativeAudioSource`. Without the platform module the SDK's synthetic device keeps pulling
//! (decoding) remote audio, and nothing else plays it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use futures_util::FutureExt;
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::AudioSourceOptions;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::native::apm::AudioProcessingModule;

pub use cpal;

#[cfg(target_os = "linux")]
#[path = "audio/pulse.rs"]
mod pulse;

/// The rate everything inside runs at (WebRTC's and RNNoise's).
pub const RATE: u32 = 48_000;
/// 10 ms at [`RATE`]: one APM / RNNoise / WebRTC frame.
pub const FRAME: usize = 480;
/// A remote source's buffer past this is latency, not jitter: drop the oldest down to
/// [`KEEP`].
const MAX_BUFFER: usize = RATE as usize * 3 / 10;
const KEEP: usize = RATE as usize * 6 / 100;
/// `MIC_TEST_MONITOR_DELAY_SECONDS` (0.9 s): how far the mic test's playback lags the mic.
const MONITOR_DELAY: usize = RATE as usize * 9 / 10;
const MONITOR_MAX: usize = MONITOR_DELAY + RATE as usize * 3 / 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A microphone track.
    Voice,
    /// Screen share audio.
    Stream,
    /// A one-shot entrance sound clip.
    Entrance,
    /// The mic test's playback of our own processed mic ([`Engine::set_monitor`]).
    Monitor,
}

/// One remote source: what it is, whose it is, and which connection (a user can join from
/// several devices; each gets its own buffer).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Source {
    pub kind: Kind,
    pub user: u64,
    pub conn: String,
}

/// Mixes the remote sources. Volumes are linear gains (Fluxer's 0..200 % slider / 100).
#[derive(Debug)]
pub struct Mixer {
    sources: HashMap<Source, VecDeque<f32>>,
    pub user_volume: HashMap<u64, f32>,
    pub stream_volume: HashMap<u64, f32>,
    /// Entrance sound volume per user; muted ones never get pushed.
    pub entrance_volume: HashMap<u64, f32>,
    pub muted_users: HashSet<u64>,
    pub muted_streams: HashSet<u64>,
    /// Whose screen share audio plays: only the watched stream's (`ScreenShareSubscriptionPolicy`
    /// subscribes to a stream's audio only while watching it).
    pub watching: Option<u64>,
    pub deaf: bool,
    pub output: f32,
}

impl Default for Mixer {
    fn default() -> Self {
        Self {
            sources: HashMap::new(),
            user_volume: HashMap::new(),
            stream_volume: HashMap::new(),
            entrance_volume: HashMap::new(),
            muted_users: HashSet::new(),
            muted_streams: HashSet::new(),
            watching: None,
            deaf: false,
            output: 1.0,
        }
    }
}

impl Mixer {
    /// Queues mono samples at [`RATE`].
    pub fn push(&mut self, src: Source, samples: &[f32]) {
        let kind = src.kind;
        let buf = self.sources.entry(src).or_default();
        buf.extend(samples);
        // The monitor carries its delay as queued samples; past it, drift becomes latency.
        if kind == Kind::Monitor && buf.len() > MONITOR_MAX {
            let drop = buf.len() - MONITOR_DELAY;
            buf.drain(..drop);
        } else if kind != Kind::Entrance && kind != Kind::Monitor && buf.len() > MAX_BUFFER {
            let drop = buf.len() - KEEP;
            buf.drain(..drop);
        }
    }

    pub fn remove(&mut self, src: &Source) {
        self.sources.remove(src);
    }

    fn gain(&self, src: &Source) -> f32 {
        let g = match src.kind {
            Kind::Voice if self.muted_users.contains(&src.user) => 0.0,
            Kind::Voice => self.user_volume.get(&src.user).copied().unwrap_or(1.0),
            Kind::Stream if self.muted_streams.contains(&src.user) || self.watching != Some(src.user) => 0.0,
            Kind::Stream => self.stream_volume.get(&src.user).copied().unwrap_or(1.0),
            Kind::Entrance => self.entrance_volume.get(&src.user).copied().unwrap_or(1.0),
            Kind::Monitor => 1.0,
        };
        if self.deaf { 0.0 } else { g * self.output }
    }

    /// Fills `out` with the next samples of every source (silence where one runs dry), and
    /// consumes them even when silenced so buffers don't grow while muted or deafened.
    pub fn mix(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let gains: Vec<(Source, f32)> = self.sources.keys().map(|s| (s.clone(), self.gain(s))).collect();
        for (src, g) in gains {
            let buf = self.sources.get_mut(&src).unwrap();
            let n = out.len().min(buf.len());
            for (o, s) in out.iter_mut().zip(buf.drain(..n)) {
                *o += s * g;
            }
            if buf.is_empty() && src.kind == Kind::Entrance {
                self.sources.remove(&src);
            }
        }
        for o in out.iter_mut() {
            *o = o.clamp(-1.0, 1.0);
        }
    }
}

pub use crate::resample::Resampler;

/// The mic processing settings (`VoiceSettings`), shared with the capture thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Processing {
    pub echo_cancellation: bool,
    pub noise: crate::Noise,
    pub auto_gain_control: bool,
    /// Linear input gain (input volume / 100).
    pub input_gain: f32,
    /// The voice-activity gate (`voice_activity` transmit mode), `None` when off.
    pub gate: Option<crate::GateConfig>,
}

impl Default for Processing {
    fn default() -> Self {
        Self { echo_cancellation: true, noise: crate::Noise::default(), auto_gain_control: true, input_gain: 1.0, gate: None }
    }
}

/// Keeps a cpal stream alive on its own thread (cpal streams aren't `Send` everywhere).
pub(super) struct IoThread {
    pub(super) stop: Arc<AtomicBool>,
}

impl Drop for IoThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run_stream(open: impl FnOnce() -> Option<cpal::Stream> + Send + 'static) -> Option<IoThread> {
    let stop = Arc::new(AtomicBool::new(false));
    let st = stop.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Some(stream) = open() else {
            let _ = tx.send(false);
            return;
        };
        let _ = tx.send(true);
        while !st.load(Ordering::Relaxed) {
            std::thread::park_timeout(Duration::from_millis(200));
        }
        drop(stream);
    });
    rx.recv().unwrap_or(false).then_some(IoThread { stop })
}

/// Names of the microphones and speakers (what the device pickers list): the PulseAudio
/// server's sinks/sources on Linux (PipeWire), else cpal's devices.
pub fn devices() -> (Vec<String>, Vec<String>) {
    #[cfg(target_os = "linux")]
    if let Some((sinks, sources)) = pulse::enumerate() {
        let names = |v: Vec<(String, String)>| v.into_iter().map(|(_, label)| label).collect::<Vec<_>>();
        let (input, output) = (names(sources), names(sinks));
        if !input.is_empty() || !output.is_empty() {
            return (input, output);
        }
    }
    let host = cpal::default_host();
    let input = host.input_devices().map(|d| d.map(|d| d.to_string()).collect()).unwrap_or_default();
    let output = host.output_devices().map(|d| d.map(|d| d.to_string()).collect()).unwrap_or_default();
    (input, output)
}

/// The input device called `name`, else the default one.
pub fn input_device(name: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    name.and_then(|n| host.input_devices().ok()?.find(|d| d.to_string() == n)).or_else(|| host.default_input_device())
}

pub fn output_device(name: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    name.and_then(|n| host.output_devices().ok()?.find(|d| d.to_string() == n)).or_else(|| host.default_output_device())
}

/// Where a [`Capture`]'s blocks go.
type CaptureSink = Arc<Mutex<dyn FnMut(&[f32]) + Send>>;

/// A running raw microphone capture: the mic test meter, the voice-activity gate outside the
/// call, and voice-message recording. It records the same selected device as the call, without
/// the mic chain. Dropping it stops capture.
pub struct Capture {
    /// Mono sample rate of the blocks.
    pub rate: u32,
    _thread: IoThread,
}

/// Opens the selected microphone for a [`Capture`]: the PulseAudio source on Linux (the same
/// pick as the call's), else cpal. `on_block` runs on the audio thread with mono samples.
pub fn capture(device: Option<&str>, on_block: impl FnMut(&[f32]) + Send + 'static) -> Option<Capture> {
    let sink: CaptureSink = Arc::new(Mutex::new(on_block));
    #[cfg(target_os = "linux")]
    if let Some(thread) = pulse::open_capture(device, sink.clone()) {
        return Some(Capture { rate: RATE, _thread: thread });
    }
    let name = device.map(str::to_string);
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let st = stop.clone();
    std::thread::spawn(move || {
        let Some((stream, rate)) = open_cpal_capture(name.as_deref(), sink) else {
            let _ = tx.send(None);
            return;
        };
        let _ = tx.send(Some(rate));
        while !st.load(Ordering::Relaxed) {
            std::thread::park_timeout(Duration::from_millis(200));
        }
        drop(stream);
    });
    let rate = rx.recv().ok().flatten()?;
    Some(Capture { rate, _thread: IoThread { stop } })
}

fn open_cpal_capture(name: Option<&str>, sink: CaptureSink) -> Option<(cpal::Stream, u32)> {
    let device = input_device(name)?;
    let cfg = device.default_input_config().ok()?;
    let channels = cfg.channels() as usize;
    let feed = move |mono: &mut dyn Iterator<Item = f32>| {
        let block: Vec<f32> = mono.collect();
        sink.lock().unwrap()(&block);
    };
    let err = |e| eprintln!("reflux-voice: microphone: {e}");
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream::<f32, _, _>(cfg.config(), move |d, _| feed(&mut d.chunks(channels).map(|f| f[0])), err, None),
        cpal::SampleFormat::I16 => device.build_input_stream::<i16, _, _>(cfg.config(), move |d, _| feed(&mut d.chunks(channels).map(|f| f[0] as f32 / 32768.0)), err, None),
        other => {
            eprintln!("reflux-voice: input sample format {other:?} isn't supported");
            return None;
        }
    }
    .map_err(|e| eprintln!("reflux-voice: microphone: {e}"))
    .ok()?;
    stream.play().ok()?;
    Some((stream, cfg.sample_rate()))
}

/// 48 kHz when the device takes it (PipeWire / PulseAudio always do), else its default.
fn config(supported: Vec<cpal::SupportedStreamConfigRange>, default: cpal::SupportedStreamConfig) -> cpal::SupportedStreamConfig {
    supported
        .into_iter()
        .filter(|c| c.channels() == default.channels() && c.sample_format() == default.sample_format())
        .find_map(|c| c.try_with_sample_rate(RATE))
        .unwrap_or(default)
}

type Apm = Arc<Mutex<Option<AudioProcessingModule>>>;

/// The local speaking ring from the gate: 0 = no gate (push-to-talk, studio), 1 = quiet,
/// 2 = talking. Fluxer's own ring is the worklet's speaking state, not the server's list.
pub const SPEAKING_NONE: u8 = 0;
pub type Speaking = Arc<AtomicU8>;

/// The call's audio: the remote mix out, and the mic into [`Engine::source`].
pub struct Engine {
    pub mixer: Arc<Mutex<Mixer>>,
    pub source: NativeAudioSource,
    /// Peak of the last frame sent (`f32` bits).
    level: Arc<AtomicU32>,
    /// The gate's speaking state ([`Speaking`]).
    speaking: Speaking,
    processing: Arc<Mutex<Processing>>,
    apm: Apm,
    /// [`crate::NoiseState`] as a `u8`, written by the mic chain.
    noise_state: Arc<AtomicU8>,
    input: Mutex<Option<IoThread>>,
    output: Mutex<Option<IoThread>>,
    /// Whether the mic chain also plays into [`Engine::mixer`] (the mic test).
    monitor: Arc<AtomicBool>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        // No WebRTC processing on the source: ours runs before it; queue 0 = 10 ms frames, sync.
        let source = NativeAudioSource::new(AudioSourceOptions::default(), RATE, 1, 0);
        let engine = Self { mixer: Arc::default(), source, level: Arc::default(), speaking: Arc::default(), processing: Arc::default(), apm: Arc::default(), noise_state: Arc::default(), input: Mutex::default(), output: Mutex::default(), monitor: Arc::default() };
        engine.set_processing(Processing::default());
        engine
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// The gate's speaking state (`None` while push-to-talk or studio has no gate).
    pub fn speaking(&self) -> Option<bool> {
        match self.speaking.load(Ordering::Relaxed) {
            SPEAKING_NONE => None,
            v => Some(v == 2),
        }
    }

    pub fn noise_state(&self) -> crate::NoiseState {
        match self.noise_state.load(Ordering::Relaxed) {
            1 => crate::NoiseState::Loading,
            2 => crate::NoiseState::Ready,
            3 => crate::NoiseState::Failed,
            _ => crate::NoiseState::Idle,
        }
    }

    pub fn set_processing(&self, p: Processing) {
        let old = std::mem::replace(&mut *self.processing.lock().unwrap(), p);
        let flags = |p: &Processing| (p.echo_cancellation, p.auto_gain_control, p.noise == crate::Noise::Standard);
        // A new APM starts its echo canceller and gain control over: the echo canceller cuts the
        // mic by up to ~28 dB for its first seconds of far-end speech, so rebuilding it on every
        // mute or volume change made the voice others hear dip and recover. Keep it unless what
        // it does changes.
        if flags(&old) == flags(&p) && self.apm.lock().unwrap().is_some() == (p.echo_cancellation || p.auto_gain_control || p.noise == crate::Noise::Standard) {
            return;
        }
        let standard = p.noise == crate::Noise::Standard;
        let any = p.echo_cancellation || p.auto_gain_control || standard;
        // High-pass rides along with any processing; studio (all off) gets none.
        *self.apm.lock().unwrap() = any.then(|| AudioProcessingModule::new(p.echo_cancellation, p.auto_gain_control, true, standard));
    }

    /// (Re)opens the speakers on `name` (`None` = default).
    pub fn open_output(&self, name: Option<&str>) -> bool {
        let mut slot = self.output.lock().unwrap();
        *slot = None;
        let (mixer, apm) = (self.mixer.clone(), self.apm.clone());
        #[cfg(target_os = "linux")]
        if let Some(thread) = pulse::open_output(name, mixer.clone(), apm.clone()) {
            *slot = Some(thread);
            return true;
        }
        let name = name.map(str::to_string);
        *slot = run_stream(move || {
            let device = output_device(name.as_deref())?;
            let cfg = config(device.supported_output_configs().ok()?.collect(), device.default_output_config().ok()?);
            match cfg.sample_format() {
                cpal::SampleFormat::F32 => build_output::<f32>(&device, &cfg, mixer, apm),
                cpal::SampleFormat::I16 => build_output::<i16>(&device, &cfg, mixer, apm),
                other => {
                    eprintln!("reflux-voice: output sample format {other:?} isn't supported");
                    None
                }
            }
        });
        slot.is_some()
    }

    /// (Re)opens the microphone on `name` (`None` = default). `REFLUX_TEST_TONE=1` feeds a
    /// generated voice-like tone through the same chain instead (live tests).
    pub fn open_input(&self, name: Option<&str>) -> bool {
        let mut slot = self.input.lock().unwrap();
        *slot = None;
        let (source, apm, processing, name) = (self.source.clone(), self.apm.clone(), self.processing.clone(), name.map(str::to_string));
        let (level, speaking, noise_state) = (self.level.clone(), self.speaking.clone(), self.noise_state.clone());
        let monitor = (self.mixer.clone(), self.monitor.clone());
        // `REFLUX_TEST_SAMPLE=<wav>`: a recorded clip through the same chain, so speech-like
        // audio exercises the noise suppression and the gate (the tone is a pure sine).
        if let Some(path) = std::env::var_os("REFLUX_TEST_SAMPLE") {
            match read_wav(std::path::Path::new(&path)) {
                Some((samples, rate)) if rate == RATE && !samples.is_empty() => {
                    let chain = MicChain::new(RATE, source, apm, processing, level, speaking, noise_state, monitor);
                    let stop = Arc::new(AtomicBool::new(false));
                    let st = stop.clone();
                    std::thread::spawn(move || {
                        let mut at = 0usize;
                        while !st.load(Ordering::Relaxed) {
                            let frame = (0..FRAME).map(|_| {
                                let s = samples[at % samples.len()];
                                at += 1;
                                s
                            });
                            chain.feed(frame);
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    });
                    *slot = Some(IoThread { stop });
                    return true;
                }
                Some((_, rate)) => eprintln!("reflux-voice: REFLUX_TEST_SAMPLE must be {RATE} Hz mono (is {rate})"),
                None => eprintln!("reflux-voice: couldn't read REFLUX_TEST_SAMPLE"),
            }
        }
        if std::env::var_os("REFLUX_TEST_TONE").is_some_and(|v| v != "0") {
            let chain = MicChain::new(RATE, source, apm, processing, level, speaking, noise_state, monitor);
            let stop = Arc::new(AtomicBool::new(false));
            let st = stop.clone();
            std::thread::spawn(move || {
                let mut n = 0u64;
                while !st.load(Ordering::Relaxed) {
                    chain.feed((0..FRAME).map(|_| {
                        n += 1;
                        test_tone(n)
                    }));
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
            *slot = Some(IoThread { stop });
            return true;
        }
        #[cfg(target_os = "linux")]
        if let Some(thread) = pulse::open_input(name.as_deref(), MicChain::new(RATE, source.clone(), apm.clone(), processing.clone(), level.clone(), speaking.clone(), noise_state.clone(), monitor.clone())) {
            *slot = Some(thread);
            return true;
        }
        *slot = run_stream(move || {
            let device = input_device(name.as_deref())?;
            let cfg = config(device.supported_input_configs().ok()?.collect(), device.default_input_config().ok()?);
            let chain = MicChain::new(cfg.sample_rate(), source, apm, processing, level, speaking, noise_state, monitor);
            match cfg.sample_format() {
                cpal::SampleFormat::F32 => build_input::<f32>(&device, &cfg, chain),
                cpal::SampleFormat::I16 => build_input::<i16>(&device, &cfg, chain),
                other => {
                    eprintln!("reflux-voice: input sample format {other:?} isn't supported");
                    None
                }
            }
        });
        slot.is_some()
    }

    /// `createMicTestAudioGraph`: the processed mic also plays out of this engine's speakers,
    /// 0.9 s late, at the mixer's output volume.
    pub fn set_monitor(&self, on: bool) {
        let src = Source { kind: Kind::Monitor, user: 0, conn: String::new() };
        let mut mixer = self.mixer.lock().unwrap();
        mixer.remove(&src);
        if on {
            mixer.push(src, &vec![0.0; MONITOR_DELAY]);
        }
        self.monitor.store(on, Ordering::Relaxed);
    }

    pub fn close_input(&self) {
        *self.input.lock().unwrap() = None;
    }
}

fn build_output<T: SizedSample + FromSample<f32>>(device: &cpal::Device, cfg: &cpal::SupportedStreamConfig, mixer: Arc<Mutex<Mixer>>, apm: Apm) -> Option<cpal::Stream> {
    let channels = cfg.channels() as usize;
    let mut resampler = Resampler::new(RATE, cfg.sample_rate());
    let ratio = RATE as f64 / cfg.sample_rate() as f64;
    let (mut mix, mut out, mut reverse) = (Vec::new(), Vec::new(), Vec::<f32>::new());
    let stream = device
        .build_output_stream::<T, _, _>(
            cfg.config(),
            move |data: &mut [T], _| {
                let frames = data.len() / channels;
                // Mix just enough at 48 kHz for this many device frames.
                let need = ((frames as f64 - 1.0) * ratio).ceil().max(0.0) as usize + 1;
                mix.resize(need, 0.0);
                mixer.lock().unwrap().mix(&mut mix);
                out.clear();
                resampler.process(&mix, &mut out);
                out.resize(frames, *out.last().unwrap_or(&0.0));
                for (frame, s) in data.chunks_mut(channels).zip(&out) {
                    frame.fill(T::from_sample(*s));
                }
                // The echo canceller's far end: what we just played, in 10 ms frames.
                reverse.extend_from_slice(&mix);
                if let Some(apm) = apm.lock().unwrap().as_mut() {
                    while reverse.len() >= FRAME {
                        let mut pcm: Vec<i16> = reverse.drain(..FRAME).map(to_i16).collect();
                        let _ = apm.process_reverse_stream(&mut pcm, RATE as i32, 1);
                    }
                } else {
                    reverse.clear();
                }
            },
            |e| eprintln!("reflux-voice: speaker: {e}"),
            None,
        )
        .map_err(|e| eprintln!("reflux-voice: speaker: {e}"))
        .ok()?;
    stream.play().ok()?;
    Some(stream)
}

fn build_input<T: SizedSample>(device: &cpal::Device, cfg: &cpal::SupportedStreamConfig, chain: MicChain) -> Option<cpal::Stream>
where
    f32: FromSample<T>,
{
    let channels = cfg.channels() as usize;
    let stream = device
        .build_input_stream::<T, _, _>(
            cfg.config(),
            move |data: &[T], _| chain.feed(data.chunks(channels).map(|f| f32::from_sample(f[0]))),
            |e| eprintln!("reflux-voice: microphone: {e}"),
            None,
        )
        .map_err(|e| eprintln!("reflux-voice: microphone: {e}"))
        .ok()?;
    stream.play().ok()?;
    Some(stream)
}

/// Sample `n` of a 150 Hz buzz with harmonics, syllable-like 4 Hz bursts: loud enough for the
/// server's speaker detection and not the steady tone RNNoise removes.
/// A 16-bit PCM mono WAV's samples and rate (`REFLUX_TEST_SAMPLE`; convert with
/// `ffmpeg -i in.wav -ar 48000 -ac 1 -c:a pcm_s16le out.wav`).
fn read_wav(path: &std::path::Path) -> Option<(Vec<f32>, u32)> {
    let b = std::fs::read(path).ok()?;
    if b.len() < 44 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let (mut rate, mut bits, mut channels, mut data) = (0u32, 0u16, 0u16, None);
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = b.get(i + 8..i + 8 + len)?;
        match id {
            b"fmt " => {
                channels = u16::from_le_bytes([body[2], body[3]]);
                rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                bits = u16::from_le_bytes([body[14], body[15]]);
            }
            b"data" => data = Some(body),
            _ => {}
        }
        i += 8 + len + (len & 1);
    }
    let data = data?;
    if bits != 16 || channels != 1 {
        return None;
    }
    let samples = data.as_chunks::<2>().0.iter().map(|p| i16::from_le_bytes(*p) as f32 / 32768.0).collect();
    Some((samples, rate))
}

pub fn test_tone(n: u64) -> f32 {
    let t = n as f32 / RATE as f32;
    let voice: f32 = (1..6).map(|h| (std::f32::consts::TAU * 150.0 * h as f32 * t).sin() / h as f32).sum();
    let env = (std::f32::consts::TAU * 4.0 * t).sin().max(0.0);
    0.3 * voice * env
}

/// Whether a 10 ms frame (−1..1) is louder than room noise: peak above −40 dBFS.
pub fn is_speech(frame: &[f32]) -> bool {
    frame.iter().any(|s| s.abs() > 0.01)
}

fn to_i16(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * 32767.0) as i16
}

/// Mic samples → 48 kHz → 10 ms frames → gain → APM → RNNoise / DeepFilterNet → gate → the WebRTC
/// source, on its own thread: the capture callback only hands over blocks, so a slow frame (the
/// DeepFilterNet round trip, a busy APM) can't stall the device and drop audio.
struct MicChain {
    blocks: std::sync::mpsc::SyncSender<Vec<f32>>,
    /// Blocks dropped since the last report, and when that was.
    dropped: std::cell::Cell<(u32, Option<std::time::Instant>)>,
}

/// How many capture blocks may wait for the worker (each is 20 ms: 80 ms of slack, then the
/// newest block is dropped rather than holding the device).
const CHAIN_QUEUE: usize = 4;

impl MicChain {
    #[allow(clippy::too_many_arguments)]
    fn new(rate: u32, source: NativeAudioSource, apm: Apm, processing: Arc<Mutex<Processing>>, level: Arc<AtomicU32>, speaking: Speaking, noise_state: Arc<AtomicU8>, monitor: (Arc<Mutex<Mixer>>, Arc<AtomicBool>)) -> Self {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(CHAIN_QUEUE);
        std::thread::spawn(move || {
            let mut inner = MicChainInner { resampler: Resampler::new(rate, RATE), pending: Vec::new(), source, apm, processing, denoise: nnnoiseless::DenoiseState::new(), deep: crate::deepfilter::Deep::new(noise_state), gate: None, speaking, level, monitor, watch: Watch::default() };
            let mut stalled = false;
            loop {
                match rx.recv_timeout(Duration::from_secs(3)) {
                    Ok(block) => {
                        stalled = false;
                        inner.feed(block);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if !std::mem::replace(&mut stalled, true) {
                            eprintln!("reflux-voice: no microphone audio for 3 s (the input stopped delivering)");
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Self { blocks: tx, dropped: Default::default() }
    }

    /// Hands one capture block over; never blocks (a full queue drops it, which others hear as
    /// a cut: reported at most once a second).
    fn feed(&self, mono: impl Iterator<Item = f32>) {
        if self.blocks.try_send(mono.collect()).is_ok() {
            return;
        }
        let (n, at) = self.dropped.get();
        let now = std::time::Instant::now();
        if at.is_none_or(|t| now.duration_since(t) >= Duration::from_secs(1)) {
            eprintln!("reflux-voice: the mic chain fell behind; dropped {} capture blocks", n + 1);
            self.dropped.set((0, Some(now)));
        } else {
            self.dropped.set((n + 1, at));
        }
    }
}

struct MicChainInner {
    resampler: Resampler,
    pending: Vec<f32>,
    source: NativeAudioSource,
    apm: Apm,
    processing: Arc<Mutex<Processing>>,
    denoise: Box<nnnoiseless::DenoiseState<'static>>,
    deep: crate::deepfilter::Deep,
    /// `fluxer-voice-gate`, kept while the gate stays on (a new one seeds its floor again).
    gate: Option<crate::gate::Gate>,
    /// [`Speaking`]: whether the gate says we're talking.
    speaking: Speaking,
    level: Arc<AtomicU32>,
    monitor: (Arc<Mutex<Mixer>>, Arc<AtomicBool>),
    watch: Watch,
}

/// Each stage's peak over [`Watch::WINDOW`], to tell which one turned loud input into silence.
#[derive(Default)]
struct Watch {
    since: Option<std::time::Instant>,
    /// Input, after APM, after noise suppression, sent.
    peaks: [f32; 4],
    reported: Option<std::time::Instant>,
}

impl Watch {
    const WINDOW: Duration = Duration::from_secs(5);

    fn frame(&mut self, peaks: [f32; 4], what: impl FnOnce() -> String) {
        let now = std::time::Instant::now();
        let since = *self.since.get_or_insert(now);
        for (m, p) in self.peaks.iter_mut().zip(peaks) {
            *m = m.max(p);
        }
        if now.duration_since(since) < Self::WINDOW {
            return;
        }
        let [input, apm, denoised, sent] = std::mem::take(&mut self.peaks);
        self.since = Some(now);
        // Speech-loud input (-26 dBFS) all window long, and not a sample of it sent.
        if input > 0.05 && sent == 0.0 && self.reported.is_none_or(|t| now.duration_since(t) >= Duration::from_secs(30)) {
            self.reported = Some(now);
            eprintln!("reflux-voice: 5 s of mic input (peak {input:.3}) went out silent: after APM {apm:.3}, after noise suppression {denoised:.3}; {}", what());
        }
    }
}

fn peak(s: &[i16]) -> f32 {
    s.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) as f32 / 32768.0
}

impl MicChainInner {
    fn feed(&mut self, input: Vec<f32>) {
        self.resampler.process(&input, &mut self.pending);
        let p = *self.processing.lock().unwrap();
        while self.pending.len() >= FRAME {
            let mut frame: Vec<f32> = self.pending.drain(..FRAME).map(|s| s * p.input_gain).collect();
            let mut pcm: Vec<i16> = frame.iter().copied().map(to_i16).collect();
            let input = peak(&pcm);
            if let Some(apm) = self.apm.lock().unwrap().as_mut() {
                let _ = apm.process_stream(&mut pcm, RATE as i32, 1);
            }
            let after_apm = peak(&pcm);
            let deep = p.noise == crate::Noise::DeepFilter && {
                let input: Vec<f32> = pcm.iter().map(|&s| s as f32 / 32768.0).collect();
                self.deep.process(&input, &mut frame)
            };
            if deep {
                pcm = frame.iter().copied().map(to_i16).collect();
            } else if matches!(p.noise, crate::Noise::Rnnoise | crate::Noise::DeepFilter) {
                // RNNoise takes samples on the i16 scale.
                let scaled: Vec<f32> = pcm.iter().map(|&s| s as f32).collect();
                self.denoise.process_frame(&mut frame, &scaled);
                pcm = frame.iter().map(|&s| s.clamp(-32768.0, 32767.0) as i16).collect();
            }
            let denoised = peak(&pcm);
            self.level.store(denoised.to_bits(), Ordering::Relaxed);
            // `VoiceInputProcessor`: noise suppression → the gate → the track. The meter reads
            // the level before it, as the worklet reports it.
            match p.gate {
                Some(cfg) => {
                    let gate = self.gate.get_or_insert_with(|| crate::gate::Gate::new(cfg.auto));
                    let mut f: Vec<f32> = pcm.iter().map(|&s| s as f32 / 32768.0).collect();
                    gate.process(cfg, &mut f);
                    pcm = f.into_iter().map(to_i16).collect();
                    self.speaking.store(u8::from(gate.speaking()) + 1, Ordering::Relaxed);
                }
                None => {
                    self.gate = None;
                    self.speaking.store(SPEAKING_NONE, Ordering::Relaxed);
                }
            }
            if self.monitor.1.load(Ordering::Relaxed) {
                let out: Vec<f32> = pcm.iter().map(|&s| s as f32 / 32768.0).collect();
                self.monitor.0.lock().unwrap().push(Source { kind: Kind::Monitor, user: 0, conn: String::new() }, &out);
            }
            let gate = self.gate.as_ref().map(|g| g.speaking());
            self.watch.frame([input, after_apm, denoised, peak(&pcm)], || format!("noise {:?} (DeepFilterNet ran: {deep}), gate {:?} (speaking {gate:?})", p.noise, p.gate));
            let af = AudioFrame { data: pcm.into(), sample_rate: RATE, num_channels: 1, samples_per_channel: FRAME as u32 };
            // Queue 0: the frame goes straight to the sinks, so this future is already done.
            let _ = self.source.capture_frame(&af).now_or_never();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_watched_stream_plays() {
        let mut m = Mixer::default();
        m.push(src(Kind::Stream, 1), &[0.5; 2]);
        m.push(src(Kind::Stream, 2), &[0.25; 2]);
        let mut out = [0.0; 2];
        m.mix(&mut out);
        assert_eq!(out, [0.0; 2], "not watching anyone");
        m.watching = Some(2);
        m.push(src(Kind::Stream, 1), &[0.5; 2]);
        m.push(src(Kind::Stream, 2), &[0.25; 2]);
        m.mix(&mut out);
        assert_eq!(out, [0.25; 2], "only user 2's stream");
    }

    #[test]
    fn monitor_keeps_its_delay_and_caps_drift() {
        let mut m = Mixer::default();
        m.push(src(Kind::Monitor, 0), &vec![0.0; MONITOR_DELAY]);
        m.push(src(Kind::Monitor, 0), &[0.5; FRAME]);
        assert_eq!(m.sources[&src(Kind::Monitor, 0)].len(), MONITOR_DELAY + FRAME, "not trimmed like a live track");
        m.push(src(Kind::Monitor, 0), &vec![0.5; MONITOR_MAX]);
        assert_eq!(m.sources[&src(Kind::Monitor, 0)].len(), MONITOR_DELAY, "drift trims back to the delay");
        m.output = 0.5;
        let mut out = [0.0; 2];
        m.mix(&mut out);
        assert_eq!(out, [0.25; 2], "plays at the output volume");
    }

    fn src(kind: Kind, user: u64) -> Source {
        Source { kind, user, conn: "c".into() }
    }

    #[test]
    fn mixes_with_per_user_volume_and_output() {
        let mut m = Mixer::default();
        m.push(src(Kind::Voice, 1), &[0.2; 4]);
        m.push(src(Kind::Voice, 2), &[0.1; 2]);
        m.user_volume.insert(1, 2.0);
        m.output = 0.5;
        let mut out = [0.0; 4];
        m.mix(&mut out);
        // user 1: 0.2 * 2 * 0.5 = 0.2; user 2: 0.1 * 0.5 = 0.05 for its two samples.
        assert!((out[0] - 0.25).abs() < 1e-6 && (out[3] - 0.2).abs() < 1e-6, "{out:?}");
    }

    #[test]
    fn mutes_deafen_and_stream_volume() {
        let mut m = Mixer::default();
        m.muted_users.insert(1);
        m.stream_volume.insert(1, 0.5);
        m.watching = Some(1);
        m.push(src(Kind::Voice, 1), &[0.5; 2]);
        m.push(src(Kind::Stream, 1), &[0.5; 2]);
        let mut out = [0.0; 2];
        m.mix(&mut out);
        assert_eq!(out, [0.25; 2], "the voice is muted, the stream plays at half");
        m.deaf = true;
        m.push(src(Kind::Stream, 1), &[0.5; 2]);
        m.mix(&mut out);
        assert_eq!(out, [0.0; 2]);
        m.deaf = false;
        m.mix(&mut out);
        assert_eq!(out, [0.0; 2], "deafened audio was consumed, not replayed later");
    }

    #[test]
    fn clips_and_bounds_latency() {
        let mut m = Mixer::default();
        m.push(src(Kind::Voice, 1), &[0.9; 2]);
        m.push(src(Kind::Voice, 2), &[0.9; 2]);
        let mut out = [0.0; 2];
        m.mix(&mut out);
        assert_eq!(out, [1.0; 2]);
        m.push(src(Kind::Voice, 1), &vec![0.1; MAX_BUFFER + 1]);
        assert_eq!(m.sources[&src(Kind::Voice, 1)].len(), KEEP);
        // Entrance clips are queued whole and dropped once played.
        m.push(src(Kind::Entrance, 3), &vec![0.1; MAX_BUFFER + 1]);
        assert_eq!(m.sources[&src(Kind::Entrance, 3)].len(), MAX_BUFFER + 1);
        let mut big = vec![0.0; MAX_BUFFER + 1];
        m.mix(&mut big);
        assert!(!m.sources.contains_key(&src(Kind::Entrance, 3)));
    }

    /// Needs a real microphone: `cargo test -p reflux-voice captures_the_selected -- --ignored
    /// --nocapture` (`REFLUX_TEST_INPUT` picks a listed source, else the default).
    #[test]
    #[ignore]
    fn captures_the_selected_input() {
        let (inputs, _) = devices();
        eprintln!("inputs: {inputs:?}");
        let device = std::env::var("REFLUX_TEST_INPUT").ok();
        let samples = Arc::new(AtomicU32::new(0));
        let peak = Arc::new(AtomicU32::new(0));
        let (n, p) = (samples.clone(), peak.clone());
        let cap = capture(device.as_deref(), move |block| {
            n.fetch_add(block.len() as u32, Ordering::Relaxed);
            let loudest = block.iter().fold(0f32, |m, s| m.max(s.abs()));
            p.fetch_max(loudest.to_bits(), Ordering::Relaxed);
        })
        .expect("the selected input opens");
        std::thread::sleep(Duration::from_secs(1));
        let (n, p) = (samples.load(Ordering::Relaxed), f32::from_bits(peak.load(Ordering::Relaxed)));
        eprintln!("{device:?}: {n} samples at {} Hz, peak {p}", cap.rate);
        assert!(n >= cap.rate / 4, "only {n} samples in a second");
    }
}
