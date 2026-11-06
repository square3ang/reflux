//! Channel-level pieces: the header bell's notification menu and the mute-duration submenus
//! (`ChannelMenuItems`, `MuteTargetMenuItem`), `ChannelTopicModal`, duplicating a channel, the
//! welcome sections at the top of a channel (`ChannelWelcomeSection`, `DMWelcomeSection`,
//! `GroupDMWelcomeSection`, `PersonalNotesWelcomeSection`), blocked message groups
//! (`BlockedMessageGroups`) and the @everyone confirmation.

use reflux_model::{Channel, ChannelType, Message, Permissions as P, RelationshipType, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, col, icon, row, text};
use serde_json::{Value, json};

use super::{Done, Modal, body, close, desc, error, footer, override_patch};
use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Menu, MenuEntry, MenuItem, Modal as Dialog, Placement, popup_access};
use crate::i18n::t;

pub const BELL: &str = "notif-bell";

/// `getMuteDurationOptions`: ms, 0 = until turned back on.
const MUTE_FOR: [(u64, &str); 9] = [
    (900_000, "For 15 minutes"),
    (1_800_000, "For 30 minutes"),
    (3_600_000, "For 1 hour"),
    (10_800_000, "For 3 hours"),
    (14_400_000, "For 4 hours"),
    (28_800_000, "For 8 hours"),
    (86_400_000, "For 24 hours"),
    (259_200_000, "For 3 days"),
    (0, "Until I turn it back on"),
];

/// `getMutedText`: "Muted until …" for a timed mute.
fn muted_text(end: Option<&str>) -> String {
    match end.and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()) {
        Some(d) => crate::i18n::tf("Muted until {mutedUntil}", &[("mutedUntil", &(d.with_timezone(&chrono::Local).format("%-m/%-d/%Y, %-I:%M %p")).to_string())]),
        None => t("Muted").into(),
    }
}

/// `MuteTargetMenuItem` for a community (`channel: None`) or a channel/DM: "Unmute …" when muted,
/// else a "Mute …" submenu of durations.
pub fn mute_entry<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, channel: Option<Snowflake>, noun: &str, acc: ShellAccess<S>) -> MenuEntry<S> {
    let s = store.guild_settings.get(&guild);
    let (muted, end) = match channel {
        None => (s.is_some_and(|s| s.muted), s.and_then(|s| s.mute_config.as_ref()?.end_time.clone())),
        Some(c) => {
            let o = s.and_then(|s| s.channel_overrides.as_ref()?.get(&c));
            (o.is_some_and(|o| o.muted), o.and_then(|o| o.mute_config.as_ref()?.end_time.clone()))
        }
    };
    let patch = |muted: bool, cfg: Value| match channel {
        None => json!({ "muted": muted, "mute_config": cfg }),
        Some(c) => override_patch(store, guild, c, |o| {
            o["muted"] = json!(muted);
            o["mute_config"] = cfg.clone();
        }),
    };
    if muted {
        let p = patch(false, Value::Null);
        return MenuEntry::Item(MenuItem::new("unmute", crate::i18n::tf("Unmute {label}", &[("label", noun)])).icon(&icons::BELL_FILL).hint(muted_text(end.as_deref())).on_select(move |s: &mut S| {
            acc(s).intents.push(Intent::PatchGuildSettings { guild, patch: p.clone() })
        }));
    }
    let now = st.now_ms;
    let items = MUTE_FOR
        .iter()
        .map(|&(ms, label)| {
            let p = patch(true, crate::guild::mute_config(ms, now));
            MenuEntry::Item(MenuItem::new(format!("mute-{ms}"), label).on_select(move |s: &mut S| acc(s).intents.push(Intent::PatchGuildSettings { guild, patch: p.clone() })))
        })
        .collect();
    MenuEntry::Submenu("mute".into(), crate::i18n::tf("Mute {displayLabel}", &[("displayLabel", noun)]), items)
}

/// `NOTIFICATION_LEVEL_*` labels.
fn level_label(l: u8) -> &'static str {
    match l {
        0 => t("All messages"),
        1 => t("Only @mentions"),
        _ => t("Nothing"),
    }
}

/// The channel's notification level radios (`ChannelNotificationSettingsMenuItem`).
fn level_entries<S: 'static>(store: &Store, c: &Channel, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let guild = c.guild_id;
    let cur = store.guild_settings.get(&guild).and_then(|s| s.channel_overrides.as_ref()?.get(&c.id)).map_or(3, |o| o.message_notifications.0);
    // What "default" resolves to: the category's level, else the community's.
    let parent = c.parent_id.and_then(|id| store.channels.get(&id));
    let default = match parent {
        Some(cat) => store.notification_level(cat).0,
        None => store.notification_level(&Channel { id: Snowflake(0), guild_id: guild, ..Default::default() }).0,
    };
    let cid = c.id;
    let set = |l: u8| {
        let p = override_patch(store, guild, cid, |o| o["message_notifications"] = json!(l));
        move |s: &mut S| acc(s).intents.push(Intent::PatchGuildSettings { guild, patch: p.clone() })
    };
    let main = if parent.is_some() { t("Use category default") } else { "Use community default" };
    let mut out = vec![MenuEntry::Item(MenuItem::new("level-default", main).hint(level_label(default)).radio(cur == 3).on_select(set(3)))];
    for l in 0..3u8 {
        out.push(MenuEntry::Item(MenuItem::new(format!("level-{l}"), level_label(l)).radio(cur == l).on_select(set(l))));
    }
    out
}

/// The header bell's menu.
pub fn bell_menu<S: 'static>(store: &Store, st: &ShellState, c: &Channel, acc: ShellAccess<S>) -> Menu<S> {
    let mut m = Menu::new(BELL).placement(Placement::BottomEnd).entry(mute_entry(store, st, c.guild_id, Some(c.id), "channel", acc)).separator();
    for e in level_entries(store, c, acc) {
        m = m.entry(e);
    }
    m
}

/// The header bell's menu, when open.
pub fn bell_layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !st.popups.is_open(BELL) {
        return;
    }
    let Some(c) = st.channel.and_then(|c| store.channels.get(&c)) else { return };
    bell_menu(store, st, c, acc).el(&st.popups, popup_access(move |s: &mut S| &mut acc(s).popups), p, layer);
}

/// The bell button for the channel header.
pub fn bell_button<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let open = st.popups.is_open(BELL);
    let glyph = if store.is_muted(c) { &icons::BELL_SLASH } else { &icons::BELL };
    row()
        .size(32.0, 32.0)
        .center()
        .radius(16.0)
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label("Notification Settings")
        .key(BELL)
        .child(icon(glyph, 20.0).color(if open { p.text_primary } else { p.text_primary_muted }).group_color(p.text_primary))
        .on_press(move |s: &mut S| acc(s).popups.toggle(BELL))
}

// ---- Topic / duplicate -----------------------------------------------------------------------

/// `ChannelTopicModal`: the whole topic, as markdown.
pub fn topic<S: 'static>(store: &Store, st: &ShellState, c: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let Some(ch) = store.channels.get(&c) else { return Dialog::new("", close(acc)).el(p) };
    let t = ch.topic.clone().unwrap_or_default();
    let title = format!("#{}", ch.name.clone().unwrap_or_default());
    let b = crate::shell::rich_text(store, st, ch.guild_id, &t, "topic-modal", p);
    Dialog::new(title, close(acc)).body(col().w_full().child(b)).el(p)
}

/// "Duplicate channel": a copy with the same type, topic, permissions and category.
pub fn duplicate<S: 'static>(store: &Store, st: &ShellState, c: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let Some(ch) = store.channels.get(&c).cloned() else { return Dialog::new(t("Duplicate channel"), close(acc)).el(p) };
    let name = ch.name.clone().unwrap_or_default();
    let mut items = vec![desc(format!("Create a copy of #{name} with the same permissions and settings."), p), super::field(st, acc, "name", &name, "Channel name", "", p)];
    items.extend(error(st, p));
    let g = ch.guild_id.unwrap_or_default();
    let go = move |s: &mut S| {
        let sh = acc(s);
        let name = sh.form.value("name", &name).trim().to_string();
        if name.is_empty() {
            sh.form.error = Some("Channel name is required".into());
            return;
        }
        let mut b = json!({ "name": name, "type": ch.kind.0, "permission_overwrites": ch.permission_overwrites, "nsfw": ch.nsfw, "rate_limit_per_user": ch.rate_limit_per_user });
        for (k, v) in [("topic", json!(ch.topic)), ("parent_id", json!(ch.parent_id)), ("bitrate", json!(ch.bitrate)), ("user_limit", json!(ch.user_limit))] {
            if !v.is_null() {
                b[k] = v;
            }
        }
        sh.social(Method::POST, format!("/guilds/{g}/channels"), Some(b), Done::Enter);
    };
    Dialog::new(t("Duplicate channel"), close(acc)).body(body(items)).footer(footer(st, t("Duplicate"), false, "duplicate-confirm", acc, go, p)).el(p)
}

// ---- Welcome sections ------------------------------------------------------------------------

/// The top of a channel's history.
pub fn welcome<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let me = store.my_id();
    match c.kind {
        ChannelType::DM => {
            let other = c.recipients.iter().find(|u| u.id != me).map(|u| u.id);
            match other {
                Some(u) => dm_welcome(store, st, u, p, acc),
                None => notes_welcome(p),
            }
        }
        ChannelType::GROUP_DM => group_welcome(store, st, c, p, acc),
        ChannelType::DM_PERSONAL_NOTES => notes_welcome(p),
        _ => {
            let name = format!("#{}", c.name.clone().unwrap_or_default());
            let visiting = c.guild_id.and_then(|g| store.guilds.get(&g)).is_some_and(|g| g.visiting);
            let line = if visiting { "Older messages are visible after you join.".to_string() } else { crate::i18n::tf("This is the start of {channelDisplayName}.", &[("channelDisplayName", &name.to_string())]) };
            col()
                .w_full()
                .mx(16.0)
                .mb(32.0)
                .pt(120.0)
                .gap(4.0)
                .key("welcome")
                .child(row().size(80.0, 80.0).center().radius(40.0).fill(p.guild_list_foreground).child(icon(crate::shell::channel_icon(c.kind), 48.0).color(p.text_primary)))
                .child(text(crate::i18n::tf("Welcome to {channelDisplayName}", &[("channelDisplayName", &name.to_string())])).font(font(30.0, 600.0).lh(38.0)).color(p.text_primary).wrap().mt(12.0))
                .child(text(line).font(font(18.0, 400.0).lh(26.0)).color(p.text_primary_muted).wrap())
        }
    }
}

fn dm_welcome<S: 'static>(store: &Store, st: &ShellState, u: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let name = store.display_name(None, u);
    let tag = store.users.get(&u).map(|x| x.username.clone()).unwrap_or_default();
    let bot = store.users.get(&u).is_some_and(|x| x.bot);
    let kind = store.relationships.get(&u).map(|r| r.kind);
    let push = |i: Intent| move |s: &mut S| acc(s).intents.push(i.clone());
    let small = |l: &str, v: ButtonVariant, k: &str| Button::<S>::new(l).variant(v).size(ButtonSize::Small).key(k.to_string());
    let action: Option<El> = if bot {
        None
    } else {
        match kind {
            None => Some(small(t("Send friend request"), ButtonVariant::Primary, "dm-add-friend").on_press(push(Intent::AddFriend(u))).el(p)),
            Some(RelationshipType::INCOMING_REQUEST) => Some(
                row()
                    .gap(8.0)
                    .child(small(t("Accept"), ButtonVariant::Primary, "dm-accept").on_press(push(Intent::AddFriend(u))).el(p))
                    .child(small(t("Ignore"), ButtonVariant::Secondary, "dm-ignore").on_press(push(Intent::RemoveRelationship(u))).el(p)),
            ),
            Some(RelationshipType::OUTGOING_REQUEST) => Some(small(t("Cancel friend request"), ButtonVariant::Secondary, "dm-cancel").on_press(push(Intent::RemoveRelationship(u))).el(p)),
            Some(RelationshipType::FRIEND) => Some(small(t("Remove friend"), ButtonVariant::Secondary, "dm-remove").on_press(push(Intent::RemoveRelationship(u))).el(p)),
            _ => None,
        }
    };
    let say = format!("Say hi to {name}. Your DM starts here.");
    let bold = rune::Span::new(10, 10 + name.len()).weight(700.0);
    col()
        .w_full()
        .px(32.0)
        .mb(32.0)
        .pt(64.0)
        .gap(24.0)
        .items_center()
        .key("welcome")
        .child(col().gap(16.0).items_center().child(crate::shell::avatar(store, st, u, 80.0, Some(crate::shell::presence(store, u)), p.background_secondary_lighter, p)).child(text(tag).font(font(24.0, 600.0).lh(30.0)).color(p.text_primary)))
        .child(rune::rich(say, vec![bold]).font(font(18.0, 400.0).lh(28.0)).color(p.text_primary).align(rune::Align::Center).wrap())
        .children(action.map(|a| row().center().child(a)))
}

fn group_welcome<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let cid = c.id;
    let full = c.recipients.len() + 1 >= 10;
    let mut buttons = row().gap(8.0).child(Button::new(t("Edit group")).variant(ButtonVariant::Secondary).icon(&icons::NOTE_PENCIL_BOLD).key("group-edit").on_press(move |s: &mut S| acc(s).open_social(Modal::EditGroup(cid))).el(p));
    if !full {
        buttons = buttons.child(Button::new(t("Add friends to group")).icon(&icons::USER_PLUS_BOLD).key("group-add").on_press(move |s: &mut S| acc(s).open_social(Modal::AddToGroup(cid))).el(p));
    }
    col()
        .w_full()
        .px(32.0)
        .mb(32.0)
        .pt(64.0)
        .gap(24.0)
        .items_center()
        .key("welcome")
        .child(col().gap(16.0).items_center().child(crate::shell::group_avatar(store, st, c, 80.0, p.background_secondary_lighter, p)).child(text(crate::shell::channel_title(store, c)).font(font(24.0, 600.0).lh(30.0)).color(p.text_primary).wrap()))
        .child(text("Add friends to get the group going.").font(font(18.0, 400.0).lh(28.0)).color(p.text_primary).align(rune::Align::Center).wrap())
        .child(buttons)
}

fn notes_welcome(p: &Palette) -> El {
    col()
        .w_full()
        .px(32.0)
        .mb(32.0)
        .pt(64.0)
        .gap(8.0)
        .items_center()
        .key("welcome")
        .child(row().size(80.0, 80.0).center().radius(40.0).fill(p.guild_list_foreground).child(icon(&icons::NOTE_PENCIL_FILL, 40.0).color(p.text_primary)))
        .child(text(t("Personal notes")).font(font(24.0, 600.0).lh(30.0)).color(p.text_primary))
        .child(text(t("Your private space for thoughts and reminders")).font(font(18.0, 400.0).lh(28.0)).color(p.text_primary_muted).align(rune::Align::Center).wrap())
}

// ---- Blocked message groups -----------------------------------------------------------------

/// Runs of consecutive messages from blocked users: (first index, length).
pub fn blocked_runs(store: &Store, msgs: &[Message]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, m) in msgs.iter().enumerate() {
        if !store.is_blocked(m.author.id) {
            continue;
        }
        match out.last_mut() {
            Some((s, n)) if *s + *n == i => *n += 1,
            _ => out.push((i, 1)),
        }
    }
    out
}

/// `.container` + `.toggle`: "N blocked messages", revealing the run when pressed.
pub fn blocked_bar<S: 'static>(first: Snowflake, n: usize, shown: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let label = if n == 1 { "1 blocked message".to_string() } else { format!("{n} blocked messages") };
    row()
        .w_full()
        .py(4.0)
        .px(16.0)
        .center()
        .fill(p.background_secondary)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .expanded(shown)
        .hover_group()
        .key(format!("blocked-{first}"))
        .child(text(label).font(font(12.0, 500.0).lh(16.0).tracking(0.24)).color(p.text_primary_muted).group_color(p.text_secondary))
        .on_press(move |s: &mut S| {
            let set = &mut acc(s).social.show_blocked;
            if !set.remove(&first) {
                set.insert(first);
            }
        })
}

// ---- @everyone confirmation -----------------------------------------------------------------

/// Above this many members, @everyone / @here asks first (`MassMentionWarning`).
const MASS_THRESHOLD: u32 = 50;

/// The mention a message would mass-notify with and how many it reaches, if it needs asking.
pub fn mass_mention(store: &Store, c: &Channel, content: &str) -> Option<(&'static str, u64)> {
    let g = c.guild_id?;
    let gs = store.guilds.get(&g)?;
    if gs.member_count <= MASS_THRESHOLD || !store.permissions(c.id).contains(P::MENTION_EVERYONE) {
        return None;
    }
    // Code spans and blocks don't ping.
    let plain: String = content.split('`').step_by(2).collect();
    if plain.contains("@everyone") {
        Some(("@everyone", gs.member_count as u64))
    } else if plain.contains("@here") {
        Some(("@here", gs.online_count.max(1) as u64))
    } else {
        None
    }
}

/// The confirmation, while a send waits on it.
pub fn mass_popout<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some((ch, mention, n)) = st.social.mass else { return };
    let cancel = move |s: &mut S| acc(s).social.mass = None;
    let line = if mention == "@here" { format!("This will notify up to {n} online members in this channel. Are you sure you want to do this?") } else { format!("This will notify {n} members in this channel. Are you sure you want to do this?") };
    let go = move |s: &mut S| {
        let sh = acc(s);
        sh.social.mass = None;
        crate::shell::send_confirmed(sh, ch);
    };
    layer.push(
        Dialog::new(crate::i18n::tf("Mention {mention}?", &[("mention", mention)]), cancel)
            .body(desc(line, p))
            .footer(
                row()
                    .gap(8.0)
                    .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("mass-cancel").on_press(cancel).el(p))
                    .child(Button::new(t("Continue")).key("mass-confirm").on_press(go).el(p)),
            )
            .el(p),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflux_model::DispatchEvent;

    #[test]
    fn blocked_runs_group_consecutive_messages() {
        let mut store = Store::default();
        let ready = json!({"session_id": "x", "user": {"id": "1", "username": "me"}, "relationships": [{"id": "9", "type": 2}],
            "guilds": [], "private_channels": [], "read_states": []});
        store.apply(&DispatchEvent::parse("READY", &ready).unwrap(), 0);
        let m = |id: u64, a: u64| -> Message { serde_json::from_value(json!({"id": id.to_string(), "channel_id": "5", "author": {"id": a.to_string(), "username": "x"}, "content": "", "timestamp": "2026-01-01T00:00:00Z"})).unwrap() };
        let msgs = [m(1, 2), m(2, 9), m(3, 9), m(4, 2), m(5, 9)];
        assert_eq!(blocked_runs(&store, &msgs), [(1, 2), (4, 1)]);
    }

    #[test]
    fn mass_mentions_skip_code() {
        let mut store = Store::default();
        let ready = json!({"session_id": "x", "user": {"id": "1", "username": "me"},
            "guilds": [{"id": "10", "properties": {"id": "10", "name": "G", "owner_id": "1"}, "member_count": 120, "roles": [{"id": "10", "name": "@everyone", "permissions": "0"}],
                        "channels": [{"id": "11", "type": 0, "name": "general"}], "members": [{"user": {"id": "1", "username": "me"}, "roles": []}]}],
            "private_channels": [], "read_states": []});
        store.apply(&DispatchEvent::parse("READY", &ready).unwrap(), 0);
        let c = store.channels[&Snowflake(11)].clone();
        assert_eq!(mass_mention(&store, &c, "hey @everyone"), Some(("@everyone", 120)), "owners may mention everyone");
        assert_eq!(mass_mention(&store, &c, "`@everyone` is how"), None);
        assert_eq!(mass_mention(&store, &c, "hi"), None);
    }
}
