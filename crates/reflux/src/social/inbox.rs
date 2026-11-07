//! The header's inbox popout (`InboxPopout`: unread channels, bookmarks and recent mentions),
//! `ForwardModal`, the publish confirmation, `ChannelFollowModal`, and the message menu's
//! social items (`MessageContextMenu`: forward, bookmark, suppress embeds, publish, remove all
//! reactions, report).

use reflux_model::{Channel, ChannelType, Message, MessageFlags, Permissions as P, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, Icon, ScrollSpec, col, icon, row, text};
use serde_json::{Value, json};

use super::{Done, Modal, body, close, desc, error, field, footer};
use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::overlay::popout_shadow;
use crate::ui::{Button, ButtonSize, ButtonVariant, Checkbox, MenuEntry, MenuItem, Modal as Dialog, Select, over, popup_access, scroller, spinner};
use crate::i18n::t;

pub const INBOX: &str = "header-inbox";

/// `InboxIcon`.
pub static INBOX_ICON: Icon = Icon {
    name: "fluxer-inbox",
    view: 24.0,
    body: r#"<path d="M19.3333 2H4.15583C2.95333 2 2.01083 2.96417 2.01083 4.16667L2 19.3333C2 20.525 2.95333 21.5 4.15583 21.5H19.3333C20.525 21.5 21.5 20.525 21.5 19.3333V4.16667C21.5 2.96417 20.525 2 19.3333 2ZM19.3333 15H15C15 16.7983 13.5375 18.25 11.75 18.25C9.9625 18.25 8.5 16.7983 8.5 15H4.15583V4.16667H19.3333V15Z"/>"#,
};

/// Tabs: (label, icon). 0 unread, 1 bookmarks, 2 mentions.
const TABS: [(&str, &Icon); 3] = [("Unread", &icons::BELL_FILL), ("Bookmarks", &icons::BOOKMARK_SIMPLE_FILL), ("Mentions", &icons::AT_BOLD)];

fn mentions_path(f: [bool; 3]) -> String {
    format!("/users/@me/mentions?limit=25&roles={}&everyone={}&guilds={}", f[0], f[1], f[2])
}

/// `UNREAD_PREVIEW_MESSAGE_LIMIT` / `BULK_PREVIEW_CHANNEL_BATCH_SIZE`.
const PREVIEW_LIMIT: usize = 25;
const BULK_BATCH: usize = 5;
const BULK: &str = "inbox-bulk:";

/// Loads the tab's list.
fn load(st: &mut ShellState, tab: u8) {
    match tab {
        1 => st.social(Method::GET, "/users/@me/saved-messages", None, Done::Data("bookmarks".into())),
        2 => {
            // `RecentMentionCommands`: pages of 25, each before the last one shown.
            st.social.data.retain(|k, _| !k.starts_with("mentions"));
            let path = mentions_path(st.social.mention_filter);
            st.social(Method::GET, path, None, Done::Append("mentions".into(), 25));
        }
        _ => {
            // Previews are fetched again as sections show.
            st.expr.fetched.retain(|k, _| !k.starts_with(BULK));
        }
    }
}

/// The header button: toggles the popout and loads its tab.
pub fn inbox_button<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let open = st.social.inbox_open;
    row()
        .size(32.0, 32.0)
        .center()
        .radius(16.0)
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(t("Inbox"))
        .key(INBOX)
        .child(icon(&INBOX_ICON, 20.0).color(if open { p.text_primary } else { p.text_primary_muted }).group_color(p.text_primary))
        .on_press(move |s: &mut S| {
            let st = acc(s);
            st.social.inbox_open ^= true;
            if st.social.inbox_open {
                let t = st.social.inbox_tab;
                load(st, t);
            }
        })
}

fn messages_of(v: Option<&Value>) -> Option<Vec<Message>> {
    v.and_then(|v| serde_json::from_value(v.clone()).ok())
}

pub fn popout<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !st.social.inbox_open {
        return;
    }
    let bg = p.form_surface_background;
    let tab = st.social.inbox_tab;
    let tabs = col().w(46.0).h_full().py(8.0).gap(8.0).items_center().child(rune::el().pin(None, Some(0.0), Some(0.0), Some(0.0)).w(1.0).fill(p.background_header_secondary)).children(TABS.iter().enumerate().map(|(i, (name, glyph))| {
        let on = tab == i as u8;
        row()
            .size(30.0, 30.0)
            .center()
            .radius(6.0)
            .fill(if on { p.background_modifier_selected } else { rune::Color::TRANSPARENT })
            .hover(p.background_modifier_hover)
            .cursor(rune::Cursor::Pointer)
            .role("tab")
            .label(name.to_string())
            .key(format!("inbox-tab-{i}"))
            .child(icon(glyph, 16.0).color(if on { p.text_primary } else { p.text_primary_muted }))
            .on_press(move |s: &mut S| {
                let st = acc(s);
                st.social.inbox_tab = i as u8;
                load(st, i as u8);
            })
    }));
    let (title, action): (&str, Option<El>) = match tab {
        0 => {
            let unread = unread_channels(store);
            let acks: Vec<(Snowflake, Snowflake)> = unread.iter().filter_map(|c| Some((c.id, c.last_message_id?))).collect();
            let b = (!acks.is_empty()).then(|| Button::new(t("Mark all as read")).variant(ButtonVariant::Secondary).size(ButtonSize::Compact).key("inbox-read-all").on_press(move |s: &mut S| acc(s).intents.push(Intent::MarkRead(acks.clone()))).el(p));
            ("Unread", b)
        }
        1 => (t("Bookmarks"), None),
        _ => (t("Mentions"), Some(filter_button(p, acc))),
    };
    let head = row().w_full().h(48.0).px(16.0).gap(8.0).center_y().child(rune::el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.background_header_secondary)).child(text(title).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).grow(1.0)).children(action);
    let list = match tab {
        0 => unread_list(store, st, p, acc),
        1 => bookmark_list(store, st, p, acc),
        _ => mention_list(store, st, p, acc),
    };
    let content = col().grow(1.0).min_w(0.0).h_full().child(head).child(col().w_full().grow(1.0).min_h(0.0).child(scroller(col().w_full().h_full().scroll(ScrollSpec { stick_bottom: false }).key("inbox-list").child(list.padding(12.0)), false, p)));
    let surface = row()
        .pin(None, Some(56.0), Some(16.0), Some(8.0))
        .w(600.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .clip_rounded(8.0)
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .role("dialog")
        .label(t("Inbox"))
        .child(tabs)
        .child(content)
        .modal(true, move |s: &mut S| acc(s).social.inbox_open = false);
    layer.push(surface);
}

/// The mentions filter menu, over the popout.
pub fn filter_menu<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !st.social.inbox_open || st.social.inbox_tab != 2 {
        return;
    }
    let f = st.social.mention_filter;
    let item = |i: usize, id: &str, l: &str| {
        MenuItem::new(id.to_string(), l.to_string()).checkbox(f[i]).on_select(move |s: &mut S| {
            let st = acc(s);
            st.social.mention_filter[i] ^= true;
            load(st, 2);
        })
    };
    crate::ui::Menu::new("inbox-filter")
        .placement(crate::ui::Placement::BottomEnd)
        .item(item(0, "f-roles", "Include role mentions"))
        .item(item(1, "f-everyone", "Include @everyone and @here"))
        .item(item(2, "f-guilds", t("Include all community mentions")))
        .el(&st.popups, popup_access(move |s: &mut S| &mut acc(s).popups), p, layer);
}

/// The mentions filter: a funnel opening a checkbox menu.
fn filter_button<S: 'static>(p: &Palette, acc: ShellAccess<S>) -> El {
    row()
        .size(28.0, 28.0)
        .center()
        .radius(6.0)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(t("Filter mentions"))
        .key("inbox-filter")
        .child(icon(&icons::FUNNEL_FILL, 18.0).color(p.text_primary_muted))
        .on_press(move |s: &mut S| acc(s).popups.toggle("inbox-filter"))
}

/// Unread, unmuted text channels and DMs, newest activity first.
fn unread_channels(store: &Store) -> Vec<&Channel> {
    let mut v: Vec<&Channel> = store.channels.values().filter(|c| !c.kind.is_thread() && c.kind != ChannelType::GUILD_CATEGORY && store.has_unread(c.id) && !store.is_muted(c)).collect();
    v.sort_by_key(|c| std::cmp::Reverse(c.last_message_id));
    v
}

fn empty(glyph: &'static Icon, title: &str, line: &str, p: &Palette) -> El {
    col()
        .w_full()
        .min_h(300.0)
        .center()
        .gap(8.0)
        .child(icon(glyph, 64.0).color(p.text_primary_muted))
        .child(text(title.to_string()).font(font(20.0, 600.0).lh(28.0)).color(p.text_primary))
        .child(text(line.to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap().align(rune::Align::Center))
}

/// `ChannelSourcePreview`'s avatar: a community channel's glyph, else the DM/group face.
fn source_icon(store: &Store, st: &ShellState, c: &Channel, size: f32, p: &Palette) -> El {
    match (c.guild_id, c.kind == ChannelType::GROUP_DM) {
        (Some(_), _) => icon(crate::shell::channel_icon(c.kind), size).color(p.text_primary_muted),
        (None, true) => crate::shell::group_avatar(store, st, c, size, p.form_surface_background, p),
        (None, false) => {
            let me = store.my_id();
            let user = c.recipients.iter().map(|u| u.id).find(|&u| u != me).unwrap_or(me);
            crate::shell::avatar(store, st, user, size, None, p.form_surface_background, p)
        }
    }
}

fn where_of(store: &Store, c: Snowflake) -> String {
    let Some(ch) = store.channels.get(&c) else { return String::new() };
    match ch.guild_id {
        Some(g) => format!("#{} · {}", ch.name.clone().unwrap_or_default(), super::guild_name(store, g)),
        None => crate::shell::channel_title(store, ch),
    }
}

/// `UnreadChannelsContent`: a section per unread channel (collapse caret, its name, mentions,
/// notification settings, mark as read) with its first unread messages, fetched through
/// `POST /channels/messages/bulk` five channels at a time as sections scroll into view.
fn unread_list<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let chans = unread_channels(store);
    if chans.is_empty() {
        return empty(&icons::CHECK_CIRCLE_FILL, "You're all caught up", "New messages show up here.", p);
    }
    let previews = previews(st);
    // (channel, its bulk request) for the sections, in order.
    let requests: Vec<(Snowflake, Value)> = chans
        .iter()
        .map(|c| {
            let req = match store.read_states.get(&c.id).and_then(|r| r.last_message_id) {
                Some(ack) => json!({ "channel_id": c.id, "limit": PREVIEW_LIMIT, "after": ack }),
                None => json!({ "channel_id": c.id, "limit": PREVIEW_LIMIT }),
            };
            (c.id, req)
        })
        .collect();
    let asked: std::collections::HashSet<Snowflake> = st.expr.fetched.keys().filter_map(|k| k.strip_prefix(BULK)).flat_map(|ids| ids.split(',').filter_map(|i| i.parse().ok()).map(Snowflake)).collect();
    let sections = chans.into_iter().map(|c| section(store, st, c, previews.get(&c.id), p, acc));
    col().w_full().gap(8.0).growth(rune::GrowthSpec::default()).virtualize(160.0).children(sections.collect::<Vec<_>>()).on_visible_rows(move |s: &mut S, r| {
        let want: Vec<&(Snowflake, Value)> = requests.get(r.start..r.end.min(requests.len())).unwrap_or_default().iter().filter(|(c, _)| !asked.contains(c)).collect();
        for batch in want.chunks(BULK_BATCH) {
            let ids = batch.iter().map(|(c, _)| c.to_string()).collect::<Vec<_>>().join(",");
            let body = json!({ "requests": batch.iter().map(|(_, r)| r.clone()).collect::<Vec<_>>() });
            let st = acc(s);
            st.expr.fetched.insert(format!("{BULK}{ids}"), crate::shell::expr::Fetched::Loading);
            st.intents.push(Intent::Fetch { key: format!("{BULK}{ids}"), method: Method::POST, path: "/channels/messages/bulk".into(), body: Some(body) });
        }
        !want.is_empty()
    })
}

/// Bulk-fetched previews by channel: the oldest unread first, at most 25.
fn previews(st: &ShellState) -> std::collections::HashMap<Snowflake, Vec<Message>> {
    let mut out = std::collections::HashMap::new();
    for (k, v) in &st.expr.fetched {
        let (Some(_), crate::shell::expr::Fetched::Done(v)) = (k.strip_prefix(BULK), v) else { continue };
        for e in v["channels"].as_array().into_iter().flatten() {
            let Some(id) = e["channel_id"].as_str().and_then(|i| i.parse().ok()).map(Snowflake) else { continue };
            let mut ms: Vec<Message> = serde_json::from_value(e["messages"].clone()).unwrap_or_default();
            ms.sort_by_key(|m| m.id);
            let skip = ms.len().saturating_sub(PREVIEW_LIMIT);
            out.insert(id, ms.split_off(skip));
        }
    }
    out
}

fn section<S: 'static>(store: &Store, st: &ShellState, c: &Channel, msgs: Option<&Vec<Message>>, p: &Palette, acc: ShellAccess<S>) -> El {
    let (id, g) = (c.id, c.guild_id);
    let mentions = store.mention_count(id);
    let collapsed = st.social.inbox_collapsed.contains(&id);
    let head_button = |glyph: &'static Icon, label: &str, key: String| {
        row().size(24.0, 24.0).center().radius(4.0).cursor(rune::Cursor::Pointer).role("button").label(label.to_string()).hover_group().hover(p.background_modifier_hover).key(key).child(icon(glyph, 16.0).color(p.text_primary_muted).group_color(p.text_primary))
    };
    let caret = head_button(if collapsed { &icons::CARET_RIGHT_BOLD } else { &icons::CARET_DOWN_BOLD }, if collapsed { t("Expand") } else { t("Collapse") }, format!("inbox-fold-{id}")).on_press(move |s: &mut S| {
        let st = acc(s);
        if !st.social.inbox_collapsed.remove(&id) {
            st.social.inbox_collapsed.insert(id);
        }
    });
    let bell = head_button(if store.is_muted(c) { &icons::BELL_SLASH_FILL } else { &icons::BELL_FILL }, t("Notification settings"), format!("inbox-bell-{id}")).on_press(move |s: &mut S| {
        let st = acc(s);
        st.social.inbox_open = false;
        st.select_guild(g, Some(id));
        st.popups.open = Some(super::channel::BELL.into());
    });
    let last = c.last_message_id;
    let read = head_button(&icons::CHECK_BOLD, t("Mark as read"), format!("inbox-read-{id}")).on_press(move |s: &mut S| {
        if let Some(m) = last {
            acc(s).intents.push(Intent::MarkRead(vec![(id, m)]));
        }
    });
    let open = move |s: &mut S| {
        let st = acc(s);
        st.social.inbox_open = false;
        st.select_guild(g, Some(id));
    };
    let mut head = row()
        .w_full()
        .h(32.0)
        .gap(6.0)
        .center_y()
        .child(caret)
        .child(source_icon(store, st, c, 16.0, p))
        .child(text(where_of(store, id)).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary).truncate().shrink().cursor(rune::Cursor::Pointer).on_press(open))
        .child(rune::el().grow(1.0));
    if mentions > 0 {
        head = head.child(row().min_w(16.0).h(16.0).px(4.0).center().radius(8.0).fill(p.status_danger).child(text(mentions.to_string()).font(font(12.0, 700.0).lh(16.0)).color(rune::Color::WHITE)));
    }
    let mut out = col().w_full().key(format!("inbox-unread-{id}")).child(head.child(bell).child(read));
    if collapsed {
        return out;
    }
    let body = match msgs {
        None => row().w_full().h(64.0).center().child(spinner(p.text_primary_muted)),
        Some(ms) if ms.is_empty() => row().w_full().h(40.0).center().child(text(t("No unread messages")).font(font(14.0, 400.0)).color(p.text_primary_muted)),
        Some(ms) => {
            let mut b = col().w_full().children(ms.iter().map(|m| col().w_full().inert().child(crate::shell::preview(store, st, m, format!("inbox-u{}", m.id), p, acc))));
            // `hasMoreUnreadThanShown`.
            if ms.len() >= PREVIEW_LIMIT && ms.last().map(|m| m.id) != last {
                b = b.child(row().w_full().h(32.0).center().cursor(rune::Cursor::Pointer).role("button").key(format!("inbox-all-{id}")).child(text(t("View all unread")).font(font(14.0, 500.0)).color(p.text_link)).on_press(open));
            }
            b
        }
    };
    out = out.child(col().w_full().py(8.0).radius(8.0).clip_rounded(8.0).border(1.0, p.background_header_secondary).child(body));
    out
}

/// One message with its channel above, Jump and an optional remove button on hover.
fn card<S: 'static>(store: &Store, st: &ShellState, m: &Message, remove: Option<(&'static str, Method, String)>, p: &Palette, acc: ShellAccess<S>) -> El {
    let (ch, id) = (m.channel_id, m.id);
    let guild = store.channels.get(&ch).and_then(|c| c.guild_id);
    let small = |label: El, key: String| row().h(20.0).px(8.0).center().radius(4.0).fill(p.background_primary).cursor(rune::Cursor::Pointer).role("button").hover_group().key(key).child(label);
    let jump = small(text(t("Jump")).font(font(11.0, 600.0).lh(20.0)).color(p.text_primary_muted).group_color(p.text_primary), format!("inbox-jump-{id}")).on_press(move |s: &mut S| {
        let st = acc(s);
        st.social.inbox_open = false;
        crate::shell::jump(st, guild, ch, id);
    });
    let mut actions = row().gap(4.0).child(jump);
    if let Some((what, method, path)) = remove {
        actions = actions.child(
            row()
                .size(24.0, 24.0)
                .center()
                .radius(4.0)
                .fill(p.background_primary)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(what)
                .hover_group()
                .key(format!("inbox-remove-{id}"))
                .child(icon(&icons::X_BOLD, 16.0).color(p.text_primary_muted).group_color(p.text_primary))
                .on_press(move |s: &mut S| {
                    let st = acc(s);
                    for k in ["mentions", "bookmarks"] {
                        if let Some(Value::Array(a)) = st.social.data.get_mut(k) {
                            a.retain(|x| x["id"].as_str() != Some(&id.to_string()));
                        }
                    }
                    st.social.saved.remove(&id);
                    st.social(method.clone(), path.clone(), None, Done::Nothing);
                }),
        );
    }
    col()
        .w_full()
        .key(format!("inbox-card-{id}"))
        .mb(8.0)
        .gap(4.0)
        .child(row().gap(6.0).center_y().px(4.0).children(store.channels.get(&ch).map(|c| source_icon(store, st, c, 14.0, p))).child(text(where_of(store, ch)).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary).truncate()))
        .child(
            col()
                .w_full()
                .py(8.0)
                .radius(8.0)
                .clip_rounded(8.0)
                .border(1.0, p.background_header_secondary)
                .hover_group()
                .child(col().w_full().inert().child(crate::shell::preview(store, st, m, format!("inbox-m{id}"), p, acc)))
                .child(actions.pin(None, Some(12.0), Some(12.0), None).group_opacity(0.0, 1.0)),
        )
}

fn loading_or(st: &ShellState, key: &str, p: &Palette, f: impl FnOnce(&Value) -> El) -> El {
    match st.social.data.get(key) {
        Some(v) => f(v),
        None => row().w_full().h(300.0).center().child(spinner(p.text_primary_muted)),
    }
}

/// `RecentMentionsContent`: the next page loads when the list's end scrolls into view; past the
/// last one, "You're at the end".
fn mention_list<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let end = st.social.data.get("mentions:end").and_then(Value::as_bool).unwrap_or(false);
    loading_or(st, "mentions", p, |v| {
        let ms = messages_of(Some(v)).unwrap_or_default();
        if ms.is_empty() {
            return empty(&icons::AT_BOLD, t("No mentions yet"), "Messages that mention you show up here.", p);
        }
        let n = ms.len();
        let before = ms.last().map(|m| m.id);
        let mut rows: Vec<El> = ms.iter().map(|m| card(store, st, m, Some(("Remove mention", Method::DELETE, format!("/users/@me/mentions/{}", m.id))), p, acc)).collect();
        rows.push(if end {
            col().w_full().py(24.0).gap(8.0).items_center().key("mentions-end")
                .child(icon(&icons::FLAG_CHECKERED_FILL, 32.0).color(p.text_primary_muted))
                .child(text(t("You're at the end")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary))
                .child(text(t("You've seen all your recent mentions. More will appear here soon.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap().align(rune::Align::Center))
        } else {
            row().w_full().h(48.0).center().key("mentions-more").child(spinner(p.text_primary_muted))
        });
        let filter = st.social.mention_filter;
        col().w_full().growth(rune::GrowthSpec::default()).virtualize(140.0).children(rows).on_visible_rows(move |s: &mut S, r| {
            let st = acc(s);
            let key = format!("mentions:before:{}", before.unwrap_or_default());
            if end || r.end <= n || st.social.data.contains_key(&key) {
                return false;
            }
            st.social.data.insert(key, Value::Null);
            let path = format!("{}&before={}", mentions_path(filter), before.unwrap_or_default());
            st.social(Method::GET, path, None, Done::Append("mentions".into(), 25));
            false
        })
    })
}

fn bookmark_list<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    loading_or(st, "bookmarks", p, |v| {
        let entries = v.as_array().cloned().unwrap_or_default();
        let ms: Vec<Message> = entries.iter().filter_map(|e| serde_json::from_value(e["message"].clone()).ok()).collect();
        if ms.is_empty() {
            return empty(&icons::BOOKMARK_SIMPLE_FILL, t("No bookmarks"), t("Bookmark messages to save them for later."), p);
        }
        col().w_full().children(ms.iter().map(|m| card(store, st, m, Some(("Remove bookmark", Method::DELETE, format!("/users/@me/saved-messages/{}", m.id))), p, acc)))
    })
}

// ---- Message menu ------------------------------------------------------------------------

fn is_user_message(m: &Message) -> bool {
    m.kind.0 == 0 || m.kind.0 == 19
}

/// The message menu's social items.
pub fn message_items<S: 'static>(store: &Store, st: &ShellState, c: &Channel, m: &Message, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let (ch, id) = (c.id, m.id);
    let perms = store.permissions(ch);
    let me = store.my_id();
    let mut out = Vec::new();
    if is_user_message(m) {
        out.push(MenuEntry::Item(MenuItem::new("forward", t("Forward")).icon(&icons::ARROW_BEND_UP_RIGHT_FILL).on_select(move |s: &mut S| acc(s).open_social(Modal::Forward { channel: ch, message: id }))));
    }
    let saved = st.social.saved.contains(&id);
    out.push(MenuEntry::Item(if saved {
        MenuItem::new("unbookmark", t("Remove bookmark")).icon(&icons::BOOKMARK_SIMPLE_FILL).on_select(move |s: &mut S| {
            let st = acc(s);
            st.social.saved.remove(&id);
            st.social(Method::DELETE, format!("/users/@me/saved-messages/{id}"), None, Done::Nothing);
        })
    } else {
        MenuItem::new("bookmark", t("Bookmark message")).icon(&icons::BOOKMARK_SIMPLE).on_select(move |s: &mut S| {
            let st = acc(s);
            st.social.saved.insert(id);
            st.social(Method::POST, "/users/@me/saved-messages", Some(json!({ "channel_id": ch, "message_id": id })), Done::Nothing);
        })
    }));
    let own = m.author.id == me;
    if !m.embeds.is_empty() && (own || perms.contains(P::MANAGE_MESSAGES)) {
        let hidden = m.flags.contains(MessageFlags::SUPPRESS_EMBEDS);
        let flags = if hidden { m.flags.bits() & !MessageFlags::SUPPRESS_EMBEDS.bits() } else { m.flags.bits() | MessageFlags::SUPPRESS_EMBEDS.bits() };
        out.push(MenuEntry::Item(MenuItem::new("suppress-embeds", if hidden { t("Show embeds") } else { t("Suppress embeds") }).icon(&icons::EYE_SLASH_FILL).on_select(move |s: &mut S| {
            acc(s).social(Method::PATCH, format!("/channels/{ch}/messages/{id}"), Some(json!({ "flags": flags })), Done::Nothing)
        })));
    }
    if c.kind == ChannelType::GUILD_ANNOUNCEMENT && is_user_message(m) && !m.flags.contains(MessageFlags::CROSSPOSTED) && (own && perms.contains(P::SEND_MESSAGES) || perms.contains(P::MANAGE_MESSAGES)) {
        out.push(MenuEntry::Item(MenuItem::new("publish", t("Publish")).icon(&icons::MEGAPHONE_FILL).on_select(move |s: &mut S| acc(s).open_social(Modal::Publish { channel: ch, message: id }))));
    }
    if !m.reactions.is_empty() && c.guild_id.is_some() && perms.contains(P::MANAGE_MESSAGES) {
        out.push(MenuEntry::Item(MenuItem::new("remove-reactions", t("Remove all reactions")).icon(&icons::SMILEY_SAD_FILL).danger().on_select(move |s: &mut S| {
            acc(s).social(Method::DELETE, format!("/channels/{ch}/messages/{id}/reactions"), None, Done::Nothing)
        })));
    }
    // `MediaMenuData.canEditAltText`: your own (or, with Manage Messages, anyone's) images and videos.
    if (own || perms.contains(P::MANAGE_MESSAGES)) && m.message_snapshots.is_empty() {
        let media: Vec<_> = m.attachments.iter().filter(|a| a.content_type.starts_with("image/") || a.content_type.starts_with("video/")).collect();
        for a in &media {
            let label = if media.len() == 1 { t("Edit alt text").to_string() } else { format!("{} ({})", t("Edit alt text"), a.filename) };
            let (att, desc) = (a.id, a.description.clone().unwrap_or_default());
            out.push(MenuEntry::Item(MenuItem::new(format!("alt-text-{att}"), label).icon(&icons::PENCIL_SIMPLE).on_select(move |s: &mut S| {
                let st = acc(s);
                st.open_social(Modal::AltText { channel: ch, message: id, attachment: att });
                st.form.text.insert("alt".into(), rune::TextEdit::new(desc.clone()));
            })));
        }
    }
    if !own {
        let r = Modal::Report { target: "message", guild: c.guild_id, channel: Some(ch), id };
        out.push(MenuEntry::Item(MenuItem::new("report-message", t("Report message")).icon(&icons::FLAG_FILL).danger().on_select(move |s: &mut S| super::moderation::open_report(acc(s), r.clone()))));
    }
    out
}

/// `EditAltTextModal`: the attachment's description, saved with a message PATCH that lists every
/// attachment (only this one changes). A textarea at least 3 rows tall with a character count.
pub fn alt_text<S: 'static>(store: &Store, st: &ShellState, ch: Snowflake, id: Snowflake, att: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    const MAX: usize = 4096; // `MAX_ATTACHMENT_ALT_TEXT_LENGTH`
    let Some(m) = store.messages(ch).iter().find(|m| m.id == id).cloned() else { return Dialog::new(t("Edit alt text"), super::close(acc)).el(p) };
    let len = st.form.value("alt", "").chars().count();
    let mut items = vec![
        {
            let edit = st.form.text.get("alt").cloned().unwrap_or_default();
            crate::ui::Input::new("form-alt", &edit, move |s: &mut S| -> &mut rune::TextEdit { acc(s).form.text.entry("alt".into()).or_default() })
                .label(t("Alt text description"))
                .placeholder(t("Describe this media for screen readers"))
                .rows(3, 8)
                .el(p)
        },
        text(format!("{len}/{MAX}")).font(font(12.0, 400.0).lh(16.0)).color(if len > MAX { p.status_danger } else { p.text_tertiary }),
    ];
    items.extend(super::error(st, p));
    let go = move |s: &mut S| {
        let sh = acc(s);
        let d = sh.form.value("alt", "");
        if d.chars().count() > MAX {
            return;
        }
        let atts: Vec<Value> = m.attachments.iter().map(|a| if a.id == att { json!({ "id": a.id, "description": Some(d.trim()).filter(|d| !d.is_empty()) }) } else { json!({ "id": a.id }) }).collect();
        sh.social(Method::PATCH, format!("/channels/{ch}/messages/{id}"), Some(json!({ "content": m.content, "attachments": atts })), Done::Toast(t("Alt text updated").into()));
    };
    Dialog::new(t("Edit alt text"), super::close(acc)).body(super::body(items)).footer(super::footer(st, t("Save"), false, "alt-text-save", acc, go, p)).el(p)
}

// ---- Forward / publish / follow ------------------------------------------------------------

/// Channels a message can be forwarded to: DMs and guild text channels where you can send.
pub fn forward_targets(store: &Store, q: &str) -> Vec<Snowflake> {
    let q = q.trim().trim_start_matches('#').to_lowercase();
    let mut v: Vec<(bool, String, Snowflake)> = store
        .channels
        .values()
        .filter(|c| match c.kind {
            ChannelType::DM | ChannelType::GROUP_DM => true,
            ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT => store.permissions(c.id).contains(P::VIEW_CHANNEL | P::SEND_MESSAGES),
            _ => false,
        })
        .map(|c| (c.guild_id.is_some(), where_of(store, c.id), c.id))
        .filter(|(_, n, _)| q.is_empty() || n.to_lowercase().contains(&q))
        .collect();
    // DMs first, then by name.
    v.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    v.into_iter().map(|x| x.2).collect()
}

/// `MAX_FORWARD_TARGETS`.
const FORWARD_MAX: usize = 5;

pub fn forward<S: 'static>(store: &Store, st: &ShellState, ch: Snowflake, msg: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let sel: Vec<Snowflake> = st.form.data.get("picked").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let full = sel.len() >= FORWARD_MAX;
    let rows: Vec<El> = forward_targets(store, &st.form.value("q", ""))
        .into_iter()
        .take(50)
        .map(|c| {
            let on = sel.contains(&c);
            let face = match store.channels.get(&c) {
                Some(x) if x.kind == ChannelType::DM => crate::shell::avatar(store, st, x.recipients.first().map_or(c, |u| u.id), 24.0, None, p.form_surface_background, p),
                Some(x) if x.kind == ChannelType::GROUP_DM => crate::shell::group_avatar(store, st, x, 24.0, p.form_surface_background, p),
                x => icon(crate::shell::channel_icon(x.map_or(ChannelType::GUILD_TEXT, |x| x.kind)), 18.0).color(p.text_primary_muted).size(24.0, 24.0),
            };
            let mut r = row()
                .w_full()
                .h(40.0)
                .px(8.0)
                .gap(10.0)
                .center_y()
                .radius(6.0)
                .hover(p.background_modifier_hover)
                .role("checkbox")
                .checked(on)
                .key(format!("fwd-{c}"))
                .child(face)
                .child(text(where_of(store, c)).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).truncate().grow(1.0).shrink())
                .child(Checkbox::<S>::new(on).el(p));
            if on || !full {
                r = r.cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| {
                    let f = &mut acc(s).form;
                    let mut v: Vec<Snowflake> = f.data.get("picked").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                    match v.iter().position(|&x| x == c) {
                        Some(i) => {
                            v.remove(i);
                        }
                        None => v.push(c),
                    }
                    f.data.insert("picked".into(), json!(v));
                });
            } else {
                r = r.opacity(0.5);
            }
            r
        })
        .collect();
    let preview = store.messages(ch).iter().find(|m| m.id == msg).map(|m| {
        col().w_full().max_h(160.0).clip_rounded(6.0).py(8.0).radius(6.0).fill(p.background_secondary).border(1.0, over(p.background_header_secondary, p.background_secondary)).inert().child(crate::shell::preview(store, st, m, format!("fwd-m{msg}"), p, acc))
    });
    let mut items = vec![field(st, acc, "q", "", "", "Search", p), col().w_full().h(240.0).scroll(ScrollSpec { stick_bottom: false }).child(col().w_full().gap(2.0).children(rows))];
    items.extend(preview);
    items.push(field(st, acc, "comment", "", "", t("Add a comment (optional)"), p));
    items.extend(error(st, p));
    let go = move |s: &mut S| {
        let sh = acc(s);
        let sel: Vec<Snowflake> = sh.form.data.get("picked").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        if sel.is_empty() {
            sh.form.error = Some("Choose where to forward it.".into());
            return;
        }
        let comment = sh.form.value("comment", "").trim().to_string();
        for (i, dest) in sel.iter().enumerate() {
            let last = i + 1 == sel.len() && comment.is_empty();
            let b = json!({ "message_reference": { "type": 1, "channel_id": ch, "message_id": msg } });
            sh.social(Method::POST, format!("/channels/{dest}/messages"), Some(b), if last { Done::Close } else { Done::Nothing });
            if !comment.is_empty() {
                sh.social(Method::POST, format!("/channels/{dest}/messages"), Some(json!({ "content": comment })), if i + 1 == sel.len() { Done::Close } else { Done::Nothing });
            }
        }
    };
    let action = crate::i18n::tf("Send ({selectedCount}/{selectionLimit})", &[("selectedCount", &(sel.len()).to_string()), ("selectionLimit", &FORWARD_MAX.to_string())]);
    Dialog::new(t("Forward message"), close(acc)).body(body(items)).footer(footer(st, &action, false, "forward-send", acc, go, p)).el(p)
}

pub fn publish<S: 'static>(store: &Store, st: &ShellState, ch: Snowflake, msg: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let mut items = vec![desc("This will send this message to all communities following this channel.", p)];
    if let Some(m) = store.messages(ch).iter().find(|m| m.id == msg) {
        items.push(col().w_full().max_h(220.0).clip_rounded(6.0).py(8.0).radius(6.0).fill(p.background_secondary).border(1.0, over(p.background_header_secondary, p.background_secondary)).inert().child(crate::shell::preview(store, st, m, format!("pub-m{msg}"), p, acc)));
    }
    items.extend(error(st, p));
    let go = move |s: &mut S| acc(s).social(Method::POST, format!("/channels/{ch}/messages/{msg}/crosspost"), None, Done::Close);
    Dialog::new(t("Publish this message?"), close(acc)).body(body(items)).footer(footer(st, t("Publish"), false, "publish-confirm", acc, go, p)).el(p)
}

/// `ChannelFollowModal`: pick a community you manage webhooks in, then a text channel of it.
pub fn follow<S: 'static>(store: &Store, st: &ShellState, ch: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let mut guilds: Vec<(String, String)> = store
        .guilds
        .iter()
        .filter(|(g, gs)| gs.info.is_some() && store.guild_permissions(**g).contains(P::MANAGE_WEBHOOKS))
        .map(|(g, _)| (g.to_string(), super::guild_name(store, *g)))
        .collect();
    guilds.sort_by(|a, b| a.1.cmp(&b.1));
    let g = st.form.data.get("guild").and_then(Value::as_str).map(str::to_string);
    let mut chans: Vec<(String, String)> = g
        .as_deref()
        .and_then(|g| g.parse::<Snowflake>().ok())
        .map(|g| {
            store
                .channels
                .values()
                .filter(|c| c.guild_id == Some(g) && c.kind == ChannelType::GUILD_TEXT && c.id != ch && store.permissions(c.id).contains(P::MANAGE_WEBHOOKS))
                .map(|c| (c.id.to_string(), format!("#{}", c.name.clone().unwrap_or_default())))
                .collect()
        })
        .unwrap_or_default();
    chans.sort_by(|a, b| a.1.cmp(&b.1));
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let gopts: Vec<(&str, &str)> = guilds.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let copts: Vec<(&str, &str)> = chans.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let target = st.form.data.get("channel").and_then(Value::as_str).map(str::to_string);
    let mut items = vec![
        desc("Get this channel's published messages in a channel of your own.", p),
        Select::new("follow-guild", &gopts, g.as_deref()).label(t("Community")).placeholder(t("Select a community")).width(406.0).on_choose(move |s: &mut S, v: &str| {
            let f = &mut acc(s).form;
            f.data.insert("guild".into(), json!(v));
            f.data.remove("channel");
        }).el(&st.popups, pops, p, layer),
    ];
    if g.is_some() {
        items.push(Select::new("follow-channel", &copts, target.as_deref()).label(t("Channel")).placeholder(t("Select a channel")).width(406.0).on_choose(move |s: &mut S, v: &str| {
            acc(s).form.data.insert("channel".into(), json!(v));
        }).el(&st.popups, pops, p, layer));
    }
    items.extend(error(st, p));
    let go = move |s: &mut S| {
        let sh = acc(s);
        let Some(t) = sh.form.data.get("channel").and_then(Value::as_str).map(str::to_string) else {
            sh.form.error = Some("Choose a channel.".into());
            return;
        };
        sh.social(Method::POST, format!("/channels/{ch}/followers"), Some(json!({ "webhook_channel_id": t })), Done::Close);
    };
    Dialog::new(t("Follow this channel"), close(acc)).body(body(items)).footer(footer(st, t("Follow"), false, "follow-confirm", acc, go, p)).el(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_filters_build_the_query() {
        assert_eq!(mentions_path([true, false, true]), "/users/@me/mentions?limit=25&roles=true&everyone=false&guilds=true");
    }
}
