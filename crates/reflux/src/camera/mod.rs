//! Camera capture, one interface per OS: [`available`], [`cameras`] (display names for the
//! Voice & video select) and [`CameraCapture::start`] (device, size, fps, RGBA frame sink; drop to
//! stop). GStreamer on Linux (`gst.rs`), Media Foundation on Windows (`mf.rs` here), AVFoundation
//! on macOS (`av.rs`).

pub mod pick;

#[cfg(target_os = "macos")]
mod av;
#[cfg(windows)]
mod mf;

#[cfg(target_os = "macos")]
pub use av::{CameraCapture, available, cameras};
#[cfg(not(any(windows, target_os = "macos")))]
pub use crate::gst::{CameraCapture, available, cameras};
#[cfg(windows)]
pub use mf::{CameraCapture, available, cameras};

/// Needs a real camera: `cargo test -p reflux camera::real -- --ignored --nocapture`.
#[cfg(test)]
mod real {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    #[test]
    #[ignore]
    fn captures_at_the_asked_size_and_rate() {
        let cams = super::cameras();
        eprintln!("cameras: {cams:?}");
        let frames: Arc<Mutex<(u32, u32, u32)>> = Arc::default();
        let f = frames.clone();
        let cap = super::CameraCapture::start(cams.first().map(String::as_str), 640, 480, 15, move |w, h, rgba| {
            assert_eq!(rgba.len(), (w * h * 4) as usize);
            let mut f = f.lock().unwrap();
            *f = (f.0 + 1, w, h);
        })
        .expect("camera opens");
        std::thread::sleep(Duration::from_secs(3));
        let t = Instant::now();
        let n0 = frames.lock().unwrap().0;
        std::thread::sleep(Duration::from_secs(2));
        let (n, w, h) = *frames.lock().unwrap();
        let fps = (n - n0) as f64 / t.elapsed().as_secs_f64();
        eprintln!("{w}x{h} at {fps:.1} fps");
        assert!(n > 0 && (fps - 15.0).abs() < 1.5, "{n} frames, {fps} fps");
        drop(cap);
    }
}
