//! Settings search (`SettingsSearchIndex.ts` + `AllSettingsRenderer.tsx`): the query is scored
//! against every tab's searchable items and sections (generated into [`search_index`]); the nav
//! keeps the matching tabs, and the content pane lists them best first as collapsible sections
//! (match count, the first three matched items as chips) with the tab itself inline.

use reflux_store::Store;
use rune::{El, col, icon, row, text};

use super::search_index::ITEMS;
use super::{SettingsUi, TABS, colors, tabs};
use crate::i18n::{t, tf};
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{mix, over};

/// One tab's matches.
pub(super) struct Hit {
    pub tab: &'static str,
    pub items: Vec<&'static str>,
    score: u32,
}

/// `normalizeSearchQuery` + `getQueryWords`.
fn words(q: &str) -> Vec<String> {
    q.to_lowercase().split_whitespace().map(str::to_string).collect()
}

/// `calculateTextMatchScore`.
fn text_score(text: &str, word: &str, exact: u32, prefix: u32, contains: u32) -> u32 {
    if !text.contains(word) {
        0
    } else if text == word {
        exact
    } else if text.starts_with(word) {
        prefix
    } else {
        contains
    }
}

/// `calculateMatchScore`: every word must hit the label, a keyword or the description.
pub(super) fn score(label: &str, keywords: &[&str], desc: &str, words: &[String]) -> u32 {
    let label = t(label).to_lowercase();
    let keywords: Vec<String> = keywords.iter().map(|k| t(k).to_lowercase()).collect();
    let desc = t(desc).to_lowercase();
    let mut total = 0;
    for w in words {
        let mut s = text_score(&label, w, 100, 50, 25);
        s += keywords.iter().map(|k| text_score(k, w, 80, 40, 20)).sum::<u32>();
        if !desc.is_empty() && desc.contains(w.as_str()) {
            s += 10;
        }
        if s == 0 {
            return 0;
        }
        total += s;
    }
    total
}

/// `searchSettings`: matches grouped by tab (the Account tab's nested ones under it), best first,
/// among the tabs `shown` allows.
pub(super) fn search(q: &str, shown: impl Fn(&str) -> bool) -> Vec<Hit> {
    let words = words(q);
    if words.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<Hit> = Vec::new();
    for &(_, tab, _, label, keywords, desc, advanced) in ITEMS {
        // `isAccountNestedSettingsTab`; advanced-only items live in the Advanced tab.
        let tab = match tab {
            _ if advanced => "advanced_settings",
            "authorized_apps" | "blocked_users" | "devices" => "account_security",
            t => t,
        };
        let Some(def) = TABS.iter().find(|d| d.id == tab).filter(|d| shown(d.id)) else { continue };
        let s = score(label, keywords, desc, &words);
        if s == 0 {
            continue;
        }
        match hits.iter_mut().find(|h| h.tab == def.id) {
            Some(h) => {
                h.items.push(label);
                h.score = h.score.max(s);
            }
            None => hits.push(Hit { tab: def.id, items: vec![label], score: s }),
        }
    }
    hits.sort_by_key(|h| std::cmp::Reverse(h.score));
    hits
}

/// `DesktopSettingsView`'s debounce: a query applies once typing pauses for this long; clearing
/// applies at once.
const DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

/// Once a frame: a settled new query expands every result again (`setExpandedTabs`) and loads
/// what the matched tabs list.
pub fn poll(store: &Store, st: &mut ShellState, tx: &mut rune::Tx) -> bool {
    let Some(u) = st.settings.as_mut() else { return false };
    let q = u.search.text.trim().to_string();
    if q == u.searched {
        u.typed = None;
        return false;
    }
    if !q.is_empty() {
        let now = std::time::Instant::now();
        let since = match &u.typed {
            Some((typed, at)) if *typed == q => *at,
            _ => {
                u.typed = Some((q, now));
                now
            }
        };
        let left = DEBOUNCE.saturating_sub(now - since);
        if !left.is_zero() {
            tx.wake_after(left);
            return false;
        }
    }
    let q = u.search.text.trim().to_string();
    u.typed = None;
    let hits = search(&q, |id| super::visible_id(id, store, st.premium));
    if let Some(u) = st.settings.as_mut() {
        u.searched = q;
        u.collapsed.clear();
    }
    for h in &hits {
        super::load_tab(st, h.tab);
    }
    true
}

/// The content pane while searching.
pub(super) fn results<S: 'static>(store: &Store, st: &ShellState, ui: &SettingsUi, hits: &[Hit], p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    if hits.is_empty() {
        // `.emptyState`.
        return col()
            .key("none")
            .w_full()
            .min_h(128.0)
            .py(16.0)
            .center()
            .child(
                col()
                    .max_w(320.0)
                    .items_center()
                    .gap(8.0)
                    .child(text(t("No settings found")).font(font(18.0, 600.0).lh(28.0)).color(p.text_primary))
                    .child(
                        text(t("Try searching for something like \"theme\", \"notifications\", or \"privacy\""))
                            .font(font(14.0, 400.0).lh(21.0))
                            .color(p.text_secondary)
                            .align(rune::Align::Center)
                            .wrap(),
                    ),
            );
    }
    let c = colors(p);
    let total: usize = hits.iter().map(|h| h.items.len()).sum();
    let header = tf(
        "Found {resultCount, plural, one {# result} other {# results}} in {categoryCount, plural, one {# category} other {# categories}}",
        &[("resultCount", &total.to_string()), ("categoryCount", &hits.len().to_string())],
    );
    let mut out = col()
        .w_full()
        .gap(16.0)
        .child(
            col()
                .w_full()
                .pt(8.0)
                .pb(12.0)
                .mb(8.0)
                .child(text(header).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary))
                .child(rune::el().pin(None, Some(0.0), Some(0.0), Some(0.0)).h(1.0).fill(c.border)),
        );
    for h in hits {
        let Some(def) = TABS.iter().find(|d| d.id == h.tab) else { continue };
        let expanded = !ui.collapsed.contains(&h.tab);
        let tab = h.tab;
        let toggle = move |s: &mut S| {
            if let Some(u) = acc(s).settings.as_mut() {
                match u.collapsed.iter().position(|&t| t == tab) {
                    Some(i) => drop(u.collapsed.remove(i)),
                    None => u.collapsed.push(tab),
                }
            }
        };
        let hover_border = mix(c.border, p.text_tertiary, 72.0);
        let count = row()
            .px(8.0)
            .py(4.0)
            .radius(999.0)
            .fill(c.hover)
            .border(1.0, over(c.border, c.hover))
            .child(text(tf("{matchCount, plural, one {# match} other {# matches}}", &[("matchCount", &h.items.len().to_string())])).font(font(12.0, 500.0).lh(16.0)).color(p.text_secondary));
        let chip = |label: &str| {
            row()
                .key(label.to_string())
                .px(8.0)
                .py(4.0)
                .radius(6.0)
                .fill(c.selected)
                .border(1.0, over(hover_border, c.selected))
                .child(text(t(label).to_string()).font(font(12.0, 500.0).lh(16.0)).color(p.text_primary))
        };
        let mut chips = row().flex_wrap().gaps(6.0, 6.0).mt(12.0).children(h.items.iter().take(3).map(|l| chip(l)));
        if h.items.len() > 3 {
            chips = chips.child(row().key("more").px(4.0).py(4.0).child(text(format!("+{}", h.items.len() - 3)).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary)));
        }
        let head = col()
            .w_full()
            .px(12.0)
            .py(8.0)
            .radius(8.0)
            .fill(c.surface)
            .border(1.0, over(c.border, c.surface))
            .hover(c.hover)
            .cursor(rune::Cursor::Pointer)
            .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
            .expanded(expanded)
            .label(t(def.label).to_string())
            .key(format!("settings-hit-{tab}"))
            .on_press(toggle)
            .child(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(icon(def.icon, 20.0).color(p.text_primary))
                    .child(text(t(def.label).to_string()).font(font(16.0, 600.0).lh(24.0)).color(p.text_primary).grow(1.0).shrink().truncate())
                    .child(count)
                    .child(icon(if expanded { &icons::CARET_DOWN_BOLD } else { &icons::CARET_RIGHT_BOLD }, 16.0).color(p.text_tertiary)),
            )
            .child(chips);
        let mut section = col().key(format!("hit-{tab}")).w_full().child(head);
        if expanded {
            // `data-settings-section`: the query is marked inside the inline tab.
            crate::ui::mark::set(&ui.searched);
            let body = tabs::body(store, st, ui, tab, p, acc, layer);
            crate::ui::mark::clear();
            section = section.child(col().w_full().gap(8.0).py(16.0).child(body));
        }
        out = out.child(section);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_like_fluxer_and_groups_by_tab() {
        assert_eq!(text_score("theme", "theme", 100, 50, 25), 100);
        assert_eq!(text_score("themes", "theme", 100, 50, 25), 50);
        assert_eq!(score("Base theme", &["Theme", "Dark"], "", &words("dark")), 80);
        assert_eq!(score("Base theme", &["Theme"], "", &words("theme nope")), 0, "every word must match");
        let hits = search("blocked", |_| true);
        assert_eq!(hits.first().map(|h| h.tab), Some("account_security"), "blocked users live in the Account tab");
        assert!(search("push to talk", |_| true).iter().any(|h| h.tab == "keybinds"));
        assert!(search("zzzz", |_| true).is_empty());
    }

    #[test]
    fn provider_items_resolve_to_advanced() {
        for (q, label) in [("search engine", "Search engines"), ("translator", "Translators")] {
            let hits = search(q, |_| true);
            let advanced = hits.iter().find(|h| h.tab == "advanced_settings").expect("an Advanced hit");
            assert!(advanced.items.contains(&label), "{q} -> {:?}", advanced.items);
        }
    }
}
