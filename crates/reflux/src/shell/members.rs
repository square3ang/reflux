//! The member list (`MemberListContainer`, `ChannelMembers`, `MemberListItem`,
//! `MemberListGroupHeader`).

use reflux_model::{Channel, MemberListGroup, Snowflake};
use reflux_store::{MemberRow, Store};
use rune::{El, ScrollSpec, col, icon, row, text};

use super::{ShellAccess, ShellState, Target, avatar, presence, role_color};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{Placement, Status, rgb, scroller};
use crate::i18n::t;

/// A list group's label: its hoisted role's name, `Online` or `Offline`.
pub fn group_label(store: &Store, guild: Snowflake, g: &MemberListGroup) -> String {
    match g.id.as_str() {
        "online" => t("Online").into(),
        "offline" => t("Offline").into(),
        id => store.guilds.get(&guild).and_then(|gs| gs.roles.iter().find(|r| r.id.to_string() == id)).map_or_else(|| id.to_string(), |r| r.name.clone()),
    }
}

/// The lazy member list of a channel, from `GUILD_MEMBER_LIST_UPDATE`; reports the rows on
/// screen to [`ShellState::member_rows`] so the runtime subscribes to them.
pub fn list<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let gid = c.guild_id.unwrap_or_default();
    let owner = store.guilds.get(&gid).and_then(|g| g.info.as_ref()).map(|i| i.owner_id);
    let rows = store.member_lists.get(&(gid, c.id.to_string())).map_or(&[][..], |l| l.rows.as_slice());
    let item_h = dim::MEMBER_LIST_ITEM_CONTENT_HEIGHT + 2.0 * dim::MEMBER_LIST_ITEM_PADDING_BLOCK;
    let items = rows.iter().enumerate().map(|(i, r)| match r {
        Some(MemberRow::Group(g)) => {
            let label = format!("{} — {}", group_label(store, gid, g), g.count);
            row()
                .w_full()
                .pt(12.0 + if i > 0 { dim::MEMBER_LIST_GROUP_SPACER_HEIGHT } else { 0.0 })
                .pb(4.0)
                .px(8.0)
                .center_y()
                .children(store.guilds.get(&gid).and_then(|gs| gs.roles.iter().find(|r| r.id.to_string() == g.id)).and_then(|r| super::role_icon::view(st, r, 16.0)).map(|i| i.mr(4.0)))
                .child(text(label).font(font(14.0, 600.0).lh(16.0)).color(p.text_primary_muted).truncate())
                .key(format!("group-{}", g.id))
        }
        Some(MemberRow::Member(u)) => col().w_full().pb(dim::MEMBER_LIST_ROW_GAP).child(member(store, st, gid, *u, owner == Some(*u), p, acc)).key(format!("member-row-{u}")),
        None => col().w_full().pb(dim::MEMBER_LIST_ROW_GAP).child(crate::nav::skeleton::member_row(i, p)).key(format!("member-gap-{i}")),
    });
    let body = col()
        .w_full()
        .pt(dim::MEMBER_LIST_SCROLLER_PADDING_BLOCK_START)
        .px(dim::MEMBER_LIST_SCROLLER_PADDING_INLINE)
        .pb(dim::MEMBER_LIST_SCROLLER_PADDING_BLOCK_END)
        .key(format!("members-{}", c.id))
        .growth(rune::GrowthSpec::default())
        .virtualize(item_h)
        .children(items)
        .on_visible_rows(move |s: &mut S, r| {
            let st = acc(s);
            let changed = st.member_rows != r;
            st.member_rows = r;
            changed
        });
    // Before the first sync: `MemberListSkeleton`.
    let body = if rows.is_empty() { col().w_full().pt(dim::MEMBER_LIST_SCROLLER_PADDING_BLOCK_START).px(dim::MEMBER_LIST_SCROLLER_PADDING_INLINE).child(crate::nav::skeleton::member_list(p)) } else { body };
    let viewport = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(body);
    col()
        .w(dim::LAYOUT_MEMBER_LIST_WIDTH)
        .h_full()
        .min_h(0.0)
        .fill(p.background_secondary_lighter)
        .child(scroller(viewport, false, p))
}

/// The 100-row windows a lazy request asks for: `[0, 99]` always, plus those the visible rows
/// touch (`MemberSidebar` ranges).
pub fn ranges(rows: std::ops::Range<usize>) -> Vec<[u32; 2]> {
    let (first, last) = (rows.start / 100, rows.end.saturating_sub(1).max(rows.start) / 100);
    std::iter::once(0).chain(first.max(1)..=last).map(|i| [(i * 100) as u32, (i * 100 + 99) as u32]).collect()
}

pub(super) fn member<S: 'static>(store: &Store, st: &ShellState, gid: Snowflake, u: Snowflake, owner: bool, p: &Palette, acc: ShellAccess<S>) -> El {
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
    let note = store.notes.get(&u).cloned().unwrap_or_default();
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
        .key(format!("member-{u}"))
        .on_press(move |s: &mut S| acc(s).open_profile(u, Some(gid), note.clone(), format!("member-{u}"), Placement::LeftStart))
        .on_secondary_press(move |s: &mut S, at, _| {
            acc(s).open_user_menu(Target::User { user: u, guild: Some(gid), dm: None }, [at.x, at.y], format!("member-{u}"), Placement::LeftStart)
        })
        .child(avatar(store, st, u, 32.0, Some(status), p.background_secondary_lighter, p))
        .child(info);
    if status == Status::Offline {
        r = r.opacity(0.3);
    }
    r
}

#[cfg(test)]
mod tests {
    #[test]
    fn ranges_cover_the_visible_rows() {
        assert_eq!(super::ranges(0..0), [[0, 99]]);
        assert_eq!(super::ranges(10..40), [[0, 99]]);
        assert_eq!(super::ranges(90..130), [[0, 99], [100, 199]]);
        assert_eq!(super::ranges(250..280), [[0, 99], [200, 299]]);
    }
}
