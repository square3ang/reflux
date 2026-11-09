//! System-wide shortcuts (`GlobalShortcutsEngine.ts`): mute, deafen and push-to-talk while the
//! window isn't focused. Backends as Fluxer picks them: Wayland binds through the
//! xdg-desktop-portal GlobalShortcuts portal (the user picks the keys in the desktop's settings,
//! `LinuxGlobalShortcutsPortal.ts`); X11, Windows and macOS watch the keyboard with a
//! listen-only hook (`desktop::hooks`: XRecord, `WH_KEYBOARD_LL`, a CGEventTap) and match the
//! user's own combos ([`Binding`]s: every active Toggle mute / Toggle deafen combo, built-in or
//! custom, as Fluxer makes `allowGlobal` keybinds global, and the push-to-talk key).
//!
//! For voice: [`ptt_held`] is the push-to-talk key's state, and every press and release also
//! wakes the UI thread, so a frame's tick sees it change.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::keybinds::{Combo, K};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleMute,
    ToggleDeafen,
    PushToTalk,
}

/// `GLOBAL_SHORTCUT_ACTIONS` ids and `GLOBAL_SHORTCUT_DEFAULT_DESCRIPTIONS` (the portal's).
#[cfg(target_os = "linux")]
pub const ACTIONS: &[(Action, &str, &str)] = &[
    (Action::ToggleMute, "voice_toggle_mute", "Toggle mute"),
    (Action::ToggleDeafen, "voice_toggle_deafen", "Toggle deafen"),
    (Action::PushToTalk, "voice_push_to_talk", "Push-to-talk"),
];

/// A key as the hooks report it, independent of layout state: letters lowercased.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GKey {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Escape,
    Tab,
    Enter,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    Ctrl,
    Shift,
    Alt,
    Super,
}

impl GKey {
    pub fn char(c: char) -> GKey {
        GKey::Char(c.to_ascii_lowercase())
    }

    /// The Voice tab's recorded push-to-talk key: `format!("{:?}", rune::Key)`.
    pub fn from_ptt(s: &str) -> Option<GKey> {
        let inner = |p: &str| s.strip_prefix(p).and_then(|r| r.strip_suffix(')'));
        if let Some(t) = inner("Text(\"").and_then(|r| r.strip_suffix('"')) {
            let mut cs = t.chars();
            return match (cs.next(), cs.next()) {
                (Some(c), None) => Some(GKey::char(c)),
                _ => None,
            };
        }
        if let Some(c) = inner("Command('").and_then(|r| r.strip_suffix('\'')) {
            return c.chars().next().map(GKey::char);
        }
        if let Some(n) = inner("Function(") {
            return n.parse().ok().map(GKey::F);
        }
        if let Some(m) = s.strip_prefix("Modifier { modifier: ") {
            return [("Control", GKey::Ctrl), ("Shift", GKey::Shift), ("Alt", GKey::Alt), ("Super", GKey::Super)].into_iter().find(|(p, _)| m.starts_with(p)).map(|(_, k)| k);
        }
        Some(match s {
            "Up" => GKey::Up,
            "Down" => GKey::Down,
            "Left" => GKey::Left,
            "Right" => GKey::Right,
            "Escape" => GKey::Escape,
            "Tab" => GKey::Tab,
            "Enter" => GKey::Enter,
            "Backspace" => GKey::Backspace,
            "Delete" => GKey::Delete,
            "Insert" => GKey::Insert,
            "Home" => GKey::Home,
            "End" => GKey::End,
            "PageUp" => GKey::PageUp,
            "PageDown" => GKey::PageDown,
            _ => return None,
        })
    }
}

/// Modifiers held with a key. `ctrl` is Cmd on macOS, as the keybinds' `ctrlOrMeta`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// One combo a hook backend watches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub action: Action,
    pub key: GKey,
    /// `None`: any modifiers (push-to-talk keeps working while Shift is held).
    pub mods: Option<Mods>,
}

impl Binding {
    fn of(action: Action, c: &Combo) -> Binding {
        let key = match c.key {
            K::Up => GKey::Up,
            K::Down => GKey::Down,
            K::Left => GKey::Left,
            K::Right => GKey::Right,
            K::Home => GKey::Home,
            K::End => GKey::End,
            K::PageUp => GKey::PageUp,
            K::PageDown => GKey::PageDown,
            K::Escape => GKey::Escape,
            K::Enter => GKey::Enter,
            K::Tab => GKey::Tab,
            K::Backspace => GKey::Backspace,
            K::Delete => GKey::Delete,
            K::Insert => GKey::Insert,
            K::Function(n) => GKey::F(n),
            K::Space => GKey::Char(' '),
            K::Char(ch) => GKey::char(ch),
        };
        Binding { action, key, mods: Some(Mods { ctrl: c.ctrl, alt: c.alt, shift: c.shift }) }
    }
}

/// `comboToXdgTrigger`: a keybinding as the portal's `preferred_trigger` (`CTRL+SHIFT+m`).
/// Key names are XKB keysyms, as the shortcuts XDG spec (and Fluxer) writes them.
pub fn xdg_trigger(key: GKey, mods: Mods) -> Option<String> {
    let key = match key {
        GKey::Char(c) if c.is_ascii_alphanumeric() => c.to_ascii_lowercase().to_string(),
        GKey::Char(' ') => "space".to_string(),
        GKey::Up => "Up".into(),
        GKey::Down => "Down".into(),
        GKey::Left => "Left".into(),
        GKey::Right => "Right".into(),
        GKey::Escape => "Escape".into(),
        GKey::Tab => "Tab".into(),
        GKey::Enter => "Return".into(),
        GKey::Backspace => "BackSpace".into(),
        GKey::Delete => "Delete".into(),
        GKey::Insert => "Insert".into(),
        GKey::Home => "Home".into(),
        GKey::End => "End".into(),
        GKey::PageUp => "Prior".into(),
        GKey::PageDown => "Next".into(),
        GKey::F(n) if (1..=24).contains(&n) => format!("F{n}"),
        _ => return None,
    };
    let mut parts = Vec::new();
    if mods.ctrl {
        parts.push("CTRL");
    }
    if mods.alt {
        parts.push("ALT");
    }
    if mods.shift {
        parts.push("SHIFT");
    }
    parts.push(&key);
    Some(parts.join("+"))
}

/// What the portal should pre-fill for each action (`preferredTrigger`), from the user's own
/// keybinds. Push-to-talk takes its key without modifiers, as it does in the app.
pub fn preferred_triggers(sh: &crate::shell::ShellState) -> Vec<(Action, Option<String>)> {
    bindings(sh).into_iter().map(|b| (b.action, xdg_trigger(b.key, b.mods.unwrap_or_default()))).collect()
}

static ENABLED: AtomicBool = AtomicBool::new(true);
static STARTED: AtomicBool = AtomicBool::new(false);
static PTT: AtomicBool = AtomicBool::new(false);
/// A listen-only hook is the backend: the focused window sees the same keys itself.
static HOOK: AtomicBool = AtomicBool::new(false);
/// The bindings, and which of them are held down.
static STATE: Mutex<(Vec<Binding>, Vec<Binding>)> = Mutex::new((Vec::new(), Vec::new()));

/// The push-to-talk shortcut is held down.
pub fn ptt_held() -> bool {
    PTT.load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if !on {
        PTT.store(false, Ordering::Relaxed);
    }
}

/// A shortcut went down (`pressed`) or up; runs on any thread.
pub fn fire(action: Action, pressed: bool) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    // With a hook the window's own keybinds already ran a toggle while it's focused
    // (Fluxer's local/global press dedupe).
    let hook = HOOK.load(Ordering::Relaxed);
    match action {
        Action::PushToTalk => {
            PTT.store(pressed, Ordering::Relaxed);
            crate::app::update(|_| {});
        }
        // A frame must run to send the voice state: unfocused, nothing else asks for one.
        Action::ToggleMute if pressed => crate::app::update_tx(move |s, tx| {
            if !(hook && s.desktop.focused) {
                s.app.shell.toggle_mute();
                tx.wake_after(std::time::Duration::ZERO);
            }
        }),
        Action::ToggleDeafen if pressed => crate::app::update_tx(move |s, tx| {
            if !(hook && s.desktop.focused) {
                s.app.shell.toggle_deaf();
                tx.wake_after(std::time::Duration::ZERO);
            }
        }),
        _ => {}
    }
}

/// A key event from a hook backend (any thread): presses a matching binding once, releases
/// what that key held.
pub fn key(key: GKey, mods: Mods, down: bool) {
    let mut fired = Vec::new();
    {
        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let (binds, held) = &mut *st;
        if down {
            for b in binds.iter().filter(|b| b.key == key && b.mods.is_none_or(|m| m == mods)) {
                if !held.contains(b) {
                    held.push(*b);
                    fired.push((b.action, true));
                }
            }
        } else {
            held.retain(|b| b.key != key || {
                fired.push((b.action, false));
                false
            });
        }
    }
    for (a, pressed) in fired {
        fire(a, pressed);
    }
}

/// The combos to watch: every active Toggle mute / Toggle deafen combo of this account, and
/// the push-to-talk key when push-to-talk is on.
pub fn bindings(sh: &crate::shell::ShellState) -> Vec<Binding> {
    use crate::keybinds::Action as A;
    let fallback = Default::default();
    let m = sh.prefs.accounts.get(&sh.account.me.to_string()).unwrap_or(&fallback);
    let mut v: Vec<Binding> = [(A::ToggleMute, Action::ToggleMute), (A::ToggleDeafen, Action::ToggleDeafen)]
        .into_iter()
        .flat_map(|(a, g)| crate::nav::keys::active(m, a).into_iter().map(move |c| Binding::of(g, &c)))
        .collect();
    let s = &sh.voice.settings;
    if s.ptt()
        && let Some(key) = s.ptt_key.as_deref().and_then(GKey::from_ptt)
    {
        v.push(Binding { action: Action::PushToTalk, key, mods: None });
    }
    v
}

/// Every frame: the hooks follow changed bindings; a removed binding that was held is released.
pub fn sync(binds: Vec<Binding>) {
    let mut released = Vec::new();
    {
        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        if st.0 == binds {
            return;
        }
        st.1.retain(|b| binds.contains(b) || {
            released.push(b.action);
            false
        });
        #[cfg(target_os = "linux")]
        if STARTED.load(Ordering::Relaxed) && wayland() {
            portal::follow(&binds);
        }
        st.0 = binds;
    }
    for a in released {
        fire(a, false);
    }
}

/// Whether [`start`] ran.
pub fn started() -> bool {
    STARTED.load(Ordering::Relaxed)
}

/// Starts listening once, in a windowed run. `triggers` are the portal's preferred ones.
pub fn start(enabled: bool, triggers: Vec<(Action, Option<String>)>) {
    set_enabled(enabled);
    if rune::is_headless() || STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    #[cfg(target_os = "linux")]
    if wayland() {
        return portal::start(triggers);
    }
    let _ = triggers;
    HOOK.store(super::hooks::start(), Ordering::Relaxed);
}

/// Whether the desktop asks the user to assign our global shortcuts itself (Wayland's portal).
/// The shortcuts only fire once they have keys: the portal keeps what the user last assigned,
/// and a first prompt that was dismissed leaves them unassigned until [`configure`] runs.
pub fn portal() -> bool {
    #[cfg(target_os = "linux")]
    if wayland() {
        return true;
    }
    false
}

/// Opens the desktop's own shortcut dialog for our actions (`ConfigureShortcuts`), where the
/// user assigns their keys. Does nothing where there is no such portal.
pub fn configure() {
    #[cfg(target_os = "linux")]
    if wayland() {
        portal::configure();
    }
}

/// `LinuxSessionType`: Wayland sessions use the portal; X11 ones a hook.
#[cfg(target_os = "linux")]
fn wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").map(|t| t == "wayland").unwrap_or_else(|_| std::env::var_os("WAYLAND_DISPLAY").is_some())
}

#[cfg(target_os = "linux")]
mod portal {
    use std::sync::Mutex;

    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    use super::{ACTIONS, Action, Binding, GKey, fire};

    /// What [`follow`] last saw: each action's keys as Qt key codes.
    static FOLLOWED: Mutex<Option<Vec<(Action, Vec<i32>)>>> = Mutex::new(None);

    /// A binding as kglobalaccel stores keys: a Qt key code with its modifier bits.
    pub(super) fn qt_key(b: &Binding) -> Option<i32> {
        let key = match b.key {
            GKey::Char(c) if c.is_ascii_alphanumeric() => c.to_ascii_uppercase() as i32,
            GKey::Char(' ') => 0x20,
            GKey::Escape => 0x0100_0000,
            GKey::Tab => 0x0100_0001,
            GKey::Backspace => 0x0100_0003,
            GKey::Enter => 0x0100_0004,
            GKey::Insert => 0x0100_0006,
            GKey::Delete => 0x0100_0007,
            GKey::Home => 0x0100_0010,
            GKey::End => 0x0100_0011,
            GKey::Left => 0x0100_0012,
            GKey::Up => 0x0100_0013,
            GKey::Right => 0x0100_0014,
            GKey::Down => 0x0100_0015,
            GKey::PageUp => 0x0100_0016,
            GKey::PageDown => 0x0100_0017,
            GKey::F(n) if (1..=24).contains(&n) => 0x0100_0030 + i32::from(n) - 1,
            _ => return None,
        };
        let m = b.mods.unwrap_or_default();
        Some(key | if m.shift { 0x0200_0000 } else { 0 } | if m.ctrl { 0x0400_0000 } else { 0 } | if m.alt { 0x0800_0000 } else { 0 })
    }

    /// An in-app keybind change reaches the desktop's assignment. The portal only takes
    /// preferred keys for shortcuts it hasn't seen, so on KDE the changed actions' keys are set in
    /// kglobalaccel directly. The first bindings seen are the baseline: a launch doesn't
    /// overwrite keys the user assigned in the system settings.
    // shortcut: KDE only (Fluxer leaves the keys to the desktop); other desktops keep theirs
    // until the user changes them there.
    pub fn follow(binds: &[Binding]) {
        if !std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|d| d == "KDE")) {
            return;
        }
        let now: Vec<(Action, Vec<i32>)> = ACTIONS.iter().map(|(a, _, _)| (*a, binds.iter().filter(|b| b.action == *a).filter_map(qt_key).collect())).collect();
        let Some(before) = FOLLOWED.lock().unwrap_or_else(|e| e.into_inner()).replace(now.clone()) else { return };
        let changed: Vec<_> = now.into_iter().filter(|x| !before.contains(x)).collect();
        if changed.is_empty() {
            return;
        }
        std::thread::spawn(move || {
            let r = (|| -> zbus::Result<()> {
                let conn = zbus::blocking::Connection::session()?;
                for (action, keys) in changed {
                    let Some((_, id, description)) = ACTIONS.iter().find(|(a, _, _)| *a == action) else { continue };
                    let action_id = [APP_ID, id, "Fluxer (Reflux)", description];
                    let keys: Vec<(Vec<i32>,)> = keys.into_iter().map(|k| (vec![k, 0, 0, 0],)).collect();
                    conn.call_method(Some("org.kde.kglobalaccel"), "/kglobalaccel", Some("org.kde.KGlobalAccel"), "setForeignShortcutKeys", &(action_id.as_slice(), keys))?;
                }
                Ok(())
            })();
            if let Err(e) = r {
                eprintln!("reflux: global shortcuts: updating KDE's keys: {e}");
            }
        });
    }

    /// The portal task asks for its shortcut dialog.
    static CONFIGURE: Mutex<Option<tokio::sync::mpsc::UnboundedSender<()>>> = Mutex::new(None);

    pub fn start(triggers: Vec<(Action, Option<String>)>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        *CONFIGURE.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        crate::app::spawn(async move {
            if let Err(e) = portal(triggers, rx).await {
                eprintln!("reflux: global shortcuts portal unavailable: {e}");
            }
        });
    }

    /// `LinuxPortalShortcutsManager.configure`: the portal's own shortcut dialog, v2.
    pub fn configure() {
        let tx = CONFIGURE.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(tx) = tx {
            let _ = tx.send(());
        }
    }

    /// Our app id with the portal. Without one the portal files the shortcuts under whatever
    /// launched us (KDE kept them under `org.kde.konsole` from a terminal, unassigned), so
    /// keys assigned in one launch were gone in the next. Registering needs a desktop entry.
    const APP_ID: &str = "app.fluxer.reflux";

    /// `~/.local/share/applications/app.fluxer.reflux.desktop`, hidden from launchers, so the
    /// portal knows the id. Release builds only, as the protocol registration on Windows.
    fn install_entry() {
        if cfg!(debug_assertions) {
            return;
        }
        let (Some(dir), Ok(exe)) = (dirs::data_dir().map(|d| d.join("applications")), std::env::current_exe()) else { return };
        let path = dir.join(format!("{APP_ID}.desktop"));
        let body = format!("[Desktop Entry]\nType=Application\nName=Fluxer (Reflux)\nExec=\"{}\" %u\nIcon=reflux\nTerminal=false\nNoDisplay=true\n", exe.display());
        if std::fs::read_to_string(&path).ok().as_deref() != Some(body.as_str()) {
            let _ = std::fs::create_dir_all(&dir);
            if let Err(e) = std::fs::write(&path, body) {
                eprintln!("reflux: {}: {e}", path.display());
            }
        }
    }

    async fn portal(triggers: Vec<(Action, Option<String>)>, mut configure: tokio::sync::mpsc::UnboundedReceiver<()>) -> ashpd::Result<()> {
        install_entry();
        // Before any other portal call on this connection, as the registry requires.
        if let Err(e) = ashpd::register_host_app(APP_ID.try_into().expect("app id")).await {
            eprintln!("reflux: global shortcuts: registering {APP_ID}: {e}");
        }
        let gs = GlobalShortcuts::new().await?;
        let session = gs.create_session(Default::default()).await?;
        // `preferred_trigger`s let the backend pre-fill the keys; KDE keeps what the user
        // assigns in the dialog and never overrides it from here.
        let shortcuts: Vec<NewShortcut> = ACTIONS
            .iter()
            .map(|&(action, id, desc)| {
                let t = triggers.iter().find(|(a, _)| *a == action).and_then(|(_, t)| t.clone());
                NewShortcut::new(id, desc).preferred_trigger(t.as_deref())
            })
            .collect();
        gs.bind_shortcuts(&session, &shortcuts, None, Default::default()).await?.response()?;
        let down = gs.receive_activated().await?.map(|a| (a.shortcut_id().to_string(), true));
        let up = gs.receive_deactivated().await?.map(|a| (a.shortcut_id().to_string(), false));
        let mut events = futures_util::stream::select(down, up).fuse();
        loop {
            tokio::select! {
                e = events.next() => {
                    let Some((id, pressed)) = e else { break };
                    if let Some(&(action, ..)) = ACTIONS.iter().find(|(_, i, _)| *i == id) {
                        fire(action, pressed);
                    }
                }
                Some(()) = configure.recv() => {
                    if let Err(e) = gs.configure_shortcuts(&session, None, Default::default()).await {
                        eprintln!("reflux: configuring global shortcuts: {e}");
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn qt_keys_match_kglobalaccel() {
        // As kglobalaccel reported them: Ctrl+Shift+M and Ctrl+M.
        let b = |key, ctrl, shift| Binding { action: Action::ToggleMute, key, mods: Some(Mods { ctrl, alt: false, shift }) };
        assert_eq!(super::portal::qt_key(&b(GKey::char('m'), true, true)), Some(100_663_373));
        assert_eq!(super::portal::qt_key(&b(GKey::char('m'), true, false)), Some(67_108_941));
        assert_eq!(super::portal::qt_key(&b(GKey::F(1), false, false)), Some(0x0100_0030));
    }

    #[test]
    fn ptt_follows_press_and_release_and_disable() {
        fire(Action::PushToTalk, true);
        assert!(ptt_held());
        set_enabled(false);
        assert!(!ptt_held());
        fire(Action::PushToTalk, true);
        assert!(!ptt_held(), "ignored while off");
        set_enabled(true);
        fire(Action::PushToTalk, false);
    }

    #[test]
    fn recorded_ptt_keys_parse() {
        assert_eq!(GKey::from_ptt("Text(\"V\")"), Some(GKey::Char('v')));
        assert_eq!(GKey::from_ptt("Function(5)"), Some(GKey::F(5)));
        assert_eq!(GKey::from_ptt("Modifier { modifier: ControlLeft, pressed: true }"), Some(GKey::Ctrl));
        assert_eq!(GKey::from_ptt("PageDown"), Some(GKey::PageDown));
        assert_eq!(GKey::from_ptt("Text(\"ab\")"), None);
    }

    #[test]
    fn matcher_presses_once_and_releases_on_key_up() {
        // Not PushToTalk: the other test owns the PTT flag.
        let ptt = Binding { action: Action::ToggleDeafen, key: GKey::F(13), mods: None };
        let mute = Binding { action: Action::ToggleMute, key: GKey::Char('m'), mods: Some(Mods { ctrl: true, shift: true, alt: false }) };
        sync(vec![ptt, mute]);
        let held = || STATE.lock().unwrap().1.clone();
        key(GKey::F(13), Mods { shift: true, ..Default::default() }, true);
        key(GKey::F(13), Mods::default(), true); // auto-repeat
        assert_eq!(held(), vec![ptt]);
        key(GKey::Char('m'), Mods { ctrl: true, ..Default::default() }, true);
        assert_eq!(held(), vec![ptt], "mute needs exactly Ctrl+Shift");
        key(GKey::Char('m'), Mods { ctrl: true, shift: true, alt: false }, true);
        assert_eq!(held(), vec![ptt, mute]);
        key(GKey::F(13), Mods::default(), false);
        assert_eq!(held(), vec![mute]);
        sync(vec![ptt]);
        assert!(held().is_empty(), "a dropped binding is released");
        sync(Vec::new());
    }
}
