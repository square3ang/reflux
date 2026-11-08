//! Voice and screen share over LiveKit.
//!
//! Fluxer hands out LiveKit grants over the gateway: send Voice State Update (op 4), receive
//! VOICE_SERVER_UPDATE with `endpoint` + `token`, then call [`Call::connect`] with them.
//! Participant identities are `user_{user_id}_{connection_id}` ([`user_of`]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{FutureExt, StreamExt};
use livekit::options::TrackPublishOptions;

/// The screen-share codec choices (`preferredScreenShareCodec`).
pub use livekit::options::VideoCodec;
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

// `real.rs` is a non-mod-rs file, so these paths are relative to `src/`.
#[path = "audio.rs"]
pub mod audio;
#[path = "mesh.rs"]
pub mod mesh;
#[path = "published.rs"]
mod published;
pub use published::Published;
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
    /// The gate's speaking state (`None`: push-to-talk or studio has no gate).
    pub fn mic_speaking(&self) -> Option<bool> {
        self.audio.as_ref().and_then(|a| a.speaking())
    }

    pub fn mic_level(&self) -> f32 {
        self.audio.as_ref().map_or(0.0, |a| a.level())
    }

    /// Whether DeepFilterNet is loading, running or failed (RNNoise stands in until it runs).
    pub fn noise_state(&self) -> crate::NoiseState {
        self.audio.as_ref().map_or(crate::NoiseState::Idle, |a| a.noise_state())
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
        // `AudioPublishOptions`: `dtx: false, red: true` (the SDK drops RED under E2EE, as livekit-client does).
        let opts = TrackPublishOptions { source: TrackSource::Microphone, dtx: false, red: true, ..Default::default() };
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
        let opts = TrackPublishOptions { source: TrackSource::ScreenshareAudio, dtx: false, red: true, ..Default::default() };
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
        let ended = Arc::new(AtomicBool::new(false));
        let slot: Arc<Mutex<Option<NativeVideoSource>>> = Arc::default();
        let (first, size) = tokio::sync::oneshot::channel();
        spawn_capture(stop.clone(), ended.clone(), slot.clone(), first, quality, pick);
        let Ok((width, height)) = size.await else {
            return Err(Error::Capture);
        };
        let (width, height) = quality.fit(width, height);
        let source = NativeVideoSource::new(VideoResolution { width, height }, true);
        let (sid, track) = match self.publish_screen(&source, quality, width, height).await {
            Ok(t) => t,
            Err(e) => {
                stop.store(true, Ordering::Relaxed);
                return Err(e);
            }
        };
        *slot.lock().unwrap() = Some(source);
        Ok(ScreenShare { sid, stop, ended, track })
    }

    /// Publishes a prepared screen source; [`start_screen`] and the live test share this so both
    /// apply the same resolution, frame rate, bitrate and degradation settings.
    async fn publish_screen(&self, source: &NativeVideoSource, quality: ScreenQuality, width: u32, height: u32) -> Result<(TrackSid, RtcVideoTrack), Error> {
        let track = LocalVideoTrack::create_video_track("screen_share", RtcVideoSource::Native(source.clone()));
        let rtc = track.rtc_track();
        let sid = self.publish(LocalTrack::Video(track), quality.publish_options(width, height), (width, height)).await?;
        Ok((sid, rtc))
    }

    pub async fn stop_screen(&self, share: ScreenShare) -> Result<(), Error> {
        share.stop.store(true, Ordering::Relaxed);
        self.unpublish(&share.sid).await
    }

    /// The first subscribed screen share of `user`.
    pub fn screen_track(&self, user: u64) -> Option<RtcVideoTrack> {
        self.video_track(user, TrackSource::Screenshare)
    }

    /// Whether `user` publishes a screen share (subscribed or not; P2P: one that reaches us).
    pub fn has_stream(&self, user: u64) -> bool {
        let room = match &self.link {
            Link::Room(r) => r,
            Link::Mesh(m) => return m.remote_video(user, mesh_src(TrackSource::Screenshare)).is_some(),
        };
        room.remote_participants().values().filter(|p| user_of(&p.identity().0) == Some(user)).any(|p| p.track_publications().values().any(|t| t.source() == TrackSource::Screenshare))
    }

    /// `ScreenShareSubscriptionPolicy`: a screen share and its audio are subscribed only while
    /// watched. Subscribing on watch also makes the SFU ask the sharer for a keyframe, so a
    /// still screen shows at once instead of staying black until it changes.
    pub fn sync_streams(&self, watching: Option<u64>, previewing: Option<u64>) {
        let Link::Room(room) = &self.link else { return };
        // `set_subscribed` spawns the SFU update on the ambient runtime; this runs on the UI thread.
        let _rt = self.rt.enter();
        for p in room.remote_participants().values() {
            let user = user_of(&p.identity().0);
            // A watched stream plays; a hovered one is only a picture, so its audio stays off.
            let (watch, preview) = (user.is_some_and(|u| Some(u) == watching), user.is_some_and(|u| Some(u) == previewing));
            for t in p.track_publications().values() {
                let want = match t.source() {
                    TrackSource::Screenshare => watch || preview,
                    TrackSource::ScreenshareAudio => watch,
                    _ => continue,
                };
                if t.is_desired() != want {
                    t.set_subscribed(want);
                }
            }
        }
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
    pub async fn publish_camera(&self, quality: CameraQuality) -> Result<Camera, Error> {
        let (width, height) = (quality.width, quality.height);
        let source = NativeVideoSource::new(VideoResolution { width, height }, false);
        let track = LocalVideoTrack::create_video_track("camera", RtcVideoSource::Native(source.clone()));
        let opts = TrackPublishOptions {
            source: TrackSource::Camera,
            video_codec: VideoCodec::H264,
            video_encoder: encoder_backend(),
            simulcast: false,
            video_encoding: Some(quality.encoding()),
            ..Default::default()
        };
        let sid = self.publish(LocalTrack::Video(track), opts, (width, height)).await?;
        Ok(Camera { sid, source, buffer: Mutex::new(I420Buffer::new(width, height)) })
    }

    /// A screen share track fed by the caller (a test pattern in live tests), so encoding can
    /// be checked without a capture source or portal dialog. `width`×`height` is the capture
    /// size; the published frame is [`ScreenQuality::fit`] of it, like [`start_screen`].
    pub async fn publish_test_screen(&self, quality: ScreenQuality, width: u32, height: u32) -> Result<Camera, Error> {
        let (width, height) = quality.fit(width, height);
        let source = NativeVideoSource::new(VideoResolution { width, height }, true);
        let (sid, _) = self.publish_screen(&source, quality, width, height).await?;
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

/// `ScreenShareContentHint` (Fluxer's default is `Text`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContentHint {
    Auto,
    Motion,
    Detail,
    #[default]
    Text,
}

/// `ScreenShareScalabilityModePreference`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Svc {
    #[default]
    Auto,
    SingleLayer,
    Temporal,
}

/// `MAX_SCREEN_SHARE_BITRATE`: Fluxer's fallback ceiling (bits/s) when the instance has no
/// `max_screen_share_bitrate` limit.
pub const MAX_SCREEN_SHARE_BITRATE: u64 = 9_000_000;

/// `DIMENSIONS` (`ScreenShareOptions.ts`): the pixel budget of each resolution preset.
const SCREEN_SHARE_RUNGS: [(u32, u32); 6] = [(426, 240), (854, 480), (1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)];

/// `BITRATE_KBPS` for 15 / 30 / 60 fps (Fluxer's 90 and 120 columns equal 60).
const SCREEN_SHARE_KBPS: [[u64; 3]; 6] = [[300, 500, 700], [1200, 2000, 3000], [2000, 3000, 4500], [3000, 4500, 6000], [4000, 5500, 6000], [4500, 6000, 6000]];

fn floor_even(value: f64) -> u32 {
    ((value / 2.0).floor() as u32 * 2).max(2)
}

/// The bitrate table's column: 15 / 30 / 60 fps.
fn fps_column(fps: u32) -> usize {
    if fps >= 60 { 2 } else if fps >= 30 { 1 } else { 0 }
}

/// The bitrate row's frame rate: `resolveScreenShareFrameRate`.
fn fps_bucket(fps: u32) -> u32 {
    if fps >= 60 { 60 } else if fps >= 30 { 30 } else { 15 }
}

/// `resolveBitrateRungForPixels`: the largest preset whose pixels fit the sent frame.
fn bitrate_rung(pixels: u64) -> usize {
    let mut rung = 0;
    for (i, (w, h)) in SCREEN_SHARE_RUNGS.iter().enumerate() {
        if *w as u64 * *h as u64 > pixels {
            break;
        }
        rung = i;
    }
    rung
}

/// A camera track's capture size, frame rate and bitrate cap (`CAMERA_RESOLUTION_PRESET_TABLE`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraQuality {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u64,
}

impl CameraQuality {
    pub fn encoding(&self) -> livekit::options::VideoEncoding {
        livekit::options::VideoEncoding { max_bitrate: self.bitrate, max_framerate: self.fps.max(1) as f64 }
    }
}

/// Screen share quality (`StreamSettings`): frame rate, the tallest frame sent (`None` =
/// source), a bitrate cap in bits/s (`None` = Fluxer's resolution/frame-rate table), the
/// instance's bitrate ceiling, and Advanced's content hint and SVC mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenQuality {
    pub fps: u32,
    pub max_height: Option<u32>,
    pub bitrate: Option<u64>,
    /// `max_screen_share_bitrate` from the instance's limits.
    pub ceiling: u64,
    pub content_hint: ContentHint,
    pub svc: Svc,
    /// `preferredScreenShareCodec`; `None` = automatic.
    pub codec: Option<VideoCodec>,
}

impl Default for ScreenQuality {
    fn default() -> Self {
        Self { fps: 30, max_height: Some(720), bitrate: None, ceiling: MAX_SCREEN_SHARE_BITRATE, content_hint: ContentHint::default(), svc: Svc::default(), codec: None }
    }
}

impl ScreenQuality {
    /// `resolveScreenShareDegradationPreference` for a display share. libwebrtc's native track
    /// has no content hint setter, so the hint acts through this; "motion" is dropped for
    /// display shares (`resolveScreenShareContentHint`) and counts as no hint.
    pub fn degradation(&self, width: u32, height: u32) -> livekit::options::DegradationPreference {
        use livekit::options::DegradationPreference::*;
        let pixels = width as u64 * height as u64;
        let detailed = matches!(self.content_hint, ContentHint::Detail | ContentHint::Text);
        if bitrate_rung(pixels) == SCREEN_SHARE_RUNGS.len() - 1 || detailed || self.max_bitrate(width, height) < 500_000 { MaintainResolution } else { MaintainFramerate }
    }

    /// `resolveScreenShareLayering`: SVC applies to AV1 and VP9 only.
    pub fn scalability_mode(&self, codec: VideoCodec) -> Option<String> {
        matches!(codec, VideoCodec::AV1 | VideoCodec::VP9).then(|| if self.svc == Svc::SingleLayer { "L1T1" } else { "L1T3" }.to_string())
    }

    /// The frame size sent for a `w`×`h` capture (`resolveEffectiveScreenShareDimensions`):
    /// scaled down to the preset's pixel budget, even sides.
    pub fn fit(&self, w: u32, h: u32) -> (u32, u32) {
        let (bw, bh) = match self.max_height {
            None => SCREEN_SHARE_RUNGS[SCREEN_SHARE_RUNGS.len() - 1],
            Some(m) => SCREEN_SHARE_RUNGS.iter().copied().find(|(_, h)| *h == m).unwrap_or_else(|| ((m as u64 * 16).div_ceil(9) as u32, m)),
        };
        let budget = bw as u64 * bh as u64;
        let pixels = w.max(1) as u64 * h.max(1) as u64;
        let (w, h) = if pixels <= budget {
            (w, h)
        } else {
            let scale = (budget as f64 / pixels as f64).sqrt();
            (floor_even(w as f64 * scale), floor_even(h as f64 * scale))
        };
        (w.max(2) & !1, h.max(2) & !1)
    }

    /// The encoder's bitrate cap: the chosen value, else `computeScreenShareBitrateBps`
    /// (`max(rung floor, 0.02 bits/pixel/frame)`, scaled by the instance ceiling).
    pub fn max_bitrate(&self, width: u32, height: u32) -> u64 {
        let pixels = width as u64 * height as u64;
        let chosen = match self.bitrate {
            Some(b) => b,
            None => {
                let scale = (self.ceiling as f64 / MAX_SCREEN_SHARE_BITRATE as f64).max(1.0);
                let floor = SCREEN_SHARE_KBPS[bitrate_rung(pixels)][fps_column(self.fps)] as f64 * 1000.0 * scale;
                let computed = 0.02 * pixels as f64 * fps_bucket(self.fps) as f64 * scale;
                floor.max(computed).round() as u64
            }
        };
        chosen.min(self.ceiling)
    }

    /// `buildScreenShareOptions`' `screenShareEncoding` plus the degradation preference. With an
    /// SVC mode chosen the share is VP9 (software): libwebrtc only offers scalability for AV1 /
    /// VP9, and the NVENC / VA-API factories here encode H.264 only.
    pub fn publish_options(&self, width: u32, height: u32) -> TrackPublishOptions {
        let svc = self.svc != Svc::Auto;
        // The preferred codec wins; automatic keeps H.264 (hardware) unless SVC needs VP9.
        let codec = self.codec.unwrap_or(if svc { VideoCodec::VP9 } else { VideoCodec::H264 });
        TrackPublishOptions {
            source: TrackSource::Screenshare,
            video_codec: codec,
            video_encoder: if svc { VideoEncoderBackend::Software } else { encoder_backend() },
            simulcast: false,
            video_encoding: Some(livekit::options::VideoEncoding { max_bitrate: self.max_bitrate(width, height), max_framerate: self.fps.clamp(1, 120) as f64 }),
            scalability_mode: self.scalability_mode(codec),
            degradation_preference: Some(self.degradation(width, height)),
            ..Default::default()
        }
    }
}

/// A running screen share; stop it with [`Call::stop_screen`]. Dropping it also stops capture.
pub struct ScreenShare {
    sid: TrackSid,
    stop: Arc<AtomicBool>,
    /// The capture ended on its own (the source went away), not through [`Call::stop_screen`].
    ended: Arc<AtomicBool>,
    /// The published track, for drawing our own stream ([`render_video`] reads local tracks too).
    track: RtcVideoTrack,
}

impl ScreenShare {
    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::Relaxed)
    }

    /// Our own published stream, for the self preview.
    pub fn track(&self) -> Option<RtcVideoTrack> {
        Some(self.track.clone())
    }
}

impl Drop for ScreenShare {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Captures on its own thread (the capturer is not `Send`) until `stop`; reports the first
/// frame's size through `first`, or drops it when capture fails or the picker is cancelled.
fn spawn_capture(stop: Arc<AtomicBool>, ended: Arc<AtomicBool>, slot: Arc<Mutex<Option<NativeVideoSource>>>, first: tokio::sync::oneshot::Sender<(u32, u32)>, quality: ScreenQuality, pick: Option<ScreenSource>) {
    std::thread::spawn(move || {
        // Generic = the portal picker (screens and windows) on Wayland/X11 via PipeWire.
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let default = DesktopCaptureSourceType::Generic;
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let default = DesktopCaptureSourceType::Screen;
        let kind = pick.as_ref().map_or(default, |p| p.kind());
        let mut options = DesktopCapturerOptions::new(kind);
        options.set_include_cursor(true);
        let Some(mut capturer) = DesktopCapturer::new(options) else {
            if !stop.load(Ordering::Relaxed) {
                ended.store(true, Ordering::Relaxed);
            }
            return;
        };
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
            // The portal's only entry, whatever it calls it.
            Some(p) if p.id == ScreenSource::PORTAL => list.into_iter().next(),
            Some(p) => list.into_iter().find(|s| s.id() == p.id),
            None => list.into_iter().next(),
        };
        capturer.start_capture(source, callback);
        while !stop.load(Ordering::Relaxed) && !failed.load(Ordering::Relaxed) {
            capturer.capture_frame();
            std::thread::sleep(Duration::from_millis(1000 / quality.fps.clamp(1, 60) as u64));
        }
        if !stop.load(Ordering::Relaxed) {
            ended.store(true, Ordering::Relaxed);
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
    /// The id of [`ScreenSource::portal`]'s pick.
    pub const PORTAL: u64 = u64::MAX;

    /// `setDisplayMediaPortalPreference`: the xdg-desktop-portal picker limited to windows
    /// (`window`) or monitors, for when sources can't be listed (Wayland).
    pub fn portal(window: bool) -> ScreenSource {
        ScreenSource { window, id: Self::PORTAL, title: String::new(), thumbnail: None }
    }

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
        let q = super::ScreenQuality::default();
        assert_eq!(q.fit(2560, 1440), (1280, 720));
        assert_eq!(q.fit(1280, 600), (1280, 600));
        assert_eq!(super::ScreenQuality { max_height: None, ..q }.fit(1921, 1081), (1920, 1080));
        // Area budget, not just the height: 21:9 1440p at 720p ends at 1280×620, not 1720×720.
        assert_eq!(q.fit(3440, 1440), (1482, 620));
        // 480p fits a 16:9 1080p source into its 854×480 pixel budget (852×480 after even flooring).
        assert_eq!(super::ScreenQuality { max_height: Some(480), ..q }.fit(1920, 1080), (852, 480));
        // 1440p is 2560×1440.
        assert_eq!(super::ScreenQuality { max_height: Some(1440), ..q }.fit(3840, 2160), (2560, 1440));
    }

    #[test]
    fn automatic_bitrate_follows_fluxers_table() {
        use super::{ScreenQuality, MAX_SCREEN_SHARE_BITRATE};
        let q = ScreenQuality::default();
        // 720p30: max(medium/30 = 3 Mbps, 0.02 × 921600 × 30).
        assert_eq!(q.max_bitrate(1280, 720), 3_000_000);
        // 480p15 floor.
        assert_eq!(ScreenQuality { max_height: Some(480), fps: 15, ..q }.max_bitrate(854, 480), 1_200_000);
        // 720p15 (the old LiveKit preset silently capped everyone at 15 FPS / 1 Mbps).
        assert_eq!(ScreenQuality { fps: 15, ..q }.max_bitrate(1280, 720), 2_000_000);
        // 1080p60: high/60 = 6 Mbps.
        assert_eq!(ScreenQuality { max_height: Some(1080), fps: 60, ..q }.max_bitrate(1920, 1080), 6_000_000);
        // Source at 4K/60: the 6 Mbps rung floor, but 0.02 bits/pixel/frame computes 9.95 Mbps,
        // so the 9 Mbps default ceiling wins.
        assert_eq!(ScreenQuality { max_height: None, fps: 60, ..q }.max_bitrate(3840, 2160), 9_000_000);
        // A manual cap wins, and the instance ceiling caps both.
        assert_eq!(ScreenQuality { bitrate: Some(8_000_000), ..q }.max_bitrate(1280, 720), 8_000_000);
        assert_eq!(ScreenQuality { bitrate: Some(50_000_000), ..q }.max_bitrate(1280, 720), MAX_SCREEN_SHARE_BITRATE);
        assert_eq!(ScreenQuality { ceiling: 16_000_000, bitrate: Some(30_000_000), ..q }.max_bitrate(1280, 720), 16_000_000);
        // The whole automatic table scales with a raised instance ceiling (Fluxer's `scale`).
        assert_eq!(ScreenQuality { ceiling: 16_000_000, ..q }.max_bitrate(1280, 720), 5_333_333);
        assert_eq!(ScreenQuality { ceiling: 16_000_000, max_height: Some(1080), fps: 60, ..q }.max_bitrate(1920, 1080), 10_666_667);
    }

    #[test]
    fn encoding_always_carries_the_frame_rate() {
        let q = super::ScreenQuality { max_height: Some(1080), fps: 60, ..Default::default() };
        let enc = q.publish_options(1920, 1080).video_encoding.expect("screen shares always set an encoding");
        assert_eq!(enc.max_bitrate, 6_000_000);
        assert_eq!(enc.max_framerate, 60.0);
        let opts = q.publish_options(1920, 1080);
        assert!(!opts.simulcast);
        assert_eq!(opts.degradation_preference, Some(livekit::options::DegradationPreference::MaintainResolution));
        // An SVC mode only applies to VP9/AV1, so choosing one publishes VP9 (software) and the
        // layering string is set; Automatic stays H.264 with the hardware encoder.
        assert_eq!(opts.video_codec, livekit::options::VideoCodec::H264);
        assert_eq!(opts.scalability_mode, None);
        let svc = super::ScreenQuality { svc: super::Svc::Temporal, ..q };
        let opts = svc.publish_options(1920, 1080);
        assert_eq!(opts.video_codec, livekit::options::VideoCodec::VP9);
        assert_eq!(opts.video_encoder, livekit::webrtc::rtp_sender::VideoEncoderBackend::Software);
        assert_eq!(opts.scalability_mode.as_deref(), Some("L1T3"));
    }

    #[test]
    fn content_hint_and_svc_follow_fluxer() {
        use super::{ContentHint, ScreenQuality, Svc};
        use livekit::options::{DegradationPreference::*, VideoCodec};
        let q = ScreenQuality { content_hint: ContentHint::Motion, ..Default::default() };
        assert_eq!(q.degradation(1280, 720), MaintainFramerate);
        assert_eq!(ScreenQuality { content_hint: ContentHint::Auto, ..q }.degradation(1280, 720), MaintainFramerate);
        assert_eq!(ScreenQuality { content_hint: ContentHint::Text, ..q }.degradation(1280, 720), MaintainResolution);
        assert_eq!(ScreenQuality { max_height: None, ..q }.degradation(3840, 2160), MaintainResolution, "source rung");
        assert_eq!(ScreenQuality { max_height: None, ..q }.degradation(1920, 1080), MaintainFramerate, "a 1080p source is not the source rung");
        assert_eq!(ScreenQuality { bitrate: Some(400_000), ..q }.degradation(1280, 720), MaintainResolution, "low bitrate");
        assert_eq!(q.scalability_mode(VideoCodec::H264), None);
        assert_eq!(q.scalability_mode(VideoCodec::VP9).as_deref(), Some("L1T3"));
        assert_eq!(ScreenQuality { svc: Svc::SingleLayer, ..q }.scalability_mode(VideoCodec::AV1).as_deref(), Some("L1T1"));
    }
}
