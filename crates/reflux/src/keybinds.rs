//! Fluxer's default keybinds (`input/state/InputKeybind.ts`) that Reflux has features for, and
//! the window key handler that runs them, after the account's custom keybinds
//! (`crate::nav::keys`). System-wide shortcuts are `desktop::shortcuts`.

use reflux_model::{ChannelType, Snowflake};
use reflux_store::Store;
use rune::{Key, KeyEvent, Tx};
use serde::{Deserialize, Serialize};

use crate::State;
use crate::app::Route;
use crate::shell::{Intent, ShellState};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    ShowKeybinds,
    GuildPrev,
    GuildNext,
    ChannelPrev,
    ChannelNext,
    UnreadPrev,
    UnreadNext,
    MentionPrev,
    MentionNext,
    /// Ctrl+1 = DMs, Ctrl+2.. = the guild rail's communities in order.
    GuildSlot(u8),
    MarkChannelRead,
    MarkGuildRead,
    ToggleMembers,
    ToggleMute,
    ToggleDeafen,
    /// `voice_toggle_compact_call_view`: expand or collapse the call surface.
    ToggleCompactCall,
    QuickSwitcher,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum K {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Escape,
    Enter,
    Tab,
    Backspace,
    Delete,
    Insert,
    /// F1 through F24.
    Function(u8),
    /// The space bar (a character, but not one `typed` accepts).
    Space,
    /// A character: `Key::Command` with Ctrl (Cmd on macOS), else typed (`Key::Text`), which
    /// reaches the keybinds only when no text field takes it.
    Char(char),
}

/// `ctrl` is Fluxer's `ctrlOrMeta` (Cmd on macOS).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Combo {
    pub key: K,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

const fn c(key: K, ctrl: bool, alt: bool, shift: bool) -> Combo {
    Combo { key, ctrl, alt, shift }
}

pub struct Bind {
    pub action: Action,
    /// Fluxer's English label (an i18n key); rows with the same label show as one.
    pub label: &'static str,
    /// `KeybindSection`.
    pub section: &'static str,
    pub combo: Combo,
}

const fn b(action: Action, label: &'static str, section: &'static str, combo: Combo) -> Bind {
    Bind { action, label, section, combo }
}

use Action::*;
use K::{Backspace, Char, Delete, Down, End, Enter, Escape, Function, Home, Insert, Left, PageDown, PageUp, Right, Space, Tab, Up};

pub const BINDS: &[Bind] = &[
    b(ShowKeybinds, "Show keybind list", "defaults", c(Char('/'), true, false, false)),
    b(GuildPrev, "Switch between communities", "navigation", c(Up, true, true, false)),
    b(GuildNext, "Switch between communities", "navigation", c(Down, true, true, false)),
    b(ChannelPrev, "Switch between channels", "navigation", c(Up, false, true, false)),
    b(ChannelNext, "Switch between channels", "navigation", c(Down, false, true, false)),
    b(UnreadPrev, "Jump between unread channels", "navigation", c(Up, false, true, true)),
    b(UnreadNext, "Jump between unread channels", "navigation", c(Down, false, true, true)),
    b(MentionPrev, "Jump between unread channels with mentions", "navigation", c(Up, true, true, true)),
    b(MentionNext, "Jump between unread channels with mentions", "navigation", c(Down, true, true, true)),
    b(GuildSlot(1), "Go to direct messages", "navigation", c(Char('1'), true, false, false)),
    b(GuildSlot(2), "Go to first community", "navigation", c(Char('2'), true, false, false)),
    b(GuildSlot(3), "Go to second community", "navigation", c(Char('3'), true, false, false)),
    b(GuildSlot(4), "Go to third community", "navigation", c(Char('4'), true, false, false)),
    b(GuildSlot(5), "Go to fourth community", "navigation", c(Char('5'), true, false, false)),
    b(GuildSlot(6), "Go to fifth community", "navigation", c(Char('6'), true, false, false)),
    b(GuildSlot(7), "Go to sixth community", "navigation", c(Char('7'), true, false, false)),
    b(GuildSlot(8), "Go to seventh community", "navigation", c(Char('8'), true, false, false)),
    b(GuildSlot(9), "Go to eighth community", "navigation", c(Char('9'), true, false, false)),
    b(QuickSwitcher, "Toggle quick switcher", "navigation", c(Char('k'), true, false, false)),
    b(ToggleMembers, "Toggle the member list or voice chat", "chat", c(Char('u'), true, false, false)),
    b(MarkChannelRead, "Mark channel as read", "chat", c(Escape, false, false, false)),
    b(MarkGuildRead, "Mark community as read", "chat", c(Escape, false, false, true)),
    b(ToggleMute, "Toggle mute", "voice_and_video", c(Char('m'), true, false, true)),
    b(ToggleDeafen, "Toggle deafen", "voice_and_video", c(Char('d'), true, false, true)),
    b(ToggleCompactCall, "Expand or collapse compact call view", "voice_and_video", c(Char('v'), true, false, true)),
    // `hideFromDefaults`: "misc" isn't a listed section.
    b(ZoomIn, "Zoom in", "misc", c(Char('='), true, false, false)),
    b(ZoomOut, "Zoom out", "misc", c(Char('-'), true, false, false)),
    b(ZoomReset, "Reset zoom", "misc", c(Char('0'), true, false, false)),
];

/// (section id, Fluxer's heading) in `DEFAULT_KEYBIND_SECTIONS` order.
pub const SECTIONS: &[(&str, &str)] =
    &[("defaults", "Defaults"), ("navigation", "Navigation"), ("chat", "Chat"), ("voice_and_video", "Voice and video")];

/// A single typed character, lowercased (Shift is in the mods); not a space.
fn typed(t: &str) -> Option<char> {
    let mut cs = t.chars();
    let ch = cs.next().filter(|c| !c.is_whitespace() && cs.next().is_none())?;
    Some(ch.to_lowercase().next().unwrap_or(ch))
}

impl Combo {
    /// What a recorder makes of a key: a character (with Ctrl / Cmd, Alt, Shift or none), or an
    /// arrow / Escape with any modifiers.
    pub fn from_event(k: &KeyEvent) -> Option<Combo> {
        let m = &k.mods;
        let key = match &k.key {
            Key::Up => Up,
            Key::Down => Down,
            Key::Left => Left,
            Key::Right => Right,
            Key::Home => Home,
            Key::End => End,
            Key::PageUp => PageUp,
            Key::PageDown => PageDown,
            Key::Escape => Escape,
            Key::Enter => Enter,
            Key::Tab => Tab,
            Key::Backspace => Backspace,
            Key::Delete => Delete,
            Key::Insert => Insert,
            Key::Function(n) => Function(*n),
            Key::Command(' ') => Space,
            Key::Command(ch) => Char(ch.to_ascii_lowercase()),
            Key::Text(t) if t == " " => Space,
            Key::Text(t) => Char(typed(t)?),
            _ => return None,
        };
        Some(c(key, m.cmd || matches!(k.key, Key::Command(_)), m.alt, m.shift))
    }

    pub fn matches(&self, k: &KeyEvent) -> bool {
        let m = &k.mods;
        let key = match (&k.key, self.key) {
            (Key::Up, Up)
            | (Key::Down, Down)
            | (Key::Left, Left)
            | (Key::Right, Right)
            | (Key::Home, Home)
            | (Key::End, End)
            | (Key::PageUp, PageUp)
            | (Key::PageDown, PageDown)
            | (Key::Escape, Escape)
            | (Key::Enter, Enter)
            | (Key::Tab, Tab)
            | (Key::Backspace, Backspace)
            | (Key::Delete, Delete)
            | (Key::Insert, Insert) => true,
            (Key::Function(n), Function(want)) => *n == want,
            (Key::Text(t), Space) => t == " ",
            (Key::Command(' '), Space) => true,
            (Key::Command(ch), Char(want)) => ch.eq_ignore_ascii_case(&want),
            (Key::Text(t), Char(want)) => typed(t) == Some(want),
            _ => false,
        };
        // Ctrl++ (Shift+= on most layouts) zooms in too (`ZOOM_IN_KEYS`).
        if matches!((&k.key, self.key), (Key::Command('+'), Char('='))) {
            return m.cmd == self.ctrl && !m.alt;
        }
        key && m.cmd == self.ctrl && m.alt == self.alt && m.shift == self.shift
    }

    /// Key caps for the settings list.
    pub fn caps(&self) -> Vec<String> {
        let mac = cfg!(target_os = "macos");
        let mut v = Vec::new();
        if self.ctrl {
            v.push(if mac { "⌘" } else { "Ctrl" }.to_string());
        }
        if self.alt {
            v.push(if mac { "⌥" } else { "Alt" }.to_string());
        }
        if self.shift {
            v.push(if mac { "⇧" } else { "Shift" }.to_string());
        }
        v.push(match self.key {
            Up => "↑".into(),
            Down => "↓".into(),
            Left => "←".into(),
            Right => "→".into(),
            Home => "Home".into(),
            End => "End".into(),
            PageUp => "PgUp".into(),
            PageDown => "PgDn".into(),
            Escape => "Esc".into(),
            Enter => "Enter".into(),
            Tab => "Tab".into(),
            Backspace => "Backspace".into(),
            Delete => "Del".into(),
            Insert => "Ins".into(),
            Function(n) => format!("F{n}"),
            Space => "Space".into(),
            Char(ch) => ch.to_ascii_uppercase().to_string(),
        });
        v
    }
}

/// `AppSpec::on_key`: keys nothing focused took (or the focused control passed on).
pub fn on_key(s: &mut State, k: &KeyEvent, tx: &mut Tx) -> bool {
    // `escapeExitsKeyboardMode`: Escape also drops the keyboard focus (and its ring); the key
    // still does what it otherwise would.
    if k.key == Key::Escape && k.mods == rune::Mods::default() && crate::look::with(|l| l.escape_exits_keyboard) {
        tx.blur();
    }
    let sh = match &mut s.shell_demo {
        Some(d) => Some(&mut d.shell),
        None if s.app.route == Route::Main => Some(&mut s.app.shell),
        None => None,
    };
    // Shift on its own: the message action bar's full layout (`MessageActionBar` showFullActions).
    // The Voice tab's push-to-talk recorder takes a modifier press as its key too.
    if let Key::Modifier { .. } = k.key {
        let Some(sh) = sh else { return false };
        if push_to_talk(sh, k) {
            return true;
        }
        sh.shift_held = k.mods.shift;
        return false;
    }
    if sh.is_some_and(|sh| crate::shell::lightbox::on_key(sh, k) || push_to_talk(sh, k) || crate::nav::keys::on_key(sh, k)) {
        return true;
    }
    let mine = crate::nav::mine(s.shell_demo.as_ref().map_or(&s.app.shell, |d| &d.shell));
    let Some(action) = crate::nav::keys::action(&mine, k) else { return false };
    match &mut s.shell_demo {
        Some(d) => run(&d.store, &mut d.shell, action),
        None if s.app.route == Route::Main || matches!(action, ZoomIn | ZoomOut | ZoomReset) => run(&s.app.store, &mut s.app.shell, action),
        None => false,
    }
}

/// The in-window push-to-talk key (its release is heard by the shell root's `on_key_up`), and
/// the Voice tab's key recorder.
fn push_to_talk(sh: &mut ShellState, k: &KeyEvent) -> bool {
    use crate::shell::voice::Op;
    // The settings modal closed (or another tab opened) under the recorder: don't capture the
    // next key silently.
    if sh.voice.recording_key && !sh.settings.as_ref().is_some_and(|u| u.tab == "voice_video") {
        sh.voice.recording_key = false;
    }
    let key = key_name(k);
    let v = &mut sh.voice;
    if v.recording_key {
        // A modifier is recorded on its press; its release and auto-repeats are ignored.
        if matches!(&k.key, Key::Modifier { pressed: false, .. }) {
            return true;
        }
        v.recording_key = false;
        if k.key != Key::Escape {
            v.settings.ptt_key = Some(key);
            sh.intents.push(Intent::Voice(Op::Settings));
        }
        return true;
    }
    if !v.settings.ptt() || v.settings.ptt_key.as_deref() != Some(key.as_str()) {
        return false;
    }
    // A modifier's release arrives here (no `on_key_up` for it): let go, don't start.
    if matches!(&k.key, Key::Modifier { pressed: false, .. }) {
        if v.ptt_held {
            v.ptt_held = false;
            sh.intents.push(Intent::Voice(Op::PushToTalk(false)));
        }
        return true;
    }
    if !v.ptt_held {
        v.ptt_held = true;
        sh.intents.push(Intent::Voice(Op::PushToTalk(true)));
    }
    true
}

/// The stored name of an in-window key (the Voice tab's PTT setting): `format!("{:?}", Key)`,
/// except a modifier loses its press state so its press and release compare equal.
pub fn key_name(k: &KeyEvent) -> String {
    match &k.key {
        Key::Modifier { modifier, .. } => format!("Modifier {{ modifier: {modifier:?}, pressed: true }}"),
        // Letters ignore Shift and Caps Lock: the stored key matches either case.
        Key::Text(t) => format!("Text({:?})", t.to_lowercase()),
        Key::Command(c) => format!("Command({:?})", c.to_ascii_lowercase()),
        _ => format!("{:?}", k.key),
    }
}

/// Runs a keybind; false when it did nothing (the key goes on).
pub fn run(store: &Store, sh: &mut ShellState, a: Action) -> bool {
    // Escape leaves the full-window call before it marks the channel read.
    if a == MarkChannelRead && sh.call.app_fullscreen {
        crate::shell::call::exit_fullscreen(sh);
        return true;
    }
    if let ZoomIn | ZoomOut | ZoomReset = a {
        let mut p = sh.prefs.clone();
        p.zoom = if a == ZoomReset { 1.0 } else { crate::desktop::window::step_zoom(p.zoom, a == ZoomIn) };
        crate::desktop::window::set_zoom(p.zoom);
        sh.intents.push(Intent::Prefs(Box::new(p)));
        return true;
    }
    // Modals keep their own keys (Esc closes them), except the keybind list itself.
    if (sh.settings.is_some() || sh.dialog.is_some() || sh.media_ui.viewer.is_some() || sh.media_ui.link_warning.is_some()) && a != ShowKeybinds {
        return false;
    }
    match a {
        ShowKeybinds => sh.nav.cheatsheet = !sh.nav.cheatsheet,
        GuildPrev | GuildNext => {
            let rail = rail(store);
            let i = rail.iter().position(|g| *g == sh.guild).unwrap_or(0);
            let g = rail[step(i, rail.len(), a == GuildNext)];
            open_guild(store, sh, g);
        }
        GuildSlot(n) => {
            let Some(&g) = rail(store).get(n as usize - 1) else { return false };
            open_guild(store, sh, g);
        }
        ChannelPrev | ChannelNext | UnreadPrev | UnreadNext | MentionPrev | MentionNext => {
            let list = channels(store, sh.guild);
            let want = |c: Snowflake| match a {
                UnreadPrev | UnreadNext => store.has_unread(c) && !store.channels.get(&c).is_some_and(|ch| store.is_muted(ch)),
                MentionPrev | MentionNext => store.mention_count(c) > 0,
                _ => true,
            };
            let next = matches!(a, ChannelNext | UnreadNext | MentionNext);
            let Some(c) = find(&list, sh.channel, next, want) else { return false };
            sh.select_channel(c);
        }
        MarkChannelRead => {
            let Some(c) = sh.channel.filter(|&c| store.has_unread(c) || store.mention_count(c) > 0) else { return false };
            sh.intents.push(Intent::MarkRead(acks(store, [c])));
        }
        MarkGuildRead => {
            let Some(g) = sh.guild else { return false };
            let ids = store.channels.values().filter(|c| c.guild_id == Some(g)).map(|c| c.id);
            sh.intents.push(Intent::MarkRead(acks(store, ids)));
        }
        ToggleMembers => sh.member_list = !sh.member_list,
        ToggleMute => sh.toggle_mute(),
        ToggleDeafen => sh.toggle_deaf(),
        ToggleCompactCall => {
            let Some((guild, _)) = sh.voice.channel else { return false };
            if guild.is_none() {
                return false;
            }
            sh.voice.call_view = !sh.voice.call_view;
        }
        QuickSwitcher => sh.toggle_switcher(),
        ZoomIn | ZoomOut | ZoomReset => unreachable!("handled above"),
    }
    true
}

/// Home, then the guild rail's guilds (folders flattened) in order.
fn rail(store: &Store) -> Vec<Option<Snowflake>> {
    let mut v = vec![None];
    for item in store.guild_list() {
        match item {
            reflux_store::GuildListItem::Guild(g) => v.push(Some(g)),
            reflux_store::GuildListItem::Folder { guilds, .. } => v.extend(guilds.into_iter().map(Some)),
        }
    }
    v
}

fn open_guild(store: &Store, sh: &mut ShellState, g: Option<Snowflake>) {
    let channel = match g {
        Some(g) => crate::shell::first_channel(store, g),
        None => store.private_channels().first().map(|c| c.id),
    };
    sh.select_guild(g, channel);
}

/// The sidebar's text channels in order (voice channels would join a call).
fn channels(store: &Store, guild: Option<Snowflake>) -> Vec<Snowflake> {
    match guild {
        Some(g) => store
            .channel_groups(g)
            .iter()
            .flat_map(|grp| grp.text.iter())
            .filter(|c| c.kind != ChannelType::GUILD_LINK)
            .map(|c| c.id)
            .collect(),
        None => store.private_channels().iter().map(|c| c.id).collect(),
    }
}

fn step(i: usize, len: usize, next: bool) -> usize {
    if next { (i + 1) % len } else { (i + len - 1) % len }
}

/// The nearest channel after (or before) `from` that `want`s, wrapping; never `from` itself.
fn find(list: &[Snowflake], from: Option<Snowflake>, next: bool, want: impl Fn(Snowflake) -> bool) -> Option<Snowflake> {
    let n = list.len();
    let start = from.and_then(|f| list.iter().position(|&c| c == f));
    let first = match start {
        Some(i) => step(i, n.max(1), next),
        None if next => 0,
        None => n.checked_sub(1)?,
    };
    (0..n).map(|k| if next { (first + k) % n } else { (first + n - k) % n }).map(|i| list[i]).find(|&c| Some(c) != from && want(c))
}

fn acks(store: &Store, ids: impl IntoIterator<Item = Snowflake>) -> Vec<(Snowflake, Snowflake)> {
    ids.into_iter().filter_map(|c| Some((c, store.channels.get(&c)?.last_message_id?))).filter(|&(c, _)| store.has_unread(c) || store.mention_count(c) > 0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_wraps_and_skips_the_current_channel() {
        let l = [Snowflake(1), Snowflake(2), Snowflake(3)];
        assert_eq!(find(&l, Some(Snowflake(3)), true, |_| true), Some(Snowflake(1)));
        assert_eq!(find(&l, Some(Snowflake(1)), false, |_| true), Some(Snowflake(3)));
        assert_eq!(find(&l, Some(Snowflake(1)), true, |c| c == Snowflake(1)), None);
        assert_eq!(find(&l, None, false, |_| true), Some(Snowflake(3)));
        assert_eq!(find(&[], None, true, |_| true), None);
    }

    #[test]
    fn compact_call_keybind_toggles_the_guild_call_view() {
        let store = Store::default();
        let mut st = ShellState::default();
        assert!(!run(&store, &mut st, ToggleCompactCall), "no call: nothing to toggle");
        st.voice.channel = Some((None, Snowflake(2)));
        assert!(!run(&store, &mut st, ToggleCompactCall), "DM calls have no full call view");
        st.voice.channel = Some((Some(Snowflake(1)), Snowflake(2)));
        assert!(run(&store, &mut st, ToggleCompactCall));
        assert!(st.voice.call_view);
        assert!(run(&store, &mut st, ToggleCompactCall));
        assert!(!st.voice.call_view);
    }

    #[test]
    fn combos_match_their_keys_only() {
        let ev = |key, cmd, alt, shift| KeyEvent { key, mods: rune::Mods { cmd, alt, shift, ..Default::default() } };
        let alt_up = BINDS.iter().find(|b| b.action == ChannelPrev).unwrap().combo;
        assert!(alt_up.matches(&ev(Key::Up, false, true, false)));
        assert!(!alt_up.matches(&ev(Key::Up, false, true, true)), "Alt+Shift+Up is the unread jump");
        let mute = BINDS.iter().find(|b| b.action == ToggleMute).unwrap().combo;
        assert!(mute.matches(&ev(Key::Command('m'), true, false, true)));
        assert_eq!(mute.caps().len(), 3);
        let zoom_in = BINDS.iter().find(|b| b.action == ZoomIn).unwrap().combo;
        assert!(zoom_in.matches(&ev(Key::Command('='), true, false, false)));
        assert!(zoom_in.matches(&ev(Key::Command('+'), true, false, true)), "Ctrl+Shift+= types +");
        let plain = Combo::from_event(&ev(Key::Text("G".into()), false, false, true)).unwrap();
        assert_eq!(plain, c(Char('g'), false, false, true), "a typed character records without Ctrl");
        assert!(plain.matches(&ev(Key::Text("G".into()), false, false, true)));
        assert!(!plain.matches(&ev(Key::Command('g'), true, false, true)));
        let f8 = Combo::from_event(&ev(Key::Function(8), false, false, false)).unwrap();
        assert_eq!(f8, c(Function(8), false, false, false));
        assert_eq!(f8.caps(), vec!["F8".to_string()]);
        assert!(f8.matches(&ev(Key::Function(8), false, false, false)));
        for k in [Key::Left, Key::Right, Key::Home, Key::End, Key::PageUp, Key::PageDown, Key::Enter, Key::Tab, Key::Backspace, Key::Delete, Key::Insert] {
            let combo = Combo::from_event(&ev(k.clone(), false, false, false)).unwrap();
            assert!(combo.matches(&ev(k, false, false, false)));
        }
        let space = Combo::from_event(&ev(Key::Text(" ".into()), false, false, false)).unwrap();
        assert_eq!(space.key, Space);
        assert!(space.matches(&ev(Key::Text(" ".into()), false, false, false)));
        assert_eq!(Combo::from_event(&ev(Key::Text("ab".into()), false, false, false)), None);
    }
}

#[cfg(test)]
mod focus_tests {
    use super::*;

    fn ev(key: Key) -> KeyEvent {
        KeyEvent { key, mods: rune::Mods::default() }
    }

    #[test]
    fn push_to_talk_records_and_accepts_a_modifier_alone() {
        let mut st = ShellState { settings: Some(crate::settings::SettingsUi::new(Some("voice_video"))), ..ShellState::default() };
        st.voice.settings.push_to_talk = true;
        st.voice.recording_key = true;
        let ctrl = || Key::Modifier { modifier: rune::Modifier::ControlLeft, pressed: true };
        assert!(push_to_talk(&mut st, &ev(ctrl())));
        assert_eq!(st.voice.settings.ptt_key.as_deref(), Some("Modifier { modifier: ControlLeft, pressed: true }"), "recorded without its press state");
        assert!(!st.voice.recording_key);
        assert!(!st.voice.ptt_held, "recording a key must not press it");
        // The next press holds it; its release lets go.
        assert!(push_to_talk(&mut st, &ev(ctrl())));
        assert!(st.voice.ptt_held);
        assert!(push_to_talk(&mut st, &ev(Key::Modifier { modifier: rune::Modifier::ControlLeft, pressed: false })));
        assert!(!st.voice.ptt_held, "the modifier release ends push-to-talk");
        // A release alone never starts it.
        assert!(push_to_talk(&mut st, &ev(Key::Modifier { modifier: rune::Modifier::ControlLeft, pressed: false })));
        assert!(!st.voice.ptt_held);
    }

    #[test]
    fn push_to_talk_ignores_a_letters_case() {
        let mut st = ShellState { settings: Some(crate::settings::SettingsUi::new(Some("voice_video"))), ..ShellState::default() };
        st.voice.settings.push_to_talk = true;
        st.voice.settings.ptt_key = Some(key_name(&ev(Key::Text("V".into()))));
        assert_eq!(st.voice.settings.ptt_key.as_deref(), Some("Text(\"v\")"), "recorded lowercase");
        assert!(push_to_talk(&mut st, &ev(Key::Text("v".into()))));
        assert!(st.voice.ptt_held);
    }

    #[test]
    fn another_settings_tab_drops_the_voice_recorder() {
        let mut st = ShellState { settings: Some(crate::settings::SettingsUi::new(Some("my_profile"))), ..ShellState::default() };
        st.voice.recording_key = true;
        assert!(!push_to_talk(&mut st, &ev(Key::Text("x".into()))));
        assert!(!st.voice.recording_key);
        assert_eq!(st.voice.settings.ptt_key, None, "a key meant for another tab is not captured");
    }

    #[test]
    fn a_closed_voice_settings_modal_drops_the_recorder() {
        let mut st = ShellState::default();
        st.voice.recording_key = true;
        assert!(!push_to_talk(&mut st, &ev(Key::Text("x".into()))));
        assert!(!st.voice.recording_key);
        assert_eq!(st.voice.settings.ptt_key, None, "the key is not silently rebound");
    }
}
