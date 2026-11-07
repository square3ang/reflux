//! The Ctrl+K quick switcher (`QuickSwitcherModal`, `search/state/QuickSwitcher.tsx`): fuzzy over
//! communities, channels, DMs and people, with `@` `#` `!` `*` prefixes to narrow it.
// shortcut: no message links, other accounts or member search over the network.

use reflux_model::{ChannelType, NotificationLevel, RelationshipType, Snowflake};
use reflux_store::Store;
use rune::{Color, El, Key, ScrollSpec, TextEdit, col, el, icon, row, text, text_input};

use super::{Intent, ShellAccess, ShellState, avatar, channel_icon};
use crate::theme::{Palette, font};
use crate::ui::{over, rgba, scroller};
use crate::i18n::t;

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
    /// The empty query's section header this hit starts or continues.
    pub section: Option<&'static str>,
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

/// `HISTORY_LIMIT`, `PREVIOUS_CHANNELS_WITH_SECTIONS`, `PREVIOUS_CHANNELS_ALONE`.
const HISTORY: usize = 8;

/// `generateDefaultResults`: up to 7 previous channels (3 when other sections follow), then
/// channels with mentions, then the open community's unread channels; the first 3 previous
/// ones and the open channel aren't repeated.
fn default_results(store: &Store, recent: &[Snowflake], current: Option<Snowflake>, guild: Option<Snowflake>) -> Vec<Hit> {
    let me = store.my_id();
    let hit = |c: &reflux_model::Channel, section: &'static str| {
        let title = c.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| c.recipients.iter().filter(|u| u.id != me).map(|u| store.display_name(None, u.id)).collect::<Vec<_>>().join(", "));
        let meta = c.guild_id.and_then(|g| store.guilds.get(&g)?.info.as_ref()).map(|i| i.name.clone()).unwrap_or_default();
        Hit { target: Target::Channel(c.guild_id, c.id, c.kind == ChannelType::GUILD_VOICE), title, meta, score: 1, section: Some(section) }
    };
    // `collectChannelResults`: known, visible and not seen yet.
    let collect = |ids: Vec<Snowflake>, seen: &mut Vec<Snowflake>, section| {
        let mut out = Vec::new();
        for id in ids {
            let Some(c) = store.channels.get(&id).filter(|c| !seen.contains(&id) && (c.guild_id.is_none() || store.can_view(id))) else { continue };
            seen.push(id);
            out.push(hit(c, section));
        }
        out
    };
    let mut history: Vec<Snowflake> = Vec::new();
    for &c in recent {
        if Some(c) != current && !history.contains(&c) {
            history.push(c);
        }
    }
    history.truncate(HISTORY - 1);
    let previous = collect(history, &mut Vec::new(), "Previous channels");
    let mut seen: Vec<Snowflake> = previous.iter().take(3).filter_map(|h| if let Target::Channel(_, c, _) = h.target { Some(c) } else { None }).chain(current).collect();
    // Newest mention first (`[...mentionChannelIds].reverse()`).
    let mentioned = store.mention_order.iter().rev().copied().filter(|&c| store.mention_count(c) > 0).collect();
    let mut sections = collect(mentioned, &mut seen, "Mentions");
    if let Some(g) = guild {
        // `isUnreadInCurrentCommunity`: text and announcement channels, not muted, unread, in a
        // community whose (locked) unread badge level is all messages.
        let s = store.guild_settings.get(&Some(g));
        let level = s.map(|s| s.message_notifications).filter(|l| *l != NotificationLevel::INHERIT).unwrap_or_else(|| {
            NotificationLevel(store.guilds.get(&g).and_then(|g| g.info.as_ref()).map_or(0, |i| i.default_message_notifications))
        });
        let all = level == NotificationLevel::ALL_MESSAGES && !s.is_some_and(|s| reflux_store::mute_active(s.muted, s.mute_config.as_ref(), crate::app::now_ms()));
        if all {
            let ids = store.channel_groups(g).iter().flat_map(|grp| grp.text.iter()).filter(|c| matches!(c.kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT) && !store.is_muted(c) && store.has_unread(c.id)).map(|c| c.id).collect();
            sections.extend(collect(ids, &mut seen, "Unread channels"));
        }
    }
    let cap = if sections.is_empty() { 7 } else { 3 };
    previous.into_iter().take(cap).chain(sections).collect()
}

/// Results for a query, best first. `recent`: channels visited, newest first; `current`: the open one.
pub fn results(store: &Store, query: &str, recent: &[Snowflake], current: Option<Snowflake>, guild: Option<Snowflake>) -> Vec<Hit> {
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
            hits.push(Hit { target, title, meta, score: s, section: None });
        }
    };
    if q.is_empty() && mode.is_none() {
        return default_results(store, recent, current, guild);
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
    let hits = results(store, &sw.query.text, &st.prefs.recent, st.channel, st.guild);
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
        .label(t("Quick switcher search"))
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
                .child(text(t("No matches found")).font(font(14.0, 500.0).lh(20.0)).color(p.text_tertiary))
                .child(text(t("Try a different name or use @ / # / ! / * prefixes to filter results.")).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).mt(4.0)),
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
        // `.sectionHeader`: 11/600 tertiary, padding 4 8, 2 below.
        if let Some(sec) = h.section.filter(|s| i == 0 || hits[i - 1].section != Some(s)) {
            list = list.child(text(t(sec)).font(font(11.0, 600.0).lh(16.0)).color(p.text_tertiary).py(4.0).px(8.0).mb(2.0).mt(if i > 0 { 8.0 } else { 0.0 }));
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
        .child(code("@", t("People")))
        .child(code("#", t("Text channels")))
        .child(code("!", t("Voice channels")))
        .child(code("*", t("Communities")))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(1.0).fill(p.background_modifier_accent));
    let card = col()
        .w(576.0)
        .radius(12.0)
        .clip_rounded(12.0)
        .fill(p.background_secondary)
        .border(1.0, over(p.background_header_secondary, p.background_secondary))
        .box_shadow(rune::Shadow { x: 0.0, y: 20.0, blur: 25.0, spread: -5.0, color: rgba(0, 0, 0, 0.3), inset: false })
        .role("dialog")
        .label(t("Quick switcher"))
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
        let all = results(&d.store, "gen", &[], None, Some(Snowflake(2000)));
        assert!(matches!(all[0].target, Target::Channel(Some(_), _, false)), "{all:?}");
        assert!(results(&d.store, "#lou", &[], None, Some(Snowflake(2000))).is_empty(), "Lounge is a voice channel");
        assert!(matches!(results(&d.store, "!lou", &[], None, Some(Snowflake(2000)))[0].target, Target::Channel(_, _, true)));
        assert!(matches!(results(&d.store, "*rust", &[], None, Some(Snowflake(2000)))[0].target, Target::Guild(_)));
        assert!(results(&d.store, "@ada", &[], None, Some(Snowflake(2000))).iter().all(|h| matches!(h.target, Target::User(_))));
        assert!(!results(&d.store, "", &[], None, Some(Snowflake(2000))).is_empty(), "unread channels by default");
        let general = Snowflake(2201);
        let r = results(&d.store, "", &[general, Snowflake(2202)], Some(general), Some(Snowflake(2000)));
        assert_eq!(r[0].target, Target::Channel(Some(Snowflake(2000)), Snowflake(2202), false), "the open channel isn't a previous one");
        assert_eq!(r[0].section, Some("Previous channels"));
        assert!(r.iter().filter(|h| h.section == Some("Unread channels")).all(|h| h.target != r[0].target), "no repeats");
    }
}
