//! Nagbar dismissals through `synced_preferences.nagbars` (`Nagbar.ts` `makeSyncedField`):
//! the `dismissed_nagbars` keys Fluxer syncs are replaced by another device's snapshot, and a
//! local change is sent back. Other keys (notices, maintenance) stay on this device, like
//! Fluxer's; fields this client has no bar for are kept as the snapshot has them.

use reflux_model::Snowflake;
use reflux_store::Store;

use crate::shell::ShellState;
use crate::synced;

/// `SyncedPreferences.nagbars`.
const FIELD: u32 = 60;

/// `NagbarDismissals` bool fields and their `dismissed_nagbars` keys.
const FLAGS: &[(u32, &str)] = &[(4, "desktop-notification"), (5, "premium-grace"), (6, "premium-expired"), (7, "premium-onboarding"), (11, "guild-membership-cta"), (12, "visionary-mfa")];
/// `map<string, bool>` fields, by guild: the key prefix.
const MAPS: &[(u32, &str)] = &[(16, "invites-disabled:"), (17, "guild-mfa:")];

fn synced_key(k: &str) -> bool {
    FLAGS.iter().any(|f| f.1 == k) || MAPS.iter().any(|m| k.starts_with(m.1))
}

/// The synced keys, sorted.
fn local(st: &ShellState) -> Vec<String> {
    let mut v: Vec<String> = st.prefs.dismissed_nagbars.iter().filter(|k| synced_key(k)).cloned().collect();
    v.sort();
    v.dedup();
    v
}

#[derive(Clone, Debug, Default)]
pub struct Seen {
    account: Snowflake,
    b64: String,
    payload: Vec<u8>,
    keys: Vec<String>,
}

fn parse(payload: &[u8]) -> Vec<String> {
    let fs = synced::fields(payload).unwrap_or_default();
    let mut out = Vec::new();
    for f in &fs {
        if let Some((_, k)) = FLAGS.iter().find(|x| x.0 == f.num && f.wire == 0) {
            out.retain(|x| x != k);
            if synced::uint_of(f) != 0 {
                out.push(k.to_string());
            }
        } else if let Some((_, pre)) = MAPS.iter().find(|x| x.0 == f.num && f.wire == 2) {
            let Some(e) = synced::fields(f.value) else { continue };
            let id = e.iter().rev().find(|x| x.num == 1).map(synced::str_of).unwrap_or_default();
            let k = format!("{pre}{id}");
            out.retain(|x| *x != k);
            if e.iter().rev().find(|x| x.num == 2).is_some_and(|x| synced::uint_of(x) != 0) {
                out.push(k);
            }
        }
    }
    out.sort();
    out
}

/// `keys` as `NagbarDismissals`, keeping `remote`'s other fields.
fn build(keys: &[String], remote: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for f in synced::fields(remote).unwrap_or_default() {
        if !FLAGS.iter().chain(MAPS).any(|x| x.0 == f.num) {
            out.extend_from_slice(f.raw);
        }
    }
    for (n, k) in FLAGS {
        if keys.iter().any(|x| x == k) {
            synced::uint(&mut out, *n, 1);
        }
    }
    for (n, pre) in MAPS {
        for id in keys.iter().filter_map(|k| k.strip_prefix(pre)) {
            let mut e = Vec::new();
            synced::bytes(&mut e, 1, id.as_bytes());
            synced::uint(&mut e, 2, 1);
            synced::bytes(&mut out, *n, &e);
        }
    }
    out
}

/// Once a frame: takes a new snapshot's dismissals, else sends local changes.
pub fn poll(store: &Store, st: &mut ShellState) -> bool {
    if store.me.is_none() {
        return false;
    }
    let b64 = st.synced.view(&store.settings.synced_preferences);
    let me = st.account.me;
    if st.nagbar_sync.as_ref().is_none_or(|s| s.b64 != b64 || s.account != me) {
        let payload = synced::get(&b64, FIELD).unwrap_or_default();
        let keys = parse(&payload);
        let first = st.nagbar_sync.as_ref().is_none_or(|s| s.account != me);
        let had = !payload.is_empty();
        st.nagbar_sync = Some(Seen { account: me, b64, payload, keys: keys.clone() });
        // A snapshot without dismissals doesn't undo this device's on first sight; they're sent below.
        if (had || !first) && local(st) != keys {
            let mut p = st.prefs.clone();
            p.dismissed_nagbars.retain(|k| !synced_key(k));
            p.dismissed_nagbars.extend(keys);
            st.prefs = p.clone();
            st.intents.push(crate::shell::Intent::Prefs(Box::new(p)));
            return true;
        }
    }
    let Some(seen) = st.nagbar_sync.as_ref() else { return false };
    let keys = local(st);
    if keys == seen.keys {
        return false;
    }
    let payload = build(&keys, &seen.payload);
    st.synced.put(me, FIELD, (!payload.is_empty()).then_some(&payload[..]));
    let b64 = st.synced.view(&store.settings.synced_preferences);
    st.nagbar_sync = Some(Seen { account: me, b64, payload, keys });
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_flags_maps_and_unknown_fields() {
        let mut remote = Vec::new();
        synced::uint(&mut remote, 1, 1); // ios_install: no bar here, kept
        synced::uint(&mut remote, 12, 1);
        let mut e = Vec::new();
        synced::bytes(&mut e, 1, b"42");
        synced::uint(&mut e, 2, 1);
        synced::bytes(&mut remote, 16, &e);
        assert_eq!(parse(&remote), ["invites-disabled:42", "visionary-mfa"]);
        let keys = vec!["guild-mfa:7".to_string(), "invites-disabled:42".into()];
        let out = build(&keys, &remote);
        assert_eq!(parse(&out), keys);
        assert!(synced::fields(&out).unwrap().iter().any(|f| f.num == 1), "unknown kept");
        assert!(synced_key("guild-membership-cta") && !synced_key("notice:1"));
    }
}
