//! Message search: the header's `MessageSearchBar` (with its filter options while empty), and
//! the `ChannelSearchResults` panel beside the chat, whose results jump to their message.
//! Arrow keys move through the open popout's rows and Enter picks one
//! (`useMessageSearchAutocomplete`): value lists start on their first row, the filter and
//! history list only once an arrow was pressed.
// shortcut: no scope menu, search chips or date pickers for before:/during:/after:.

use reflux_model::Message;
use reflux_store::Store;
use rune::{El, Key, ScrollSpec, TextEdit, col, el, icon, row, text, text_input};
use serde_json::json;

use super::markdown::Md;
use super::search_query::{self, Ctx, OPTIONS};
use super::{Intent, ShellAccess, ShellState};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::overlay::{anchor, popout_shadow};
use crate::ui::{Button, ButtonSize, ButtonVariant, over, scroller, spinner};
use crate::i18n::t;

const PER_PAGE: u32 = 25;

/// What the panel shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Results {
    #[default]
    Loading,
    /// The instance is still indexing the channel.
    Indexing,
    Failed,
    Done { messages: Vec<Message>, total: u32 },
}

#[derive(Debug, Default)]
pub struct Search {
    pub query: TextEdit,
    /// The results panel is open; `None` = closed.
    pub results: Option<Results>,
    pub page: u32,
    /// Oldest first (`order:asc`).
    pub oldest: bool,
    /// The search the results belong to (to page and re-sort it).
    pub ran: String,
    /// The popout row the arrow keys moved to, with the query it was for.
    pub nav: Option<(String, usize)>,
}

/// A row of the bar's popout.
#[derive(Clone, Debug, PartialEq)]
enum Pick {
    /// A filter key to type (`from:`).
    Filter(&'static str),
    /// A value replacing the token being typed.
    Value(String),
    /// A recent search to run again.
    Recent(String),
}

fn apply(st: &mut ShellState, pick: &Pick) {
    st.search.nav = None;
    match pick {
        Pick::Filter(key) => {
            st.search.query.insert(key);
            st.search.query.focused = true;
        }
        Pick::Value(token) => {
            let q = &mut st.search.query;
            let keep = q.text.rsplit_once(' ').map_or(String::new(), |(head, _)| format!("{head} "));
            *q = TextEdit::new(format!("{keep}{token} "));
            q.focused = true;
        }
        Pick::Recent(text) => {
            st.search.query = TextEdit::new(text.clone());
            st.search.page = 1;
            st.search_requested = true;
        }
    }
}

/// The highlighted row: the arrowed one for this query, else the first value.
fn selected(s: &Search, values: bool) -> Option<usize> {
    match &s.nav {
        Some((q, i)) if *q == s.query.text => Some(*i),
        _ => values.then_some(0),
    }
}

/// ArrowDown/ArrowUp wrap through `n` rows (Up from nothing lands on the last).
fn step(cur: Option<usize>, n: usize, down: bool) -> usize {
    match (cur, down) {
        (None, true) => 0,
        (None, false) => n - 1,
        (Some(i), true) => (i + 1) % n,
        (Some(i), false) => (i + n - 1) % n,
    }
}

/// Builds the request for `query` in the open channel's guild (or the DM itself) and opens the
/// panel. Empty and unparseable queries search nothing.
pub fn run(store: &Store, st: &mut ShellState, page: u32) {
    let Some(ch) = st.channel else { return };
    let q = st.search.query.text.trim().to_string();
    let guild = store.channels.get(&ch).and_then(|c| c.guild_id);
    let ctx = Ctx { store, guild, now: chrono::Local::now() };
    let mut body = search_query::parse(&q, &ctx);
    if body.is_empty() {
        return;
    }
    match guild {
        Some(g) => body.insert("context_guild_id".into(), json!(g)),
        None => body.insert("context_channel_id".into(), json!(ch)),
    };
    if st.search.oldest && !body.contains_key("sort_order") {
        body.insert("sort_order".into(), json!("asc"));
    }
    body.insert("hits_per_page".into(), json!(PER_PAGE));
    body.insert("page".into(), json!(page));
    // `SearchHistory.add`: per scope, newest first, at most 10.
    let mut prefs = st.prefs.clone();
    let h = prefs.search_history.entry(guild.unwrap_or(ch)).or_default();
    if h.first() != Some(&q) {
        h.retain(|x| *x != q);
        h.insert(0, q.clone());
        h.truncate(10);
        st.prefs = prefs.clone();
        st.intents.push(Intent::Prefs(Box::new(prefs)));
    }
    st.search.ran = q;
    st.search.page = page;
    st.search.results = Some(Results::Loading);
    st.intents.push(Intent::Search(serde_json::Value::Object(body)));
}

/// A value the popout offers for the filter being typed.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub label: String,
    pub meta: String,
    pub user: Option<reflux_model::Snowflake>,
    /// The token that replaces the one being typed.
    pub token: String,
}

/// `useMessageSearchAutocomplete`: the section title and values for the query's last token
/// (`from:`/`mentions:` users, `in:` channels, `has:`/`pinned:` values).
pub fn suggest(store: &Store, guild: Option<reflux_model::Snowflake>, query: &str) -> Option<(&'static str, Vec<Suggestion>)> {
    let last = query.rsplit(' ').next()?;
    let (key, partial) = last.split_once(':')?;
    let neg = if key.starts_with('-') { "-" } else { "" };
    let partial = partial.to_lowercase();
    let value = |key: &str, v: &str| Suggestion { label: v.to_string(), meta: String::new(), user: None, token: format!("{neg}{key}:{v}") };
    let (title, mut out): (&str, Vec<Suggestion>) = match key.trim_start_matches('-') {
        k @ ("from" | "mentions") => {
            let me = store.me.as_ref().map(|m| &m.base);
            let mut users: Vec<_> = match guild {
                Some(g) => store.guilds.get(&g).map(|gs| gs.members.keys().filter_map(|u| store.users.get(u).or(me.filter(|m| m.id == *u))).collect()).unwrap_or_default(),
                None => store.users.values().chain(me).collect(),
            };
            users.retain(|u| u.username.to_lowercase().contains(&partial) || store.display_name(guild, u.id).to_lowercase().contains(&partial));
            users.sort_by_key(|u| (!u.username.to_lowercase().starts_with(&partial), u.username.to_lowercase()));
            let tag = |u: &reflux_model::PartialUser| if u.discriminator.is_empty() || u.discriminator == "0" { u.username.clone() } else { format!("{}#{}", u.username, u.discriminator) };
            ("Users", users.into_iter().map(|u| Suggestion { label: store.display_name(guild, u.id), meta: tag(u), user: Some(u.id), token: format!("{neg}{k}:{}", tag(u)) }).collect())
        }
        "in" => {
            let mut chans: Vec<_> = store.channels.values().filter(|c| c.guild_id.is_some() && c.guild_id == guild && matches!(c.kind, reflux_model::ChannelType::GUILD_TEXT | reflux_model::ChannelType::GUILD_ANNOUNCEMENT | reflux_model::ChannelType::GUILD_VOICE | reflux_model::ChannelType::GUILD_FORUM) && store.can_view(c.id)).collect();
            chans.sort_by_key(|c| (c.position, c.id));
            let names = chans.into_iter().filter_map(|c| c.name.clone()).filter(|n| n.to_lowercase().contains(&partial));
            ("Channels", names.map(|n| Suggestion { label: format!("#{n}"), meta: String::new(), user: None, token: format!("{neg}in:{n}") }).collect())
        }
        "has" => ("Values", ["link", "embed", "file", "image", "video", "sound", "sticker", "poll", "forward"].iter().filter(|v| v.starts_with(&partial)).map(|v| value("has", v)).collect()),
        "pinned" => ("Values", ["true", "false"].iter().filter(|v| v.starts_with(&partial)).map(|v| value("pinned", v)).collect()),
        _ => return None,
    };
    out.truncate(10);
    Some((title, out))
}

/// The header box: a real field once clicked; Enter searches, Escape clears.
pub fn bar<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let s = &st.search;
    let has_text = !s.query.text.is_empty();
    // The popout and its rows, for the arrow keys.
    let scope = st.channel.map(|c| store.channels.get(&c).and_then(|ch| ch.guild_id).unwrap_or(c));
    let mut picks: Vec<Pick> = Vec::new();
    let mut values = false;
    if s.query.focused && !has_text {
        let history = scope.and_then(|k| st.prefs.search_history.get(&k)).map(Vec::as_slice).unwrap_or_default();
        picks = OPTIONS.iter().map(|(k, _)| Pick::Filter(k)).chain(history.iter().take(5).map(|q| Pick::Recent(q.clone()))).collect();
        layer.push(options(st, scope, history, selected(s, false), p, acc));
    } else if s.query.focused
        && let Some((title, list)) = suggest(store, st.guild, &s.query.text).filter(|(_, l)| !l.is_empty())
    {
        values = true;
        picks = list.iter().map(|sg| Pick::Value(sg.token.clone())).collect();
        layer.push(suggestions(store, st, title, list, selected(s, true), p, acc));
    }
    let field = text_input(&s.query, font(14.0, 400.0).lh(20.0))
        .key("search-field")
        .grow(1.0)
        .single_line()
        .color(p.text_primary)
        .placeholder(t("Search"), p.text_tertiary)
        .caret(p.text_primary, p.text_selection)
        .on_edit(move |s: &mut S| -> &mut TextEdit { &mut acc(s).search.query })
        .on_input_key(move |s: &mut S, k, _| {
            let st = acc(s);
            match k.key {
                Key::Escape => {
                    st.search.query = TextEdit::new("");
                    (st.search.results, st.search.nav) = (None, None);
                    true
                }
                Key::Down | Key::Up if !picks.is_empty() => {
                    let next = step(selected(&st.search, values), picks.len(), k.key == Key::Down);
                    st.search.nav = Some((st.search.query.text.clone(), next));
                    true
                }
                Key::Enter => match selected(&st.search, values).and_then(|i| picks.get(i)) {
                    Some(pick) => {
                        apply(st, pick);
                        true
                    }
                    None => false,
                },
                _ => false,
            }
        })
        .on_submit(move |s: &mut S, _| {
            let st = acc(s);
            st.search.nav = None;
            st.search.page = 1;
            st.search_requested = true;
        });
    let mut b = row()
        .key("search-bar")
        .w(dim::MESSAGE_SEARCH_BAR_WIDTH)
        .h(dim::MESSAGE_SEARCH_BAR_HEIGHT)
        .px(dim::INPUT_CONTAINER_PADDING + 1.0)
        .gap(dim::MESSAGE_SEARCH_BAR_ICON_GAP)
        .center_y()
        .radius(dim::RADIUS_XL)
        .fill(p.background_textarea)
        .border(1.0, over(if s.query.focused { p.background_modifier_accent_focus } else { p.background_modifier_accent }, p.background_textarea))
        .cursor(rune::Cursor::Text)
        .child(icon(&icons::MAGNIFYING_GLASS, dim::MESSAGE_SEARCH_BAR_ICON_SIZE).color(p.text_tertiary))
        .child(field);
    if has_text || s.results.is_some() {
        b = b.child(
            row()
                .size(20.0, 20.0)
                .center()
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Clear search"))
                .hover_group()
                .child(icon(&icons::X_CIRCLE_FILL, 16.0).color(p.text_tertiary).group_color(p.text_primary))
                .on_press(move |s: &mut S| {
                    let st = acc(s);
                    st.search.query = TextEdit::new("");
                    st.search.results = None;
                }),
        );
    }
    b
}

/// A popout section header (`.popoutSectionHeader`): an icon and an uppercase label.
fn section_head(glyph: &'static rune::Icon, label: &str, p: &Palette) -> El {
    row().w_full().px(8.0).py(4.0).gap(8.0).center_y().child(icon(glyph, 12.0).color(p.text_primary_muted)).child(text(label.to_uppercase()).font(font(12.0, 600.0)).color(p.text_primary_muted))
}

fn popout(p: &Palette) -> El {
    let bg = p.form_surface_background;
    col().w(320.0).padding(8.0).radius(8.0).fill(bg).border(1.0, over(p.background_modifier_accent, bg)).box_shadow(popout_shadow(p, 0.24, 0.14))
}

/// The value list for the filter being typed; picking one replaces the token.
fn suggestions<S: 'static>(store: &Store, st: &ShellState, title: &'static str, list: Vec<Suggestion>, sel: Option<usize>, p: &Palette, acc: ShellAccess<S>) -> El {
    let mut c = popout(p).child(section_head(&icons::MAGNIFYING_GLASS, t(title), p));
    for (i, sg) in list.into_iter().enumerate() {
        let mut r = option_row(sel == Some(i), p).gap(8.0).key(format!("search-suggest-{i}"));
        if let Some(u) = sg.user {
            r = r.child(super::avatar(store, st, u, 16.0, None, p.form_surface_background, p));
        }
        let pick = Pick::Value(sg.token.clone());
        c = c.child(
            r.child(text(sg.label).font(font(14.0, 600.0)).color(p.text_primary).truncate().shrink())
                .child(text(sg.meta).font(font(14.0, 400.0)).color(p.text_primary_muted).truncate().shrink())
                .on_press(move |s: &mut S| apply(acc(s), &pick)),
        );
    }
    anchor(c, "search-bar", crate::ui::Placement::BottomEnd, 8.0)
}

/// A 32px popout row, lit when hovered or arrowed to.
fn option_row(lit: bool, p: &Palette) -> El {
    let r = row().w_full().h(32.0).px(8.0).center_y().radius(4.0).cursor(rune::Cursor::Pointer).role("option");
    if lit { r.fill(p.background_modifier_hover) } else { r.hover(p.background_modifier_hover) }
}

/// The filter list under the empty, focused bar (picking one types its key), then the scope's
/// recent searches (`HistorySection`).
fn options<S: 'static>(st: &ShellState, scope: Option<reflux_model::Snowflake>, history: &[String], sel: Option<usize>, p: &Palette, acc: ShellAccess<S>) -> El {
    let mut c = popout(p).child(section_head(&icons::FUNNEL, t("Search filters"), p));
    for (i, (key, hint)) in OPTIONS.into_iter().enumerate() {
        c = c.child(
            option_row(sel == Some(i), p)
                .gap(6.0)
                .key(format!("search-opt-{key}"))
                .child(text(key).font(font(14.0, 600.0)).color(p.text_primary))
                .child(text(hint).font(font(14.0, 400.0)).color(p.text_primary_muted).truncate())
                .on_press(move |s: &mut S| apply(acc(s), &Pick::Filter(key))),
        );
    }
    if let (Some(scope), false) = (scope, history.is_empty()) {
        let mut prefs = st.prefs.clone();
        prefs.search_history.remove(&scope);
        let clear = row()
            .gap(4.0)
            .center_y()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .key("search-history-clear")
            .hover_group()
            .on_press(move |s: &mut S| acc(s).intents.push(Intent::Prefs(Box::new(prefs.clone()))))
            .child(icon(&icons::TRASH, 10.0).color(p.text_primary_muted).group_color(p.text_primary))
            .child(text(t("Clear")).font(font(12.0, 500.0)).color(p.text_primary_muted).group_color(p.text_primary));
        c = c.child(row().w_full().mt(8.0).child(section_head(&icons::CLOCK, t("Recent searches"), p).grow(1.0)).child(clear.mr(8.0)));
        for (i, q) in history.iter().take(5).enumerate() {
            let pick = Pick::Recent(q.clone());
            c = c.child(
                option_row(sel == Some(OPTIONS.len() + i), p)
                    .key(format!("search-history-{i}"))
                    .child(text(q.clone()).font(font(14.0, 500.0)).color(p.text_primary).truncate())
                    .on_press(move |s: &mut S| apply(acc(s), &pick)),
            );
        }
    }
    anchor(c, "search-bar", crate::ui::Placement::BottomEnd, 8.0)
}

/// `ChannelSearchResults`: 420 wide beside the chat.
pub fn panel<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let r = st.search.results.as_ref()?;
    let close = row()
        .size(32.0, 32.0)
        .center()
        .radius(4.0)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label("Close search")
        .key("search-close")
        .child(icon(&icons::X_BOLD, 18.0).color(p.text_primary_muted))
        .on_press(move |s: &mut S| acc(s).search.results = None);
    let title = match r {
        Results::Loading => t("Searching").to_string(),
        Results::Indexing => t("Indexing").to_string(),
        Results::Done { total: 1, .. } => "1 result".to_string(),
        Results::Done { total, .. } => format!("{total} results"),
        Results::Failed => t("Search results").to_string(),
    };
    let sort = Button::new(if st.search.oldest { t("Oldest") } else { t("Newest") })
        .variant(ButtonVariant::Secondary)
        .size(ButtonSize::Compact)
        .key("search-sort")
        .on_press(move |s: &mut S| {
            let st = acc(s);
            st.search.oldest ^= true;
            st.search.page = 1;
            st.search_requested = true;
        })
        .el(p);
    let header = row()
        .w_full()
        .h(56.0)
        .px(16.0)
        .justify_between()
        .center_y()
        .child(text(title).font(font(16.0, 600.0)).color(p.text_primary))
        .child(row().gap(8.0).center_y().child(sort).child(close))
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.background_modifier_hover));
    let message_state = |glyph: &'static rune::Icon, title: &str, line: &str| {
        col()
            .w_full()
            .grow(1.0)
            .center()
            .gap(8.0)
            .padding(16.0)
            .child(icon(glyph, 64.0).color(p.text_primary_muted))
            .child(text(title.to_string()).font(font(20.0, 600.0)).color(p.text_primary))
            .child(text(line.to_string()).font(font(14.0, 400.0)).color(p.text_primary_muted).wrap().align(rune::Align::Center))
    };
    let body = match r {
        Results::Loading => col().w_full().grow(1.0).center().child(spinner(p.text_primary_muted)),
        Results::Indexing => message_state(&icons::CIRCLE_NOTCH, t("Indexing channel"), "This channel is being indexed. Try again in a moment."),
        Results::Failed => message_state(&icons::WARNING_CIRCLE, t("Error"), t("An error occurred while searching.")),
        Results::Done { messages, .. } if messages.is_empty() => message_state(&icons::MAGNIFYING_GLASS, t("No results"), t("Try a different search query.")),
        Results::Done { messages, total } => {
            let mut list = col().w_full().pt(8.0).px(8.0).pb(12.0);
            let mut last = None;
            for m in messages {
                if last != Some(m.channel_id) {
                    let name = store.channels.get(&m.channel_id).and_then(|c| c.name.clone()).unwrap_or_default();
                    if !name.is_empty() {
                        list = list.child(
                            row()
                                .gap(4.0)
                                .px(4.0)
                                .pt(8.0)
                                .pb(4.0)
                                .center_y()
                                .child(icon(super::channel_icon(store.channels.get(&m.channel_id).map(|c| c.kind).unwrap_or_default()), 16.0).color(p.text_primary_muted))
                                .child(text(name).font(font(14.0, 600.0)).color(p.text_primary_muted)),
                        );
                    }
                    last = Some(m.channel_id);
                }
                list = list.child(result(store, st, m, p, acc));
            }
            let pages = total.div_ceil(PER_PAGE).min(400);
            if pages > 1 {
                let page = st.search.page;
                let go = move |to: u32| {
                    move |s: &mut S| {
                        let st = acc(s);
                        st.search.page = to;
                        st.search_requested = true;
                    }
                };
                list = list.child(
                    row()
                        .w_full()
                        .py(8.0)
                        .gap(8.0)
                        .center()
                        .child(Button::new(t("Previous")).variant(ButtonVariant::Secondary).size(ButtonSize::Compact).disabled(page <= 1).key("search-prev").on_press(go(page.saturating_sub(1).max(1))).el(p))
                        .child(text(format!("{page} / {pages}")).font(font(13.0, 500.0)).color(p.text_primary_muted))
                        .child(Button::new(t("Next")).variant(ButtonVariant::Secondary).size(ButtonSize::Compact).disabled(page >= pages).key("search-next").on_press(go(page + 1)).el(p)),
                );
            }
            col().w_full().grow(1.0).min_h(0.0).child(scroller(col().w_full().h_full().scroll(ScrollSpec { stick_bottom: false }).key("search-results").child(list), false, p))
        }
    };
    Some(
        col()
            .w(420.0)
            .h_full()
            .fill(p.background_secondary)
            .child(el().pin(Some(0.0), Some(0.0), None, Some(0.0)).w(1.0).fill(p.background_modifier_hover))
            .child(header)
            .child(body),
    )
}

/// `.messageItem`: the message as a group head; Jump on hover.
fn result<S: 'static>(store: &Store, st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> El {
    let guild = store.channels.get(&m.channel_id).and_then(|c| c.guild_id);
    let md = Md::new(store, &st.media, guild, p, format!("search-m{}", m.id));
    let (ch, id) = (m.channel_id, m.id);
    let jump = row()
        .h(20.0)
        .px(8.0)
        .center()
        .radius(4.0)
        .fill(p.background_primary)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .hover_group()
        .key(format!("search-jump-{id}"))
        .child(text(t("Jump")).font(font(11.0, 600.0).lh(20.0)).color(p.text_primary_muted).group_color(p.text_primary))
        .on_press(move |s: &mut S| super::jump(acc(s), guild, ch, id));
    col()
        .w_full()
        .mb(8.0)
        .py(8.0)
        .radius(6.0)
        // shortcut: media wider than the card is cropped, not scaled down to fit.
        .clip_rounded(6.0)
        .fill(p.background_secondary_lighter)
        .border(1.0, p.background_header_secondary)
        .hover_group()
        .child(col().w_full().inert().child(super::chat::message_preview(store, st, m, &md, p, acc)))
        .child(jump.pin(None, Some(12.0), Some(12.0), None).group_opacity(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflux_model::Snowflake;

    #[test]
    fn arrow_keys_wrap_and_values_start_selected() {
        assert_eq!(step(None, 3, true), 0);
        assert_eq!(step(None, 3, false), 2);
        assert_eq!(step(Some(2), 3, true), 0);
        assert_eq!(step(Some(0), 3, false), 2);
        let mut s = Search { query: TextEdit::new("from:"), ..Default::default() };
        assert_eq!(selected(&s, true), Some(0));
        assert_eq!(selected(&s, false), None);
        s.nav = Some(("from:".into(), 2));
        assert_eq!(selected(&s, true), Some(2));
        s.query = TextEdit::new("from:a");
        assert_eq!(selected(&s, true), Some(0), "typing resets the arrowed row");
    }

    #[test]
    fn suggests_values_for_the_last_token() {
        let d = crate::shell::demo::Demo::new();
        let g = Some(Snowflake(2000));
        let (title, users) = suggest(&d.store, g, "hello from:ad").unwrap();
        assert_eq!(title, "Users");
        assert_eq!(users[0].token, "from:ada", "{users:?}");
        let (_, chans) = suggest(&d.store, g, "-in:gen").unwrap();
        assert_eq!(chans[0].token, "-in:general");
        let (_, has) = suggest(&d.store, g, "has:").unwrap();
        assert!(has.iter().any(|v| v.token == "has:image"));
        assert!(suggest(&d.store, g, "plain words").is_none());
        assert!(suggest(&d.store, g, "before:").is_none(), "dates have no list");
    }
}
