//! Favorites through `synced_preferences.favorites` (`Favorites.ts` `makeSyncedField`): a
//! change on another device replaces the local list, a local change is sent back. Categories,
//! collapsed ones, nicknames and the muted flag stay as the snapshot has them.

use reflux_model::Snowflake;
use reflux_store::Store;

use super::favorites::Favorite;
use super::{mine, update_mine};
use crate::shell::{Intent, ShellState};
use crate::synced::{self, Field};

/// `SyncedPreferences.favorites`.
const FIELD: u32 = 40;

/// The snapshot last seen: its base64, the `FavoritesState` payload, and what it says.
#[derive(Clone, Debug, Default)]
pub(crate) struct Seen {
    account: Snowflake,
    b64: String,
    payload: Vec<u8>,
    list: Vec<Favorite>,
    hide_muted: bool,
}

/// `FavoritesState` → channels by `position`, and `hide_muted_channels`.
fn parse(payload: &[u8]) -> (Vec<Favorite>, bool) {
    let fs = synced::fields(payload).unwrap_or_default();
    let mut list: Vec<(u64, Favorite)> = fs
        .iter()
        .filter(|f| f.num == 1 && f.wire == 2)
        .filter_map(|f| {
            let ch = synced::fields(f.value)?;
            let get = |n: u32| ch.iter().find(|c| c.num == n);
            let channel: Snowflake = synced::str_of(get(1)?).parse().ok()?;
            let guild = get(2).map(synced::str_of).and_then(|g| g.parse().ok());
            Some((get(4).map_or(0, synced::uint_of), Favorite { guild, channel }))
        })
        .collect();
    list.sort_by_key(|(pos, _)| *pos);
    let hide = fs.iter().rev().find(|f| f.num == 4 && f.wire == 0).is_some_and(|f| synced::uint_of(f) != 0);
    (list.into_iter().map(|(_, f)| f).collect(), hide)
}

/// The local list as a `FavoritesState`, keeping `remote`'s categories, collapsed ids and muted
/// flag, and each kept channel's category and nickname.
fn build(list: &[Favorite], hide_muted: bool, remote: &[u8]) -> Vec<u8> {
    let fs = synced::fields(remote).unwrap_or_default();
    let mut out = Vec::new();
    for (i, f) in list.iter().enumerate() {
        let id = f.channel.to_string();
        let old = fs.iter().filter(|r| r.num == 1 && r.wire == 2).find_map(|r| {
            let ch = synced::fields(r.value)?;
            ch.iter().any(|c| c.num == 1 && synced::str_of(c) == id).then_some(ch)
        });
        let mut ch = Vec::new();
        synced::bytes(&mut ch, 1, id.as_bytes());
        synced::bytes(&mut ch, 2, f.guild.map_or_else(|| "@me".to_string(), |g| g.to_string()).as_bytes());
        // parent_id and nickname.
        for c in old.iter().flatten().filter(|c| matches!(c.num, 3 | 5)) {
            ch.extend_from_slice(c.raw);
        }
        if i > 0 {
            synced::uint(&mut ch, 4, i as u64);
        }
        synced::bytes(&mut out, 1, &ch);
    }
    for f in fs.iter().filter(|f: &&Field| matches!(f.num, 2 | 3 | 5) || f.num > 5) {
        out.extend_from_slice(f.raw);
    }
    if hide_muted {
        synced::uint(&mut out, 4, 1);
    }
    out
}

/// Once a frame: takes a new snapshot's favorites, else sends local changes.
pub fn poll(store: &Store, st: &mut ShellState) -> bool {
    if store.me.is_none() {
        return false;
    }
    let b64 = &store.settings.synced_preferences;
    let me = st.account.me;
    if st.nav.synced_favorites.as_ref().is_none_or(|s| s.b64 != *b64 || s.account != me) {
        let payload = synced::get(b64, FIELD).unwrap_or_default();
        let (list, hide_muted) = parse(&payload);
        let first = st.nav.synced_favorites.as_ref().is_none_or(|s| s.account != me);
        let had = !payload.is_empty();
        st.nav.synced_favorites = Some(Seen { account: me, b64: b64.clone(), payload, list: list.clone(), hide_muted });
        // A snapshot that never had favorites doesn't wipe this device's on first sight; they're
        // uploaded below.
        if had || !first {
            let a = mine(st);
            if a.favorites != list || a.hide_muted_favorites != hide_muted {
                update_mine(st, |a| (a.favorites, a.hide_muted_favorites) = (list, hide_muted));
                return true;
            }
        }
    }
    let Some(seen) = st.nav.synced_favorites.as_ref() else { return false };
    let a = mine(st);
    if a.favorites == seen.list && a.hide_muted_favorites == seen.hide_muted {
        return false;
    }
    let payload = build(&a.favorites, a.hide_muted_favorites, &seen.payload);
    let next = synced::set(&seen.b64, FIELD, (!payload.is_empty()).then_some(&payload[..]));
    st.nav.synced_favorites = Some(Seen { account: me, b64: next.clone(), payload, list: a.favorites, hide_muted: a.hide_muted_favorites });
    st.intents.push(Intent::PatchSettings(serde_json::json!({ "synced_preferences": next })));
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_keeps_categories_and_nicknames() {
        let fav = |g: Option<u64>, c: u64| Favorite { guild: g.map(Snowflake), channel: Snowflake(c) };
        let mut remote = Vec::new();
        let mut ch = Vec::new();
        synced::bytes(&mut ch, 1, b"7");
        synced::bytes(&mut ch, 2, b"1");
        synced::bytes(&mut ch, 3, b"cat");
        synced::bytes(&mut ch, 5, b"nick");
        synced::bytes(&mut remote, 1, &ch);
        let mut cat = Vec::new();
        synced::bytes(&mut cat, 1, b"cat");
        synced::bytes(&mut remote, 2, &cat);
        assert_eq!(parse(&remote), (vec![fav(Some(1), 7)], false));
        let out = build(&[fav(None, 9), fav(Some(1), 7)], true, &remote);
        assert_eq!(parse(&out), (vec![fav(None, 9), fav(Some(1), 7)], true));
        let fs = synced::fields(&out).unwrap();
        assert!(fs.iter().any(|f| f.num == 2), "categories kept");
        let seven = synced::fields(fs[1].value).unwrap();
        assert!(seven.iter().any(|c| c.num == 5 && c.value == b"nick"), "nickname kept");
        assert!(build(&[], false, &[]).is_empty());
    }
}
