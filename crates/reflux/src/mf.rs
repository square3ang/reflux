//! Media Foundation playback on Windows (built into the OS, nothing to install): the same
//! `VideoPlayer` interface as the GStreamer one in `gst.rs`. A decode thread reads the clip with
//! an `IMFSourceReader` (RGB32 frames through its video processor, float PCM), paces samples by
//! their timestamps against a wall clock, hands frames to the sink and sound to rodio.
//! Self-contained (std, `windows`, `rodio`) so `scripts/mf-wine-test.sh` can build it alone.

use std::num::NonZero;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::{GUID, HSTRING};

pub(crate) const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const AUDIO: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
/// How far ahead of the clock sound is queued once it plays (rodio plays what it gets at once).
const AUDIO_LEAD: f64 = 0.5;

pub fn available() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(|e| eprintln!("reflux: media foundation: {e}")).is_ok())
}

/// The media position: `base` at `since` (running), advancing at `rate`.
struct Clock {
    base: f64,
    since: Option<Instant>,
    rate: f64,
}

impl Clock {
    fn now(&self) -> f64 {
        self.base + self.since.map_or(0.0, |s| s.elapsed().as_secs_f64() * self.rate)
    }
    fn set(&mut self, base: f64, running: bool) {
        (self.base, self.since) = (base, running.then(Instant::now));
    }
}

struct Shared {
    clock: Clock,
    duration: Option<f64>,
    /// Set by the decode thread, taken by `poll`.
    ended: bool,
    failed: bool,
    /// Decoded audio samples so far (the wine test checks sound decodes without a device).
    audio_samples: u64,
}

enum Cmd {
    Play,
    Pause,
    Seek(f64),
    Rate(f64),
    Volume(f32),
}

/// One clip: frames to the sink, sound to the default output.
pub struct VideoPlayer {
    tx: Sender<Cmd>,
    shared: Arc<Mutex<Shared>>,
    pub ended: bool,
    pub playing: bool,
}

impl VideoPlayer {
    pub fn new(url: &str, sink: impl FnMut(u32, u32, Vec<u8>) + Send + 'static) -> Option<Self> {
        if !available() {
            return None;
        }
        let shared = Arc::new(Mutex::new(Shared { clock: Clock { base: 0.0, since: None, rate: 1.0 }, duration: None, ended: false, failed: false, audio_samples: 0 }));
        let (tx, rx) = channel();
        let (url, sh) = (url.to_string(), shared.clone());
        std::thread::Builder::new().name("reflux-video".into()).spawn(move || run(&url, rx, sh, sink)).ok()?;
        Some(Self { tx, shared, ended: false, playing: false })
    }

    pub fn play(&mut self) {
        if self.ended {
            self.seek(0.0);
            self.ended = false;
        }
        let _ = self.tx.send(Cmd::Play);
        self.playing = true;
    }

    pub fn pause(&mut self) {
        let _ = self.tx.send(Cmd::Pause);
        self.playing = false;
    }

    pub fn seek(&self, secs: f64) {
        let _ = self.tx.send(Cmd::Seek(secs.max(0.0)));
    }

    /// Plays at `rate`.
    // shortcut: rodio's speed shifts the pitch (no `preservesPitch` like playbin's scaletempo);
    // upgrade to a time-stretch source if 1.5×/2× voice messages sound wrong to someone.
    pub fn set_rate(&mut self, rate: f64) {
        let _ = self.tx.send(Cmd::Rate(rate));
    }

    /// `volume` 0..1 (cubic, like the web player's slider), `muted` on top.
    pub fn set_volume(&self, volume: f64, muted: bool) {
        let v = if muted { 0.0 } else { volume.clamp(0.0, 1.0).powi(3) };
        let _ = self.tx.send(Cmd::Volume(v as f32));
    }

    pub fn position(&self) -> Option<f64> {
        let s = self.shared.lock().ok()?;
        let now = s.clock.now();
        Some(s.duration.map_or(now, |d| now.min(d)))
    }

    pub fn duration(&self) -> Option<f64> {
        self.shared.lock().ok()?.duration
    }

    #[allow(dead_code)] // the wine test's check
    pub fn audio_samples(&self) -> u64 {
        self.shared.lock().map_or(0, |s| s.audio_samples)
    }

    /// True when something changed (the clip ended or failed).
    pub fn poll(&mut self) -> bool {
        let Ok(mut s) = self.shared.lock() else { return false };
        let (ended, failed) = (std::mem::take(&mut s.ended), std::mem::take(&mut s.failed));
        if ended {
            self.ended = true;
        }
        if ended || failed {
            self.playing = false;
        }
        ended || failed
    }
}

#[derive(Clone, Copy)]
pub(crate) struct VideoFormat {
    h: u32,
    stride: i32,
    /// The picture inside the decoded frame: x, y, width, height.
    pub(crate) crop: (u32, u32, u32, u32),
}

pub(crate) struct Sample {
    stream: u32,
    pub(crate) ts: f64,
    end: f64,
    pub(crate) sample: IMFSample,
}

fn run(url: &str, rx: Receiver<Cmd>, shared: Arc<Mutex<Shared>>, sink: impl FnMut(u32, u32, Vec<u8>)) {
    // shortcut: readers open and close one at a time, and an E_UNEXPECTED open is retried: under
    // wine concurrent opens deadlock and an open right after a reader closed fails (wine-tested);
    // costs little on Windows, drop both once a real Windows run shows they're not needed there.
    static OPEN: Mutex<()> = Mutex::new(());
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let opened = {
        let _one = OPEN.lock();
        let mut tries = 0;
        loop {
            match unsafe { open(url) } {
                Err(e) if e.code() == windows::Win32::Foundation::E_UNEXPECTED && tries < 5 => tries += 1,
                r => break r,
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    };
    match opened {
        Ok(reader) => {
            if let Err(e) = pump(&reader, rx, &shared, sink) {
                eprintln!("reflux: video: {e}");
                shared.lock().map(|mut s| s.failed = true).ok();
            }
            let _one = OPEN.lock();
            drop(reader);
        }
        Err(e) => {
            eprintln!("reflux: video: open {url}: {e}");
            shared.lock().map(|mut s| s.failed = true).ok();
        }
    }
}

/// Plays `reader` until the player is dropped.
fn pump(reader: &IMFSourceReader, rx: Receiver<Cmd>, shared: &Mutex<Shared>, mut sink: impl FnMut(u32, u32, Vec<u8>)) -> windows::core::Result<()> {
    let duration = unsafe { reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION) }.ok().and_then(|v| u64::try_from(&v).ok());
    shared.lock().unwrap().duration = duration.map(|d| d as f64 / 1e7);
    let mut video = unsafe { video_format(reader) };
    let audio_format = unsafe { audio_format(reader) };
    let out = rodio::DeviceSinkBuilder::open_default_sink().map_err(|e| eprintln!("reflux: video sound output: {e}")).ok().map(|mut s| {
        s.log_on_drop(false);
        s
    });
    let (mut volume, mut rate) = (1.0f32, 1.0f64);
    let new_audio = |running: bool, volume: f32, rate: f64| {
        out.as_ref().map(|o| {
            let p = rodio::Player::connect_new(o.mixer());
            (p.set_volume(volume), p.set_speed(rate as f32));
            if !running {
                p.pause();
            }
            p
        })
    };
    let mut audio = new_audio(false, volume, rate);
    // Pending sample per stream; a stream is done once it reports its end.
    let mut pending: [Option<Sample>; 2] = [None, None];
    let mut done = [video.is_none(), audio_format.is_none()];
    let (mut end, mut preroll, mut ended) = (0.0f64, true, false);
    loop {
        let running = shared.lock().unwrap().clock.since.is_some();
        for (i, stream) in [VIDEO, AUDIO].into_iter().enumerate() {
            if pending[i].is_none() && !done[i] {
                match unsafe { read(reader, stream) } {
                    Ok((flags, s)) => {
                        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                            done[i] = true;
                        }
                        if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 && stream == VIDEO {
                            video = unsafe { video_format(reader) };
                        }
                        pending[i] = s;
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        // A gap (stream tick) gave no sample: read again.
        if (0..2).any(|i| pending[i].is_none() && !done[i]) {
            continue;
        }
        // The next sample due and when; sound goes out ahead once some is queued.
        let lead = |s: &Sample| if s.stream == AUDIO && audio.as_ref().is_some_and(|p| !p.empty()) { AUDIO_LEAD } else { 0.0 };
        let now = shared.lock().unwrap().clock.now();
        // Paused after opening or a seek: show that frame.
        let preroll_now = preroll && !running && pending[0].is_some();
        let next = match preroll_now {
            true => Some((0, now)),
            false => pending.iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|s| (i, s.ts - lead(s)))).min_by(|a, b| a.1.total_cmp(&b.1)),
        };
        let wait = match next {
            Some(_) if preroll_now => Some(0.0),
            Some((_, due)) if running => Some((due - now) / rate),
            None if running && !ended && done.iter().all(|d| *d) => {
                if now >= end {
                    ended = true;
                    let mut s = shared.lock().unwrap();
                    (s.ended, s.clock) = (true, Clock { base: end, since: None, rate });
                    None
                } else {
                    Some((end - now) / rate)
                }
            }
            _ => None,
        };
        if let (Some(_), Some((i, _))) = (wait.filter(|w| *w <= 0.0), next) {
            let s = pending[i].take().unwrap();
            end = end.max(s.end);
            if s.stream == VIDEO {
                preroll = false;
                if let Some(f) = video
                    && let Some(rgba) = unsafe { frame(&s.sample, &f) }
                {
                    sink(f.crop.2, f.crop.3, rgba);
                }
            } else if s.end >= now {
                // (Late sound — decoding fell behind, or before a seek target — is dropped to stay in sync.)
                let pcm = unsafe { samples(&s.sample) };
                shared.lock().unwrap().audio_samples += pcm.len() as u64;
                if let (Some(p), Some((ch, hz)), false) = (&audio, audio_format, pcm.is_empty()) {
                    p.append(rodio::buffer::SamplesBuffer::new(ch, hz, pcm));
                }
            }
            continue;
        }
        let cmd = match wait {
            Some(w) => match rx.recv_timeout(Duration::from_secs_f64(w.clamp(0.001, 0.05))) {
                Ok(c) => c,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            },
            None => match rx.recv() {
                Ok(c) => c,
                Err(_) => return Ok(()),
            },
        };
        let mut s = shared.lock().unwrap();
        let now = s.clock.now();
        match cmd {
            Cmd::Play => {
                s.clock.set(now, true);
                audio.as_ref().map(|p| p.play());
            }
            Cmd::Pause => {
                s.clock.set(now, false);
                audio.as_ref().map(|p| p.pause());
            }
            Cmd::Rate(r) => {
                s.clock.set(now, running);
                (s.clock.rate, rate) = (r, r);
                audio.as_ref().map(|p| p.set_speed(r as f32));
            }
            Cmd::Volume(v) => {
                volume = v;
                audio.as_ref().map(|p| p.set_volume(v));
            }
            Cmd::Seek(at) => {
                let pos = PROPVARIANT::from((at * 1e7) as i64);
                if let Err(e) = unsafe { reader.SetCurrentPosition(&GUID::zeroed(), &pos) } {
                    eprintln!("reflux: video seek: {e}");
                }
                s.clock.set(at, running);
                (pending, done, end, preroll, ended, s.ended) = ([None, None], [video.is_none(), audio_format.is_none()], at, true, false, false);
                // Dropping the old player drops its queued sound.
                audio = new_audio(running, volume, rate);
            }
        }
    }
}

unsafe fn open(url: &str) -> windows::core::Result<IMFSourceReader> {
    unsafe {
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.ok_or_else(windows::core::Error::empty)?;
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(url), &attrs)?;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        let mut any = false;
        for (stream, major, sub) in [(VIDEO, MFMediaType_Video, MFVideoFormat_RGB32), (AUDIO, MFMediaType_Audio, MFAudioFormat_Float)] {
            let mt = MFCreateMediaType()?;
            mt.SetGUID(&MF_MT_MAJOR_TYPE, &major)?;
            mt.SetGUID(&MF_MT_SUBTYPE, &sub)?;
            // No such stream (an audio file), or one MF can't decode: play the rest.
            if reader.SetCurrentMediaType(stream, None, &mt).is_ok() && reader.SetStreamSelection(stream, true).is_ok() {
                any = true;
            }
        }
        if !any {
            return Err(windows::core::Error::new(MF_E_INVALIDMEDIATYPE, "no playable stream"));
        }
        Ok(reader)
    }
}

pub(crate) unsafe fn video_format(reader: &IMFSourceReader) -> Option<VideoFormat> {
    unsafe {
        let mt = reader.GetCurrentMediaType(VIDEO).ok()?;
        let size = mt.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
        let (w, h) = ((size >> 32) as u32, size as u32);
        // Bytes per row; wine's video processor reports it in pixels, so a short one means tight rows.
        let stride = mt.GetUINT32(&MF_MT_DEFAULT_STRIDE).map_or(w as i32 * 4, |s| s as i32);
        let row = w as i32 * 4;
        let stride = if stride.unsigned_abs() >= w * 4 { stride } else if stride < 0 { -row } else { row };
        let mut area = MFVideoArea::default();
        let bytes = std::slice::from_raw_parts_mut(&mut area as *mut MFVideoArea as *mut u8, size_of::<MFVideoArea>());
        let crop = match mt.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, bytes, None) {
            Ok(()) => (area.OffsetX.value.max(0) as u32, area.OffsetY.value.max(0) as u32, area.Area.cx as u32, area.Area.cy as u32),
            Err(_) => (0, 0, w, h),
        };
        let crop = if crop.2 > 0 && crop.3 > 0 && crop.0 + crop.2 <= w && crop.1 + crop.3 <= h { crop } else { (0, 0, w, h) };
        Some(VideoFormat { h, stride, crop })
    }
}

unsafe fn audio_format(reader: &IMFSourceReader) -> Option<(NonZero<u16>, NonZero<u32>)> {
    unsafe {
        let mt = reader.GetCurrentMediaType(AUDIO).ok()?;
        Some((NonZero::new(mt.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).ok()? as u16)?, NonZero::new(mt.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND).ok()?)?))
    }
}

/// The next sample of `stream` (none at a gap or the end) and the reader's flags.
pub(crate) unsafe fn read(reader: &IMFSourceReader, stream: u32) -> windows::core::Result<(u32, Option<Sample>)> {
    unsafe {
        let (mut flags, mut ts, mut sample) = (0u32, 0i64, None);
        reader.ReadSample(stream, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut sample))?;
        if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
            return Err(windows::core::Error::new(MF_E_INVALIDMEDIATYPE, "stream error"));
        }
        let s = sample.map(|sample| {
            let ts = ts as f64 / 1e7;
            let dur = sample.GetSampleDuration().map_or(0.0, |d| d as f64 / 1e7);
            Sample { stream, ts, end: ts + dur, sample }
        });
        Ok((flags, s))
    }
}

/// Runs `f` over the sample's bytes.
unsafe fn with_bytes<T>(sample: &IMFSample, f: impl FnOnce(&[u8]) -> T) -> Option<T> {
    unsafe {
        let buf = sample.ConvertToContiguousBuffer().ok()?;
        let (mut p, mut len) = (std::ptr::null_mut(), 0u32);
        buf.Lock(&mut p, None, Some(&mut len)).ok()?;
        let out = f(std::slice::from_raw_parts(p, len as usize));
        let _ = buf.Unlock();
        Some(out)
    }
}

pub(crate) unsafe fn frame(sample: &IMFSample, f: &VideoFormat) -> Option<Vec<u8>> {
    unsafe { with_bytes(sample, |b| bgrx_to_rgba(b, f.h, f.stride, f.crop)).flatten() }
}

unsafe fn samples(sample: &IMFSample) -> Vec<f32> {
    unsafe { with_bytes(sample, |b| b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect()).unwrap_or_default() }
}

/// RGB32 (BGRX, rows `stride` apart; negative = bottom-up) to opaque RGBA, cropped.
fn bgrx_to_rgba(data: &[u8], h: u32, stride: i32, (x, y, w, ch): (u32, u32, u32, u32)) -> Option<Vec<u8>> {
    let pitch = stride.unsigned_abs() as usize;
    if data.len() < pitch * h as usize || pitch < (x + w) as usize * 4 {
        return None;
    }
    let mut out = Vec::with_capacity((w * ch * 4) as usize);
    for r in y..y + ch {
        let row = if stride < 0 { h - 1 - r } else { r } as usize * pitch;
        for px in data[row + x as usize * 4..row + (x + w) as usize * 4].chunks_exact(4) {
            out.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn bgrx_rows_crop_and_flip() {
        // 2×2 BGRX with 4 bytes of row padding; row 0 = blue, white; row 1 = red, black.
        let data = [255, 0, 0, 0, 255, 255, 255, 0, 9, 9, 9, 9, 0, 0, 255, 0, 0, 0, 0, 0, 9, 9, 9, 9];
        assert_eq!(super::bgrx_to_rgba(&data, 2, 12, (0, 0, 2, 2)).unwrap(), [0, 0, 255, 255, 255, 255, 255, 255, 255, 0, 0, 255, 0, 0, 0, 255]);
        assert_eq!(super::bgrx_to_rgba(&data, 2, -12, (1, 0, 1, 1)).unwrap(), [0, 0, 0, 255]);
        assert!(super::bgrx_to_rgba(&data[..20], 2, 12, (0, 0, 2, 2)).is_none());
    }
}
