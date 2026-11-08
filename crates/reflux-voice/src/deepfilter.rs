//! DeepFilterNet3 on the mic path (`DeepFilter.ts`): the bundled model, a 30 dB attenuation
//! limit, 10 ms hops at 48 kHz. The model loads on a thread; until it runs the caller falls
//! back to RNNoise.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use df::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::{ArrayView2, ArrayViewMut2};

/// `DEEP_FILTER_ATTEN_LIM_DB`.
const ATTEN_LIM_DB: f32 = 30.0;
/// A frame that isn't back by then goes through RNNoise instead (10 ms frames).
const DEADLINE: Duration = Duration::from_millis(8);

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
        match rx.recv_timeout(DEADLINE) {
            Ok(f) if f.len() == out.len() => {
                out.copy_from_slice(&f);
                true
            }
            Ok(_) | Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => false,
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
        state.store(2, Ordering::Relaxed);
        for frame in in_rx {
            let mut out = vec![0.0; frame.len()];
            let ok = frame.len() == model.hop_size
                && match (ArrayView2::from_shape((1, frame.len()), &frame[..]), ArrayViewMut2::from_shape((1, out.len()), &mut out[..])) {
                    (Ok(i), Ok(o)) => model.process(i, o).is_ok(),
                    _ => false,
                };
            if !ok || out_tx.send(out).is_err() {
                return;
            }
        }
    });
    (in_tx, out_rx)
}
