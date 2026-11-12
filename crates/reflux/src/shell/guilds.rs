//! The guild rail (`GuildsLayout.module.css`, `sidebar_nav/*`, `guild/components/GuildAcronym`).

use reflux_model::{GuildFolder, Snowflake};
use reflux_store::{GuildListItem, Store};
use std::rc::Rc;

use rune::{Align, El, ScrollSpec, Shadow, col, el, icon, row, text};

use super::{ShellAccess, ShellState, user_area_overlay, first_channel, role_color};
use crate::icons;
use crate::media::{self, Load};
use crate::nav::RailDrag;
use crate::nav::rail::{self, Drop, ROW, Slot};
use crate::theme::{Palette, dim, font};
use crate::ui::{mention_badge, mix, scroller};
use crate::i18n::t;

const ICON: f32 = dim::GUILD_ICON_SIZE;

pub fn rail<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let rows = Rc::new(rail::rows(store, &st.nav.open));
    // Up/Down move between the rail's items in keyboard mode.
    let mut list = col()
        .w_full()
        .focus_group()
        .role("list")
        .pt(dim::SPACING_1)
        .pb(user_area_overlay(st) + dim::SPACING_2)
        .child(home(store, st, p, acc).key("rail-home"))
        .children(crate::look::with(|l| l.show_favorites).then(|| crate::nav::favorites::rail_button(st, p, acc)))
        .child(divider(p).key("rail-divider"));
    for item in store.guild_list() {
        list = match item {
            GuildListItem::Guild(id) => list.child(guild(store, st, id, &rows, p, acc)),
            GuildListItem::Folder { folder, guilds } => list.child(folder_item(store, st, folder, &guilds, &rows, p, acc)),
        };
    }
    // `buildGuildNavigationRows`: discovery, then add-guild; download is hidden in the desktop app (`!Platform.isElectron`).
    list = list.child(explore_button(st, p, acc).key("rail-explore-row")).child(add_button(p, acc).key("rail-add-row"));
    if crate::look::with(|l| l.show_help_button)
        && let Some(base) = st.marketing_base.as_deref()
    {
        let url = format!("{}/help", base.trim_end_matches('/'));
        list = list.child(rail_action(&icons::QUESTION_BOLD, t("Help center"), "rail-help", p, move |_: &mut S| crate::settings::open_url(&url)));
    }
    let viewport = col().w(dim::LAYOUT_GUILD_LIST_WIDTH).h_full().fill(p.background_secondary).scroll(ScrollSpec { stick_bottom: false }).key("guild-rail-list");
    scroller(crate::ui::chromium_wheel::<S>(viewport.child(list), "guild-rail-list"), false, p).key("guild-rail")
}

/// Makes a rail row draggable (`UseGuildListItemDragAndDrop`): the pointer maps onto the rail's
/// rows, and a release queues the drop for [`crate::nav::poll`].
fn draggable<S: 'static>(e: El, src: Slot, rows: &Rc<Vec<rail::Row>>, acc: ShellAccess<S>) -> El {
    let Some(top) = rows.iter().find(|r| r.slot == src).map(|r| r.top) else { return e };
    let rows = rows.clone();
    e.on_drag(move |s: &mut S, d: &mut rune::Drag, _| {
        let nav = &mut acc(s).nav;
        match d.phase {
            rune::Phase::Down => nav.drag = None,
            rune::Phase::Move => {
                // `DRAG_THRESHOLD`: a short wobble stays a click.
                if nav.drag.is_none() && d.dx.abs().max(d.dy.abs()) < 4.0 {
                    return;
                }
                nav.drag = Some(RailDrag { src, drop: rail::resolve(&rows, src, top + d.y) });
            }
            rune::Phase::Up => {
                if let Some(RailDrag { drop: Some(drop), .. }) = nav.drag.take().filter(|_| !d.cancelled()) {
                    nav.pending = Some((src, drop));
                }
            }
        }
    })
}

/// The drop line over (`top`) or under a row: 2px `brand-primary-light`, box-wide, centred.
fn drop_line(top: bool, p: &Palette) -> El {
    let c = if crate::ui::is_light(p) { p.brand_primary } else { p.brand_primary_light };
    let w = dim::GUILD_LIST_ITEM_BOX_SIZE;
    let y = if top { 0.0 } else { ROW - 2.0 };
    el().absolute((dim::LAYOUT_GUILD_LIST_WIDTH - w) / 2.0, y).size(w, 2.0).radius(1.0).fill(c).inert()
}

/// Drop lines for `slot` while a drag would land beside it.
fn drop_marks(st: &ShellState, slot: Slot, p: &Palette) -> Option<El> {
    match st.nav.drag.and_then(|d| d.drop)? {
        Drop::Before(s) if s == slot => Some(drop_line(true, p)),
        Drop::After(s) if s == slot => Some(drop_line(false, p)),
        _ => None,
    }
}

/// `.guildIndicator`: the white pill on the rail's edge (8 unread, 20 hovered, 40 selected).
fn indicator(h: f32, p: &Palette) -> El {
    let w = dim::GUILD_LIST_INDICATOR_BAR_WIDTH;
    row()
        .absolute(-0.15 * 16.0, (ROW - ICON) / 2.0)
        .size(dim::GUILD_LIST_INDICATOR_TRACK_WIDTH, ICON)
        .center()
        .child(el().size(w, h).corners([0.0, w / 2.0, w / 2.0, 0.0]).fill(p.text_primary))
        .inert()
}

/// One rail slot: pill, 44px icon, and an optional badge, with hover tracking for the pill and
/// the squircle.
#[allow(clippy::too_many_arguments)]
fn slot<S: 'static>(
    target: Option<Snowflake>,
    selected: bool,
    unread: bool,
    st: &ShellState,
    face: El,
    badge: Option<El>,
    voice: Option<&'static rune::Icon>,
    p: &Palette,
    acc: ShellAccess<S>,
) -> El {
    let hovered = st.hover_guild == Some(target);
    let mut item = row().w_full().h(ROW).center();
    if selected || hovered || unread {
        item = item.child(indicator(if selected { 40.0 } else if hovered { 20.0 } else { 8.0 }, p).key("pill"));
    }
    // The keyboard ring hugs the icon (the row is wider than the rail's clip).
    let mut frame = el().size(ICON, ICON).radius(ICON / 2.0).focus_ring(4.0, 0.0, p.focus_primary).child(face);
    if let Some(b) = badge {
        // `.guildBadge` sits 4px past the corner on a 3px `guild-badge-surface` cutout.
        frame = frame.child(
            row()
                .pin(None, None, Some(-4.0), Some(-4.0))
                .radius(dim::RADIUS_MD + 3.0)
                .box_shadow(Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 3.0, color: p.guild_badge_surface, inset: false })
                .child(b),
        );
    }
    if let Some(glyph) = voice {
        // `VoiceBadge`: a 20px status-online circle at the top corner on the same cutout.
        frame = frame.child(
            row()
                .pin(None, Some(-4.0), Some(-4.0), None)
                .size(20.0, 20.0)
                .center()
                .radius(10.0)
                .fill(p.status_online)
                .box_shadow(Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 3.0, color: p.guild_badge_surface, inset: false })
                .child(icon(glyph, 12.0).color(rune::Color::WHITE))
                .key("voice"),
        );
    }
    item.child(frame.key("icon"))
        .cursor(rune::Cursor::Pointer)
        .role("button").focusable(true)
        .on_hover_change(move |s: &mut S, inside, _| {
            let st = acc(s);
            if inside {
                st.hover_guild = Some(target);
            } else if st.hover_guild == Some(target) {
                st.hover_guild = None;
            }
        })
}

fn home<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    // Favorites has no guild either; it lights its own button.
    let selected = st.guild.is_none() && !st.nav.favorites;
    let lit = selected || st.hover_guild == Some(None);
    let (bg, fg) = if lit { (p.brand_primary, p.brand_primary_fill) } else { (p.guilds_layout_item_bg, p.text_primary) };
    let face = row()
        .size(ICON, ICON)
        .center()
        .radius(if lit { ICON * 0.3 } else { ICON / 2.0 })
        .fill(bg)
        // `ChatCircleIcon weight="fill"` (`.messageBubbleIcon`, 1.75rem).
        .child(icon(&icons::CHAT_CIRCLE_FILL, 28.0).color(fg));
    let mentions: u32 = store.private_channels().iter().map(|c| store.mention_count(c.id)).sum();
    slot(None, selected, false, st, face, mention_badge(mentions as u64, true, p), None, p, acc)
        .label("Direct Messages")
        .on_press(move |s: &mut S| acc(s).select_guild(None, None))
}

/// `.guildDividerSlot`: 6 + 2 + 6, a 32-wide line.
fn divider(p: &Palette) -> El {
    row().w_full().py(dim::GUILD_LIST_DIVIDER_BASE_GAP).justify_center().child(
        el().size(32.0, dim::GUILD_LIST_DIVIDER_LINE_SIZE).radius(1.0).fill(p.background_modifier_hover),
    )
}

fn guild<S: 'static>(store: &Store, st: &ShellState, id: Snowflake, rows: &Rc<Vec<rail::Row>>, p: &Palette, acc: ShellAccess<S>) -> El {
    let selected = st.guild == Some(id) && !st.nav.favorites;
    let lit = selected || st.hover_guild == Some(Some(id));
    let name = store.guilds.get(&id).and_then(|g| g.info.as_ref()).map_or("", |i| i.name.as_str());
    let (unread, mentions) = super::unread_badges::guild(store, id);
    let drag = st.nav.drag;
    let face = match drag {
        // `CombinePreview`: the target and the dragged guild in a ringed 2×2 grid.
        Some(RailDrag { src: Slot::Guild(src), drop: Some(Drop::Combine(t)) }) if t == id => {
            let c = if crate::ui::is_light(p) { p.brand_primary } else { p.brand_primary_light };
            let mini = |g| guild_face(store, st, g, (ICON - 10.0) / 2.0, true, p);
            row()
                .size(ICON, ICON)
                .padding(4.0)
                .gap(2.0)
                .items_start()
                .radius(ICON * 0.3)
                .fill(mix(c, p.background_secondary, 8.0))
                .outline(2.0, 1.0, c)
                .child(mini(id))
                .child(mini(src))
        }
        _ => guild_face(store, st, id, ICON, lit, p),
    };
    let first = first_channel(store, id);
    let dragging = drag.is_some_and(|d| d.src == Slot::Guild(id));
    // `resolveVoiceBadgeActivity`: screen share, then camera, then plain voice.
    let states: Vec<_> = store.voice_states.iter().filter(|((g, _), v)| *g == Some(id) && v.channel_id.is_some()).map(|(_, v)| v).collect();
    let voice = (!states.is_empty()).then(|| {
        if states.iter().any(|v| v.self_stream) {
            &icons::MONITOR_PLAY_FILL
        } else if states.iter().any(|v| v.self_video) {
            &icons::VIDEO_CAMERA_FILL
        } else {
            &icons::SPEAKER_HIGH_FILL
        }
    });
    let item = slot(Some(id), selected, unread, st, face, mention_badge(mentions as u64, true, p), voice, p, acc)
        .label(name.to_string())
        .key(format!("guild-{id}"))
        .opacity(if dragging { 0.5 } else { 1.0 })
        .children(drop_marks(st, Slot::Guild(id), p))
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.nav.favorites = false;
            sh.select_guild(Some(id), first)
        })
        .on_secondary_press(move |s: &mut S, at, _| acc(s).open_menu(super::Target::Guild(id), [at.x, at.y]));
    draggable(item, Slot::Guild(id), rows, acc)
}

/// `GuildIcon`: the icon from the media proxy, else the acronym; a circle that rounds to a
/// squircle (30%) when lit.
pub(crate) fn guild_face(store: &Store, st: &ShellState, id: Snowflake, size: f32, lit: bool, p: &Palette) -> El {
    let info = store.guilds.get(&id).and_then(|g| g.info.as_ref());
    let radius = if lit { size * 0.3 } else { size / 2.0 };
    let load = info.and_then(|i| i.icon.as_deref()).map(|h| st.media.asset(|b, s| media::guild_icon_url(b, id, h, size, s)));
    match load {
        Some(Load::Ready(px)) => el().size(size, size).radius(radius).pixels_fit(&px, rune::SurfaceFit::Fill),
        Some(Load::Pending) => el().size(size, size).radius(radius).fill(p.guilds_layout_item_bg),
        Some(Load::Failed) | None => acronym_face(info.map_or("", |i| i.name.as_str()), size, lit, p),
    }
}

pub(super) fn acronym_face(name: &str, size: f32, lit: bool, p: &Palette) -> El {
    let (bg, fg) = if lit { (p.brand_primary, p.brand_primary_fill) } else { (p.guilds_layout_item_bg, p.text_primary) };
    let a = acronym(name);
    row()
        .size(size, size)
        .center()
        .radius(if lit { size * 0.3 } else { size / 2.0 })
        .fill(bg)
        .child(text(a.clone()).font(font(acronym_font_size(&a, size), 500.0).lh(acronym_font_size(&a, size))).color(fg).align(Align::Center))
}

/// `getGuildAcronym`: drop possessive `'s`, then the first character of every word run.
pub fn acronym(name: &str) -> String {
    let name = name.replace("'s ", " ").replace("’s ", " ");
    let mut out = String::new();
    let mut in_word = false;
    for c in name.chars() {
        let word = c.is_alphanumeric() || c == '_';
        if (word && !in_word) || (!word && !c.is_whitespace()) {
            out.push(c);
        }
        in_word = word;
    }
    if out.is_empty() { "?".into() } else { out }
}

/// `getFittedGuildAcronymFontScale`: 18 18 16 14 12 10 px per 48px icon by length, then a
/// smaller step (or the exact fit) when the text is wider than the icon less 4px each side.
fn acronym_font_size(a: &str, size: f32) -> f32 {
    // `measureText` at 100px, weight 500, in the UI font.
    let (w, _) = rune::text::measure(a, &[], font(100.0, 500.0), None);
    (fit_scale(a.chars().count(), w / 100.0) * size).max(8.0)
}

/// `fitGuildAcronymFontScale`.
fn fit_scale(len: usize, width_per_em: f32) -> f32 {
    const SIZES: [f32; 6] = [18.0, 18.0, 16.0, 14.0, 12.0, 10.0];
    let scale = SIZES[len.clamp(1, 6) - 1] / 48.0;
    if width_per_em <= 0.0 {
        return scale;
    }
    let max = (48.0 - 8.0) / 48.0 / width_per_em;
    if scale <= max {
        return scale;
    }
    SIZES.iter().map(|s| s / 48.0).find(|&c| c < scale && c <= max).unwrap_or(max)
}

/// The folder's accent: its colour, else `brand-primary-light` (`brand-primary` on light).
fn folder_accent(color: Option<u32>, p: &Palette) -> rune::Color {
    match color.filter(|&c| c != 0) {
        Some(c) => role_color(c),
        None if crate::ui::is_light(p) => p.brand_primary,
        None => p.brand_primary_light,
    }
}

/// `GuildFolderIcons` → the Phosphor fill glyph.
pub(crate) fn folder_glyph(icon: &str) -> &'static rune::Icon {
    match icon {
        "star" => &icons::STAR_FILL,
        "heart" => &icons::HEART_FILL,
        "bookmark" => &icons::BOOKMARK_SIMPLE_FILL,
        "game_controller" => &icons::GAME_CONTROLLER_FILL,
        "shield" => &icons::SHIELD_FILL,
        "music_note" => &icons::MUSIC_NOTE_FILL,
        _ => &icons::FOLDER_FILL,
    }
}

/// `GuildFolderItem`: closed, a 2×2 grid of mini icons (or the folder glyph with
/// SHOW_ICON_WHEN_COLLAPSED) on the folder surface; open, the glyph over a tinted backdrop
/// that runs behind its guilds. A click toggles it; no expand animation.
#[allow(clippy::too_many_arguments)]
fn folder_item<S: 'static>(store: &Store, st: &ShellState, folder: &GuildFolder, guilds: &[Snowflake], rows: &Rc<Vec<rail::Row>>, p: &Palette, acc: ShellAccess<S>) -> El {
    let fid = folder.id;
    let open = st.nav.open.contains(&fid);
    let light = crate::ui::is_light(p);
    let accent = folder_accent(folder.color, p);
    let hovered = st.nav.hover_folder == Some(fid) || st.popups.is_open("ctx-menu") && st.nav.menu_folder == Some(fid);
    let selected = !st.nav.favorites && st.guild.is_some_and(|g| guilds.contains(&g));
    let (fill, base) = match (selected, hovered) {
        (true, _) => (if light { 16.0 } else { 18.0 }, p.surface_interactive_selected_bg),
        (_, true) => (if light { 12.0 } else { 14.0 }, p.surface_interactive_hover_bg),
        _ => (if light { 8.0 } else { 10.0 }, p.surface_interactive_hover_bg),
    };
    let surface = mix(accent, base, fill);
    let size = dim::GUILD_FOLDER_EXPANDED_SURFACE_SIZE;
    let radius = if hovered || selected { size * 0.24 } else { dim::FOLDER_RADIUS };
    let glyph_color = mix(accent, p.text_primary, if selected { 28.0 } else { 14.0 });
    let glyph = || row().size(size, size).center().radius(radius).fill(surface).child(icon(folder_glyph(&folder.icon), 24.0).color(glyph_color));
    let mut unread = false;
    let mut mentions = 0;
    for &g in guilds {
        let (u, m) = super::unread_badges::guild(store, g);
        unread |= u;
        mentions += m;
    }
    let inside = matches!(st.nav.drag.and_then(|d| d.drop), Some(Drop::Inside(f)) if f == fid);
    let mut face = if open || folder.flags & 1 != 0 {
        glyph()
    } else {
        // `.collapsedFolder`: 4px padding, 2px gaps, minis at 38% of the folder radius.
        let mini_bg = if light { mix(accent, p.background_primary, 12.0) } else { mix(accent, p.background_secondary, 14.0) };
        let cell = (size - 8.0 - 2.0) / 2.0;
        let mini = |id: &Snowflake| mini_face(store, st, *id, cell, dim::FOLDER_RADIUS * 0.38, mini_bg, p);
        let cells: Vec<El> = guilds.iter().take(4).map(mini).collect();
        col()
            .size(size, size)
            .padding(4.0)
            .gap(2.0)
            .radius(radius)
            .fill(surface)
            .child(row().gap(2.0).children(cells.iter().take(2).cloned()))
            .child(row().gap(2.0).children(cells.iter().skip(2).cloned()))
    };
    if inside {
        face = face.outline(2.0, 2.0, if light { p.brand_primary } else { p.brand_primary_light });
    }
    let mut frame = el().size(size, size).child(face);
    if !open && let Some(b) = mention_badge(mentions as u64, true, p) {
        frame = frame.child(
            row()
                .pin(None, None, Some(-4.0), Some(-4.0))
                .radius(dim::RADIUS_MD + 3.0)
                .box_shadow(Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 3.0, color: surface, inset: false })
                .child(b),
        );
    }
    let mut header = row().w_full().h(ROW).center();
    if !open && (selected || hovered || unread) && st.nav.drag.is_none() {
        header = header.child(indicator(if selected { 40.0 } else if hovered { 20.0 } else { 8.0 }, p).key("pill"));
    }
    let name = crate::nav::folder::display_name(store, folder);
    let header = header
        .child(frame.key("icon"))
        .children(drop_marks(st, Slot::Folder(fid), p))
        .opacity(if st.nav.drag.is_some_and(|d| d.src == Slot::Folder(fid)) { 0.5 } else { 1.0 })
        .cursor(rune::Cursor::Pointer)
        .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
        .label(format!("{name} folder, {}", if open { "expanded" } else { "collapsed" }))
        .key(format!("folder-{}", fid.unwrap_or(0)))
        .on_hover_change(move |s: &mut S, inside, _| {
            let nav = &mut acc(s).nav;
            if inside {
                nav.hover_folder = Some(fid);
            } else if nav.hover_folder == Some(fid) {
                nav.hover_folder = None;
            }
        })
        .on_press(move |s: &mut S| {
            let open = &mut acc(s).nav.open;
            if !open.remove(&fid) {
                open.insert(fid);
            }
        })
        .on_secondary_press(move |s: &mut S, at, _| crate::nav::folder::open_menu(acc(s), fid, [at.x, at.y]));
    let header = draggable(header, Slot::Folder(fid), rows, acc);
    if !open {
        return header;
    }
    let n = guilds.len();
    let mut list = col().w_full().pb(rail::FOLDER_PAD).gap(rail::FOLDER_GAP);
    for &g in guilds {
        list = list.child(guild(store, st, g, rows, p, acc));
    }
    let h = ROW * (n as f32 + 1.0) + rail::FOLDER_GAP * n.saturating_sub(1) as f32;
    // `.expandedFolderBackground`: 4px wider than the surface, radius 32% of the box.
    let back_w = size + 4.0;
    let back = el()
        .absolute((dim::LAYOUT_GUILD_LIST_WIDTH - back_w) / 2.0, (ROW - size) / 2.0)
        .size(back_w, h - (ROW - size) / 2.0)
        .radius(dim::GUILD_LIST_ITEM_BOX_SIZE * 0.32)
        .fill(mix(accent, p.background_secondary, if light { 7.0 } else { 8.0 }))
        .inert();
    col().w_full().key(format!("folder-open-{}", fid.unwrap_or(0))).child(back).child(header).child(list)
}

/// `MiniGuildIcon`: the icon, else the acronym on the mini surface.
fn mini_face(store: &Store, st: &ShellState, id: Snowflake, size: f32, radius: f32, bg: rune::Color, p: &Palette) -> El {
    let info = store.guilds.get(&id).and_then(|g| g.info.as_ref());
    match info.and_then(|i| i.icon.as_deref()).map(|h| st.media.asset(|b, s| media::guild_icon_url(b, id, h, size, s))) {
        Some(Load::Ready(px)) => el().size(size, size).radius(radius).pixels_fit(&px, rune::SurfaceFit::Fill),
        _ => {
            let a = acronym(info.map_or("", |i| i.name.as_str()));
            let fs = acronym_font_size(&a, size);
            row().size(size, size).center().radius(radius).fill(bg).child(text(a).font(font(fs, 500.0).lh(fs)).color(p.text_primary))
        }
    }
}

/// `.createGuildButton`: a dashed ring with a plus.
fn add_button<S: 'static>(p: &Palette, acc: ShellAccess<S>) -> El {
    row().w_full().h(dim::GUILD_LIST_ITEM_BOX_SIZE).center().child(
        row()
            .size(ICON, ICON)
            .center()
            .radius(ICON / 2.0)
            .border(2.0, p.background_modifier_accent)
            .border_dash(4.0, 3.0)
            .hover(mix(p.text_primary, rune::Color::TRANSPARENT, 4.0))
            .cursor(rune::Cursor::Pointer)
            .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
            .label("Add a Community")
            .on_press(move |s: &mut S| acc(s).open_dialog(crate::guild::Dialog::Add(crate::guild::AddView::Landing)))
            .child(icon(&icons::PLUS_BOLD, 20.0).color(p.text_primary)),
    )
}

/// The Explore button under Add a Community.
fn explore_button<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let on = st.social.discovery;
    row().w_full().h(dim::GUILD_LIST_ITEM_BOX_SIZE).center().child(
        row()
            .size(ICON, ICON)
            .center()
            .radius(if on { 16.0 } else { ICON / 2.0 })
            .fill(if on { p.brand_primary } else { p.background_tertiary })
            .hover(p.brand_primary)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
            .label(t("Explore"))
            .key("rail-explore")
            .on_press(move |s: &mut S| {
                let st = acc(s);
                st.select_guild(None, None);
                crate::social::open_discovery(st);
            })
            .child(icon(&icons::COMPASS_FILL, 22.0).color(if on { rune::Color::WHITE } else { p.text_primary }).group_color(rune::Color::WHITE)),
    )
}

/// The help / download rail rows: a circle with the glyph, opening `url` in the browser.
fn rail_action<S: 'static>(glyph: &'static rune::Icon, label: &str, key: &'static str, p: &Palette, go: impl Fn(&mut S) + 'static) -> El
{
    row().w_full().h(dim::GUILD_LIST_ITEM_BOX_SIZE).center().child(
        row()
            .size(ICON, ICON)
            .center()
            .radius(ICON / 2.0)
            .fill(p.background_tertiary)
            .hover(p.brand_primary)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
            .label(label.to_string())
            .key(key)
            .on_press(move |s: &mut S| go(s))
            .child(icon(glyph, 22.0).color(p.text_primary).group_color(rune::Color::WHITE)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acronyms_follow_fluxer() {
        assert_eq!(acronym("Fluxer HQ"), "FH");
        assert_eq!(acronym("Ada's Lab"), "AL");
        assert_eq!(acronym("rust-lang"), "r-l");
        assert_eq!(acronym("  "), "?");
        assert_eq!(acronym_font_size("FH", 48.0), 18.0);
        assert_eq!(fit_scale(2, 1.0), 18.0 / 48.0, "fits");
        assert_eq!(fit_scale(2, 2.5), 14.0 / 48.0, "the first smaller size that fits");
        assert_eq!(fit_scale(6, 5.0), 40.0 / 48.0 / 5.0, "nothing fits: the exact scale");
    }
}
