//! The microphone outside the call: the level the mic test meter and the voice-activity gate
//! read, and voice message recording (`VoiceMessageRecordingUtils.ts`: mono 16-bit WAV plus a
//! base64 waveform). WebRTC's audio module owns the call's capture; this opens the default input
//! a second time through cpal, which PipeWire / PulseAudio / WASAPI / CoreAudio allow.
// shortcut: always the system default input, not the device picked in settings (WebRTC's
// device names don't match cpal's); upgrade by matching on the PulseAudio description.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use rodio::cpal;
use rodio::cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// The quietest level the meter shows (`-100` dB in Fluxer's activity slider).
pub const FLOOR_DB: f32 = -100.0;

/// A running input stream; dropping it stops capture.
pub struct Mic {
    /// Peak level of the last block in dBFS (f32 bits).
    level: Arc<AtomicU32>,
    /// Mono samples while recording.
    rec: Arc<Mutex<Option<Vec<f32>>>>,
    rate: u32,
    stop: Arc<AtomicBool>,
}

impl Mic {
    /// Opens the default input; `on_level` runs on the audio thread after each block.
    pub fn start(on_level: impl Fn(f32) + Send + 'static) -> Option<Self> {
        let level = Arc::new(AtomicU32::new(FLOOR_DB.to_bits()));
        let rec: Arc<Mutex<Option<Vec<f32>>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let (l, r, st) = (level.clone(), rec.clone(), stop.clone());
        // The cpal stream isn't Send everywhere: it lives and dies on this thread.
        std::thread::spawn(move || {
            let Some((stream, rate)) = open(l, r, on_level) else {
                let _ = tx.send(None);
                return;
            };
            let _ = tx.send(Some(rate));
            while !st.load(Ordering::Relaxed) {
                std::thread::park_timeout(std::time::Duration::from_millis(200));
            }
            drop(stream);
        });
        let rate = rx.recv().ok().flatten()?;
        Some(Self { level, rec, rate, stop })
    }

    pub fn level_db(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    pub fn start_recording(&self) {
        *self.rec.lock().unwrap() = Some(Vec::new());
    }

    /// Recorded seconds so far (`None` when not recording).
    pub fn recorded_secs(&self) -> Option<f32> {
        self.rec.lock().unwrap().as_ref().map(|v| v.len() as f32 / self.rate as f32)
    }

    /// Ends the recording: the samples and their rate.
    pub fn stop_recording(&self) -> Option<(Vec<f32>, u32)> {
        self.rec.lock().unwrap().take().map(|v| (v, self.rate))
    }

    /// The live recording's last `n` block peaks (0..1) for the composer's bars.
    pub fn recent_peaks(&self, n: usize, block: usize) -> Vec<f32> {
        let rec = self.rec.lock().unwrap();
        let Some(v) = rec.as_ref() else { return Vec::new() };
        let start = v.len().saturating_sub(n * block);
        v[start..].chunks(block).map(|c| c.iter().fold(0f32, |m, s| m.max(s.abs()))).collect()
    }
}

impl Drop for Mic {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn open(level: Arc<AtomicU32>, rec: Arc<Mutex<Option<Vec<f32>>>>, on_level: impl Fn(f32) + Send + 'static) -> Option<(cpal::Stream, u32)> {
    let device = cpal::default_host().default_input_device()?;
    let config = device.default_input_config().ok()?;
    let (format, channels, rate) = (config.sample_format(), config.channels() as usize, config.sample_rate());
    let feed = move |mono: &mut dyn Iterator<Item = f32>| {
        let mut peak = 0f32;
        let mut rec = rec.lock().unwrap();
        for s in mono {
            peak = peak.max(s.abs());
            if let Some(v) = rec.as_mut() {
                v.push(s);
            }
        }
        let db = to_db(peak);
        level.store(db.to_bits(), Ordering::Relaxed);
        on_level(db);
    };
    let err = |e| eprintln!("reflux: microphone: {e}");
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_input_stream::<f32, _, _>(config.config(), move |d, _| feed(&mut d.chunks(channels).map(|f| f[0])), err, None),
        cpal::SampleFormat::I16 => {
            device.build_input_stream::<i16, _, _>(config.config(), move |d, _| feed(&mut d.chunks(channels).map(|f| f[0] as f32 / 32768.0)), err, None)
        }
        other => {
            eprintln!("reflux: microphone sample format {other:?} isn't supported");
            return None;
        }
    }
    .map_err(|e| eprintln!("reflux: microphone: {e}"))
    .ok()?;
    stream.play().ok()?;
    Some((stream, rate))
}

pub fn to_db(peak: f32) -> f32 {
    if peak <= 0.0 { FLOOR_DB } else { (20.0 * peak.log10()).max(FLOOR_DB) }
}

/// Whether to transmit, and the voice-activity state carried between blocks.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gate {
    /// When (ms) the signal last cleared the threshold, or the PTT key was last held.
    pub last_active: Option<u64>,
    /// The slow-moving noise floor the automatic threshold sits above (dB).
    pub floor: Option<f32>,
}

/// How the microphone is opened (`Keybind.transmitMode` + `VoiceSettings`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// `threshold` in dB; `auto` follows the noise floor instead.
    Activity { threshold: f32, auto: bool },
    /// Key held, then `release_ms` more.
    PushToTalk { held: bool, release_ms: u64 },
}

/// Speech keeps the gate open this long after the level drops (`VoiceActivityDetector`'s hangover).
const HOLD_MS: u64 = 400;
/// The automatic threshold sits this far over the measured noise floor.
const AUTO_MARGIN_DB: f32 = 15.0;

impl Gate {
    /// Feeds one level reading (dB) at `now` ms; true = transmit.
    pub fn step(&mut self, mode: Mode, level: f32, now: u64) -> bool {
        match mode {
            Mode::PushToTalk { held, release_ms } => {
                if held {
                    self.last_active = Some(now);
                }
                held || self.last_active.is_some_and(|t| now.saturating_sub(t) < release_ms)
            }
            Mode::Activity { threshold, auto } => {
                // The floor falls fast and rises slowly, so speech barely lifts it.
                let floor = self.floor.unwrap_or(level);
                let floor = if level < floor { level } else { floor + (level - floor) * 0.002 };
                self.floor = Some(floor);
                let threshold = if auto { (floor + AUTO_MARGIN_DB).clamp(-80.0, -20.0) } else { threshold };
                if level > threshold {
                    self.last_active = Some(now);
                }
                self.last_active.is_some_and(|t| now.saturating_sub(t) < HOLD_MS)
            }
        }
    }
}

/// `buildWaveformBytes`: one RMS point per 0.1 s (at most 256), normalised to the loudest and
/// square-rooted into 0..255; base64 as the attachment's `waveform`. Duration rounds to whole
/// seconds, at least 1.
pub fn waveform(samples: &[f32], rate: u32) -> (u32, String) {
    use base64::Engine;
    let secs = samples.len() as f32 / rate.max(1) as f32;
    let points = ((secs / 0.1).ceil() as usize).clamp(1, 256);
    let per = (samples.len() / points).max(1);
    let rms: Vec<f32> = (0..points)
        .map(|i| {
            let s = &samples[(i * per).min(samples.len())..((i + 1) * per).min(samples.len())];
            if s.is_empty() { 0.0 } else { (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt() }
        })
        .collect();
    let max = rms.iter().copied().fold(0f32, f32::max);
    let bytes: Vec<u8> = rms.iter().map(|r| if max <= 0.0 { 0 } else { ((r / max).clamp(0.0, 1.0).sqrt() * 255.0).round() as u8 }).collect();
    ((secs.round() as u32).max(1), base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Decodes an attachment's base64 `waveform` into 0..1 bars.
pub fn decode_waveform(b64: &str) -> Vec<f32> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(b64).map(|b| b.into_iter().map(|v| v as f32 / 255.0).collect()).unwrap_or_default()
}

/// Mono 16-bit PCM WAV (`encodeAudioBufferSliceToWav` with `downmixToMono`).
pub fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_gate_holds_then_closes() {
        let mut g = Gate::default();
        let m = Mode::Activity { threshold: -50.0, auto: false };
        assert!(!g.step(m, -70.0, 0));
        assert!(g.step(m, -30.0, 100));
        assert!(g.step(m, -70.0, 400), "hangover keeps it open");
        assert!(!g.step(m, -70.0, 600));
    }

    #[test]
    fn auto_threshold_sits_over_the_noise_floor() {
        let mut g = Gate::default();
        let m = Mode::Activity { threshold: 0.0, auto: true };
        for t in 0..50 {
            assert!(!g.step(m, -60.0, t * 20), "steady noise stays closed");
        }
        assert!(g.step(m, -35.0, 1000), "speech 25 dB over the floor opens");
    }

    #[test]
    fn ptt_release_delay() {
        let mut g = Gate::default();
        assert!(g.step(Mode::PushToTalk { held: true, release_ms: 200 }, FLOOR_DB, 0));
        assert!(g.step(Mode::PushToTalk { held: false, release_ms: 200 }, FLOOR_DB, 150));
        assert!(!g.step(Mode::PushToTalk { held: false, release_ms: 200 }, FLOOR_DB, 250));
        assert!(!Gate::default().step(Mode::PushToTalk { held: false, release_ms: 0 }, 0.0, 0));
    }

    #[test]
    fn waveform_matches_fluxer() {
        // 0.25 s at 1 kHz: 3 points; loud, silent, half.
        let mut s = vec![1.0; 83];
        s.extend(vec![0.0; 83]);
        s.extend(vec![0.25; 84]);
        let (secs, b64) = waveform(&s, 1000);
        assert_eq!(secs, 1, "rounds up to at least a second");
        let bars = decode_waveform(&b64);
        assert_eq!(bars.len(), 3);
        assert_eq!((bars[0], bars[1]), (1.0, 0.0));
        assert!((bars[2] - 0.5).abs() < 0.01, "sqrt(0.25) = 0.5");
    }

    #[test]
    fn wav_header() {
        let w = wav(&[0.0, 1.0, -1.0], 48_000);
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 48_000);
        assert_eq!(i16::from_le_bytes([w[46], w[47]]), 32767);
    }
}
