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
use crate::theme::{Palette, font};
use crate::ui::{rgb, rgba};

/// `VoiceControlBar`'s state as the pop-out shows it (the main window owns the actions).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bar {
    pub mute: bool,
    pub deaf: bool,
    pub camera: bool,
    pub screen: bool,
}

#[derive(Default)]
pub struct Popout {
    /// Its key in [`SLOTS`] (so closing it removes the slot).
    key: String,
    title: String,
    /// One video (a camera or stream); `None` = the call grid.
    video: Option<Pixels>,
    tiles: Vec<Tile>,
    bar: Bar,
}

#[derive(Clone)]
struct Tile {
    name: String,
    speaking: bool,
    video: Option<Pixels>,
}

/// One open pop-out window and what it shows.
struct Slot {
    app: AppHandle<Popout>,
    win: WindowHandle,
    /// `getVoiceCallPopoutKey` / `getVoiceTilePopoutKey`: `call:{channel}` or
    /// `tile:{user}:{camera|screen}`.
    key: String,
    /// A call pop-out's last tiles: (name, speaking, has video), and its control bar.
    shown: Vec<(String, bool, bool)>,
    shown_bar: Bar,
}

thread_local! {
    static SLOTS: RefCell<Vec<Slot>> = const { RefCell::new(Vec::new()) };
}

/// The watched stream ended or was closed: stream pop-outs go with it.
pub fn stream_ended() {
    close_where(|k| k.ends_with(":screen"));
}

/// Opens a pop-out window for `key` (`open_window`, as `PopoutWindowManager` opens one per key).
fn spawn(key: &str, size: (f32, f32)) -> Slot {
    let spec = AppSpec { fonts: crate::theme::FONTS, families: crate::theme::FAMILIES, min_size: Some((160.0, 90.0)), ..AppSpec::new("Fluxer", size, Popout::default, view) };
    let options = WindowOptions { min_size: Some((160.0, 90.0)), ..Default::default() };
    // Closing it from the title bar forgets it.
    let papp = rune::App::new(spec).on_close(|p: &mut Popout, _| forget(&p.key));
    let app = papp.handle();
    let win = rune::open_window(WindowSpec::from_app(format!("voice-popout-{key}"), papp).options(options));
    Slot { app, win, key: key.to_string(), shown: Vec::new(), shown_bar: Bar::default() }
}

fn forget(key: &str) {
    SLOTS.with(|s| s.borrow_mut().retain(|x| x.key != key));
}

/// Closes the pop-outs whose key matches.
fn close_where(f: impl Fn(&str) -> bool) {
    SLOTS.with(|s| {
        s.borrow_mut().retain(|slot| {
            let gone = f(&slot.key);
            if gone {
                slot.win.close();
            }
            !gone
        })
    });
}

/// The pop-out key (`PopoutWindowManager`): a participant's camera or stream, or the call.
fn key(user: Option<Snowflake>, screen: bool, channel: Snowflake) -> String {
    match user {
        Some(u) => format!("tile:{u}:{}", if screen { "screen" } else { "camera" }),
        None => format!("call:{channel}"),
    }
}


fn view(s: &Popout, _cx: &mut Cx<Popout>) -> El {
    let p = palette();
    if let Some(px) = &s.video {
        return col().w_full().h_full().fill(rgb(0x0d, 0x0d, 0x0f)).child(el().w_full().h_full().pixels_fit(px, rune::SurfaceFit::Fit)).child(el().pin(Some(8.0), None, None, Some(8.0)).child(pill(&s.title)));
    }
    call_view(&s.tiles, s.bar, &p)
}

/// `VoicePopoutHost`: the call pop-out is the same call view — tiles, then the control bar
/// floating at the bottom.
fn call_view(tiles: &[Tile], bar: Bar, p: &Palette) -> El {
    let root = col().w_full().h_full().fill(rgb(0, 0, 0));
    if tiles.is_empty() {
        return root;
    }
    let n = tiles.len();
    let cols = (n as f32).sqrt().ceil() as usize;
    let mut grid = col().w_full().grow(1.0).min_h(0.0).gap(8.0).padding(8.0).center();
    for chunk in tiles.chunks(cols) {
        grid = grid.child(row().w_full().grow(1.0).min_h(0.0).gap(8.0).justify_center().children(chunk.iter().map(tile)));
    }
    let controls = row().pin(Some(0.0), None, Some(0.0), Some(0.0)).pb(12.0).justify_center().child(control_bar(bar, p));
    root.child(grid).child(controls)
}

/// `.container`: the black tray of 44px buttons with its shadow.
fn control_bar(bar: Bar, _p: &Palette) -> El {
    let button = |on: bool, tint: Color, icon: &'static rune::Icon, label: &'static str, act: fn(&mut crate::State)| {
        let (bg, fg) = if on { (Color { a: 0.16, ..tint }, tint) } else { (rgba(255, 255, 255, 0.05), rgb(255, 255, 255)) };
        row()
            .size(44.0, 44.0)
            .center()
            .radius(16.0)
            .fill(bg)
            .hover(rgba(255, 255, 255, 0.09))
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label)
            .on_press(move |_: &mut Popout| crate::app::update(act))
            .child(rune::icon(icon, 22.0).color(fg))
    };
    row()
        .padding(6.0)
        .gap(4.0)
        .center_y()
        .radius(24.0)
        .fill(rgb(0, 0, 0))
        .border(1.0, rgba(255, 255, 255, 0.1))
        .box_shadow(rune::Shadow { x: 0.0, y: 8.0, blur: 24.0, spread: 0.0, color: rgba(0, 0, 0, 0.45), inset: false })
        .child(button(bar.mute, rgb(0xff, 0x45, 0x3a), if bar.mute { &crate::icons::MICROPHONE_SLASH_FILL } else { &crate::icons::MICROPHONE_FILL }, if bar.mute { "Unmute" } else { "Mute" }, |s| s.app.shell.toggle_mute()))
        .child(button(bar.deaf, rgb(0xff, 0x45, 0x3a), if bar.deaf { &crate::icons::SPEAKER_SLASH_FILL } else { &crate::icons::SPEAKER_HIGH_FILL }, if bar.deaf { "Undeafen" } else { "Deafen" }, |s| s.app.shell.toggle_deaf()))
        .child(button(bar.screen, rgb(0x23, 0xa5, 0x5a), &crate::icons::MONITOR_PLAY_FILL, "Share your screen", |s| s.app.shell.intents.push(crate::shell::Intent::ToggleScreen)))
        .child(button(bar.camera, rgb(0x23, 0xa5, 0x5a), if bar.camera { &crate::icons::CAMERA_FILL } else { &crate::icons::CAMERA_SLASH_FILL }, "Camera", |s| s.app.shell.intents.push(crate::shell::Intent::Voice(crate::shell::voice::Op::ToggleCamera))))
        .child(el().size(1.0, 22.0).radius(999.0).fill(rgba(255, 255, 255, 0.12)))
        .child(button(false, rgb(0xff, 0x3b, 0x30), &crate::icons::PHONE_X_FILL, "Disconnect", |s| s.app.shell.leave_voice()))
}

/// The theme's palette for the pop-out windows (they render outside the main window's state).
fn palette() -> Palette {
    let look = crate::look::with(|l| l.clone());
    crate::theme::custom::effective(look.theme("dark"), true, look.saturation, &look.theme_css())
}

thread_local! {
    /// The demo shell renders the pop-out's call view in the main window so it can be snapped.
    static DEMO: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn demo(on: bool) {
    DEMO.with(|d| d.set(on));
}

pub fn demoing() -> bool {
    DEMO.with(|d| d.get())
}

pub fn demo_call() -> El {
    let p = palette();
    let tiles: Vec<Tile> = ["Square", "Ada Lovelace", "Linus", "Grace Hopper"]
        .iter()
        .enumerate()
        .map(|(i, name)| Tile { name: (*name).into(), speaking: i == 1, video: None })
        .collect();
    let bar = Bar { mute: false, deaf: false, camera: true, screen: false };
    call_view(&tiles, bar, &p)
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
        // A DM has no name of its own: its recipients' names.
        None => (s.app.store.channels.get(&v.channel.map_or(Snowflake(0), |(_, c)| c)).map(|c| crate::shell::chat::channel_title(&s.app.store, c)).unwrap_or_default(), None),
    };
    if user.is_none() {
        close_where(|x| x.starts_with("call:") && x != k);
    }
    let size = if pip { (320.0, 180.0) } else { (960.0, 540.0) };
    SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        let i = match slots.iter().position(|s| s.key == k) {
            Some(i) => {
                if pip {
                    slots[i].win.resize(size);
                }
                i
            }
            None => {
                slots.push(spawn(&k, size));
                slots.len() - 1
            }
        };
        let slot = &mut slots[i];
        slot.shown = Vec::new();
        let key = k.clone();
        slot.app.update(move |p, tx| {
            tx.set_title(title.clone());
            (p.key, p.title, p.video, p.tiles) = (key, title, video, Vec::new());
        });
        slot.win.set_always_on_top(pip);
        slot.win.focus();
    });
    sync(s);
}

/// `isCallPopoutOpenForChannel`.
pub fn call_open(channel: Snowflake) -> bool {
    let k = key(None, false, channel);
    SLOTS.with(|s| s.borrow().iter().any(|x| x.key == k))
}

/// Closes every pop-out (the call ended).
pub fn close() {
    close_where(|_| true);
}

/// Once a frame: refreshes a call pop-out's tiles when who's there, speaking or on camera changed.
pub fn sync(s: &State) {
    let open = SLOTS.with(|x| !x.borrow().is_empty());
    if !open {
        return;
    }
    let v = &s.app.shell.voice;
    let Some((guild, channel)) = v.channel else { return close() };
    let call = format!("call:{channel}");
    let Some(shown) = SLOTS.with(|x| x.borrow().iter().find(|s| s.key == call).map(|s| s.shown.clone())) else { return };
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
    let bar = Bar { mute: v.self_mute, deaf: v.self_deaf, camera: v.camera, screen: v.screen };
    let key: Vec<(String, bool, bool)> = tiles.iter().map(|t| (t.name.clone(), t.speaking, t.video.is_some())).collect();
    let same = key == shown && SLOTS.with(|x| x.borrow().iter().find(|s| s.key == call).is_some_and(|s| s.shown_bar == bar));
    if same {
        return;
    }
    SLOTS.with(|x| {
        if let Some(slot) = x.borrow_mut().iter_mut().find(|s| s.key == call) {
            (slot.shown, slot.shown_bar) = (key, bar);
            slot.app.update(move |p, _| (p.tiles, p.bar) = (tiles, bar));
        }
    });
}

#[cfg(test)]
mod tests {
    /// The demo hook swaps the shell for the pop-out's call view; keep it per thread.
    #[test]
    fn the_demo_flag_round_trips() {
        super::demo(true);
        assert!(super::demoing());
        super::demo(false);
        assert!(!super::demoing());
    }

    #[test]
    fn keys() {
        use reflux_model::Snowflake;
        assert_eq!(super::key(Some(Snowflake(5)), true, Snowflake(9)), "tile:5:screen");
        assert_eq!(super::key(None, false, Snowflake(9)), "call:9");
    }
}
