//! The main column: channel header, message list, composer and member list
//! (`ChannelHeader`, `ChannelMessages`, `ChannelDivider`, `theme/styles/Message.module.css`,
//! `MessageReactions`, `textarea/TextareaInput`).

use chrono::{DateTime, Datelike, Local, TimeZone};
use reflux_model::{
    Attachment, Channel, ChannelType, Embed, Message, MessageFlags, MessageType, Permissions, ReactionEmoji, Snowflake,
};
use reflux_store::Store;
use rune::text::Span;
use rune::{Color, El, GrowthSpec, Key, ScrollSpec, Shadow, TextEdit, Tx, col, el, icon, rich, row, text, text_input};

use super::markdown::{self, Md};
use crate::media::Load;
use super::{Intent, ReplyTo, ShellAccess, ShellState, Target, avatar, channel_icon, emoji, members, role_color};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::overlay::anchor;
use crate::ui::{Button, ButtonVariant, Menu, MenuItem, Modal, Placement, PopupAccess, Tooltip, mix, over, popup_access, rgb, scroller};

/// `MESSAGE_GROUP_TIMEOUT`.
const GROUP_MS: u64 = 7 * 60 * 1000;
/// `--message-group-spacing`.
const GROUP_SPACING: f32 = 16.0;
/// Avatar column start: `--chat-horizontal-padding`.
const PAD: f32 = dim::CHAT_HORIZONTAL_PADDING_DEFAULT;
/// Content column start: padding + avatar + gutter.
const CONTENT_X: f32 = PAD + dim::MESSAGE_AVATAR_SIZE + dim::MESSAGE_GUTTER;

pub fn main<S: 'static>(store: &Store, st: &ShellState, channel: Option<&Channel>, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some(c) = channel else {
        return col().grow(1.0).min_w(0.0).h_full().fill(p.background_secondary_lighter);
    };
    let show_members = st.member_list && c.guild_id.is_some();
    let chat = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .child(messages(store, st, c, p, acc, layer))
        .child(composer(store, st, c, p, acc));
    let mut content = row().w_full().grow(1.0).min_h(0.0).child(chat);
    if show_members {
        content = content
            .child(members::list(store, st, c, p, acc))
            .child(el().pin(None, Some(0.0), Some(dim::LAYOUT_MEMBER_LIST_WIDTH), Some(0.0)).w(1.0).fill(over(p.user_area_divider_color, p.background_secondary_lighter)));
    }
    if let Some(m) = st.confirm_delete.and_then(|id| find(store, c.id, id)) {
        layer.push(delete_modal(store, st, m, p, acc));
    }
    col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .fill(p.background_secondary_lighter)
        .child(header(store, st, c, p, acc))
        .child(content)
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

fn channel_title(store: &Store, c: &Channel) -> String {
    match c.kind {
        ChannelType::DM | ChannelType::GROUP_DM => c.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| {
            let me = store.my_id();
            c.recipients.iter().filter(|u| u.id != me).map(|u| store.display_name(None, u.id)).collect::<Vec<_>>().join(", ")
        }),
        _ => c.name.clone().unwrap_or_default(),
    }
}

fn header<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let muted = p.text_primary_muted;
    let action = |i: &'static rune::Icon, label: &str, selected: bool| {
        row()
            .size(dim::CHANNEL_HEADER_ACTION_SIZE, dim::CHANNEL_HEADER_ACTION_SIZE)
            .center()
            .radius(dim::RADIUS_FULL)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label.to_string())
            .child(icon(i, dim::CHANNEL_HEADER_ACTION_ICON_SIZE).color(if selected { p.text_primary } else { muted }).group_color(p.text_primary))
    };
    let mut left = row().grow(1.0).min_w(0.0).center_y();
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
                markdown::inline_text(topic, &Md { store, media: &st.media, guild: c.guild_id, p }, font(13.0, 400.0).lh(18.0), p.text_tertiary)
                    .grow(1.0)
                    .shrink()
                    .pr(dim::SPACING_5),
            );
    }
    let mut right = row().gap(dim::CHANNEL_HEADER_ACTIONS_GAP).center_y();
    if c.guild_id.is_some() {
        right = right
            .child(action(&icons::BELL, "Notification Settings", false))
            .child(action(&icons::PUSH_PIN, "Pinned Messages", false))
            .child(
                action(&icons::USERS, "Toggle Member List", st.member_list)
                    .key("toggle-members")
                    .on_press(move |s: &mut S| acc(s).member_list ^= true),
            );
    } else {
        right = right.child(action(&icons::PHONE, "Start Voice Call", false)).child(action(&icons::VIDEO_CAMERA, "Start Video Call", false));
    }
    // `MessageSearchBar`: 244×36, radius 12, 1px accent border, `background-textarea`.
    let search = row()
        .w(dim::MESSAGE_SEARCH_BAR_WIDTH)
        .h(dim::MESSAGE_SEARCH_BAR_HEIGHT)
        .px(dim::INPUT_CONTAINER_PADDING + 1.0)
        .gap(dim::MESSAGE_SEARCH_BAR_ICON_GAP)
        .center_y()
        .radius(dim::RADIUS_XL)
        .fill(p.background_textarea)
        .border(1.0, over(p.background_modifier_accent, p.background_textarea))
        .cursor(rune::Cursor::Text)
        .child(icon(&icons::MAGNIFYING_GLASS, dim::MESSAGE_SEARCH_BAR_ICON_SIZE).color(p.text_tertiary))
        .child(text("Search").font(font(14.0, 400.0).lh(20.0)).color(p.text_tertiary));
    right = right.child(search);
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

/// en-US `hour: numeric, minute: 2-digit` (12-hour).
// shortcut: always en-US 12-hour; follow the user's locale and time-format setting later.
pub fn format_time(ms: u64) -> String {
    local(ms).format("%-I:%M %p").to_string()
}

/// `getRelativeDateString`: "Today at 3:04 PM", "Yesterday at …", else "11/6/2026, 3:04 PM".
pub fn format_relative(ms: u64, now_ms: u64) -> String {
    let (d, now) = (local(ms), local(now_ms));
    let days = now.date_naive().signed_duration_since(d.date_naive()).num_days();
    match days {
        0 => format!("Today at {}", format_time(ms)),
        1 => format!("Yesterday at {}", format_time(ms)),
        _ => format!("{}/{}/{}, {}", d.month(), d.day(), d.year(), format_time(ms)),
    }
}

/// `getFormattedFullDate`: "Friday, November 6, 2026" (the date divider).
pub fn format_full_date(ms: u64) -> String {
    local(ms).format("%A, %B %-d, %Y").to_string()
}

pub fn format_full_date_time(ms: u64) -> String {
    format!("{} {}", format_full_date(ms), format_time(ms))
}

/// A message's time: its ISO timestamp, else its snowflake.
fn message_ms(m: &Message) -> u64 {
    DateTime::parse_from_rfc3339(&m.timestamp).map_or_else(|_| m.id.timestamp_ms(), |d| d.timestamp_millis() as u64)
}

fn same_day(a: u64, b: u64) -> bool {
    local(a).date_naive() == local(b).date_naive()
}

fn is_user_message(m: &Message) -> bool {
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
fn stream(msgs: &[Message], first_unread: Option<Snowflake>) -> Vec<Item> {
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
                if prev_sys && sys { GROUP_SPACING / 2.0 } else { GROUP_SPACING }
            }
            _ => 0.0,
        };
        last_kind = Some(sys);
        out.push(Item::Msg { i, head, space, unread });
    }
    out
}

fn messages<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let msgs = store.messages(c.id);
    let sticky = st.sticky_unread.filter(|s| s.0 == c.id).map(|s| s.1);
    let first_unread = if store.has_unread(c.id) {
        let ack = store.read_states.get(&c.id).and_then(|r| r.last_message_id).unwrap_or_default();
        msgs.iter().find(|m| m.id > ack && m.author.id != store.my_id()).map(|m| m.id)
    } else {
        sticky
    };
    let md = Md { store, media: &st.media, guild: c.guild_id, p };
    let perms = Perms::of(store, c);
    // The action bar's message: the hovered one, or the one whose menu or picker is open.
    let active = st.active_msg.filter(|_| st.popups.open.is_some());
    let items = stream(msgs, first_unread);
    let n_items = items.len();
    let mut rows = Vec::with_capacity(n_items + 1);
    for it in &items {
        rows.push(match *it {
            Item::Date(ms) => date_divider(ms, p).key(format!("date-{}", format_full_date(ms))),
            Item::Msg { i, head, space, unread } => {
                let m = &msgs[i];
                let mut r = col().w_full();
                if unread {
                    r = r.child(new_divider(p));
                } else if space > 0.0 {
                    r = r.pt(space);
                }
                let mut body = message(store, st, m, head, &md, p, acc);
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
                        });
                    let editing = st.editing.as_ref().is_some_and(|e| e.0 == id);
                    if !editing && (st.hover_msg == Some(id) || active == Some(id)) {
                        body = body.child(action_bar(m, perms, st, p, acc, layer));
                    }
                }
                r.child(body).key(format!("msg-{}", m.id))
            }
        });
    }
    if let Some(m) = active.and_then(|id| find(store, c.id, id)) {
        let open = st.popups.open.clone().unwrap_or_default();
        if open == format!("msg-menu-{}", m.id) || open == format!("msg-more-{}", m.id) {
            message_menu(&open, m, c, perms, st, acc).el(&st.popups, pops(acc), p, layer);
        } else if open == format!("emoji-{}", m.id) {
            let close = move |s: &mut S| {
                let st = acc(s);
                st.popups.close();
                st.active_msg = None;
            };
            let picker = emoji::picker(st, c.id, m.id, p, acc).modal(true, close);
            layer.push(anchor(picker, &format!("react-{}", m.id), Placement::LeftStart, 8.0));
        }
    }
    let more = store.messages.get(&c.id).is_some_and(|w| w.has_more_before);
    let cid = c.id;
    let list = col()
        .w_full()
        .key(format!("msgs-{cid}"))
        .growth(GrowthSpec::default())
        .virtualize(48.0)
        .children(rows)
        // `--messages-bottom-clearance`: room for the typing rail over the list's end.
        .child(el().h(dim::MESSAGES_BOTTOM_CLEARANCE).key("msgs-end"))
        .on_visible_rows(move |s: &mut S, r| {
            let st = acc(s);
            if r.start == 0 && more && !st.intents.contains(&Intent::LoadOlder { channel: cid }) {
                st.intents.push(Intent::LoadOlder { channel: cid });
            }
            // The clearance row is the last; seeing it means the newest message is on screen.
            let bottom = r.end > n_items;
            let changed = st.at_bottom != bottom;
            st.at_bottom = bottom;
            changed
        });
    let viewport = col().w_full().grow(1.0).min_h(0.0).justify_end().scroll(ScrollSpec { stick_bottom: true }).child(list);
    let mut area = col().w_full().grow(1.0).min_h(0.0).child(scroller(viewport, true, p));
    let typing = store.typing_users(c.id, st.now_ms);
    if !typing.is_empty() {
        area = area.child(typing_rail(store, c.guild_id, &typing, p));
    }
    area
}

fn pops<S: 'static>(acc: ShellAccess<S>) -> impl PopupAccess<S> {
    popup_access(move |s: &mut S| &mut acc(s).popups)
}

fn start_reply(st: &mut ShellState, store_me: Snowflake, m: &Message) {
    st.reply = Some(ReplyTo { channel: m.channel_id, message: m.id, mention: m.author.id != store_me });
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

/// `MessageActionBar` (default layout): quick reactions, add reaction, edit (own) or reply,
/// and More. 28px items in a 2px-padded toolbar, its bottom 10px into the message.
// shortcut: the Shift-held full layout (copy ID/link, pin, mark unread, delete) isn't shown;
// those stay in More.
fn action_bar<S: 'static>(m: &Message, perms: Perms, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let id = m.id;
    let open = st.popups.open.as_deref();
    let button = |glyph: El, active: bool| {
        let fg = if active { p.text_primary } else { p.text_tertiary };
        let mut b = row().size(28.0, 28.0).center().radius(6.0).cursor(rune::Cursor::Pointer).role("button").hover_group().child(glyph.color(fg).group_color(p.text_primary));
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
    if perms.react {
        for (name, glyph) in [("thumbsup", "👍"), ("ok_hand", "👌"), ("tada", "🎉")] {
            let m2 = m.clone();
            let b = button(text(glyph).font(font(18.0, 400.0).lh(20.0)), false).on_press(move |s: &mut S| {
                toggle_reaction(acc(s), &m2, ReactionEmoji { name: glyph.into(), ..Default::default() })
            });
            bar = bar.child(tip(format!("quick-{name}-{id}"), &format!(":{name}:"), b, layer));
        }
        let picking = open == Some(&*format!("emoji-{id}"));
        let b = button(icon(&icons::SMILEY_FILL, 20.0), picking).on_press(move |s: &mut S| {
            let st = acc(s);
            st.popups.toggle(&format!("emoji-{id}"));
            st.active_msg = Some(id);
            st.emoji_query = TextEdit::new("");
        });
        bar = bar.child(tip(format!("react-{id}"), "Add reaction", b, layer));
    }
    if perms.can_edit(m) {
        let m2 = m.clone();
        let b = button(icon(&icons::PENCIL_SIMPLE_FILL, 20.0), false).on_press(move |s: &mut S| start_edit(acc(s), &m2));
        bar = bar.child(tip(format!("edit-{id}"), "Edit message", b, layer));
    } else if perms.send && is_user_message(m) {
        let (m2, me) = (m.clone(), perms.me);
        let b = button(icon(&icons::ARROW_BEND_UP_LEFT_FILL, 20.0), false).on_press(move |s: &mut S| start_reply(acc(s), me, &m2));
        bar = bar.child(tip(format!("reply-{id}"), "Reply", b, layer));
    }
    let more_id = format!("msg-more-{id}");
    let b = button(icon(&icons::DOTS_THREE_BOLD, 20.0), open == Some(&*more_id)).on_press(move |s: &mut S| {
        let st = acc(s);
        st.popups.toggle(&format!("msg-more-{id}"));
        st.active_msg = Some(id);
    });
    bar = bar.child(tip(more_id, "More", b, layer));
    bar.pin(None, Some(-24.0), Some(14.0), None)
}

/// `MessageContextMenu`, for a right-click and the action bar's More.
// shortcut: no forward, threads, bookmarks, speak, suppress embeds, remove reactions, report or
// media items.
fn message_menu<S: 'static>(menu: &str, m: &Message, c: &Channel, perms: Perms, st: &ShellState, acc: ShellAccess<S>) -> Menu<S> {
    let mut menu = Menu::new(menu).placement(Placement::LeftStart);
    let id = m.id;
    if perms.react && is_user_message(m) {
        menu = menu
            .item(MenuItem::new("add-reaction", "Add reaction").icon(&icons::SMILEY_FILL).on_select(move |s: &mut S| {
                let st = acc(s);
                st.popups.open = Some(format!("emoji-{id}"));
                st.active_msg = Some(id);
                st.emoji_query = TextEdit::new("");
            }))
            .separator();
    }
    if perms.can_edit(m) {
        let m2 = m.clone();
        menu = menu.item(MenuItem::new("edit", "Edit message").icon(&icons::PENCIL_SIMPLE_FILL).on_select(move |s: &mut S| start_edit(acc(s), &m2)));
    }
    if perms.send && is_user_message(m) {
        let (m2, me) = (m.clone(), perms.me);
        menu = menu.item(MenuItem::new("reply", "Reply").icon(&icons::ARROW_BEND_UP_LEFT_FILL).on_select(move |s: &mut S| start_reply(acc(s), me, &m2)));
    }
    menu = menu.separator();
    if !m.content.is_empty() {
        let t = m.content.clone();
        menu = menu.item(MenuItem::new("copy-text", "Copy text").icon(&icons::CLIPBOARD_TEXT_FILL).on_select_tx(move |_: &mut S, tx| tx.copy(t.clone())));
    }
    if perms.pin && is_user_message(m) {
        let pin = !m.pinned;
        let (label, glyph) = if pin { ("Pin message", &icons::PUSH_PIN_FILL) } else { ("Unpin message", &icons::PUSH_PIN_SLASH_FILL) };
        let ch = m.channel_id;
        menu = menu.item(MenuItem::new("pin", label).icon(glyph).on_select(move |s: &mut S| acc(s).intents.push(Intent::Pin { channel: ch, message: id, pin })));
    }
    let ch = m.channel_id;
    menu = menu.item(MenuItem::new("mark-unread", "Mark as unread").icon(&icons::ENVELOPE_OPEN_FILL).on_select(move |s: &mut S| {
        acc(s).intents.push(Intent::MarkUnread { channel: ch, message: id })
    }));
    let link = message_link(st, c, m);
    menu = menu.item(MenuItem::new("copy-link", "Copy message link").icon(&icons::LINK_BOLD).on_select_tx(move |_: &mut S, tx| tx.copy(link.clone())));
    if perms.can_delete(m) {
        let m2 = m.clone();
        menu = menu.separator().item(
            MenuItem::new("delete", "Delete message")
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
    menu.separator().item(MenuItem::new("copy-id", "Copy message ID").icon(&icons::SNOWFLAKE_FILL).on_select_tx(move |_: &mut S, tx: &mut Tx| {
        tx.copy(if tx.mods().shift { format!("{cid}-{mid}") } else { mid.to_string() })
    }))
}

/// `showDeleteConfirmation`: the `ConfirmModal` with a preview of the message.
fn delete_modal<S: 'static>(store: &Store, st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).confirm_delete = None;
    let md = Md { store, media: &st.media, guild: store.channels.get(&m.channel_id).and_then(|c| c.guild_id), p };
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
        .child(text("Delete this message? Can't be undone.").font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap())
        .child(preview)
        .child(rich(tip, spans).font(font(13.0, 400.0).lh(18.2)).color(p.text_primary_muted).wrap());
    let (ch, id) = (m.channel_id, m.id);
    Modal::new("Delete message", close)
        .body(body)
        .footer(
            row()
                .gap(8.0)
                .child(Button::new("Cancel").variant(ButtonVariant::Secondary).key("delete-cancel").on_press(close).el(p))
                .child(
                    Button::new("Delete")
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
            .child(text("NEW").font(font(10.0, 700.0).tracking(0.5)).color(Color::WHITE)),
    )
}

fn author_color(store: &Store, m: &Message, p: &Palette) -> Color {
    m.guild_id
        .or_else(|| store.channels.get(&m.channel_id).and_then(|c| c.guild_id))
        .and_then(|g| store.member_color(g, m.author.id))
        .map_or(p.text_primary, role_color)
}

fn author_name(store: &Store, m: &Message) -> String {
    let g = store.channels.get(&m.channel_id).and_then(|c| c.guild_id);
    let n = store.display_name(g, m.author.id);
    if n.is_empty() { m.author.global_name.clone().unwrap_or_else(|| m.author.username.clone()) } else { n }
}

fn message<S: 'static>(store: &Store, st: &ShellState, m: &Message, head: bool, md: &Md, p: &Palette, acc: ShellAccess<S>) -> El {
    if !is_user_message(m) {
        return system_message(store, m, md, p);
    }
    let ms = message_ms(m);
    let mentioned = m.mention_everyone || m.mentions.iter().any(|u| u.id == store.my_id());
    let mut root = col().w_full().pr(PAD + dim::MESSAGE_ACTION_BAR_OFFSET).py(dim::MESSAGE_SPACING_Y).hover_group();
    if mentioned {
        root = root
            .fill(p.message_mention_bg)
            .hover(mix(p.message_mention_bg_hover, Color::TRANSPARENT, 40.0))
            .child(el().pin(Some(0.0), Some(0.0), None, Some(0.0)).w(dim::MESSAGE_HIGHLIGHT_BAR_WIDTH).fill(p.message_mention_color));
    } else {
        root = root.hover(p.background_modifier_hover);
    }
    if head && m.kind == MessageType::REPLY {
        root = root.child(reply_preview(store, st, m, md, p));
    }

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
            avatar(store, st, m.author.id, dim::MESSAGE_AVATAR_SIZE, None, p.background_secondary_lighter, p)
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
        // shortcut: the whole name line is the click target, not just the name.
        content = content.child(
            rich(line, spans)
                .font(font(16.0, 400.0).lh(dim::MESSAGE_LINE_HEIGHT))
                .color(p.text_primary)
                .truncate()
                .cursor(rune::Cursor::Pointer)
                .on_press(open)
                .on_secondary_press(menu),
        );
    }
    if let Some((_, edit)) = st.editing.as_ref().filter(|e| e.0 == m.id) {
        content = content.child(editor(edit, m, p, acc));
    } else if !m.content.is_empty() {
        content = content.child(markdown::render(&m.content, md));
    }
    if m.edited_timestamp.is_some() && st.editing.as_ref().is_none_or(|e| e.0 != m.id) {
        // shortcut: "(edited)" sits on its own line under the body instead of inline after it.
        content = content.child(text("(edited)").font(font(dim::MESSAGE_EDITED_LABEL_FONT_SIZE, 400.0).lh(16.0)).color(p.message_timestamp_color));
    }
    let acc_el = accessories(store, st, m, p, acc);
    if let Some(a) = acc_el {
        content = content.child(a);
    }
    root.child(row().w_full().items_start().child(el().w(PAD)).child(gutter).child(el().w(dim::MESSAGE_GUTTER)).child(content))
}

/// `EditingMessageInput`: the message's text in a composer-like box, then "escape to cancel •
/// enter to save".
fn editor<S: 'static>(edit: &TextEdit, m: &Message, p: &Palette, acc: ShellAccess<S>) -> El {
    let (id, original) = (m.id, m.content.clone());
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
            move |s: &mut S, _| save_edit(acc(s), id, &original)
        });
    let small = font(12.0, 400.0).lh(16.0);
    let footer = row()
        .py(8.0)
        .center_y()
        .child(text("escape to ").font(small).color(p.text_chat))
        .child(text("cancel").font(small).color(p.text_link).cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| acc(s).editing = None))
        .child(el().size(4.0, 4.0).radius(2.0).fill(p.text_chat_muted).mx(4.0))
        .child(text("enter to ").font(small).color(p.text_chat))
        .child(text("save").font(small).color(p.text_link).cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| save_edit(acc(s), id, &original)));
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
fn save_edit(st: &mut ShellState, id: Snowflake, original: &str) {
    let Some((_, e)) = st.editing.take() else { return };
    st.composer.focused = true;
    let content = e.text.trim().to_string();
    if content.is_empty() {
        st.confirm_delete = Some(id);
    } else if content != original.trim() {
        let channel = st.channel.unwrap_or_default();
        st.intents.push(Intent::Edit { channel, message: id, content });
    }
}

/// The reply bar above a reply: spine from the avatar, mini avatar, name and a one-line preview.
fn reply_preview(store: &Store, st: &ShellState, m: &Message, md: &Md, p: &Palette) -> El {
    let h = dim::MESSAGE_REPLY_HEIGHT;
    let spine_c = mix(p.text_chat_muted, Color::BLACK, 82.0);
    let w = dim::MESSAGE_REPLY_SPINE_WIDTH;
    // `.repliedMessage:before`: from the avatar centre to 3px short of the reply, curving down to
    // 3px under it. Rune borders draw every side, so the box is clipped to its top-left corner.
    let (x0, x1) = (PAD + dim::MESSAGE_AVATAR_SIZE / 2.0, CONTENT_X - 3.0);
    let (y0, y1) = (h / 2.0 - w / 2.0, h + dim::MESSAGE_REPLY_SPACING + 3.0);
    let spine = el()
        .absolute(x0, y0)
        .size(x1 - x0, y1 - y0)
        .clip_rounded(0.0)
        .child(el().absolute(0.0, 0.0).size(x1 - x0 + 10.0, y1 - y0 + 10.0).corners([10.0, 0.0, 0.0, 0.0]).border(w, spine_c));
    let mut line = row().h(h).min_w(0.0).center_y().gap(0.0);
    match m.referenced_message.as_ref().and_then(|r| r.as_deref()) {
        Some(r) => {
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
            line = line.child(preview.shrink().min_w(0.0));
        }
        None => {
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
                .child(text("Original message was deleted").font(font(14.0, 400.0).italic().lh(h)).color(p.text_primary_muted));
        }
    }
    row().w_full().h(h + dim::MESSAGE_REPLY_SPACING).items_start().child(spine).child(el().w(CONTENT_X)).child(line.shrink())
}

/// System messages: an icon in the avatar column and one line of muted text.
// shortcut: covers joins, pins and recipient changes with generic wording and icons.
fn system_message(store: &Store, m: &Message, _md: &Md, p: &Palette) -> El {
    let name = author_name(store, m);
    let (glyph, color, body) = match m.kind {
        MessageType::USER_JOIN => (&icons::ARROW_RIGHT_BOLD, p.status_online, format!("{name} joined the community.")),
        MessageType::CHANNEL_PINNED_MESSAGE => (&icons::PUSH_PIN_FILL, p.text_primary_muted, format!("{name} pinned a message to this channel.")),
        MessageType::RECIPIENT_ADD => (&icons::ARROW_RIGHT_BOLD, p.status_online, format!("{name} added someone to the group.")),
        MessageType::RECIPIENT_REMOVE => (&icons::ARROW_LEFT_BOLD, p.status_danger, format!("{name} left the group.")),
        MessageType::CHANNEL_NAME_CHANGE => (&icons::PENCIL_SIMPLE_BOLD, p.text_primary_muted, format!("{name} changed the channel name: {}", m.content)),
        MessageType::CALL => (&icons::PHONE_FILL, p.status_online, format!("{name} started a call.")),
        _ => (&icons::INFO_BOLD, p.text_primary_muted, if m.content.is_empty() { format!("{name} did something.") } else { m.content.clone() }),
    };
    row()
        .w_full()
        .pr(PAD + dim::MESSAGE_ACTION_BAR_OFFSET)
        .py(dim::MESSAGE_SPACING_Y)
        .items_start()
        .hover(p.background_modifier_hover)
        .child(el().w(PAD))
        .child(row().w(dim::MESSAGE_AVATAR_SIZE).h(dim::MESSAGE_LINE_HEIGHT).justify_end().child(icon(glyph, dim::SYSTEM_MESSAGE_ICON_SIZE).color(color).opacity(0.6 + 0.4 * (color != p.text_primary_muted) as u8 as f32)))
        .child(el().w(dim::MESSAGE_GUTTER))
        .child(
            text(body)
                .font(font(16.0, 400.0).lh(dim::MESSAGE_LINE_HEIGHT))
                .color(p.text_chat_muted)
                .wrap()
                .grow(1.0)
                .shrink(),
        )
}

/// `.container`: attachments, embeds, reactions; row-gap 4, padding-block 2.
fn accessories<S: 'static>(store: &Store, st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if m.attachments.is_empty() && m.embeds.is_empty() && m.reactions.is_empty() {
        return None;
    }
    let mut c = col().w_full().gap(dim::MESSAGE_CONTAINER_GAP).py(dim::MESSAGE_CONTAINER_PADDING_Y).items_start();
    for a in &m.attachments {
        c = c.child(attachment(st, a, p));
    }
    if !m.flags.contains(MessageFlags::SUPPRESS_EMBEDS) {
        for e in &m.embeds {
            c = c.child(embed(e, store, st, m, p));
        }
    }
    if !m.reactions.is_empty() {
        c = c.child(reactions(st, m, p, acc));
    }
    Some(c)
}

/// `fitMediaWithinBounds`: the size media draws at inside `max`, never upscaled.
fn fit(w: u32, h: u32, max: (f32, f32)) -> (f32, f32) {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let k = (max.0 / w).min(max.1 / h).min(1.0);
    ((w * k).round(), (h * k).round())
}

/// The size an image attachment draws at: fit in 550×350 (`MediaDimensionConfig`).
pub fn media_size(w: u32, h: u32) -> (f32, f32) {
    fit(w, h, (550.0, 350.0))
}

/// An image (or a video's poster frame) through the media proxy at its drawn size.
fn media(st: &ShellState, proxy_url: Option<&str>, (w, h): (f32, f32), p: &Palette) -> El {
    let load = match proxy_url {
        Some(u) => st.media.get(&crate::media::fit_url(u, w, st.media.scale())),
        None => Load::Failed,
    };
    if matches!(load, Load::Failed) {
        return row().size(w, h).center().radius(dim::MEDIA_BORDER_RADIUS).fill(p.background_secondary_alt).child(icon(&icons::IMAGE, 32.0).color(p.text_tertiary_muted));
    }
    super::picture(load, w, h, dim::MEDIA_BORDER_RADIUS, p)
}

fn attachment(st: &ShellState, a: &Attachment, p: &Palette) -> El {
    if let (Some(w), Some(h), true) = (a.width, a.height, a.content_type.starts_with("image/") || a.content_type.starts_with("video/")) {
        // shortcut: videos show their first frame without a player.
        return media(st, a.proxy_url.as_deref().or(a.url.as_deref()), media_size(w, h), p);
    }
    // A file card.
    // shortcut: approximates `AttachmentFile` (no download button or type-specific icons).
    let size = if a.size >= 1 << 20 { format!("{:.2} MB", a.size as f64 / (1 << 20) as f64) } else { format!("{:.2} KB", a.size as f64 / 1024.0) };
    row()
        .w(432.0)
        .padding(13.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .fill(p.background_secondary)
        .border(1.0, over(p.background_modifier_accent, p.background_secondary))
        .child(icon(&icons::FILE_TEXT, 30.0).color(p.text_tertiary))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .child(text(a.filename.clone()).font(font(16.0, 500.0).lh(20.0)).color(p.text_link).truncate())
                .child(text(size).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary)),
        )
}

/// Embed media with its dimensions (`isValidMedia`).
fn sized(m: &reflux_model::EmbedMedia) -> Option<(&reflux_model::EmbedMedia, u32, u32)> {
    Some((m, m.width?, m.height?))
}

// shortcut: rich embeds show provider, author, title, description, fields and image or
// thumbnail; no author/footer icons, galleries or players.
fn embed(e: &Embed, store: &Store, st: &ShellState, m: &Message, p: &Palette) -> El {
    let thumb = e.thumbnail.as_ref().and_then(sized);
    // An image or gifv link with nothing else is the bare media (`EMBED_MAX_*` 400×300).
    if matches!(e.kind.as_str(), "image" | "gifv") && e.title.is_none() && e.description.is_none()
        && let Some((t, w, h)) = e.image.as_ref().and_then(sized).or(thumb)
    {
        return media(st, t.proxy_url.as_deref(), fit(w, h, (400.0, 300.0)), p);
    }
    // `RichEmbed`: articles and images promote the thumbnail; else it sits inline at 80px tall.
    let promote = matches!(e.kind.as_str(), "article" | "image");
    let image = e.image.as_ref().and_then(sized).or(thumb.filter(|_| promote || e.video.is_some()));
    let inline = thumb.filter(|_| image.is_none());
    let bar = e.color.map_or(p.background_modifier_accent, role_color);
    let md = Md { store, media: &st.media, guild: m.guild_id, p };
    let mut body = col().grow(1.0).min_w(0.0).gap(8.0).pt(8.0).pr(16.0).pb(16.0).pl(12.0);
    if let Some(pr) = &e.provider {
        body = body.child(text(pr.name.clone()).font(font(12.0, 400.0)).color(p.text_tertiary));
    }
    if let Some(a) = &e.author {
        body = body.child(text(a.name.clone()).font(font(14.0, 600.0)).color(p.text_primary));
    }
    if let Some(t) = &e.title {
        body = body.child(text(t.clone()).font(font(16.0, 600.0).lh(22.0)).color(if e.url.is_some() { p.text_link } else { p.text_primary }).wrap());
    }
    if let Some(d) = &e.description {
        body = body.child(markdown::render(d, &md).font(font(14.0, 400.0).lh(18.0)));
    }
    for f in &e.fields {
        body = body.child(
            col()
                .child(text(f.name.clone()).font(font(14.0, 600.0).lh(18.0)).color(p.text_primary))
                .child(markdown::render(&f.value, &md)),
        );
    }
    if let Some((t, w, h)) = image {
        body = body.child(media(st, t.proxy_url.as_deref(), fit(w, h, (400.0, 300.0)), p));
    }
    if let Some(f) = &e.footer {
        body = body.child(text(f.text.clone()).font(font(12.0, 500.0)).color(p.text_tertiary));
    }
    if let Some((t, w, h)) = inline {
        let tw = 80f32.min((80.0 * w as f32 / h.max(1) as f32).round());
        body = row().grow(1.0).min_w(0.0).items_start().child(body).child(media(st, t.proxy_url.as_deref(), (tw, 80.0), p).mt(8.0).mr(16.0).ml(16.0));
    }
    row().w_full().max_w(432.0).radius(4.0).fill(p.background_secondary).child(el().w(4.0).self_stretch().corners([4.0, 0.0, 0.0, 4.0]).fill(bar)).child(body)
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
        // `.emoji`: a 20px custom emoji image, or the unicode glyph.
        let glyph = match r.emoji.id {
            Some(id) => match st.media.asset(|b, _| crate::media::emoji_url(b, &id.to_string())) {
                Load::Ready(px) => el().size(20.0, 20.0).my(2.0).pixels_fit(&px, rune::SurfaceFit::Fit),
                _ => text(format!(":{}:", r.emoji.name)).font(font(16.0, 400.0).lh(20.0)),
            },
            None => text(r.emoji.name.clone()).font(font(16.0, 400.0).lh(20.0)),
        };
        let (m2, e) = (m.clone(), r.emoji.clone());
        grid = grid.child(
            row()
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
                .on_press(move |s: &mut S| toggle_reaction(acc(s), &m2, e.clone())),
        );
    }
    grid
}

fn typing_rail(store: &Store, guild: Option<Snowflake>, users: &[Snowflake], p: &Palette) -> El {
    let names: Vec<String> = users.iter().map(|u| store.display_name(guild, *u)).collect();
    let line = match names.as_slice() {
        [a] => format!("{a} is typing…"),
        [a, b] => format!("{a} and {b} are typing…"),
        [a, b, c] => format!("{a}, {b} and {c} are typing…"),
        _ => "Several people are typing…".into(),
    };
    row()
        .pin(Some(PAD), None, Some(PAD), Some(0.0))
        .h(dim::COMPOSER_STATUS_ROW_HEIGHT)
        .center_y()
        .child(text(line).font(font(12.0, 500.0).lh(dim::COMPOSER_STATUS_LINE_HEIGHT)).color(p.text_primary_muted).truncate())
        .inert()
}

// ---- Composer --------------------------------------------------------------------------------

fn composer<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
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
    let placeholder = match c.kind {
        ChannelType::DM => format!("Message @{}", channel_title(store, c)),
        ChannelType::GROUP_DM => format!("Message {}", channel_title(store, c)),
        _ => format!("Message #{}", c.name.clone().unwrap_or_default()),
    };
    let cid = c.id;
    // The newest own user message, for Up-arrow editing.
    let me = store.my_id();
    let last_own = store.messages(cid).iter().rev().find(|m| m.author.id == me && is_user_message(m) && !is_pending(m)).cloned();
    let field = text_input(&st.composer, font(16.0, 400.0).lh(22.0))
        .key("composer")
        .w_full()
        .color(p.text_chat)
        .placeholder(placeholder, p.text_tertiary_secondary)
        .caret(p.text_primary, p.text_selection)
        .on_edit(move |s: &mut S| -> &mut rune::TextEdit { &mut acc(s).composer })
        .on_input_key(move |s: &mut S, k, _| composer_key(acc(s), cid, k, last_own.as_ref()))
        // Enter sends; Shift+Enter is the field's own newline.
        .on_submit(move |s: &mut S, _| {
            let st = acc(s);
            let content = st.composer.text.trim().to_string();
            if !content.is_empty() {
                let reply = st.reply.take().filter(|r| r.channel == cid).map(|r| (r.message, r.mention));
                st.intents.push(Intent::SendMessage { channel: cid, content, reply });
                st.composer.clear();
                st.typing_sent = None; // `handleOwnMessageSent`
            }
        });
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
        .child(row().px((dim::MESSAGE_AVATAR_SIZE - 32.0) / 2.0).child(button(&icons::PLUS_BOLD, "Upload")))
        .child(col().grow(1.0).min_w(0.0).pt((32.0 - 22.0) / 2.0).child(field))
        .child(
            row()
                .gap(dim::COMPOSER_ACTION_GAP)
                .center_y()
                .child(button(&icons::GIF, "GIFs"))
                .child(button(&icons::IMAGE_SQUARE, "Memes"))
                .child(button(&icons::STICKER, "Stickers"))
                .child(button(&icons::SMILEY, "Emojis")),
        );
    let mut boxed = col()
        .w_full()
        .radius(dim::FOOTER_BOX_RADIUS)
        .fill(p.background_textarea)
        .box_shadow(Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 1.0, color: if st.composer.focused { p.floating_surface_ring_color_strong } else { p.floating_surface_ring_color }, inset: true });
    if let Some(r) = st.reply.filter(|r| r.channel == cid)
        && let Some(target) = find(store, cid, r.message)
    {
        boxed = boxed.child(reply_bar(store, target, r, p, acc));
    }
    row().w_full().px(dim::FOOTER_BOX_INSET).pb(dim::COMPOSER_BOX_INSET).fill(p.composer_surface_color).child(boxed.child(input_row))
}

/// The composer's own keys: Escape drops the reply, Up in an empty composer edits the last own
/// message, and typing sends a typing notice at most every 8 s (`TypingSendThrottle`).
// shortcut: no 1.5 s send delay or stop-typing on clear; the notice goes out at once.
fn composer_key(st: &mut ShellState, channel: Snowflake, k: &rune::KeyEvent, last_own: Option<&Message>) -> bool {
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
            if st.typing_sent.is_none_or(|(c, at)| c != channel || st.now_ms.saturating_sub(at) >= TYPING_THROTTLE_MS) {
                st.typing_sent = Some((channel, st.now_ms));
                st.intents.push(Intent::Typing(channel));
            }
            false
        }
        _ => false,
    }
}

/// `TYPING_ROLLING_THROTTLE_MS`.
const TYPING_THROTTLE_MS: u64 = 8000;

/// `ChannelReplyBar`: "Replying to **name**", the @ ON/OFF mention toggle and a close button, in
/// the composer's action stack (`.composerActionStack`).
fn reply_bar<S: 'static>(store: &Store, target: &Message, r: ReplyTo, p: &Palette, acc: ShellAccess<S>) -> El {
    let name = author_name(store, target);
    let line = format!("Replying to {name}");
    let spans = vec![Span::new(12, line.len()).weight(600.0)];
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
            .label("Cancel reply")
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
        let s = stream(&ms, Some(Snowflake(4)));
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
        assert!(!composer_key(&mut st, ch, &key(Key::Text("a".into())), None), "typing still reaches the field");
        st.now_ms += TYPING_THROTTLE_MS - 1;
        composer_key(&mut st, ch, &key(Key::Text("b".into())), None);
        assert_eq!(st.intents, [Intent::Typing(ch)], "one notice per 8 s");
        st.now_ms += 1;
        composer_key(&mut st, ch, &key(Key::Text("c".into())), None);
        assert_eq!(st.intents.len(), 2);

        let mut mine = msg(9, 1, "2026-11-06T12:00:00Z");
        mine.content = "hi".into();
        assert!(composer_key(&mut st, ch, &key(Key::Up), Some(&mine)));
        assert_eq!(st.editing.as_ref().map(|e| (e.0, e.1.text.as_str())), Some((Snowflake(9), "hi")));
        st.composer = TextEdit::new("draft");
        st.editing = None;
        assert!(!composer_key(&mut st, ch, &key(Key::Up), Some(&mine)), "Up only edits from an empty composer");

        st.reply = Some(ReplyTo { channel: ch, message: Snowflake(9), mention: true });
        assert!(composer_key(&mut st, ch, &key(Key::Escape), None));
        assert!(st.reply.is_none());
    }

    #[test]
    fn saving_an_edit() {
        let mut st = ShellState { channel: Some(Snowflake(5)), ..Default::default() };
        st.editing = Some((Snowflake(9), TextEdit::new(" hi ")));
        save_edit(&mut st, Snowflake(9), "hi");
        assert!(st.intents.is_empty() && st.editing.is_none(), "unchanged closes");
        st.editing = Some((Snowflake(9), TextEdit::new("  ")));
        save_edit(&mut st, Snowflake(9), "hi");
        assert_eq!(st.confirm_delete, Some(Snowflake(9)), "emptied asks to delete");
        st.editing = Some((Snowflake(9), TextEdit::new("hey")));
        save_edit(&mut st, Snowflake(9), "hi");
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
