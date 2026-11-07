//! Favorites through `synced_preferences.favorites` (`Favorites.ts` `makeSyncedField`): a
//! change on another device replaces the local list, categories, collapsed ones and the
//! hide-muted flag; a local change is sent back. The `muted` flag and unknown fields stay as the
//! snapshot has them.

use reflux_model::Snowflake;
use reflux_store::Store;

use super::favorites::{Category, Favorite};
use super::{AccountPrefs, mine, update_mine};
use crate::shell::ShellState;
use crate::synced::{self, Field};

/// `SyncedPreferences.favorites`.
const FIELD: u32 = 40;

/// What a `FavoritesState` says (or the local prefs would send).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct State {
    list: Vec<Favorite>,
    cats: Vec<Category>,
    collapsed: Vec<String>,
    hide_muted: bool,
}

impl State {
    fn of(a: &AccountPrefs) -> Self {
        Self { list: a.favorites.clone(), cats: a.favorite_categories.clone(), collapsed: a.collapsed_favorites.clone(), hide_muted: a.hide_muted_favorites }
    }
    fn put(self, a: &mut AccountPrefs) {
        (a.favorites, a.favorite_categories, a.collapsed_favorites, a.hide_muted_favorites) = (self.list, self.cats, self.collapsed, self.hide_muted);
    }
}

/// The snapshot last seen: its base64, the `FavoritesState` payload, and what it says.
#[derive(Clone, Debug, Default)]
pub(crate) struct Seen {
    account: Snowflake,
    b64: String,
    payload: Vec<u8>,
    state: State,
}

/// `FavoritesState`: channels (1) and categories (2) by `position`, collapsed IDs (3),
/// `hide_muted_channels` (4).
fn parse(payload: &[u8]) -> State {
    let fs = synced::fields(payload).unwrap_or_default();
    let sub = |n: u32| fs.iter().filter(move |f| f.num == n && f.wire == 2).filter_map(|f| synced::fields(f.value));
    let mut list: Vec<(u64, Favorite)> = sub(1)
        .filter_map(|ch| {
            let get = |n: u32| ch.iter().find(|c| c.num == n);
            let channel: Snowflake = synced::str_of(get(1)?).parse().ok()?;
            let guild = get(2).map(synced::str_of).and_then(|g| g.parse().ok());
            let parent = get(3).map(synced::str_of);
            let nickname = get(5).map(synced::str_of);
            Some((get(4).map_or(0, synced::uint_of), Favorite { guild, channel, parent, nickname }))
        })
        .collect();
    list.sort_by_key(|(pos, _)| *pos);
    let mut cats: Vec<(u64, Category)> = sub(2)
        .filter_map(|c| {
            let get = |n: u32| c.iter().find(|x| x.num == n);
            Some((get(3).map_or(0, synced::uint_of), Category { id: synced::str_of(get(1)?), name: get(2).map(synced::str_of).unwrap_or_default() }))
        })
        .collect();
    cats.sort_by_key(|(pos, _)| *pos);
    let collapsed = fs.iter().filter(|f| f.num == 3 && f.wire == 2).map(synced::str_of).collect();
    let hide_muted = fs.iter().rev().find(|f| f.num == 4 && f.wire == 0).is_some_and(|f| synced::uint_of(f) != 0);
    State { list: list.into_iter().map(|(_, f)| f).collect(), cats: cats.into_iter().map(|(_, c)| c).collect(), collapsed, hide_muted }
}

/// `state` as a `FavoritesState`, keeping `remote`'s `muted` flag and unknown fields.
fn build(state: &State, remote: &[u8]) -> Vec<u8> {
    let fs = synced::fields(remote).unwrap_or_default();
    let mut out = Vec::new();
    for (i, f) in state.list.iter().enumerate() {
        let mut ch = Vec::new();
        synced::bytes(&mut ch, 1, f.channel.to_string().as_bytes());
        synced::bytes(&mut ch, 2, f.guild.map_or_else(|| "@me".to_string(), |g| g.to_string()).as_bytes());
        if let Some(p) = &f.parent {
            synced::bytes(&mut ch, 3, p.as_bytes());
        }
        if i > 0 {
            synced::uint(&mut ch, 4, i as u64);
        }
        if let Some(n) = &f.nickname {
            synced::bytes(&mut ch, 5, n.as_bytes());
        }
        synced::bytes(&mut out, 1, &ch);
    }
    for (i, c) in state.cats.iter().enumerate() {
        let mut cat = Vec::new();
        synced::bytes(&mut cat, 1, c.id.as_bytes());
        synced::bytes(&mut cat, 2, c.name.as_bytes());
        if i > 0 {
            synced::uint(&mut cat, 3, i as u64);
        }
        synced::bytes(&mut out, 2, &cat);
    }
    for id in &state.collapsed {
        synced::bytes(&mut out, 3, id.as_bytes());
    }
    if state.hide_muted {
        synced::uint(&mut out, 4, 1);
    }
    for f in fs.iter().filter(|f: &&Field| f.num >= 5) {
        out.extend_from_slice(f.raw);
    }
    out
}

/// Once a frame: takes a new snapshot's favorites, else sends local changes.
pub fn poll(store: &Store, st: &mut ShellState) -> bool {
    if store.me.is_none() {
        return false;
    }
    let b64 = &st.synced.view(&store.settings.synced_preferences);
    let me = st.account.me;
    if st.nav.synced_favorites.as_ref().is_none_or(|s| s.b64 != *b64 || s.account != me) {
        let payload = synced::get(b64, FIELD).unwrap_or_default();
        let state = parse(&payload);
        let first = st.nav.synced_favorites.as_ref().is_none_or(|s| s.account != me);
        let had = !payload.is_empty();
        st.nav.synced_favorites = Some(Seen { account: me, b64: b64.clone(), payload, state: state.clone() });
        // A snapshot that never had favorites doesn't wipe this device's on first sight; they're
        // uploaded below.
        if (had || !first) && State::of(&mine(st)) != state {
            update_mine(st, |a| state.put(a));
            return true;
        }
    }
    let Some(seen) = st.nav.synced_favorites.as_ref() else { return false };
    let local = State::of(&mine(st));
    if local == seen.state {
        return false;
    }
    let payload = build(&local, &seen.payload);
    st.synced.put(me, FIELD, (!payload.is_empty()).then_some(&payload[..]));
    let b64 = st.synced.view(&store.settings.synced_preferences);
    st.nav.synced_favorites = Some(Seen { account: me, b64, payload, state: local });
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_categories_nicknames_and_unknown_fields() {
        let fav = |g: Option<u64>, c: u64, parent: Option<&str>, nick: Option<&str>| Favorite { guild: g.map(Snowflake), channel: Snowflake(c), parent: parent.map(Into::into), nickname: nick.map(Into::into) };
        let mut remote = Vec::new();
        let mut ch = Vec::new();
        synced::bytes(&mut ch, 1, b"7");
        synced::bytes(&mut ch, 2, b"1");
        synced::bytes(&mut ch, 3, b"cat");
        synced::bytes(&mut ch, 5, b"nick");
        synced::bytes(&mut remote, 1, &ch);
        let mut cat = Vec::new();
        synced::bytes(&mut cat, 1, b"cat");
        synced::bytes(&mut cat, 2, b"Work");
        synced::bytes(&mut remote, 2, &cat);
        synced::bytes(&mut remote, 3, b"cat");
        synced::uint(&mut remote, 5, 1);
        let st = parse(&remote);
        assert_eq!(st.list, [fav(Some(1), 7, Some("cat"), Some("nick"))]);
        assert_eq!((st.cats[0].name.as_str(), st.collapsed.as_slice()), ("Work", &["cat".to_string()][..]));
        let next = State { list: vec![fav(None, 9, None, None), fav(Some(1), 7, Some("cat"), Some("nick"))], hide_muted: true, ..st };
        let out = build(&next, &remote);
        assert_eq!(parse(&out), next);
        assert!(synced::fields(&out).unwrap().iter().any(|f| f.num == 5), "muted kept");
        assert!(build(&State::default(), &[]).is_empty());
    }
}
