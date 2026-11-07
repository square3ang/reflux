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
        Action::ToggleMute if pressed => crate::app::update(move |s| {
            if !(hook && s.desktop.focused) {
                s.app.shell.toggle_mute()
            }
        }),
        Action::ToggleDeafen if pressed => crate::app::update(move |s| {
            if !(hook && s.desktop.focused) {
                s.app.shell.toggle_deaf()
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
        st.0 = binds;
    }
    for a in released {
        fire(a, false);
    }
}

/// Starts listening once, in a windowed run.
pub fn start(enabled: bool) {
    set_enabled(enabled);
    if rune::is_headless() || STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    #[cfg(target_os = "linux")]
    if wayland() {
        return portal::start();
    }
    HOOK.store(super::hooks::start(), Ordering::Relaxed);
}

/// `LinuxSessionType`: Wayland sessions use the portal; X11 ones a hook.
#[cfg(target_os = "linux")]
fn wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").map(|t| t == "wayland").unwrap_or_else(|_| std::env::var_os("WAYLAND_DISPLAY").is_some())
}

#[cfg(target_os = "linux")]
mod portal {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    use super::{ACTIONS, fire};

    pub fn start() {
        crate::app::spawn(async {
            if let Err(e) = portal().await {
                eprintln!("reflux: global shortcuts portal unavailable: {e}");
            }
        });
    }

    async fn portal() -> ashpd::Result<()> {
        let gs = GlobalShortcuts::new().await?;
        let session = gs.create_session(Default::default()).await?;
        let shortcuts: Vec<NewShortcut> = ACTIONS.iter().map(|&(_, id, desc)| NewShortcut::new(id, desc)).collect();
        gs.bind_shortcuts(&session, &shortcuts, None, Default::default()).await?.response()?;
        let down = gs.receive_activated().await?.map(|a| (a.shortcut_id().to_string(), true));
        let up = gs.receive_deactivated().await?.map(|a| (a.shortcut_id().to_string(), false));
        let mut events = futures_util::stream::select(down, up);
        while let Some((id, pressed)) = events.next().await {
            if let Some(&(action, ..)) = ACTIONS.iter().find(|(_, i, _)| *i == id) {
                fire(action, pressed);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
