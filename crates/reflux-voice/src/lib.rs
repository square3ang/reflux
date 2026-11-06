//! Voice and screen share over LiveKit.
//!
//! Fluxer hands out LiveKit grants over the gateway: send Voice State Update (op 4), receive
//! VOICE_SERVER_UPDATE with `endpoint` + `token`, then call [`Call::connect`] with them.
//! Skeleton only: the gateway side, e2ee_key, P2P grants and devices UI come later.

use livekit::options::TrackPublishOptions;
use livekit::prelude::*;
use livekit::webrtc::video_source::native::NativeVideoSource;
use livekit::webrtc::video_source::{RtcVideoSource, VideoResolution};
use livekit::{AudioError, PlatformAudio};
use tokio::sync::mpsc::UnboundedReceiver;

pub use livekit;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Room(#[from] RoomError),
    #[error(transparent)]
    Audio(#[from] AudioError),
}

pub struct Call {
    pub room: Room,
    pub events: UnboundedReceiver<RoomEvent>,
    // WebRTC's own audio device module: mic capture + speaker playout with its AEC/NS/AGC.
    audio: Option<PlatformAudio>,
}

impl Call {
    /// `with_audio` opens the platform audio devices; it must happen before connecting.
    pub async fn connect(endpoint: &str, token: &str, with_audio: bool) -> Result<Self, Error> {
        let audio = with_audio.then(PlatformAudio::new).transpose()?;
        let (room, events) = Room::connect(endpoint, token, RoomOptions::default()).await?;
        Ok(Self { room, events, audio })
    }

    /// Publishes the default microphone. No-op without audio.
    pub async fn publish_mic(&self) -> Result<(), Error> {
        let Some(audio) = &self.audio else { return Ok(()) };
        let track = LocalAudioTrack::create_audio_track("microphone", audio.rtc_source());
        let opts = TrackPublishOptions { source: TrackSource::Microphone, ..Default::default() };
        self.room.local_participant().publish_track(LocalTrack::Audio(track), opts).await?;
        Ok(())
    }

    /// Publishes a screen share track; push frames into the returned source
    /// (`capture_frame` for I420/NV12, `capture_dmabuf_frame` for zero-copy PipeWire buffers).
    pub async fn publish_screen(&self, width: u32, height: u32) -> Result<NativeVideoSource, Error> {
        let source = NativeVideoSource::new(VideoResolution { width, height }, true);
        let track = LocalVideoTrack::create_video_track(
            "screen_share",
            RtcVideoSource::Native(source.clone()),
        );
        let opts = TrackPublishOptions { source: TrackSource::Screenshare, ..Default::default() };
        self.room.local_participant().publish_track(LocalTrack::Video(track), opts).await?;
        Ok(source)
    }

    pub async fn leave(self) -> Result<(), Error> {
        Ok(self.room.close().await?)
    }
}
