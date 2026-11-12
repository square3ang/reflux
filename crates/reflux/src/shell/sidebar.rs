//! The channel sidebar, the DM list and the user area (`GuildHeader`, `ChannelListContent`,
//! `ChannelItem`, `ChannelItemSurface`, `direct_message/DirectMessageList`, `UserArea`).

use reflux_model::{Channel, ChannelType, Snowflake};
use reflux_store::Store;
use rune::{Color, El, ScrollSpec, col, el, row, text};
use crate::ui::icon;

use super::{ShellAccess, ShellState, Target, avatar, channel_icon, group_avatar, presence, user_area_overlay, voice};
use crate::icons;
use crate::media::{self, Load};
use crate::theme::{Palette, dim, font};
use crate::ui::{Status, mention_badge, mix, over, rgba, scroller};
use crate::i18n::t;

fn header_bar(p: &Palette) -> El {
    // `border-bottom: 1px solid var(--user-area-divider-color)`, drawn as a child row.
    el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(over(p.user_area_divider_color, p.background_secondary))
}

fn list_following<S: 'static>(content: El, f: impl Fn(rune::ScrollPos) + 'static, p: &Palette) -> El {
    let viewport = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).key("channel-list-banner");
    scroller(crate::ui::chromium_wheel_with::<S>(viewport.child(content.focus_group().role("list")), "channel-list-banner", f), false, p)
}

fn list<S: 'static>(content: El, key: &str, p: &Palette) -> El {
    let viewport = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).key(key);
    scroller(crate::ui::chromium_wheel::<S>(viewport.child(content.focus_group().role("list")), key), false, p)
}

pub fn guild<S: 'static>(store: &Store, st: &ShellState, gid: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let info = store.guilds.get(&gid).and_then(|g| g.info.as_ref());
    let name = info.map_or(String::new(), |i| i.name.clone());
    // `GuildHeader` with a banner: the image (cover, wide-asset aspect 16:9 to 32:9) behind the
    // header and the top of the list, collapsing as the list scrolls ([`super::guild_banner`]).
    let banner = info.and_then(|i| Some((i.banner.as_deref()?, i)));
    let banner = banner.map(|(h, i)| {
        let ratio = match (i.banner_width, i.banner_height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => (w as f32 / h as f32).clamp(16.0 / 9.0, 32.0 / 9.0),
            _ => 16.0 / 9.0,
        };
        let w = dim::LAYOUT_SIDEBAR_WIDTH;
        let viewport_h = rune::memo::layout_size(super::BODY_KEY).map_or(800.0, |s| s[1]);
        let (bh, dist) = super::guild_banner::geometry(w, viewport_h, dim::LAYOUT_HEADER_HEIGHT, ratio);
        let load = st.media.asset(|b, s| media::guild_banner_url(b, gid, h, s));
        let img = match load {
            Load::Ready(px) => el().size(w, bh).pixels_fit(&px, rune::SurfaceFit::Fill),
            _ => el().size(w, bh).fill(p.background_secondary),
        };
        let lives = super::guild_banner::Lives::new(dist);
        (lives.banner(img, w, bh), lives, dist > 0.0)
    });
    // The name and caret: white on the banner, the text colour once the header frosts.
    let tinted = |white: El, plain: El| match &banner {
        Some((_, l, _)) => el().child(plain.live_opacity(&l.frosted)).child(white.pin(Some(0.0), Some(0.0), None, None).live_opacity(&l.on_banner)),
        None => plain,
    };
    // `CaretDownIcon weight="bold"`, turned over (`.caretIconOpen`) while the menu is open.
    let caret = &icons::CARET_DOWN_BOLD;
    let turn = if st.popups.is_open(crate::guild::HEADER_MENU) { std::f32::consts::PI } else { 0.0 };
    let name_el = |c: Color| text(name.clone()).font(font(16.0, 600.0).lh(20.0)).color(c).truncate();
    // `.headerContainer` / `.headerContent`: 56 tall, padding 0 16, gap 4.
    let mut header = row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::SPACING_4)
        .gap(dim::SPACING_1)
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
        .label(name.clone())
        .key(crate::guild::HEADER_MENU)
        .on_press(move |s: &mut S| acc(s).popups.toggle(crate::guild::HEADER_MENU))
        // `GuildBadge` (`visibleCustomBadges={2}`, 20px) before the name.
        // shortcut: the `banner` variant's drop shadow isn't drawn.
        .children(info.and_then(|i| super::invite_embed::guild_badge_sized(&i.features, &i.badge_ids, 20.0, 2, false, st, p, acc)))
        .child(row().key("name").grow(1.0).shrink().min_w(0.0).child(tinted(name_el(Color::WHITE), name_el(p.text_primary)).w_full()))
        .child(tinted(icon(caret, 16.0).color(Color::WHITE).rotate(turn), icon(caret, 16.0).color(p.text_primary).rotate(turn)).key("caret"));
    header = match &banner {
        Some(_) => header,
        None => header.fill(p.background_secondary).hover(p.background_modifier_hover).child(header_bar(p)),
    };
    let header = match &banner {
        // `.headerGlass` (only while it can collapse) and `.bannerGradient` between the image and
        // the header row; the image hangs below the header, behind the list's spacer.
        Some((img, l, collapsible)) => {
            let scrim = el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(40.0).linear_gradient(std::f32::consts::PI, [(0.0, rgba(0, 0, 0, 0.3)), (1.0, Color::TRANSPARENT)]).live_opacity(&l.scrim);
            let glass = collapsible.then(|| el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(p.background_secondary).child(header_bar(p)).live_opacity(&l.glass));
            el().w_full().h(dim::LAYOUT_HEADER_HEIGHT).child(img.clone()).children(glass).child(scrim.key("scrim")).child(header)
        }
        None => header,
    };

    // `.navigationContainer`: padding-right 8; 10px drop zones above and below.
    let mut groups = col().w_full().pr(dim::SPACING_2).pt(10.0).children(banner.as_ref().map(|(_, l, _)| l.spacer()));
    groups = groups.children(super::members_page::sidebar_row(store, st, gid, p, acc).map(|r| r.key("members")));
    let hide_muted = store.guild_settings.get(&Some(gid)).is_some_and(|s| s.hide_muted_channels);
    let connected = |c: &Channel| c.kind == ChannelType::GUILD_VOICE && st.voice.channel == Some((c.guild_id, c.id));
    for g in store.channel_groups(gid) {
        let collapsed = g.category.is_some_and(|c| st.collapsed.contains(&c.id));
        let selected = |c: &Channel| st.channel == Some(c.id) && st.social.members.is_none();
        let shown = |c: &Channel| !hide_muted || selected(c) || connected(c) || !store.is_muted(c);
        let mut voice: Vec<&Channel> = g.voice.iter().copied().filter(|c| shown(c)).collect();
        let connected_here = voice.iter().any(|c| connected(c));
        if let Some(cat) = g.category {
            groups = groups.child(category(cat, collapsed, p, acc).key(format!("cat-{}", cat.id)));
            if collapsed && !connected_here {
                groups = groups.children(super::voice_list::collapsed_category(store, st, cat.id, Some(gid), &voice, p, acc).map(|e| e.key(format!("cat-voice-{}", cat.id))));
            }
        }
        // A collapsed category still shows its selected, connected and unread channels, the
        // connected one first.
        let keep = |c: &Channel| !collapsed || selected(c) || connected(c) || store.has_unread(c.id);
        if collapsed {
            voice.retain(|c| keep(c));
            voice.sort_by_key(|c| !connected(c));
        }
        for c in g.text.iter().copied().filter(|c| shown(c) && keep(c)) {
            groups = groups.child(channel_row(store, st, c, selected(c), p, acc));
            groups = groups.children(super::threads::sidebar_rows(store, st, c, p, acc).map(|t| t.key(format!("threads-{}", c.id))));
        }
        for c in voice {
            groups = groups.child(channel_row(store, st, c, selected(c), p, acc));
            let people = if !collapsed {
                super::voice_list::list(store, st, c, p, acc)
            } else if connected(c) {
                Some(super::voice_list::collapsed_channel(store, st, c, p, acc))
            } else {
                None
            };
            groups = groups.children(people.map(|e| e.key(format!("voice-{}", c.id))));
        }
    }
    groups = groups.child(el().h(10.0 + user_area_overlay(st) + dim::SPACING_2).key("end"));
    col()
        .w(dim::LAYOUT_SIDEBAR_WIDTH)
        .h_full()
        .min_h(0.0)
        .fill(p.background_secondary)
        .clip_rounded(0.0)
        .child(header)
        .child(match &banner {
            Some((_, l, _)) => list_following::<S>(groups, l.follow(), p),
            None => list::<S>(groups, "channel-list", p),
        })
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
            .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
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
                    .grow(1.0)
                    .shrink(),
            )
            .child(
                // `.categoryIcon` sits in `.actionsContainer` (margin-inline-start 0.5rem) at the row's end.
                icon(if collapsed { &icons::CARET_RIGHT_BOLD } else { &icons::CARET_DOWN_BOLD }, 12.0)
                    .ml(8.0)
                    .color(p.text_tertiary_muted)
                    .group_color(p.text_primary),
            ),
    )
}

fn channel_row<S: 'static>(store: &Store, st: &ShellState, c: &Channel, selected: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let muted = store.is_muted(c);
    let (unread, mentions, lit, dim_dot) = super::unread_badges::apply(super::unread_badges::level(store, c), store.has_unread(c.id), store.mention_count(c.id));
    // `ChannelItemSurface` swaps the modifier colours for the surface-interactive ones.
    let fg = if selected {
        p.surface_interactive_selected_color
    } else if lit && !muted {
        p.text_secondary
    } else {
        p.text_tertiary_muted
    };
    let id = c.id;
    let link = c.kind == ChannelType::GUILD_LINK;
    let voice = c.kind == ChannelType::GUILD_VOICE;
    // `isVoiceSelected`: connected here, which counts as selected for the badge and typing.
    let connected = voice && st.voice.channel == Some((c.guild_id, id));
    let can_connect = !voice || store.permissions(id).contains(reflux_model::Permissions::CONNECT);
    // `channelItemHighlight`: connected but viewing another channel, a text-secondary icon.
    let icon_fg = if connected && !selected { p.text_secondary } else { fg };
    let glyph = if voice { super::voice_channel_icon(c, connected && st.voice.encrypted, !can_connect) } else { channel_icon(c.kind) };
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
        .role("link").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
        .label(c.name.clone().unwrap_or_default())
        .hover_group()
        .child(icon(glyph, 20.0).color(icon_fg).group_color(if selected { fg } else { p.text_chat }))
        .child(
            text(c.name.clone().unwrap_or_default())
                .font(font(16.0, 500.0).lh(20.0))
                .color(fg)
                .group_color(if selected { fg } else { p.text_chat })
                .truncate()
                .grow(1.0)
                .shrink(),
        );
    // `channelTypingIndicatorMode`: the typing dots, with the typers' avatars unless "only".
    let mode = crate::look::with(|l| l.typing_indicator);
    // Not on a selected or connected channel (`!isSelected`).
    let typers: Vec<_> = if mode < 2 && !selected && !connected { super::devtools::typing(store, st, c.id) } else { Vec::new() };
    if !typers.is_empty() {
        let dot_c = if selected { p.surface_interactive_selected_color } else { p.text_chat };
        let mut t = row().gap(4.0).center_y().child(typing_dots(dot_c));
        if mode == 0 {
            // `AvatarStack` of 12px avatars, at most 5, overlapping by 4.
            t = t.child(row().children(typers.iter().take(5).enumerate().map(|(i, u)| {
                super::avatar(store, st, *u, 12.0, None, p.background_secondary, p).ml(if i == 0 { 0.0 } else { -4.0 }).key(format!("t-{u}"))
            })));
        }
        item = item.child(t.key("typing"));
    }
    // `hoverAffordancesActive`: hovered, selected, or its context menu is open.
    let menu = st.popups.open.is_some() && st.target == Some(Target::Channel(id));
    let affordances = (selected || st.hover_channel == Some(id) || menu) && c.kind != ChannelType::GUILD_CATEGORY;
    // `showMentionBadge`: not on a connected channel, nor while the affordances show.
    if let Some(b) = mention_badge(mentions as u64, true, p).filter(|_| !affordances && !connected) {
        item = item.child(b.key("mention"));
    }
    // `shouldShowVoiceUserCount`: with a user limit, hidden while hover affordances show.
    let limit = c.user_limit.filter(|l| voice && *l > 0);
    let (icons, has_icons) = affordance_icons(store, c, selected, p, acc);
    if let Some(l) = limit.filter(|_| !(has_icons && affordances)) {
        let count = super::voice_list::connections(store, c.guild_id, id).iter().map(|e| e.1.len()).sum();
        item = item.child(super::voice_list::user_count(count, l, p).key("user-count"));
    }
    if affordances {
        item = item.child(icons.key("affordances"));
    }
    item = item.on_hover_change(move |s: &mut S, inside, _| {
        let st = acc(s);
        if inside {
            st.hover_channel = Some(id);
        } else if st.hover_channel == Some(id) {
            st.hover_channel = None;
        }
    });
    item = if selected {
        item.fill(p.surface_interactive_selected_bg)
    } else {
        item.hover(p.surface_interactive_hover_bg)
    };
    let guild = c.guild_id;
    if voice {
        // `Accessibility.voiceChannelJoinRequiresDoubleClick` / `confirmBeforeJoiningVoiceChannels`;
        // a channel we're in, or can't connect to, only opens.
        let double = crate::look::with(|l| l.join_double_click);
        let joinable = !connected && can_connect;
        item = item.on_press(move |s: &mut S| {
            let sh: &mut ShellState = acc(s);
            if !double && joinable {
                super::voice_join::request(sh, guild, id);
            } else if sh.channel != Some(id) {
                sh.select_channel(id);
            }
        });
        if joinable {
            item = item.on_double_press(move |s: &mut S| acc(s).join_voice(guild, id));
        }
    } else if !link {
        item = item.on_press(move |s: &mut S| acc(s).select_channel(id));
    }
    item = item.on_secondary_press(move |s: &mut S, at, _| acc(s).open_menu(Target::Channel(id), [at.x, at.y]));
    if muted && !selected {
        item = item.opacity(0.5);
    }
    // `.channelItemOver`: the voice channel a dragged participant would land in.
    if voice && st.voice_drag.as_ref().is_some_and(|d| d.over == Some(id)) {
        item = item.fill(p.background_modifier_hover);
    }
    let mut container = row().w_full().pt(1.0).child(item);
    // Muted channels show their (faded) unread pill only with "Show unread indicator on muted
    // channels".
    if unread && !selected && (!muted || crate::look::with(|l| l.faded_unread_muted)) {
        // `.unreadIndicator`: in `GenericChannelItem`'s container (`extraContent`), 4px past its
        // left edge, so the list's edge cuts it to a half pill.
        container = container.child(
            el().absolute(-4.0, 1.0 + 16.0 - 4.0).size(8.0, 8.0).corners([0.0, 4.0, 4.0, 0.0]).fill(p.text_primary).opacity(if muted || dim_dot { 0.4 } else { 1.0 }),
        );
    }
    container.key(format!("ch-{id}"))
}

/// `ChannelItem`'s `hoverAffordance` icons (`ChannelItemIcon`, 16px, 4 apart): Open chat on
/// voice channels, Invite members, Channel settings.
fn affordance_icons<S: 'static>(store: &Store, c: &Channel, selected: bool, p: &Palette, acc: ShellAccess<S>) -> (El, bool) {
    let id = c.id;
    let voice = c.kind == ChannelType::GUILD_VOICE;
    let button = |glyph: &'static rune::Icon, label: &str| {
        let (rest, hover) = if selected { (p.surface_interactive_selected_color, p.surface_interactive_selected_color) } else { (p.text_primary_muted, p.text_primary) };
        row()
            .size(16.0, 16.0)
            .center()
            .radius(999.0)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label.to_owned())
            .hover_group()
            .child(icon(glyph, 16.0).color(rest).group_color(hover))
    };
    let mut r = row().gap(4.0).center_y().ml(8.0);
    let mut any = false;
    if voice && !crate::look::with(|l| l.join_double_click) {
        any = true;
        r = r.child(button(&icons::CHAT_TEARDROP_FILL, t("Open chat")).key("chat").on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.call_view = false;
            sh.select_channel(id);
        }));
    }
    let invitable = matches!(c.kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT | ChannelType::GUILD_VOICE)
        && store.permissions(id).contains(reflux_model::Permissions::CREATE_INSTANT_INVITE);
    if invitable {
        any = true;
        r = r.child(button(&icons::USER_PLUS_FILL, t("Invite members")).key("invite").on_press(move |s: &mut S| crate::guild::open_invite(acc(s), id)));
    }
    if let Some(tab) = crate::guild::first_channel_tab(store, id) {
        any = true;
        r = r.child(button(&icons::GEAR_FILL, t("Channel settings")).key("settings").on_press(move |s: &mut S| acc(s).open_channel_settings(id, tab, voice)));
    }
    (r, any)
}

/// Home: the DM list (`DirectMessageList`, `DMListItem`).
pub fn home<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    // `.dmListHeader`: the quick switcher button with its keybind hint.
    let kbd = |k: &str| {
        row().h(18.0).min_w(18.0).px(4.0).center().radius(dim::RADIUS_SM).fill(p.background_modifier_accent).child(
            text(k).font(font(10.0, 500.0).mono()).color(p.surface_interactive_selected_color),
        )
    };
    let header = row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::SPACING_2)
        .center_y()
        .fill(p.background_secondary)
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .child(
            row()
                .w_full()
                .h(28.0)
                .px(dim::SPACING_3)
                .center_y()
                .radius(dim::RADIUS_MD)
                .hover(p.background_modifier_hover)
                .child(text(t("Quick switcher")).font(font(13.0, 500.0)).color(p.text_primary_muted).group_color(p.text_primary).truncate().grow(1.0).shrink())
                .children((!crate::look::with(|l| l.hide_keyboard_hints)).then(|| row().gap(2.0).child(kbd("Ctrl")).child(kbd("K")))),
        )
        .child(header_bar(p));
    let me = store.my_id();
    let pending = store.relationships.values().filter(|r| r.kind == reflux_model::RelationshipType::INCOMING_REQUEST).count();
    let friends_on = st.guild.is_none() && st.channel.is_none();
    let mut rows = col().w_full().px(dim::SPACING_2).gap(1.0).child(el().h(dim::SPACING_2));
    rows = rows.child(special(&icons::USERS_THREE_FILL, t("Friends"), friends_on && !st.social.discovery, pending, p).on_press(move |s: &mut S| {
        let st = acc(s);
        st.channel = None;
        st.social.discovery = false;
    }));
    if st.personal_notes {
        rows = rows.child(special(&icons::NOTE_PENCIL_FILL, "Personal Notes", st.channel == Some(me), 0, p).key("notes").on_press(move |s: &mut S| acc(s).select_channel(me)));
    }
    rows = rows
        .child(el().h(1.0).mx(-dim::SPACING_2).my(dim::SPACING_2).fill(p.user_area_divider_color).key("divider"))
        .child(
            row()
                .key("dm-header")
                .mt(8.0)
                .mb(8.0)
                .px(8.0)
                .center_y()
                .child(text(t("Direct messages")).font(font(13.0, 500.0).lh(16.0)).color(p.text_primary_muted).grow(1.0))
                .child(
                    row()
                        .size(16.0, 16.0)
                        .center()
                        .hover_group()
                        .cursor(rune::Cursor::Pointer)
                        .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
                        .label(t("Create DM"))
                        .key("create-dm")
                        .on_press(move |s: &mut S| crate::social::open_create_dm(acc(s)))
                        .child(icon(&icons::PLUS_BOLD, 16.0).color(p.text_primary_muted).group_color(p.text_primary)),
                ),
        );
    for c in store.private_channels() {
        rows = rows.child(dm_row(store, st, c, p, acc));
    }
    rows = rows.child(el().h(user_area_overlay(st) + dim::SPACING_2).key("end"));
    col()
        .w(dim::LAYOUT_SIDEBAR_WIDTH)
        .h_full()
        .min_h(0.0)
        .fill(p.background_secondary)
        .child(header)
        .child(list::<S>(rows, "dm-list", p))
}

/// `ClickableItem`: the Friends and Personal Notes rows, a 32px icon disc and a label.
fn special(glyph: &'static rune::Icon, label: &str, selected: bool, badge: usize, p: &Palette) -> El {
    let (disc, glyph_c) = if selected { (p.brand_primary, p.brand_primary_fill) } else { (p.guild_list_foreground, p.text_primary) };
    let fg = if selected { p.surface_interactive_selected_color } else { p.text_primary_muted };
    let mut item = row()
        .w_full()
        .h(42.0)
        .px(8.0)
        .gap(12.0)
        .center_y()
        .radius(dim::RADIUS_MD)
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("link").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
        .label(label.to_string())
        .child(row().size(32.0, 32.0).center().radius(16.0).fill(disc).child(icon(glyph, 20.0).color(glyph_c)))
        .child(text(label).font(font(16.0, 500.0).lh(20.0)).color(fg).group_color(if selected { fg } else { p.text_primary }).grow(1.0));
    if let Some(b) = mention_badge(badge as u64, true, p) {
        item = item.child(b);
    }
    if selected { item.fill(mix(p.surface_interactive_selected_bg, Color::TRANSPARENT, 35.0)) } else { item.hover(p.surface_interactive_hover_bg) }
}

/// `DMListItem` (desktop): avatar, name and subtext, the hover close button, unread pill.
fn dm_row<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let me = store.my_id();
    let id = c.id;
    let selected = st.channel == Some(id);
    let group = c.kind == ChannelType::GROUP_DM;
    let other = c.recipients.iter().find(|u| u.id != me).map(|u| u.id);
    let name = c.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| {
        c.recipients.iter().filter(|u| u.id != me).map(|u| store.display_name(None, u.id)).collect::<Vec<_>>().join(", ")
    });
    let unread = store.has_unread(id);
    let muted = store.is_muted(c);
    let fg = if selected { p.surface_interactive_selected_color } else if unread { p.text_primary } else { p.text_primary_muted };
    let surface = if selected { over(mix(p.surface_interactive_selected_bg, Color::TRANSPARENT, 35.0), p.background_secondary) } else { p.background_secondary };
    let face = match other {
        Some(u) if !group => avatar(store, st, u, 32.0, Some(presence(store, u)), surface, p),
        _ => group_avatar(store, st, c, 32.0, surface, p),
    };
    let sub_c = mix(if selected { p.surface_interactive_selected_color } else { p.text_primary_muted }, Color::TRANSPARENT, 85.0);
    let sub_style = font(11.0, 500.0).lh(if group { 14.0 } else { 16.0 });
    let preview = if group { None } else { dm_preview(store, st, c, unread, sub_style, sub_c, p) };
    let subtext = preview.or_else(|| {
        let s = if group {
            crate::i18n::tp("{count, plural, one {# member} other {# members}}", (c.recipients.len() + 1) as i64, &[])
        } else {
            other.and_then(|u| store.presences.get(&u)?.custom_status.as_ref()?.text.clone()).filter(|t| !t.is_empty())?
        };
        Some(text(s).font(sub_style).color(sub_c).truncate())
    });
    // `.dmItemName`: the pin icon (14px) 0.15rem before the name.
    let mut name_row = row().min_w(0.0).gap(2.4).center_y();
    if store.pinned_dms.contains(&id) {
        name_row = name_row.child(icon(&icons::PUSH_PIN_FILL, 14.0).color(fg).group_color(if selected { fg } else { p.text_primary }).key("pin"));
    }
    let name_row = name_row.child(text(name.clone()).font(font(16.0, 500.0).lh(20.0)).color(fg).group_color(if selected { fg } else { p.text_primary }).truncate().shrink().key("name"));
    let mut info = col().grow(1.0).min_w(0.0).gap(1.0).child(name_row);
    if let Some(t) = subtext {
        info = info.child(t.mt(if group { -2.0 } else { -1.0 }));
    }
    let close = row()
        .size(24.0, 24.0)
        .center()
        .radius(dim::RADIUS_FULL)
        .cursor(rune::Cursor::Pointer)
        .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
        .label(crate::i18n::tf("Close direct message with {displayName}", &[("displayName", &name)]))
        .group_opacity(0.0, 1.0)
        .on_press(move |s: &mut S| {
            let st = acc(s);
            if st.channel == Some(id) {
                st.channel = None;
            }
            st.intents.push(super::Intent::CloseDm(id));
        })
        .child(icon(&icons::X_BOLD, 16.0).color(fg));
    let mut content = row().w_full().gap(12.0).center_y().child(face).child(info);
    if let Some(b) = mention_badge(store.mention_count(id) as u64, true, p) {
        content = content.child(b.key("mention"));
    }
    let target = match other {
        Some(user) if !group => Target::User { user, guild: None, dm: Some(id) },
        _ => Target::Channel(id),
    };
    let mut item = row()
        .w_full()
        .h(42.0)
        .px(8.0)
        .center_y()
        .radius(dim::RADIUS_MD)
        .cursor(rune::Cursor::Pointer)
        .hover_group()
        .role("link").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
        .label(name)
        .key(format!("dm-{id}"))
        .on_press(move |s: &mut S| acc(s).select_channel(id))
        .on_secondary_press(move |s: &mut S, at, _| acc(s).open_user_menu(target, [at.x, at.y], format!("dm-{id}"), crate::ui::Placement::RightStart))
        .child(content.child(close.key("close")));
    item = if selected { item.fill(mix(p.surface_interactive_selected_bg, Color::TRANSPARENT, 35.0)) } else { item.hover(p.surface_interactive_hover_bg) };
    if unread && !selected {
        // `.dmItemUnreadIndicator`: an 8px half-pill 12px past the row's left edge.
        item = item.child(el().absolute(-12.0, 17.0).size(8.0, 8.0).corners([0.0, 4.0, 4.0, 0.0]).fill(p.text_primary));
    }
    if muted {
        item = item.opacity(0.5);
    }
    item
}

/// `.userAreaWrapper` + `.userAreaContainer`, floating over the rail and sidebar bottoms.
pub fn user_area<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let me = store.my_id();
    let user = store.me.as_ref().map(|u| &u.base);
    let name = user.map_or(String::new(), |u| u.global_name.clone().unwrap_or_else(|| u.username.clone()));
    let status = store.sessions.first().map(|s| s.status).unwrap_or(store.settings.status);
    let (dot, label) = match status {
        reflux_model::Status::Idle => (Status::Idle, t("Idle")),
        reflux_model::Status::Dnd => (Status::Dnd, "Do Not Disturb"),
        reflux_model::Status::Invisible => (Status::Offline, t("Invisible")),
        reflux_model::Status::Online => (Status::Online, t("Online")),
        _ => (Status::Online, t("Online")),
    };
    let surface = p.panel_control_bg;
    // `openUserSettingsModal`: until the privacy setup is reviewed, settings opens there and
    // the gear is labelled and dotted.
    let review = crate::settings::privacy::review_pending(store, st);
    // `.controlButton`; `.active` (muted / deafened) turns it danger-red, `.disabled` halves it.
    let control = |i: &'static rune::Icon, label: &str, active: bool, disabled: bool| {
        let (fg, hover_fg) = if active || disabled {
            (p.control_button_danger_text, p.control_button_danger_text)
        } else {
            (p.control_button_normal_text, p.control_button_hover_text)
        };
        let mut b = row()
            .size(32.0, 32.0)
            .center()
            .radius(dim::RADIUS_MD)
            .hover_group()
            .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
            .label(label.to_string())
            .child(icon(i, 20.0).color(fg).group_color(hover_fg));
        if active {
            b = b.fill(mix(p.control_button_danger_text, Color::TRANSPARENT, 10.0)).hover(mix(p.control_button_danger_text, Color::TRANSPARENT, 20.0));
        } else if disabled {
            b = b.hover(mix(p.control_button_danger_text, Color::TRANSPARENT, 15.0));
        } else {
            b = b.hover(mix(p.control_button_normal_text, Color::TRANSPARENT, 10.0));
        }
        if disabled {
            b.opacity(0.5)
        } else {
            b.cursor(rune::Cursor::Pointer)
        }
    };
    let v = &st.voice;
    let mine = v.channel.and_then(|(g, _)| store.voice_states.get(&(g, me)));
    let (server_mute, server_deaf) = mine.map_or((false, false), |s| (s.mute, s.deaf));
    let no_speak = !voice::can(store, v, reflux_model::Permissions::SPEAK);
    let mic_locked = server_mute || server_deaf || no_speak;
    let mic_label = if server_deaf {
        "Deafened by moderators"
    } else if server_mute {
        "Muted by moderators"
    } else if no_speak {
        "You don't have permission to speak in this channel"
    } else if v.self_mute {
        "Unmute microphone"
    } else {
        "Mute microphone"
    };
    let muted = v.self_mute || mic_locked;
    let deaf = v.self_deaf || server_deaf;
    let mut mic = control(if muted { &icons::MICROPHONE_SLASH_FILL } else { &icons::MICROPHONE_FILL }, mic_label, muted && !mic_locked, mic_locked);
    if !mic_locked {
        mic = mic.on_press(move |s: &mut S| acc(s).toggle_mute());
    }
    let deaf_label = if server_deaf { t("Deafened by moderators") } else if v.self_deaf { t("Undeafen") } else { t("Deafen") };
    let mut speaker = control(if deaf { &icons::SPEAKER_SLASH_FILL } else { &icons::SPEAKER_HIGH_FILL }, deaf_label, deaf && !server_deaf, server_deaf);
    if !server_deaf {
        speaker = speaker.on_press(move |s: &mut S| acc(s).toggle_deaf());
    }
    let info = row()
        .grow(1.0)
        .min_w(0.0)
        .h(32.0)
        .gap(dim::SPACING_2)
        .center_y()
        .key(super::status::MENU)
        .radius(dim::RADIUS_MD)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
        .label("User area")
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.popups.toggle(super::status::MENU);
            if sh.popups.is_open(super::status::MENU) {
                sh.fetch_profile(me, None);
            }
        })
        .child(avatar(store, st, me, 32.0, Some(dot), surface, p))
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
    let settings_label = if review { t("User settings, privacy review needed") } else { t("User Settings") };
    let mut gear = control(&icons::GEAR_FILL, settings_label, false, false).on_press(move |s: &mut S| acc(s).open_settings(review.then_some("privacy_safety")));
    if review {
        gear = gear.child(el().pin(Some(21.0), Some(5.0), None, None).size(8.0, 8.0).radius(4.0).fill(p.status_danger).border(2.0, surface));
    }
    let controls = row()
        .gap(dim::SPACING_1)
        .pl(dim::SPACING_3)
        .center_y()
        .child(mic)
        .child(speaker)
        .child(gear);
    // --user-area-avatar-lead = max(8, 72/2 - 6 - 32/2) = 14.
    let lead = (dim::LAYOUT_GUILD_LIST_WIDTH / 2.0 - dim::FOOTER_BOX_INSET - 16.0).max(dim::FOOTER_BOX_INNER_INSET);
    let user_row = row()
        .w_full()
        .min_h(dim::LAYOUT_USER_AREA_HEIGHT)
        .py(dim::FOOTER_BOX_PADDING_Y)
        .pl(lead)
        .pr(dim::FOOTER_BOX_INNER_INSET)
        .gap(dim::SPACING_3)
        .center_y()
        .child(info)
        .child(controls);
    // `.userAreaInnerWrapper`: the voice panel (when in voice) stacked over the user row.
    let mut inner = col()
        .w_full()
        .radius(dim::FOOTER_BOX_RADIUS)
        .clip_rounded(dim::FOOTER_BOX_RADIUS)
        .fill(surface)
        .box_shadow(rune::Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 1.0, color: p.floating_surface_ring_color, inset: true });
    if let Some(panel) = voice::panel(store, st, p, acc) {
        inner = inner.child(panel);
    }
    let inner = inner.child(user_row.key("user"));
    row()
        .pin(Some(0.0), None, None, Some(0.0))
        .w(dim::LAYOUT_GUILD_LIST_WIDTH + dim::LAYOUT_SIDEBAR_WIDTH + 1.0)
        .px(dim::FOOTER_BOX_INSET)
        .pb(dim::FOOTER_BOX_INSET + 1.0)
        .fill(p.background_secondary)
        .child(inner)
}

/// `Typing`: three 4px dots, 2 apart, blinking over 1s staggered 250ms (`Typing.module.css`).
fn typing_dots(c: Color) -> El {
    row().gap(2.0).center_y().children((0..3).map(|i| el().size(4.0, 4.0).radius(2.0).fill(c).looping(rune::LoopSpec::opacity(1.0, 0.25 * i as f64, 0.0, 1.0))))
}

/// `DMListItem` `getMessagePreview`, per "DM message previews" (0 all, 1 unread, 2 none): a
/// system message's text, or "Name: " and the restricted-inline markdown, or an italic
/// "Sent an attachment".
fn dm_preview(store: &Store, st: &ShellState, c: &Channel, unread: bool, style: rune::tok::TextStyle, color: Color, p: &Palette) -> Option<El> {
    let mode = crate::look::with(|l| l.dm_previews);
    if mode == 2 || (mode == 1 && !unread) {
        return None;
    }
    let m = store.messages.get(&c.id)?.messages.last()?;
    if !matches!(m.kind, reflux_model::MessageType::DEFAULT | reflux_model::MessageType::REPLY) {
        let s = super::system_msg::stringify(store, m)?;
        return Some(text(s.strip_suffix('.').unwrap_or(&s).to_string()).font(style).color(color).truncate());
    }
    let who = if m.author.id == store.my_id() { crate::i18n::t("You").to_string() } else { store.display_name(None, m.author.id) };
    let body = if !m.content.is_empty() {
        let md = super::markdown::Md::new(store, &st.media, None, p, format!("dm-preview-{}", c.id));
        super::markdown::inline_text(&m.content, &md, style, color)
    } else if !m.attachments.is_empty() {
        text(crate::i18n::t("Sent an attachment")).font(style.italic()).color(mix(color, Color::TRANSPARENT, 70.0)).truncate()
    } else {
        return None;
    };
    Some(row().min_w(0.0).child(text(format!("{who}: ")).font(style).color(color)).child(body.shrink().min_w(0.0)))
}
