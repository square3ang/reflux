//! Message search: the header's `MessageSearchBar` (with its filter options while empty), and
//! the `ChannelSearchResults` panel beside the chat, whose results jump to their message.
// shortcut: no scope menu, search chips or per-filter value suggestions (users, channels, dates).

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
    st.search.ran = q;
    st.search.page = page;
    st.search.results = Some(Results::Loading);
    st.intents.push(Intent::Search(serde_json::Value::Object(body)));
}

/// The header box: a real field once clicked; Enter searches, Escape clears.
pub fn bar<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let s = &st.search;
    let has_text = !s.query.text.is_empty();
    let field = text_input(&s.query, font(14.0, 400.0).lh(20.0))
        .key("search-field")
        .grow(1.0)
        .single_line()
        .color(p.text_primary)
        .placeholder(t("Search"), p.text_tertiary)
        .caret(p.text_primary, p.text_selection)
        .on_edit(move |s: &mut S| -> &mut TextEdit { &mut acc(s).search.query })
        .on_input_key(move |s: &mut S, k, _| {
            if k.key != Key::Escape {
                return false;
            }
            let st = acc(s);
            st.search.query = TextEdit::new("");
            st.search.results = None;
            true
        })
        .on_submit(move |s: &mut S, _| {
            let st = acc(s);
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
    if s.query.focused && !has_text {
        layer.push(options(p, acc));
    }
    b
}

/// The filter list under the empty, focused bar: picking one types its key.
fn options<S: 'static>(p: &Palette, acc: ShellAccess<S>) -> El {
    let bg = p.form_surface_background;
    let mut c = col()
        .w(320.0)
        .padding(8.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .child(text("SEARCH OPTIONS").font(font(12.0, 600.0)).color(p.text_primary_muted).px(8.0).py(4.0));
    for (key, hint) in OPTIONS {
        c = c.child(
            row()
                .w_full()
                .h(32.0)
                .px(8.0)
                .gap(6.0)
                .center_y()
                .radius(4.0)
                .hover(p.background_modifier_hover)
                .cursor(rune::Cursor::Pointer)
                .key(format!("search-opt-{key}"))
                .child(text(key).font(font(14.0, 600.0)).color(p.text_primary))
                .child(text(hint).font(font(14.0, 400.0)).color(p.text_primary_muted).truncate())
                .on_press(move |s: &mut S| {
                    let q = &mut acc(s).search.query;
                    q.insert(key);
                    q.focused = true;
                }),
        );
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
