//! `GuildMembersPage` (`/channels/{guild}/members`): the "Members" row above a community's
//! channels opens a table of members from `POST /guilds/{id}/members-search` (name, member
//! since, joined Fluxer, join method, top role) with search, sort and paging. A row opens the
//! profile; right-click is the usual member menu (roles, nickname, kick, ban…).
// shortcut: search runs on Enter (Fluxer debounces 300 ms while typing); no column filters
// (member since, account age, join method, roles), no rows-per-page picker (25), no
// indexing poll.

use reflux_model::{Permissions as P, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, TextEdit, col, el, icon, row, text, text_input};
use serde::Deserialize;
use serde_json::json;

use super::{ShellAccess, ShellState, Target};
use crate::i18n::t;
use crate::icons;
use crate::social::Done;
use crate::theme::{Palette, dim, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Placement, mix, over};

const PAGE: u32 = 25;
const KEY: &str = "members-search";

/// `MEMBERS_PAGE_PERMISSIONS`: who sees the row.
pub fn can_view(store: &Store, g: Snowflake) -> bool {
    store.guild_permissions(g).intersects(P::MANAGE_GUILD | P::MANAGE_ROLES | P::MANAGE_NICKNAMES | P::BAN_MEMBERS | P::KICK_MEMBERS | P::MODERATE_MEMBERS)
}

#[derive(Default)]
pub struct MembersPage {
    pub guild: Snowflake,
    pub query: TextEdit,
    /// 1-based.
    pub page: u32,
    pub oldest_first: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Response {
    members: Vec<Member>,
    total_result_count: u64,
    indexing: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Member {
    user_id: Snowflake,
    username: String,
    discriminator: String,
    global_name: Option<String>,
    nickname: Option<String>,
    role_ids: Vec<Snowflake>,
    /// Unix seconds.
    joined_at: i64,
    is_bot: bool,
    supplemental: Supplemental,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Supplemental {
    join_source_type: Option<u8>,
    source_invite_code: Option<String>,
}

impl ShellState {
    /// Shows the members page of `g` and loads its first page.
    pub fn open_members_page(&mut self, g: Snowflake) {
        self.social.members = Some(MembersPage { guild: g, page: 1, ..Default::default() });
        self.social.data.remove(KEY);
        self.search_members();
    }

    fn search_members(&mut self) {
        let Some(m) = &self.social.members else { return };
        let q = m.query.text.trim();
        let mut body = json!({ "limit": PAGE, "offset": (m.page.max(1) - 1) * PAGE, "sort_by": "joinedAt", "sort_order": if m.oldest_first { "asc" } else { "desc" } });
        if !q.is_empty() {
            body["query"] = json!(q);
        }
        let path = format!("/guilds/{}/members-search", m.guild);
        self.social(Method::POST, path, Some(body), Done::Data(KEY.into()));
    }
}

/// The channel-list row (`.membersSection` + `.membersSeparator`).
pub fn sidebar_row<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if !can_view(store, g) {
        return None;
    }
    let selected = st.social.members.as_ref().is_some_and(|m| m.guild == g);
    let fg = if selected { p.surface_interactive_selected_color } else { p.text_tertiary_muted };
    let item = row()
        .grow(1.0)
        .min_w(0.0)
        .ml(8.0)
        .px(8.0)
        .py(6.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .cursor(rune::Cursor::Pointer)
        .role("link")
        .label(t("Members").to_string())
        .key("members-page-row")
        .hover_group()
        .on_press(move |s: &mut S| acc(s).open_members_page(g))
        .child(icon(&icons::USERS, 20.0).color(fg).group_color(if selected { fg } else { p.text_chat }))
        .child(text(t("Members")).font(font(16.0, 500.0).lh(20.0)).color(fg).group_color(if selected { fg } else { p.text_chat }).truncate().grow(1.0).shrink());
    let item = if selected { item.fill(p.surface_interactive_selected_bg) } else { item.hover(p.surface_interactive_hover_bg) };
    Some(col().w_full().child(row().w_full().pt(1.0).pb(2.0).child(item)).child(el().h(1.0).mx(12.0).my(6.0).fill(p.background_modifier_hover)))
}

/// The page itself, in place of the chat.
pub fn view<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let m = st.social.members.as_ref().filter(|m| Some(m.guild) == st.guild)?;
    let g = m.guild;
    let header = row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::CHANNEL_HEADER_PADDING_INLINE)
        .gap(8.0)
        .center_y()
        .child(icon(&icons::USERS, 20.0).color(p.text_tertiary))
        .child(text(t("Members")).font(font(16.0, 600.0).lh(24.0)).color(p.text_primary))
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(over(p.user_area_divider_color, p.background_secondary_lighter)));
    let res: Option<Response> = st.social.data.get(KEY).and_then(|v| serde_json::from_value(v.clone()).ok());
    let total = res.as_ref().map_or(0, |r| r.total_result_count);
    let shown = res.as_ref().map_or(0, |r| r.members.len());
    let border = if m.query.focused { p.background_modifier_accent_focus } else { p.background_modifier_accent };
    let search = row()
        .w(320.0)
        .h(36.0)
        .px(12.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .fill(p.form_surface_background)
        .border(1.0, over(border, p.form_surface_background))
        .child(icon(&icons::MAGNIFYING_GLASS_BOLD, 16.0).color(p.text_secondary))
        .child(
            text_input(&m.query, font(14.0, 400.0).lh(20.0))
                .single_line()
                .key("members-page-search")
                .grow(1.0)
                .shrink()
                .color(p.text_primary)
                .placeholder(t("Search by username or ID"), p.text_tertiary)
                .caret(p.text_primary, p.background_modifier_selected)
                .label(t("Search by username or ID"))
                .on_edit(move |s: &mut S| -> &mut TextEdit { &mut acc(s).social.members.get_or_insert_with(Default::default).query })
                .on_submit(move |s: &mut S, _| {
                    let sh = acc(s);
                    if let Some(m) = &mut sh.social.members {
                        m.page = 1;
                    }
                    sh.search_members();
                }),
        );
    let oldest = m.oldest_first;
    let sort = Button::new(if oldest { t("Oldest first") } else { t("Newest first") })
        .variant(ButtonVariant::Secondary)
        .size(ButtonSize::Compact)
        .icon(&icons::SORT_ASCENDING)
        .key("members-page-sort")
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            if let Some(m) = &mut sh.social.members {
                (m.oldest_first, m.page) = (!oldest, 1);
            }
            sh.search_members();
        })
        .el(p);
    let count = crate::i18n::tf("Showing {displayedCount, number} of {totalCount, plural, one {# total member} other {# total members}}", &[("displayedCount", &shown.to_string()), ("totalCount", &total.to_string())]);
    let toolbar = row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(row().gap(12.0).center_y().grow(1.0).min_w(0.0).child(search).child(sort))
        .child(text(count).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted));

    // `--guild-members-columns`: 240 / 120 / 120 / 150 / 160 minimums at 2.2 / 1 / 1 / 1.15 / 1.35.
    let cols: [(f32, f32); 5] = [(240.0, 2.2), (120.0, 1.0), (120.0, 1.0), (150.0, 1.15), (160.0, 1.35)];
    let cell = |i: usize, e: El| row().grow(cols[i].1).w(0.0).min_w(cols[i].0).px(14.0).center_y().child(e);
    let head = |s: &str| text(s.to_uppercase()).font(font(11.0, 700.0).lh(16.0)).color(p.text_tertiary);
    let joined = crate::i18n::tf("Joined {productName}", &[("productName", "Fluxer")]);
    let head_row = row()
        .w_full()
        .py(5.0)
        .child(cell(0, head(t("Name"))))
        .child(cell(1, head(t("Member since"))))
        .child(cell(2, head(&joined)))
        .child(cell(3, head(t("Join method"))))
        .child(cell(4, head(t("Roles"))))
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.background_header_secondary));
    let locale = crate::i18n::locale();
    let date = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).map(|d| crate::desktop::time::short_date(&d.with_timezone(&chrono::Local), locale)).unwrap_or_default();
    let roles = store.guilds.get(&g).map(|gs| gs.roles.as_slice()).unwrap_or_default();
    let body: El = match &res {
        None if st.form.error.is_some() => empty(t("Something went wrong loading members. Try again later."), p),
        None => row().w_full().grow(1.0).center().child(crate::ui::spinner(p.text_primary_muted)),
        Some(r) if r.members.is_empty() => empty(if r.indexing { t("Indexing…") } else { t("Nobody matches that search.") }, p),
        Some(r) => {
            let rows = r.members.iter().map(|mm| {
                let u = mm.user_id;
                let name = mm.nickname.clone().or(mm.global_name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| mm.username.clone());
                let name = match name.trim().chars().next() {
                    Some(c) if store.mask_names => format!("{c}…"),
                    _ => name,
                };
                let tag = if mm.discriminator.is_empty() || mm.discriminator == "0" { mm.username.clone() } else { format!("{}#{}", mm.username, mm.discriminator) };
                let method = match mm.supplemental.join_source_type {
                    Some(0) => t("Community creator").to_string(),
                    Some(1) => mm.supplemental.source_invite_code.as_ref().map_or_else(|| t("Invite").to_string(), |c| crate::i18n::tf("Invite ({sourceInviteCode})", &[("sourceInviteCode", c)])),
                    Some(2) => t("Vanity URL").to_string(),
                    Some(3) => t("Bot invite").to_string(),
                    Some(4) => t("Platform admin").to_string(),
                    Some(6) => t("Explore").to_string(),
                    _ => t("Invite").to_string(),
                };
                let mut mine: Vec<_> = roles.iter().filter(|r| mm.role_ids.contains(&r.id)).collect();
                mine.sort_by_key(|r| std::cmp::Reverse(r.position));
                let mut role_cell = row().gap(4.0).center_y().min_w(0.0);
                if let Some(top) = mine.first() {
                    let c = (top.color != 0).then(|| super::role_color(top.color));
                    let bg = c.map_or(p.background_secondary, |c| mix(c, p.background_secondary_alt, 20.0));
                    role_cell = role_cell.child(
                        row()
                            .gap(6.0)
                            .py(2.0)
                            .px(8.0)
                            .radius(4.0)
                            .center_y()
                            .min_w(0.0)
                            .fill(bg)
                            .child(el().size(9.0, 9.0).radius(5.0).fill(c.unwrap_or(p.text_tertiary)))
                            .child(text(top.name.clone()).font(font(12.0, 500.0).lh(16.0)).color(p.text_primary).truncate().shrink()),
                    );
                }
                if mine.len() > 1 {
                    role_cell = role_cell.child(row().py(2.0).px(6.0).radius(4.0).fill(p.background_secondary).child(text(format!("+{}", mine.len() - 1)).font(font(12.0, 500.0).lh(16.0)).color(p.text_primary_muted)));
                }
                let name_cell = row()
                    .gap(10.0)
                    .center_y()
                    .min_w(0.0)
                    .child(super::avatar(store, st, u, 32.0, None, p.background_secondary_alt, p))
                    .child(
                        col()
                            .min_w(0.0)
                            .child(row().gap(4.0).center_y().child(text(name).font(font(14.0, 600.0).lh(18.2)).color(p.text_primary).truncate().shrink()).children(mm.is_bot.then(|| text("BOT").font(font(10.0, 600.0)).color(p.text_primary_muted))))
                            .child(text(tag).font(font(12.0, 400.0).lh(15.0)).color(p.text_tertiary).truncate()),
                    );
                let small = |s: String| text(s).font(font(14.0, 400.0).lh(18.0)).color(p.text_primary).truncate();
                let key = format!("members-page-row-{u}");
                let anchor = key.clone();
                row()
                    .w_full()
                    .min_h(48.0)
                    .cursor(rune::Cursor::Pointer)
                    .hover(p.background_modifier_hover)
                    .key(key)
                    .on_press(move |s: &mut S| acc(s).open_profile(u, Some(g), String::new(), anchor.clone(), Placement::RightStart))
                    .on_secondary_press(move |s: &mut S, at, _| acc(s).open_menu(Target::User { user: u, guild: Some(g), dm: None }, [at.x, at.y]))
                    .child(cell(0, name_cell))
                    .child(cell(1, small(date(mm.joined_at * 1000))))
                    .child(cell(2, small(date(u.timestamp_ms() as i64))))
                    .child(cell(3, small(method)))
                    .child(cell(4, role_cell))
                    .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.background_header_secondary))
            });
            let list = col().w_full().children(rows);
            crate::ui::scroller(col().w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(list), false, p)
        }
    };
    let pages = total.div_ceil(u64::from(PAGE)).max(1) as u32;
    let page = m.page.max(1);
    let go = move |to: u32| {
        move |s: &mut S| {
            let sh = acc(s);
            if let Some(m) = &mut sh.social.members {
                m.page = to;
            }
            sh.search_members();
        }
    };
    let footer = row()
        .w_full()
        .py(12.0)
        .px(14.0)
        .gap(12.0)
        .center_y()
        .child(text(format!("{page} / {pages}")).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted).grow(1.0))
        .child(Button::new(t("Previous")).variant(ButtonVariant::Secondary).size(ButtonSize::Compact).disabled(page <= 1).key("members-page-prev").on_press(go(page.saturating_sub(1))).el(p))
        .child(Button::new(t("Next")).variant(ButtonVariant::Secondary).size(ButtonSize::Compact).disabled(page >= pages).key("members-page-next").on_press(go(page + 1)).el(p))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(1.0).fill(p.background_header_secondary));
    let table = col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .radius(8.0)
        .clip_rounded(8.0)
        .fill(p.background_secondary_alt)
        .border(1.0, p.background_header_secondary)
        .child(head_row)
        .child(body)
        .child(footer);
    Some(
        col()
            .grow(1.0)
            .min_w(0.0)
            .h_full()
            .fill(p.background_secondary_lighter)
            .child(header)
            .child(col().w_full().grow(1.0).min_h(0.0).py(16.0).px(20.0).gap(12.0).child(toolbar).child(table)),
    )
}

fn empty(msg: &str, p: &Palette) -> El {
    row().w_full().grow(1.0).padding(32.0).center().child(text(msg.to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap())
}

#[cfg(test)]
mod tests {
    #[test]
    fn response_parses() {
        let r: super::Response = serde_json::from_value(serde_json::json!({"guild_id": "1", "members": [{"id": "x", "guild_id": "1", "user_id": "5", "username": "ada",
            "discriminator": "0001", "global_name": null, "nickname": "Ada", "role_ids": ["9"], "joined_at": 1700000000,
            "supplemental": {"join_source_type": 1, "source_invite_code": "abc", "inviter_id": null}, "is_bot": false}],
            "page_result_count": 1, "total_result_count": 40, "indexing": false})).unwrap();
        assert_eq!(r.members[0].nickname.as_deref(), Some("Ada"));
        assert_eq!(r.members[0].supplemental.source_invite_code.as_deref(), Some("abc"));
        assert_eq!(r.total_result_count, 40);
    }
}
