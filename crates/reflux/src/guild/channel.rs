//! Channel settings (`ChannelSettingsModal`, `channel_tabs/ChannelOverviewTab`,
//! `ChannelPermissionsTab`) in the user settings frame, with Delete channel at the nav's foot.

use reflux_model::{ChannelType, OverwriteType, PermissionOverwrite, Permissions as P, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, Icon, col, icon, row, text};
use serde_json::{Value, json};

use super::modals::confirm;
use super::settings::{card, color_dot, empty, save_bar, small, sorted_roles, title};
use super::{Confirm, Dialog, Form, Then, error, field, perms};
use crate::icons;
use crate::settings::tabs::{not_available, section};
use crate::settings::{colors, frame, nav_group, nav_item};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Select, Switch, mix, popup_access};

/// (id, label, icon, permission needed).
const TABS: &[(&str, &str, &Icon, P)] = &[
    ("overview", "Overview", &icons::GEAR, P::MANAGE_CHANNELS),
    ("permissions", "Permissions", &icons::SHIELD, P::MANAGE_ROLES),
    ("invites", "Invites", &icons::TICKET, P::MANAGE_CHANNELS),
    ("webhooks", "Webhooks", &icons::WEBHOOKS_LOGO, P::MANAGE_WEBHOOKS),
];

/// The first tab you may open on a channel, if any.
pub fn first_tab(store: &Store, channel: Snowflake) -> Option<&'static str> {
    let have = store.permissions(channel);
    TABS.iter().find(|t| have.contains(t.3)).map(|t| t.0)
}

/// `SLOWMODE_OPTIONS` (seconds).
pub(super) const SLOWMODE: [(&str, &str); 14] = [
    ("0", "Off"),
    ("5", "5 seconds"),
    ("10", "10 seconds"),
    ("15", "15 seconds"),
    ("30", "30 seconds"),
    ("60", "1 minute"),
    ("120", "2 minutes"),
    ("300", "5 minutes"),
    ("600", "10 minutes"),
    ("900", "15 minutes"),
    ("1800", "30 minutes"),
    ("3600", "1 hour"),
    ("7200", "2 hours"),
    ("21600", "6 hours"),
];

#[allow(clippy::too_many_arguments)]
pub fn view<S: 'static>(store: &Store, st: &ShellState, channel: Snowflake, tab: &'static str, target: Option<Snowflake>, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some(ch) = store.channels.get(&channel) else { return };
    let c = colors(p);
    let mut popups = Vec::new();
    let close = move |s: &mut S| acc(s).dialog = None;
    let have = store.permissions(channel);
    let name = ch.name.clone().unwrap_or_default();
    let head = row()
        .w_full()
        .pt(6.0)
        .pb(6.0)
        .pl(10.0)
        .pr(4.0)
        .mb(24.0)
        .gap(6.0)
        .center_y()
        .child(icon(crate::shell::channel_icon(ch.kind), 16.0).color(p.text_primary_muted))
        .child(text(name).font(font(17.07, 500.0).lh(22.0)).color(p.text_primary).truncate().grow(1.0).shrink());
    let items: Vec<El> = TABS
        .iter()
        .filter(|t| have.contains(t.3))
        .map(|&(id, label, glyph, _)| {
            nav_item(glyph, label, tab == id, false, p, &c).on_press(move |s: &mut S| {
                let sh = acc(s);
                sh.popups.close();
                sh.form = Form::default();
                sh.dialog = Some(Dialog::Channel { channel, tab: id, target: None });
                crate::social::load_channel_tab(sh, channel, id);
            })
        })
        .collect();
    let mut nav = col().w_full().gap(3.0).child(nav_group(None, items, p));
    if have.contains(P::MANAGE_CHANNELS) {
        let label = if ch.kind == ChannelType::GUILD_CATEGORY { "Delete category" } else { "Delete channel" };
        nav = nav.child(nav_item(&icons::TRASH, label, false, true, p, &c).on_press(move |s: &mut S| acc(s).form.confirm = Some(Confirm::DeleteChannel(channel))));
    }
    let label = TABS.iter().find(|t| t.0 == tab).map_or("Channel settings", |t| t.1);
    let body = match tab {
        "overview" => overview(store, st, channel, p, acc, &mut popups),
        "permissions" => match target {
            Some(t) => overwrite_editor(store, st, channel, t, p, acc),
            None => overwrites(store, st, channel, p, acc, &mut popups),
        },
        _ => crate::social::channel_tab(store, st, channel, tab, p, acc).unwrap_or_else(|| not_available(p)),
    };
    let mut modal = frame("Channel settings", label, vec![head], nav, body, close, p);
    if let Some(cf) = st.form.confirm {
        modal = modal.child(confirm(store, st, cf, p, acc));
    }
    layer.push(modal);
    layer.extend(popups);
}

// ---- Overview -------------------------------------------------------------------------------

fn overview<S: 'static>(store: &Store, st: &ShellState, id: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some(ch) = store.channels.get(&id) else { return not_available(p) };
    let f = &st.form;
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let cur_name = ch.name.clone().unwrap_or_default();
    let cur_topic = ch.topic.clone().unwrap_or_default();
    let texty = matches!(ch.kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT);
    let mut content = col().w_full().gap(20.0).child(field(st, acc, "name", &cur_name, "Channel name", "", p));
    let mut patch = serde_json::Map::new();
    let name = f.value("name", &cur_name);
    if name.trim() != cur_name && !name.trim().is_empty() {
        patch.insert("name".into(), json!(name.trim()));
    }
    if texty {
        // shortcut: a single-line topic field (Fluxer's is a growing textarea).
        content = content.child(field(st, acc, "topic", &cur_topic, "Channel topic", "Let everyone know how to use this channel!", p));
        let topic = f.value("topic", &cur_topic);
        if topic.trim() != cur_topic {
            patch.insert("topic".into(), if topic.trim().is_empty() { Value::Null } else { json!(topic.trim()) });
        }
        let slow = f.data.get("slowmode").and_then(Value::as_str).map_or(ch.rate_limit_per_user.to_string(), str::to_string);
        content = content.child(
            col().w_full().gap(8.0).child(
                Select::new("slowmode", &SLOWMODE, Some(&slow))
                    .label("Slowmode")
                    .width(340.0)
                    .on_choose(move |s: &mut S, v| {
                        acc(s).form.data.insert("slowmode".into(), json!(v));
                    })
                    .el(&st.popups, pops, p, layer),
            ).child(small("Members have to wait this long between messages, unless they can bypass slowmode.", p).wrap()),
        );
        if slow != ch.rate_limit_per_user.to_string() {
            patch.insert("rate_limit_per_user".into(), json!(slow.parse::<u32>().unwrap_or(0)));
        }
        let nsfw = f.data.get("nsfw").and_then(Value::as_bool).unwrap_or(ch.nsfw);
        content = content.child(
            Switch::new(nsfw)
                .label("Age-restricted channel")
                .description("Members have to confirm they're of age to view this channel.")
                .on_toggle(move |s: &mut S| {
                    acc(s).form.data.insert("nsfw".into(), json!(!nsfw));
                })
                .el(p),
        );
        if nsfw != ch.nsfw {
            patch.insert("nsfw".into(), json!(nsfw));
        }
    }
    if reflux_store::threads::is_forum(ch.kind) {
        content = super::forum::fields(st, ch, p, acc, layer, content, &mut patch);
    }
    let mut body = col().w_full().child(section("Overview", content, true, p));
    if !patch.is_empty() {
        let patch = Value::Object(patch);
        body = body.child(save_bar(st, p, acc, move |s: &mut S| {
            let sh = acc(s);
            sh.api(Method::PATCH, format!("/channels/{id}"), Some(patch.clone()), Then::Nothing);
            sh.form.text.clear();
            sh.form.data.clear();
        }));
    } else if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    body
}

// ---- Permissions ----------------------------------------------------------------------------

/// Whose overwrite `id` is: a role of the channel's guild, else a member.
fn kind_of(store: &Store, guild: Snowflake, id: Snowflake) -> OverwriteType {
    let role = id == guild || store.guilds.get(&guild).is_some_and(|g| g.roles.iter().any(|r| r.id == id));
    if role { OverwriteType::ROLE } else { OverwriteType::MEMBER }
}

fn subject_name(store: &Store, guild: Snowflake, id: Snowflake) -> String {
    if id == guild {
        return "@everyone".into();
    }
    if let Some(r) = store.guilds.get(&guild).and_then(|g| g.roles.iter().find(|r| r.id == id)) {
        return r.name.clone();
    }
    store.users.get(&id).map_or_else(|| id.to_string(), |u| u.global_name.clone().unwrap_or_else(|| u.username.clone()))
}

fn edit_target(st: &mut ShellState, channel: Snowflake, target: Option<Snowflake>) {
    st.form = Form::default();
    st.dialog = Some(Dialog::Channel { channel, tab: "permissions", target });
}

fn overwrites<S: 'static>(store: &Store, st: &ShellState, id: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some(ch) = store.channels.get(&id) else { return not_available(p) };
    let Some(g) = ch.guild_id else { return not_available(p) };
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let roles = sorted_roles(store, g);
    let mut list: Vec<&PermissionOverwrite> = ch.permission_overwrites.iter().collect();
    // Roles first in rank order, then members.
    list.sort_by_key(|o| (o.kind != OverwriteType::ROLE, roles.iter().position(|r| r.id == o.id).unwrap_or(usize::MAX)));
    let rows: Vec<El> = list
        .iter()
        .map(|o| {
            let t = o.id;
            let lead = if o.kind == OverwriteType::ROLE {
                color_dot(roles.iter().find(|r| r.id == t).map_or(0, |r| r.color), p)
            } else {
                crate::shell::avatar(store, st, t, 24.0, None, p.background_secondary, p)
            };
            let sub = format!("{} allowed · {} denied", o.allow.bits().count_ones(), o.deny.bits().count_ones());
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(lead)
                .child(col().grow(1.0).min_w(0.0).child(title(subject_name(store, g, t), p)).child(small(sub, p)))
                .child(Button::new("Edit").variant(ButtonVariant::Secondary).size(ButtonSize::Compact).on_press(move |s: &mut S| edit_target(acc(s), id, Some(t))).el(p))
        })
        .collect();
    // Roles, and members the client knows, without an overwrite yet.
    let mut add: Vec<(String, String)> = roles.iter().filter(|r| !list.iter().any(|o| o.id == r.id)).map(|r| (r.id.to_string(), if r.id == g { "@everyone".into() } else { format!("@{}", r.name) })).collect();
    if let Some(gs) = store.guilds.get(&g) {
        let mut members: Vec<(String, String)> = gs.members.keys().filter(|u| !list.iter().any(|o| o.id == **u)).map(|&u| (u.to_string(), subject_name(store, g, u))).collect();
        members.sort_by_key(|m| m.1.to_lowercase());
        add.extend(members);
    }
    let refs: Vec<(&str, &str)> = add.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let picker = Select::new("add-overwrite", &refs, None)
        .placeholder("Add a role or member")
        .width(280.0)
        .on_choose(move |s: &mut S, v| {
            if let Ok(t) = v.parse::<Snowflake>() {
                // Edits start from an empty overwrite; saving creates it.
                edit_target(acc(s), id, Some(t));
            }
        })
        .el(&st.popups, pops, p, layer);
    let mut content = col()
        .w_full()
        .gap(16.0)
        .child(row().w_full().gap(12.0).justify_between().center_y().child(small("Overwrites change what roles and members can do in this channel.", p).wrap().shrink()).child(picker));
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    // shortcut: members to add come from the client's cache; there's no member search.
    section("Permissions", content.child(if rows.is_empty() { empty("No overwrites yet.", p) } else { card(rows, p) }), true, p)
}

/// Deny / inherit / allow for one bit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tri {
    Deny,
    Inherit,
    Allow,
}

pub fn tri(allow: P, deny: P, bit: P) -> Tri {
    if allow.contains(bit) {
        Tri::Allow
    } else if deny.contains(bit) {
        Tri::Deny
    } else {
        Tri::Inherit
    }
}

pub fn set_tri(allow: P, deny: P, bit: P, t: Tri) -> (P, P) {
    let (a, d) = (allow - bit, deny - bit);
    match t {
        Tri::Allow => (a | bit, d),
        Tri::Deny => (a, d | bit),
        Tri::Inherit => (a, d),
    }
}

fn overwrite_editor<S: 'static>(store: &Store, st: &ShellState, id: Snowflake, t: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let Some(ch) = store.channels.get(&id) else { return not_available(p) };
    let Some(g) = ch.guild_id else { return not_available(p) };
    let existing = ch.permission_overwrites.iter().find(|o| o.id == t);
    let (cur_allow, cur_deny) = existing.map_or((P::empty(), P::empty()), |o| (o.allow, o.deny));
    let f = &st.form;
    let allow = f.data.get("allow").and_then(Value::as_u64).map_or(cur_allow, P::from_bits_retain);
    let deny = f.data.get("deny").and_then(Value::as_u64).map_or(cur_deny, P::from_bits_retain);
    let kind = existing.map_or_else(|| kind_of(store, g, t), |o| o.kind);
    let back = row()
        .gap(6.0)
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .role("link")
        .on_press(move |s: &mut S| edit_target(acc(s), id, None))
        .child(icon(&icons::ARROW_LEFT_BOLD, 16.0).color(p.text_link))
        .child(text("Back to permissions").font(font(14.0, 500.0).lh(20.0)).color(p.text_link));
    let mut header = row().w_full().justify_between().center_y().child(back);
    if existing.is_some() {
        header = header.child(
            Button::new("Remove overwrite")
                .variant(ButtonVariant::Danger)
                .size(ButtonSize::Small)
                .on_press(move |s: &mut S| {
                    let sh = acc(s);
                    sh.api(Method::DELETE, format!("/channels/{id}/permissions/{t}"), None, Then::Nothing);
                    edit_target(sh, id, None);
                })
                .el(p),
        );
    }
    let mut groups = col().w_full().gap(24.0);
    for (gt, entries) in perms::channel(ch.kind) {
        let mut list = col().w_full().gap(12.0).child(text(gt.to_string()).font(font(12.0, 600.0).lh(16.0)).text_case(rune::TextCase::Uppercase).color(p.text_tertiary));
        for &(bit, name, desc) in entries {
            let now = tri(allow, deny, bit);
            let seg = |state: Tri, glyph: &'static Icon, on_color: rune::Color, label: &str| {
                let on = now == state;
                let (a, d) = set_tri(allow, deny, bit, state);
                row()
                    .size(32.0, 28.0)
                    .center()
                    .fill(if on { on_color } else { p.background_tertiary })
                    .hover(if on { on_color } else { p.background_modifier_hover })
                    .cursor(rune::Cursor::Pointer)
                    .role("radio")
                    .checked(on)
                    .label(format!("{label} {name}"))
                    .on_press(move |s: &mut S| {
                        let d_ = &mut acc(s).form.data;
                        d_.insert("allow".into(), json!(a.bits()));
                        d_.insert("deny".into(), json!(d.bits()));
                    })
                    .child(icon(glyph, 14.0).color(if on { p.text_on_brand_primary } else { p.text_primary_muted }))
            };
            let control = row()
                .radius(6.0)
                .border(1.0, p.background_modifier_accent)
                .child(seg(Tri::Deny, &icons::X_BOLD, p.status_danger, "Deny"))
                .child(seg(Tri::Inherit, &icons::MINUS_BOLD, mix(p.text_primary_muted, p.background_tertiary, 60.0), "Inherit"))
                .child(seg(Tri::Allow, &icons::CHECK_BOLD, p.status_online, "Allow"));
            list = list.child(
                row()
                    .w_full()
                    .gap(16.0)
                    .center_y()
                    .child(col().grow(1.0).min_w(0.0).child(title(name, p)).child(small(desc, p).wrap()))
                    .child(control),
            );
        }
        groups = groups.child(list);
    }
    let lead = if kind == OverwriteType::ROLE { color_dot(store.guilds.get(&g).and_then(|gs| gs.roles.iter().find(|r| r.id == t)).map_or(0, |r| r.color), p) } else { crate::shell::avatar(store, st, t, 24.0, None, p.background_secondary, p) };
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(header)
        .child(row().gap(8.0).center_y().child(lead).child(text(subject_name(store, g, t)).font(font(20.0, 600.0).lh(24.0)).color(p.text_primary)))
        .child(section("Permissions", groups, true, p));
    // An overwrite that doesn't exist yet is saved even when empty, so it shows in the list.
    if existing.is_none() || (allow, deny) != (cur_allow, cur_deny) {
        body = body.child(save_bar(st, p, acc, move |s: &mut S| {
            let sh = acc(s);
            let b = json!({ "type": kind.0, "allow": allow.bits().to_string(), "deny": deny.bits().to_string() });
            sh.api(Method::PUT, format!("/channels/{id}/permissions/{t}"), Some(b), Then::Nothing);
            sh.form.data.clear();
        }));
    } else if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tri_state_moves_a_bit_between_allow_and_deny() {
        let (a, d) = (P::SEND_MESSAGES, P::VIEW_CHANNEL);
        assert_eq!(tri(a, d, P::SEND_MESSAGES), Tri::Allow);
        assert_eq!(tri(a, d, P::VIEW_CHANNEL), Tri::Deny);
        assert_eq!(tri(a, d, P::CONNECT), Tri::Inherit);
        assert_eq!(set_tri(a, d, P::VIEW_CHANNEL, Tri::Allow), (P::SEND_MESSAGES | P::VIEW_CHANNEL, P::empty()));
        assert_eq!(set_tri(a, d, P::SEND_MESSAGES, Tri::Inherit), (P::empty(), P::VIEW_CHANNEL));
    }
}
