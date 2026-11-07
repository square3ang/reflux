//! Pop-out windows (`PopoutWindow`, `VoicePopoutHost`, `PiPOverlay`): one secondary Rune
//! window that shows the call's tiles, one participant's camera, or a watched stream; picture in
//! picture is the same window small and on top. It has its own state, fed from the main
//! window: video goes through the shared `Pixels`, the tile list through [`sync`].
// shortcut: on Wayland the compositor ignores always-on-top and placement (winit can't ask for
// them there), so picture in picture is just a small window.

use std::cell::RefCell;

use reflux_model::Snowflake;
use rune::{AppHandle, AppSpec, Color, Cx, El, Pixels, WindowHandle, WindowOptions, WindowSpec, col, el, row, text};

use crate::State;
use crate::theme::font;
use crate::ui::{rgb, rgba};

#[derive(Default)]
pub struct Popout {
    title: String,
    /// One video (a camera or stream); `None` = the call grid.
    video: Option<Pixels>,
    tiles: Vec<Tile>,
}

#[derive(Clone)]
struct Tile {
    name: String,
    speaking: bool,
    video: Option<Pixels>,
}

thread_local! {
    static WINDOW: RefCell<Option<(AppHandle<Popout>, WindowHandle)>> = const { RefCell::new(None) };
    /// What the open call pop-out shows: (name, speaking, has video) per tile; `None` = not a
    /// call pop-out.
    static SHOWN: RefCell<Option<Vec<(String, bool, bool)>>> = const { RefCell::new(None) };
    /// The pop-out shows a watched stream.
    static STREAM: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The watched stream ended or was closed: a stream pop-out goes with it.
pub fn stream_ended() {
    if STREAM.with(|s| s.get()) {
        close();
    }
}

/// Adds the (hidden) pop-out window to the app.
pub fn register(app: &mut rune::App<State>) {
    let spec = AppSpec { fonts: crate::theme::FONTS, families: crate::theme::FAMILIES, min_size: Some((160.0, 90.0)), ..AppSpec::new("Fluxer", (960.0, 540.0), Popout::default, view) };
    let options = WindowOptions { visible: false, min_size: Some((160.0, 90.0)), ..Default::default() };
    let handles = app.window(WindowSpec::new("voice-popout", spec).options(options));
    WINDOW.with(|w| *w.borrow_mut() = Some(handles));
}

fn view(s: &Popout, _cx: &mut Cx<Popout>) -> El {
    let root = col().w_full().h_full().fill(rgb(0x0d, 0x0d, 0x0f));
    if let Some(px) = &s.video {
        return root.child(el().w_full().h_full().pixels_fit(px, rune::SurfaceFit::Fit)).child(el().pin(Some(8.0), None, None, Some(8.0)).child(pill(&s.title)));
    }
    let n = s.tiles.len().max(1);
    let cols = (n as f32).sqrt().ceil() as usize;
    let mut grid = root.gap(8.0).padding(8.0);
    for chunk in s.tiles.chunks(cols) {
        grid = grid.child(row().w_full().grow(1.0).min_h(0.0).gap(8.0).children(chunk.iter().map(tile)));
    }
    grid
}

fn pill(name: &str) -> El {
    row().px(6.0).py(3.0).radius(9.0).fill(rgb(0, 0, 0)).border(1.0, rgba(255, 255, 255, 0.1)).child(text(name.to_string()).font(font(12.0, 500.0).lh(14.4)).color(Color::WHITE).truncate())
}

fn tile(t: &Tile) -> El {
    let mut body = el().grow(1.0).h_full().min_w(0.0).radius(12.0).clip_rounded(12.0).fill(rgb(0x23, 0x23, 0x26));
    body = match &t.video {
        Some(px) => body.pixels_fit(px, rune::SurfaceFit::Fit),
        None => {
            let initial = t.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            body.child(row().w_full().h_full().center().child(row().size(64.0, 64.0).center().radius(32.0).fill(rgb(0x4d, 0x4d, 0x55)).child(text(initial).font(font(24.0, 600.0).lh(28.0)).color(Color::WHITE))))
        }
    };
    body = body.child(el().pin(None, None, Some(6.0), Some(6.0)).child(pill(&t.name)));
    if t.speaking {
        body = body.child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).radius(12.0).border(3.5, rgb(0x23, 0xa5, 0x5a)));
    }
    body
}

/// Opens the pop-out on a participant's stream (`screen`) or camera, or the whole call.
pub fn open(s: &mut State, user: Option<Snowflake>, screen: bool, pip: bool) {
    let v = &s.app.shell.voice;
    let me = s.app.store.my_id();
    let guild = v.channel.and_then(|(g, _)| g);
    let (title, video) = match user {
        Some(u) => {
            let name = s.app.store.display_name(guild, u);
            let video = if screen { Some(v.stream.clone()) } else if u == me { Some(v.self_view.clone()) } else { v.videos.get(&u).cloned() };
            let title = if screen { crate::i18n::tf("{streamerDisplayName}'s screen", &[("streamerDisplayName", &name)]) } else { name };
            (title, video)
        }
        None => (s.app.store.channels.get(&v.channel.map_or(Snowflake(0), |(_, c)| c)).and_then(|c| c.name.clone()).unwrap_or_default(), None),
    };
    SHOWN.with(|x| *x.borrow_mut() = video.is_none().then(Vec::new));
    STREAM.with(|x| x.set(user.is_some() && screen));
    WINDOW.with(|w| {
        let Some((app, win)) = &*w.borrow() else { return };
        app.update(move |p, _| (p.title, p.video, p.tiles) = (title, video, Vec::new()));
        win.set_always_on_top(pip);
        win.resize(if pip { (320.0, 180.0) } else { (960.0, 540.0) });
        win.open();
    });
    sync(s);
}

/// Hides the pop-out (the call ended).
pub fn close() {
    SHOWN.with(|x| *x.borrow_mut() = None);
    STREAM.with(|x| x.set(false));
    WINDOW.with(|w| {
        if let Some((app, win)) = &*w.borrow() {
            app.update(|p, _| *p = Popout::default());
            win.hide();
        }
    });
}

/// Once a frame: refreshes a call pop-out's tiles when who's there, speaking or on camera changed.
pub fn sync(s: &State) {
    let Some(shown) = SHOWN.with(|x| x.borrow().clone()) else { return };
    let v = &s.app.shell.voice;
    let Some((guild, channel)) = v.channel else { return close() };
    let me = s.app.store.my_id();
    let mut people: Vec<Snowflake> = s.app.store.voice_states.iter().filter(|((g, _), st)| *g == guild && st.channel_id == Some(channel)).map(|((_, u), _)| *u).collect();
    people.sort();
    let tiles: Vec<Tile> = people
        .into_iter()
        .map(|u| Tile {
            name: s.app.store.display_name(guild, u),
            speaking: v.speaking.contains(&u),
            video: if u == me { v.camera.then(|| v.self_view.clone()) } else { v.videos.get(&u).cloned() },
        })
        .collect();
    let key: Vec<(String, bool, bool)> = tiles.iter().map(|t| (t.name.clone(), t.speaking, t.video.is_some())).collect();
    if key == shown {
        return;
    }
    SHOWN.with(|x| *x.borrow_mut() = Some(key));
    WINDOW.with(|w| {
        if let Some((app, _)) = &*w.borrow() {
            app.update(move |p, _| p.tiles = tiles);
        }
    });
}
