//! Windowed-sinc rate conversion, shared by the real audio path and the no-WebRTC stub (voice
//! message decoding in `reflux`).

/// Windowed-sinc rate converter for a mono stream (the browser's WebAudio resampling, which
/// Fluxer's playout and capture go through): 16 taps each side under a Blackman window, the
/// cutoff lowered to the output's Nyquist when downsampling, from a 512-phase table.
#[derive(Debug, Clone)]
pub struct Resampler {
    step: f64,
    /// Read position in `buf`; output `n` is at `pos + n * step`.
    pos: f64,
    /// The last `2 * HALF` input samples and the new ones.
    buf: Vec<f32>,
    /// `PHASES + 1` rows of `2 * HALF` weights (the last row repeats the first, shifted).
    table: std::sync::Arc<Vec<f32>>,
}

const HALF: usize = 16;
const PHASES: usize = 512;

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        let step = from as f64 / to as f64;
        // `SincResampler`: the cutoff scaled by 0.9 (and by the ratio when downsampling).
        let cutoff = (1.0 / step).min(1.0) * 0.9;
        let taps = 2 * HALF;
        let mut table = vec![0.0f32; (PHASES + 1) * taps];
        for ph in 0..=PHASES {
            let frac = ph as f64 / PHASES as f64;
            let row = &mut table[ph * taps..(ph + 1) * taps];
            let mut sum = 0.0;
            for (j, w) in row.iter_mut().enumerate() {
                // Tap j is input sample floor(x) - HALF + 1 + j, at distance t from x.
                let t = frac + HALF as f64 - 1.0 - j as f64;
                let u = t / HALF as f64;
                let x = std::f64::consts::PI * cutoff * t;
                let sinc = if x.abs() < 1e-9 { 1.0 } else { x.sin() / x };
                let win = if u.abs() >= 1.0 { 0.0 } else { 0.42 + 0.5 * (std::f64::consts::PI * u).cos() + 0.08 * (2.0 * std::f64::consts::PI * u).cos() };
                let v = cutoff * sinc * win;
                *w = v as f32;
                sum += v;
            }
            // Unity gain at DC for every phase.
            for w in row.iter_mut() {
                *w = (*w as f64 / sum) as f32;
            }
        }
        Self { step, pos: HALF as f64, buf: vec![0.0; HALF], table: std::sync::Arc::new(table) }
    }

    pub fn passthrough(&self) -> bool {
        self.step == 1.0
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.passthrough() {
            return out.extend_from_slice(input);
        }
        self.buf.extend_from_slice(input);
        let taps = 2 * HALF;
        while (self.pos.floor() as usize) + HALF < self.buf.len() {
            let i = self.pos.floor() as usize;
            let ph = ((self.pos - i as f64) * PHASES as f64).round() as usize;
            let w = &self.table[ph * taps..(ph + 1) * taps];
            let x = &self.buf[i + 1 - HALF..=i + HALF];
            out.push(x.iter().zip(w).map(|(a, b)| a * b).sum());
            self.pos += self.step;
        }
        // Keep what the next output still needs.
        let keep_from = (self.pos.floor() as usize + 1).saturating_sub(HALF).min(self.buf.len());
        self.buf.drain(..keep_from);
        self.pos -= keep_from as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples() {
        let mut r = Resampler::new(24_000, 48_000);
        let mut out = Vec::new();
        r.process(&[1.0, 1.0], &mut out);
        r.process(&[1.0, 1.0], &mut out);
        r.process(&[1.0; 64], &mut out);
        // Twice the samples, less the 16-sample lookahead still held back.
        assert_eq!(out.len(), 2 * 68 - 2 * HALF, "{}", out.len());
        assert!(out[2 * HALF + 8..].iter().all(|&s| (s - 1.0).abs() < 1e-3), "a steady signal keeps its level");
        let mut down = Resampler::new(48_000, 44_100);
        let mut out = Vec::new();
        down.process(&[0.0; 4800], &mut out);
        assert!((4394..=4411).contains(&out.len()), "{}", out.len());
        // A 23 kHz tone is above 44.1 kHz's Nyquist: filtered out, not aliased. 1 kHz passes.
        let tone = |f: f32| (0..48_000).map(|n| (2.0 * std::f32::consts::PI * f * n as f32 / 48_000.0).sin()).collect::<Vec<f32>>();
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        let mut out = Vec::new();
        Resampler::new(48_000, 44_100).process(&tone(23_000.0), &mut out);
        assert!(rms(&out[1000..]) < 0.05, "aliased: {}", rms(&out[1000..]));
        let mut out = Vec::new();
        Resampler::new(48_000, 44_100).process(&tone(1_000.0), &mut out);
        assert!((rms(&out[1000..]) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01, "{}", rms(&out[1000..]));
    }
}
