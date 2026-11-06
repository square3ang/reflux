//! The main column: channel header, message list, composer and member list
//! (`ChannelHeader`, `ChannelMessages`, `ChannelDivider`, `theme/styles/Message.module.css`,
//! `MessageReactions`, `textarea/TextareaInput`).

use chrono::{DateTime, Datelike, Local, TimeZone};
use reflux_model::{Attachment, Channel, ChannelType, Embed, Message, MessageFlags, MessageType, Snowflake};
use reflux_store::Store;
use rune::text::Span;
use rune::{Color, El, GrowthSpec, ScrollSpec, Shadow, col, el, icon, rich, row, text, text_input};

use super::markdown::{self, Md};
use super::{ShellAccess, ShellState, avatar, channel_icon, members, role_color};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{mix, over, scroller};

/// `MESSAGE_GROUP_TIMEOUT`.
const GROUP_MS: u64 = 7 * 60 * 1000;
/// `--message-group-spacing`.
const GROUP_SPACING: f32 = 16.0;
/// Avatar column start: `--chat-horizontal-padding`.
const PAD: f32 = dim::CHAT_HORIZONTAL_PADDING_DEFAULT;
/// Content column start: padding + avatar + gutter.
const CONTENT_X: f32 = PAD + dim::MESSAGE_AVATAR_SIZE + dim::MESSAGE_GUTTER;

pub fn main<S: 'static>(store: &Store, st: &ShellState, channel: Option<&Channel>, p: &Palette, acc: ShellAccess<S>) -> El {
    let Some(c) = channel else {
        return col().grow(1.0).min_w(0.0).h_full().fill(p.background_secondary_lighter);
    };
    let show_members = st.member_list && c.guild_id.is_some();
    let chat = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .child(messages(store, st, c, p, acc))
        .child(composer(store, st, c, p, acc));
    let mut content = row().w_full().grow(1.0).min_h(0.0).child(chat);
    if show_members {
        content = content
            .child(members::list(store, st, c, p))
            .child(el().pin(None, Some(0.0), Some(dim::LAYOUT_MEMBER_LIST_WIDTH), Some(0.0)).w(1.0).fill(over(p.user_area_divider_color, p.background_secondary_lighter)));
    }
    col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .fill(p.background_secondary_lighter)
        .child(header(store, st, c, p, acc))
        .child(content)
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
        left = left.child(avatar(st, other, 24.0, None, p.background_secondary_lighter, p));
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
                markdown::inline_text(topic, &Md { store, guild: c.guild_id, p }, font(13.0, 400.0).lh(18.0), p.text_tertiary)
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

fn messages<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let msgs = store.messages(c.id);
    let first_unread = if store.has_unread(c.id) {
        let ack = store.read_states.get(&c.id).and_then(|r| r.last_message_id).unwrap_or_default();
        msgs.iter().find(|m| m.id > ack && m.author.id != store.my_id()).map(|m| m.id)
    } else {
        None
    };
    let md = Md { store, guild: c.guild_id, p };
    let items = stream(msgs, first_unread);
    let rows = items.iter().map(|it| match *it {
        Item::Date(ms) => date_divider(ms, p).key(format!("date-{}", format_full_date(ms))),
        Item::Msg { i, head, space, unread } => {
            let m = &msgs[i];
            let mut r = col().w_full();
            if unread {
                r = r.child(new_divider(p));
            } else if space > 0.0 {
                r = r.pt(space);
            }
            r.child(message(store, st, m, head, &md, p)).key(format!("msg-{}", m.id))
        }
    });
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
            if r.start == 0 && more && !st.intents.contains(&super::Intent::LoadOlder { channel: cid }) {
                st.intents.push(super::Intent::LoadOlder { channel: cid });
            }
            false
        });
    let viewport = col().w_full().grow(1.0).min_h(0.0).justify_end().scroll(ScrollSpec { stick_bottom: true }).child(list);
    let mut area = col().w_full().grow(1.0).min_h(0.0).child(scroller(viewport, true, p));
    let typing = store.typing_users(c.id, st.now_ms);
    if !typing.is_empty() {
        area = area.child(typing_rail(store, c.guild_id, &typing, p));
    }
    area
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

fn message(store: &Store, st: &ShellState, m: &Message, head: bool, md: &Md, p: &Palette) -> El {
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
    let gutter = if head {
        col().w(dim::MESSAGE_AVATAR_SIZE).pt(dim::MESSAGE_SPACING_Y + dim::MESSAGE_AVATAR_ALIGN_OFFSET).child(
            avatar(st, m.author.id, dim::MESSAGE_AVATAR_SIZE, None, p.background_secondary_lighter, p),
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
        content = content.child(rich(line, spans).font(font(16.0, 400.0).lh(dim::MESSAGE_LINE_HEIGHT)).color(p.text_primary).truncate());
    }
    if !m.content.is_empty() {
        content = content.child(markdown::render(&m.content, md));
    }
    if m.edited_timestamp.is_some() {
        // shortcut: "(edited)" sits on its own line under the body instead of inline after it.
        content = content.child(text("(edited)").font(font(dim::MESSAGE_EDITED_LABEL_FONT_SIZE, 400.0).lh(16.0)).color(p.message_timestamp_color));
    }
    let acc_el = accessories(store, m, p);
    if let Some(a) = acc_el {
        content = content.child(a);
    }
    root.child(row().w_full().items_start().child(el().w(PAD)).child(gutter).child(el().w(dim::MESSAGE_GUTTER)).child(content))
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
                .child(avatar(st, r.author.id, 16.0, None, p.background_secondary_lighter, p).mr(dim::MESSAGE_CONTAINER_GAP))
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
fn accessories(store: &Store, m: &Message, p: &Palette) -> Option<El> {
    if m.attachments.is_empty() && m.embeds.is_empty() && m.reactions.is_empty() {
        return None;
    }
    let mut c = col().w_full().gap(dim::MESSAGE_CONTAINER_GAP).py(dim::MESSAGE_CONTAINER_PADDING_Y).items_start();
    for a in &m.attachments {
        c = c.child(attachment(a, p));
    }
    if !m.flags.contains(MessageFlags::SUPPRESS_EMBEDS) {
        for e in &m.embeds {
            c = c.child(embed(e, store, m, p));
        }
    }
    if !m.reactions.is_empty() {
        c = c.child(reactions(m, p));
    }
    Some(c)
}

/// The size an image attachment draws at: fit in 550×350 (`MediaDimensionConfig`), never upscaled.
pub fn media_size(w: u32, h: u32) -> (f32, f32) {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let k = (550.0 / w).min(350.0 / h).min(1.0);
    ((w * k).round(), (h * k).round())
}

fn attachment(a: &Attachment, p: &Palette) -> El {
    if let (Some(w), Some(h), true) = (a.width, a.height, a.content_type.starts_with("image/") || a.content_type.starts_with("video/")) {
        // shortcut: a placeholder box at the final size; the image itself loads later.
        let (w, h) = media_size(w, h);
        return row()
            .size(w, h)
            .center()
            .radius(dim::MEDIA_BORDER_RADIUS)
            .fill(p.background_secondary_alt)
            .child(icon(&icons::IMAGE, 32.0).color(p.text_tertiary_muted));
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

// shortcut: rich embeds show provider, author, title, description and fields; no media or footer icons.
fn embed(e: &Embed, store: &Store, m: &Message, p: &Palette) -> El {
    let bar = e.color.map_or(p.background_modifier_accent, role_color);
    let md = Md { store, guild: m.guild_id, p };
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
    if let Some(f) = &e.footer {
        body = body.child(text(f.text.clone()).font(font(12.0, 500.0)).color(p.text_tertiary));
    }
    row().w_full().max_w(432.0).radius(4.0).fill(p.background_secondary).child(el().w(4.0).self_stretch().corners([4.0, 0.0, 0.0, 4.0]).fill(bar)).child(body)
}

/// `.reactionsGrid`.
fn reactions(m: &Message, p: &Palette) -> El {
    let mut grid = row().flex_wrap().gaps(4.0, 0.0).pt(4.0).center_y();
    for r in &m.reactions {
        let (bg, border, fg) = if r.me {
            (p.brand_tint, p.brand_tint_border, p.text_on_brand_tint)
        } else {
            (p.background_secondary, Color::TRANSPARENT, p.text_tertiary)
        };
        let glyph = if r.emoji.id.is_some() { format!(":{}:", r.emoji.name) } else { r.emoji.name.clone() };
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
                .child(row().min_w(20.0).h(24.0).center().child(text(glyph).font(font(16.0, 400.0).lh(20.0))))
                .child(text(r.count.to_string()).font(font(14.0, 600.0).lh(16.0)).color(fg).min_w(12.0).ml(6.0)),
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
    let field = text_input(&st.composer, font(16.0, 400.0).lh(22.0))
        .key("composer")
        .w_full()
        .color(p.text_chat)
        .placeholder(placeholder, p.text_tertiary_secondary)
        .caret(p.text_primary, p.text_selection)
        .on_edit(move |s: &mut S| -> &mut rune::TextEdit { &mut acc(s).composer })
        // Enter sends; Shift+Enter is the field's own newline.
        .on_submit(move |s: &mut S, _| {
            let st = acc(s);
            let content = st.composer.text.trim().to_string();
            if !content.is_empty() {
                st.intents.push(super::Intent::SendMessage { channel: cid, content });
                st.composer.clear();
            }
        });
    // `.textareaOuterRow`: inset 6, min-height 58, radius 8, ring; `.mainWrapperDense` grid with
    // 12px columns gaps, padding 13 / 10.
    let boxed = row()
        .w_full()
        .min_h(dim::FOOTER_BOX_HEIGHT)
        .py(dim::FOOTER_BOX_PADDING_Y)
        .px(dim::CHAT_HORIZONTAL_PADDING_DEFAULT - dim::FOOTER_BOX_INSET)
        .gap(dim::MESSAGE_GUTTER)
        .items_start()
        .radius(dim::FOOTER_BOX_RADIUS)
        .fill(p.background_textarea)
        .box_shadow(Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 1.0, color: if st.composer.focused { p.floating_surface_ring_color_strong } else { p.floating_surface_ring_color }, inset: true })
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
    row().w_full().px(dim::FOOTER_BOX_INSET).pb(dim::COMPOSER_BOX_INSET).fill(p.composer_surface_color).child(boxed)
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
