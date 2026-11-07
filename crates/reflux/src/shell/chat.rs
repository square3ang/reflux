//! The main column: channel header, message list, composer and member list
//! (`ChannelHeader`, `ChannelMessages`, `ChannelDivider`, `theme/styles/Message.module.css`,
//! `MessageReactions`, `textarea/TextareaInput`).

use std::cell::RefCell;
use std::collections::HashSet;

use chrono::{DateTime, Local, TimeZone};
use reflux_model::{
    Channel, ChannelType, Message, MessageFlags, MessageType, Permissions, ReactionEmoji, Snowflake,
};
use reflux_store::Store;
use rune::text::Span;
use rune::{Color, El, GrowthSpec, Key, ScrollSpec, Shadow, TextEdit, Tx, col, el, icon, rich, row, text, text_input};

use super::autocomplete;
use super::markdown::{self, Md};
use crate::media::Load;
use super::{Intent, ReplyTo, ShellAccess, ShellState, Target, avatar, channel_icon, emoji, members, role_color};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::overlay::anchor;
use crate::ui::{Button, ButtonVariant, Menu, MenuItem, Modal, Placement, PopupAccess, Tooltip, mix, over, popup_access, rgb, scroller};
use crate::i18n::t;

/// `MESSAGE_GROUP_TIMEOUT`.
const GROUP_MS: u64 = 7 * 60 * 1000;
/// `--message-group-spacing` (the default; the Appearance setting picks the real one).
const GROUP_SPACING: f32 = 16.0;
/// Avatar column start: `--chat-horizontal-padding`.
const PAD: f32 = dim::CHAT_HORIZONTAL_PADDING_DEFAULT;
/// Content column start: padding + avatar + gutter.
const CONTENT_X: f32 = PAD + dim::MESSAGE_AVATAR_SIZE + dim::MESSAGE_GUTTER;

pub fn main<S: 'static>(store: &Store, st: &ShellState, channel: Option<&Channel>, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some(c) = channel else {
        return col().grow(1.0).min_w(0.0).h_full().fill(p.background_secondary_lighter);
    };
    let show_members = st.member_list && c.guild_id.is_some() && !reflux_store::threads::is_forum(c.kind);
    if reflux_store::threads::is_forum(c.kind) {
        // A forum lists its posts instead of messages (`ForumChannelView`).
        let mut content = row().w_full().grow(1.0).min_h(0.0).child(super::forum::view(store, st, c, p, acc, layer));
        content = content.children(super::threads::panel(store, st, c, p, acc, layer));
        super::threads::layer(store, st, Some(c), p, acc, layer);
        return col().grow(1.0).min_w(0.0).h_full().fill(p.background_secondary_lighter).child(header(store, st, c, p, acc, layer)).child(content);
    }
    let mut chat = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        // Fixed-width cards stay under the search panel when the column narrows.
        .clip_rounded(0.0)
        .children(match super::attachments::mature_gate(store, st, c, p, acc) {
            Some(gate) => vec![gate],
            None => {
                let mut v = vec![messages(store, st, c, p, acc, layer, false)];
                v.extend(if c.kind.is_thread() { super::threads::composer_area(store, st, c, false, p, acc) } else { vec![composer(store, st, c, p, acc, Slot::Main)] });
                v
            }
        })
        // Files dragged in from other apps join the tray.
        .on_drop_files(move |s: &mut S, files, _| {
            let st = acc(s);
            st.drop_hover = false;
            super::upload::add_paths(st, files);
        })
        .on_drop_hover(move |s: &mut S, over, _| acc(s).drop_hover = over);
    if st.drop_hover {
        chat = chat.child(drop_overlay(c, p));
    }
    chat = chat.children(super::compact::announcer(store, c));
    let mut content = row().w_full().grow(1.0).min_h(0.0).child(chat);
    if let Some(panel) = super::search::panel(store, st, p, acc) {
        content = content.child(panel);
    } else if let Some(panel) = super::threads::panel(store, st, c, p, acc, layer) {
        content = content.child(panel);
    } else if show_members {
        content = content
            .child(if c.kind.is_thread() { super::threads::members_column(store, st, c, p, acc) } else { members::list(store, st, c, p, acc) })
            .child(el().pin(None, Some(0.0), Some(dim::LAYOUT_MEMBER_LIST_WIDTH), Some(0.0)).w(1.0).fill(over(p.user_area_divider_color, p.background_secondary_lighter)));
    }
    if let Some(m) = st.confirm_delete.and_then(|id| find(store, c.id, id)) {
        layer.push(delete_modal(store, st, m, p, acc));
    }
    super::pins::popout(store, st, c, p, acc, layer);
    super::threads::layer(store, st, Some(c), p, acc, layer);
    super::upload::plus_menu(store, st, c, p, acc, layer);
    col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .fill(p.background_secondary_lighter)
        .child(header(store, st, c, p, acc, layer))
        .children(super::call::dm_strip(store, st, c, p, acc))
        .child(content)
}

/// `.dropTarget` overlay while files are dragged over the chat.
fn drop_overlay(c: &Channel, p: &Palette) -> El {
    let name = c.name.clone().map_or_else(|| "this conversation".to_string(), |n| format!("#{n}"));
    row().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().fill(Color { a: 0.6, ..Color::BLACK }).inert().child(
        col()
            .padding(32.0)
            .gap(8.0)
            .center()
            .radius(16.0)
            .fill(p.brand_primary)
            .child(icon(&icons::UPLOAD_SIMPLE_FILL, 48.0).color(Color::WHITE))
            .child(text(crate::i18n::tf("Upload to {displayName}", &[("displayName", &name.to_string())])).font(font(20.0, 600.0)).color(Color::WHITE))
            .child(text("You can add comments before uploading.").font(font(14.0, 400.0)).color(Color::WHITE)),
    )
}

/// A message drawn on its own as a group head (pins, search results).
pub(super) fn message_preview<S: 'static>(store: &Store, st: &ShellState, m: &Message, md: &Md, p: &Palette, acc: ShellAccess<S>) -> El {
    message(store, st, m, true, md, p, acc)
}

fn find(store: &Store, channel: Snowflake, id: Snowflake) -> Option<&Message> {
    let ms = store.messages(channel);
    ms.binary_search_by_key(&id, |m| m.id).ok().map(|i| &ms[i])
}

// ---- Permissions -------------------------------------------------------------------------------

/// What the current user may do to messages in a channel (`useMessagePermissions`).
#[derive(Clone, Copy)]
struct Perms {
    me: Snowflake,
    send: bool,
    react: bool,
    /// `MANAGE_MESSAGES` in a guild: delete others' messages.
    manage: bool,
    pin: bool,
    /// Developer mode: show Copy message ID.
    dev: bool,
    /// Messages here can start threads (in this parent).
    thread_from: Option<Snowflake>,
}

impl Perms {
    fn of(store: &Store, c: &Channel) -> Self {
        let p = store.permissions(c.id);
        Perms {
            me: store.my_id(),
            send: p.contains(Permissions::SEND_MESSAGES),
            react: p.contains(Permissions::ADD_REACTIONS),
            manage: c.guild_id.is_some() && p.contains(Permissions::MANAGE_MESSAGES),
            pin: p.contains(Permissions::PIN_MESSAGES),
            dev: store.settings.developer_mode,
            thread_from: (matches!(c.kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT)
                && store.can_create_thread(c, reflux_store::threads::CreateKind::FromMessage))
            .then_some(c.id),
        }
    }
    fn can_edit(&self, m: &Message) -> bool {
        m.author.id == self.me && is_user_message(m)
    }
    /// Only these types can be deleted (`http-api/messages` Delete message).
    fn can_delete(&self, m: &Message) -> bool {
        (m.author.id == self.me || self.manage)
            && (is_user_message(m) || matches!(m.kind, MessageType::USER_JOIN | MessageType::CHANNEL_PINNED_MESSAGE))
    }
}

/// An optimistic own message the server hasn't echoed yet (its ID is still the nonce).
fn is_pending(m: &Message) -> bool {
    m.nonce.as_deref().is_some_and(|n| n == m.id.to_string())
}

// ---- Header --------------------------------------------------------------------------------

pub(crate) fn channel_title(store: &Store, c: &Channel) -> String {
    match c.kind {
        ChannelType::DM | ChannelType::GROUP_DM => c.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| {
            let me = store.my_id();
            c.recipients.iter().filter(|u| u.id != me).map(|u| store.display_name(None, u.id)).collect::<Vec<_>>().join(", ")
        }),
        _ => c.name.clone().unwrap_or_default(),
    }
}

fn header<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let muted = p.text_primary_muted;
    let action = |i: &'static rune::Icon, label: &str, selected: bool| {
        row()
            .size(dim::CHANNEL_HEADER_ACTION_SIZE, dim::CHANNEL_HEADER_ACTION_SIZE)
            .center()
            .radius(dim::RADIUS_FULL)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
            .label(label.to_string())
            .child(icon(i, dim::CHANNEL_HEADER_ACTION_ICON_SIZE).color(if selected { p.text_primary } else { muted }).group_color(p.text_primary))
    };
    let mut left = row().grow(1.0).min_w(0.0).center_y();
    left = left.children(super::threads::parent_crumb(store, c, p, acc));
    if c.kind == ChannelType::DM {
        let other = c.recipients.iter().find(|u| u.id != store.my_id()).map(|u| u.id).unwrap_or_default();
        left = left.child(avatar(store, st, other, 24.0, None, p.background_secondary_lighter, p));
    } else {
        left = left.child(icon(channel_icon(c.kind), dim::CHANNEL_HEADER_ICON_SIZE).color(muted));
    }
    left = left.child(
        text(channel_title(store, c))
            .font(font(16.0, 500.0).lh(24.0))
            .color(p.text_primary)
            .truncate()
            .shrink()
            .ml(dim::CHANNEL_HEADER_NAME_GAP),
    );
    if let Some(topic) = c.topic.as_ref().filter(|t| !t.is_empty()) {
        left = left
            .child(text("•").font(font(16.0, 400.0).lh(24.0)).color(p.background_modifier_hover).mx(dim::CHANNEL_HEADER_TOPIC_DIVIDER_GAP))
            .child(
                markdown::inline_text(topic, &Md::new(store, &st.media, c.guild_id, p, "topic"), font(13.0, 400.0).lh(18.0), p.text_tertiary)
                    .grow(1.0)
                    .shrink()
                    .pr(dim::SPACING_5)
                    .cursor(rune::Cursor::Pointer)
                    .key("header-topic")
                    .on_press({
                        let cid = c.id;
                        move |s: &mut S| acc(s).open_social(crate::social::Modal::Topic(cid))
                    }),
            );
    }
    let mut right = row().gap(dim::CHANNEL_HEADER_ACTIONS_GAP).center_y();
    let cid = c.id;
    let pins = action(&icons::PUSH_PIN, "Pinned Messages", st.popups.is_open(super::pins::POPOUT))
        .key(super::pins::POPOUT)
        .on_press(move |s: &mut S| super::pins::open(acc(s), cid));
    let forum = reflux_store::threads::is_forum(c.kind);
    if c.guild_id.is_some() {
        right = right.child(if c.kind.is_thread() { super::threads::notification_button(st, c, p, acc) } else { crate::social::bell_button(store, st, c, p, acc) });
        right = right.children(super::threads::header_button(store, st, c, p, acc));
        if !forum {
            right = right.child(pins).child(
                action(&icons::USERS, "Toggle Member List", st.member_list)
                    .key("toggle-members")
                    .on_press(move |s: &mut S| acc(s).member_list ^= true),
            );
        }
    } else {
        right = right
            .child(super::call::header_action(store, st, c, false, action(&icons::PHONE, "Start Voice Call", false), acc))
            .child(super::call::header_action(store, st, c, true, action(&icons::VIDEO_CAMERA, "Start Video Call", false), acc))
            .child(pins);
    }
    if !forum {
        right = right.child(super::search::bar(store, st, p, acc, layer));
    }
    right = right.child(crate::social::inbox_button(st, p, acc));
    row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::CHANNEL_HEADER_PADDING_INLINE)
        .gap(dim::CHANNEL_HEADER_GAP)
        .center_y()
        .fill(p.background_secondary_lighter)
        .child(left)
        .child(right)
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(over(p.user_area_divider_color, p.background_secondary_lighter)))
}

// ---- Time ------------------------------------------------------------------------------------

fn local(ms: u64) -> DateTime<Local> {
    Local.timestamp_millis_opt(ms as i64).single().unwrap_or_default()
}

/// `hour: numeric, minute: 2-digit` in the interface language and time format.
pub fn format_time(ms: u64) -> String {
    crate::desktop::time::time(&local(ms), crate::i18n::locale())
}

/// `getRelativeDateString`: "Today at 3:04 PM", "Yesterday at …", else "11/6/2026, 3:04 PM".
pub fn format_relative(ms: u64, now_ms: u64) -> String {
    let (d, now) = (local(ms), local(now_ms));
    let days = now.date_naive().signed_duration_since(d.date_naive()).num_days();
    let time = format_time(ms);
    let arg = [("timeString", time.as_str())];
    match days {
        0 => crate::i18n::tf("Today at {timeString}", &arg),
        1 => crate::i18n::tf("Yesterday at {timeString}", &arg),
        _ => format!("{}, {time}", crate::desktop::time::short_date(&d, crate::i18n::locale())),
    }
}

/// `getFormattedFullDate`: "Friday, November 6, 2026" (the date divider).
pub fn format_full_date(ms: u64) -> String {
    crate::desktop::time::long_date(&local(ms), crate::i18n::locale())
}

pub fn format_full_date_time(ms: u64) -> String {
    format!("{} {}", format_full_date(ms), format_time(ms))
}

/// A message's time: its ISO timestamp, else its snowflake.
pub(super) fn message_ms(m: &Message) -> u64 {
    DateTime::parse_from_rfc3339(&m.timestamp).map_or_else(|_| m.id.timestamp_ms(), |d| d.timestamp_millis() as u64)
}

fn same_day(a: u64, b: u64) -> bool {
    local(a).date_naive() == local(b).date_naive()
}

pub(super) fn is_user_message(m: &Message) -> bool {
    m.kind == MessageType::DEFAULT || m.kind == MessageType::REPLY
}

/// `isNewMessageGroup` (`messaging/utils/MessageGroupingUtils.ts`).
pub fn is_new_group(prev: Option<&Message>, m: &Message) -> bool {
    let Some(prev) = prev else { return true };
    if m.kind == MessageType::REPLY {
        return true;
    }
    let (sys, prev_sys) = (!is_user_message(m), !is_user_message(prev));
    if sys != prev_sys {
        return true;
    }
    if !(sys && prev_sys) && m.kind != prev.kind && !(is_user_message(m) && is_user_message(prev)) {
        return true;
    }
    if prev.kind.0 <= MessageType::REPLY.0 && prev.author.id != m.author.id {
        return true;
    }
    if m.webhook_id.is_some() && prev.author.username != m.author.username {
        return true;
    }
    let (a, b) = (message_ms(prev), message_ms(m));
    if !same_day(a, b) || b.saturating_sub(a) > GROUP_MS {
        return true;
    }
    let (ps, cs) = (prev.flags.contains(MessageFlags::SUPPRESS_NOTIFICATIONS), m.flags.contains(MessageFlags::SUPPRESS_NOTIFICATIONS));
    if !ps && cs {
        return true;
    }
    ps && !cs && (!m.mentions.is_empty() || !m.mention_roles.is_empty() || m.mention_everyone)
}

// ---- Message list ------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Item {
    Date(u64),
    /// Index into the window; `head` starts a group, `space` is the gap above it.
    Msg { i: usize, head: bool, space: f32, unread: bool },
}

/// The channel stream: date dividers, group heads and the spacing between groups
/// (`createChannelStream` + `renderChannelStream`).
fn stream(msgs: &[Message], first_unread: Option<Snowflake>, spacing: f32) -> Vec<Item> {
    let mut out = Vec::new();
    let mut last_day: Option<u64> = None;
    let mut last_kind: Option<bool> = None; // Some(is_system) of the last rendered group
    for (i, m) in msgs.iter().enumerate() {
        let ms = message_ms(m);
        if last_day.is_none_or(|d| !same_day(d, ms)) {
            out.push(Item::Date(ms));
            last_day = Some(ms);
            last_kind = None;
        }
        let prev = i.checked_sub(1).map(|j| &msgs[j]).filter(|_| last_kind.is_some());
        let head = is_new_group(prev, m);
        let unread = first_unread == Some(m.id);
        let sys = !is_user_message(m);
        let space = match last_kind {
            Some(prev_sys) if head && !unread => {
                if prev_sys && sys { spacing / 2.0 } else { spacing }
            }
            _ => 0.0,
        };
        last_kind = Some(sys);
        out.push(Item::Msg { i, head, space, unread });
    }
    out
}

/// The message list; `pane`: the thread side panel's (its own list position, row keys prefixed).
pub(super) fn messages<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>, pane: bool) -> El {
    let pre = if pane { "pane-" } else { "" };
    let (list_fits, rows_below) = if pane { (st.threads.list_fits, st.threads.rows_below) } else { (st.list_fits, st.rows_below) };
    let msgs = store.messages(c.id);
    let sticky = st.sticky_unread.filter(|s| s.0 == c.id).map(|s| s.1);
    let first_unread = if store.has_unread(c.id) {
        let ack = store.read_states.get(&c.id).and_then(|r| r.last_message_id).unwrap_or_default();
        msgs.iter().find(|m| m.id > ack && m.author.id != store.my_id()).map(|m| m.id)
    } else {
        sticky
    };
    let mut md = Md::new(store, &st.media, c.guild_id, p, "");
    // Links to messages on this instance jump in-app, invites and gifts open their modals;
    // others open in the browser.
    let (web, inv, gift) = (st.web_base.clone(), st.invite_base.clone(), st.gift_base.clone());
    md.on_link = Some(std::rc::Rc::new(move |t: El| {
        let (web, inv, gift) = (web.clone(), inv.clone(), gift.clone());
        t.on_link(move |s: &mut S, url: &str, tx: &mut Tx| {
            if let Some((g, ch, m)) = super::parse_message_link(&web, url) {
                super::jump(acc(s), g, ch, m)
            } else if let Some(code) = crate::social::invite_code(&inv, &web, url) {
                crate::social::open_invite(acc(s), code)
            } else if let Some(code) = crate::social::gift_code(&gift, &web, url) {
                crate::settings::open_gift(acc(s), code)
            } else {
                super::links::open(acc(s), url, tx)
            }
        })
    }));
    md.on_emoji = Some(std::rc::Rc::new(move |e: El, kind: &reflux_markdown::EmojiKind, key: String| {
        let kind = kind.clone();
        e.cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| super::expr::open_emoji_info(acc(s), &kind, key.clone()))
    }));
    let copy = RefCell::new(markdown::CopyMap::new());
    let mut heads = HashSet::new();
    let perms = Perms::of(store, c);
    // The action bar's message: the hovered one, or the one whose menu or picker is open.
    let active = st.active_msg.filter(|_| st.popups.open.is_some());
    let spacing = crate::look::with(|l| if store.settings.message_display_compact { l.compact_group_spacing } else { l.group_spacing });
    let items = stream(msgs, first_unread, spacing);
    let n_items = items.len();
    let mut rows = Vec::with_capacity(n_items + 1);
    // `BlockedMessageGroups`: a run of blocked authors folds into one bar (rows stay one per item).
    let runs = crate::social::blocked_runs(store, msgs);
    let run_of = |i: usize| runs.iter().find(|(s, n)| (*s..s + n).contains(&i)).copied();
    // The channel's start gets its welcome section (not in the thread pane).
    let welcome = !pane && !store.messages.get(&c.id).is_some_and(|w| w.has_more_before);
    for (k, it) in items.iter().enumerate() {
        let row_el = match *it {
            Item::Date(ms) => date_divider(ms, p).key(format!("{pre}date-{}", format_full_date(ms))),
            // "Hide messages from blocked users": no bar either.
            Item::Msg { i, .. } if run_of(i).is_some() && crate::look::with(|l| l.hide_blocked) => el().key(format!("msg-{}", msgs[i].id)),
            Item::Msg { i, .. } if run_of(i).is_some_and(|(s, _)| !st.social.show_blocked.contains(&msgs[s].id)) => {
                let (s, n) = run_of(i).unwrap_or_default();
                if s == i { crate::social::blocked_bar(msgs[s].id, n, false, p, acc).key(format!("msg-{}", msgs[i].id)) } else { el().key(format!("msg-{}", msgs[i].id)) }
            }
            Item::Msg { i, head, space, unread } => {
                let m = &msgs[i];
                let shown_run = run_of(i).filter(|(s, _)| *s == i).map(|(s, n)| crate::social::blocked_bar(msgs[s].id, n, true, p, acc));
                let mut r = col().w_full().children(shown_run);
                if unread {
                    r = r.child(new_divider(p));
                } else if space > 0.0 {
                    r = r.pt(space);
                }
                if head {
                    heads.insert(m.id);
                }
                let mut body = message(store, st, m, head, &md.body(format!("m{}", m.id), Some((m.id, &copy))), p, acc);
                if !is_pending(m) {
                    let id = m.id;
                    body = body
                        .on_hover_change(move |s: &mut S, inside, _| {
                            let st = acc(s);
                            if inside {
                                st.hover_msg = Some(id);
                            } else if st.hover_msg == Some(id) {
                                st.hover_msg = None;
                            }
                        })
                        .on_secondary_press(move |s: &mut S, at, _| {
                            let st = acc(s);
                            st.popups.open_at(&format!("msg-menu-{id}"), [at.x, at.y]);
                            st.active_msg = Some(id);
                            st.media_ui.invite_menu = (None, None);
                        });
                    // `messageDoubleClickActions`: edit my own message, else reply.
                    if crate::look::with(|l| l.double_click_actions) && perms.send && is_user_message(m) && m.message_snapshots.is_empty() {
                        let (m2, me, edit) = (m.clone(), perms.me, perms.can_edit(m));
                        body = body.on_double_press(move |s: &mut S| {
                            let st = acc(s);
                            if st.editing.as_ref().is_some_and(|e| e.0 == m2.id) {
                                return;
                            }
                            if edit { start_edit(st, &m2) } else { start_reply(st, me, &m2) }
                        });
                    }
                    let editing = st.editing.as_ref().is_some_and(|e| e.0 == id);
                    if !editing && (st.hover_msg == Some(id) || active == Some(id)) {
                        let perms = Perms { thread_from: perms.thread_from.filter(|_| store.thread(id).is_none()), ..perms };
                        body = body.child(action_bar(m, c, perms, st, p, acc, layer));
                    }
                } else if st.failed.contains_key(&m.id) {
                    let id = m.id;
                    body = body.on_hover_change(move |s: &mut S, inside, _| {
                        let st = acc(s);
                        if inside {
                            st.hover_msg = Some(id);
                        } else if st.hover_msg == Some(id) {
                            st.hover_msg = None;
                        }
                    });
                    if st.hover_msg == Some(id) {
                        body = body.child(super::queue::bar(m, st, p, acc, layer));
                    }
                }
                r.child(body).key(format!("msg-{}", m.id))
            }
        };
        // The channel's start: its welcome section above the first row.
        rows.push(if k == 0 && welcome { col().w_full().key("msgs-start").child(crate::social::welcome(store, st, c, p, acc)).child(row_el) } else { row_el });
    }
    if rows.is_empty() && welcome && store.messages.contains_key(&c.id) {
        rows.push(col().w_full().key("msgs-start").child(crate::social::welcome(store, st, c, p, acc)));
    }
    if let Some(m) = active.and_then(|id| find(store, c.id, id)) {
        let open = st.popups.open.clone().unwrap_or_default();
        if open == format!("msg-menu-{}", m.id) || open == format!("msg-more-{}", m.id) {
            let perms = Perms { thread_from: perms.thread_from.filter(|_| store.thread(m.id).is_none()), ..perms };
            let mut menu = message_menu(store, &open, m, c, perms, st, acc).separator();
            for e in crate::social::message_items(store, st, c, m, acc) {
                menu = menu.entry(e);
            }
            let invite = super::invite_embed::menu_items(st);
            if !invite.is_empty() {
                menu = menu.separator();
                for e in invite {
                    menu = menu.entry(e);
                }
            }
            menu.el(&st.popups, pops(acc), p, layer);
        } else if open == format!("emoji-{}", m.id) {
            let close = move |s: &mut S| {
                let st = acc(s);
                st.popups.close();
                st.active_msg = None;
            };
            let picker = super::expr::react_picker(store, st, c.id, m.id, p, acc).modal(true, close);
            layer.push(anchor(picker, &format!("react-{}", m.id), Placement::LeftStart, 8.0));
        }
    }
    let more = store.messages.get(&c.id).is_some_and(|w| w.has_more_before);
    let newer = store.messages.get(&c.id).is_some_and(|w| w.has_more_after);
    let cid = c.id;
    let list = col()
        .w_full()
        .key(format!("msgs-{cid}"))
        .growth(GrowthSpec::default())
        .virtualize(48.0)
        .children(rows)
        .selection_group(true)
        .selection_copy({
            let copy = copy.into_inner();
            move |sel| {
                let texts = sel.texts().iter().filter(|t| t.is_selected()).map(|t| (t.key(), t.range(), t.text()));
                Some(markdown::copy_selection(texts, &copy, &heads))
            }
        })
        // `--messages-bottom-clearance`: room for the typing rail over the list's end.
        .child(el().h(dim::MESSAGES_BOTTOM_CLEARANCE).key(format!("{pre}msgs-end")))
        .on_visible_rows(move |s: &mut S, r| {
            let st = acc(s);
            if r.start == 0 && more && !st.intents.contains(&Intent::LoadOlder { channel: cid }) {
                st.intents.push(Intent::LoadOlder { channel: cid });
            }
            // The clearance row is the last; seeing it means the newest message is on screen.
            let bottom = r.end > n_items;
            if bottom && newer && !st.intents.contains(&Intent::LoadNewer { channel: cid }) {
                st.intents.push(Intent::LoadNewer { channel: cid });
            }
            let below = (n_items + 1).saturating_sub(r.end);
            let fits = r.start == 0 && bottom;
            let (at_bottom, rows_below, list_fits) = if pane {
                (&mut st.threads.at_bottom, &mut st.threads.rows_below, &mut st.threads.list_fits)
            } else {
                (&mut st.at_bottom, &mut st.rows_below, &mut st.list_fits)
            };
            let changed = *at_bottom != bottom || (*rows_below > PRESENT_ROWS) != (below > PRESENT_ROWS) || *list_fits != fits;
            *list_fits = fits;
            *at_bottom = bottom;
            *rows_below = below;
            changed
        });
    let viewport = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: !newer }).child(list);
    // Bottom-aligned like Fluxer's `.scrollerContent` only while everything fits: Rune's reveal
    // and centring miss rows in an overflowing `justify_end` viewport (seen live: a jump showed
    // an empty list).
    let viewport = if list_fits && !newer { viewport.justify_end() } else { viewport };
    let mut area = col().w_full().grow(1.0).min_h(0.0).child(scroller(viewport, true, p));
    if newer || rows_below > PRESENT_ROWS {
        area = area.child(present_bar(cid, pre, p, acc));
    }
    let me = store.my_id();
    let typing: Vec<_> = store.typing_users(c.id, st.now_ms).into_iter().filter(|u| *u != me && !store.is_blocked(*u)).collect();
    if !typing.is_empty() {
        area = area.child(typing_rail(store, st, c.guild_id, &typing, p));
    }
    area
}

/// Rows below the screen past which the jump-to-present bar shows.
const PRESENT_ROWS: usize = 30;

/// `.jumpToPresentBar`: a pill over the list's foot while reading history.
fn present_bar<S: 'static>(channel: Snowflake, pre: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let bg = mix(p.background_textarea, p.background_tertiary, 90.0);
    row()
        // Above the typing rail.
        .pin(Some(PAD), None, Some(PAD), Some(dim::COMPOSER_STATUS_ROW_HEIGHT + 4.0))
        .min_h(44.0)
        .pl(16.0)
        .pr(6.0)
        .gap(12.0)
        .center_y()
        .radius(dim::RADIUS_FULL)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .key(format!("{pre}jump-present-bar"))
        .child(text(t("You're viewing older messages")).font(font(14.0, 500.0).lh(20.0)).color(p.text_secondary).truncate().grow(1.0).shrink().min_w(0.0))
        .child(
            Button::new(t("Jump to present"))
                .size(crate::ui::ButtonSize::Compact)
                .key(format!("{pre}jump-present"))
                .on_press(move |s: &mut S| {
                    let st = acc(s);
                    st.highlight = None;
                    st.intents.push(Intent::JumpToPresent(channel));
                })
                .el(p),
        )
}

fn pops<S: 'static>(acc: ShellAccess<S>) -> impl PopupAccess<S> {
    popup_access(move |s: &mut S| &mut acc(s).popups)
}

fn start_reply(st: &mut ShellState, store_me: Snowflake, m: &Message) {
    // `getDefaultReplyMention`: the author's "Reply mention preference" (their community one
    // first), else on; never for my own messages or outside a community.
    let member = m.member.as_ref().and_then(|g| g.mention_flags).map_or(0, |f| f.0);
    let pref = if member != 0 { member } else { m.author.mention_flags.map_or(0, |f| f.0) };
    let in_guild = m.guild_id.or(st.guild).is_some();
    let mention = m.author.id != store_me && in_guild && pref != 2;
    st.reply = Some(ReplyTo { channel: m.channel_id, message: m.id, mention });
    st.editing = None;
    st.composer.focused = true;
}

fn start_edit(st: &mut ShellState, m: &Message) {
    let mut e = TextEdit::new(m.content.clone());
    e.focused = true;
    st.editing = Some((m.id, e));
    st.composer.focused = false;
}

/// Shift skips the confirmation (`handleDelete(event.shiftKey)`).
fn request_delete(st: &mut ShellState, m: &Message, shift: bool) {
    if shift {
        st.intents.push(Intent::Delete { channel: m.channel_id, message: m.id });
    } else {
        st.confirm_delete = Some(m.id);
    }
}

fn toggle_reaction(st: &mut ShellState, m: &Message, emoji: ReactionEmoji) {
    let add = !m.reactions.iter().any(|r| r.me && r.emoji.id == emoji.id && (emoji.id.is_some() || r.emoji.name == emoji.name));
    st.intents.push(Intent::React { channel: m.channel_id, message: m.id, emoji, add });
}

/// `https://{webapp}/channels/{guild or @me}/{channel}/{message}`.
fn message_link(st: &ShellState, c: &Channel, m: &Message) -> String {
    let g = c.guild_id.map_or_else(|| "@me".to_string(), |g| g.to_string());
    format!("{}/channels/{g}/{}/{}", st.web_base.trim_end_matches('/'), c.id, m.id)
}

/// `MessageActionBar`: quick reactions, add reaction, edit (own) or reply, forward, thread and
/// More; with Shift held, the full layout. 28px items in a 2px-padded toolbar, its bottom 10px into the message.
fn action_bar<S: 'static>(m: &Message, c: &Channel, perms: Perms, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let thread_from = perms.thread_from.filter(|_| is_user_message(m));
    let id = m.id;
    let open = st.popups.open.as_deref();
    let button = |glyph: El, active: bool| {
        let fg = if active { p.text_primary } else { p.text_tertiary };
        let mut b = row().size(28.0, 28.0).center().radius(6.0).cursor(rune::Cursor::Pointer).role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary).hover_group().child(glyph.color(fg).group_color(p.text_primary));
        b = if active { b.fill(p.surface_interactive_selected_bg) } else { b.hover(p.surface_interactive_hover_bg) };
        b
    };
    let tip = |key: String, label: &str, trigger: El, layer: &mut Vec<El>| Tooltip::new(&key, label).el(trigger, &st.popups, pops(acc), p, layer);
    let mut bar = row()
        .padding(3.0)
        .center_y()
        .radius(8.0)
        .fill(p.form_surface_background)
        .border(1.0, over(p.background_modifier_accent, p.form_surface_background))
        .box_shadow(Shadow { x: 0.0, y: 6.0, blur: 14.0, spread: 0.0, color: rune::Color { a: 0.18, ..Color::BLACK }, inset: false });
    let (show, more_only, quick, expand) = crate::look::with(|l| (l.action_bar, l.action_bar_more_only, l.action_bar_quick_reactions, l.action_bar_shift_expand));
    // `showFullActions`: Shift held over the bar shows the menu's main items instead of More.
    let full = show && expand && !more_only && st.shift_held;
    let more_id = format!("msg-more-{id}");
    let more = |bar: El, layer: &mut Vec<El>| {
        let b = button(icon(&icons::DOTS_THREE_BOLD, 20.0), open == Some(&*more_id)).on_press(move |s: &mut S| {
            let st = acc(s);
            st.popups.toggle(&format!("msg-more-{id}"));
            st.active_msg = Some(id);
        });
        bar.child(tip(more_id.clone(), t("More"), b, layer))
    };
    if !show || more_only {
        return more(bar, layer).pin(None, Some(-24.0), Some(14.0), None);
    }
    if perms.react && quick && !full {
        for (name, glyph) in [("thumbsup", "👍"), ("ok_hand", "👌"), ("tada", "🎉")] {
            let m2 = m.clone();
            let b = button(text(glyph).font(font(18.0, 400.0).lh(20.0)), false).on_press(move |s: &mut S| {
                toggle_reaction(acc(s), &m2, ReactionEmoji { name: glyph.into(), ..Default::default() })
            });
            bar = bar.child(tip(format!("quick-{name}-{id}"), &format!(":{name}:"), b, layer));
        }
    }
    if full {
        if perms.dev {
            let mid = m.id;
            let b = button(icon(&icons::SNOWFLAKE_FILL, 20.0), false).on_press_tx(move |_: &mut S, tx: &mut Tx| tx.copy(mid.to_string()));
            bar = bar.child(tip(format!("copy-id-{id}"), t("Copy message ID"), b, layer));
        }
        let link = message_link(st, c, m);
        let b = button(icon(&icons::LINK_BOLD, 20.0), false).on_press_tx(move |_: &mut S, tx: &mut Tx| tx.copy(link.clone()));
        bar = bar.child(tip(format!("copy-link-{id}"), t("Copy message link"), b, layer));
        let ch = m.channel_id;
        let b = button(icon(&icons::ENVELOPE_OPEN_FILL, 20.0), false).on_press(move |s: &mut S| acc(s).intents.push(Intent::MarkUnread { channel: ch, message: id }));
        bar = bar.child(tip(format!("unread-{id}"), t("Mark as unread"), b, layer));
        if perms.pin && is_user_message(m) {
            let pin = !m.pinned;
            let (label, glyph) = if pin { ("Pin message", &icons::PUSH_PIN_FILL) } else { ("Unpin message", &icons::PUSH_PIN_SLASH_FILL) };
            let b = button(icon(glyph, 20.0), false).on_press(move |s: &mut S| acc(s).intents.push(Intent::Pin { channel: ch, message: id, pin }));
            bar = bar.child(tip(format!("pin-{id}"), t(label), b, layer));
        }
        if perms.can_edit(m) && perms.send {
            let (m2, me) = (m.clone(), perms.me);
            let b = button(icon(&icons::ARROW_BEND_UP_LEFT_FILL, 20.0), false).on_press(move |s: &mut S| start_reply(acc(s), me, &m2));
            bar = bar.child(tip(format!("reply-full-{id}"), t("Reply"), b, layer));
        }
    }
    if perms.react {
        let picking = open == Some(&*format!("emoji-{id}"));
        let ch = m.channel_id;
        let b = button(icon(&icons::SMILEY_FILL, 20.0), picking).on_press(move |s: &mut S| {
            let st = acc(s);
            if st.popups.is_open(&format!("emoji-{id}")) {
                st.popups.close();
            } else {
                super::expr::open_react(st, ch, id);
            }
        });
        bar = bar.child(tip(format!("react-{id}"), t("Add reaction"), b, layer));
    }
    if perms.can_edit(m) {
        let m2 = m.clone();
        let b = button(icon(&icons::PENCIL_SIMPLE_FILL, 20.0), false).on_press(move |s: &mut S| start_edit(acc(s), &m2));
        bar = bar.child(tip(format!("edit-{id}"), t("Edit message"), b, layer));
    } else if perms.send && is_user_message(m) {
        let (m2, me) = (m.clone(), perms.me);
        let b = button(icon(&icons::ARROW_BEND_UP_LEFT_FILL, 20.0), false).on_press(move |s: &mut S| start_reply(acc(s), me, &m2));
        bar = bar.child(tip(format!("reply-{id}"), t("Reply"), b, layer));
    }
    if let Some(parent) = thread_from {
        let b = button(icon(&icons::CHATS_FILL, 20.0), false).on_press(move |s: &mut S| super::threads::start_create(acc(s), parent, Some(id)));
        bar = bar.child(tip(format!("thread-{id}"), t("Create thread"), b, layer));
    }
    if full && perms.can_delete(m) {
        let m2 = m.clone();
        let b = button(icon(&icons::TRASH_FILL, 20.0).color(p.status_danger), false).on_press(move |s: &mut S| request_delete(acc(s), &m2, true));
        bar = bar.child(tip(format!("delete-{id}"), t("Delete message"), b, layer));
    } else {
        bar = more(bar, layer);
    }
    bar.pin(None, Some(-24.0), Some(14.0), None)
}

/// `MessageContextMenu`, for a right-click and the action bar's More.
// Forward, bookmarks, suppress embeds, publish, alt text and report come from
// `social::message_items`.
// shortcut: no media items (copy/save image, open link) when the right-click lands on an
// attachment; they need hooks in `shell/attachments.rs`.
fn message_menu<S: 'static>(store: &Store, menu: &str, m: &Message, c: &Channel, perms: Perms, st: &ShellState, acc: ShellAccess<S>) -> Menu<S> {
    let mut menu = Menu::new(menu).placement(Placement::LeftStart);
    let (id, ch) = (m.id, m.channel_id);
    if perms.react && is_user_message(m) {
        menu = menu
            .item(MenuItem::new("add-reaction", t("Add reaction")).icon(&icons::SMILEY_FILL).on_select(move |s: &mut S| {
                super::expr::open_react(acc(s), ch, id)
            }));
    }
    menu = super::expr::message_menu_items(menu, store, m, acc).separator();
    if perms.can_edit(m) {
        let m2 = m.clone();
        menu = menu.item(MenuItem::new("edit", t("Edit message")).icon(&icons::PENCIL_SIMPLE_FILL).on_select(move |s: &mut S| start_edit(acc(s), &m2)));
    }
    if perms.send && is_user_message(m) {
        let (m2, me) = (m.clone(), perms.me);
        menu = menu.item(MenuItem::new("reply", t("Reply")).icon(&icons::ARROW_BEND_UP_LEFT_FILL).on_select(move |s: &mut S| start_reply(acc(s), me, &m2)));
    }
    if let Some(parent) = perms.thread_from.filter(|_| is_user_message(m)) {
        menu = menu.item(MenuItem::new("create-thread", t("Create thread")).icon(&icons::CHATS_FILL).on_select(move |s: &mut S| super::threads::start_create(acc(s), parent, Some(id))));
    }
    menu = menu.separator();
    if !m.content.is_empty() {
        let t = m.content.clone();
        menu = menu.item(MenuItem::new("copy-text", crate::i18n::t("Copy text")).icon(&icons::CLIPBOARD_TEXT_FILL).on_select_tx(move |_: &mut S, tx| tx.copy(t.clone())));
    }
    // `speakMessage`: reads the raw content; a second press while speaking stops.
    if !m.content.trim().is_empty() {
        let (label, glyph) = if crate::desktop::tts::speaking() { ("Stop speaking", &icons::STOP_CIRCLE_FILL) } else { ("Speak message", &icons::SPEAKER_HIGH_FILL) };
        let text = m.content.clone();
        menu = menu.item(MenuItem::new("speak", t(label)).icon(glyph).on_select(move |_: &mut S| {
            if crate::desktop::tts::speaking() {
                crate::desktop::tts::stop();
            } else {
                crate::desktop::tts::speak(&text, crate::look::with(|l| l.tts_rate));
            }
        }));
    }
    if perms.pin && is_user_message(m) {
        let pin = !m.pinned;
        let (label, glyph) = if pin { ("Pin message", &icons::PUSH_PIN_FILL) } else { ("Unpin message", &icons::PUSH_PIN_SLASH_FILL) };
        let ch = m.channel_id;
        menu = menu.item(MenuItem::new("pin", label).icon(glyph).on_select(move |s: &mut S| acc(s).intents.push(Intent::Pin { channel: ch, message: id, pin })));
    }
    let ch = m.channel_id;
    menu = menu.item(MenuItem::new("mark-unread", t("Mark as unread")).icon(&icons::ENVELOPE_OPEN_FILL).on_select(move |s: &mut S| {
        acc(s).intents.push(Intent::MarkUnread { channel: ch, message: id })
    }));
    let link = message_link(st, c, m);
    menu = menu.item(MenuItem::new("copy-link", t("Copy message link")).icon(&icons::LINK_BOLD).on_select_tx(move |_: &mut S, tx| tx.copy(link.clone())));
    if perms.can_delete(m) {
        let m2 = m.clone();
        menu = menu.separator().item(
            MenuItem::new("delete", t("Delete message"))
                .icon(&icons::TRASH_FILL)
                .danger()
                .on_select_tx(move |s: &mut S, tx: &mut Tx| request_delete(acc(s), &m2, tx.mods().shift)),
        );
    }
    if !perms.dev {
        return menu;
    }
    // Developer mode only; Shift copies `channel-message`, as Fluxer's does.
    let (cid, mid) = (m.channel_id, m.id);
    menu.separator().item(MenuItem::new("copy-id", t("Copy message ID")).icon(&icons::SNOWFLAKE_FILL).on_select_tx(move |_: &mut S, tx: &mut Tx| {
        tx.copy(if tx.mods().shift { format!("{cid}-{mid}") } else { mid.to_string() })
    }))
    .item(super::debug::item("debug-message", t("Debug message"), super::debug::Subject::Message(cid, mid), acc))
}

/// `showDeleteConfirmation`: the `ConfirmModal` with a preview of the message.
fn delete_modal<S: 'static>(store: &Store, st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).confirm_delete = None;
    let md = Md::new(store, &st.media, store.channels.get(&m.channel_id).and_then(|c| c.guild_id), p, format!("del-m{}", m.id));
    let preview = col()
        .w_full()
        .max_h(220.0)
        .clip_rounded(6.0)
        .py(8.0)
        .radius(6.0)
        .fill(p.background_secondary)
        .border(1.0, over(p.background_header_secondary, p.background_secondary))
        .inert()
        .child(message(store, st, m, true, &md, p, acc));
    let tip = "Pro tip: Hold Shift while choosing this action to skip this confirmation.";
    let spans = vec![Span::new(0, 8).weight(700.0).color(p.accent_success)];
    let body = col()
        .w_full()
        .gap(16.0)
        .child(text(t("Delete this message? Can't be undone.")).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap())
        .child(preview)
        .child(rich(tip, spans).font(font(13.0, 400.0).lh(18.2)).color(p.text_primary_muted).wrap());
    let (ch, id) = (m.channel_id, m.id);
    Modal::new(t("Delete message"), close)
        .body(body)
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("delete-cancel").on_press(close).el(p))
                .child(
                    Button::new(t("Delete"))
                        .variant(ButtonVariant::Danger)
                        .key("delete-confirm")
                        .on_press(move |s: &mut S| {
                            let st = acc(s);
                            st.confirm_delete = None;
                            st.intents.push(Intent::Delete { channel: ch, message: id });
                        })
                        .el(p),
                ),
        )
        .el(p)
}

/// `.container` date divider: 20 tall, 16 above and below, lines on both sides of the date.
fn date_divider(ms: u64, p: &Palette) -> El {
    let line = || el().grow(1.0).h(2.0).fill(p.background_modifier_accent).opacity(0.3);
    row()
        .w_full()
        .py(16.0)
        .child(
            row()
                .w_full()
                .h(20.0)
                .px(PAD + 20.0)
                .center_y()
                .child(line())
                .child(text(format_full_date(ms)).font(font(12.0, 500.0)).color(p.text_tertiary).px(12.0))
                .child(line()),
        )
}

/// `.unreadContainer`: the red "NEW" line, in place of the group spacing.
fn new_divider(p: &Palette) -> El {
    row().w_full().h(GROUP_SPACING).px(PAD + GROUP_SPACING).center_y().child(el().grow(1.0).h(2.0).fill(p.status_danger).opacity(0.4)).child(
        row()
            .h(GROUP_SPACING)
            .px(8.0)
            .center()
            .radius(3.0)
            .fill(p.status_danger)
            .opacity(0.9)
            .child(text(t("NEW")).font(font(10.0, 700.0).tracking(0.5)).color(Color::WHITE)),
    )
}

pub(super) fn author_color(store: &Store, m: &Message, p: &Palette) -> Color {
    m.guild_id
        .or_else(|| store.channels.get(&m.channel_id).and_then(|c| c.guild_id))
        .and_then(|g| store.member_color(g, m.author.id))
        .map_or(p.text_primary, role_color)
}

pub(super) fn author_name(store: &Store, m: &Message) -> String {
    let g = store.channels.get(&m.channel_id).and_then(|c| c.guild_id);
    let n = store.display_name(g, m.author.id);
    if n.is_empty() { m.author.global_name.clone().unwrap_or_else(|| m.author.username.clone()) } else { n }
}

fn message<S: 'static>(store: &Store, st: &ShellState, m: &Message, head: bool, md: &Md, p: &Palette, acc: ShellAccess<S>) -> El {
    if m.kind == MessageType::THREAD_CREATED {
        return super::threads::created_message(store, m, &author_name(store, m), p, acc);
    }
    if m.kind == MessageType::THREAD_STARTER_MESSAGE {
        return super::threads::starter_message(store, st, m, p, acc);
    }
    if super::client_system::is(m) {
        return super::client_system::view(store, st, m, md, p, acc);
    }
    if !is_user_message(m) {
        return super::system_msg::view(store, st, m, p, acc);
    }
    // `.messageFailed`: text, links and mentions in the danger colour.
    let (red, md_red);
    let (md, p) = if st.failed.contains_key(&m.id) {
        red = Palette { text_chat: p.status_danger, text_link: p.status_danger, markup_mention_text: p.status_danger, ..*p };
        md_red = Md::new(store, md.media, md.guild, &red, "").body(format!("m{}", m.id), md.copy);
        (&md_red, &red)
    } else {
        (md, p)
    };
    if store.settings.message_display_compact {
        return super::compact::message(store, st, m, md, p, acc);
    }
    let ms = message_ms(m);
    let root = frame(store, st, m, col().w_full().pr(PAD + dim::MESSAGE_ACTION_BAR_OFFSET).py(dim::MESSAGE_SPACING_Y).hover_group(), p);
    let mut root = root;
    if head && m.kind == MessageType::REPLY {
        root = root.child(reply_preview(store, st, m, md, p, acc, PAD + dim::MESSAGE_AVATAR_SIZE / 2.0, CONTENT_X));
    }
    message_rest(store, st, m, head, md, p, acc, root, ms)
}

/// A message's background: jumped-to, mentioning me, or hover.
pub(super) fn frame(store: &Store, st: &ShellState, m: &Message, mut root: El, p: &Palette) -> El {
    let mentioned = m.mention_everyone || m.mentions.iter().any(|u| u.id == store.my_id());
    if st.highlight == Some(m.id) {
        // `.messageHighlight`: the jumped-to message.
        root = root
            .fill(p.message_reply_bg)
            .child(el().pin(Some(0.0), Some(0.0), None, Some(0.0)).w(dim::MESSAGE_HIGHLIGHT_BAR_WIDTH).fill(p.brand_primary_light));
    } else if mentioned {
        root = root
            .fill(p.message_mention_bg)
            .hover(mix(p.message_mention_bg_hover, Color::TRANSPARENT, 40.0))
            .child(el().pin(Some(0.0), Some(0.0), None, Some(0.0)).w(dim::MESSAGE_HIGHLIGHT_BAR_WIDTH).fill(p.message_mention_color));
    } else {
        root = root.hover(p.background_modifier_hover);
    }
    root
}

#[allow(clippy::too_many_arguments)]
fn message_rest<S: 'static>(store: &Store, st: &ShellState, m: &Message, head: bool, md: &Md, p: &Palette, acc: ShellAccess<S>, root: El, ms: u64) -> El {
    // Avatar column: the avatar on a group head, the hover time on the rest.
    // The avatar and the name line open the author's profile; right-click opens their menu.
    let (author, guild, mid) = (m.author.id, store.channels.get(&m.channel_id).and_then(|c| c.guild_id), m.id);
    let note = store.notes.get(&author).cloned().unwrap_or_default();
    let open = move |s: &mut S| acc(s).open_profile(author, guild, note.clone(), format!("avatar-{mid}"), Placement::RightStart);
    let menu = move |s: &mut S, at: rune::PointerPress, _: &mut Tx| {
        acc(s).open_user_menu(Target::User { user: author, guild, dm: None }, [at.x, at.y], format!("avatar-{mid}"), Placement::RightStart)
    };
    let gutter = if head {
        col().w(dim::MESSAGE_AVATAR_SIZE).pt(dim::MESSAGE_SPACING_Y + dim::MESSAGE_AVATAR_ALIGN_OFFSET).child(
            super::avatar_anim(store, st, m.author.id, dim::MESSAGE_AVATAR_SIZE, None, p.background_secondary_lighter, st.hover_msg == Some(m.id), p)
                .key(format!("avatar-{mid}"))
                .cursor(rune::Cursor::Pointer)
                .on_press(open.clone())
                .on_secondary_press(menu),
        )
    } else {
        row().w(dim::MESSAGE_AVATAR_SIZE).h(dim::MESSAGE_LINE_HEIGHT).justify_end().child(
            text(format_time(ms))
                .font(font(dim::MESSAGE_TIMESTAMP_COMPACT_FONT_SIZE, 500.0).lh(dim::MESSAGE_LINE_HEIGHT))
                .color(p.message_timestamp_color)
                .offset(8.0, 0.0)
                .group_opacity(0.0, 1.0),
        )
    };
    let mut content = col().grow(1.0).min_w(0.0);
    if head {
        // Username, then the dim 12px time on the same baseline.
        let name = author_name(store, m);
        let time = format_relative(ms, st.now_ms);
        let line = format!("{name}  {time}");
        let n = name.len();
        let spans = vec![
            Span::new(0, n).weight(500.0).color(author_color(store, m, p)),
            Span::new(n, n + 2).size(12.0),
            Span::new(n + 2, line.len()).size(dim::MESSAGE_TIMESTAMP_FONT_SIZE).color(mix(p.message_timestamp_color, Color::TRANSPARENT, 60.0)),
        ];
        // Only the name opens the profile (`.messageUsername`, underlined on hover).
        let name_link = |e: El, range: std::ops::Range<usize>| {
            let (open, menu) = (open.clone(), menu.clone());
            e.link(rune::Link::new(range, "name").hover(rune::LinkPaint::new().underline(author_color(store, m, p))))
                .on_link(move |s: &mut S, _: &str, _: &mut Tx| open(s))
                .on_link_secondary(move |s: &mut S, _: &str, at, tx: &mut Tx| menu(s, at, tx))
        };
        let role_icon = m.guild_id.or_else(|| store.channels.get(&m.channel_id).and_then(|c| c.guild_id)).and_then(|g| super::role_icon::icon_role(store, g, m.author.id)).and_then(|r| super::role_icon::view(st, r, 20.0));
        let line = match role_icon {
            // `.messageRoleIcon`: 4px after the name, the time after it.
            Some(ic) => row()
                .center_y()
                .min_w(0.0)
                .child(name_link(rich(name.clone(), spans[..1].to_vec()).font(font(16.0, 400.0).lh(dim::MESSAGE_LINE_HEIGHT)).truncate(), 0..n))
                .child(ic.ml(4.0))
                .child(rich(line[n..].to_string(), spans[1..].iter().map(|s| Span { start: s.start - n as u32, end: s.end - n as u32, ..s.clone() }).collect()).font(font(16.0, 400.0).lh(dim::MESSAGE_LINE_HEIGHT))),
            None => name_link(rich(line, spans), 0..n),
        };
        content = content.child(line.font(font(16.0, 400.0).lh(dim::MESSAGE_LINE_HEIGHT)).color(p.text_primary).truncate());
    }
    if let Some((_, edit)) = st.editing.as_ref().filter(|e| e.0 == m.id) {
        content = content.child(editor(edit, m, p, acc));
    } else if !m.content.is_empty() || m.edited_timestamp.is_some() {
        content = content.child(markdown::render_edited(&m.content, md, m.edited_timestamp.is_some().then(|| t("(edited)"))));
    }
    let acc_el = accessories(store, st, m, p, acc);
    if let Some(a) = acc_el {
        content = content.child(a);
    }
    // Previews (no copy map: pins, search, a thread's starter) have no chip, like `!isPreview`.
    if md.copy.is_some() {
        content = content.children(super::threads::chip(store, st, m, p, acc));
    }
    root.child(row().w_full().items_start().child(el().w(PAD)).child(gutter).child(el().w(dim::MESSAGE_GUTTER)).child(content))
}

/// `EditingMessageInput`: the message's text in a composer-like box, then "escape to cancel •
/// enter to save".
pub(super) fn editor<S: 'static>(edit: &TextEdit, m: &Message, p: &Palette, acc: ShellAccess<S>) -> El {
    let (ch, id, original) = (m.channel_id, m.id, m.content.clone());
    let field = text_input(edit, font(16.0, 400.0).lh(22.0))
        .key("edit-field")
        .w_full()
        .color(p.text_chat)
        .caret(p.text_primary, p.text_selection)
        // A stale field after the edit closed binds a throwaway draft (ID 0 matches no message).
        .on_edit(move |s: &mut S| -> &mut TextEdit { &mut acc(s).editing.get_or_insert_with(|| (Snowflake(0), TextEdit::new(""))).1 })
        .on_input_key(move |s: &mut S, k, _| {
            if k.key != Key::Escape {
                return false;
            }
            let st = acc(s);
            st.editing = None;
            st.composer.focused = true;
            true
        })
        .on_submit({
            let original = original.clone();
            move |s: &mut S, _| save_edit(acc(s), ch, id, &original)
        });
    let small = font(12.0, 400.0).lh(16.0);
    let footer = row()
        .py(8.0)
        .center_y()
        .child(text("escape to ").font(small).color(p.text_chat))
        .child(text("cancel").font(small).color(p.text_link).cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| acc(s).editing = None))
        .child(el().size(4.0, 4.0).radius(2.0).fill(p.text_chat_muted).mx(4.0))
        .child(text("enter to ").font(small).color(p.text_chat))
        .child(text("save").font(small).color(p.text_link).cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| save_edit(acc(s), ch, id, &original)));
    col()
        .w_full()
        .pt(4.0)
        .child(
            row()
                .w_full()
                .py(dim::FOOTER_BOX_PADDING_Y)
                .px(16.0)
                .radius(dim::FOOTER_BOX_RADIUS)
                .fill(p.background_textarea)
                .box_shadow(Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 1.0, color: p.floating_surface_ring_color_strong, inset: true })
                .cursor(rune::Cursor::Text)
                .child(field),
        )
        .child(footer)
}

/// Enter or "save": an emptied edit asks to delete the message instead, as in Fluxer; an
/// unchanged one just closes.
fn save_edit(st: &mut ShellState, channel: Snowflake, id: Snowflake, original: &str) {
    let Some((_, e)) = st.editing.take() else { return };
    st.composer.focused = true;
    let content = e.text.trim().to_string();
    if content.is_empty() {
        st.confirm_delete = Some(id);
    } else if content != original.trim() {
        st.intents.push(Intent::Edit { channel, message: id, content });
    }
}

/// What a reply's bar shows (fluxer `MessageReferences.getMessageReference`).
#[derive(Debug, PartialEq)]
enum Reference<'a> {
    Loaded(&'a Message),
    /// `referenced_message: null`, or deleted since.
    Deleted,
    /// Absent (search results leave it out) and not in the loaded messages.
    NotLoaded,
}

fn reference<'a>(store: &'a Store, m: &'a Message) -> Reference<'a> {
    let Some(r) = &m.message_reference else { return Reference::NotLoaded };
    let loaded = r.message_id.and_then(|id| store.messages(r.channel_id).iter().find(|x| x.id == id));
    match (&m.referenced_message, loaded) {
        (Some(None), _) => Reference::Deleted,
        // The loaded copy has later edits.
        (_, Some(x)) => Reference::Loaded(x),
        (Some(Some(x)), None) => Reference::Loaded(x),
        (None, None) => Reference::NotLoaded,
    }
}

/// The reply bar above a reply: spine from the avatar, mini avatar, name and a one-line preview.
/// `spine` / `content`: where the spine starts and the reply line begins (cozy: the avatar's
/// centre and [`CONTENT_X`]; compact: `repliedMessageCompact`).
#[allow(clippy::too_many_arguments)]
pub(super) fn reply_preview<S: 'static>(store: &Store, st: &ShellState, m: &Message, md: &Md, p: &Palette, acc: ShellAccess<S>, spine: f32, content: f32) -> El {
    let h = dim::MESSAGE_REPLY_HEIGHT;
    let spine_c = mix(p.text_chat_muted, Color::BLACK, 82.0);
    let w = dim::MESSAGE_REPLY_SPINE_WIDTH;
    // `.repliedMessage:before`: from the avatar centre to 3px short of the reply, curving down to
    // 3px under it. Rune borders draw every side, so the box is clipped to its top-left corner.
    let (x0, x1) = (spine, content - 3.0);
    let (y0, y1) = (h / 2.0 - w / 2.0, h + dim::MESSAGE_REPLY_SPACING + 3.0);
    let spine = el()
        .absolute(x0, y0)
        .size(x1 - x0, y1 - y0)
        .clip_rounded(0.0)
        .child(el().absolute(0.0, 0.0).size(x1 - x0 + 10.0, y1 - y0 + 10.0).corners([10.0, 0.0, 0.0, 0.0]).border(w, spine_c));
    let mut line = row().h(h).min_w(0.0).center_y().gap(0.0);
    let g = store.channels.get(&m.channel_id).and_then(|c| c.guild_id);
    match reference(store, m) {
        Reference::Loaded(r) => {
            let name = author_name(store, r);
            line = line
                .child(avatar(store, st, r.author.id, 16.0, None, p.background_secondary_lighter, p).mr(dim::MESSAGE_CONTAINER_GAP))
                .child(
                    text(name)
                        .font(font(dim::MESSAGE_REPLY_FONT_SIZE, 500.0).lh(h))
                        .color(mix(author_color(store, r, p), Color::TRANSPARENT, 64.0))
                        .mr(dim::MESSAGE_COMPACT_USERNAME_GAP - 2.0),
                );
            let preview = if r.content.is_empty() {
                text("Click to see attachment").font(font(14.0, 400.0).italic().lh(h)).color(p.text_primary_muted)
            } else {
                markdown::inline_text(&r.content, md, font(14.0, 400.0).lh(h), p.text_primary_muted)
            };
            // Clicking the preview jumps to the replied message.
            let (ch, id) = (m.channel_id, r.id);
            line = line.child(preview.shrink().min_w(0.0)).cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| super::jump(acc(s), g, ch, id));
        }
        missing => {
            let failed = missing == Reference::NotLoaded;
            line = line
                .child(
                    row()
                        .size(dim::MESSAGE_ICON_SIZE_MD, dim::MESSAGE_ICON_SIZE_MD)
                        .center()
                        .radius(8.0)
                        .fill(p.background_primary)
                        .mr(dim::MESSAGE_CONTAINER_GAP)
                        .child(icon(&icons::ARROW_BEND_UP_LEFT_BOLD, 10.0).color(p.text_primary)),
                )
                .child(
                    text(if failed { t("Original message failed to load") } else { t("Original message was deleted") })
                        .font(font(14.0, 400.0).italic().lh(h))
                        .color(p.text_primary_muted),
                );
            // A reply not loaded can still be jumped to.
            if let Some(id) = m.message_reference.as_ref().and_then(|r| r.message_id).filter(|_| failed) {
                let ch = m.message_reference.as_ref().map_or(m.channel_id, |r| r.channel_id);
                line = line.cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| super::jump(acc(s), g, ch, id));
            }
        }
    }
    row().w_full().h(h + dim::MESSAGE_REPLY_SPACING).items_start().child(spine).child(el().w(content)).child(line.shrink())
}

/// `.container`: invites, stickers, attachments, embeds, reactions; row-gap 4, padding-block 2.
pub(super) fn accessories<S: 'static>(store: &Store, st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let invites = super::invite_embed::embeds(store, st, m, p, acc);
    if invites.is_empty() && m.attachments.is_empty() && m.embeds.is_empty() && m.reactions.is_empty() && m.stickers.is_empty() && !st.progress.contains_key(&m.id) {
        return None;
    }
    let mut c = col().w_full().gap(dim::MESSAGE_CONTAINER_GAP).py(dim::MESSAGE_CONTAINER_PADDING_Y).items_start().children(invites);
    if !m.stickers.is_empty() {
        // `.stickersContainer`: wrapping, 8 apart, 4 down; each 160×160 (`.stickerWrapper`), contained.
        // `animate_stickers`: 0 always, 1 on hover, 2 never.
        let hovered = st.hover_msg == Some(m.id);
        // Reduced motion turns "always" into "on interaction".
        let mode = match store.settings.animate_stickers {
            0 if crate::look::reduced_motion() => 1,
            m => m,
        };
        let one = |s: &reflux_model::StickerItem| {
            let animated = s.animated && (mode == 0 || (mode == 1 && hovered));
            let img = match st.media.asset(|b, _| crate::media::sticker_url(b, s.id, animated)) {
                Load::Ready(px) => el().size(160.0, 160.0).pixels_fit(&px, rune::SurfaceFit::Fit),
                _ => el().size(160.0, 160.0),
            };
            let (key, s2) = (format!("sticker-{}-{}", m.id, s.id), s.clone());
            img.key(key.clone()).cursor(rune::Cursor::Pointer).on_press(move |st: &mut S| super::expr::open_sticker_info(acc(st), &s2, key.clone()))
        };
        c = c.child(row().flex_wrap().gaps(8.0, 8.0).mt(4.0).children(m.stickers.iter().map(one)));
    }
    c = c.children(super::attachments::attachments(store, st, m, p, acc));
    // An optimistic message's upload progress.
    if let Some(&f) = st.progress.get(&m.id) {
        c = c.child(row().w(432.0).h(4.0).radius(2.0).fill(p.background_modifier_accent).child(el().w(432.0 * f).h(4.0).radius(2.0).fill(p.brand_primary)));
    }
    if !m.flags.contains(MessageFlags::SUPPRESS_EMBEDS) && store.settings.render_embeds {
        c = c.children(super::embeds::embeds(store, st, m, p, acc));
    }
    if !m.reactions.is_empty() && store.settings.render_reactions {
        c = c.child(reactions(st, m, p, acc));
    }
    Some(c)
}

/// `fitMediaWithinBounds`: the size media draws at inside `max`, never upscaled.
pub(super) fn fit(w: u32, h: u32, max: (f32, f32)) -> (f32, f32) {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let k = (max.0 / w).min(max.1 / h).min(1.0);
    ((w * k).round(), (h * k).round())
}

/// The size an image attachment draws at: fit in 550×350 (`MediaDimensionConfig`).
pub fn media_size(w: u32, h: u32) -> (f32, f32) {
    fit(w, h, (550.0, 350.0))
}

/// `.reactionsGrid`.
fn reactions<S: 'static>(st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> El {
    let mut grid = row().flex_wrap().gaps(4.0, 0.0).pt(4.0).center_y();
    for r in &m.reactions {
        let (bg, border, fg) = if r.me {
            (p.brand_tint, p.brand_tint_border, p.text_on_brand_tint)
        } else {
            (p.background_secondary, Color::TRANSPARENT, p.text_tertiary)
        };
        // `.emoji`: a 20px image (custom, or unicode from the spritesheet), else its text.
        let load = match r.emoji.id {
            Some(id) => st.media.asset(|b, _| crate::media::emoji_url(b, &id.to_string())),
            None => emoji::sprite_index(&r.emoji.name).map_or(Load::Failed, |i| st.media.sprite(i)),
        };
        let glyph = match load {
            Load::Ready(px) => el().size(20.0, 20.0).my(2.0).pixels_fit(&px, rune::SurfaceFit::Fit),
            _ if r.emoji.id.is_some() => text(format!(":{}:", r.emoji.name)).font(font(16.0, 400.0).lh(20.0)),
            _ => text(r.emoji.name.clone()).font(font(16.0, 400.0).lh(20.0)),
        };
        let (m2, e) = (m.clone(), r.emoji.clone());
        let tip = super::expr::reactor_tip_key(&r.emoji);
        let pill = row()
                .key(format!("pill-{}-{tip}", m.id))
                .py(3.0)
                .px(7.0)
                .center_y()
                .radius(8.0)
                .fill(bg)
                .border(1.0, over(border, bg))
                .hover(if r.me { mix(p.brand_tint_hover, Color::TRANSPARENT, 50.0) } else { p.background_modifier_hover })
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .child(row().min_w(20.0).h(24.0).center().child(glyph))
                .child(text(r.count.to_string()).font(font(14.0, 600.0).lh(16.0)).color(fg).min_w(12.0).ml(6.0))
                .on_press(move |s: &mut S| toggle_reaction(acc(s), &m2, e.clone()));
        grid = grid.child(super::expr::pill_hover(pill, m.channel_id, m.id, tip, acc));
    }
    grid
}

/// `getTypingTierText`: past three typists, a count-based line (the playful tiers are
/// English-only).
fn typing_tier(count: usize, locale: &str) -> String {
    match count {
        _ if count <= 4 || !matches!(locale, "en-US" | "en-GB") => t("Several people are typing...").into(),
        5..=9 => "A handful of keyboard warriors are assembling...".into(),
        10..=14 => "A symphony of clacking keys is underway...".into(),
        15..=19 => "It's a full-blown typing fiesta in here".into(),
        _ => "Whoa, it's a typing apocalypse".into(),
    }
}

/// `TypingUsers` in the composer status row: blinking dots (`Typing`), up to five faces, and
/// `getRollingTypingText` with the names bold in their role colour.
fn typing_rail(store: &Store, st: &ShellState, guild: Option<Snowflake>, users: &[Snowflake], p: &Palette) -> El {
    let names: Vec<String> = users.iter().map(|u| store.display_name(guild, *u)).collect();
    let (line, spans) = if users.len() > 3 {
        (typing_tier(users.len(), crate::i18n::locale()), Vec::new())
    } else {
        let line = match names.as_slice() {
            [a] => crate::i18n::tf("{a} is typing...", &[("a", a)]),
            [a, b] => crate::i18n::tf("{a} and {b} are typing...", &[("a", a), ("b", b)]),
            [a, b, c] => crate::i18n::tf("{a}, {b} and {c} are typing...", &[("a", a), ("b", b), ("c", c)]),
            _ => String::new(),
        };
        // Bold each name where it lands in the translated line.
        let mut spans = Vec::new();
        let mut from = 0;
        for (u, n) in users.iter().zip(&names) {
            if let Some(at) = line[from..].find(n.as_str()).map(|i| i + from) {
                let color = guild.and_then(|g| store.member_color(g, *u)).map_or(p.text_primary_muted, role_color);
                spans.push(Span::new(at, at + n.len()).weight(600.0).color(color));
                from = at + n.len();
            }
        }
        (line, spans)
    };
    let dots = row().gap(2.0).center_y().children((0..3).map(|i| el().size(4.0, 4.0).radius(2.0).fill(p.text_primary_muted).looping(rune::LoopSpec::opacity(1.0, 0.25 * i as f64, 0.0, 1.0))));
    let mut faces = row().center_y().ml(6.0);
    for (i, u) in users.iter().take(5).enumerate() {
        let face = row().padding(1.0).radius(7.0).fill(p.background_secondary_lighter).child(super::avatar(store, st, *u, 12.0, None, p.background_secondary_lighter, p));
        faces = faces.child(if i == 0 { face } else { face.ml(-4.0) });
    }
    row()
        .pin(Some(PAD), None, Some(PAD), Some(0.0))
        .h(dim::COMPOSER_STATUS_ROW_HEIGHT)
        .center_y()
        .child(dots)
        .child(faces)
        .child(rich(line, spans).font(font(12.0, 400.0).lh(dim::COMPOSER_STATUS_LINE_HEIGHT)).color(p.text_primary_muted).truncate().ml(8.0).shrink().min_w(0.0))
        .inert()
}

// ---- Composer --------------------------------------------------------------------------------

/// Which composer: the main column's, the thread panel's, or the new-thread pane's (whose Enter
/// starts the thread in `c`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slot {
    Main,
    Pane,
    Create,
}

/// Runs a composer handler on the slot's draft: the panel's composer and tray are swapped into
/// the main fields for the duration, so every composer helper works on either.
fn in_slot<R>(st: &mut ShellState, slot: Slot, f: impl FnOnce(&mut ShellState) -> R) -> R {
    if slot == Slot::Main {
        return f(st);
    }
    std::mem::swap(&mut st.composer, &mut st.threads.composer);
    std::mem::swap(&mut st.uploads, &mut st.threads.uploads);
    let r = f(st);
    std::mem::swap(&mut st.composer, &mut st.threads.composer);
    std::mem::swap(&mut st.uploads, &mut st.threads.uploads);
    r
}

pub(super) fn composer<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>, slot: Slot) -> El {
    let draft = if slot == Slot::Main { &st.composer } else { &st.threads.composer };
    let button = |i: &'static rune::Icon, label: &str| {
        row()
            .size(32.0, 32.0)
            .center()
            .radius(dim::RADIUS_MD)
            .hover(p.background_modifier_hover)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label.to_string())
            .child(icon(i, 22.0).color(p.text_primary_muted).group_color(p.text_primary))
    };
    let post = c.kind.is_thread() && c.parent_id.and_then(|p| store.channels.get(&p)).is_some_and(|p| reflux_store::threads::is_forum(p.kind));
    let placeholder = match c.kind {
        _ if slot == Slot::Create => t("Enter a message to start the conversation!").to_string(),
        _ if post => crate::i18n::tf("Send a message in \"{name}\"", &[("name", &(c.name.clone().unwrap_or_default()).to_string())]),
        ChannelType::DM => crate::i18n::tf("Message @{userName}", &[("userName", &(channel_title(store, c)).to_string())]),
        ChannelType::GROUP_DM => crate::i18n::tf("Message {groupName}", &[("groupName", &(channel_title(store, c)).to_string())]),
        _ => crate::i18n::tf("Message #{channelName}", &[("channelName", &(c.name.clone().unwrap_or_default()).to_string())]),
    };
    let cid = c.id;
    // The newest own user message, for Up-arrow editing.
    let me = store.my_id();
    let last_own = store.messages(cid).iter().rev().find(|m| m.author.id == me && is_user_message(m) && !is_pending(m)).cloned().filter(|_| slot != Slot::Create);
    // The completions for the word at the caret, unless dismissed with Escape.
    let ac = autocomplete::trigger(&draft.text, draft.focus)
        .map(|t| {
            let items = autocomplete::completions(store, st, c, &t);
            (t, items)
        })
        .or_else(|| super::command_hints::hints(store, st, c, &draft.text, draft.focus))
        .filter(|(t, items)| draft.focused && !items.is_empty() && st.ac_dismissed.as_ref() != Some(&(t.start, t.query.clone())));
    let ac_keys = ac.clone();
    let (limit, slow) = (st.limits.message_length, if slot == Slot::Create { 0 } else { slowmode(store, c) });
    let suggested = match (slot, st.threads.panel) {
        (Slot::Create, Some(super::threads::Panel::Create { message: Some(m), .. })) => super::threads::suggested_name(store, c.guild_id, find(store, cid, m)),
        _ => String::new(),
    };
    let spelling = if st.prefs.spellcheck { crate::compose::unpilled(crate::desktop::spell::spans(&draft.text, draft.range().0, p.status_danger), draft) } else { Vec::new() };
    let field = text_input(draft, font(16.0, 400.0).lh(22.0))
        .key(match slot {
            Slot::Main => "composer",
            Slot::Pane => "thread-composer",
            Slot::Create => "thread-create-composer",
        })
        .decorations(spelling)
        .pill_colors(p.markup_mention_fill, Some(p.markup_mention_text))
        .pill_icon(crate::compose::pill_icon)
        // shortcut: the context menu edits the main composer only.
        .on_secondary_press(move |s: &mut S, at, _| {
            if slot == Slot::Main {
                crate::desktop::nagbar::open_spell_menu(acc(s), [at.x, at.y]);
            }
        })
        .w_full()
        .color(p.text_chat)
        .placeholder(placeholder, p.text_tertiary_secondary)
        .caret(p.text_primary, p.text_selection)
        .on_edit(move |s: &mut S| -> &mut rune::TextEdit {
            let st = acc(s);
            if slot == Slot::Main { &mut st.composer } else { &mut st.threads.composer }
        })
        .on_input_key(move |s: &mut S, k, _| {
            in_slot(acc(s), slot, |st| {
                // The create pane neither sends typing nor edits the last message.
                if slot == Slot::Create && matches!(k.key, Key::Text(_)) {
                    return false;
                }
                composer_key(st, cid, k, last_own.as_ref(), ac_keys.as_ref())
            })
        })
        // A pasted image joins the tray.
        .on_input_paste(move |s: &mut S, content, _| match content.image() {
            Some(img) if slot == Slot::Main => {
                super::upload::add_png(acc(s), img.png().to_vec());
                true
            }
            _ => false,
        })
        // Enter sends; Shift+Enter is the field's own newline.
        .on_submit({
            let mass = crate::social::mass_mention(store, c, &st.composer.text).filter(|_| slot == Slot::Main);
            move |s: &mut S, _| match (slot, mass) {
                (Slot::Create, _) => super::threads::submit_create(acc(s), &suggested),
                (Slot::Main, Some((mention, n))) => {
                    let st = acc(s);
                    st.social.mass = Some((cid, mention, n));
                    st.social.pending_send = (limit, slow);
                }
                _ => in_slot(acc(s), slot, |st| send(st, cid, limit, slow)),
            }
        });
    // shortcut: the thread panels have no upload tray; files go through the full view.
    let plus = button(&icons::PLUS_BOLD, t("Upload")).key("composer-plus").on_press(move |s: &mut S| acc(s).popups.toggle("composer-plus"));
    let plus = if slot == Slot::Main { plus } else { el().size(32.0, 32.0) };
    // `.textareaOuterRow`: inset 6, min-height 58, radius 8, ring; `.mainWrapperDense` grid with
    // 12px columns gaps, padding 13 / 10.
    let input_row = row()
        .w_full()
        .min_h(dim::FOOTER_BOX_HEIGHT)
        .py(dim::FOOTER_BOX_PADDING_Y)
        .px(dim::CHAT_HORIZONTAL_PADDING_DEFAULT - dim::FOOTER_BOX_INSET)
        .gap(dim::MESSAGE_GUTTER)
        .items_start()
        .cursor(rune::Cursor::Text)
        .child(row().px((dim::MESSAGE_AVATAR_SIZE - 32.0) / 2.0).child(plus))
        // `max-height: 50svh`: the field scrolls past half the window.
        .child(col().grow(1.0).min_w(0.0).pt((32.0 - 22.0) / 2.0).max_h(rune::memo::layout_size(super::BODY_KEY).map_or(400.0, |[_, h]| h / 2.0)).scroll(ScrollSpec { stick_bottom: true }).child(field))
        .child(
            row()
                .gap(dim::COMPOSER_ACTION_GAP)
                .center_y()
                .key("composer-expr")
                .children(super::expr::gif::enabled(st).then(|| button(&icons::GIF, t("GIFs")).on_press(move |s: &mut S| super::expr::toggle(acc(s), cid, super::expr::Tab::Gifs))))
                .child(button(&icons::IMAGE_SQUARE, "Memes").on_press(move |s: &mut S| super::expr::toggle(acc(s), cid, super::expr::Tab::Memes)))
                .child(button(&icons::STICKER, t("Stickers")).on_press(move |s: &mut S| super::expr::toggle(acc(s), cid, super::expr::Tab::Stickers)))
                .child(button(&icons::SMILEY, t("Emojis")).on_press(move |s: &mut S| super::expr::toggle(acc(s), cid, super::expr::Tab::Emojis))),
        );
    let mut boxed = col()
        .w_full()
        .radius(dim::FOOTER_BOX_RADIUS)
        .fill(p.background_textarea)
        .box_shadow(Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 1.0, color: if draft.focused && crate::look::with(|l| l.focus_ring) { p.floating_surface_ring_color_strong } else { p.floating_surface_ring_color }, inset: true });
    if let Some(r) = st.reply.filter(|r| r.channel == cid && slot != Slot::Create)
        && let Some(target) = find(store, cid, r.message)
    {
        boxed = boxed.child(reply_bar(store, target, r, p, acc));
    }
    if let Some(tray) = super::upload::tray(st, p, acc).filter(|_| slot == Slot::Main) {
        boxed = boxed.child(tray);
    }
    if let Some(sticker) = super::expr::pending_sticker(store, st, cid, p, acc).filter(|_| slot == Slot::Main) {
        boxed = boxed.child(sticker);
    }
    boxed = boxed.child(input_row);
    // `CharacterCounter`: from 80% of the limit; red under 50 left.
    let len = draft.text.chars().count();
    if len * 10 > limit * 8 {
        let left = limit as i64 - len as i64;
        boxed = boxed.child(
            text(left.to_string())
                .font(font(12.0, 400.0).mono())
                .color(if left < 50 { p.status_danger } else { p.text_tertiary })
                .label(if left < 0 { crate::i18n::tf("Character limit exceeded by {remaining}", &[("remaining", &(-left).to_string())]) } else { format!("{left} characters left") })
                .pin(None, None, Some(12.0), Some(4.0)),
        );
    }
    let mut out = col().w_full().px(dim::FOOTER_BOX_INSET).pb(dim::COMPOSER_BOX_INSET).fill(p.composer_surface_color);
    // The autocomplete list hangs above the box, as wide as it.
    if let Some((t, items)) = &ac {
        // shortcut: in the thread panels the list is keyboard-only (its clicks edit the main draft).
        let list = autocomplete::view(store, st, t, items, p, acc).pin(Some(0.0), None, Some(0.0), Some(8.0));
        out = out.child(el().w_full().h(0.0).child(if slot == Slot::Main { list } else { list.inert() }));
    }
    if slow > 0 {
        out = out.child(slowmode_label(st.cooldowns.get(&cid).map_or(0, |t| t.saturating_sub(st.now_ms)), p));
    }
    out.child(boxed)
}

/// The channel's slowmode in seconds, 0 if none or the user bypasses it.
fn slowmode(store: &Store, c: &Channel) -> u32 {
    let bypass = store.permissions(c.id).intersects(Permissions::MANAGE_MESSAGES | Permissions::MANAGE_CHANNELS);
    if bypass { 0 } else { c.rate_limit_per_user }
}

/// `SlowmodeIndicator`, right of the typing rail: "Slowmode is active (m:ss)" while cooling down.
fn slowmode_label(left_ms: u64, p: &Palette) -> El {
    let secs = left_ms.div_ceil(1000);
    let (line, color) = if secs > 0 {
        (crate::i18n::tf("Slowmode is active ({remaining})", &[("remaining", &format!("{}:{:02}", secs / 60, secs % 60))]), p.status_danger)
    } else {
        (t("Slowmode is enabled").to_string(), p.text_primary_muted)
    };
    el().w_full().h(0.0).child(
        row()
            .pin(None, None, Some(PAD - dim::FOOTER_BOX_INSET), Some(0.0))
            .h(dim::COMPOSER_STATUS_ROW_HEIGHT)
            .gap(4.0)
            .center_y()
            .inert()
            .child(icon(&icons::CLOCK_FILL, 12.0).color(color))
            .child(text(line).font(font(12.0, 500.0).lh(dim::COMPOSER_STATUS_LINE_HEIGHT)).color(color)),
    )
}

/// Enter: sends the text (after `/me`, `/spoiler`) with the tray's files, unless over the length
/// limit or cooling down from slowmode.
/// Sends after the @everyone confirmation, with the limits the composer had.
pub(super) fn send_now(st: &mut ShellState, channel: Snowflake) {
    let (limit, slow) = st.social.pending_send;
    send(st, channel, limit, slow);
}

fn send(st: &mut ShellState, channel: Snowflake, limit: usize, slowmode: u32) {
    let text = crate::compose::serialize(&st.composer);
    if crate::compose::run(st, channel, st.guild, &text) {
        return;
    }
    let mut content = autocomplete::apply_command(text.trim());
    if crate::look::with(|l| l.convert_emoticons) {
        content = super::emoticons::convert(&content);
    }
    let sticker = st.pending_sticker.is_some_and(|(c, _)| c == channel);
    if (content.is_empty() && st.uploads.is_empty() && !sticker) || st.cooldowns.get(&channel).is_some_and(|t| *t > st.now_ms) {
        return;
    }
    let len = content.chars().count();
    if len > limit {
        st.notice = Some(("Message is too long".into(), crate::i18n::tf("Character limit exceeded by {remaining}", &[("remaining", &(len - limit).to_string())])));
        return;
    }
    let reply = st.reply.take().filter(|r| r.channel == channel).map(|r| (r.message, r.mention));
    let attachments = std::mem::take(&mut st.uploads);
    st.intents.push(Intent::SendMessage { channel, content, reply, attachments });
    if slowmode > 0 {
        st.cooldowns.insert(channel, st.now_ms + slowmode as u64 * 1000);
    }
    st.composer.clear();
    st.typing = None; // `handleOwnMessageSent`
    st.ac_dismissed = None;
}

type Ac = (autocomplete::Trigger, Vec<autocomplete::Completion>);

/// The composer's own keys: the autocomplete's Up/Down/Tab/Enter/Escape when it is open, Escape
/// drops the reply, Up in an empty composer edits the last own message. The main composer's
/// typing notices come from [`super::typing::tick`]; a thread panel's start on a key press.
fn composer_key(st: &mut ShellState, channel: Snowflake, k: &rune::KeyEvent, last_own: Option<&Message>, ac: Option<&Ac>) -> bool {
    if let Some((t, items)) = ac {
        let n = items.len();
        match &k.key {
            Key::Up => {
                st.ac_index = (st.ac_index.min(n - 1) + n - 1) % n;
                return true;
            }
            Key::Down => {
                st.ac_index = (st.ac_index.min(n - 1) + 1) % n;
                return true;
            }
            Key::Tab | Key::Enter if !k.mods.shift => {
                autocomplete::accept(st, t, &items[st.ac_index.min(n - 1)]);
                return true;
            }
            Key::Escape => {
                st.ac_dismissed = Some((t.start, t.query.clone()));
                return true;
            }
            _ => {}
        }
    }
    match &k.key {
        Key::Escape if st.reply.is_some() => {
            st.reply = None;
            true
        }
        Key::Up if st.composer.text.is_empty() => match last_own {
            Some(m) => {
                start_edit(st, m);
                true
            }
            None => false,
        },
        Key::Text(_) if !k.mods.cmd => {
            st.ac_index = 0;
            if st.channel != Some(channel) {
                super::typing::start(st, channel);
            }
            false
        }
        _ => false,
    }
}

/// `ChannelReplyBar`: "Replying to **name**", the @ ON/OFF mention toggle and a close button, in
/// the composer's action stack (`.composerActionStack`).
fn reply_bar<S: 'static>(store: &Store, target: &Message, r: ReplyTo, p: &Palette, acc: ShellAccess<S>) -> El {
    let name = author_name(store, target);
    let (line, b) = crate::i18n::tf_marked("Replying to <0>{authorNickname}</0>", &[("authorNickname", &name)]);
    let spans = vec![Span::new(b.start, b.end).weight(600.0)];
    let mut controls = row().gap(2.0).center_y().min_h(32.0);
    if target.author.id != store.my_id() {
        let fg = if r.mention { rgb(96, 165, 250) } else { p.text_primary_muted };
        controls = controls.child(
            row()
                .w(56.0)
                .h(28.0)
                .px(8.0)
                .center()
                .radius(dim::RADIUS_FULL)
                .cursor(rune::Cursor::Pointer)
                .role("switch")
                .checked(r.mention)
                .label("Mention replied user")
                .hover_group()
                .child(icon(&icons::AT_BOLD, 16.0).color(fg).group_color(if r.mention { fg } else { p.text_primary }).mr(3.0))
                .child(text(if r.mention { "ON" } else { "OFF" }).font(font(12.0, 600.0)).color(fg).group_color(if r.mention { fg } else { p.text_primary }))
                .on_press(move |s: &mut S| {
                    if let Some(r) = &mut acc(s).reply {
                        r.mention ^= true;
                    }
                }),
        );
    }
    controls = controls.child(
        row()
            .size(32.0, 32.0)
            .center()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(t("Cancel reply"))
            .hover_group()
            .child(icon(&icons::X_CIRCLE_FILL, 16.0).color(p.text_primary_muted).group_color(p.text_primary))
            .on_press(move |s: &mut S| acc(s).reply = None),
    );
    let bg = mix(p.background_textarea, p.background_tertiary, 84.0);
    row()
        .w_full()
        .px(16.0)
        .center_y()
        .corners([dim::FOOTER_BOX_RADIUS, dim::FOOTER_BOX_RADIUS, 0.0, 0.0])
        .fill(bg)
        .child(rich(line, spans).font(font(13.0, 400.0).lh(16.25)).color(p.text_primary).truncate().grow(1.0).shrink().min_w(0.0))
        .child(controls)
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.floating_surface_ring_color))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflux_model::PartialUser;

    #[test]
    fn typing_tiers_follow_fluxer() {
        assert_eq!(typing_tier(4, "en-US"), "Several people are typing...");
        assert_eq!(typing_tier(5, "en-US"), "A handful of keyboard warriors are assembling...");
        assert_eq!(typing_tier(14, "en-GB"), "A symphony of clacking keys is underway...");
        assert_eq!(typing_tier(19, "en-US"), "It's a full-blown typing fiesta in here");
        assert_eq!(typing_tier(20, "en-US"), "Whoa, it's a typing apocalypse");
        assert_eq!(typing_tier(20, "ko"), t("Several people are typing..."), "tiers are English-only");
    }

    /// Found live: a reply in search results (which leave out `referenced_message`) said its
    /// original was deleted.
    #[test]
    fn reply_reference_resolution() {
        let mut store = Store::default();
        let (ch, orig) = (Snowflake(10), msg(20, 1, "2026-01-01T00:00:00Z"));
        let mut reply = msg(21, 1, "2026-01-01T00:01:00Z");
        reply.channel_id = ch;
        reply.message_reference = Some(reflux_model::MessageReference { channel_id: ch, message_id: Some(orig.id), ..Default::default() });
        assert_eq!(reference(&store, &reply), Reference::NotLoaded, "absent and not loaded");
        store.set_latest_messages(ch, vec![Message { channel_id: ch, ..orig.clone() }], 50);
        assert!(matches!(reference(&store, &reply), Reference::Loaded(r) if r.id == orig.id), "absent but loaded");
        reply.referenced_message = Some(None);
        assert_eq!(reference(&store, &reply), Reference::Deleted, "null means deleted");
    }

    fn msg(id: u64, author: u64, ts: &str) -> Message {
        Message {
            id: Snowflake(id),
            author: PartialUser { id: Snowflake(author), ..Default::default() },
            timestamp: ts.into(),
            ..Default::default()
        }
    }

    #[test]
    fn grouping_follows_fluxer() {
        let a = msg(1, 10, "2026-11-06T12:00:00Z");
        let b = msg(2, 10, "2026-11-06T12:06:59Z");
        let c = msg(3, 10, "2026-11-06T12:14:00Z");
        let d = msg(4, 11, "2026-11-06T12:14:10Z");
        assert!(is_new_group(None, &a));
        assert!(!is_new_group(Some(&a), &b), "same author within 7 min");
        assert!(is_new_group(Some(&b), &c), "over 7 min");
        assert!(is_new_group(Some(&c), &d), "other author");
        let mut r = msg(5, 11, "2026-11-06T12:14:20Z");
        r.kind = MessageType::REPLY;
        assert!(is_new_group(Some(&d), &r), "replies always start a group");
        let mut j = msg(6, 11, "2026-11-06T12:14:30Z");
        j.kind = MessageType::USER_JOIN;
        assert!(is_new_group(Some(&r), &j), "system after user");
    }

    #[test]
    fn stream_inserts_dates_and_spacing() {
        let ms = [
            msg(1, 10, "2026-11-05T12:00:00Z"),
            msg(2, 10, "2026-11-06T12:00:00Z"),
            msg(3, 10, "2026-11-06T12:01:00Z"),
            msg(4, 11, "2026-11-06T12:02:00Z"),
        ];
        let s = stream(&ms, Some(Snowflake(4)), GROUP_SPACING);
        assert!(matches!(s[0], Item::Date(_)));
        assert_eq!(s[1], Item::Msg { i: 0, head: true, space: 0.0, unread: false });
        assert!(matches!(s[2], Item::Date(_)), "new day");
        assert_eq!(s[3], Item::Msg { i: 1, head: true, space: 0.0, unread: false });
        assert_eq!(s[4], Item::Msg { i: 2, head: false, space: 0.0, unread: false });
        assert_eq!(s[5], Item::Msg { i: 3, head: true, space: 0.0, unread: true }, "the NEW line replaces the spacing");
    }

    #[test]
    fn composer_keys_throttle_typing_and_start_edits() {
        let ch = Snowflake(5);
        let key = |k: Key| rune::KeyEvent { key: k, mods: Default::default() };
        let mut st = ShellState { now_ms: 10_000, ..Default::default() };
        assert!(!composer_key(&mut st, ch, &key(Key::Text("a".into())), None, None), "typing still reaches the field");
        assert_eq!(st.typing.map(|t| t.channel), Some(ch), "a thread panel schedules its notice");
        let mut mine = msg(9, 1, "2026-11-06T12:00:00Z");
        mine.content = "hi".into();
        assert!(composer_key(&mut st, ch, &key(Key::Up), Some(&mine), None));
        assert_eq!(st.editing.as_ref().map(|e| (e.0, e.1.text.as_str())), Some((Snowflake(9), "hi")));
        st.composer = TextEdit::new("draft");
        st.editing = None;
        assert!(!composer_key(&mut st, ch, &key(Key::Up), Some(&mine), None), "Up only edits from an empty composer");

        st.reply = Some(ReplyTo { channel: ch, message: Snowflake(9), mention: true });
        assert!(composer_key(&mut st, ch, &key(Key::Escape), None, None));
        assert!(st.reply.is_none());

        // The autocomplete takes the arrows, Tab and Escape while open.
        st.composer = TextEdit::new("/sh");
        let t = crate::shell::autocomplete::trigger("/sh", 3).unwrap();
        let item = |i: &str| crate::shell::autocomplete::Completion { glyph: crate::shell::autocomplete::Glyph::Text("/"), label: i.into(), detail: String::new(), insert: format!("{i} ") };
        let ac = (t.clone(), vec![item("a"), item("b")]);
        assert!(composer_key(&mut st, ch, &key(Key::Up), None, Some(&ac)));
        assert_eq!(st.ac_index, 1, "Up wraps to the last");
        assert!(composer_key(&mut st, ch, &key(Key::Tab), None, Some(&ac)));
        assert_eq!(st.composer.text, "b ");
        assert!(composer_key(&mut st, ch, &key(Key::Escape), None, Some(&ac)));
        assert_eq!(st.ac_dismissed, Some((0, "sh".into())));
    }

    #[test]
    fn sending_applies_commands_limits_and_slowmode() {
        let ch = Snowflake(5);
        let mut st = ShellState { now_ms: 1000, limits: crate::shell::upload::Limits { message_length: 10, ..Default::default() }, ..Default::default() };
        st.composer = TextEdit::new("/me waves");
        send(&mut st, ch, 10, 30);
        assert_eq!(st.intents, [Intent::SendMessage { channel: ch, content: "_waves_".into(), reply: None, attachments: vec![] }]);
        assert_eq!(st.cooldowns[&ch], 31_000);
        st.composer = TextEdit::new("hi");
        send(&mut st, ch, 10, 30);
        assert_eq!(st.intents.len(), 1, "cooling down");
        st.now_ms = 31_000;
        st.composer = TextEdit::new("way too long text");
        send(&mut st, ch, 10, 0);
        assert_eq!(st.notice.as_ref().map(|n| n.0.as_str()), Some("Message is too long"));
        assert_eq!(st.composer.text, "way too long text", "kept for shortening");
    }

    #[test]
    fn message_links_parse_on_this_instance_only() {
        let web = "https://web.fluxer.app/";
        let p = |u: &str| crate::shell::parse_message_link(web, u);
        assert_eq!(p("https://web.fluxer.app/channels/@me/5/9"), Some((None, Snowflake(5), Snowflake(9))));
        assert_eq!(p("https://web.fluxer.app/channels/1/5/9"), Some((Some(Snowflake(1)), Snowflake(5), Snowflake(9))));
        assert_eq!(p("https://web.fluxer.app/channels/1/5"), None, "a channel link isn't a message");
        assert_eq!(p("https://example.com/channels/1/5/9"), None);
        assert_eq!(crate::shell::parse_message_link("", "https://x/channels/1/5/9"), None);
    }

    #[test]
    fn saving_an_edit() {
        let mut st = ShellState { channel: Some(Snowflake(5)), ..Default::default() };
        st.editing = Some((Snowflake(9), TextEdit::new(" hi ")));
        save_edit(&mut st, Snowflake(5), Snowflake(9), "hi");
        assert!(st.intents.is_empty() && st.editing.is_none(), "unchanged closes");
        st.editing = Some((Snowflake(9), TextEdit::new("  ")));
        save_edit(&mut st, Snowflake(5), Snowflake(9), "hi");
        assert_eq!(st.confirm_delete, Some(Snowflake(9)), "emptied asks to delete");
        st.editing = Some((Snowflake(9), TextEdit::new("hey")));
        save_edit(&mut st, Snowflake(5), Snowflake(9), "hi");
        assert_eq!(st.intents, [Intent::Edit { channel: Snowflake(5), message: Snowflake(9), content: "hey".into() }]);
    }

    #[test]
    fn relative_dates_and_media_size() {
        // Local noon, so the test holds in any timezone.
        let now = Local.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap().timestamp_millis() as u64;
        assert!(format_relative(now - 60_000, now).starts_with("Today at "));
        assert!(format_relative(now - 86_400_000, now).starts_with("Yesterday at "));
        assert!(format_relative(now - 5 * 86_400_000, now).contains("/2026, "));
        assert_eq!(media_size(1280, 720), (550.0, 309.0));
        assert_eq!(media_size(200, 100), (200.0, 100.0));
        assert_eq!(media_size(400, 1000), (140.0, 350.0));
    }
}

#[cfg(test)]
mod call_tests {
    #[test]
    fn call_durations_read_like_fluxer() {
        assert_eq!(super::super::system_msg::call_duration(30), "a few seconds");
        assert_eq!(super::super::system_msg::call_duration(60), "a minute");
        assert_eq!(super::super::system_msg::call_duration(3600 + 120), "1 hour and 2 minutes");
        assert_eq!(super::super::system_msg::call_duration(86_400 + 3600 + 60), "1 day, 1 hour, and a minute");
    }
}
