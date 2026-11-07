//! The pickers' state through `synced_preferences` (`makeSyncedField` in `EmojiPicker.ts`,
//! `StickerPicker.ts`, `Emoji.ts` and `FavoriteGif.ts`): emoji and sticker frecency, favorites
//! and folded categories, the skin tone, and favorite GIFs. A snapshot from another device
//! merges usage (higher count and later use win, `mergeWireUsageMaps`) and replaces the rest;
//! local changes are sent back after 1.5 s (`USAGE_SYNC_DEBOUNCE_MS`). Fields this client
//! doesn't model (memes picker, GIF prompt flags) keep the snapshot's bytes.

use std::collections::BTreeMap;

use reflux_model::Snowflake;
use reflux_store::Store;
use serde_json::{Map, Value, json};

use super::{Saved, Usage};
use crate::shell::{Intent, ShellState};
use crate::synced::{self, Field};

/// `SyncedPreferences.emoji_picker`, `.sticker_picker`, `.emoji`, `.favorite_gifs`.
const EMOJI: u32 = 20;
const STICKER: u32 = 21;
const EMOJI_STATE: u32 = 23;
const GIFS: u32 = 25;
const DEBOUNCE_MS: u64 = 1_500;
/// Skin tone index → `EmojiState.skin_tone` (the modifier's surrogate; "" = default).
const TONES: [&str; 6] = [
    "",
    "\u{1F3FB}",
    "\u{1F3FC}",
    "\u{1F3FD}",
    "\u{1F3FE}",
    "\u{1F3FF}",
];
/// Local sticker keys and folded sticker categories carry this; the wire's don't.
const STICKER_PREFIX: &str = "sticker:";

/// The snapshot last seen (or sent).
#[derive(Clone, Debug, Default)]
pub struct Seen {
    account: Snowflake,
    b64: String,
    /// What the snapshot holds, as this client would encode it.
    wire: [Vec<u8>; 4],
    /// When the local state started to differ from it.
    dirty_since: Option<u64>,
}

fn of<'a>(fs: &'a [Field<'a>], num: u32) -> impl Iterator<Item = &'a Field<'a>> {
    fs.iter().filter(move |f| f.num == num)
}

fn str_field(out: &mut Vec<u8>, num: u32, v: &str) {
    if !v.is_empty() {
        synced::bytes(out, num, v.as_bytes());
    }
}

fn uint_field(out: &mut Vec<u8>, num: u32, v: u64) {
    if v != 0 {
        synced::uint(out, num, v);
    }
}

/// `{Emoji,Sticker}PickerState`: usage (map<string, UsageStat>), favorite ids, folded ids.
fn parse_picker(payload: &[u8]) -> (BTreeMap<String, Usage>, Vec<String>, Vec<String>) {
    let fs = synced::fields(payload).unwrap_or_default();
    let usage = of(&fs, 1)
        .filter_map(|e| {
            let kv = synced::fields(e.value)?;
            let key = synced::str_of(of(&kv, 1).last()?);
            let stat = of(&kv, 2)
                .last()
                .and_then(|v| synced::fields(v.value))
                .unwrap_or_default();
            let n = |num| of(&stat, num).last().map_or(0, synced::uint_of);
            Some((
                key,
                Usage {
                    count: n(1) as u32,
                    last_used: n(2),
                },
            ))
        })
        .collect();
    let list = |num| of(&fs, num).map(synced::str_of).collect();
    (usage, list(2), list(3))
}

fn build_picker(
    usage: &BTreeMap<String, Usage>,
    favorites: &[String],
    collapsed: &[String],
    remote: &[u8],
) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, u) in usage {
        let mut stat = Vec::new();
        uint_field(&mut stat, 1, u.count.into());
        uint_field(&mut stat, 2, u.last_used);
        let mut kv = Vec::new();
        str_field(&mut kv, 1, k);
        synced::bytes(&mut kv, 2, &stat);
        synced::bytes(&mut out, 1, &kv);
    }
    for f in favorites {
        synced::bytes(&mut out, 2, f.as_bytes());
    }
    for c in collapsed {
        synced::bytes(&mut out, 3, c.as_bytes());
    }
    keep_unknown(&mut out, remote, 3);
    out
}

/// Copies `remote`'s fields numbered above `known`.
fn keep_unknown(out: &mut Vec<u8>, remote: &[u8], known: u32) {
    for f in synced::fields(remote)
        .unwrap_or_default()
        .iter()
        .filter(|f| f.num > known)
    {
        out.extend_from_slice(f.raw);
    }
}

/// `FavoriteGifEntry` → the JSON the GIF tab shows (`url`, `proxy_url`, size, `media`, …).
fn gif_from_wire(b: &[u8]) -> Option<Value> {
    let fs = synced::fields(b)?;
    let s = |num| of(&fs, num).last().map(synced::str_of).unwrap_or_default();
    let n = |num| of(&fs, num).last().map_or(0, synced::uint_of);
    let mut media = Map::new();
    for e in of(&fs, 5) {
        let Some(kv) = synced::fields(e.value) else {
            continue;
        };
        let Some(key) = of(&kv, 1).last().map(synced::str_of) else {
            continue;
        };
        let fm = of(&kv, 2)
            .last()
            .and_then(|v| synced::fields(v.value))
            .unwrap_or_default();
        let s = |num| of(&fm, num).last().map(synced::str_of).unwrap_or_default();
        let n = |num| of(&fm, num).last().map_or(0, synced::uint_of);
        media.insert(
            key,
            json!({ "src": s(1), "proxy_src": s(2), "width": n(3), "height": n(4) }),
        );
    }
    let placeholder = s(7);
    Some(json!({
        "url": s(1), "proxy_url": s(2), "width": n(3), "height": n(4), "media": media,
        "content_type": s(6), "placeholder": if placeholder.is_empty() { Value::Null } else { placeholder.into() },
    }))
}

/// A favorite (a GIF object from the picker or a resolved entry) as a `FavoriteGifEntry`.
fn gif_to_wire(v: &Value) -> Option<Vec<u8>> {
    let url = v["url"].as_str().filter(|u| !u.is_empty())?;
    let s = |k: &str| v[k].as_str().unwrap_or_default();
    let n = |k: &str| v[k].as_u64().unwrap_or(0);
    let mut out = Vec::new();
    str_field(&mut out, 1, url);
    str_field(
        &mut out,
        2,
        [s("proxy_url"), s("proxy_src")]
            .into_iter()
            .find(|x| !x.is_empty())
            .unwrap_or(url),
    );
    uint_field(&mut out, 3, n("width"));
    uint_field(&mut out, 4, n("height"));
    for (k, f) in v["media"].as_object().into_iter().flatten() {
        let mut fm = Vec::new();
        str_field(&mut fm, 1, f["src"].as_str().unwrap_or_default());
        str_field(&mut fm, 2, f["proxy_src"].as_str().unwrap_or_default());
        uint_field(&mut fm, 3, f["width"].as_u64().unwrap_or(0));
        uint_field(&mut fm, 4, f["height"].as_u64().unwrap_or(0));
        let mut kv = Vec::new();
        str_field(&mut kv, 1, k);
        synced::bytes(&mut kv, 2, &fm);
        synced::bytes(&mut out, 5, &kv);
    }
    str_field(&mut out, 6, s("content_type"));
    str_field(&mut out, 7, s("placeholder"));
    Some(out)
}

/// What the snapshot says, and which of the four fields it has.
fn parse(b64: &str) -> (Saved, [bool; 4]) {
    let get = |num| synced::get(b64, num);
    let (emoji, sticker, state, gifs) = (get(EMOJI), get(STICKER), get(EMOJI_STATE), get(GIFS));
    let has = [
        emoji.is_some(),
        sticker.is_some(),
        state.is_some(),
        gifs.is_some(),
    ];
    let (emoji_usage, favorite_emojis, mut collapsed) =
        parse_picker(emoji.as_deref().unwrap_or_default());
    let (su, sf, sc) = parse_picker(sticker.as_deref().unwrap_or_default());
    let local = |k: String| format!("{STICKER_PREFIX}{k}");
    collapsed.extend(sc.into_iter().map(local));
    let tone = state
        .as_deref()
        .and_then(synced::fields)
        .and_then(|fs| of(&fs, 1).last().map(synced::str_of))
        .unwrap_or_default();
    let gif_fs = gifs.as_deref().and_then(synced::fields).unwrap_or_default();
    let saved = Saved {
        skin_tone: TONES.iter().position(|t| *t == tone).unwrap_or(0) as u8,
        emoji_usage,
        favorite_emojis,
        sticker_usage: su.into_iter().map(|(k, u)| (local(k), u)).collect(),
        favorite_stickers: sf.into_iter().map(local).collect(),
        collapsed,
        favorite_gifs: of(&gif_fs, 1)
            .filter_map(|f| gif_from_wire(f.value))
            .collect(),
    };
    (saved, has)
}

/// `saved` as the four payloads, keeping `b64`'s fields this client doesn't model.
fn encode(saved: &Saved, b64: &str) -> [Vec<u8>; 4] {
    let remote = |num| synced::get(b64, num).unwrap_or_default();
    let wire = |k: &String| k.strip_prefix(STICKER_PREFIX).map(str::to_string);
    let (emoji_collapsed, sticker_collapsed): (Vec<String>, Vec<String>) = saved
        .collapsed
        .iter()
        .cloned()
        .partition(|c| !c.starts_with(STICKER_PREFIX));
    let sticker_usage = saved
        .sticker_usage
        .iter()
        .filter_map(|(k, u)| Some((wire(k)?, *u)))
        .collect();
    let emoji = build_picker(
        &saved.emoji_usage,
        &saved.favorite_emojis,
        &emoji_collapsed,
        &remote(EMOJI),
    );
    let sticker = build_picker(
        &sticker_usage,
        &saved
            .favorite_stickers
            .iter()
            .filter_map(wire)
            .collect::<Vec<_>>(),
        &sticker_collapsed
            .iter()
            .filter_map(wire)
            .collect::<Vec<_>>(),
        &remote(STICKER),
    );
    let mut state = Vec::new();
    str_field(
        &mut state,
        1,
        TONES
            .get(saved.skin_tone as usize)
            .copied()
            .unwrap_or_default(),
    );
    keep_unknown(&mut state, &remote(EMOJI_STATE), 1);
    let mut gifs = Vec::new();
    for g in saved.favorite_gifs.iter().filter_map(gif_to_wire) {
        synced::bytes(&mut gifs, 1, &g);
    }
    // save_as_saved_media, seen_first_time_prompt and anything newer.
    keep_unknown(&mut gifs, &remote(GIFS), 1);
    [emoji, sticker, state, gifs]
}

/// The local state after `remote` arrives: usage merged, the rest replaced — except, on first
/// sight, fields the snapshot never had, so this device's are uploaded instead of wiped.
fn apply(local: &Saved, remote: Saved, has: [bool; 4], first: bool) -> Saved {
    let merge = |a: &BTreeMap<String, Usage>, b: BTreeMap<String, Usage>| {
        let mut out = a.clone();
        for (k, u) in b {
            let e = out.entry(k).or_insert(u);
            (e.count, e.last_used) = (e.count.max(u.count), e.last_used.max(u.last_used));
        }
        out
    };
    let take = |i: usize| has[i] || !first;
    let mut next = local.clone();
    next.emoji_usage = merge(&local.emoji_usage, remote.emoji_usage);
    next.sticker_usage = merge(&local.sticker_usage, remote.sticker_usage);
    let (emoji_collapsed, sticker_collapsed): (Vec<String>, Vec<String>) = remote
        .collapsed
        .into_iter()
        .partition(|c| !c.starts_with(STICKER_PREFIX));
    let (local_emoji, local_sticker): (Vec<String>, Vec<String>) = local
        .collapsed
        .iter()
        .cloned()
        .partition(|c| !c.starts_with(STICKER_PREFIX));
    let emoji_collapsed = if take(0) {
        emoji_collapsed
    } else {
        local_emoji
    };
    let sticker_collapsed = if take(1) {
        sticker_collapsed
    } else {
        local_sticker
    };
    next.collapsed = emoji_collapsed
        .into_iter()
        .chain(sticker_collapsed)
        .collect();
    if take(0) {
        next.favorite_emojis = remote.favorite_emojis;
    }
    if take(1) {
        next.favorite_stickers = remote.favorite_stickers;
    }
    if take(2) {
        next.skin_tone = remote.skin_tone;
    }
    if take(3) {
        next.favorite_gifs = remote.favorite_gifs;
    }
    next
}

/// Once a frame: takes a new snapshot, else sends local changes once they've settled.
pub fn poll(store: &Store, st: &mut ShellState, tx: &mut rune::Tx) -> bool {
    if store.me.is_none() {
        return false;
    }
    let b64 = &store.settings.synced_preferences;
    let me = st.account.me;
    let mut changed = false;
    if st
        .expr
        .synced
        .as_ref()
        .is_none_or(|s| s.b64 != *b64 || s.account != me)
    {
        let first = st.expr.synced.as_ref().is_none_or(|s| s.account != me);
        let (remote, has) = parse(b64);
        let wire = encode(&remote, b64);
        let next = apply(&st.expr.saved, remote, has, first);
        if next != st.expr.saved {
            st.expr.saved = next;
            st.expr.save();
            changed = true;
        }
        st.expr.synced = Some(Seen {
            account: me,
            b64: b64.clone(),
            wire,
            dirty_since: None,
        });
    }
    let now = st.now_ms;
    // A reverse image search default goes out at once, in the same snapshot.
    let ris = st.expr.ris_default.take();
    let Some(seen) = st.expr.synced.as_mut() else {
        return changed;
    };
    let local = encode(&st.expr.saved, &seen.b64);
    if local == seen.wire && ris.is_none() {
        seen.dirty_since = None;
        return changed;
    }
    let since = *seen.dirty_since.get_or_insert(now);
    if now < since + DEBOUNCE_MS && ris.is_none() {
        tx.wake_after(std::time::Duration::from_millis(since + DEBOUNCE_MS - now));
        return changed;
    }
    let mut next = match ris {
        Some(d) => super::image_search::with_default(&seen.b64, d.as_deref()),
        None => seen.b64.clone(),
    };
    for (num, payload) in [EMOJI, STICKER, EMOJI_STATE, GIFS].into_iter().zip(&local) {
        next = synced::set(&next, num, (!payload.is_empty()).then_some(&payload[..]));
    }
    *seen = Seen {
        account: me,
        b64: next.clone(),
        wire: local,
        dirty_since: None,
    };
    st.intents
        .push(Intent::PatchSettings(json!({ "synced_preferences": next })));
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Saved {
        Saved {
            skin_tone: 3,
            emoji_usage: [(
                "unicode:joy".to_string(),
                Usage {
                    count: 4,
                    last_used: 1_700_000_000_000,
                },
            )]
            .into(),
            favorite_emojis: vec!["custom:1:2".into(), "unicode:heart".into()],
            sticker_usage: [(
                "sticker:1:88".to_string(),
                Usage {
                    count: 1,
                    last_used: 5,
                },
            )]
            .into(),
            favorite_stickers: vec!["sticker:1:88".into()],
            collapsed: vec!["favorites".into(), "sticker:1".into()],
            favorite_gifs: vec![
                json!({"id": "x", "url": "https://klipy.com/gifs/a", "proxy_src": "https://m/a.webm", "width": 220, "height": 229,
                "media": {"webm": {"src": "https://s/a.webm", "proxy_src": "https://m/a.webm", "width": 220, "height": 229}}}),
            ],
        }
    }

    #[test]
    fn round_trips_through_the_wire_and_keeps_unknown_fields() {
        let s = sample();
        // A snapshot with an unknown EmojiPickerState field and GIF prompt flags.
        let mut emoji = Vec::new();
        synced::uint(&mut emoji, 9, 1);
        let mut gifs = Vec::new();
        synced::uint(&mut gifs, 2, 1);
        let b64 = synced::set(&synced::set("", EMOJI, Some(&emoji)), GIFS, Some(&gifs));
        let mut next = b64.clone();
        for (num, p) in [EMOJI, STICKER, EMOJI_STATE, GIFS]
            .into_iter()
            .zip(encode(&s, &b64))
        {
            next = synced::set(&next, num, Some(&p));
        }
        let (back, has) = parse(&next);
        assert_eq!(has, [true; 4]);
        assert_eq!(
            (back.skin_tone, &back.emoji_usage, &back.favorite_emojis),
            (3, &s.emoji_usage, &s.favorite_emojis)
        );
        assert_eq!(
            (
                &back.sticker_usage,
                &back.favorite_stickers,
                &back.collapsed
            ),
            (&s.sticker_usage, &s.favorite_stickers, &s.collapsed)
        );
        assert_eq!(back.favorite_gifs[0]["proxy_url"], "https://m/a.webm");
        assert_eq!(back.favorite_gifs[0]["media"]["webm"]["width"], 220);
        assert_eq!(
            encode(&back, &next),
            encode(&s, &next),
            "stable once on the wire"
        );
        let e = synced::get(&next, EMOJI).unwrap();
        assert!(
            synced::fields(&e).unwrap().iter().any(|f| f.num == 9),
            "unknown field kept"
        );
        let g = synced::get(&next, GIFS).unwrap();
        assert!(
            synced::fields(&g).unwrap().iter().any(|f| f.num == 2),
            "save_as_saved_media kept"
        );
    }

    #[test]
    fn remote_merges_usage_and_replaces_the_rest_except_on_first_sight() {
        let local = sample();
        let remote = Saved {
            emoji_usage: [(
                "unicode:joy".to_string(),
                Usage {
                    count: 2,
                    last_used: 1_800_000_000_000,
                },
            )]
            .into(),
            favorite_emojis: vec!["unicode:fire".into()],
            ..Default::default()
        };
        let next = apply(&local, remote.clone(), [true, false, false, false], true);
        assert_eq!(
            next.emoji_usage["unicode:joy"],
            Usage {
                count: 4,
                last_used: 1_800_000_000_000
            }
        );
        assert_eq!(next.favorite_emojis, ["unicode:fire"]);
        assert_eq!(
            next.favorite_stickers, local.favorite_stickers,
            "absent on first sight: kept"
        );
        assert_eq!(
            next.collapsed,
            ["sticker:1"],
            "emoji folds replaced, sticker folds kept"
        );
        let later = apply(&local, remote, [true, false, false, false], false);
        assert!(later.favorite_stickers.is_empty() && later.favorite_gifs.is_empty());
    }
}
