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
}

impl Call {
    /// `with_audio` opens the platform audio devices; it must happen before connecting.
    pub async fn connect(endpoint: &str, token: &str, with_audio: bool) -> Result<(Self, UnboundedReceiver<RoomEvent>), Error> {
        let audio = with_audio.then(PlatformAudio::new).transpose()?;
        let (room, events) = Room::connect(endpoint, token, RoomOptions::default()).await?;
        Ok((Self { room, audio, mic: Mutex::new(None), deaf: AtomicBool::new(false) }, events))
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
        for p in self.room.remote_participants().values() {
            for publication in p.track_publications().values() {
                if let Some(RemoteTrack::Audio(t)) = publication.track() {
                    if deaf { t.disable() } else { t.enable() }
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
    pub async fn start_screen(&self) -> Result<ScreenShare, Error> {
        let stop = Arc::new(AtomicBool::new(false));
        let slot: Arc<Mutex<Option<NativeVideoSource>>> = Arc::default();
        let (first, size) = tokio::sync::oneshot::channel();
        spawn_capture(stop.clone(), slot.clone(), first);
        let Ok((width, height)) = size.await else {
            return Err(Error::Capture);
        };
        let source = NativeVideoSource::new(VideoResolution { width, height }, true);
        let track = LocalVideoTrack::create_video_track("screen_share", RtcVideoSource::Native(source.clone()));
        let opts = TrackPublishOptions {
            source: TrackSource::Screenshare,
            video_codec: VideoCodec::H264,
            video_encoder: VideoEncoderBackend::Hardware,
            simulcast: false,
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
        self.room.remote_participants().values().filter(|p| user_of(&p.identity().0) == Some(user)).find_map(|p| {
            p.track_publications().values().find_map(|t| match (t.source(), t.track()) {
                (TrackSource::Screenshare, Some(RemoteTrack::Video(v))) => Some(v),
                _ => None,
            })
        })
    }

    pub async fn leave(&self) -> Result<(), Error> {
        Ok(self.room.close().await?)
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
fn spawn_capture(stop: Arc<AtomicBool>, slot: Arc<Mutex<Option<NativeVideoSource>>>, first: tokio::sync::oneshot::Sender<(u32, u32)>) {
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
                source.capture_frame(&out);
            }
        };
        let source = capturer.get_source_list().into_iter().next();
        capturer.start_capture(source, callback);
        while !stop.load(Ordering::Relaxed) && !failed.load(Ordering::Relaxed) {
            capturer.capture_frame();
            std::thread::sleep(Duration::from_millis(33));
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
}
