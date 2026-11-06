//! The member list (`MemberListContainer`, `ChannelMembers`, `MemberListItem`,
//! `MemberListGroupHeader`).

use reflux_model::{Channel, Snowflake};
use reflux_store::Store;
use rune::{El, ScrollSpec, col, icon, row, text};

use super::{ShellState, avatar, presence, role_color};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{Status, rgb, scroller};

/// (group label, members) in display order: hoisted roles (highest first), Online, Offline.
// shortcut: built from the cached members instead of the gateway's GUILD_MEMBER_LIST_UPDATE
// list (whose ID needs Fluxer's per-channel hash); switch once the runtime subscribes to lists.
pub fn groups(store: &Store, guild: Snowflake) -> Vec<(String, Vec<Snowflake>)> {
    let Some(g) = store.guilds.get(&guild) else { return Vec::new() };
    let mut hoisted: Vec<_> = g.roles.iter().filter(|r| r.hoist && r.id != guild).collect();
    hoisted.sort_by_key(|r| (std::cmp::Reverse(r.position), r.id));
    let mut out: Vec<(String, Vec<Snowflake>)> = hoisted.iter().map(|r| (r.name.clone(), Vec::new())).collect();
    let (mut online, mut offline) = (Vec::new(), Vec::new());
    for (id, m) in &g.members {
        if presence(store, *id) == Status::Offline {
            offline.push(*id);
        } else if let Some(i) = hoisted.iter().position(|r| m.roles.contains(&r.id)) {
            out[i].1.push(*id);
        } else {
            online.push(*id);
        }
    }
    out.push(("Online".into(), online));
    out.push(("Offline".into(), offline));
    out.retain(|(_, v)| !v.is_empty());
    for (_, v) in &mut out {
        v.sort_by_cached_key(|u| (store.display_name(Some(guild), *u).to_lowercase(), *u));
    }
    out
}

pub fn list(store: &Store, st: &ShellState, c: &Channel, p: &Palette) -> El {
    let gid = c.guild_id.unwrap_or_default();
    let owner = store.guilds.get(&gid).and_then(|g| g.info.as_ref()).map(|i| i.owner_id);
    let mut body = col()
        .w_full()
        .pt(dim::MEMBER_LIST_SCROLLER_PADDING_BLOCK_START)
        .px(dim::MEMBER_LIST_SCROLLER_PADDING_INLINE)
        .pb(dim::MEMBER_LIST_SCROLLER_PADDING_BLOCK_END);
    for (i, (label, users)) in groups(store, gid).into_iter().enumerate() {
        if i > 0 {
            body = body.child(row().h(dim::MEMBER_LIST_GROUP_SPACER_HEIGHT));
        }
        body = body.child(
            row()
                .w_full()
                .pt(12.0)
                .pb(4.0)
                .px(8.0)
                .child(text(format!("{label} — {}", users.len())).font(font(14.0, 600.0).lh(16.0)).color(p.text_primary_muted).truncate()),
        );
        let rows = col().w_full().gap(dim::MEMBER_LIST_ROW_GAP).children(users.into_iter().map(|u| member(store, st, gid, u, owner == Some(u), p)));
        body = body.child(rows);
    }
    let viewport = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(body);
    col()
        .w(dim::LAYOUT_MEMBER_LIST_WIDTH)
        .h_full()
        .min_h(0.0)
        .fill(p.background_secondary_lighter)
        .child(scroller(viewport, false, p))
}

fn member(store: &Store, st: &ShellState, gid: Snowflake, u: Snowflake, owner: bool, p: &Palette) -> El {
    let status = presence(store, u);
    let color = store.member_color(gid, u).map_or(p.text_chat, role_color);
    let mut name = row()
        .min_w(0.0)
        .center_y()
        .child(text(store.display_name(Some(gid), u)).font(font(16.0, 500.0).lh(dim::MEMBER_LIST_ITEM_NAME_LINE_HEIGHT)).color(color).truncate().shrink());
    if owner {
        // `.crownIcon`: hsl(39, 57%, 64%).
        name = name.child(icon(&icons::CROWN_FILL, 14.0).color(rgb(220, 179, 113)).ml(4.0));
    }
    let custom = store.presences.get(&u).and_then(|pr| pr.custom_status.as_ref()).and_then(|c| c.text.clone()).filter(|t| !t.is_empty());
    let mut info = col().grow(1.0).min_w(0.0).child(name);
    if let Some(t) = custom {
        info = info.child(
            text(t)
                .font(font(11.0, 500.0).lh(dim::MEMBER_LIST_ITEM_STATUS_LINE_HEIGHT))
                .color(crate::ui::mix(p.text_primary_muted, rune::Color::TRANSPARENT, 85.0))
                .truncate(),
        );
    }
    let mut r = row()
        .w_full()
        .min_h(dim::MEMBER_LIST_ITEM_CONTENT_HEIGHT + 2.0 * dim::MEMBER_LIST_ITEM_PADDING_BLOCK)
        .py(dim::MEMBER_LIST_ITEM_PADDING_BLOCK)
        .px(dim::MEMBER_LIST_ITEM_PADDING_INLINE)
        .gap(dim::MEMBER_LIST_ITEM_CONTENT_GAP)
        .center_y()
        .radius(6.0)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .child(avatar(store, st, u, 32.0, Some(status), p.background_secondary_lighter, p))
        .child(info);
    if status == Status::Offline {
        r = r.opacity(0.3);
    }
    r
}
