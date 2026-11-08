//! The voice-activity gate on the mic path (`voiceGateProcessor.worklet.js`): it fades the
//! processed voice in within 5 ms of speech and out over 20 ms after 180 ms below the
//! threshold, 10 ms behind a lookahead, so word starts aren't cut. The automatic threshold
//! sits at 3× the noise floor (the 10th percentile of the last 2 s, ×1.5).

/// Samples per decision. The worklet's render quantum is 128; our 10 ms frames split evenly
/// into 120.
pub const BLOCK: usize = 120;
const RATE: f32 = 48_000.0;
const FLOOR_PERCENTILE: f32 = 0.1;
const FLOOR_BIAS: f32 = 1.5;
const FLOOR_WINDOW_SECONDS: f32 = 2.0;
const SEED_SECONDS: f32 = 0.3;
const LOOKAHEAD_SECONDS: f32 = 0.01;
const ATTACK_SECONDS: f32 = 0.005;
const RELEASE_SECONDS: f32 = 0.02;
/// `LOCAL_AUTO_MIN_RMS`, `LOCAL_MAX_RMS`, `SPEAKING_LOCAL_RELEASE_MS`.
const MIN_RMS: f32 = 0.000_003;
const MAX_RMS: f32 = 0.04;
const RELEASE_MS: f32 = 180.0;

/// How the gate decides: following the noise floor, or over a fixed RMS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateConfig {
    pub auto: bool,
    pub threshold_rms: f32,
}

pub struct Gate {
    delay: Vec<f32>,
    delay_at: usize,
    energy: [f64; 4],
    energy_at: usize,
    energy_count: usize,
    energy_sum: f64,
    histogram: [u16; 240],
    floor_bins: Vec<u8>,
    floor_at: usize,
    floor_count: usize,
    silence: usize,
    frame: usize,
    seed_until: usize,
    gain: f32,
    speaking: bool,
}

impl Gate {
    pub fn new(auto: bool) -> Self {
        Self {
            delay: vec![0.0; (LOOKAHEAD_SECONDS * RATE).round() as usize],
            delay_at: 0,
            energy: [0.0; 4],
            energy_at: 0,
            energy_count: 0,
            energy_sum: 0.0,
            histogram: [0; 240],
            floor_bins: vec![0; (FLOOR_WINDOW_SECONDS * RATE / BLOCK as f32).ceil() as usize],
            floor_at: 0,
            floor_count: 0,
            silence: 0,
            frame: 0,
            seed_until: (SEED_SECONDS * RATE) as usize,
            gain: if auto { 1.0 } else { 0.0 },
            speaking: false,
        }
    }

    /// Whether the gate is open for speech.
    #[cfg(test)]
    pub fn speaking(&self) -> bool {
        self.speaking
    }

    /// `trackFloor`: the noise floor RMS from a histogram of block levels (half-dB bins).
    fn track_floor(&mut self, rms: f32) -> f32 {
        let bin = ((20.0 * rms.max(1e-6).log10() + 120.0) * 2.0).floor().clamp(0.0, 239.0) as u8;
        if self.floor_count == self.floor_bins.len() {
            self.histogram[self.floor_bins[self.floor_at] as usize] -= 1;
        } else {
            self.floor_count += 1;
        }
        self.floor_bins[self.floor_at] = bin;
        self.floor_at = (self.floor_at + 1) % self.floor_bins.len();
        self.histogram[bin as usize] += 1;
        let target = (self.floor_count as f32 * FLOOR_PERCENTILE).ceil() as u32;
        let (mut count, mut floor) = (0u32, 0usize);
        while floor < 239 {
            count += u32::from(self.histogram[floor]);
            if count >= target {
                break;
            }
            floor += 1;
        }
        (10f32.powf((floor as f32 / 2.0 - 120.0) / 20.0) * FLOOR_BIAS).clamp(MIN_RMS / 3.0, MAX_RMS)
    }

    /// Gates `samples` ([-1, 1], 48 kHz mono) in place, in [`BLOCK`]s; a shorter tail is a
    /// block of its own.
    pub fn process(&mut self, cfg: GateConfig, samples: &mut [f32]) {
        let release_frames = (RELEASE_MS / 1000.0 * RATE).round() as usize;
        let attack_step = 1.0 / (ATTACK_SECONDS * RATE);
        let release_factor = (-1.0 / (RELEASE_SECONDS * RATE)).exp();
        for block in samples.chunks_mut(BLOCK) {
            let len = block.len();
            let energy = block.iter().map(|&x| if x.is_finite() { f64::from(x * x) } else { 0.0 }).sum::<f64>() / len.max(1) as f64;
            self.energy_sum += energy - self.energy[self.energy_at];
            self.energy[self.energy_at] = energy;
            self.energy_at = (self.energy_at + 1) % self.energy.len();
            self.energy_count = (self.energy_count + 1).min(self.energy.len());
            let rms = (self.energy_sum / self.energy_count as f64).max(0.0).sqrt() as f32;
            let floor = self.track_floor(rms);
            let threshold = if cfg.auto { (floor * 3.0).clamp(MIN_RMS, MAX_RMS) } else { cfg.threshold_rms };
            let seeding = cfg.auto && self.frame < self.seed_until;
            if seeding {
                self.speaking = false;
                self.silence = 0;
            } else if energy.sqrt() as f32 >= threshold {
                self.speaking = true;
                self.silence = 0;
            } else if rms < threshold * 0.7 {
                self.silence += len;
                if self.silence >= release_frames {
                    self.speaking = false;
                }
            } else {
                self.silence = 0;
            }
            let open = self.speaking || seeding;
            for x in block.iter_mut() {
                self.gain = if open { (self.gain + attack_step).min(1.0) } else { self.gain * release_factor };
                let input = if x.is_finite() { *x } else { 0.0 };
                *x = self.delay[self.delay_at] * self.gain;
                self.delay[self.delay_at] = input;
                self.delay_at = (self.delay_at + 1) % self.delay.len();
            }
            self.frame += len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(level: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| level * (i as f32 * 0.1).sin()).collect()
    }

    #[test]
    fn opens_on_speech_without_cutting_its_start_and_closes_after_the_hold() {
        let cfg = GateConfig { auto: false, threshold_rms: 0.01 };
        let mut g = Gate::new(false);
        let mut quiet = tone(0.001, 4800);
        g.process(cfg, &mut quiet);
        assert!(!g.speaking() && quiet.iter().all(|x| x.abs() < 1e-4), "closed under the threshold");
        let mut speech = tone(0.2, 4800);
        g.process(cfg, &mut speech);
        assert!(g.speaking());
        // The 10 ms lookahead delays the voice; it arrives at (nearly) full level.
        let peak = speech[960..1440].iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.19, "{peak}");
        // 180 ms of quiet holds it open, then it fades.
        let mut tail = tone(0.0, 48 * 150);
        g.process(cfg, &mut tail);
        assert!(g.speaking(), "held");
        let mut tail = tone(0.0, 48 * 100);
        g.process(cfg, &mut tail);
        assert!(!g.speaking(), "released");
    }

    #[test]
    fn auto_follows_the_noise_floor() {
        let cfg = GateConfig { auto: true, threshold_rms: 0.0 };
        let mut g = Gate::new(true);
        // Seeding (0.3 s) passes everything; then steady noise stays closed.
        let mut noise = tone(0.003, 48_000);
        g.process(cfg, &mut noise);
        assert!(!g.speaking());
        let mut speech = tone(0.1, 4800);
        g.process(cfg, &mut speech);
        assert!(g.speaking(), "speech well over 3× the floor opens it");
    }
}
