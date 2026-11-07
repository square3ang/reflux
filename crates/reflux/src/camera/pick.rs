//! OS-independent pieces of the Windows / macOS camera backends: which native mode to open and
//! which frames to keep for the configured frame rate (GStreamer does both itself on Linux).

use std::time::Duration;

/// The native mode (width, height, fps) to open for `want`: one that covers the size, then one
/// that reaches the frame rate, then the closest size (so 720p30 MJPG beats 720p10 YUY2).
#[cfg_attr(not(windows), allow(dead_code))] // macOS picks a session preset instead
pub fn mode(modes: &[(u32, u32, f64)], want: (u32, u32, u32)) -> Option<usize> {
    let target = want.0 as u64 * want.1 as u64;
    (0..modes.len()).min_by_key(|&i| {
        let (w, h, fps) = modes[i];
        (w < want.0 || h < want.1, fps + 0.5 < want.2 as f64, (w as u64 * h as u64).abs_diff(target))
    })
}

/// Drops frames that come faster than `fps` (by capture timestamp).
pub struct Pacer {
    interval: Duration,
    next: Option<Duration>,
}

impl Pacer {
    pub fn new(fps: u32) -> Self {
        // A little slack, so a 30 fps camera's jitter doesn't halve a 30 fps setting.
        Self { interval: Duration::from_secs_f64(0.9 / fps.max(1) as f64), next: None }
    }

    /// Whether the frame captured at `at` is kept.
    pub fn due(&mut self, at: Duration) -> bool {
        if self.next.is_some_and(|n| at < n) {
            return false;
        }
        self.next = Some(at + self.interval);
        true
    }
}

/// BGRA rows `stride` bytes apart (a CVPixelBuffer) to tight opaque RGBA.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn bgra_to_rgba(data: &[u8], w: usize, h: usize, stride: usize) -> Option<Vec<u8>> {
    if stride < w * 4 || data.len() < stride * h.saturating_sub(1) + w * 4 {
        return None;
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for r in 0..h {
        for px in data[r * stride..r * stride + w * 4].as_chunks::<4>().0 {
            out.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn bgra_rows() {
        // 2×2 with 4 bytes of row padding (the last row's may be missing).
        let data = [255, 0, 0, 7, 0, 255, 0, 7, 9, 9, 9, 9, 0, 0, 255, 7, 1, 2, 3, 7];
        assert_eq!(super::bgra_to_rgba(&data, 2, 2, 12).unwrap(), [0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 3, 2, 1, 255]);
        assert!(super::bgra_to_rgba(&data[..19], 2, 2, 12).is_none());
        assert!(super::bgra_to_rgba(&data, 4, 1, 12).is_none());
    }

    #[test]
    fn picks_covering_then_fps_then_closest() {
        let modes = [(640, 480, 30.0), (1280, 720, 10.0), (1280, 720, 30.0), (1920, 1080, 30.0)];
        assert_eq!(super::mode(&modes, (1280, 720, 30)), Some(2));
        assert_eq!(super::mode(&modes, (854, 480, 30)), Some(2));
        assert_eq!(super::mode(&modes, (1920, 1080, 60)), Some(3));
        // Nothing covers 4K: the biggest.
        assert_eq!(super::mode(&modes[..3], (3840, 2160, 30)), Some(2));
        // 29.97 counts as 30.
        assert_eq!(super::mode(&[(1280, 720, 15.0), (1280, 720, 29.97)], (1280, 720, 30)), Some(1));
        assert_eq!(super::mode(&[], (1280, 720, 30)), None);
    }

    #[test]
    fn pacer_halves_60_to_30() {
        let mut p = super::Pacer::new(30);
        let kept = (0..60).filter(|i| p.due(Duration::from_micros(i * 16_667))).count();
        assert_eq!(kept, 30);
        let mut p = super::Pacer::new(30);
        assert_eq!((0..30).filter(|i| p.due(Duration::from_micros(i * 33_333))).count(), 30);
    }
}
