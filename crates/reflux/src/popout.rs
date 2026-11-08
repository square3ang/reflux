//! Pop-out windows (`PopoutWindow`, `VoicePopoutHost`, `PiPOverlay`): secondary Rune windows,
//! keyed like `PopoutWindowManager` (one call pop-out, plus one per participant's camera or
//! stream), each showing the call's tiles, one camera or a watched stream; picture in picture
//! is a pop-out small and on top. Each has its own state, fed from the main window: video goes
//! through the shared `Pixels`, the tile list through [`sync`].
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
    /// Its slot in [`SLOTS`] (so closing it frees the slot).
    slot: usize,
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

/// Windows Rune can add only before the app runs, so the pop-outs are a fixed pool.
// shortcut: at most this many pop-outs at once (Fluxer has no limit); the oldest gives way.
const POOL: usize = 4;

/// One pooled window and what it shows.
struct Slot {
    app: AppHandle<Popout>,
    win: WindowHandle,
    /// `getVoiceCallPopoutKey` / `getVoiceTilePopoutKey`: `call:{channel}` or
    /// `tile:{user}:{camera|screen}`; `None` = free.
    key: Option<String>,
    /// A call pop-out's last tiles: (name, speaking, has video).
    shown: Vec<(String, bool, bool)>,
    /// When it opened, so the oldest gives way when all are taken.
    opened: u64,
}

thread_local! {
    static SLOTS: RefCell<Vec<Slot>> = const { RefCell::new(Vec::new()) };
    static OPENED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The watched stream ended or was closed: stream pop-outs go with it.
pub fn stream_ended() {
    close_where(|k| k.ends_with(":screen"));
}

/// Adds the (hidden) pop-out windows to the app.
pub fn register(app: &mut rune::App<State>) {
    for i in 0..POOL {
        let spec = AppSpec { fonts: crate::theme::FONTS, families: crate::theme::FAMILIES, min_size: Some((160.0, 90.0)), ..AppSpec::new("Fluxer", (960.0, 540.0), Popout::default, view) };
        let options = WindowOptions { visible: false, min_size: Some((160.0, 90.0)), ..Default::default() };
        // Closing it from the title bar frees its slot.
        // `windowless`: no window until opened (Wayland can't hide one, so a hidden pooled window
        // was a blank taskbar entry that also broke the launcher's click-to-minimize).
        let papp = rune::App::new(spec).windowless().on_close(|p: &mut Popout, _| free(p.slot));
        let (handle, win) = app.window(WindowSpec::from_app(format!("voice-popout-{i}"), papp).options(options));
        SLOTS.with(|s| s.borrow_mut().push(Slot { app: handle, win, key: None, shown: Vec::new(), opened: 0 }));
    }
}

fn free(i: usize) {
    SLOTS.with(|s| {
        if let Some(slot) = s.borrow_mut().get_mut(i) {
            (slot.key, slot.shown) = (None, Vec::new());
        }
    });
}

/// Hides the pop-outs whose key matches.
fn close_where(f: impl Fn(&str) -> bool) {
    SLOTS.with(|s| {
        for slot in s.borrow_mut().iter_mut().filter(|x| x.key.as_deref().is_some_and(&f)) {
            (slot.key, slot.shown) = (None, Vec::new());
            slot.app.update(|p, _| (p.title, p.video, p.tiles) = (String::new(), None, Vec::new()));
            slot.win.hide();
        }
    });
}

/// The pop-out key (`PopoutWindowManager`): a participant's camera or stream, or the call.
fn key(user: Option<Snowflake>, screen: bool, channel: Snowflake) -> String {
    match user {
        Some(u) => format!("tile:{u}:{}", if screen { "screen" } else { "camera" }),
        None => format!("call:{channel}"),
    }
}

/// The slot for `key`: its own if open, else a free one, else the oldest.
fn pick(slots: &[Slot], key: &str) -> Option<usize> {
    slots.iter().position(|s| s.key.as_deref() == Some(key)).or_else(|| slots.iter().position(|s| s.key.is_none())).or_else(|| (0..slots.len()).min_by_key(|i| slots[*i].opened))
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

/// Opens (or brings forward) the pop-out on a participant's stream (`screen`) or camera, or the
/// whole call; a call pop-out for another channel closes (`openCallPopout`).
pub fn open(s: &mut State, user: Option<Snowflake>, screen: bool, pip: bool) {
    // `disablePictureInPicturePopoutScreenShare`: streams stay in the call, no PiP window.
    if screen && pip && crate::look::with(|l| l.disable_screen_pip) {
        return;
    }
    let v = &s.app.shell.voice;
    let k = key(user, screen, v.channel.map_or(Snowflake(0), |(_, c)| c));
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
    if user.is_none() {
        close_where(|x| x.starts_with("call:") && x != k);
    }
    let n = OPENED.with(|o| {
        o.set(o.get() + 1);
        o.get()
    });
    SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let Some(i) = pick(&slots, &k) else { return };
        let slot = &mut slots[i];
        let again = slot.key.as_deref() == Some(k.as_str());
        (slot.key, slot.shown, slot.opened) = (Some(k), Vec::new(), n);
        slot.app.update(move |p, tx| {
            tx.set_title(title.clone());
            (p.slot, p.title, p.video, p.tiles) = (i, title, video, Vec::new());
        });
        slot.win.set_always_on_top(pip);
        if !again || pip {
            slot.win.resize(if pip { (320.0, 180.0) } else { (960.0, 540.0) });
        }
        slot.win.open();
        slot.win.focus();
    });
    sync(s);
}

/// Hides every pop-out (the call ended).
pub fn close() {
    close_where(|_| true);
}

/// Once a frame: refreshes a call pop-out's tiles when who's there, speaking or on camera changed.
pub fn sync(s: &State) {
    let open = SLOTS.with(|x| x.borrow().iter().any(|s| s.key.is_some()));
    if !open {
        return;
    }
    let v = &s.app.shell.voice;
    let Some((guild, channel)) = v.channel else { return close() };
    let call = format!("call:{channel}");
    let Some(shown) = SLOTS.with(|x| x.borrow().iter().find(|s| s.key.as_deref() == Some(call.as_str())).map(|s| s.shown.clone())) else { return };
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
    SLOTS.with(|x| {
        if let Some(slot) = x.borrow_mut().iter_mut().find(|s| s.key.as_deref() == Some(call.as_str())) {
            slot.shown = key;
            slot.app.update(move |p, _| p.tiles = tiles);
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn keys() {
        use reflux_model::Snowflake;
        assert_eq!(super::key(Some(Snowflake(5)), true, Snowflake(9)), "tile:5:screen");
        assert_eq!(super::key(None, false, Snowflake(9)), "call:9");
    }
}
