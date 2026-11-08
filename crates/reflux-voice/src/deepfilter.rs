//! DeepFilterNet3 on the mic path (`DeepFilter.ts` + `deepFilterProcessor.worklet.js`): the
//! bundled model, a 30 dB attenuation limit, 10 ms hops at 48 kHz, a 60 Hz high-pass and ×10 in /
//! ×0.1 out around it, 8 warm-up frames. The model loads on a thread; until it runs (or after it
//! fails) the caller falls back to RNNoise. Once running, every frame goes through it: swapping
//! single frames to another denoiser breaks both models' state and crackles.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use df::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::{ArrayView2, ArrayViewMut2};

/// `DEEP_FILTER_ATTEN_LIM_DB`.
const ATTEN_LIM_DB: f32 = 30.0;
/// `DEEP_FILTER_INPUT_GAIN` / `DEEP_FILTER_OUTPUT_GAIN`.
const INPUT_GAIN: f32 = 10.0;
const OUTPUT_GAIN: f32 = 0.1;
/// `DEEP_FILTER_HIGH_PASS_HZ`.
const HIGH_PASS_HZ: f32 = 60.0;
/// `WARM_UP_FRAMES`.
const WARM_UP_FRAMES: usize = 8;
/// A frame takes ~1 ms; one this late means the model is stuck: give up on it (RNNoise).
const STUCK: Duration = Duration::from_millis(200);

/// Frames out to the model's thread and back.
type Worker = (Sender<Vec<f32>>, Receiver<Vec<f32>>);

/// The model isn't `Send` (tract keeps `Rc`s), so it lives on its own thread and frames make a
/// round trip to it.
pub struct Deep {
    state: Arc<AtomicU8>,
    worker: Option<Worker>,
}

impl Deep {
    pub fn new(state: Arc<AtomicU8>) -> Self {
        Self { state, worker: None }
    }

    /// Denoises one 480-sample frame of [-1, 1] floats into `out`; false (and `out` untouched)
    /// while the model loads, after it failed, or when the frame misses its deadline.
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) -> bool {
        let state = self.state.clone();
        let (tx, rx) = self.worker.get_or_insert_with(|| spawn(state));
        if self.state.load(Ordering::Relaxed) != 2 {
            return false;
        }
        // Drop anything a late frame left behind.
        while rx.try_recv().is_ok() {}
        if tx.send(input.to_vec()).is_err() {
            return false;
        }
        match rx.recv_timeout(STUCK) {
            Ok(f) if f.len() == out.len() => {
                out.copy_from_slice(&f);
                true
            }
            Ok(_) | Err(RecvTimeoutError::Disconnected) => false,
            Err(RecvTimeoutError::Timeout) => {
                eprintln!("reflux-voice: DeepFilterNet stalled; using RNNoise");
                self.state.store(3, Ordering::Relaxed);
                false
            }
        }
    }
}

fn spawn(state: Arc<AtomicU8>) -> Worker {
    state.store(1, Ordering::Relaxed);
    let (in_tx, in_rx) = channel::<Vec<f32>>();
    let (out_tx, out_rx) = channel();
    std::thread::spawn(move || {
        let rp = RuntimeParams::default_with_ch(1).with_atten_lim(ATTEN_LIM_DB);
        let mut model = match DfTract::new(DfParams::default(), &rp) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("reflux-voice: DeepFilterNet failed to load: {e:#}");
                state.store(3, Ordering::Relaxed);
                return;
            }
        };
        let hop = model.hop_size;
        let mut run = |frame: &[f32], out: &mut [f32]| -> bool {
            frame.len() == hop
                && match (ArrayView2::from_shape((1, frame.len()), frame), ArrayViewMut2::from_shape((1, out.len()), out)) {
                    (Ok(i), Ok(o)) => model.process(i, o).is_ok(),
                    _ => false,
                }
        };
        let mut scratch = vec![0.0; hop];
        for _ in 0..WARM_UP_FRAMES {
            run(&vec![0.0; hop], &mut scratch);
        }
        let mut hp = HighPass::new(HIGH_PASS_HZ, 48_000.0);
        state.store(2, Ordering::Relaxed);
        for mut frame in in_rx {
            for x in frame.iter_mut() {
                *x = hp.step(if x.is_finite() { *x } else { 0.0 }) * INPUT_GAIN;
            }
            let mut out = vec![0.0; frame.len()];
            if !run(&frame, &mut out) {
                return;
            }
            for x in out.iter_mut() {
                *x = if x.is_finite() { *x * OUTPUT_GAIN } else { 0.0 };
            }
            if out_tx.send(out).is_err() {
                return;
            }
        }
    });
    (in_tx, out_rx)
}

/// `createHighPass`: an RBJ biquad high-pass, Q = 1/√2.
struct HighPass {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl HighPass {
    fn new(cutoff: f32, rate: f32) -> Self {
        let w = std::f32::consts::TAU * cutoff / rate;
        let alpha = w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let (cos, a0) = (w.cos(), 1.0 + alpha);
        let b0 = (1.0 + cos) / 2.0 / a0;
        Self { b: [b0, -(1.0 + cos) / a0, b0], a: [-2.0 * cos / a0, (1.0 - alpha) / a0], x: [0.0; 2], y: [0.0; 2] }
    }

    fn step(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

#[cfg(test)]
mod tests {
    use super::HighPass;

    #[test]
    fn high_pass_cuts_dc_and_keeps_voice() {
        let mut hp = HighPass::new(60.0, 48_000.0);
        let dc: Vec<f32> = (0..48_000).map(|_| hp.step(0.5)).collect();
        assert!(dc[47_999].abs() < 1e-3, "DC removed");
        let mut hp = HighPass::new(60.0, 48_000.0);
        let tone: Vec<f32> = (0..48_000).map(|i| hp.step((std::f32::consts::TAU * 300.0 * i as f32 / 48_000.0).sin())).collect();
        let peak = tone[40_000..].iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.95, "300 Hz passes: {peak}");
    }
}
