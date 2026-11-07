//! Voice and screen share over LiveKit.
//!
//! Fluxer hands out LiveKit grants over the gateway: send Voice State Update (op 4), receive
//! VOICE_SERVER_UPDATE with `endpoint` + `token`, then call [`Call::connect`] with them.
//! Participant identities are `user_{user_id}_{connection_id}` ([`user_of`]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{FutureExt, StreamExt};
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
use livekit::webrtc::audio_frame::AudioFrame;
use livekit::webrtc::audio_source::native::NativeAudioSource;
use livekit::webrtc::audio_source::{AudioSourceOptions, RtcAudioSource};
use livekit::webrtc::audio_stream::native::NativeAudioStream;
use livekit::webrtc::prelude::RtcVideoTrack;
use tokio::sync::mpsc::UnboundedReceiver;

pub mod audio;
pub mod mesh;
pub use audio::{Engine, Kind, Mixer, Processing, Source};
pub use livekit;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Room(#[from] RoomError),
    #[error("screen capture is unavailable or was cancelled")]
    Capture,
}

/// Where the media goes: a LiveKit room, or peer connections to each participant.
enum Link {
    Room(Room),
    Mesh(mesh::Mesh),
}

pub struct Call {
    link: Link,
    /// Our own audio I/O (see [`audio`]); `None` = no audio at all.
    audio: Option<Engine>,
    mic: Mutex<Option<LocalAudioTrack>>,
    /// The selected microphone, for reopening it.
    input: Mutex<Option<String>>,
    /// The room media is encrypted with the grant's `e2ee_key`.
    pub encrypted: bool,
    /// The runtime `connect` ran on: LiveKit's sync track calls (mute / unmute) spawn on it,
    /// and they're made from the UI thread.
    rt: tokio::runtime::Handle,
}

impl Call {
    /// `with_audio` plays the room through our mixer on the default output (see
    /// [`Call::set_devices`]).
    /// `e2ee_key` (VOICE_SERVER_UPDATE) turns on LiveKit frame encryption with it as the shared
    /// key, as livekit-client's `ExternalE2EEKeyProvider.setKey(string)` does (PBKDF2 over the
    /// UTF-8 bytes, salt `LKFrameEncryptionKey`, AES-GCM).
    pub async fn connect(endpoint: &str, token: &str, with_audio: bool, e2ee_key: Option<&str>) -> Result<(Self, UnboundedReceiver<RoomEvent>), Error> {
        let audio = with_audio.then(Engine::new);
        if let Some(a) = &audio
            && !a.open_output(None)
        {
            eprintln!("reflux-voice: no speaker; call audio is silent");
        }
        let mut options = RoomOptions::default();
        if let Some(key) = e2ee_key {
            // `ExternalE2EEKeyProvider` never ratchets (window 0); ratcheting on a decrypt failure
            // here would derive keys the web client never uses.
            let opts = livekit::e2ee::key_provider::KeyProviderOptions { ratchet_window_size: 0, ..Default::default() };
            let key_provider = livekit::e2ee::key_provider::KeyProvider::with_shared_key(opts, key.as_bytes().to_vec());
            options.encryption = Some(livekit::e2ee::E2eeOptions { encryption_type: livekit::e2ee::EncryptionType::Gcm, key_provider });
        }
        let (room, events) = Room::connect(endpoint, token, options).await?;
        if e2ee_key.is_some() {
            room.e2ee_manager().set_enabled(true);
        }
        let call = Self { link: Link::Room(room), audio, mic: Mutex::new(None), input: Mutex::new(None), encrypted: e2ee_key.is_some(), rt: tokio::runtime::Handle::current() };
        Ok((call, events))
    }

    /// A peer-to-peer call ([`mesh`]) with our audio path; feed it the roster and signals
    /// through [`Call::mesh`].
    pub fn connect_mesh(target: mesh::Target) -> (Self, UnboundedReceiver<mesh::Event>) {
        let audio = Engine::new();
        if !audio.open_output(None) {
            eprintln!("reflux-voice: no speaker; call audio is silent");
        }
        let rt = tokio::runtime::Handle::current();
        let (mesh, events) = mesh::Mesh::new(target, rt.clone());
        mesh.set_encoder(encoder_backend());
        let call = Self { link: Link::Mesh(mesh), audio: Some(audio), mic: Mutex::new(None), input: Mutex::new(None), encrypted: false, rt };
        (call, events)
    }

    pub fn mesh(&self) -> Option<&mesh::Mesh> {
        match &self.link {
            Link::Mesh(m) => Some(m),
            Link::Room(_) => None,
        }
    }

    fn room(&self) -> Option<&Room> {
        match &self.link {
            Link::Room(r) => Some(r),
            Link::Mesh(_) => None,
        }
    }

    /// Publishes `track` as `source` on whichever link this is, returning its track SID.
    async fn publish(&self, track: LocalTrack, opts: TrackPublishOptions, size: (u32, u32)) -> Result<TrackSid, Error> {
        match &self.link {
            Link::Room(room) => Ok(room.local_participant().publish_track(track, opts).await?.sid()),
            Link::Mesh(mesh) => {
                let src = mesh_src(opts.source);
                let bitrate = opts.video_encoding.map(|e| e.max_bitrate);
                let rtc: livekit::webrtc::prelude::MediaStreamTrack = match &track {
                    LocalTrack::Audio(a) => a.rtc_track().into(),
                    LocalTrack::Video(v) => v.rtc_track().into(),
                };
                mesh.publish(src, Some(rtc), false, size, bitrate);
                Ok(TrackSid::try_from(mesh::track_sid(mesh.connection_id(), src)).expect("TR_ prefix"))
            }
        }
    }

    async fn unpublish(&self, sid: &TrackSid) -> Result<(), Error> {
        match &self.link {
            Link::Room(room) => {
                room.local_participant().unpublish_track(sid).await?;
            }
            Link::Mesh(mesh) => {
                if let Some(src) = mesh::Src::of_sid(sid.as_str()) {
                    mesh.publish(src, None, false, (0, 0), None);
                }
            }
        }
        Ok(())
    }

    /// The level of the last 10 ms the microphone sent (0..1 peak), for speaking rings when
    /// no server reports them.
    pub fn mic_level(&self) -> f32 {
        self.audio.as_ref().map_or(0.0, |a| a.level())
    }

    /// Mic processing (echo cancellation, RNNoise, gain control) and input volume.
    pub fn set_processing(&self, p: Processing) {
        if let Some(audio) = &self.audio {
            audio.set_processing(p);
        }
    }

    /// The remote mix's volumes and mutes.
    pub fn mixer(&self) -> Option<Arc<Mutex<Mixer>>> {
        self.audio.as_ref().map(|a| a.mixer.clone())
    }

    /// Voice activity / push-to-talk gate: a closed gate sends silence without signalling a
    /// mute (the voice state's `self_mute` is what others show).
    pub fn set_mic_gate(&self, open: bool) {
        if let Some(t) = &*self.mic.lock().unwrap() {
            if open { t.enable() } else { t.disable() }
        }
    }

    /// Publishes the microphone (muted if `muted`). No-op without audio.
    pub async fn publish_mic(&self, muted: bool) -> Result<(), Error> {
        let Some(audio) = &self.audio else { return Ok(()) };
        if !audio.open_input(self.input.lock().unwrap().as_deref()) {
            eprintln!("reflux-voice: no microphone");
        }
        let track = LocalAudioTrack::create_audio_track("microphone", RtcAudioSource::Native(audio.source.clone()));
        let opts = TrackPublishOptions { source: TrackSource::Microphone, ..Default::default() };
        self.publish(LocalTrack::Audio(track.clone()), opts, (0, 0)).await?;
        if muted {
            track.mute();
            if let Some(m) = self.mesh() {
                m.set_muted(mesh::Src::Microphone, true);
            }
        }
        *self.mic.lock().unwrap() = Some(track);
        Ok(())
    }

    pub fn set_mic_muted(&self, muted: bool) {
        if let Some(t) = &*self.mic.lock().unwrap() {
            let _rt = self.rt.enter();
            if muted { t.mute() } else { t.unmute() }
            if let Some(m) = self.mesh() {
                m.set_muted(mesh::Src::Microphone, muted);
            }
        }
    }

    /// Plays a subscribed remote audio track through the mixer until it ends. Run it as a task;
    /// abort it on unsubscribe.
    pub fn play(&self, track: RemoteAudioTrack, participant: &RemoteParticipant, source: TrackSource) -> Option<impl Future<Output = ()> + Send + 'static> {
        let kind = if source == TrackSource::ScreenshareAudio { Kind::Stream } else { Kind::Voice };
        let src = Source { kind, user: user_of(&participant.identity().0)?, conn: participant.identity().0 };
        self.play_rtc(track.rtc_track(), src, |_| {})
    }

    /// Plays a remote audio track through the mixer as `src` until it ends; `speaking` hears
    /// when its voice activity starts and stops (300 ms hang).
    pub fn play_rtc(&self, track: livekit::webrtc::prelude::RtcAudioTrack, src: Source, mut speaking: impl FnMut(bool) + Send + 'static) -> Option<impl Future<Output = ()> + Send + 'static> {
        let mixer = self.mixer()?;
        Some(async move {
            let mut stream = NativeAudioStream::new(track, audio::RATE as i32, 1);
            let (mut buf, mut quiet_for, mut on) = (Vec::new(), u32::MAX, false);
            while let Some(frame) = stream.next().await {
                buf.clear();
                buf.extend(frame.data.iter().map(|&s| s as f32 / 32768.0));
                quiet_for = if audio::is_speech(&buf) { 0 } else { quiet_for.saturating_add(1) };
                if on != (quiet_for < 30) {
                    on = !on;
                    speaking(on);
                }
                mixer.lock().unwrap().push(src.clone(), &buf);
            }
            mixer.lock().unwrap().remove(&src);
            if on {
                speaking(false);
            }
        })
    }

    /// Switches the microphone / speaker to the cpal devices with these names (`None` = the
    /// system default).
    pub fn set_devices(&self, input: Option<&str>, output: Option<&str>) {
        let Some(audio) = &self.audio else { return };
        let mut cur = self.input.lock().unwrap();
        if cur.as_deref() != input {
            *cur = input.map(str::to_string);
            if self.mic.lock().unwrap().is_some() {
                audio.open_input(input);
            }
        }
        drop(cur);
        audio.open_output(output);
    }

    /// Publishes screen share audio (`ScreenshareAudio`): feed it with [`StreamAudio::push`].
    pub async fn publish_stream_audio(&self) -> Result<StreamAudio, Error> {
        let source = NativeAudioSource::new(AudioSourceOptions::default(), audio::RATE, 2, 0);
        let track = LocalAudioTrack::create_audio_track("screen_share_audio", RtcAudioSource::Native(source.clone()));
        let opts = TrackPublishOptions { source: TrackSource::ScreenshareAudio, ..Default::default() };
        let sid = self.publish(LocalTrack::Audio(track), opts, (0, 0)).await?;
        Ok(StreamAudio { sid, source, pending: Mutex::default() })
    }

    pub async fn unpublish_stream_audio(&self, audio: &StreamAudio) -> Result<(), Error> {
        self.unpublish(&audio.sid).await
    }

    /// Round-trip time to the media server from the selected ICE candidate pair.
    pub async fn rtt_ms(&self) -> Option<u32> {
        let room = match &self.link {
            Link::Room(r) => r,
            Link::Mesh(m) => return m.rtt_ms().await,
        };
        let stats = room.get_stats().await.ok()?;
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
    /// `pick` is a source from [`screen_sources`]; `None` = the system picker / main screen.
    pub async fn start_screen(&self, quality: ScreenQuality, pick: Option<ScreenSource>) -> Result<ScreenShare, Error> {
        let stop = Arc::new(AtomicBool::new(false));
        let slot: Arc<Mutex<Option<NativeVideoSource>>> = Arc::default();
        let (first, size) = tokio::sync::oneshot::channel();
        spawn_capture(stop.clone(), slot.clone(), first, quality, pick);
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
        let sid = match self.publish(LocalTrack::Video(track), opts, (width, height)).await {
            Ok(sid) => sid,
            Err(e) => {
                stop.store(true, Ordering::Relaxed);
                return Err(e);
            }
        };
        *slot.lock().unwrap() = Some(source);
        Ok(ScreenShare { sid, stop })
    }

    pub async fn stop_screen(&self, share: ScreenShare) -> Result<(), Error> {
        share.stop.store(true, Ordering::Relaxed);
        self.unpublish(&share.sid).await
    }

    /// The first subscribed screen share of `user`.
    pub fn screen_track(&self, user: u64) -> Option<RtcVideoTrack> {
        self.video_track(user, TrackSource::Screenshare)
    }

    /// The first subscribed video track of `user` from `source` (camera or screen share).
    pub fn video_track(&self, user: u64, source: TrackSource) -> Option<RtcVideoTrack> {
        let room = match &self.link {
            Link::Room(r) => r,
            Link::Mesh(m) => return m.remote_video(user, mesh_src(source)),
        };
        room.remote_participants().values().filter(|p| user_of(&p.identity().0) == Some(user)).find_map(|p| {
            p.track_publications().values().find_map(|t| match (t.source(), t.track()) {
                (s, Some(RemoteTrack::Video(v))) if s == source => Some(v.rtc_track()),
                _ => None,
            })
        })
    }

    /// Publishes a camera track the app feeds with [`Camera::push_rgba`]. H.264 Constrained
    /// Baseline (42e01f) so NVENC / VA-API can encode it, like the screen share.
    pub async fn publish_camera(&self, width: u32, height: u32) -> Result<Camera, Error> {
        self.publish_video(width, height, TrackSource::Camera).await
    }

    /// A screen share track fed by the caller (a test pattern in live tests), so encoding can
    /// be checked without a capture source or portal dialog.
    pub async fn publish_test_screen(&self, width: u32, height: u32) -> Result<Camera, Error> {
        self.publish_video(width, height, TrackSource::Screenshare).await
    }

    async fn publish_video(&self, width: u32, height: u32, kind: TrackSource) -> Result<Camera, Error> {
        let screen = kind == TrackSource::Screenshare;
        let source = NativeVideoSource::new(VideoResolution { width, height }, screen);
        let track = LocalVideoTrack::create_video_track(if screen { "screen_share" } else { "camera" }, RtcVideoSource::Native(source.clone()));
        let opts = TrackPublishOptions {
            source: kind,
            video_codec: VideoCodec::H264,
            video_encoder: encoder_backend(),
            simulcast: false,
            ..Default::default()
        };
        let sid = self.publish(LocalTrack::Video(track), opts, (width, height)).await?;
        Ok(Camera { sid, source, buffer: Mutex::new(I420Buffer::new(width, height)) })
    }

    pub async fn unpublish_camera(&self, camera: &Camera) -> Result<(), Error> {
        self.unpublish(&camera.sid).await
    }

    /// Each outbound video stream's (encoder, frames encoded, codec id), from WebRTC stats
    /// (`encoderImplementation`: e.g. `NvidiaVideoEncoder`, `libvpx`, `OpenH264`).
    pub async fn video_encoders(&self) -> Vec<(String, u32, String)> {
        let Some(Ok(stats)) = (match self.room() {
            Some(r) => Some(r.get_stats().await),
            None => None,
        }) else {
            return Vec::new();
        };
        stats
            .publisher_stats
            .iter()
            .filter_map(|s| match s {
                RtcStats::OutboundRtp(o) if o.stream.kind == "video" => Some((o.outbound.encoder_implementation.clone(), o.outbound.frames_encoded, o.stream.codec_id.clone())),
                _ => None,
            })
            .collect()
    }

    /// The LiveKit server's region and our identity, for the voice details popout.
    pub fn server_info(&self) -> (String, String) {
        match &self.link {
            Link::Room(r) => (r.name(), r.local_participant().identity().0),
            Link::Mesh(m) => ("P2P".into(), m.connection_id().into()),
        }
    }

    pub async fn leave(&self) -> Result<(), Error> {
        match &self.link {
            Link::Room(r) => Ok(r.close().await?),
            Link::Mesh(m) => {
                m.close();
                Ok(())
            }
        }
    }
}

/// Published screen share audio.
pub struct StreamAudio {
    sid: TrackSid,
    source: NativeAudioSource,
    /// Interleaved stereo samples short of a 10 ms frame.
    pending: Mutex<Vec<i16>>,
}

impl StreamAudio {
    /// Sends interleaved stereo 48 kHz samples (any count).
    pub fn push(&self, samples: &[f32]) {
        let mut pending = self.pending.lock().unwrap();
        pending.extend(samples.iter().map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16));
        let frame = audio::FRAME * 2;
        while pending.len() >= frame {
            let data: Vec<i16> = pending.drain(..frame).collect();
            let af = AudioFrame { data: data.into(), sample_rate: audio::RATE, num_channels: 2, samples_per_channel: audio::FRAME as u32 };
            // Queue 0 delivers synchronously.
            let _ = self.source.capture_frame(&af).now_or_never();
        }
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
fn spawn_capture(stop: Arc<AtomicBool>, slot: Arc<Mutex<Option<NativeVideoSource>>>, first: tokio::sync::oneshot::Sender<(u32, u32)>, quality: ScreenQuality, pick: Option<ScreenSource>) {
    std::thread::spawn(move || {
        // Generic = the portal picker (screens and windows) on Wayland/X11 via PipeWire.
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let default = DesktopCaptureSourceType::Generic;
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let default = DesktopCaptureSourceType::Screen;
        let kind = pick.as_ref().map_or(default, |p| p.kind());
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
        let list = capturer.get_source_list();
        let source = match &pick {
            Some(p) => list.into_iter().find(|s| s.id() == p.id),
            None => list.into_iter().next(),
        };
        capturer.start_capture(source, callback);
        while !stop.load(Ordering::Relaxed) && !failed.load(Ordering::Relaxed) {
            capturer.capture_frame();
            std::thread::sleep(Duration::from_millis(1000 / quality.fps.clamp(1, 60) as u64));
        }
    });
}

/// The mesh source a LiveKit track source maps to.
fn mesh_src(source: TrackSource) -> mesh::Src {
    match source {
        TrackSource::Camera => mesh::Src::Camera,
        TrackSource::Screenshare => mesh::Src::ScreenShare,
        TrackSource::ScreenshareAudio => mesh::Src::ScreenShareAudio,
        _ => mesh::Src::Microphone,
    }
}

/// Feeds `track`'s frames as straight RGBA to `sink` until the track ends.
pub async fn render_video(track: RtcVideoTrack, mut sink: impl FnMut(u32, u32, Vec<u8>)) {
    let mut stream = NativeVideoStream::new(track);
    while let Some(frame) = stream.next().await {
        let i420 = frame.buffer.to_i420();
        let (w, h) = (i420.width(), i420.height());
        let mut rgba = vec![0; (w * h * 4) as usize];
        // libyuv's "ABGR" is R, G, B, A in memory.
        i420.to_argb(VideoFormatType::ABGR, &mut rgba, w * 4, w as i32, h as i32);
        sink(w, h, rgba);
    }
}

/// Names of the microphones and speakers (cpal's, which our audio path opens).
pub fn devices() -> (Vec<String>, Vec<String>) {
    audio::devices()
}

/// A screen or window for the in-app picker (`ScreenSharePickerModal`).
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenSource {
    pub window: bool,
    pub id: u64,
    pub title: String,
    /// A small straight-RGBA preview: (width, height, pixels).
    pub thumbnail: Option<(u32, u32, Vec<u8>)>,
}

impl ScreenSource {
    fn kind(&self) -> DesktopCaptureSourceType {
        if self.window { DesktopCaptureSourceType::Window } else { DesktopCaptureSourceType::Screen }
    }
}

/// Whether the platform lets us list screens and windows ourselves. Wayland only allows the
/// portal's own picker, and macOS needs ScreenCaptureKit's.
pub fn can_list_sources() -> bool {
    cfg!(target_os = "windows") || (cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_some())
}

/// Screens, then windows, each with one captured frame scaled to `thumb_w` wide. Blocking:
/// run it off the UI thread.
pub fn screen_sources(thumb_w: u32) -> Vec<ScreenSource> {
    if !can_list_sources() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for window in [false, true] {
        let kind = if window { DesktopCaptureSourceType::Window } else { DesktopCaptureSourceType::Screen };
        let Some(lister) = DesktopCapturer::new(DesktopCapturerOptions::new(kind)) else { continue };
        for (i, s) in lister.get_source_list().into_iter().enumerate() {
            let title = match s.title() {
                t if t.is_empty() && !window => format!("Screen {}", i + 1),
                t => t,
            };
            let thumbnail = thumbnail(kind, s, thumb_w);
            out.push(ScreenSource { window, id: thumbnail.0, title, thumbnail: thumbnail.1 });
        }
    }
    out
}

/// One frame of `source`, nearest-neighbour scaled to `w` wide.
/// (width, height, straight RGBA).
type Thumbnail = (u32, u32, Vec<u8>);

fn thumbnail(kind: DesktopCaptureSourceType, source: livekit::webrtc::desktop_capturer::CaptureSource, w: u32) -> (u64, Option<Thumbnail>) {
    let id = source.id();
    let Some(mut capturer) = DesktopCapturer::new(DesktopCapturerOptions::new(kind)) else { return (id, None) };
    let out: Arc<Mutex<Option<Thumbnail>>> = Arc::default();
    let slot = out.clone();
    capturer.start_capture(Some(source), move |r| {
        let Ok(f) = r else { return };
        let (fw, fh, stride) = (f.width().max(1) as usize, f.height().max(1) as usize, f.stride() as usize);
        let w = (w as usize).min(fw);
        let h = (fh * w / fw).max(1);
        let data = f.data();
        let mut rgba = vec![0; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let o = (y * fh / h) * stride + (x * fw / w) * 4;
                let px = &data[o..o + 4]; // B, G, R, A
                rgba[(y * w + x) * 4..][..4].copy_from_slice(&[px[2], px[1], px[0], 255]);
            }
        }
        *slot.lock().unwrap() = Some((w as u32, h as u32, rgba));
    });
    for _ in 0..3 {
        capturer.capture_frame();
        if out.lock().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    let t = out.lock().unwrap().take();
    (id, t)
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
