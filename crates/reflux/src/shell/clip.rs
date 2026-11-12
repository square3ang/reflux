//! Inline playback where Rune's `Player` can't decode (GStreamer on Linux, Media Foundation on
//! Windows, both `crate::gst::VideoPlayer`): the clip
//! playing in a message or the media viewer, with `VideoPlayer`'s controls (play/pause, time,
//! seek, volume, mute, full screen). Frames land in a `Pixels` like the stream viewer's.

use rune::{Color, El, Pixels, Player, col, el, row, text};

use super::{ShellAccess, ShellState};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::rgba;
use crate::i18n::t;

pub struct GstClip {
    pub key: String,
    pub player: crate::gst::VideoPlayer,
    pub pixels: Pixels,
    /// 0..1 and muted, as the volume control shows them.
    pub volume: f32,
    pub muted: bool,
    pub failed: bool,
}

/// Our `VideoPlayer` plays here (Rune's own player covers macOS).
pub fn native() -> bool {
    !cfg!(target_os = "macos") && crate::gst::video_available()
}

/// A `VideoPlayer` sink that shows its frames in `pixels`.
pub fn frames(pixels: &Pixels) -> impl FnMut(u32, u32, Vec<u8>) + Send + 'static {
    let pixels = pixels.clone();
    move |w, h, rgba| pixels.set(w, h, rgba)
}

/// Plays `url` under `key`, stopping whatever played before (`audio`: no picture).
pub fn start(st: &mut ShellState, key: String, url: String, audio: bool) {
    if !native() {
        let clip = if audio { Player::audio(url) } else { Player::url(url) };
        // Rune's player has mute but no volume.
        clip.set_muted(st.prefs.media_muted);
        return super::attachments::play(st, key, clip);
    }
    if let Some((_, old)) = st.media_ui.playing.take() {
        old.pause();
    }
    st.media_ui.gst = None;
    let pixels = Pixels::default();
    let Some(mut player) = crate::gst::VideoPlayer::new(&url, frames(&pixels)) else { return };
    let (volume, muted) = (st.prefs.media_volume, st.prefs.media_muted);
    player.set_volume(volume as f64, muted);
    player.play();
    st.media_ui.gst = Some(GstClip { key, player, pixels, volume, muted, failed: false });
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

pub fn set_rate<S: 'static>(acc: ShellAccess<S>, key: &str, s: &mut S, rate: f64) {
    if let Some((_, c)) = acc(s).media_ui.playing.as_ref().filter(|(k, _)| k == key) {
        c.set_rate(rate as f32);
    }
    with(acc, key, s, |c| c.player.set_rate(rate));
}

/// Sets the shared media volume / mute (saved), and the clip `key`'s if it plays.
pub fn set_volume<S: 'static>(acc: ShellAccess<S>, key: &str, s: &mut S, volume: f32, muted: bool) {
    let st = acc(s);
    (st.prefs.media_volume, st.prefs.media_muted) = (volume.clamp(0.0, 1.0), muted);
    let prefs = Box::new(st.prefs.clone());
    st.intents.push(super::Intent::Prefs(prefs));
    if let Some((_, c)) = st.media_ui.playing.as_ref().filter(|(k, _)| k == key) {
        c.set_muted(muted);
    }
    with(acc, key, s, |c| {
        (c.volume, c.muted) = (volume.clamp(0.0, 1.0), muted);
        c.player.set_volume(c.volume as f64, c.muted);
    });
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

/// Opens the media viewer.
pub(crate) type Opener<S> = std::rc::Rc<dyn Fn(&mut S)>;

/// An inline video at `(w, h)`: the frames and the control bar. `full`: open the media viewer.
pub fn video<S: 'static>(c: &GstClip, size: (f32, f32), p: &Palette, acc: ShellAccess<S>, full: Option<Opener<S>>) -> El {
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
        .on_change(move |s: &mut S, v| set_volume(acc, &k4, s, v / 100.0, v <= 0.0))
        .el(p);
    let mute_icon = if c.muted || c.volume <= 0.0 { &icons::SPEAKER_X_FILL } else { &icons::SPEAKER_HIGH_FILL };
    let mut bar = row()
        .pin(Some(0.0), None, Some(0.0), Some(0.0))
        .h(40.0)
        .px(4.0)
        .gap(4.0)
        .center_y()
        .fill(rgba(0, 0, 0, 0.6))
        .child(hud(if playing { &icons::PAUSE_FILL } else { &icons::PLAY_FILL }, if playing { t("Pause") } else { t("Play") }, move |s: &mut S| toggle(acc, &k1, s)))
        .child(text(format!("{} / {}", super::attachments::clock(now), super::attachments::clock(dur))).font(font(12.0, 500.0).mono().lh(16.0)).color(Color::WHITE))
        .child(el().grow(1.0).min_w(0.0).px(8.0).child(seek))
        .child(hud(mute_icon, t("Mute"), move |s: &mut S| {
            let (v, m) = (acc(s).prefs.media_volume, acc(s).prefs.media_muted);
            set_volume(acc, &k3, s, v, !m)
        }))
        .child(el().w(64.0).child(volume));
    if let Some(full) = full {
        bar = bar.child(hud(&icons::CORNERS_OUT_FILL, crate::i18n::t("Enter fullscreen"), move |s: &mut S| full(s)));
    }
    frame.child(el().w_full().h_full().pixels_fit(&c.pixels, rune::SurfaceFit::Fit)).child(bar)
}
