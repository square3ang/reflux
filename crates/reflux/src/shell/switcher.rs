//! The Ctrl+K quick switcher (`QuickSwitcherModal`, `search/state/QuickSwitcher.tsx`): fuzzy over
//! communities, channels, DMs and people, with `@` `#` `!` `*` prefixes to narrow it.
// shortcut: the empty query lists unread channels and DMs, not Fluxer's recents; no message
// links, other accounts or member search over the network.

use reflux_model::{ChannelType, RelationshipType, Snowflake};
use reflux_store::Store;
use rune::{Color, El, Key, ScrollSpec, TextEdit, col, el, icon, row, text, text_input};

use super::{Intent, ShellAccess, ShellState, avatar, channel_icon};
use crate::theme::{Palette, font};
use crate::ui::{over, rgba, scroller};

#[derive(Debug, Default)]
pub struct Switcher {
    pub query: TextEdit,
    pub selected: usize,
}

impl Switcher {
    pub fn new() -> Self {
        let mut query = TextEdit::new("");
        query.focused = true;
        Switcher { query, selected: 0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Guild(Snowflake),
    /// (guild, channel); voice channels join.
    Channel(Option<Snowflake>, Snowflake, bool),
    User(Snowflake),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub target: Target,
    pub title: String,
    /// The guild name, or a username.
    pub meta: String,
    pub score: u32,
}

/// `scoreSearchTerm`: 10 exact, 7 prefix, 5 substring, 3 every word, 1 in order, 0 none.
pub fn score(value: &str, q: &str) -> u32 {
    let (v, q) = (value.to_lowercase(), q.to_lowercase());
    if v.starts_with(&q) {
        return if v == q { 10 } else { 7 };
    }
    if v.contains(&q) {
        return 5;
    }
    if q.split([' ', ',']).filter(|w| !w.is_empty()).all(|w| v.contains(w)) {
        return 3;
    }
    let mut it = v.chars();
    if q.chars().all(|c| it.any(|x| x == c)) { 1 } else { 0 }
}

/// Results for a query, best first.
pub fn results(store: &Store, query: &str) -> Vec<Hit> {
    let q = query.trim();
    let (mode, q) = match q.chars().next() {
        Some(c @ ('@' | '#' | '!' | '*')) => (Some(c), q[1..].trim()),
        _ => (None, q),
    };
    let me = store.my_id();
    let want = |m: char| mode.is_none_or(|x| x == m);
    let guild_name = |g: Snowflake| store.guilds.get(&g).and_then(|g| g.info.as_ref()).map(|i| i.name.clone()).unwrap_or_default();
    let mut hits = Vec::new();
    let mut push = |target, title: String, meta: String, also: &str| {
        let s = if q.is_empty() { 1 } else { score(&title, q).max(score(also, q)) };
        if s > 0 {
            hits.push(Hit { target, title, meta, score: s });
        }
    };
    if q.is_empty() && mode.is_none() {
        // Unread channels and DMs.
        for c in store.private_channels() {
            if store.has_unread(c.id) {
                let name = c.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| c.recipients.iter().filter(|u| u.id != me).map(|u| store.display_name(None, u.id)).collect::<Vec<_>>().join(", "));
                push(Target::Channel(None, c.id, false), name, String::new(), "");
            }
        }
        for g in store.guilds.keys() {
            for grp in store.channel_groups(*g) {
                for c in grp.text.iter().filter(|c| store.has_unread(c.id) && !store.is_muted(c)) {
                    push(Target::Channel(Some(*g), c.id, false), c.name.clone().unwrap_or_default(), guild_name(*g), "");
                }
            }
        }
        return hits;
    }
    if want('@') {
        let mut seen = std::collections::HashSet::new();
        let friends = store.relationships.values().filter(|r| r.kind == RelationshipType::FRIEND).map(|r| r.id);
        let dm_users = store.private_channels().into_iter().filter(|c| c.kind == ChannelType::DM).flat_map(|c| c.recipients.iter().map(|u| u.id)).collect::<Vec<_>>();
        for u in friends.chain(dm_users).filter(|&u| u != me) {
            if seen.insert(u) {
                let name = store.users.get(&u).map(|x| x.username.clone()).unwrap_or_default();
                push(Target::User(u), store.display_name(None, u), name.clone(), &name);
            }
        }
    }
    if mode.is_none() {
        for c in store.private_channels().into_iter().filter(|c| c.kind == ChannelType::GROUP_DM) {
            let name = c.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| c.recipients.iter().map(|u| store.display_name(None, u.id)).collect::<Vec<_>>().join(", "));
            push(Target::Channel(None, c.id, false), name, String::new(), "");
        }
    }
    for g in store.guilds.keys() {
        if want('*') {
            push(Target::Guild(*g), guild_name(*g), String::new(), "");
        }
        for grp in store.channel_groups(*g) {
            if want('#') {
                for c in grp.text.iter().filter(|c| c.kind != ChannelType::GUILD_LINK) {
                    push(Target::Channel(Some(*g), c.id, false), c.name.clone().unwrap_or_default(), guild_name(*g), "");
                }
            }
            if want('!') {
                for c in &grp.voice {
                    push(Target::Channel(Some(*g), c.id, true), c.name.clone().unwrap_or_default(), guild_name(*g), "");
                }
            }
        }
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())));
    hits.truncate(50);
    hits
}

/// Goes to a result and closes the switcher.
fn choose(store_first: impl Fn(Snowflake) -> Option<Snowflake>, st: &mut ShellState, t: Target) {
    st.switcher = None;
    match t {
        Target::Guild(g) => st.select_guild(Some(g), store_first(g)),
        Target::Channel(g, c, false) => st.select_guild(g, Some(c)),
        Target::Channel(g, c, true) => st.intents.push(Intent::JoinVoice { guild: g, channel: c }),
        Target::User(u) => st.intents.push(Intent::OpenDm(u)),
    }
}

pub fn view<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let sw = st.switcher.as_ref()?;
    let hits = results(store, &sw.query.text);
    let selected = sw.selected.min(hits.len().saturating_sub(1));
    let firsts: std::collections::HashMap<Snowflake, Option<Snowflake>> = store.guilds.keys().map(|g| (*g, super::first_channel(store, *g))).collect();
    let pick = {
        let (hits, firsts) = (hits.clone(), firsts.clone());
        move |s: &mut S, i: usize| {
            if let Some(h) = hits.get(i) {
                let f = firsts.clone();
                choose(move |g| f.get(&g).copied().flatten(), acc(s), h.target);
            }
        }
    };
    let n = hits.len();
    let field = text_input(&sw.query, font(16.0, 400.0).lh(22.0))
        .key("switcher-field")
        .grow(1.0)
        .single_line()
        .color(p.text_primary)
        .placeholder("Where would you like to go?", p.text_tertiary)
        .caret(p.text_primary, p.text_selection)
        .label("Quick switcher search")
        .on_edit(move |s: &mut S| -> &mut TextEdit {
            let st = acc(s);
            let sw = st.switcher.get_or_insert_with(Switcher::new);
            sw.selected = 0;
            &mut sw.query
        })
        .on_input_key(move |s: &mut S, k, _| {
            let st = acc(s);
            let Some(sw) = st.switcher.as_mut() else { return false };
            match k.key {
                Key::Escape => st.switcher = None,
                Key::Down if n > 0 => sw.selected = (sw.selected + 1) % n,
                Key::Up if n > 0 => sw.selected = (sw.selected + n - 1) % n,
                _ => return false,
            }
            true
        })
        .on_submit({
            let pick = pick.clone();
            move |s: &mut S, _| pick(s, selected)
        });
    let input = row()
        .w_full()
        .h(44.0)
        .px(12.0)
        .center_y()
        .radius(8.0)
        .fill(if crate::ui::is_light(p) { p.background_secondary } else { p.background_primary })
        .border(1.0, over(p.background_modifier_accent, p.background_primary))
        .child(field);
    let mut list = col().w_full().padding(4.0);
    if hits.is_empty() {
        list = list.child(
            col()
                .w_full()
                .h(312.0)
                .center()
                .child(text("No matches found").font(font(14.0, 500.0).lh(20.0)).color(p.text_tertiary))
                .child(text("Try a different name or use @ / # / ! / * prefixes to filter results.").font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).mt(4.0)),
        );
    }
    for (i, h) in hits.iter().enumerate() {
        let active = i == selected;
        let glyph = match h.target {
            Target::User(u) => avatar(store, st, u, 24.0, None, p.background_secondary, p),
            Target::Guild(g) => {
                let name = store.guilds.get(&g).and_then(|x| x.info.as_ref()).map(|i| i.name.clone()).unwrap_or_default();
                row().size(24.0, 24.0).center().radius(12.0).fill(p.background_primary).child(text(super::acronym(&name)).font(font(9.0, 600.0)).color(p.text_primary))
            }
            Target::Channel(g, c, _) => {
                let kind = store.channels.get(&c).map(|c| c.kind).unwrap_or_default();
                match (g, store.channels.get(&c)) {
                    (None, Some(ch)) if kind == ChannelType::DM => avatar(store, st, ch.recipients.iter().find(|u| u.id != store.my_id()).map(|u| u.id).unwrap_or_default(), 24.0, None, p.background_secondary, p),
                    (None, Some(ch)) => super::group_avatar(store, st, ch, 24.0, p.background_secondary, p),
                    _ => el().padding(2.0).child(icon(channel_icon(kind), 20.0).color(if active { p.text_primary } else { p.text_primary_muted })),
                }
            }
        };
        let title_color = if active { p.text_primary } else { p.text_secondary };
        let pick = pick.clone();
        let mut r = row()
            .w_full()
            .h(36.0)
            .px(8.0)
            .gap(10.0)
            .center_y()
            .radius(4.0)
            .cursor(rune::Cursor::Pointer)
            .role("option")
            .key(format!("switcher-{i}"))
            .child(glyph)
            .child(text(h.title.clone()).font(font(13.0, if active { 700.0 } else { 600.0 }).lh(16.0)).color(title_color).truncate().shrink().min_w(0.0))
            .child(el().grow(1.0))
            .child(text(h.meta.clone()).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).truncate().max_w(160.0))
            .on_hover_change(move |s: &mut S, inside, _| {
                if inside && let Some(sw) = acc(s).switcher.as_mut() {
                    sw.selected = i;
                }
            })
            .on_press(move |s: &mut S| pick(s, i));
        if active {
            r = r.fill(if crate::ui::is_light(p) { p.background_modifier_hover } else { p.background_modifier_accent });
        }
        if i > 0 {
            r = r.mt(2.0);
        }
        list = list.child(r);
    }
    let code = |sym: &str, label: &str| {
        row()
            .gap(4.0)
            .center_y()
            .child(row().h(20.0).min_w(20.0).px(4.0).center().radius(4.0).fill(p.background_primary).border(1.0, p.background_modifier_accent).child(text(sym.to_string()).font(font(11.0, 400.0).lh(20.0)).color(p.text_tertiary)))
            .child(text(label.to_string()).font(font(11.0, 400.0)).color(p.text_primary_muted))
    };
    let footer = row()
        .w_full()
        .py(8.0)
        .px(12.0)
        .gap(12.0)
        .center_y()
        .fill(p.background_secondary)
        .child(code("@", "People"))
        .child(code("#", "Text channels"))
        .child(code("!", "Voice channels"))
        .child(code("*", "Communities"))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(1.0).fill(p.background_modifier_accent));
    let card = col()
        .w(576.0)
        .radius(12.0)
        .clip_rounded(12.0)
        .fill(p.background_secondary)
        .border(1.0, over(p.background_header_secondary, p.background_secondary))
        .box_shadow(rune::Shadow { x: 0.0, y: 20.0, blur: 25.0, spread: -5.0, color: rgba(0, 0, 0, 0.3), inset: false })
        .role("dialog")
        .label("Quick switcher")
        .child(row().w_full().padding(12.0).child(input))
        .child(col().w_full().h(320.0).child(scroller(col().w_full().h_full().scroll(ScrollSpec { stick_bottom: false }).child(list), false, p)))
        .child(footer)
        .modal(true, move |s: &mut S| acc(s).switcher = None);
    Some(
        el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(Color::BLACK).opacity(0.85))
            .child(col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).px(16.0).pt(120.0).items_center().child(card)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_like_fluxer() {
        assert_eq!(score("general", "general"), 10);
        assert_eq!(score("general", "gen"), 7);
        assert_eq!(score("off-topic", "topic"), 5);
        assert_eq!(score("dev talk", "talk dev"), 3);
        assert_eq!(score("announcements", "anmt"), 1);
        assert_eq!(score("rules", "xyz"), 0);
    }

    #[test]
    fn prefixes_narrow_the_demo() {
        let d = crate::shell::demo::Demo::new();
        let all = results(&d.store, "gen");
        assert!(matches!(all[0].target, Target::Channel(Some(_), _, false)), "{all:?}");
        assert!(results(&d.store, "#lou").is_empty(), "Lounge is a voice channel");
        assert!(matches!(results(&d.store, "!lou")[0].target, Target::Channel(_, _, true)));
        assert!(matches!(results(&d.store, "*rust")[0].target, Target::Guild(_)));
        assert!(results(&d.store, "@ada").iter().all(|h| matches!(h.target, Target::User(_))));
        assert!(!results(&d.store, "").is_empty(), "unread channels by default");
    }
}
