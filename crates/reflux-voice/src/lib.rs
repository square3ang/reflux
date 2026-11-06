//! Voice and screen share over LiveKit.
//!
//! Fluxer hands out LiveKit grants over the gateway: send Voice State Update (op 4), receive
//! VOICE_SERVER_UPDATE with `endpoint` + `token`, then call [`Call::connect`] with them.
//! Participant identities are `user_{user_id}_{connection_id}` ([`user_of`]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use livekit::options::{TrackPublishOptions, VideoCodec};
use livekit::prelude::*;
use livekit::webrtc::desktop_capturer::{CaptureError, DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions, DesktopFrame};
use livekit::webrtc::native::yuv_helper;
use livekit::webrtc::prelude::{I420Buffer, VideoBuffer, VideoFormatType, VideoFrame, VideoResolution, VideoRotation};
use livekit::webrtc::rtp_sender::VideoEncoderBackend;
use livekit::webrtc::stats::RtcStats;
use livekit::webrtc::video_frame::native::VideoFrameBufferExt;
use livekit::webrtc::video_source::RtcVideoSource;
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_stream::native::NativeVideoStream;
use livekit::{AudioError, PlatformAudio};
use tokio::sync::mpsc::UnboundedReceiver;

pub use livekit;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Room(#[from] RoomError),
    #[error(transparent)]
    Audio(#[from] AudioError),
    #[error("screen capture is unavailable or was cancelled")]
    Capture,
}

pub struct Call {
    pub room: Room,
    // WebRTC's own audio device module: mic capture + speaker playout with its AEC/NS/AGC.
    audio: Option<PlatformAudio>,
    mic: Mutex<Option<LocalAudioTrack>>,
    deaf: AtomicBool,
    /// Users muted locally (their audio tracks stay disabled).
    muted_users: Mutex<std::collections::HashSet<u64>>,
    /// The room media is encrypted with the grant's `e2ee_key`.
    pub encrypted: bool,
}

impl Call {
    /// `with_audio` opens the platform audio devices; it must happen before connecting.
    /// `e2ee_key` (VOICE_SERVER_UPDATE) turns on LiveKit frame encryption with it as the shared
    /// key, as livekit-client's `ExternalE2EEKeyProvider.setKey(string)` does (PBKDF2 over the
    /// UTF-8 bytes, salt `LKFrameEncryptionKey`, AES-GCM).
    pub async fn connect(endpoint: &str, token: &str, with_audio: bool, e2ee_key: Option<&str>) -> Result<(Self, UnboundedReceiver<RoomEvent>), Error> {
        let audio = with_audio.then(PlatformAudio::new).transpose()?;
        let mut options = RoomOptions::default();
        if let Some(key) = e2ee_key {
            let key_provider = livekit::e2ee::key_provider::KeyProvider::with_shared_key(Default::default(), key.as_bytes().to_vec());
            options.encryption = Some(livekit::e2ee::E2eeOptions { encryption_type: livekit::e2ee::EncryptionType::Gcm, key_provider });
        }
        let (room, events) = Room::connect(endpoint, token, options).await?;
        if e2ee_key.is_some() {
            room.e2ee_manager().set_enabled(true);
        }
        let call = Self { room, audio, mic: Mutex::new(None), deaf: AtomicBool::new(false), muted_users: Default::default(), encrypted: e2ee_key.is_some() };
        Ok((call, events))
    }

    /// Voice processing: echo cancellation, noise suppression and automatic gain control.
    // shortcut: the Rust SDK only switches the platform's built-in (hardware) processors here;
    // WebRTC's software APM on the device source stays on. Expose AudioSourceOptions upstream to
    // turn it off too ("Direct input").
    pub fn set_processing(&self, echo_cancellation: bool, noise_suppression: bool, auto_gain_control: bool) {
        let Some(audio) = &self.audio else { return };
        let opts = livekit::AudioProcessingOptions { echo_cancellation, noise_suppression, auto_gain_control, ..Default::default() };
        if let Err(e) = audio.configure_audio_processing(opts) {
            eprintln!("reflux-voice: audio processing: {e}");
        }
    }

    /// Voice activity / push-to-talk gate: a closed gate sends silence without signalling a
    /// mute (the voice state's `self_mute` is what others show).
    pub fn set_mic_gate(&self, open: bool) {
        if let Some(t) = &*self.mic.lock().unwrap() {
            if open { t.enable() } else { t.disable() }
        }
    }

    /// Mutes one user's audio locally (`VoiceParticipantMenuItems` "Mute").
    pub fn set_user_muted(&self, user: u64, muted: bool) {
        let mut m = self.muted_users.lock().unwrap();
        if muted { m.insert(user) } else { m.remove(&user) };
        drop(m);
        self.apply_deaf();
    }

    /// Publishes the microphone (muted if `muted`). No-op without audio.
    pub async fn publish_mic(&self, muted: bool) -> Result<(), Error> {
        let Some(audio) = &self.audio else { return Ok(()) };
        let track = LocalAudioTrack::create_audio_track("microphone", audio.rtc_source());
        let opts = TrackPublishOptions { source: TrackSource::Microphone, ..Default::default() };
        self.room.local_participant().publish_track(LocalTrack::Audio(track.clone()), opts).await?;
        if muted {
            track.mute();
        }
        *self.mic.lock().unwrap() = Some(track);
        Ok(())
    }

    pub fn set_mic_muted(&self, muted: bool) {
        if let Some(t) = &*self.mic.lock().unwrap() {
            if muted { t.mute() } else { t.unmute() }
        }
    }

    /// Silences (or restores) every remote audio track. Call [`Call::apply_deaf`] again on
    /// `TrackSubscribed` so new tracks follow.
    pub fn set_deafened(&self, deaf: bool) {
        self.deaf.store(deaf, Ordering::Relaxed);
        self.apply_deaf();
    }

    pub fn apply_deaf(&self) {
        let deaf = self.deaf.load(Ordering::Relaxed);
        let muted = self.muted_users.lock().unwrap().clone();
        for p in self.room.remote_participants().values() {
            let off = deaf || user_of(&p.identity().0).is_some_and(|u| muted.contains(&u));
            for publication in p.track_publications().values() {
                if let Some(RemoteTrack::Audio(t)) = publication.track() {
                    if off { t.disable() } else { t.enable() }
                }
            }
        }
    }

    /// Switches the microphone / speaker to the devices with these names (`None` = keep).
    pub fn set_devices(&self, input: Option<&str>, output: Option<&str>) -> Result<(), Error> {
        let Some(audio) = &self.audio else { return Ok(()) };
        if let Some(d) = input.and_then(|n| audio.recording_devices().find(|d| d.name == n)) {
            audio.switch_recording_device(&d.id)?;
        }
        if let Some(d) = output.and_then(|n| audio.playout_devices().find(|d| d.name == n)) {
            audio.switch_playout_device(&d.id)?;
        }
        Ok(())
    }

    /// Round-trip time to the media server from the selected ICE candidate pair.
    pub async fn rtt_ms(&self) -> Option<u32> {
        let stats = self.room.get_stats().await.ok()?;
        stats
            .publisher_stats
            .iter()
            .chain(&stats.subscriber_stats)
            .filter_map(|s| match s {
                RtcStats::CandidatePair(c) if c.candidate_pair.nominated && c.candidate_pair.current_round_trip_time > 0.0 => {
                    Some((c.candidate_pair.current_round_trip_time * 1000.0).round() as u32)
                }
                _ => None,
            })
            .min()
    }

    /// Starts capturing a screen (the XDG portal picker on Wayland) and publishes it once the
    /// first frame arrives. H.264 so the NVENC / VA-API factories can take it: LiveKit offers
    /// Constrained Baseline (42e01f) first, the only profile those encoders advertise.
    pub async fn start_screen(&self, quality: ScreenQuality) -> Result<ScreenShare, Error> {
        let stop = Arc::new(AtomicBool::new(false));
        let slot: Arc<Mutex<Option<NativeVideoSource>>> = Arc::default();
        let (first, size) = tokio::sync::oneshot::channel();
        spawn_capture(stop.clone(), slot.clone(), first, quality);
        let Ok((width, height)) = size.await else {
            return Err(Error::Capture);
        };
        let (width, height) = quality.fit(width, height);
        let source = NativeVideoSource::new(VideoResolution { width, height }, true);
        let track = LocalVideoTrack::create_video_track("screen_share", RtcVideoSource::Native(source.clone()));
        let opts = TrackPublishOptions {
            source: TrackSource::Screenshare,
            video_codec: VideoCodec::H264,
            video_encoder: encoder_backend(),
            simulcast: false,
            video_encoding: quality.bitrate.map(|b| livekit::options::VideoEncoding { max_bitrate: b, max_framerate: quality.fps as f64 }),
            ..Default::default()
        };
        let publication = match self.room.local_participant().publish_track(LocalTrack::Video(track), opts).await {
            Ok(p) => p,
            Err(e) => {
                stop.store(true, Ordering::Relaxed);
                return Err(e.into());
            }
        };
        *slot.lock().unwrap() = Some(source);
        Ok(ScreenShare { sid: publication.sid(), stop })
    }

    pub async fn stop_screen(&self, share: ScreenShare) -> Result<(), Error> {
        share.stop.store(true, Ordering::Relaxed);
        self.room.local_participant().unpublish_track(&share.sid).await?;
        Ok(())
    }

    /// The first subscribed screen share of `user`.
    pub fn screen_track(&self, user: u64) -> Option<RemoteVideoTrack> {
        self.video_track(user, TrackSource::Screenshare)
    }

    /// The first subscribed video track of `user` from `source` (camera or screen share).
    pub fn video_track(&self, user: u64, source: TrackSource) -> Option<RemoteVideoTrack> {
        self.room.remote_participants().values().filter(|p| user_of(&p.identity().0) == Some(user)).find_map(|p| {
            p.track_publications().values().find_map(|t| match (t.source(), t.track()) {
                (s, Some(RemoteTrack::Video(v))) if s == source => Some(v),
                _ => None,
            })
        })
    }

    /// Publishes a camera track the app feeds with [`Camera::push_rgba`]. H.264 Constrained
    /// Baseline (42e01f) so NVENC / VA-API can encode it, like the screen share.
    pub async fn publish_camera(&self, width: u32, height: u32) -> Result<Camera, Error> {
        let source = NativeVideoSource::new(VideoResolution { width, height }, false);
        let track = LocalVideoTrack::create_video_track("camera", RtcVideoSource::Native(source.clone()));
        let opts = TrackPublishOptions {
            source: TrackSource::Camera,
            video_codec: VideoCodec::H264,
            video_encoder: encoder_backend(),
            simulcast: false,
            ..Default::default()
        };
        let publication = self.room.local_participant().publish_track(LocalTrack::Video(track), opts).await?;
        Ok(Camera { sid: publication.sid(), source, buffer: Mutex::new(I420Buffer::new(width, height)) })
    }

    pub async fn unpublish_camera(&self, camera: &Camera) -> Result<(), Error> {
        self.room.local_participant().unpublish_track(&camera.sid).await?;
        Ok(())
    }

    /// The LiveKit server's region and our identity, for the voice details popout.
    pub fn server_info(&self) -> (String, String) {
        (self.room.name(), self.room.local_participant().identity().0)
    }

    pub async fn leave(&self) -> Result<(), Error> {
        Ok(self.room.close().await?)
    }
}

/// A published camera track.
pub struct Camera {
    sid: TrackSid,
    source: NativeVideoSource,
    buffer: Mutex<I420Buffer>,
}

impl Camera {
    /// Sends one straight-RGBA frame (tightly packed, `width * 4` per row).
    pub fn push_rgba(&self, width: u32, height: u32, rgba: &[u8]) {
        let mut buffer = self.buffer.lock().unwrap();
        if buffer.width() != width || buffer.height() != height {
            *buffer = I420Buffer::new(width, height);
        }
        let (sy, su, sv) = buffer.strides();
        let (y, u, v) = buffer.data_mut();
        // libyuv's "ABGR" is R, G, B, A in memory.
        yuv_helper::abgr_to_i420(rgba, width * 4, y, sy, u, su, v, sv, width as i32, height as i32);
        let frame = VideoFrame { rotation: VideoRotation::VideoRotation0, timestamp_us: 0, frame_metadata: None, buffer: &*buffer };
        self.source.capture_frame(&frame);
    }
}

static SOFTWARE_ENCODE: AtomicBool = AtomicBool::new(false);

/// Encoder preference: `false` lets the NVENC / VA-API factories take H.264
/// (`LIVEKIT_PREFERRED_HW_ENCODER` picks between them), `true` forces software encoding.
pub fn set_software_encoding(on: bool) {
    SOFTWARE_ENCODE.store(on, Ordering::Relaxed);
}

fn encoder_backend() -> VideoEncoderBackend {
    if SOFTWARE_ENCODE.load(Ordering::Relaxed) { VideoEncoderBackend::Software } else { VideoEncoderBackend::Hardware }
}

/// Screen share quality (`StreamSettings`): frame rate, the tallest frame sent (`None` =
/// source size) and a bitrate cap in bits/s (`None` = LiveKit's choice).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenQuality {
    pub fps: u32,
    pub max_height: Option<u32>,
    pub bitrate: Option<u64>,
}

impl Default for ScreenQuality {
    fn default() -> Self {
        Self { fps: 30, max_height: Some(720), bitrate: None }
    }
}

impl ScreenQuality {
    /// The frame size sent for a `w`×`h` capture: scaled down to `max_height`, even sides.
    pub fn fit(&self, w: u32, h: u32) -> (u32, u32) {
        let (w, h) = match self.max_height {
            Some(m) if h > m => ((w as u64 * m as u64 / h as u64) as u32, m),
            _ => (w, h),
        };
        (w.max(2) & !1, h.max(2) & !1)
    }
}

/// A running screen share; stop it with [`Call::stop_screen`]. Dropping it also stops capture.
pub struct ScreenShare {
    sid: TrackSid,
    stop: Arc<AtomicBool>,
}

impl Drop for ScreenShare {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Captures on its own thread (the capturer is not `Send`) until `stop`; reports the first
/// frame's size through `first`, or drops it when capture fails or the picker is cancelled.
fn spawn_capture(stop: Arc<AtomicBool>, slot: Arc<Mutex<Option<NativeVideoSource>>>, first: tokio::sync::oneshot::Sender<(u32, u32)>, quality: ScreenQuality) {
    std::thread::spawn(move || {
        // Generic = the portal picker (screens and windows) on Wayland/X11 via PipeWire.
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let kind = DesktopCaptureSourceType::Generic;
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let kind = DesktopCaptureSourceType::Screen;
        let mut options = DesktopCapturerOptions::new(kind);
        options.set_include_cursor(true);
        let Some(mut capturer) = DesktopCapturer::new(options) else { return };
        let first = Mutex::new(Some(first));
        let failed = Arc::new(AtomicBool::new(false));
        let mut out = VideoFrame { rotation: VideoRotation::VideoRotation0, timestamp_us: 0, frame_metadata: None, buffer: I420Buffer::new(1, 1) };
        let cb_failed = failed.clone();
        let callback = move |r: Result<DesktopFrame, CaptureError>| {
            let frame = match r {
                Ok(f) => f,
                Err(CaptureError::Temporary) => return,
                Err(CaptureError::Permanent) => {
                    first.lock().unwrap().take();
                    cb_failed.store(true, Ordering::Relaxed);
                    return;
                }
            };
            let (w, h) = (frame.width(), frame.height());
            if let Some(tx) = first.lock().unwrap().take() {
                let _ = tx.send((w as u32, h as u32));
            }
            if out.buffer.width() as i32 != w || out.buffer.height() as i32 != h {
                out.buffer = I420Buffer::new(w as u32, h as u32);
            }
            let (sy, su, sv) = out.buffer.strides();
            let (y, u, v) = out.buffer.data_mut();
            yuv_helper::argb_to_i420(frame.data(), frame.stride(), y, sy, u, su, v, sv, w, h);
            if let Some(source) = slot.lock().unwrap().as_ref() {
                let (fw, fh) = quality.fit(w as u32, h as u32);
                if (fw, fh) == (w as u32, h as u32) {
                    source.capture_frame(&out);
                } else {
                    let scaled = out.buffer.scale(fw as i32, fh as i32);
                    source.capture_frame(&VideoFrame { rotation: VideoRotation::VideoRotation0, timestamp_us: 0, frame_metadata: None, buffer: scaled });
                }
            }
        };
        let source = capturer.get_source_list().into_iter().next();
        capturer.start_capture(source, callback);
        while !stop.load(Ordering::Relaxed) && !failed.load(Ordering::Relaxed) {
            capturer.capture_frame();
            std::thread::sleep(Duration::from_millis(1000 / quality.fps.clamp(1, 60) as u64));
        }
    });
}

/// Feeds `track`'s frames as straight RGBA to `sink` until the track ends.
pub async fn render_video(track: RemoteVideoTrack, mut sink: impl FnMut(u32, u32, Vec<u8>)) {
    let mut stream = NativeVideoStream::new(track.rtc_track());
    while let Some(frame) = stream.next().await {
        let i420 = frame.buffer.to_i420();
        let (w, h) = (i420.width(), i420.height());
        let mut rgba = vec![0; (w * h * 4) as usize];
        // libyuv's "ABGR" is R, G, B, A in memory.
        i420.to_argb(VideoFormatType::ABGR, &mut rgba, w * 4, w as i32, h as i32);
        sink(w, h, rgba);
    }
}

/// Names of the microphones and speakers WebRTC's audio device module sees.
pub fn devices() -> Result<(Vec<String>, Vec<String>), Error> {
    let audio = PlatformAudio::new()?;
    Ok((audio.recording_devices().map(|d| d.name).collect(), audio.playout_devices().map(|d| d.name).collect()))
}

/// The user ID in a Fluxer participant identity (`user_{id}_{connection}`).
pub fn user_of(identity: &str) -> Option<u64> {
    let rest = identity.strip_prefix("user_")?;
    let (id, conn) = rest.split_once('_')?;
    (!conn.is_empty()).then_some(())?;
    id.parse().ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn identities() {
        assert_eq!(super::user_of("user_123_abc"), Some(123));
        assert_eq!(super::user_of("user_123_a_b"), Some(123));
        assert_eq!(super::user_of("user_123_"), None);
        assert_eq!(super::user_of("bot_1_x"), None);
    }

    #[test]
    fn screen_quality_fits() {
        let q = super::ScreenQuality { fps: 30, max_height: Some(720), bitrate: None };
        assert_eq!(q.fit(2560, 1440), (1280, 720));
        assert_eq!(q.fit(1280, 600), (1280, 600));
        assert_eq!(super::ScreenQuality { max_height: None, ..q }.fit(1921, 1081), (1920, 1080));
    }
}
