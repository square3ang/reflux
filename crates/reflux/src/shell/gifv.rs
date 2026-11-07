//! `EmbedGifv`: a GIF link's video plays muted and looping in place of its still, through
//! GStreamer, while its tile is on screen and GIFs autoplay (else while hovered).

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use rune::{El, Pixels, el};

use super::ShellState;

/// Players at once; more on screen show their stills.
const MAX: usize = 6;
/// A tile not drawn for this long stops its player.
const LINGER: Duration = Duration::from_secs(2);

pub struct Clip {
    player: crate::gst::VideoPlayer,
    pixels: Pixels,
    started: bool,
}

#[derive(Default)]
pub struct Gifv {
    clips: HashMap<String, Clip>,
    /// Tiles drawn by the view, with their video URL and when they were last drawn.
    seen: RefCell<HashMap<String, (String, Instant)>>,
}

/// From the view: `key` wants `url` playing. Returns its frames once the first one arrived.
pub fn want(st: &ShellState, key: &str, url: &str) -> Option<Pixels> {
    if !super::clip::native() {
        return None;
    }
    st.media_ui.gifv.seen.borrow_mut().insert(key.to_string(), (url.to_string(), Instant::now()));
    st.media_ui.gifv.clips.get(key).filter(|c| c.started).map(|c| c.pixels.clone())
}

/// The playing frame at `size`.
pub fn frame(px: &Pixels, size: (f32, f32)) -> El {
    el().size(size.0, size.1).pixels_fit(px, rune::SurfaceFit::Fill)
}

/// Once a frame: starts players for tiles drawn just now, loops ended ones, stops the rest.
pub fn poll(st: &mut ShellState, tx: &mut rune::Tx) -> bool {
    let g = &mut st.media_ui.gifv;
    let now = Instant::now();
    let seen: Vec<(String, String)> = {
        let mut s = g.seen.borrow_mut();
        s.retain(|_, (_, at)| now.duration_since(*at) < LINGER);
        s.iter().map(|(k, (u, _))| (k.clone(), u.clone())).collect()
    };
    let before = g.clips.len();
    g.clips.retain(|k, _| seen.iter().any(|(s, _)| s == k));
    let mut changed = g.clips.len() != before;
    for (k, url) in seen {
        if g.clips.len() >= MAX || g.clips.contains_key(&k) {
            continue;
        }
        let pixels = Pixels::default();
        if let Some(mut player) = crate::gst::VideoPlayer::new(&url, pixels.clone()) {
            player.set_volume(0.0, true);
            player.play();
            g.clips.insert(k, Clip { player, pixels, started: false });
        }
    }
    for c in g.clips.values_mut() {
        c.player.poll();
        if c.player.ended {
            c.player.play();
        }
        // The still shows until the first frame lands.
        if !c.started && c.pixels.size().0 > 0 {
            (c.started, changed) = (true, true);
        }
    }
    if !g.clips.is_empty() {
        // Redraw so tiles keep reporting themselves (and dropped ones stop).
        tx.wake_after(Duration::from_millis(500));
        changed = true;
    }
    changed
}
