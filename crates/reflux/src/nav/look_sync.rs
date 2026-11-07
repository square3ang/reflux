//! The device look preferences Fluxer syncs through `synced_preferences`
//! (`Accessibility.ts`, `Sound.ts`, `ChatInputSettings.ts`, `PrivacyPreferences`): a change on
//! another device lands in [`crate::look::Prefs`], a local change is sent back. Fields Reflux has
//! no setting for stay as the snapshot has them.

use reflux_model::Snowflake;
use reflux_store::Store;

use crate::look::Prefs;
use crate::shell::{Intent, ShellState};
use crate::synced;

/// `SyncedPreferences`: accessibility, privacy, sound, chat_input.
const TOPS: [u32; 4] = [1, 80, 100, 112];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Val {
    B(bool),
    D(f64),
    /// A proto enum: Fluxer's `…_UNSPECIFIED` is 0, so ours is the value less one.
    E(u64),
}

type Get = fn(&Prefs) -> Val;
type Put = fn(&mut Prefs, Val);

/// (sub-message field, plain proto3 field that reads as its zero when absent, get, put).
struct Entry(u32, bool, Get, Put);

macro_rules! b {
    ($n:expr, $f:ident) => {
        b!($n, false, $f)
    };
    ($n:expr, $plain:expr, $f:ident) => {
        Entry($n, $plain, |p| Val::B(p.$f), |p, v| if let Val::B(x) = v { p.$f = x })
    };
}
macro_rules! d {
    ($n:expr, $f:ident) => {
        Entry($n, false, |p| Val::D(f64::from(p.$f)), |p, v| if let Val::D(x) = v { p.$f = x as f32 })
    };
}
macro_rules! e {
    ($n:expr, $f:ident) => {
        Entry($n, true, |p| Val::E(u64::from(p.$f) + 1), |p, v| if let Val::E(x) = v { p.$f = x.saturating_sub(1).min(2) as u8 })
    };
}

/// The fields Reflux has, by sub-message (`AccessibilitySettings`, `PrivacyPreferences`,
/// `SoundSettings`, `ChatInputSettings`).
fn entries(top: u32) -> Vec<Entry> {
    match top {
        1 => vec![
            d!(1, saturation),
            b!(2, true, underline_links),
            b!(4, show_send_button),
            b!(5, focus_ring),
            b!(7, escape_exits_keyboard),
            b!(8, sync_motion),
            b!(9, reduce_motion),
            d!(10, group_spacing),
            d!(12, font_size),
            b!(13, compact_avatars),
            b!(21, show_gif_button),
            b!(22, show_memes_button),
            b!(23, show_stickers_button),
            b!(24, show_emoji_button),
            e!(32, typing_indicator),
            b!(34, action_bar),
            b!(35, action_bar_quick_reactions),
            b!(36, action_bar_shift_expand),
            b!(37, action_bar_more_only),
            e!(48, dm_previews),
            b!(49, tts_command),
            d!(50, tts_rate),
            b!(51, faded_unread_muted),
            b!(52, menu_shortcuts),
            b!(53, confirm_calls),
            b!(58, announce_messages),
            d!(60, compact_group_spacing),
            b!(62, dim_strikethrough),
            b!(66, double_click_actions),
        ],
        80 => vec![b!(2, active_now)],
        100 => vec![d!(2, volume)],
        112 => vec![b!(1, convert_emoticons)],
        _ => Vec::new(),
    }
}

fn put(out: &mut Vec<u8>, num: u32, v: Val) {
    match v {
        Val::B(b) => synced::uint(out, num, u64::from(b)),
        Val::E(e) => synced::uint(out, num, e),
        Val::D(d) => {
            synced::put_varint(out, u64::from(num) << 3 | 1);
            out.extend_from_slice(&d.to_le_bytes());
        }
    }
}

/// `SoundSettings` maps: `disabled_sounds` (3) and `sound_overrides` (4), as key/value entries.
fn put_sound_maps(out: &mut Vec<u8>, p: &Prefs) {
    for (k, on) in &p.sounds_off {
        let mut e = Vec::new();
        synced::bytes(&mut e, 1, k.as_bytes());
        synced::uint(&mut e, 2, u64::from(*on));
        synced::bytes(out, 3, &e);
    }
    for (k, v) in &p.sound_volumes {
        let mut e = Vec::new();
        synced::bytes(&mut e, 1, k.as_bytes());
        put(&mut e, 2, Val::D(f64::from(*v)));
        synced::bytes(out, 4, &e);
    }
}

/// `top`'s sub-message with `p`'s values, keeping `remote`'s other fields.
fn encode(top: u32, p: &Prefs, remote: &[u8]) -> Vec<u8> {
    let es = entries(top);
    let mine = |n: u32| es.iter().any(|e| e.0 == n) || (top == 100 && (n == 3 || n == 4));
    let mut out: Vec<u8> = synced::fields(remote).unwrap_or_default().iter().filter(|f| !mine(f.num)).flat_map(|f| f.raw.iter().copied()).collect();
    for e in &es {
        put(&mut out, e.0, (e.2)(p));
    }
    if top == 100 {
        put_sound_maps(&mut out, p);
    }
    out
}

/// Applies `top`'s sub-message to `p` (`applyMessage`: set fields, plain ones always).
fn decode(top: u32, sub: &[u8], p: &mut Prefs) {
    let fs = synced::fields(sub).unwrap_or_default();
    for e in entries(top) {
        let f = fs.iter().rev().find(|f| f.num == e.0);
        let v = match ((e.2)(p), f) {
            (Val::B(_), Some(f)) => Val::B(synced::uint_of(f) != 0),
            (Val::E(_), Some(f)) => Val::E(synced::uint_of(f)),
            (Val::D(_), Some(f)) if f.wire == 1 => Val::D(f64::from_le_bytes(f.value.try_into().unwrap_or_default())),
            (Val::B(_), None) if e.1 => Val::B(false),
            (Val::E(_), None) if e.1 => Val::E(0),
            _ => continue,
        };
        (e.3)(p, v);
    }
    if top == 100 {
        // (key, value's wire type and bytes).
        let entry = |f: &synced::Field| -> Option<(String, Option<(u8, Vec<u8>)>)> {
            let kv = synced::fields(f.value)?;
            let k = synced::str_of(kv.iter().find(|x| x.num == 1)?);
            Some((k, kv.iter().find(|x| x.num == 2).map(|v| (v.wire, v.value.to_vec()))))
        };
        p.sounds_off = fs.iter().filter(|f| f.num == 3).filter_map(entry).map(|(k, v)| (k, v.is_some_and(|(_, b)| b.iter().any(|&x| x != 0)))).collect();
        p.sound_volumes = fs
            .iter()
            .filter(|f| f.num == 4)
            .filter_map(entry)
            .filter_map(|(k, v)| Some((k, f64::from_le_bytes(v.filter(|v| v.0 == 1)?.1.try_into().ok()?) as f32)))
            .collect();
    }
}

/// What `p` would send, per sub-message (with nothing else kept), to spot local changes.
fn project(p: &Prefs) -> Vec<Vec<u8>> {
    TOPS.iter().map(|&t| encode(t, p, &[])).collect()
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Seen {
    account: Snowflake,
    b64: String,
    sent: Vec<Vec<u8>>,
}

/// Once a frame: takes a new snapshot's values, else sends local changes.
pub fn poll(store: &Store, st: &mut ShellState) -> bool {
    if store.me.is_none() {
        return false;
    }
    let b64 = &store.settings.synced_preferences;
    let me = st.account.me;
    if st.nav.synced_look.as_ref().is_none_or(|s| s.b64 != *b64 || s.account != me) {
        let first = st.nav.synced_look.as_ref().is_none_or(|s| s.account != me);
        let subs: Vec<Option<Vec<u8>>> = TOPS.iter().map(|&t| synced::get(b64, t)).collect();
        // A snapshot without any of these doesn't reset this device on first sight; its values
        // are uploaded below.
        if first && subs.iter().all(Option::is_none) {
            st.nav.synced_look = Some(Seen { account: me, b64: b64.clone(), sent: Vec::new() });
        } else {
            let mut look = st.prefs.look.clone();
            for (&t, sub) in TOPS.iter().zip(&subs) {
                if let Some(sub) = sub {
                    decode(t, sub, &mut look);
                }
            }
            st.nav.synced_look = Some(Seen { account: me, b64: b64.clone(), sent: project(&look) });
            if look != st.prefs.look {
                st.prefs.look = look;
                st.intents.push(Intent::Prefs(Box::new(st.prefs.clone())));
                return true;
            }
        }
    }
    let Some(seen) = st.nav.synced_look.as_ref() else { return false };
    let local = project(&st.prefs.look);
    if local == seen.sent {
        return false;
    }
    let mut next = seen.b64.clone();
    for (i, &t) in TOPS.iter().enumerate() {
        if seen.sent.get(i) != Some(&local[i]) {
            let remote = synced::get(&seen.b64, t).unwrap_or_default();
            next = synced::set(&next, t, Some(&encode(t, &st.prefs.look, &remote)));
        }
    }
    st.nav.synced_look = Some(Seen { account: me, b64: next.clone(), sent: local });
    st.intents.push(Intent::PatchSettings(serde_json::json!({ "synced_preferences": next })));
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_keeps_other_fields() {
        let mut p = Prefs { font_size: 18.0, saturation: 0.5, typing_indicator: 2, dm_previews: 0, underline_links: true, convert_emoticons: true, volume: 80.0, ..Prefs::default() };
        p.sounds_off.insert("message".into(), true);
        p.sound_volumes.insert("mention".into(), 150.0);
        let mut remote = Vec::new();
        synced::uint(&mut remote, 6, 1); // hide_keyboard_hints: not ours, kept
        synced::uint(&mut remote, 12, 9); // a stale font_size of the wrong wire type: replaced
        let acc = encode(1, &p, &remote);
        let fs = synced::fields(&acc).unwrap();
        assert!(fs.iter().any(|f| f.num == 6), "unknown fields kept");
        assert_eq!(fs.iter().filter(|f| f.num == 12).count(), 1);
        let mut q = Prefs::default();
        for t in TOPS {
            decode(t, &encode(t, &p, &[]), &mut q);
        }
        assert_eq!(q, p);
        // A plain bool and an enum missing from the message read as their zero / unspecified.
        decode(1, &[], &mut q);
        assert_eq!((q.underline_links, q.typing_indicator, q.font_size), (false, 0, 18.0), "optional fields stay");
    }
}
