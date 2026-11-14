//! Screen-share audio, per platform. Linux captures one PipeWire application node (`gst.rs`,
//! `AudioSourcePickerLinux`); Windows captures the default render device with WASAPI loopback;
//! macOS captures the system mix with a ScreenCaptureKit audio stream. Every path hands
//! interleaved stereo f32 at 48 kHz to the same `reflux_voice::StreamAudio` track.
//!
//! The macOS path needs the Screen Recording permission (TCC); without it `SCStream` fails to
//! start and the caller continues with a video-only share. A sandboxed build also needs the
//! `com.apple.security.device.screen-capture` and `com.apple.security.device.audio-input`
//! entitlements (Fluxer ships both).

/// The picker/settings id for the whole system mix (on Linux: every app but this one).
pub const SYSTEM_ID: &str = "system";
/// The mixer's sample rate; `StreamAudio::push` takes samples at this rate.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub const RATE: u32 = 48_000;

/// Whether this platform can capture screen-share audio at all.
pub fn available() -> bool {
    #[cfg(target_os = "linux")]
    {
        crate::gst::available()
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        true
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        false
    }
}

/// Picker entries: `(setting id, label)`. Linux lists PipeWire applications; Windows and macOS
/// offer the system mix.
pub fn sources() -> Vec<(String, String)> {
    #[cfg(target_os = "linux")]
    {
        // `AudioSourcePickerLinux`: "Entire system audio" above the apps. Off the UI thread, so
        // it waits for GStreamer's init rather than caching an empty list made before it ends.
        if crate::gst::ready() { std::iter::once((SYSTEM_ID.to_string(), crate::i18n::t("Entire system audio").to_string())).chain(crate::gst::audio_apps()).collect() } else { Vec::new() }
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        vec![(SYSTEM_ID.to_string(), "Capture entire system audio".to_string())]
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        Vec::new()
    }
}

/// Starts the capture named by `id`: a PipeWire `object.serial` on Linux, [`SYSTEM_ID`] on
/// Windows/macOS. `None` when the platform cannot start it (no device, no permission); the
/// caller keeps the video share.
pub fn start(id: &str, sink: impl FnMut(&[f32]) + Send + 'static) -> Option<Capture> {
    #[cfg(target_os = "linux")]
    {
        crate::gst::AudioCapture::start(id, sink)
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        (id == SYSTEM_ID).then(|| platform::SystemAudio::start(sink)).flatten()
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        let _ = (id, sink);
        None
    }
}

/// Sample encodings WASAPI's shared mix can hand back (little-endian).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum SampleFormat {
    F32,
    I16,
    I32,
}

#[allow(dead_code)]
impl SampleFormat {
    pub fn size(self) -> usize {
        match self {
            Self::F32 | Self::I32 => 4,
            Self::I16 => 2,
        }
    }
}

/// Converts interleaved little-endian PCM to f32 (-1..1).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn le_to_f32(bytes: &[u8], format: SampleFormat) -> Vec<f32> {
    match format {
        SampleFormat::F32 => bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect(),
        SampleFormat::I16 => bytes.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b) as f32 / 32768.0).collect(),
        SampleFormat::I32 => bytes.as_chunks::<4>().0.iter().map(|b| i32::from_le_bytes(*b) as f32 / 2_147_483_648.0).collect(),
    }
}

/// Interleaves planar channel buffers. `frames` is the frame count in each plane.
#[allow(dead_code)]
pub fn interleave(planes: &[&[f32]], frames: usize, channels: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * channels);
    for frame in 0..frames {
        for plane in planes.iter().take(channels) {
            out.push(plane.get(frame).copied().unwrap_or(0.0));
        }
    }
    out
}

/// Streaming linear resampler from `rate`/`channels` to 48 kHz stereo. It keeps one input frame
/// of history, so the output lags the input by at most one frame.
#[derive(Clone, Debug)]
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub struct Resampler {
    rate: u32,
    channels: usize,
    /// Fractional position between `prev` and the next input frame, in input frames.
    frac: f64,
    prev: Vec<f32>,
    started: bool,
}

#[allow(dead_code)]
impl Resampler {
    pub fn new(rate: u32, channels: usize) -> Self {
        Self { rate: rate.max(1), channels: channels.max(1), frac: 0.0, prev: Vec::new(), started: false }
    }

    pub fn push(&mut self, input: &[f32]) -> Vec<f32> {
        let frames = input.len() / self.channels;
        if frames == 0 {
            return Vec::new();
        }
        if self.rate == RATE {
            let mut out = Vec::with_capacity(frames * 2);
            for frame in input.chunks_exact(self.channels) {
                let (l, r) = stereo(frame);
                out.extend_from_slice(&[l, r]);
            }
            return out;
        }
        let step = self.rate as f64 / RATE as f64;
        let mut out = Vec::with_capacity((frames as f64 / step).ceil() as usize * 2 + 2);
        for frame in input.chunks_exact(self.channels) {
            if !self.started {
                self.prev = frame.to_vec();
                self.started = true;
                continue;
            }
            while self.frac < 1.0 {
                let (pl, pr) = stereo(&self.prev);
                let (cl, cr) = stereo(frame);
                let t = self.frac as f32;
                out.push(pl + (cl - pl) * t);
                out.push(pr + (cr - pr) * t);
                self.frac += step;
            }
            self.frac -= 1.0;
            self.prev.copy_from_slice(frame);
        }
        out
    }
}

/// First two channels as stereo; mono is duplicated.
// shortcut: first two channels; downmix when a 5.1 user reports a missing centre channel.
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
fn stereo(frame: &[f32]) -> (f32, f32) {
    match frame {
        [m] => (*m, *m),
        [l, r, ..] => (*l, *r),
        [] => (0.0, 0.0),
    }
}

#[cfg(target_os = "linux")]
mod platform {
    pub use crate::gst::AudioCapture as Capture;
}

#[cfg(any(windows, target_os = "macos"))]
mod platform {
    #[cfg(windows)]
    pub use super::windows::SystemAudio;
    #[cfg(target_os = "macos")]
    pub use super::macos::SystemAudio;
    pub type Capture = SystemAudio;
}

pub(crate) use platform::Capture;

#[cfg(windows)]
mod windows;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_formats_convert_to_f32() {
        assert_eq!(le_to_f32(&[0, 0, 0x80, 0x3f], SampleFormat::F32), vec![1.0]);
        assert_eq!(le_to_f32(&[0xff, 0x7f, 0, 0x80], SampleFormat::I16), vec![0.9999695, -1.0]);
        assert_eq!(le_to_f32(&[0, 0, 0, 0x40], SampleFormat::I32), vec![0.5]);
    }

    #[test]
    fn interleaves_planar_stereo() {
        let l = [0.1, 0.2];
        let r = [-0.1, -0.2];
        assert_eq!(interleave(&[&l, &r], 2, 2), vec![0.1, -0.1, 0.2, -0.2]);
    }

    #[test]
    fn stereo_48k_passes_through() {
        let mut r = Resampler::new(RATE, 2);
        assert_eq!(r.push(&[0.1, -0.1, 0.2, -0.2]), vec![0.1, -0.1, 0.2, -0.2]);
    }

    #[test]
    fn mono_is_duplicated_and_upsampled() {
        let mut r = Resampler::new(24_000, 1);
        // The first frame is the interpolation anchor, then 0.0 and 0.5 are emitted.
        assert_eq!(r.push(&[0.0, 1.0]), vec![0.0, 0.0, 0.5, 0.5]);
        // The next input frame finishes the ramp: 1.0, then the next anchor 2.0 starts.
        assert_eq!(r.push(&[2.0]), vec![1.0, 1.0, 1.5, 1.5]);
    }

    #[test]
    fn downsample_keeps_the_rate_ratio() {
        let mut r = Resampler::new(96_000, 1);
        let out = r.push(&[0.0, 1.0, 2.0, 3.0, 4.0]);
        assert_eq!(out, vec![0.0, 0.0, 2.0, 2.0]);
        let mut r = Resampler::new(44_100, 1);
        let out = r.push(&[0.1; 4410]);
        // 4410 input frames (one anchor) at 44.1 kHz are about 4799 stereo output frames.
        assert!((out.len() / 2).abs_diff(4800) <= 3, "{} frames", out.len() / 2);
    }
}
