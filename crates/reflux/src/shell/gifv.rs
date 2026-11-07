//! `EmbedGifv`: a GIF link's video plays muted and looping in place of its still, while its
//! tile is on screen and GIFs autoplay (else while hovered). `crate::gst::VideoPlayer` decodes on
//! Linux (GStreamer) and Windows (Media Foundation), Rune's `Player` on macOS.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use rune::{El, Pixels, Player, PlayerState, SurfaceFit, el};

use super::ShellState;

/// Players at once; more on screen show their stills.
const MAX: usize = 6;
/// A tile not drawn for this long stops its player.
const LINGER: Duration = Duration::from_secs(2);

enum Backend {
    Gst { player: crate::gst::VideoPlayer, pixels: Pixels },
    Rune(Player),
}

pub struct Clip {
    backend: Backend,
    started: bool,
}

#[derive(Default)]
pub struct Gifv {
    clips: HashMap<String, Clip>,
    /// Tiles drawn by the view, with their video URL and when they were last drawn.
    seen: RefCell<HashMap<String, (String, Instant)>>,
}

fn supported() -> bool {
    super::clip::native() || cfg!(target_os = "macos")
}

/// From the view: `key` wants `url` playing. Returns the playing video at `size` once its
/// first frame arrived (the still shows until then).
pub fn want(st: &ShellState, key: &str, url: &str, size: (f32, f32)) -> Option<El> {
    if !supported() {
        return None;
    }
    st.media_ui.gifv.seen.borrow_mut().insert(key.to_string(), (url.to_string(), Instant::now()));
    let c = st.media_ui.gifv.clips.get(key).filter(|c| c.started)?;
    let e = el().size(size.0, size.1);
    Some(match &c.backend {
        Backend::Gst { pixels, .. } => e.pixels_fit(pixels, SurfaceFit::Fill),
        Backend::Rune(p) => e.video(p, SurfaceFit::Fill),
    })
}

fn open(url: &str) -> Option<Backend> {
    if super::clip::native() {
        let pixels = Pixels::default();
        let mut player = crate::gst::VideoPlayer::new(url, super::clip::frames(&pixels))?;
        player.set_volume(0.0, true);
        player.play();
        return Some(Backend::Gst { player, pixels });
    }
    let p = Player::url(url);
    p.set_muted(true);
    p.set_looping(true);
    p.play();
    Some(Backend::Rune(p))
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
        if let Some(backend) = open(&url) {
            g.clips.insert(k, Clip { backend, started: false });
        }
    }
    for c in g.clips.values_mut() {
        let ready = match &mut c.backend {
            Backend::Gst { player, pixels } => {
                player.poll();
                if player.ended {
                    player.play();
                }
                pixels.size().0 > 0
            }
            // A failed clip keeps its still.
            Backend::Rune(p) => p.state() == PlayerState::Playing && p.natural_size().is_some(),
        };
        if !c.started && ready {
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
