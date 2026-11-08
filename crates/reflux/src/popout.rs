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
    /// Its slot in [`SLOTS`] (so closing it frees the slot).
    slot: usize,
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
    /// A call pop-out's last tiles: (name, speaking, has video), and its control bar.
    shown: Vec<(String, bool, bool)>,
    shown_bar: Bar,
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
        SLOTS.with(|s| s.borrow_mut().push(Slot { app: handle, win, key: None, shown: Vec::new(), shown_bar: Bar::default(), opened: 0 }));
    }
}

fn free(i: usize) {
    SLOTS.with(|s| {
        if let Some(slot) = s.borrow_mut().get_mut(i) {
            (slot.key, slot.shown, slot.shown_bar) = (None, Vec::new(), Bar::default());
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
    let p = palette();
    if let Some(px) = &s.video {
        return col().w_full().h_full().fill(rgb(0x0d, 0x0d, 0x0f)).child(el().w_full().h_full().pixels_fit(px, rune::SurfaceFit::Fit)).child(el().pin(Some(8.0), None, None, Some(8.0)).child(pill(&s.title)));
    }
    call_view(&s.tiles, s.bar, &p)
}

/// `VoicePopoutHost`: the call pop-out is the same call view — tiles, then the control bar
/// floating at the bottom.
pub(crate) fn call_view(tiles: &[Tile], bar: Bar, p: &Palette) -> El {
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
            .on_press(move |_: &mut Popout| crate::app::update(move |s| act(s)))
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

/// `isCallPopoutOpenForChannel`.
pub fn call_open(channel: Snowflake) -> bool {
    let k = key(None, false, channel);
    SLOTS.with(|s| s.borrow().iter().any(|x| x.key.as_deref() == Some(k.as_str())))
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
    let bar = Bar { mute: v.self_mute, deaf: v.self_deaf, camera: v.camera, screen: v.screen };
    let key: Vec<(String, bool, bool)> = tiles.iter().map(|t| (t.name.clone(), t.speaking, t.video.is_some())).collect();
    let same = key == shown && SLOTS.with(|x| x.borrow().iter().find(|s| s.key.as_deref() == Some(call.as_str())).is_some_and(|s| s.shown_bar == bar));
    if same {
        return;
    }
    SLOTS.with(|x| {
        if let Some(slot) = x.borrow_mut().iter_mut().find(|s| s.key.as_deref() == Some(call.as_str())) {
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
