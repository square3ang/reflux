//! Inline playback through GStreamer where Rune's `Player` can't decode (Linux): the clip
//! playing in a message or the media viewer, with `VideoPlayer`'s controls (play/pause, time,
//! seek, volume, mute, full screen). Frames land in a `Pixels` like the stream viewer's.

use rune::{Color, El, Pixels, Player, col, el, row, text};

use super::{ShellAccess, ShellState};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::rgba;

pub struct GstClip {
    pub key: String,
    pub player: crate::gst::VideoPlayer,
    pub pixels: Pixels,
    /// 0..1 and muted, as the volume control shows them.
    pub volume: f32,
    pub muted: bool,
    pub failed: bool,
}

/// GStreamer plays here (Rune's own player covers macOS).
pub fn native() -> bool {
    !cfg!(target_os = "macos") && crate::gst::available()
}

/// Plays `url` under `key`, stopping whatever played before (`audio`: no picture).
pub fn start(st: &mut ShellState, key: String, url: String, audio: bool) {
    if !native() {
        let clip = if audio { Player::audio(url) } else { Player::url(url) };
        return super::attachments::play(st, key, clip);
    }
    if let Some((_, old)) = st.media_ui.playing.take() {
        old.pause();
    }
    st.media_ui.gst = None;
    let pixels = Pixels::default();
    let Some(mut player) = crate::gst::VideoPlayer::new(&url, pixels.clone()) else { return };
    player.set_volume(1.0, false);
    player.play();
    st.media_ui.gst = Some(GstClip { key, player, pixels, volume: 1.0, muted: false, failed: false });
}

pub fn get<'a>(st: &'a ShellState, key: &str) -> Option<&'a GstClip> {
    st.media_ui.gst.as_ref().filter(|c| c.key == key)
}

fn with<S: 'static>(acc: ShellAccess<S>, key: &str, s: &mut S, f: impl FnOnce(&mut GstClip)) {
    if let Some(c) = acc(s).media_ui.gst.as_mut().filter(|c| c.key == key) {
        f(c);
    }
}

pub fn toggle<S: 'static>(acc: ShellAccess<S>, key: &str, s: &mut S) {
    with(acc, key, s, |c| if c.player.playing { c.player.pause() } else { c.player.play() });
}

pub fn seek<S: 'static>(acc: ShellAccess<S>, key: &str, s: &mut S, secs: f64) {
    with(acc, key, s, |c| c.player.seek(secs));
}

/// Once a frame: the bus, and a wake while playing so the time and seek bar move.
pub fn poll(st: &mut ShellState, tx: &mut rune::Tx) -> bool {
    let Some(c) = st.media_ui.gst.as_mut() else { return false };
    let mut changed = c.player.poll();
    if changed && !c.player.playing && !c.player.ended {
        c.failed = true;
    }
    if c.player.playing {
        tx.wake_after(std::time::Duration::from_millis(250));
        changed = true;
    }
    changed
}

fn hud<S: 'static>(glyph: &'static rune::Icon, label: &str, f: impl Fn(&mut S) + 'static) -> El {
    row()
        .size(32.0, 32.0)
        .center()
        .radius(6.0)
        .hover(rgba(255, 255, 255, 0.14))
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.to_owned())
        .child(rune::icon(glyph, 20.0).color(Color::WHITE))
        .on_press(f)
}

/// An inline video at `(w, h)`: the frames and the control bar. `full`: open the media viewer.
pub fn video<S: 'static>(c: &GstClip, size: (f32, f32), p: &Palette, acc: ShellAccess<S>, full: Option<std::rc::Rc<dyn Fn(&mut S)>>) -> El {
    let (w, h) = size;
    let key = c.key.clone();
    let frame = el().size(w, h).radius(dim::MEDIA_BORDER_RADIUS).clip_rounded(dim::MEDIA_BORDER_RADIUS).fill(Color::BLACK);
    if c.failed {
        return frame.child(
            col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().child(text("This video can't be played.").font(font(13.0, 500.0).lh(18.0)).color(rgba(255, 255, 255, 0.8))),
        );
    }
    let dur = c.player.duration().unwrap_or(0.0);
    let now = c.player.position().unwrap_or(0.0);
    let playing = c.player.playing;
    let (k1, k2, k3, k4) = (key.clone(), key.clone(), key.clone(), key.clone());
    let seek = crate::ui::Slider::new(format!("{key}-seek"), now as f32, 0.0, dur.max(0.01) as f32)
        .mini()
        .on_change(move |s: &mut S, v| seek(acc, &k2, s, v as f64))
        .el(p);
    let volume = crate::ui::Slider::new(format!("{key}-volume"), if c.muted { 0.0 } else { c.volume * 100.0 }, 0.0, 100.0)
        .mini()
        .on_change(move |s: &mut S, v| {
            with(acc, &k4, s, |c| {
                (c.volume, c.muted) = (v / 100.0, v <= 0.0);
                c.player.set_volume(c.volume as f64, c.muted);
            })
        })
        .el(p);
    let mute_icon = if c.muted || c.volume <= 0.0 { &icons::SPEAKER_X_FILL } else { &icons::SPEAKER_HIGH_FILL };
    let mut bar = row()
        .pin(Some(0.0), None, Some(0.0), Some(0.0))
        .h(40.0)
        .px(4.0)
        .gap(4.0)
        .center_y()
        .fill(rgba(0, 0, 0, 0.6))
        .child(hud(if playing { &icons::PAUSE_FILL } else { &icons::PLAY_FILL }, if playing { "Pause" } else { "Play" }, move |s: &mut S| toggle(acc, &k1, s)))
        .child(text(format!("{} / {}", super::attachments::clock(now), super::attachments::clock(dur))).font(font(12.0, 500.0).mono().lh(16.0)).color(Color::WHITE))
        .child(el().grow(1.0).min_w(0.0).px(8.0).child(seek))
        .child(hud(mute_icon, "Mute", move |s: &mut S| {
            with(acc, &k3, s, |c| {
                c.muted = !c.muted;
                c.player.set_volume(c.volume as f64, c.muted);
            })
        }))
        .child(el().w(64.0).child(volume));
    if let Some(full) = full {
        bar = bar.child(hud(&icons::CORNERS_OUT, "Full screen", move |s: &mut S| full(s)));
    }
    frame.child(el().w_full().h_full().pixels_fit(&c.pixels, rune::SurfaceFit::Fit)).child(bar)
}
