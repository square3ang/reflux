//! "Sync keybinds" (`InputKeybind.ts` `makeSyncedField('keybinds')`): when on,
//! the custom keybinds and per-action built-in disables go through
//! `synced_preferences.keybinds`. Entries this client can't run (other actions, mouse / gamepad
//! / plain-key combos) and the voice transmit settings are kept as the snapshot has them.

use reflux_model::Snowflake;
use reflux_store::Store;

use super::keys::Custom;
use super::{mine, update_mine};
use crate::keybinds::{Action, Combo, K};
use crate::shell::{Intent, ShellState};
use crate::synced::{self, Field};

/// `SyncedPreferences.keybinds`.
const FIELD: u32 = 111;

#[derive(Clone, Debug, Default, PartialEq)]
struct Local {
    keybinds: Vec<Custom>,
    disabled: Vec<Action>,
}

/// The snapshot last seen (or sent) and what it says.
#[derive(Clone, Debug, Default)]
pub(crate) struct Seen {
    account: Snowflake,
    b64: String,
    payload: Vec<u8>,
    local: Local,
}

/// `KeybindCommands.ts` names.
fn command(a: Action) -> String {
    use Action::*;
    match a {
        ShowKeybinds => "system_toggle_shortcuts_overlay",
        GuildPrev => "nav_guild_prev",
        GuildNext => "nav_guild_next",
        ChannelPrev => "nav_channel_prev",
        ChannelNext => "nav_channel_next",
        UnreadPrev => "nav_unread_prev",
        UnreadNext => "nav_unread_next",
        MentionPrev => "nav_mention_prev",
        MentionNext => "nav_mention_next",
        GuildSlot(n) => return format!("nav_guild_slot_{n}"),
        MarkChannelRead => "chat_mark_channel_read",
        MarkGuildRead => "chat_mark_guild_read",
        ToggleMembers => "chat_toggle_member_list",
        ToggleMute => "voice_toggle_mute",
        ToggleDeafen => "voice_toggle_deafen",
        QuickSwitcher => "nav_quick_switcher",
        ZoomIn => "system_zoom_in",
        ZoomOut => "system_zoom_out",
        ZoomReset => "system_zoom_reset",
    }
    .to_string()
}

fn action_of(s: &str) -> Option<Action> {
    crate::keybinds::BINDS.iter().map(|b| b.action).find(|&a| command(a) == s)
}

/// `KeyboardEvent.key` for a combo.
fn key_name(k: K) -> String {
    match k {
        K::Up => "ArrowUp".into(),
        K::Down => "ArrowDown".into(),
        K::Escape => "Escape".into(),
        K::Char(c) => c.to_string(),
    }
}

/// A `KeybindCombo` this client can record: Ctrl/Cmd + a character, or an arrow / Escape.
fn combo_of(fs: &[Field]) -> Option<Combo> {
    let get = |n: u32| fs.iter().rev().find(|f| f.num == n);
    let flag = |n: u32| get(n).is_some_and(|f| synced::uint_of(f) != 0);
    if (flag(7) && !cfg!(target_os = "macos")) || flag(10) || get(12).is_some() || get(13).is_some() {
        return None;
    }
    let key = get(1).map(synced::str_of).unwrap_or_default();
    let ctrl = flag(3) || flag(4) || (cfg!(target_os = "macos") && flag(7));
    let key = match key.as_str() {
        "ArrowUp" => K::Up,
        "ArrowDown" => K::Down,
        "Escape" => K::Escape,
        s if s.chars().count() == 1 && ctrl => K::Char(s.chars().next()?.to_ascii_lowercase()),
        _ => return None,
    };
    Some(Combo { key, ctrl, alt: flag(5), shift: flag(6) })
}

/// `KeybindSettings` → what this client keeps, and the raw entries it can't.
fn parse(payload: &[u8]) -> Local {
    let mut out = Local::default();
    for f in synced::fields(payload).unwrap_or_default().iter().filter(|f| f.num == 1 && f.wire == 2) {
        let Some(e) = synced::fields(f.value) else { continue };
        let get = |n: u32| e.iter().rev().find(|x| x.num == n);
        let action = get(2).map(synced::str_of).filter(|s| !s.is_empty());
        let combo = get(3).and_then(|c| synced::fields(c.value)).unwrap_or_default();
        let enabled = get(4).is_some_and(|x| synced::uint_of(x) != 0);
        let cget = |n: u32| combo.iter().rev().find(|x| x.num == n);
        let key_set = cget(1).is_some_and(|x| !x.value.is_empty()) || cget(2).is_some_and(|x| !x.value.is_empty()) || cget(12).is_some() || cget(13).is_some();
        // `isBuiltinDisableMarker`: an action whose combo is switched off.
        if cget(9).is_some_and(|x| synced::uint_of(x) == 0) && action.is_some() {
            if let Some(a) = action.as_deref().and_then(action_of) {
                out.disabled.push(a);
            }
            continue;
        }
        let a = action.as_deref().map(action_of);
        if a == Some(None) {
            continue;
        }
        let c = if key_set { combo_of(&combo) } else { None };
        if key_set && c.is_none() {
            continue;
        }
        out.keybinds.push(Custom { action: a.flatten(), combo: c, enabled });
    }
    out
}

/// Whether a raw entry is one [`parse`] skips (so [`build`] keeps it as is).
fn foreign(raw: &[u8]) -> bool {
    let mut one = Vec::new();
    synced::bytes(&mut one, 1, raw);
    let p = parse(&one);
    p.keybinds.is_empty() && p.disabled.is_empty()
}

fn id() -> String {
    let h = |n: usize| (0..n).map(|_| format!("{:x}", fastrand::u8(0..16))).collect::<String>();
    format!("{}-{}-4{}-{}-{}", h(8), h(4), h(3), h(4), h(12))
}

/// The local keybinds as `KeybindSettings`, keeping `remote`'s foreign entries, ids and voice
/// settings.
fn build(l: &Local, remote: &[u8]) -> Vec<u8> {
    let fs = synced::fields(remote).unwrap_or_default();
    let mut ids: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for f in fs.iter().filter(|f| f.num == 1 && f.wire == 2) {
        if foreign(f.value) {
            out.extend_from_slice(f.raw);
        } else if let Some(i) = synced::fields(f.value).and_then(|e| e.iter().find(|x| x.num == 1).map(synced::str_of)) {
            ids.push(i);
        }
    }
    let mut ids = ids.into_iter();
    let mut entry = |action: Option<Action>, combo: Vec<u8>, enabled: bool| {
        let mut e = Vec::new();
        synced::bytes(&mut e, 1, ids.next().unwrap_or_else(id).as_bytes());
        if let Some(a) = action {
            synced::bytes(&mut e, 2, command(a).as_bytes());
        }
        synced::bytes(&mut e, 3, &combo);
        if enabled {
            synced::uint(&mut e, 4, 1);
        }
        synced::bytes(&mut out, 1, &e);
    };
    for c in &l.keybinds {
        let mut combo = Vec::new();
        if let Some(k) = c.combo {
            synced::bytes(&mut combo, 1, key_name(k.key).as_bytes());
            for (n, on) in [(3, k.ctrl), (5, k.alt), (6, k.shift)] {
                if on {
                    synced::uint(&mut combo, n, 1);
                }
            }
        }
        entry(c.action, combo, c.enabled);
    }
    for &a in &l.disabled {
        let mut combo = Vec::new();
        synced::bytes(&mut combo, 1, b"");
        synced::uint(&mut combo, 9, 0);
        entry(Some(a), combo, true);
    }
    for f in fs.iter().filter(|f| f.num != 1) {
        out.extend_from_slice(f.raw);
    }
    out
}

/// Once a frame while syncing is on: takes a new snapshot's keybinds, else sends local changes.
/// `push` (set by the switch) sends this device's keybinds first instead (`pushSyncedKeybinds`).
pub fn poll(store: &Store, st: &mut ShellState) -> bool {
    if store.me.is_none() {
        return false;
    }
    let me = st.account.me;
    let a = mine(st);
    if !a.sync_keybinds {
        st.nav.synced_keys = None;
        return false;
    }
    let local = Local { keybinds: a.keybinds.clone(), disabled: a.disabled_builtins.clone() };
    let b64 = store.settings.synced_preferences.clone();
    let push = std::mem::take(&mut st.nav.push_keys);
    let fresh = st.nav.synced_keys.as_ref().is_none_or(|s| s.account != me);
    if !push && st.nav.synced_keys.as_ref().is_none_or(|s| s.b64 != b64 || s.account != me) {
        let payload = synced::get(&b64, FIELD).unwrap_or_default();
        let remote = parse(&payload);
        let had = !payload.is_empty();
        st.nav.synced_keys = Some(Seen { account: me, b64, payload, local: remote.clone() });
        if (had || !fresh) && local != remote {
            update_mine(st, |m| (m.keybinds, m.disabled_builtins) = (remote.keybinds, remote.disabled));
            return true;
        }
    }
    let seen = st.nav.synced_keys.clone().unwrap_or_else(|| {
        let b64 = store.settings.synced_preferences.clone();
        Seen { account: me, payload: synced::get(&b64, FIELD).unwrap_or_default(), b64, local: Local::default() }
    });
    if !push && local == seen.local {
        return false;
    }
    let payload = build(&local, &seen.payload);
    let next = synced::set(&seen.b64, FIELD, Some(&payload));
    st.nav.synced_keys = Some(Seen { account: me, b64: next.clone(), payload, local });
    st.intents.push(Intent::PatchSettings(serde_json::json!({ "synced_preferences": next })));
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_keeps_foreign_entries() {
        let j = Combo { key: K::Char('j'), ctrl: true, alt: false, shift: false };
        let mut remote = Vec::new();
        let mut e = Vec::new();
        synced::bytes(&mut e, 1, b"x");
        synced::bytes(&mut e, 2, b"voice_push_to_talk");
        synced::bytes(&mut remote, 1, &e);
        synced::bytes(&mut remote, 2, b"voice_push_to_talk");
        let l = Local {
            keybinds: vec![Custom { action: Some(Action::QuickSwitcher), combo: Some(j), enabled: true }, Custom { action: None, combo: None, enabled: false }],
            disabled: vec![Action::GuildSlot(3)],
        };
        let out = build(&l, &remote);
        assert_eq!(parse(&out), l);
        let fs = synced::fields(&out).unwrap();
        assert!(fs.iter().any(|f| f.num == 1 && f.value == &e[..]), "foreign entry kept");
        assert!(fs.iter().any(|f| f.num == 2), "transmit mode kept");
    }
}
