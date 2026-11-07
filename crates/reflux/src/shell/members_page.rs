//! `GuildMembersPage` (`/channels/{guild}/members`): the "Members" row above a community's
//! channels opens a table of members from `POST /guilds/{id}/members-search` (name, member
//! since, joined Fluxer, join method, top role) with search, sort and paging. A row opens the
//! profile; right-click is the usual member menu (roles, nickname, kick, ban…).
//! Search debounces 300 ms; the header's funnels filter by member since, account age, join method
//! and roles; rows per page is 12/25/50/100; while the server is indexing it asks again every 5 s.
//! "Custom range..." opens `GuildMembersDateRangeModal` (after / before date pickers).

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
use crate::ui::{Button, ButtonSize, ButtonVariant, Menu, MenuItem, Placement, mix, over};

const KEY: &str = "members-search";
/// `SEARCH_DEBOUNCE_MS`, `INDEXING_POLL_INTERVAL_MS`, `PAGE_SIZE_OPTIONS`.
const DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);
const INDEXING_POLL: std::time::Duration = std::time::Duration::from_secs(5);
const PAGE_SIZES: [(&str, &str); 4] = [("12", "12"), ("25", "25"), ("50", "50"), ("100", "100")];
const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 86_400_000;
/// The date-range presets (`PAST_*`), as a window back from now.
const PRESETS: [(&str, u64); 7] = [
    ("Past 1 hour", HOUR_MS),
    ("Past 24 hours", 24 * HOUR_MS),
    ("Past 7 days", 7 * DAY_MS),
    ("Past 2 weeks", 14 * DAY_MS),
    ("Past 3 weeks", 21 * DAY_MS),
    ("Past 4 weeks", 28 * DAY_MS),
    ("Past 3 months", 90 * DAY_MS),
];

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
    /// Rows per page (0 = 25).
    pub page_size: u32,
    /// Date filters: a window back from now in ms (member since, joined Fluxer).
    pub since: Option<u64>,
    pub created: Option<u64>,
    /// A custom range instead (`gte`, `lte` in seconds).
    pub since_range: Option<(Option<i64>, Option<i64>)>,
    pub created_range: Option<(Option<i64>, Option<i64>)>,
    /// Join method: an invite code (the vanity code for "Vanity URL").
    pub invite: Option<String>,
    pub roles: Vec<Snowflake>,
    /// A search waiting for typing to pause, or the next indexing poll.
    pub due: Option<std::time::Instant>,
}

impl MembersPage {
    fn size(&self) -> u32 {
        if self.page_size == 0 { 25 } else { self.page_size }
    }

    /// `buildMemberSearchParams`.
    fn params(&self, now_s: u64) -> serde_json::Value {
        let size = self.size();
        let mut body = json!({ "limit": size, "offset": (self.page.max(1) - 1) * size, "sort_by": "joinedAt", "sort_order": if self.oldest_first { "asc" } else { "desc" } });
        let q = self.query.text.trim();
        if !q.is_empty() {
            body["query"] = json!(q);
        }
        if !self.roles.is_empty() {
            body["role_ids"] = json!(self.roles.iter().map(|r| r.to_string()).collect::<Vec<_>>());
        }
        if let Some(w) = self.since {
            body["joined_at_gte"] = json!(now_s.saturating_sub(w / 1000));
        }
        if let Some(w) = self.created {
            body["user_created_at_gte"] = json!(now_s.saturating_sub(w / 1000));
        }
        for (key, r) in [("joined_at", self.since_range), ("user_created_at", self.created_range)] {
            for (end, v) in [("gte", r.and_then(|r| r.0)), ("lte", r.and_then(|r| r.1))] {
                if let Some(v) = v {
                    body[format!("{key}_{end}")] = json!(v);
                }
            }
        }
        if let Some(c) = &self.invite {
            body["source_invite_code"] = json!([c]);
        }
        body
    }
}

/// Once a frame: runs a debounced search or the indexing poll when it's due.
pub fn poll(st: &mut ShellState, tx: &mut rune::Tx) -> bool {
    let indexing = st.social.data.get(KEY).and_then(|v| v.get("indexing")).and_then(serde_json::Value::as_bool) == Some(true);
    let Some(m) = &mut st.social.members else { return false };
    if m.due.is_none() && indexing {
        m.due = Some(std::time::Instant::now() + INDEXING_POLL);
    }
    let Some(due) = m.due else { return false };
    let now = std::time::Instant::now();
    if now < due {
        tx.wake_after(due - now);
        return false;
    }
    if let Some(m) = &mut st.social.members {
        m.due = None;
    }
    st.search_members();
    true
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
        self.search_members();
    }

    fn search_members(&mut self) {
        let Some(m) = &self.social.members else { return };
        let body = m.params(crate::app::now_ms() / 1000);
        let path = format!("/guilds/{}/members-search", m.guild);
        // The table shows progress until the answer (`isSearching`).
        self.social.data.remove(KEY);
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
        .role("link").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
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
pub fn view<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> Option<El> {
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
                .on_edit(move |s: &mut S| -> &mut TextEdit {
                    let m = acc(s).social.members.get_or_insert_with(Default::default);
                    (m.due, m.page) = (Some(std::time::Instant::now() + DEBOUNCE), 1);
                    &mut m.query
                })
                .on_submit(move |s: &mut S, _| {
                    let sh = acc(s);
                    if let Some(m) = &mut sh.social.members {
                        (m.page, m.due) = (1, None);
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
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    // A header funnel opening `menu` (`.filterButton`, 12px, `focus-primary` while active).
    let funnel = |id: &'static str, label: &str, active: bool, menu: Menu<S>, layer: &mut Vec<El>| {
        menu.placement(Placement::BottomEnd).el(&st.popups, pops, p, layer);
        let open = st.popups.is_open(id);
        let c = if active { p.focus_primary } else if open { p.text_primary } else { p.text_tertiary };
        row()
            .padding(2.0)
            .radius(4.0)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label.to_string())
            .key(id)
            .on_press(move |s: &mut S| acc(s).popups.toggle(id))
            .child(icon(if active { &icons::FUNNEL_FILL } else { &icons::FUNNEL_BOLD }, 12.0).color(c))
    };
    let set = move |f: fn(&mut MembersPage)| {
        move |s: &mut S| {
            let sh = acc(s);
            if let Some(m) = &mut sh.social.members {
                f(m);
                (m.page, m.due) = (1, None);
            }
            sh.search_members();
        }
    };
    let presets = |id: &'static str, cur: Option<u64>, pick: fn(&mut MembersPage, Option<u64>)| {
        let mut menu = Menu::new(id).item(MenuItem::new(format!("{id}-all"), t("All")).radio(cur.is_none()).on_select(move |s: &mut S| {
            let sh = acc(s);
            if let Some(m) = &mut sh.social.members {
                pick(m, None);
                (m.page, m.due) = (1, None);
            }
            sh.search_members();
        }));
        for (i, (label, ms)) in PRESETS.iter().enumerate() {
            let ms = *ms;
            menu = menu.item(MenuItem::new(format!("{id}-{i}"), t(label)).radio(cur == Some(ms)).on_select(move |s: &mut S| {
                let sh = acc(s);
                if let Some(m) = &mut sh.social.members {
                    pick(m, Some(ms));
                    (m.page, m.due) = (1, None);
                }
                sh.search_members();
            }));
        }
        menu
    };
    // "Custom range...": the range modal, filled from the current range.
    let custom = |menu: Menu<S>, id: &'static str, which: &'static str, cur: Option<(Option<i64>, Option<i64>)>| {
        menu.item(MenuItem::new(format!("{id}-custom"), t("Custom range...")).radio(cur.is_some()).on_select(move |s: &mut S| {
            let f = &mut acc(s).form;
            f.data.insert("mrange".into(), json!(which));
            for (k, v) in [("after", cur.and_then(|c| c.0)), ("before", cur.and_then(|c| c.1))] {
                let at = v.and_then(|v| chrono::DateTime::from_timestamp(v, 0)).map(|d| d.with_timezone(&chrono::Local));
                f.data.insert(format!("mrange-{k}"), at.map_or(serde_json::Value::Null, |d| json!(d.date_naive().to_string())));
                f.text.insert(format!("mrange-{k}-time"), TextEdit::new(at.map_or("00:00".into(), |d| d.format("%H:%M").to_string())));
            }
        }))
    };
    let since_menu = custom(presets("members-filter-since", m.since.filter(|_| m.since_range.is_none()), |m, v| (m.since, m.since_range) = (v, None)), "members-filter-since", "since", m.since_range);
    let created_menu = custom(presets("members-filter-created", m.created.filter(|_| m.created_range.is_none()), |m, v| (m.created, m.created_range) = (v, None)), "members-filter-created", "created", m.created_range);
    // Join method: all, the vanity URL, and the invite codes on this page.
    let vanity = store.guilds.get(&g).and_then(|gs| gs.info.as_ref()?.vanity_url_code.clone());
    let mut codes: Vec<String> = res.as_ref().map(|r| r.members.iter().filter_map(|x| x.supplemental.source_invite_code.clone()).collect()).unwrap_or_default();
    codes.sort();
    codes.dedup();
    codes.retain(|c| Some(c) != vanity.as_ref());
    let mut join_menu = Menu::new("members-filter-join").item(MenuItem::new("join-all", t("All")).radio(m.invite.is_none()).on_select(set(|m| m.invite = None)));
    for (label, code) in vanity.iter().map(|v| (t("Vanity URL").to_string(), v.clone())).chain(codes.into_iter().map(|c| (c.clone(), c))) {
        let on = m.invite.as_ref() == Some(&code);
        join_menu = join_menu.item(MenuItem::new(format!("join-{code}"), label).radio(on).on_select(move |s: &mut S| {
            let sh = acc(s);
            if let Some(m) = &mut sh.social.members {
                (m.invite, m.page, m.due) = (Some(code.clone()), 1, None);
            }
            sh.search_members();
        }));
    }
    let mut roles_menu = Menu::new("members-filter-roles").item(MenuItem::new("roles-clear", t("Clear all")).disabled(m.roles.is_empty()).on_select(set(|m| m.roles.clear())));
    let mut by_rank: Vec<_> = store.guilds.get(&g).map(|gs| gs.roles.iter().filter(|r| r.id != g).collect()).unwrap_or_default();
    by_rank.sort_by_key(|r| std::cmp::Reverse(r.position));
    for r in by_rank {
        let (rid, on) = (r.id, m.roles.contains(&r.id));
        roles_menu = roles_menu.item(MenuItem::new(format!("role-filter-{rid}"), r.name.clone()).checkbox(on).on_select(move |s: &mut S| {
            let sh = acc(s);
            if let Some(m) = &mut sh.social.members {
                if on { m.roles.retain(|x| *x != rid) } else { m.roles.push(rid) }
                (m.page, m.due) = (1, None);
            }
            sh.search_members();
        }));
    }
    let with = |label: &str, f: El| row().gap(4.0).center_y().child(head(label)).child(f);
    let head_row = row()
        .w_full()
        .py(5.0)
        .child(cell(0, head(t("Name"))))
        .child(cell(1, with(t("Member since"), funnel("members-filter-since", t("Filter by member since"), m.since.is_some() || m.since_range.is_some(), since_menu, layer))))
        .child(cell(2, with(&joined, funnel("members-filter-created", t("Filter by account creation date"), m.created.is_some() || m.created_range.is_some(), created_menu, layer))))
        .child(cell(3, with(t("Join method"), funnel("members-filter-join", t("Filter by join method"), m.invite.is_some(), join_menu, layer))))
        .child(cell(4, with(t("Roles"), funnel("members-filter-roles", t("Filter by roles"), !m.roles.is_empty(), roles_menu, layer))))
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.background_header_secondary));
    let locale = crate::i18n::locale();
    let date = |ms: i64| chrono::DateTime::from_timestamp_millis(ms).map(|d| crate::desktop::time::short_date(&d.with_timezone(&chrono::Local), locale)).unwrap_or_default();
    let roles = store.guilds.get(&g).map(|gs| gs.roles.as_slice()).unwrap_or_default();
    let body: El = match &res {
        None if st.form.error.is_some() => empty(t("Something went wrong loading members. Try again later."), p),
        None => row().w_full().grow(1.0).center().child(crate::ui::spinner(p.text_primary_muted)),
        Some(r) if r.indexing => row().w_full().grow(1.0).center().gap(8.0).child(crate::ui::spinner(p.text_primary_muted)).child(text(t("Indexing…")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted)),
        Some(r) if r.members.is_empty() => empty(t("Nobody matches that search."), p),
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
    let pages = total.div_ceil(u64::from(m.size())).max(1) as u32;
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
        .child(text(t("Rows per page")).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted))
        .child(
            crate::ui::Select::new("members-page-size", &PAGE_SIZES, Some(&m.size().to_string()))
                .width(88.0)
                .on_choose(move |s: &mut S, v| {
                    let sh = acc(s);
                    if let Some(m) = &mut sh.social.members {
                        (m.page_size, m.page, m.due) = (v.parse().unwrap_or(25), 1, None);
                    }
                    sh.search_members();
                })
                .el(&st.popups, pops, p, layer),
        )
        .child(text(format!("{page} / {pages}")).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted).grow(1.0).align(rune::Align::End))
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
    range_modal(st, p, acc, layer);
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

/// `GuildMembersDateRangeModal`: after / before pickers (each bounding the other), Clear / Apply.
fn range_modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some(which) = st.form.data.get("mrange").and_then(serde_json::Value::as_str) else { return };
    let created = which == "created";
    let day = |k: &str| st.form.data.get(&format!("mrange-{k}")).and_then(serde_json::Value::as_str).and_then(|d| d.parse::<chrono::NaiveDate>().ok());
    let (after, before) = (day("after"), day("before"));
    let time = |k: &str| st.form.text.get(&format!("mrange-{k}-time")).map_or("00:00".to_string(), |e| e.text.clone());
    // Seconds of a picked day at its typed time.
    let secs = |d: Option<chrono::NaiveDate>, k: &str| d.and_then(|d| crate::ui::datepicker::with_time(d, &time(k)).and_local_timezone(chrono::Local).earliest()).map(|d| d.timestamp());
    let range = (secs(after, "after"), secs(before, "before"));
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    let close = move |s: &mut S| {
        acc(s).form.data.remove("mrange");
    };
    let apply = move |r: Option<(Option<i64>, Option<i64>)>| {
        move |s: &mut S| {
            let sh = acc(s);
            sh.form.data.remove("mrange");
            if let Some(m) = &mut sh.social.members {
                let r = r.filter(|r| r.0.is_some() || r.1.is_some());
                if created { (m.created, m.created_range) = (None, r) } else { (m.since, m.since_range) = (None, r) }
                (m.page, m.due) = (1, None);
            }
            sh.search_members();
        }
    };
    let mut cal = Vec::new();
    let empty = TextEdit::new("00:00");
    let mut body = col().w_full().gap(16.0);
    for (k, label, value, min, max) in [("after", t("After date"), after, None, before), ("before", t("Before date"), before, after, None)] {
        let (id, tk) = (if k == "after" { "mrange-after" } else { "mrange-before" }, format!("mrange-{k}-time"));
        let edit = st.form.text.get(&tk).unwrap_or(&empty);
        body = body.child(
            crate::ui::DatePicker::new(id, value)
                .label(label)
                .min(min)
                .max(max)
                .time(edit, move |s: &mut S| acc(s).form.text.entry(tk.clone()).or_insert_with(|| TextEdit::new("00:00")))
                .on_pick(move |s: &mut S, d| {
                    acc(s).form.data.insert(id.into(), json!(d.to_string()));
                })
                .el(&st.popups, pops, p, &mut cal),
        );
    }
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Clear")).variant(ButtonVariant::Secondary).on_press(apply(None)).el(p))
        .child(Button::new(t("Apply")).on_press(apply(Some(range))).el(p));
    layer.push(crate::ui::Modal::new(t("Custom date range"), close).body(body).footer(footer).el(p));
    layer.extend(cal);
}

fn empty(msg: &str, p: &Palette) -> El {
    row().w_full().grow(1.0).padding(32.0).center().child(text(msg.to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap())
}

#[cfg(test)]
mod tests {
    #[test]
    fn params_carry_filters_and_page_size() {
        let m = super::MembersPage { page: 3, page_size: 50, since: Some(super::DAY_MS), roles: vec![reflux_model::Snowflake(9)], invite: Some("abc".into()), ..Default::default() };
        let b = m.params(1_000_000);
        assert_eq!((b["limit"].as_u64(), b["offset"].as_u64()), (Some(50), Some(100)));
        assert_eq!(b["joined_at_gte"].as_u64(), Some(1_000_000 - 86_400));
        assert_eq!(b["role_ids"][0], "9");
        assert_eq!(b["source_invite_code"][0], "abc");
        assert!(b.get("query").is_none() && b.get("user_created_at_gte").is_none());
    }

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
