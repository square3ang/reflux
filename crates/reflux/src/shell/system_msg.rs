//! System messages (`SystemMessage`, `GuildJoinMessage`, `PinSystemMessage`, `RecipientAdd/Remove`,
//! `ChannelName/IconChange`, `ChannelFollowAdd`, `CallMessage`) and their plain text
//! (`SystemMessageUtils.stringify`, used by DM previews).

use chrono::DateTime;
use reflux_model::{ChannelType, Message, MessageType, Snowflake};
use reflux_store::Store;
use rune::text::Span;
use rune::{Color, El, Tx, el, icon, rich, row, text};

use super::chat::{author_name, format_relative, format_time};
use super::{ShellAccess, ShellState};
use crate::i18n::{t, tf, tp};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{Placement, mix};

/// `getGuildJoinMessagesPlaintext`, picked by the message's timestamp.
const JOIN: [&str; 17] = [
    "Glad you're here, {username}!",
    "Welcome, {username}! Make yourself at home.",
    "Hello, {username}! Nice to have you here.",
    "Hello, {username}! Jump in whenever you're ready.",
    "Hey {username}, great to see you here!",
    "Hey there, {username}! Hope you enjoy your stay.",
    "Hey, {username}, welcome aboard!",
    "Glad you made it, {username}!",
    "Welcome in, {username}!",
    "Welcome, {username}!",
    "Welcome, {username}! We're glad you're here.",
    "Welcome, {username}! Hope you enjoy your time here.",
    "Welcome, {username}! Your next conversation starts here.",
    "Welcome, {username}. We're happy to have you here.",
    "Great to see you, {username}! Welcome in.",
    "You're here, {username}! Good to have you with us.",
    "You've arrived, {username}! Let's get started.",
];

fn join_template(m: &Message) -> &'static str {
    JOIN[(m.id.timestamp_ms() % JOIN.len() as u64) as usize]
}

/// A piece of a system message: a pressable name or link.
#[derive(Clone, Debug, PartialEq)]
enum Part {
    User(Snowflake, String),
    Link(&'static str),
    /// A name drawn like a link but not pressable (a group DM's new name).
    Name(String),
}

/// Fills a lingui template: `<N/>` → `parts[N]`'s text, `<N>x</N>` → `x` as `parts[N]`, `{key}` →
/// its value (a part when it names one). Returns the text and each part's byte range.
fn fill(template: &str, parts: &[Part], vars: &[(&str, &str, Option<usize>)]) -> (String, Vec<(std::ops::Range<usize>, usize)>) {
    let (mut out, mut ranges) = (String::new(), Vec::new());
    let mut rest = template;
    while let Some(i) = rest.find(['<', '{']) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let tag = |r: &str| -> Option<(usize, bool, usize)> {
            let end = r.find('>')?;
            let inner = &r[1..end];
            let (num, closed) = inner.strip_suffix('/').map_or((inner, false), |n| (n, true));
            Some((num.parse().ok()?, closed, end + 1))
        };
        if rest.starts_with('{') {
            if let Some(end) = rest.find('}')
                && let Some((_, v, part)) = vars.iter().find(|v| v.0 == &rest[1..end])
            {
                let start = out.len();
                out.push_str(v);
                if let Some(k) = part {
                    ranges.push((start..out.len(), *k));
                }
                rest = &rest[end + 1..];
                continue;
            }
        } else if let Some((n, closed, len)) = tag(rest) {
            let start = out.len();
            rest = &rest[len..];
            if closed {
                out.push_str(&parts.get(n).map_or(String::new(), |p| match p {
                    Part::User(_, s) | Part::Name(s) => s.clone(),
                    Part::Link(_) => String::new(),
                }));
            } else {
                let close = format!("</{n}>");
                let end = rest.find(&close).unwrap_or(rest.len());
                out.push_str(&rest[..end]);
                rest = rest.get(end + close.len()..).unwrap_or("");
            }
            if n < parts.len() {
                ranges.push((start..out.len(), n));
            }
            continue;
        }
        out.push_str(&rest[..1]);
        rest = &rest[1..];
    }
    out.push_str(rest);
    (out, ranges)
}

fn user(store: &Store, g: Option<Snowflake>, id: Snowflake) -> Option<String> {
    let n = store.display_name(g, id);
    (!n.is_empty()).then_some(n).or_else(|| store.users.get(&id).map(|u| u.global_name.clone().unwrap_or_else(|| u.username.clone())))
}

/// `formatCallDuration`: "a few seconds" under a minute, else whole units (minutes rounded up)
/// joined as a list.
pub(super) fn call_duration(secs: u64) -> String {
    if secs < 60 {
        return t("a few seconds").into();
    }
    let units = [("year", 525_600), ("month", 43_800), ("week", 10_080), ("day", 1_440), ("hour", 60), ("minute", 1)];
    let mut left = secs.div_ceil(60).max(1);
    let mut parts = Vec::new();
    for (unit, minutes) in units {
        if left < minutes {
            continue;
        }
        let n = left / minutes;
        left -= n * minutes;
        let label = n.to_string();
        let args = [("countLabel", label.as_str())];
        parts.push(match (n, unit) {
            (1, "year") => t("1 year").into(),
            (1, "month") => t("1 month").into(),
            (1, "week") => t("1 week").into(),
            (1, "day") => t("1 day").into(),
            (1, "hour") => t("1 hour").into(),
            (1, _) => t("a minute").into(),
            (n, "year") => tp("{count, plural, one {{countLabel} year} other {{countLabel} years}}", n as i64, &args),
            (n, "month") => tp("{count, plural, one {{countLabel} month} other {{countLabel} months}}", n as i64, &args),
            (n, "week") => tp("{count, plural, one {{countLabel} week} other {{countLabel} weeks}}", n as i64, &args),
            (n, "day") => tp("{count, plural, one {{countLabel} day} other {{countLabel} days}}", n as i64, &args),
            (n, "hour") => tp("{count, plural, one {{countLabel} hour} other {{countLabel} hours}}", n as i64, &args),
            (n, _) => tp("{count, plural, one {{countLabel} minute} other {{countLabel} minutes}}", n as i64, &args),
        });
    }
    crate::i18n::list_and(&parts)
}

/// (ended, missed, duration) of a call message.
fn call_state(store: &Store, m: &Message) -> (bool, bool, String) {
    let Some(ended) = m.call.as_ref().and_then(|c| c.ended_timestamp.as_ref()) else { return (false, false, t("a few seconds").into()) };
    let me = store.my_id();
    let start = DateTime::parse_from_rfc3339(&m.timestamp).map_or(0, |d| d.timestamp());
    let end = DateTime::parse_from_rfc3339(ended).map_or(start, |d| d.timestamp());
    let missed = !m.call.as_ref().is_some_and(|c| c.participants.contains(&me)) && m.author.id != me;
    (true, missed, call_duration((end - start).max(0) as u64))
}

/// `SystemMessageUtils.stringify` (DM previews strip its trailing period).
pub fn stringify(store: &Store, m: &Message) -> Option<String> {
    let name = user(store, None, m.author.id)?;
    let u = [("username", name.as_str())];
    let mention = m.mentions.first().map(|u| u.id);
    Some(match m.kind {
        MessageType::USER_JOIN => tf(join_template(m), &u),
        MessageType::CHANNEL_PINNED_MESSAGE => tf("{username} pinned a message to this channel.", &u),
        MessageType::RECIPIENT_ADD => match mention.and_then(|id| user(store, None, id)) {
            Some(other) => tf("{username} added {userName} to the group.", &[u[0], ("userName", &other)]),
            None => tf("{username} added someone to the group.", &u),
        },
        MessageType::RECIPIENT_REMOVE if mention == Some(m.author.id) => tf("{username} has left the group.", &u),
        MessageType::RECIPIENT_REMOVE => match mention.and_then(|id| user(store, None, id)) {
            Some(other) => tf("{username} removed {userName} from the group.", &[u[0], ("userName", &other)]),
            None => tf("{username} removed someone from the group.", &u),
        },
        MessageType::CHANNEL_NAME_CHANGE if m.content.is_empty() => tf("{username} changed the channel name.", &u),
        MessageType::CHANNEL_NAME_CHANGE => tf("{username} changed the channel name to {newName}.", &[u[0], ("newName", &m.content)]),
        MessageType::CHANNEL_ICON_CHANGE => tf("{username} changed the channel icon.", &u),
        MessageType::CALL => tf("{username} started a call.", &u),
        MessageType::CHANNEL_FOLLOW_ADD => tf("{username} followed {name} into this channel. Messages published there will appear here.", &[u[0], ("name", &m.content)]),
        MessageType::THREAD_CREATED => tf("{username} started a thread: {threadName}", &[u[0], ("threadName", &m.content)]),
        _ => return None,
    })
}

/// `icons::PHONE_FILL` mirrored horizontally: the missed-call phone (Fluxer's `scaleX(-1)`).
static PHONE_MIRRORED: rune::Icon = rune::Icon { name: "phone_fill_mirrored", view: 256.0, body: r#"<path d="M24.12,175.08 A56.26,56.26,0,0,0,80,224 C159.4,224,224,159.4,224,80 A56.26,56.26,0,0,0,175.08,24.12 a16,16,0,0,0,-16.62,9.52 l-21.12,47.15 0,0.12 A16,16,0,0,0,138.61,96 c0.18,0.27,0.37,0.52,0.57,0.77 L160,121.45 c-7.49,15.22,-23.41,31,-38.83,38.51 l-24.34,-20.71 a8.12,8.12,0,0,0,-0.75,-0.56 16,16,0,0,0,-15.17,-1.4 l-0.13,0.06 -47.11,21.11 A16,16,0,0,0,24.12,175.08 Z"/>"# };

/// The message's icon and its colour (`None` = the default, at 60%).
fn glyph(m: &Message, missed: bool, p: &Palette) -> (&'static rune::Icon, Option<Color>) {
    match m.kind {
        MessageType::USER_JOIN => (&icons::ARROW_RIGHT_BOLD, Some(crate::ui::rgb(34, 197, 94))),
        MessageType::RECIPIENT_ADD => (&icons::USER_PLUS_BOLD, Some(crate::ui::rgb(34, 197, 94))),
        MessageType::RECIPIENT_REMOVE => (&icons::USER_MINUS_BOLD, Some(crate::ui::rgb(239, 68, 68))),
        MessageType::CHANNEL_PINNED_MESSAGE => (&icons::PUSH_PIN_FILL, None),
        MessageType::CHANNEL_NAME_CHANGE => (&icons::PENCIL_SIMPLE_BOLD, None),
        MessageType::CHANNEL_ICON_CHANGE => (&icons::IMAGE_SQUARE_BOLD, None),
        MessageType::CHANNEL_FOLLOW_ADD => (&icons::ARROW_RIGHT_BOLD, None),
        MessageType::CALL if missed => (&PHONE_MIRRORED, Some(p.text_tertiary_muted)),
        MessageType::CALL => (&icons::PHONE_FILL, Some(p.status_online)),
        _ => (&icons::INFO_BOLD, None),
    }
}

/// The message's text and parts, as Fluxer's components word it.
/// The filled text, its link ranges (range, part index) and the parts.
type Filled = (String, Vec<(std::ops::Range<usize>, usize)>, Vec<Part>);

fn content(store: &Store, st: &ShellState, m: &Message) -> Filled {
    let c = store.channels.get(&m.channel_id);
    let g = c.and_then(|c| c.guild_id);
    let me = Part::User(m.author.id, author_name(store, m));
    let mention = m.mentions.first().map(|u| u.id);
    let other = mention.and_then(|id| Some(Part::User(id, user(store, g, id)?)));
    #[allow(clippy::type_complexity)]
    let (tpl, parts, vars): (&str, Vec<Part>, Vec<(&str, String, Option<usize>)>) = match m.kind {
        MessageType::USER_JOIN => (join_template(m), vec![me], vec![]),
        MessageType::CHANNEL_PINNED_MESSAGE => (
            "<0/> pinned <1>a message</1> to this channel. See <2>all pinned messages</2>.",
            vec![me, Part::Link("jump"), Part::Link("pins")],
            vec![],
        ),
        MessageType::RECIPIENT_ADD => match other {
            Some(o) => ("<0/> added <1/> to the group.", vec![me, o], vec![]),
            None => ("<0/> added someone to the group.", vec![me], vec![]),
        },
        MessageType::RECIPIENT_REMOVE if mention == Some(m.author.id) => ("<0/> has left the group.", vec![me], vec![]),
        MessageType::RECIPIENT_REMOVE => match other {
            Some(o) => ("<0/> removed <1/> from the group.", vec![me, o], vec![]),
            None => ("<0/> removed someone from the group.", vec![me], vec![]),
        },
        MessageType::CHANNEL_NAME_CHANGE if m.content.is_empty() => ("<0/> changed the channel name.", vec![me], vec![]),
        MessageType::CHANNEL_NAME_CHANGE => {
            let group = c.is_some_and(|c| c.kind == ChannelType::GROUP_DM);
            let part = if group { Some(1) } else { None };
            ("<0/> changed the channel name to {nameComponent}.", vec![me, Part::Name(m.content.clone())], vec![("nameComponent", m.content.clone(), part)])
        }
        MessageType::CHANNEL_ICON_CHANGE => ("<0/> changed the channel icon.", vec![me], vec![]),
        MessageType::CHANNEL_FOLLOW_ADD => (
            "<0/> followed {sourceComponent} into this channel. Messages published there will appear here.",
            vec![me, Part::Name(m.content.clone())],
            vec![("sourceComponent", m.content.clone(), Some(1))],
        ),
        MessageType::CALL => {
            let (ended, missed, duration) = call_state(store, m);
            if !ended {
                // `shouldShowJoinLink`: the call runs and we're not in it.
                let joinable = store.calls.contains_key(&m.channel_id) && st.voice.channel != Some((None, m.channel_id));
                if joinable {
                    let s = format!("{} {{join}}", t("<0/> started a call."));
                    return finish(&s, vec![me, Part::Link("call")], &[("join", t("Join the call"), Some(1))]);
                }
                ("<0/> started a call.", vec![me], vec![])
            } else if missed {
                ("You missed a call from <0/> that lasted {durationText}.", vec![me], vec![("durationText", duration, None)])
            } else {
                ("<0/> started a call that lasted {durationText}.", vec![me], vec![("durationText", duration, None)])
            }
        }
        _ => {
            let body = if m.content.is_empty() { tf("Update {productName} to view this message.", &[("productName", "Reflux")]) } else { m.content.clone() };
            return (body, vec![], vec![]);
        }
    };
    if m.kind == MessageType::USER_JOIN {
        // The join lines are plain-text templates with `{username}` as the name.
        let name = if let Part::User(_, n) = &parts[0] { n.clone() } else { String::new() };
        let (s, r) = fill(t(tpl), &parts, &[("username", &name, Some(0))]);
        return (s, r, parts);
    }
    let vars: Vec<_> = vars.iter().map(|(k, v, p)| (*k, v.as_str(), *p)).collect();
    finish(t(tpl), parts, &vars)
}

fn finish(tpl: &str, parts: Vec<Part>, vars: &[(&str, &str, Option<usize>)]) -> Filled {
    let (s, r) = fill(tpl, &parts, vars);
    (s, r, parts)
}

/// `SystemMessage`: the icon in the avatar column, then the wording with pressable names and
/// links and the dim timestamp after it (compact: `[time]`, icon, text).
pub(super) fn view<S: 'static>(store: &Store, st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> El {
    let (_, missed, _) = call_state(store, m);
    let (glyph, color) = glyph(m, missed, p);
    let (mut body, ranges, parts) = content(store, st, m);
    let ms = super::chat::message_ms(m);
    let compact = store.settings.message_display_compact;
    let g = store.channels.get(&m.channel_id).and_then(|c| c.guild_id);
    let mut spans = Vec::new();
    let mut links = Vec::new();
    for (r, k) in &ranges {
        let (c, id) = match &parts[*k] {
            Part::User(u, _) => (g.and_then(|g| store.member_color(g, *u)).map_or(p.text_primary, super::role_color), Some(format!("user:{u}"))),
            Part::Link(l) if *l == "call" => (p.text_link, Some(l.to_string())),
            Part::Link(l) => (p.text_primary, Some(l.to_string())),
            Part::Name(_) => (p.text_primary, None),
        };
        spans.push(Span::new(r.start, r.end).weight(500.0).color(c));
        if let Some(id) = id {
            links.push(rune::Link::new(r.clone(), id).role("button").hover(rune::LinkPaint::new().underline(c)));
        }
    }
    if !compact {
        // `.systemMessageTimestamp`: 0.25rem after the text, 60% of the timestamp colour.
        let n = body.len();
        body.push(' ');
        body.push_str(&format_relative(ms, st.now_ms));
        spans.push(Span::new(n + 1, body.len()).size(dim::MESSAGE_TIMESTAMP_FONT_SIZE).color(mix(p.message_timestamp_color, Color::TRANSPARENT, 60.0)));
    }
    let (mid, cid) = (m.id, m.channel_id);
    let pinned = m.message_reference.as_ref().and_then(|r| r.message_id);
    let anchor = format!("sys-{mid}");
    let words = rich(body, spans)
        .font(font(16.0, 400.0).lh(dim::MESSAGE_LINE_HEIGHT))
        .color(p.text_tertiary)
        .wrap()
        .grow(1.0)
        .shrink()
        .key(anchor.clone());
    let words = links.into_iter().fold(words, |w, l| w.link(l)).on_link(move |s: &mut S, id: &str, _: &mut Tx| {
        let sh = acc(s);
        match id {
            "jump" => {
                if let Some(target) = pinned {
                    super::jump(sh, g, cid, target);
                }
            }
            "pins" => super::pins::open(sh, cid),
            "call" => sh.intents.push(super::Intent::Voice(super::voice::Op::AcceptCall(cid))),
            u => {
                if let Some(u) = u.strip_prefix("user:").and_then(|u| u.parse().ok()).map(Snowflake) {
                    sh.open_profile(u, g, String::new(), anchor.clone(), Placement::RightStart);
                }
            }
        }
    });
    let ic = |size: f32| {
        let i = icon(glyph, size).color(color.unwrap_or(p.text_tertiary));
        if color.is_none() { i.opacity(0.6) } else { i }
    };
    let root = row().w_full().items_start().hover(p.background_modifier_hover);
    if compact {
        let ts = format!("[{}]", format_time(ms));
        return root
            .pr(dim::MESSAGE_COMPACT_HORIZONTAL_PADDING + dim::MESSAGE_ACTION_BAR_OFFSET)
            .py(dim::MESSAGE_SPACING_Y)
            .child(el().w(dim::MESSAGE_COMPACT_HORIZONTAL_PADDING))
            .child(text(ts).font(font(dim::MESSAGE_TIMESTAMP_COMPACT_FONT_SIZE, 400.0).lh(dim::MESSAGE_LINE_HEIGHT)).color(p.message_timestamp_color).offset(0.0, 1.5))
            .child(el().w(dim::MESSAGE_COMPACT_GAP))
            .child(row().h(dim::MESSAGE_LINE_HEIGHT).center_y().child(ic(dim::SYSTEM_MESSAGE_ICON_SIZE)))
            .child(el().w(dim::MESSAGE_COMPACT_GAP))
            .child(words);
    }
    let pad = dim::CHAT_HORIZONTAL_PADDING_DEFAULT;
    root.pr(pad + dim::MESSAGE_ACTION_BAR_OFFSET)
        .py(dim::MESSAGE_SPACING_Y)
        .child(el().w(pad))
        .child(row().w(dim::MESSAGE_AVATAR_SIZE).h(dim::MESSAGE_LINE_HEIGHT).justify_end().center_y().child(ic(dim::SYSTEM_MESSAGE_ICON_SIZE)))
        .child(el().w(dim::MESSAGE_GUTTER))
        .child(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_fill_parts_and_vars() {
        let parts = [Part::User(Snowflake(1), "Ann".into()), Part::Link("jump"), Part::Link("pins")];
        let (s, r) = fill("<0/> pinned <1>a message</1> to this channel. See <2>all pinned messages</2>.", &parts, &[]);
        assert_eq!(s, "Ann pinned a message to this channel. See all pinned messages.");
        assert_eq!(r.iter().map(|(r, k)| (&s[r.clone()], *k)).collect::<Vec<_>>(), [("Ann", 0), ("a message", 1), ("all pinned messages", 2)]);
        let (s, r) = fill("You missed a call from <0/> that lasted {durationText}.", &parts[..1], &[("durationText", "a minute", None)]);
        assert_eq!(s, "You missed a call from Ann that lasted a minute.");
        assert_eq!(r.len(), 1);
        let (s, r) = fill("Hey {username}, great to see you here!", &parts[..1], &[("username", "Ann", Some(0))]);
        assert_eq!((s.as_str(), r[0].0.clone()), ("Hey Ann, great to see you here!", 4..7));
    }
}
