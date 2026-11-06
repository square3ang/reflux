//! UI sounds (`notification/utils/SoundUtils.ts`): Fluxer's own files, played on one audio thread
//! that opens the default output the first time a sound plays.

use std::io::Cursor;
use std::sync::OnceLock;
use std::sync::mpsc::{Sender, channel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Message,
    Mute,
    Unmute,
    Deaf,
    Undeaf,
    UserJoin,
    UserLeave,
    IncomingRing,
    VoiceDisconnect,
    CameraOn,
    CameraOff,
    StreamStart,
    StreamStop,
    UserMove,
}

impl Sound {
    fn bytes(self) -> &'static [u8] {
        macro_rules! file {
            ($f:literal) => {
                include_bytes!(concat!("../../../../assets/media/sounds/", $f))
            };
        }
        match self {
            Sound::Message => file!("message.mp3"),
            Sound::Mute => file!("mute.mp3"),
            Sound::Unmute => file!("unmute.mp3"),
            Sound::Deaf => file!("deaf.mp3"),
            Sound::Undeaf => file!("undeaf.mp3"),
            Sound::UserJoin => file!("user-join.mp3"),
            Sound::UserLeave => file!("user-leave.mp3"),
            Sound::IncomingRing => file!("incoming-ring.mp3"),
            Sound::VoiceDisconnect => file!("voice-disconnect.mp3"),
            Sound::CameraOn => file!("camera-on.mp3"),
            Sound::CameraOff => file!("camera-off.mp3"),
            Sound::StreamStart => file!("stream-start.mp3"),
            Sound::StreamStop => file!("stream-stop.mp3"),
            Sound::UserMove => file!("user-move.mp3"),
        }
    }
}

/// Plays `s` without waiting. Never plays headless (snapshots).
pub fn play(s: Sound) {
    static TX: OnceLock<Option<Sender<Sound>>> = OnceLock::new();
    if rune::is_headless() {
        return;
    }
    let tx = TX.get_or_init(|| {
        let (tx, rx) = channel::<Sound>();
        std::thread::Builder::new()
            .name("reflux-sound".into())
            .spawn(move || {
                let sink = match rodio::DeviceSinkBuilder::open_default_sink() {
                    Ok(s) => s,
                    Err(e) => return eprintln!("reflux: sound output: {e}"),
                };
                for s in rx {
                    match rodio::play(sink.mixer(), Cursor::new(s.bytes())) {
                        Ok(p) => p.detach(),
                        Err(e) => eprintln!("reflux: sound {s:?}: {e}"),
                    }
                }
            })
            .ok()?;
        Some(tx)
    });
    if let Some(tx) = tx {
        let _ = tx.send(s);
    }
}
