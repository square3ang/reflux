//! Quick switcher extras: confusable-skeleton matching for people and group DMs
//! (`SearchTextMatching.ts`, `ForwardDestinationSearch.ts` `searchUsers` / `searchGroupDMs`),
//! the "Go to message" / "Open link" rows (`QuickSwitcher.computeResultsForQuery`) and the other
//! signed-in accounts (`QuickSwitcherAccountResults.searchOtherAccounts`).

use std::collections::HashMap;
use std::sync::OnceLock;

use icu_normalizer::{ComposingNormalizerBorrowed, DecomposingNormalizerBorrowed};
use reflux_model::Snowflake;

use super::switcher::{Hit, Target, score};
use crate::i18n::{t, tf};

/// `UNICODE_CONFUSABLES`: `from:to` code points (hex, space separated), comma separated.
fn confusables() -> &'static HashMap<String, String> {
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAP.get_or_init(|| {
        let cps = |s: &str| s.split(' ').filter_map(|h| char::from_u32(u32::from_str_radix(h, 16).ok()?)).collect::<String>();
        include_str!("../../../../assets/search/unicode-confusables.txt")
            .split(',')
            .filter_map(|e| e.split_once(':'))
            .map(|(a, b)| (cps(a), cps(b)))
            .collect()
    })
}

fn nfd(s: &str) -> String {
    DecomposingNormalizerBorrowed::new_nfd().normalize(s).into_owned()
}

/// `stripCombiningMarks`.
fn strip_marks(s: &str) -> String {
    let bare: String = nfd(s).chars().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect();
    ComposingNormalizerBorrowed::new_nfc().normalize(&bare).into_owned()
}

/// `toConfusableSkeleton`.
fn skeleton(s: &str) -> String {
    let map = confusables();
    let mut buf = [0u8; 4];
    let out: String = nfd(s).chars().map(|c| map.get(&*c.encode_utf8(&mut buf)).cloned().unwrap_or_else(|| c.to_string())).collect();
    nfd(&out).to_lowercase()
}

/// `fuzzySearch`: the needle's characters in order.
fn fuzzy(needle: &str, hay: &str) -> bool {
    let (n, h) = (needle.chars().count(), hay.chars().count());
    if n > h {
        return false;
    }
    if n == h {
        return needle == hay;
    }
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|x| x == c))
}

/// `searchUsers`' field score: 10 prefix, 5 substring, 1 fuzzy (plain or by skeleton).
pub fn user_field(field: &str, q: &str) -> u32 {
    let (f, q) = (field.to_lowercase(), q.to_lowercase());
    if f.starts_with(&q) {
        return 10;
    }
    if f.contains(&q) {
        return 5;
    }
    let stripped = strip_marks(&f);
    u32::from(fuzzy(&q, &stripped) || fuzzy(&skeleton(&q), &skeleton(&stripped)))
}

/// `searchGroupDMs`: the folded name, or a member's name capped at 5.
pub fn group_dm(name: &str, members: &[String], q: &str) -> u32 {
    let fold = |s: &str| strip_marks(&skeleton(&s.to_lowercase()));
    let q = fold(q);
    members.iter().map(|m| score(&fold(m), &q).min(5)).fold(score(&fold(name), &q), u32::max)
}

/// `parseChannelUrl` on this instance, else a `fluxer:` deep link (`isAppDeepLink`).
pub fn link_hit(web_base: &str, query: &str) -> Option<Hit> {
    let q = query.trim();
    let base = web_base.trim_end_matches('/');
    let path = q.strip_prefix(base).filter(|_| !base.is_empty()).and_then(|r| r.strip_prefix("/channels/"));
    if let Some(path) = path {
        let id = |s: &str| s.parse::<u64>().ok().filter(|&n| n > 0).map(Snowflake);
        let parts: Vec<&str> = path.split(['?', '#']).next()?.split('/').filter(|s| !s.is_empty()).collect();
        let target = match parts.as_slice() {
            ["@me"] => Target::Jump(None, None, None),
            ["@me", c] => Target::Jump(None, Some(id(c)?), None),
            ["@me", c, m] => Target::Jump(None, Some(id(c)?), Some(id(m)?)),
            [g, c] => Target::Jump(Some(id(g)?), Some(id(c)?), None),
            [g, c, m] => Target::Jump(Some(id(g)?), Some(id(c)?), Some(id(m)?)),
            _ => return None,
        };
        return Some(Hit { target, title: t("Go to message").into(), meta: q.into(), score: u32::MAX, section: None });
    }
    crate::desktop::link::parse(q)?;
    Some(Hit { target: Target::DeepLink(q.into()), title: t("Open link").into(), meta: q.into(), score: u32::MAX, section: None })
}

/// `searchOtherAccounts` without the account engine's activity: (leading, trailing) account rows.
// shortcut: Reflux keeps no other account connected, so there are no unread/mention sections or
// channel rows of other accounts, and the row shows a letter, not the account's avatar.
pub fn accounts(list: &[crate::session::Account], q: &str) -> (Vec<Hit>, Vec<Hit>) {
    let q = q.trim();
    if q.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let host = |a: &crate::session::Account| a.instance.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/').to_string();
    let several = list.iter().map(host).collect::<std::collections::HashSet<_>>().len() > 1;
    // `scoreSearchTerm(…, fuzzy = false)`.
    let best = |vals: &[&str]| vals.iter().filter(|v| !v.is_empty()).map(|v| score(v, q)).filter(|&s| s > 1).max().unwrap_or(0);
    let (mut lead, mut trail) = (Vec::new(), Vec::new());
    for (i, a) in list.iter().enumerate() {
        let name_score = best(&[&a.display_name, &a.username]);
        let s = name_score.max(best(&[&host(a)]));
        if s == 0 {
            continue;
        }
        let title = if several { tf("{accountName} on {host}", &[("accountName", &a.display_name), ("host", &host(a))]) } else { a.display_name.clone() };
        let meta = if a.username.is_empty() { t("Switch to this account").into() } else { a.username.clone() };
        let hit = Hit { target: Target::Account(i), title, meta, score: s, section: None };
        if name_score >= 7 { lead.push(hit) } else { trail.push(hit) }
    }
    for v in [&mut lead, &mut trail] {
        v.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));
    }
    (lead, trail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeletons_and_marks_match_lookalikes() {
        // Cyrillic "а" and "е" read as Latin.
        assert_eq!(user_field("\u{430}d\u{435}l", "adel"), 1);
        assert_eq!(user_field("Zoë", "zoe"), 1);
        assert_eq!(user_field("adele", "ad"), 10);
        assert_eq!(user_field("xadele", "ad"), 5);
        assert_eq!(group_dm("Weekend", &["\u{430}da".into()], "ada"), 5, "a member's name is capped");
    }

    #[test]
    fn links_and_accounts() {
        let web = "https://web.fluxer.app";
        let h = link_hit(web, "https://web.fluxer.app/channels/1/2/3").unwrap();
        assert_eq!(h.target, Target::Jump(Some(Snowflake(1)), Some(Snowflake(2)), Some(Snowflake(3))));
        assert_eq!(link_hit(web, "https://web.fluxer.app/channels/@me").unwrap().target, Target::Jump(None, None, None));
        assert!(link_hit(web, "https://web.fluxer.app/channels/x/2").is_none());
        assert!(matches!(link_hit(web, "fluxer://channels/@me/5").unwrap().target, Target::DeepLink(_)));
        let acc = |n: &str, i: &str| crate::session::Account { display_name: n.into(), username: n.to_lowercase(), instance: i.into(), ..Default::default() };
        let (lead, trail) = accounts(&[acc("Ada", "https://a.example"), acc("Bob", "https://ada.example")], "ada");
        assert_eq!(lead.len(), 1);
        assert_eq!(lead[0].title, "Ada on a.example");
        assert_eq!(trail.len(), 1, "a host match trails");
    }
}
