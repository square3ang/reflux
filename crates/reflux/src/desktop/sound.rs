//! UI sounds (`notification/utils/SoundUtils.ts`): Fluxer's own files (or the user's custom file
//! for a sound), played on one audio thread that opens the default output the first time a sound
//! plays.

use std::io::Cursor;
use std::sync::OnceLock;
use std::sync::mpsc::{Sender, channel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Message,
    DirectMessage,
    SameChannelMessage,
    Mute,
    Unmute,
    Deaf,
    Undeaf,
    UserJoin,
    UserLeave,
    UserMove,
    ViewerJoin,
    ViewerLeave,
    VoiceDisconnect,
    IncomingRing,
    CameraOn,
    CameraOff,
    ScreenShareStart,
    ScreenShareStop,
}

/// Every sound, in the order of Fluxer's sound settings (`getSoundLabels`).
pub const ALL: [Sound; 18] = [
    Sound::Message,
    Sound::DirectMessage,
    Sound::SameChannelMessage,
    Sound::Mute,
    Sound::Unmute,
    Sound::Deaf,
    Sound::Undeaf,
    Sound::UserJoin,
    Sound::UserLeave,
    Sound::UserMove,
    Sound::ViewerJoin,
    Sound::ViewerLeave,
    Sound::VoiceDisconnect,
    Sound::IncomingRing,
    Sound::CameraOn,
    Sound::CameraOff,
    Sound::ScreenShareStart,
    Sound::ScreenShareStop,
];

/// `MAX_CUSTOM_SOUND_BYTES`.
pub const CUSTOM_MAX_BYTES: u64 = 2 * 1024 * 1024;

impl Sound {
    /// Fluxer's `SoundType` value (the settings key).
    pub fn id(self) -> &'static str {
        match self {
            Sound::Message => "message",
            Sound::DirectMessage => "direct-message",
            Sound::SameChannelMessage => "same-channel-message",
            Sound::Mute => "mute",
            Sound::Unmute => "unmute",
            Sound::Deaf => "deaf",
            Sound::Undeaf => "undeaf",
            Sound::UserJoin => "user-join",
            Sound::UserLeave => "user-leave",
            Sound::UserMove => "user-move",
            Sound::ViewerJoin => "viewer-join",
            Sound::ViewerLeave => "viewer-leave",
            Sound::VoiceDisconnect => "voice-disconnect",
            Sound::IncomingRing => "incoming-ring",
            Sound::CameraOn => "camera-on",
            Sound::CameraOff => "camera-off",
            Sound::ScreenShareStart => "screen-share-start",
            Sound::ScreenShareStop => "screen-share-stop",
        }
    }

    /// `SoundLabels.ts` (English; translate with `t`).
    pub fn label(self) -> &'static str {
        match self {
            Sound::Message => "Community message notifications",
            Sound::DirectMessage => "Direct message notifications",
            Sound::SameChannelMessage => "Current channel message notifications",
            Sound::Mute => "Voice mute",
            Sound::Unmute => "Voice unmute",
            Sound::Deaf => "Voice deafen",
            Sound::Undeaf => "Voice undeafen",
            Sound::UserJoin => "User joins channel",
            Sound::UserLeave => "User leaves channel",
            Sound::UserMove => "User moves channel",
            Sound::ViewerJoin => "Viewer joins stream",
            Sound::ViewerLeave => "Viewer leaves stream",
            Sound::VoiceDisconnect => "Voice disconnected",
            Sound::IncomingRing => "Incoming call",
            Sound::CameraOn => "Camera on",
            Sound::CameraOff => "Camera off",
            Sound::ScreenShareStart => "Screen share start",
            Sound::ScreenShareStop => "Screen share stop",
        }
    }

    fn bytes(self) -> &'static [u8] {
        macro_rules! file {
            ($f:literal) => {
                include_bytes!(concat!("../../../../assets/media/sounds/", $f))
            };
        }
        match self {
            Sound::Message | Sound::DirectMessage => file!("message.mp3"),
            Sound::SameChannelMessage => file!("in-channel-notification.ogg"),
            Sound::Mute => file!("mute.mp3"),
            Sound::Unmute => file!("unmute.mp3"),
            Sound::Deaf => file!("deaf.mp3"),
            Sound::Undeaf => file!("undeaf.mp3"),
            Sound::UserJoin => file!("user-join.mp3"),
            Sound::UserLeave => file!("user-leave.mp3"),
            Sound::UserMove => file!("user-move.mp3"),
            Sound::ViewerJoin => file!("viewer-join.mp3"),
            Sound::ViewerLeave => file!("viewer-leave.mp3"),
            Sound::VoiceDisconnect => file!("voice-disconnect.mp3"),
            Sound::IncomingRing => file!("incoming-ring.mp3"),
            Sound::CameraOn => file!("camera-on.mp3"),
            Sound::CameraOff => file!("camera-off.mp3"),
            Sound::ScreenShareStart => file!("stream-start.mp3"),
            Sound::ScreenShareStop => file!("stream-stop.mp3"),
        }
    }
}

/// Where a custom sound file for `s` is kept (`<config>/sounds/<id>`).
pub fn custom_path(s: Sound) -> Option<std::path::PathBuf> {
    Some(crate::session::path()?.parent()?.join("sounds").join(s.id()))
}

/// Plays `s` at `volume` (0..=1) without waiting, from `custom` when that file reads. Never
/// plays headless (snapshots).
pub fn play(s: Sound, volume: f32, custom: Option<&str>) {
    /// A sound, its volume and a custom file's bytes.
    type Job = (Sound, f32, Option<Vec<u8>>);
    static TX: OnceLock<Option<Sender<Job>>> = OnceLock::new();
    if rune::is_headless() {
        return;
    }
    let tx = TX.get_or_init(|| {
        let (tx, rx) = channel::<(Sound, f32, Option<Vec<u8>>)>();
        std::thread::Builder::new()
            .name("reflux-sound".into())
            .spawn(move || {
                let sink = match rodio::DeviceSinkBuilder::open_default_sink() {
                    Ok(s) => s,
                    Err(e) => return eprintln!("reflux: sound output: {e}"),
                };
                for (s, volume, custom) in rx {
                    let played = match custom {
                        Some(b) => rodio::play(sink.mixer(), Cursor::new(b)),
                        None => rodio::play(sink.mixer(), Cursor::new(s.bytes())),
                    };
                    match played {
                        Ok(p) => {
                            p.set_volume(volume);
                            p.detach()
                        }
                        Err(e) => eprintln!("reflux: sound {s:?}: {e}"),
                    }
                }
            })
            .ok()?;
        Some(tx)
    });
    let custom = custom.and_then(|p| std::fs::read(p).ok());
    if let Some(tx) = tx {
        let _ = tx.send((s, volume.clamp(0.0, 1.0), custom));
    }
}
