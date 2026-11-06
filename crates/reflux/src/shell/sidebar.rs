//! The channel sidebar, the DM list and the user area (`GuildHeader`, `ChannelListContent`,
//! `ChannelItem`, `ChannelItemSurface`, `direct_message/DirectMessageList`, `UserArea`).

use reflux_model::{Channel, ChannelType, Snowflake};
use reflux_store::Store;
use rune::{Color, El, ScrollSpec, col, el, icon, row, text};

use super::{ShellAccess, ShellState, USER_AREA_OVERLAY, avatar, channel_icon, presence};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{Status, mention_badge, mix, over, scroller};

fn header_bar(p: &Palette) -> El {
    // `border-bottom: 1px solid var(--user-area-divider-color)`, drawn as a child row.
    el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(over(p.user_area_divider_color, p.background_secondary))
}

fn list(content: El, p: &Palette) -> El {
    let viewport = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false });
    scroller(viewport.child(content), false, p)
}

pub fn guild<S: 'static>(store: &Store, st: &ShellState, gid: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let name = store.guilds.get(&gid).and_then(|g| g.info.as_ref()).map_or(String::new(), |i| i.name.clone());
    // `.headerContainer` / `.headerContent`: 56 tall, padding 0 16, gap 4.
    // shortcut: guild banners aren't fetched yet, so the header always shows the plain variant.
    let header = row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::SPACING_4)
        .gap(dim::SPACING_1)
        .center_y()
        .fill(p.background_secondary)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .child(text(name).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).truncate().grow(1.0).shrink())
        .child(icon(&icons::CARET_DOWN, 16.0).color(p.text_primary))
        .child(header_bar(p));

    // `.navigationContainer`: padding-right 8; 10px drop zones above and below.
    let mut groups = col().w_full().pr(dim::SPACING_2).pt(10.0);
    for g in store.channel_groups(gid) {
        let collapsed = g.category.is_some_and(|c| st.collapsed.contains(&c.id));
        if let Some(cat) = g.category {
            groups = groups.child(category(cat, collapsed, p, acc));
        }
        for c in g.text.iter().chain(g.voice.iter()) {
            let selected = st.channel == Some(c.id);
            // A collapsed category still shows its selected and unread channels.
            if collapsed && !selected && !store.has_unread(c.id) {
                continue;
            }
            groups = groups.child(channel_row(store, c, selected, p, acc));
            if c.kind == ChannelType::GUILD_VOICE {
                groups = groups.child(voice_users(store, st, c, p));
            }
        }
    }
    groups = groups.child(el().h(10.0 + USER_AREA_OVERLAY + dim::SPACING_2));
    col()
        .w(dim::LAYOUT_SIDEBAR_WIDTH)
        .h_full()
        .min_h(0.0)
        .fill(p.background_secondary)
        .child(header)
        .child(list(groups, p))
}

/// `.channelItemCategory`: 14/600 name and a 12px bold caret that turns -90° when collapsed.
fn category<S: 'static>(cat: &Channel, collapsed: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let id = cat.id;
    let name = cat.name.clone().unwrap_or_default();
    row().w_full().pt(4.0).child(
        row()
            .grow(1.0)
            .min_w(0.0)
            .ml(8.0)
            .px(8.0)
            .py(6.0)
            .gap(4.0)
            .center_y()
            .radius(6.0)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(name.clone())
            .on_press(move |s: &mut S| {
                let c = &mut acc(s).collapsed;
                if !c.remove(&id) {
                    c.insert(id);
                }
            })
            .child(
                text(name)
                    .font(font(14.0, 600.0).lh(20.0))
                    .color(p.text_tertiary_muted)
                    .group_color(p.text_primary)
                    .truncate()
                    .shrink(),
            )
            .child(
                icon(if collapsed { &icons::CARET_RIGHT_BOLD } else { &icons::CARET_DOWN_BOLD }, 12.0)
                    .color(p.text_tertiary_muted)
                    .group_color(p.text_primary),
            ),
    )
}

fn channel_row<S: 'static>(store: &Store, c: &Channel, selected: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let unread = store.has_unread(c.id);
    let muted = store.is_muted(c);
    let mentions = store.mention_count(c.id);
    // `ChannelItemSurface` swaps the modifier colours for the surface-interactive ones.
    let fg = if selected {
        p.surface_interactive_selected_color
    } else if unread && !muted {
        p.text_secondary
    } else {
        p.text_tertiary_muted
    };
    let id = c.id;
    let link = c.kind == ChannelType::GUILD_LINK;
    let mut item = row()
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
        .label(c.name.clone().unwrap_or_default())
        .hover_group()
        .child(icon(channel_icon(c.kind), 20.0).color(fg).group_color(if selected { fg } else { p.text_chat }))
        .child(
            text(c.name.clone().unwrap_or_default())
                .font(font(16.0, 500.0).lh(20.0))
                .color(fg)
                .group_color(if selected { fg } else { p.text_chat })
                .truncate()
                .grow(1.0)
                .shrink(),
        );
    if let Some(b) = mention_badge(mentions as u64, true, p) {
        item = item.child(b);
    }
    item = if selected {
        item.fill(p.surface_interactive_selected_bg)
    } else {
        item.hover(p.surface_interactive_hover_bg)
    };
    if !link {
        item = item.on_press(move |s: &mut S| acc(s).select_channel(id));
    }
    if muted && !selected {
        item = item.opacity(0.5);
    }
    let mut container = row().w_full().pt(1.0).child(item);
    if unread && !selected {
        // `.unreadIndicator`: an 8px half-pill hanging off the list's left edge.
        container = container.child(
            el().absolute(-4.0, 1.0 + 16.0 - 4.0).size(8.0, 8.0).corners([0.0, 4.0, 4.0, 0.0]).fill(p.text_primary).opacity(if muted { 0.4 } else { 1.0 }),
        );
    }
    container.key(format!("ch-{id}"))
}

/// Members connected to a voice channel, under its row.
// shortcut: approximates `VoiceParticipantItem` (24px avatar row, indented past the icon); no
// speaking ring or mute/deafen glyphs yet.
fn voice_users(store: &Store, st: &ShellState, c: &Channel, p: &Palette) -> El {
    let gid = c.guild_id;
    let mut users: Vec<Snowflake> =
        store.voice_states.iter().filter(|((g, _), v)| *g == gid && v.channel_id == Some(c.id)).map(|((_, u), _)| *u).collect();
    users.sort();
    col().w_full().pl(8.0 + 8.0 + 20.0 + 6.0 - 8.0).children(users.into_iter().map(|u| {
        row()
            .w_full()
            .h(32.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(6.0)
            .hover(p.surface_interactive_hover_bg)
            .child(avatar(st, u, 24.0, None, p.background_secondary, p))
            .child(text(store.display_name(gid, u)).font(font(14.0, 500.0).lh(18.0)).color(p.text_tertiary_muted).truncate().shrink())
    }))
}

/// Home: the DM list.
// shortcut: the friends/notes rows and pinned DMs aren't drawn; only the conversation list.
pub fn home<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let header = row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::SPACING_2)
        .center_y()
        .fill(p.background_secondary)
        .hover(p.background_modifier_hover)
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .child(
            row().w_full().h(28.0).px(dim::SPACING_3).center_y().radius(dim::RADIUS_MD).child(
                text("Find or start a conversation")
                    .font(font(13.0, 500.0))
                    .color(p.text_primary_muted)
                    .group_color(p.text_primary)
                    .truncate(),
            ),
        )
        .child(header_bar(p));
    let me = store.my_id();
    let mut rows = col().w_full().pr(dim::SPACING_2).pt(10.0).gap(2.0);
    for c in store.private_channels() {
        let selected = st.channel == Some(c.id);
        let other = c.recipients.iter().find(|u| u.id != me);
        let name = c.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| {
            c.recipients.iter().filter(|u| u.id != me).map(|u| store.display_name(None, u.id)).collect::<Vec<_>>().join(", ")
        });
        let unread = store.has_unread(c.id);
        let fg = if selected { p.surface_interactive_selected_color } else if unread { p.text_secondary } else { p.text_tertiary_muted };
        let surface = if selected { over(p.surface_interactive_selected_bg, p.background_secondary) } else { p.background_secondary };
        let face = match other {
            Some(u) if c.kind == ChannelType::DM => avatar(st, u.id, 32.0, Some(presence(store, u.id)), surface, p),
            _ => avatar(st, c.id, 32.0, None, surface, p),
        };
        let id = c.id;
        let mut item = row()
            .grow(1.0)
            .min_w(0.0)
            .ml(8.0)
            .h(42.0)
            .px(8.0)
            .gap(12.0)
            .center_y()
            .radius(6.0)
            .cursor(rune::Cursor::Pointer)
            .hover_group()
            .on_press(move |s: &mut S| acc(s).select_channel(id))
            .child(face)
            .child(
                text(name)
                    .font(font(16.0, 500.0).lh(20.0))
                    .color(fg)
                    .group_color(if selected { fg } else { p.text_chat })
                    .truncate()
                    .grow(1.0)
                    .shrink(),
            );
        if let Some(b) = mention_badge(store.mention_count(c.id) as u64, true, p) {
            item = item.child(b);
        }
        item = if selected { item.fill(p.surface_interactive_selected_bg) } else { item.hover(p.surface_interactive_hover_bg) };
        rows = rows.child(row().w_full().child(item));
    }
    rows = rows.child(el().h(10.0 + USER_AREA_OVERLAY + dim::SPACING_2));
    col()
        .w(dim::LAYOUT_SIDEBAR_WIDTH)
        .h_full()
        .min_h(0.0)
        .fill(p.background_secondary)
        .child(header)
        .child(list(rows, p))
}

/// `.userAreaWrapper` + `.userAreaContainer`, floating over the rail and sidebar bottoms.
pub fn user_area<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let me = store.my_id();
    let user = store.me.as_ref().map(|u| &u.base);
    let name = user.map_or(String::new(), |u| u.global_name.clone().unwrap_or_else(|| u.username.clone()));
    let status = store.sessions.first().map(|s| s.status).unwrap_or(store.settings.status);
    let (dot, label) = match status {
        reflux_model::Status::Idle => (Status::Idle, "Idle"),
        reflux_model::Status::Dnd => (Status::Dnd, "Do Not Disturb"),
        reflux_model::Status::Invisible => (Status::Offline, "Invisible"),
        reflux_model::Status::Online => (Status::Online, "Online"),
        _ => (Status::Online, "Online"),
    };
    let surface = p.panel_control_bg;
    let control = |i: &'static rune::Icon, label: &str| {
        row()
            .size(32.0, 32.0)
            .center()
            .radius(dim::RADIUS_MD)
            .hover(mix(p.control_button_normal_text, Color::TRANSPARENT, 10.0))
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label.to_string())
            .child(icon(i, 20.0).color(p.control_button_normal_text).group_color(p.control_button_hover_text))
    };
    let info = row()
        .grow(1.0)
        .min_w(0.0)
        .h(32.0)
        .gap(dim::SPACING_2)
        .center_y()
        .child(avatar(st, me, 32.0, Some(dot), surface, p))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(1.0)
                .child(text(name).font(font(14.0, 500.0).lh(18.0)).color(p.text_primary).truncate())
                .child(
                    text(label)
                        .font(font(11.0, 500.0).lh(14.0))
                        .color(mix(p.text_primary_muted, Color::TRANSPARENT, 85.0))
                        .truncate()
                        .mt(-1.0),
                ),
        );
    let controls = row()
        .gap(dim::SPACING_1)
        .pl(dim::SPACING_3)
        .center_y()
        .child(control(&icons::MICROPHONE, "Mute"))
        .child(control(&icons::SPEAKER_HIGH, "Deafen"))
        .child(control(&icons::GEAR, "User Settings").on_press(move |s: &mut S| acc(s).settings = Some(crate::settings::SettingsUi::new(None))));
    // --user-area-avatar-lead = max(8, 72/2 - 6 - 32/2) = 14.
    let lead = (dim::LAYOUT_GUILD_LIST_WIDTH / 2.0 - dim::FOOTER_BOX_INSET - 16.0).max(dim::FOOTER_BOX_INNER_INSET);
    let inner = row()
        .w_full()
        .min_h(dim::LAYOUT_USER_AREA_HEIGHT)
        .py(dim::FOOTER_BOX_PADDING_Y)
        .pl(lead)
        .pr(dim::FOOTER_BOX_INNER_INSET)
        .gap(dim::SPACING_3)
        .center_y()
        .radius(dim::FOOTER_BOX_RADIUS)
        .fill(surface)
        .box_shadow(rune::Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 1.0, color: p.floating_surface_ring_color, inset: true })
        .child(info)
        .child(controls);
    row()
        .pin(Some(0.0), None, None, Some(0.0))
        .w(dim::LAYOUT_GUILD_LIST_WIDTH + dim::LAYOUT_SIDEBAR_WIDTH + 1.0)
        .px(dim::FOOTER_BOX_INSET)
        .pb(dim::FOOTER_BOX_INSET + 1.0)
        .fill(p.background_secondary)
        .child(inner)
}
