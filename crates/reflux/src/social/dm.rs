//! Direct messages and group DMs: `CreateDMModal` / `AddFriendsToGroupModal` (one friend picker),
//! `EditGroupModal`, `ChangeFriendNicknameModal`, `ChangeGroupDMNicknameModal`, and the group
//! items of the user and channel menus.

use reflux_model::{ChannelType, RelationshipType, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, Tx, col, icon, row, text};
use serde_json::{Value, json};

use super::{Done, Modal, body, close, desc, error, field, footer, label, user_name};
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Checkbox, MenuEntry, MenuItem, Modal as Dialog, over};
use crate::i18n::t;

/// `max_group_dm_recipients` minus you.
const MAX_PICK: usize = 9;

pub fn open_create_dm(st: &mut ShellState) {
    st.open_social(Modal::CreateDm);
}

/// Friends whose name matches `q` (case-insensitive), by name.
pub fn friends_matching(store: &Store, q: &str) -> Vec<Snowflake> {
    let q = q.trim().to_lowercase();
    let mut v: Vec<(String, Snowflake)> = store
        .relationships
        .values()
        .filter(|r| r.kind == RelationshipType::FRIEND)
        .map(|r| (user_name(store, r.id), r.id))
        .filter(|(n, id)| q.is_empty() || n.to_lowercase().contains(&q) || store.users.get(id).is_some_and(|u| u.username.to_lowercase().contains(&q)))
        .collect();
    v.sort_by_key(|(n, _)| n.to_lowercase());
    v.into_iter().map(|(_, id)| id).collect()
}

fn picked(st: &ShellState) -> Vec<Snowflake> {
    st.form.data.get("picked").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// The friend picker; `group` = add to that group DM instead of creating one.
pub fn create_dm<S: 'static>(store: &Store, st: &ShellState, group: Option<Snowflake>, p: &Palette, acc: ShellAccess<S>) -> El {
    let sel = picked(st);
    let existing: Vec<Snowflake> = group.and_then(|c| store.channels.get(&c)).map(|c| c.recipients.iter().map(|u| u.id).collect()).unwrap_or_default();
    let ids: Vec<Snowflake> = friends_matching(store, &st.form.value("q", "")).into_iter().filter(|u| !existing.contains(u)).collect();
    let full = sel.len() >= MAX_PICK.saturating_sub(existing.len());
    let rows: Vec<El> = ids
        .iter()
        .map(|&u| {
            let on = sel.contains(&u);
            let toggle = move |s: &mut S| {
                let f = &mut acc(s).form;
                let mut v: Vec<Snowflake> = f.data.get("picked").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                match v.iter().position(|&x| x == u) {
                    Some(i) => {
                        v.remove(i);
                    }
                    None => v.push(u),
                }
                f.data.insert("picked".into(), json!(v));
            };
            let username = store.users.get(&u).map(|x| x.username.clone()).unwrap_or_default();
            let mut r = row()
                .w_full()
                .py(6.0)
                .px(8.0)
                .gap(12.0)
                .center_y()
                .radius(6.0)
                .hover(p.background_modifier_hover)
                .role("checkbox")
                .checked(on)
                .label(user_name(store, u))
                .key(format!("pick-{u}"))
                .child(crate::shell::avatar(store, st, u, 32.0, Some(crate::shell::presence(store, u)), p.background_secondary, p))
                .child(col().grow(1.0).min_w(0.0).child(text(user_name(store, u)).font(font(14.0, 500.0).lh(18.0)).color(p.text_primary).truncate()).child(text(username).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).truncate()))
                .child(Checkbox::<S>::new(on).el(p));
            if on || !full {
                r = r.cursor(rune::Cursor::Pointer).on_press(toggle);
            } else {
                r = r.opacity(0.5);
            }
            r
        })
        .collect();
    let list = if rows.is_empty() {
        text(if store.relationships.values().any(|r| r.kind == RelationshipType::FRIEND) { t("No friends found") } else { t("You have no friends yet") })
            .font(font(14.0, 400.0).lh(20.0))
            .color(p.text_primary_muted)
            .py(24.0)
            .w_full()
            .align(rune::Align::Center)
    } else {
        col().w_full().max_h(320.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(col().w_full().gap(2.0).children(rows))
    };
    let mut items = vec![desc(t("Choose friends to message."), p), field(st, acc, "q", "", "", "Search friends", p), list];
    if let Some(c) = group {
        items.push(invite_section(st, c, p, acc));
    }
    items.extend(error(st, p));
    let me = store.my_id();
    let go = move |s: &mut S| {
        let sh = acc(s);
        let sel = picked(sh);
        match group {
            Some(c) => {
                for u in &sel {
                    sh.social(Method::PUT, format!("/channels/{c}/recipients/{u}"), None, if Some(u) == sel.last() { Done::Close } else { Done::Nothing });
                }
            }
            None if sel.len() == 1 => sh.social(Method::POST, "/users/@me/channels", Some(json!({ "recipient_id": sel[0] })), Done::OpenChannel),
            None if sel.is_empty() => sh.form.error = Some("Choose at least one friend.".into()),
            None => sh.social(Method::POST, "/users/@me/channels", Some(json!({ "recipients": sel.iter().filter(|&&u| u != me).collect::<Vec<_>>() })), Done::OpenChannel),
        }
    };
    let action = match (group, sel.len()) {
        (Some(_), _) => t("Add"),
        (None, 1) => t("Create DM"),
        _ => t("Create group DM"),
    };
    Dialog::new(t("Select friends"), close(acc)).body(body(items)).footer(footer(st, action, false, "create-dm", acc, go, p)).el(p)
}

/// "or send an invite to a friend:" with a 24-hour group invite link.
fn invite_section<S: 'static>(st: &ShellState, c: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let link = st.form.data.get("invite").and_then(|v| v["code"].as_str()).map(|code| format!("{}/{code}", st.invite_base.trim_end_matches('/')));
    let bg = p.form_surface_background;
    let action = match &link {
        Some(l) => {
            let l = l.clone();
            Button::<S>::new(t("Copy")).size(ButtonSize::Compact).key("group-invite-copy").el(p).on_press_tx(move |_: &mut S, tx: &mut Tx| tx.copy(l.clone()))
        }
        None => Button::new(t("Generate invite link")).size(ButtonSize::Compact).key("group-invite").on_press(move |s: &mut S| {
            acc(s).social(Method::POST, format!("/channels/{c}/invites"), Some(json!({ "max_age": 86400 })), Done::Form("invite".into()))
        }).el(p),
    };
    col()
        .w_full()
        .gap(8.0)
        .child(label(t("or send an invite to a friend:"), p))
        .child(
            row()
                .w_full()
                .min_h(44.0)
                .pl(17.0)
                .pr(5.0)
                .gap(8.0)
                .center_y()
                .radius(8.0)
                .fill(bg)
                .border(1.0, over(p.background_modifier_accent, bg))
                .child(text(link.clone().unwrap_or_default()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).selectable(true).truncate().grow(1.0).shrink())
                .child(action),
        )
        .children(link.map(|_| text(t("Your invite expires in 24 hours")).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted)))
}

/// `EditGroupModal`: icon and name.
pub fn edit_group<S: 'static>(store: &Store, st: &ShellState, c: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let Some(ch) = store.channels.get(&c) else { return Dialog::new(t("Edit group"), close(acc)).el(p) };
    let cur = ch.name.clone().unwrap_or_default();
    let icon_pick = st.form.data.get("icon").cloned();
    let face = match &icon_pick {
        Some(Value::String(_)) => row().size(80.0, 80.0).center().radius(40.0).fill(p.brand_primary).child(icon(&icons::IMAGE_FILL, 32.0).color(p.brand_primary_fill)),
        _ => crate::shell::group_avatar(store, st, ch, 80.0, p.background_secondary, p),
    };
    let has_icon = matches!(icon_pick, Some(Value::String(_))) || (icon_pick.is_none() && ch.icon.is_some());
    let mut buttons = row().gap(8.0).child(Button::<S>::new(if has_icon { t("Change icon") } else { t("Upload icon") }).variant(ButtonVariant::Secondary).size(ButtonSize::Small).el(p).on_press_tx(crate::guild::pick_image(acc, "icon")));
    if has_icon {
        buttons = buttons.child(Button::new(t("Remove icon")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).on_press(move |s: &mut S| {
            acc(s).form.data.insert("icon".into(), Value::Null);
        }).el(p));
    }
    let placeholder = if cur.is_empty() { crate::shell::channel_title(store, ch) } else { "My group".into() };
    let mut items = vec![
        col().gap(8.0).child(label(t("Group icon"), p)).child(row().gap(16.0).center_y().child(face).child(buttons)),
        field(st, acc, "name", &cur, t("Group name"), &placeholder, p),
    ];
    items.extend(error(st, p));
    let go = move |s: &mut S| {
        let sh = acc(s);
        let name = sh.form.value("name", &cur).trim().to_string();
        if name.chars().count() > 100 {
            sh.form.error = Some(t("Group name must not exceed 100 characters").into());
            return;
        }
        let mut b = json!({ "name": if name.is_empty() { Value::Null } else { json!(name) } });
        if let Some(i) = sh.form.data.get("icon") {
            b["icon"] = i.clone();
        }
        sh.social(Method::PATCH, format!("/channels/{c}"), Some(b), Done::Close);
    };
    Dialog::new(t("Edit group"), close(acc)).body(body(items)).footer(footer(st, t("Save"), false, "group-save", acc, go, p)).el(p)
}

/// `ChangeFriendNicknameModal`: a private name for a friend.
pub fn friend_nickname<S: 'static>(store: &Store, st: &ShellState, u: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let cur = store.relationships.get(&u).and_then(|r| r.nickname.clone()).unwrap_or_default();
    let mut items = vec![field(st, acc, "nick", &cur, "Nickname", &user_name(store, u), p)];
    items.extend(error(st, p));
    let go = move |s: &mut S| {
        let sh = acc(s);
        let nick = sh.form.value("nick", &cur).trim().to_string();
        if nick.chars().count() > 32 {
            sh.form.error = Some(t("Nickname must not exceed 32 characters").into());
            return;
        }
        sh.social(Method::PATCH, format!("/users/@me/relationships/{u}"), Some(json!({ "nickname": if nick.is_empty() { Value::Null } else { json!(nick) } })), Done::Close);
    };
    Dialog::new(t("Change friend nickname"), close(acc)).body(body(items)).footer(footer(st, t("Save"), false, "friend-nick-save", acc, go, p)).el(p)
}

/// `ChangeGroupDMNicknameModal`: the channel's `nicks` map, replaced whole.
pub fn group_nickname<S: 'static>(store: &Store, st: &ShellState, c: Snowflake, u: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let nicks = store.channels.get(&c).map(|ch| ch.nicks.clone()).unwrap_or_default();
    let cur = nicks.get(&u).cloned().unwrap_or_default();
    let mut items = vec![field(st, acc, "nick", &cur, "Nickname", &user_name(store, u), p)];
    items.extend(error(st, p));
    let go = move |s: &mut S| {
        let sh = acc(s);
        let nick = sh.form.value("nick", &cur).trim().to_string();
        let mut all = nicks.clone();
        if nick.is_empty() {
            all.remove(&u);
        } else {
            all.insert(u, nick);
        }
        sh.social(Method::PATCH, format!("/channels/{c}"), Some(json!({ "nicks": all })), Done::Close);
    };
    Dialog::new(t("Change group nickname"), close(acc)).body(body(items)).footer(footer(st, t("Save"), false, "group-nick-save", acc, go, p)).el(p)
}

/// The user menu's items for a member of the group DM on screen (owner only), and Change
/// friend nickname for friends.
pub fn group_items<S: 'static>(store: &Store, st: &ShellState, u: Snowflake, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let me = store.my_id();
    let mut out = Vec::new();
    if u != me && store.relationships.get(&u).is_some_and(|r| r.kind == RelationshipType::FRIEND) {
        out.push(MenuEntry::Item(MenuItem::new("friend-nick", t("Change friend nickname")).icon(&icons::IDENTIFICATION_CARD_FILL).on_select(move |s: &mut S| acc(s).open_social(Modal::FriendNickname(u)))));
    }
    let Some(c) = st.channel.and_then(|c| store.channels.get(&c)).filter(|c| c.kind == ChannelType::GROUP_DM) else { return out };
    let cid = c.id;
    let member = c.recipients.iter().any(|r| r.id == u) || u == me;
    if !member {
        return out;
    }
    out.push(MenuEntry::Separator);
    out.push(MenuEntry::Item(MenuItem::new("group-nick", t("Change group nickname")).icon(&icons::IDENTIFICATION_CARD_FILL).on_select(move |s: &mut S| acc(s).open_social(Modal::GroupNickname { channel: cid, user: u }))));
    if c.owner_id == Some(me) && u != me {
        out.push(MenuEntry::Item(MenuItem::new("group-remove", t("Remove from group")).icon(&icons::USER_MINUS_FILL).danger().on_select(move |s: &mut S| acc(s).social(Method::DELETE, format!("/channels/{cid}/recipients/{u}"), None, Done::Nothing))));
        if store.users.get(&u).is_none_or(|x| !x.bot) {
            out.push(MenuEntry::Item(MenuItem::new("group-owner", "Make group owner").icon(&icons::CROWN_FILL).danger().on_select(move |s: &mut S| acc(s).social(Method::PATCH, format!("/channels/{cid}"), Some(json!({ "owner_id": u })), Done::Nothing))));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflux_model::DispatchEvent;

    #[test]
    fn friend_search_matches_names_and_usernames() {
        let mut store = Store::default();
        let ready = json!({
            "session_id": "x", "user": {"id": "1", "username": "me"},
            "users": [{"id": "2", "username": "ada", "global_name": "Ada Lovelace"}, {"id": "3", "username": "grace", "global_name": "Grace"},
                      {"id": "4", "username": "zed", "global_name": "Zed"}],
            "relationships": [{"id": "2", "type": 1}, {"id": "3", "type": 1}, {"id": "4", "type": 3}],
            "guilds": [], "private_channels": [], "read_states": []
        });
        store.apply(&DispatchEvent::parse("READY", &ready).unwrap(), 0);
        assert_eq!(friends_matching(&store, ""), [Snowflake(2), Snowflake(3)], "friends only, by name");
        assert_eq!(friends_matching(&store, "LOVE"), [Snowflake(2)]);
        assert_eq!(friends_matching(&store, "grac"), [Snowflake(3)]);
        assert!(friends_matching(&store, "zed").is_empty(), "requests aren't friends");
    }
}
