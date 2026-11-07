//! Favorites (`messaging/state/Favorites.ts`, `sidebar_nav/FavoritesButton`,
//! `FavoritesGuildHeader`, `FavoritesChannelListContent`, `AddFavoriteChannelModal`).
// Synced through `synced_preferences` by [`super::favorites_sync`].
//! Channels sit in favorites categories (collapsible, renamable), may carry a nickname, and drag
//! to reorder ([`super::favorites_drag`]).

use reflux_model::{ChannelType, Snowflake};
use reflux_store::Store;
use rune::{El, ScrollSpec, TextEdit, Tx, col, el, icon, row, text};
use serde::{Deserialize, Serialize};

use super::favorites_drag as drag;
use super::{mine, update_mine};
use crate::icons;
use crate::shell::{ShellAccess, ShellState, Target};
use crate::theme::{Palette, dim, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Checkbox, Input, Menu, MenuEntry, MenuItem, Modal, ModalSize, Select, mention_badge, over, popup_access, scroller};

const HEADER_MENU: &str = "favorites-menu";
const ROW_MENU: &str = "favorite-row-menu";
const CATEGORY_MENU: &str = "favorite-category-menu";
const LIST_MENU: &str = "favorite-list-menu";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Favorite {
    pub guild: Option<Snowflake>,
    pub channel: Snowflake,
    /// The favorites category it's in (`parentId`), and its nickname here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub name: String,
}

/// The text modal open over the favorites (`CreateFavoriteCategoryModal`, `RenameChannelModal`
/// for a category or a channel's nickname).
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Create,
    Rename(String),
    Nickname(Snowflake),
}

/// `Favorites.moveChannel`: out of its place, into `parent` at `index` among that category's.
pub fn move_channel(list: &mut Vec<Favorite>, channel: Snowflake, parent: Option<String>, index: usize) {
    let Some(i) = list.iter().position(|f| f.channel == channel) else { return };
    let mut f = list.remove(i);
    f.parent = parent;
    let same: Vec<usize> = list.iter().enumerate().filter(|(_, x)| x.parent == f.parent).map(|(i, _)| i).collect();
    let at = match same.get(index) {
        Some(&j) => j,
        None => same.last().map_or(list.len(), |&j| j + 1),
    };
    list.insert(at, f);
}

/// `Favorites.moveCategory`: `index` counts the categories before the move.
pub fn move_category(cats: &mut Vec<Category>, id: &str, index: usize) {
    let Some(i) = cats.iter().position(|c| c.id == id) else { return };
    let c = cats.remove(i);
    let at = if i < index { index - 1 } else { index };
    cats.insert(at.min(cats.len()), c);
}

/// `Favorites.removeCategory`: its channels become uncategorized.
pub fn remove_category(a: &mut super::AccountPrefs, id: &str) {
    a.favorite_categories.retain(|c| c.id != id);
    a.collapsed_favorites.retain(|c| c != id);
    for f in a.favorites.iter_mut().filter(|f| f.parent.as_deref() == Some(id)) {
        f.parent = None;
    }
}

pub fn is_favorite(st: &ShellState, channel: Snowflake) -> bool {
    st.prefs.accounts.get(&st.account.me.to_string()).is_some_and(|a| a.favorites.iter().any(|f| f.channel == channel))
}

/// `Favorites.addChannel` (no duplicates).
pub fn add(st: &mut ShellState, guild: Snowflake, channel: Snowflake) {
    add_to(st, guild, channel, None);
}

pub fn add_to(st: &mut ShellState, guild: Snowflake, channel: Snowflake, parent: Option<String>) {
    update_mine(st, |a| {
        if !a.favorites.iter().any(|f| f.channel == channel) {
            a.favorites.push(Favorite { guild: Some(guild), channel, parent, nickname: None });
        }
    });
}

pub fn remove(st: &mut ShellState, channel: Snowflake) {
    update_mine(st, |a| a.favorites.retain(|f| f.channel != channel));
}

/// The channel menu's "Add to favorites" / "Remove from favorites" with its toast.
pub fn toggle(st: &mut ShellState, guild: Snowflake, channel: Snowflake) {
    let on = is_favorite(st, channel);
    if on {
        remove(st, channel);
    } else {
        add(st, guild, channel);
    }
    crate::desktop::toast(&mut st.desktop, if on { "Removed from favorites" } else { "Added to favorites" }, true);
}

/// The Favorites rail button: shows the favorites list and its first channel.
pub fn open(st: &mut ShellState, _store: &Store) {
    select(st);
}

fn select(st: &mut ShellState) {
    st.popups.close();
    st.nav.favorites = true;
    match mine(st).favorites.first() {
        Some(f) => st.select_channel(f.channel),
        None => st.channel = None,
    }
}

/// Keeps `ShellState::guild` on the open favorite's community (chat and members read it).
pub fn reconcile(store: &Store, st: &mut ShellState) -> bool {
    if !st.nav.favorites {
        return false;
    }
    let g = st.channel.and_then(|c| store.channels.get(&c)).and_then(|c| c.guild_id);
    if g.is_some() && g != st.guild {
        st.guild = g;
        st.intents.push(crate::shell::Intent::SelectGuild(g));
        return true;
    }
    false
}

/// `FavoritesButton`: a 28px filled star in a rail slot (circle, squircle when lit).
pub fn rail_button<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let selected = st.nav.favorites;
    let hovered = st.nav.hover_favorites;
    let lit = selected || hovered;
    let size = dim::GUILD_ICON_SIZE;
    let (bg, fg) = if lit { (p.brand_primary, p.brand_primary_fill) } else { (p.guilds_layout_item_bg, p.text_primary) };
    let face = row().size(size, size).center().radius(if lit { size * 0.3 } else { size / 2.0 }).fill(bg).child(icon(&icons::STAR_FILL, 28.0).color(fg));
    let mut item = row().w_full().h(dim::GUILD_LIST_ITEM_TARGET_SIZE).center();
    if lit {
        let w = dim::GUILD_LIST_INDICATOR_BAR_WIDTH;
        item = item.child(
            row()
                .absolute(-0.15 * 16.0, (dim::GUILD_LIST_ITEM_TARGET_SIZE - size) / 2.0)
                .size(dim::GUILD_LIST_INDICATOR_TRACK_WIDTH, size)
                .center()
                .child(el().size(w, if selected { 40.0 } else { 20.0 }).corners([0.0, w / 2.0, w / 2.0, 0.0]).fill(p.text_primary))
                .inert(),
        );
    }
    item.child(face)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label("Favorites")
        .key("rail-favorites")
        .on_hover_change(move |s: &mut S, inside, _| acc(s).nav.hover_favorites = inside)
        .on_press(move |s: &mut S| select(acc(s)))
}

/// `FavoritesGuildHeader` + `FavoritesChannelListContent`.
pub fn sidebar<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let open = st.popups.is_open(HEADER_MENU);
    let header = row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::SPACING_4)
        .gap(dim::SPACING_2)
        .center_y()
        .fill(p.background_secondary)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label("Open favorites menu")
        .key(HEADER_MENU)
        .on_press(move |s: &mut S| acc(s).popups.toggle(HEADER_MENU))
        .child(icon(&icons::STAR_FILL, 16.0).color(p.text_primary))
        .child(text("Favorites").font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).truncate().grow(1.0).shrink())
        .child(icon(if open { &icons::X } else { &icons::CARET_DOWN_BOLD }, 16.0).color(p.text_primary))
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(over(p.user_area_divider_color, p.background_secondary)));
    let a = mine(st);
    let hide = a.hide_muted_favorites;
    let mut list = col().w_full().pt(10.0).pr(dim::SPACING_2);
    let mut any = false;
    // Uncategorized first, then each category (`channelGroups`), as rows for the drag.
    let mut groups: Vec<(Option<&Category>, Vec<&Favorite>)> = vec![(None, Vec::new())];
    groups.extend(a.favorite_categories.iter().map(|c| (Some(c), Vec::new())));
    for f in &a.favorites {
        let Some(c) = store.channels.get(&f.channel) else { continue };
        if hide && store.is_muted(c) && st.channel != Some(c.id) {
            continue;
        }
        if let Some(g) = groups.iter_mut().find(|g| g.0.map(|c| &c.id) == f.parent.as_ref()) {
            g.1.push(f);
        }
    }
    let mut rows: Vec<drag::Row> = Vec::new();
    let mut y = 0.0;
    for (cat, favs) in &groups {
        let collapsed = cat.is_some_and(|c| a.collapsed_favorites.contains(&c.id));
        if let Some(c) = cat {
            rows.push(drag::Row { top: y, h: drag::CATEGORY_H, slot: drag::Slot::Category(c.id.clone()) });
            y += drag::CATEGORY_H;
        }
        if !collapsed {
            for f in favs {
                rows.push(drag::Row { top: y, h: drag::CHANNEL_H, slot: drag::Slot::Channel(f.channel) });
                y += drag::CHANNEL_H;
            }
        }
    }
    let rows = std::rc::Rc::new(rows);
    for (cat, favs) in &groups {
        let collapsed = cat.is_some_and(|c| a.collapsed_favorites.contains(&c.id));
        if let Some(c) = cat {
            list = list.child(category_row(st, c, collapsed, &rows, p, acc));
        }
        if collapsed {
            continue;
        }
        for f in favs {
            let c = &store.channels[&f.channel];
            any = true;
            list = list.child(fav_row(store, st, c, f.nickname.as_deref(), st.channel == Some(c.id), &rows, p, acc));
        }
    }
    let any = any || !a.favorite_categories.is_empty();
    if !any {
        // An empty list is `GuildChannelListSkeleton`.
        for i in 0..10 {
            list = list.child(
                row()
                    .w_full()
                    .h(34.0)
                    .pl(16.0)
                    .gap(8.0)
                    .center_y()
                    .child(el().size(20.0, 20.0).radius(10.0).fill(p.background_modifier_accent).opacity(0.45))
                    .child(el().size(60.0 + ((i * 37) % 70) as f32, 10.0).radius(5.0).fill(p.background_modifier_accent).opacity(0.45))
                    .inert(),
            );
        }
    }
    list = list.child(el().h(10.0 + dim::LAYOUT_USER_AREA_HEIGHT + dim::FOOTER_BOX_INSET + 1.0 + dim::SPACING_2));
    // `FavoritesChannelListContextMenu` on the list's empty space.
    let list = list.on_secondary_press(move |s: &mut S, at, _| acc(s).popups.open_at(LIST_MENU, [at.x, at.y]));
    let viewport = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(list);
    col().w(dim::LAYOUT_SIDEBAR_WIDTH).h_full().min_h(0.0).fill(p.background_secondary).child(header).child(scroller(viewport, false, p))
}

/// `FavoriteChannelItem`: the community icon (24px) with the channel glyph in a corner badge,
/// then the name.
#[allow(clippy::too_many_arguments)]
fn fav_row<S: 'static>(store: &Store, st: &ShellState, c: &reflux_model::Channel, nickname: Option<&str>, selected: bool, rows: &std::rc::Rc<Vec<drag::Row>>, p: &Palette, acc: ShellAccess<S>) -> El {
    let shown = nickname.filter(|n| !n.is_empty()).map(str::to_string).or_else(|| c.name.clone()).unwrap_or_default();
    let id = c.id;
    let guild = c.guild_id;
    let muted = store.is_muted(c);
    let unread = store.has_unread(id);
    let fg = if selected { p.surface_interactive_selected_color } else if unread && !muted { p.text_secondary } else { p.text_primary_muted };
    let face = guild.map_or_else(|| el().size(24.0, 24.0), |g| crate::shell::guild_face(store, st, g, 24.0, false, p));
    let badge = row()
        .pin(None, None, Some(-2.0), Some(-2.0))
        .size(14.0, 14.0)
        .center()
        .radius(7.0)
        .fill(p.background_primary)
        .child(icon(crate::shell::channel_icon(c.kind), 12.0).color(if selected { p.surface_interactive_selected_color } else { p.text_primary_muted }));
    let mut item = row()
        .grow(1.0)
        .min_w(0.0)
        .mx(8.0)
        .px(8.0)
        .py(6.0)
        .gap(8.0)
        .center_y()
        .radius(6.0)
        .cursor(rune::Cursor::Pointer)
        .role("link")
        .label(shown.clone())
        .hover_group()
        .child(el().size(24.0, 24.0).child(face).child(badge))
        .child(text(shown).font(font(16.0, 500.0).lh(20.0)).color(fg).group_color(if selected { fg } else { p.text_chat }).truncate().grow(1.0).shrink())
        .children(mention_badge(store.mention_count(id) as u64, true, p));
    item = if selected { item.fill(p.background_modifier_selected) } else { item.hover(p.background_modifier_hover) };
    if muted && !selected {
        item = item.opacity(0.5);
    }
    let voice = c.kind == ChannelType::GUILD_VOICE;
    item = item
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            if voice {
                sh.join_voice(guild, id);
            } else {
                sh.select_channel(id);
            }
        })
        .on_secondary_press(move |s: &mut S, at, _| {
            let sh = acc(s);
            sh.popups.open_at(ROW_MENU, [at.x, at.y]);
            sh.target = Some(Target::Channel(id));
        });
    let mut container = row().w_full().pt(1.0).child(item);
    if unread && !selected {
        container = container.child(el().absolute(-4.0, 1.0 + 16.0 - 4.0).size(8.0, 8.0).corners([0.0, 4.0, 4.0, 0.0]).fill(p.text_primary).opacity(if muted { 0.4 } else { 1.0 }));
    }
    let slot = drag::Slot::Channel(id);
    let container = container.children(drop_marks(st, &slot, p));
    draggable(container, slot, rows, acc).key(format!("fav-{id}"))
}

/// `FavoriteCategoryItem`: the name and caret (a press collapses it), "+" (Add channel) on hover.
fn category_row<S: 'static>(st: &ShellState, c: &Category, collapsed: bool, rows: &std::rc::Rc<Vec<drag::Row>>, p: &Palette, acc: ShellAccess<S>) -> El {
    let (id, id2, id3) = (c.id.clone(), c.id.clone(), c.id.clone());
    // `.hoverAffordance`: shown while the header is hovered.
    let add = icon(&icons::PLUS_BOLD, 16.0)
        .color(p.text_tertiary_muted)
        .group_color(p.text_primary)
        .group_opacity(0.0, 1.0)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label("Add channel")
        .on_press(move |s: &mut S| open_add(acc(s), Some(id2.clone())));
    let head = row()
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
        .label(c.name.clone())
        .on_press(move |s: &mut S| {
            let id = id.clone();
            update_mine(acc(s), |a| toggle_collapsed(a, &id));
        })
        .on_secondary_press(move |s: &mut S, at, _| {
            let sh = acc(s);
            sh.popups.open_at(CATEGORY_MENU, [at.x, at.y]);
            sh.nav.add_favorite_category = Some(id3.clone());
        })
        .child(text(c.name.clone()).font(font(14.0, 600.0).lh(20.0)).color(p.text_tertiary_muted).group_color(p.text_primary).truncate().shrink())
        .child(icon(if collapsed { &icons::CARET_RIGHT_BOLD } else { &icons::CARET_DOWN_BOLD }, 12.0).color(p.text_tertiary_muted).group_color(p.text_primary))
        .child(el().grow(1.0))
        .child(add);
    let slot = drag::Slot::Category(c.id.clone());
    let e = row().w_full().pt(4.0).child(head).children(drop_marks(st, &slot, p));
    draggable(e, slot, rows, acc).key(format!("fav-cat-{}", c.id))
}

pub fn toggle_collapsed(a: &mut super::AccountPrefs, id: &str) {
    if let Some(i) = a.collapsed_favorites.iter().position(|c| c == id) {
        a.collapsed_favorites.remove(i);
    } else {
        a.collapsed_favorites.push(id.to_string());
    }
}

fn open_add(st: &mut ShellState, category: Option<String>) {
    st.popups.close();
    st.nav.add_favorite = Some(st.guild);
    st.nav.add_favorite_category = category;
    st.nav.favorite_query = TextEdit::new("");
}

fn open_edit(st: &mut ShellState, e: Edit, initial: &str) {
    st.popups.close();
    st.nav.favorite_edit = Some(e);
    st.nav.favorite_text = TextEdit::new(initial);
    st.nav.favorite_text.focused = true;
}

/// A row that drags (past 4pt) to a drop resolved over the list's rows.
fn draggable<S: 'static>(e: El, src: drag::Slot, rows: &std::rc::Rc<Vec<drag::Row>>, acc: ShellAccess<S>) -> El {
    let Some(top) = rows.iter().find(|r| r.slot == src).map(|r| r.top) else { return e };
    let rows = rows.clone();
    e.on_drag(move |s: &mut S, d: &mut rune::Drag, _| {
        let sh = acc(s);
        match d.phase {
            rune::Phase::Down => sh.nav.favorite_drag = None,
            rune::Phase::Move => {
                if sh.nav.favorite_drag.is_none() && d.dx.abs().max(d.dy.abs()) < 4.0 {
                    return;
                }
                let drop = drag::resolve(&rows, &src, top + d.y);
                sh.nav.favorite_drag = Some(drag::Drag { src: src.clone(), drop });
            }
            rune::Phase::Up => {
                if let Some(drag::Drag { src, drop: Some(drop) }) = sh.nav.favorite_drag.take().filter(|_| !d.cancelled()) {
                    update_mine(sh, |a| drag::apply(a, &src, &drop));
                }
            }
        }
    })
}

/// The 2px drop line above or below `slot`, or a ring round a category taking a channel.
fn drop_marks(st: &ShellState, slot: &drag::Slot, p: &Palette) -> Option<El> {
    let c = if crate::ui::is_light(p) { p.brand_primary } else { p.brand_primary_light };
    let line = |top: bool| el().pin(if top { Some(0.0) } else { None }, Some(8.0), Some(8.0), if top { None } else { Some(0.0) }).h(2.0).radius(1.0).fill(c).inert();
    match st.nav.favorite_drag.as_ref()?.drop.as_ref()? {
        drag::Drop::Before(s) if s == slot => Some(line(true)),
        drag::Drop::After(s) if s == slot => Some(line(false)),
        drag::Drop::Into(Some(id)) if *slot == drag::Slot::Category(id.clone()) => Some(el().pin(Some(4.0), Some(8.0), Some(0.0), Some(0.0)).radius(6.0).border(2.0, c).inert()),
        _ => None,
    }
}

pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let add_item = |cat: Option<String>| MenuItem::new("add", "Add channel").icon(&icons::PLUS_CIRCLE_FILL).on_select(move |s: &mut S| open_add(acc(s), cat.clone()));
    let create_item = || MenuItem::new("create-category", "Create category").icon(&icons::FOLDER_PLUS_FILL).on_select(move |s: &mut S| open_edit(acc(s), Edit::Create, ""));
    let a = mine(st);
    if st.popups.is_open(HEADER_MENU) {
        let hide = a.hide_muted_favorites;
        Menu::new(HEADER_MENU)
            .item(add_item(None))
            .item(create_item())
            .separator()
            .item(MenuItem::new("hide-muted", "Hide muted channels").checkbox(hide).on_select(move |s: &mut S| update_mine(acc(s), |a| a.hide_muted_favorites = !hide)))
            .el(&st.popups, pops, p, layer);
    }
    if st.popups.is_open(LIST_MENU) {
        Menu::new(LIST_MENU).item(add_item(None)).item(create_item()).el(&st.popups, pops, p, layer);
    }
    // `FavoritesCategoryContextMenu`.
    if st.popups.is_open(CATEGORY_MENU)
        && let Some(cat) = st.nav.add_favorite_category.as_ref().and_then(|id| a.favorite_categories.iter().find(|c| c.id == *id))
    {
        let (id, id2, id3, name) = (cat.id.clone(), cat.id.clone(), cat.id.clone(), cat.name.clone());
        let collapsed = a.collapsed_favorites.contains(&cat.id);
        Menu::new(CATEGORY_MENU)
            .item(add_item(Some(cat.id.clone())))
            .item(MenuItem::new("rename", "Rename category").icon(&icons::PENCIL_SIMPLE_FILL).on_select(move |s: &mut S| open_edit(acc(s), Edit::Rename(id.clone()), &name)))
            .separator()
            .item(MenuItem::new("collapse", if collapsed { "Expand category" } else { "Collapse category" }).icon(if collapsed { &icons::CARET_DOWN_BOLD } else { &icons::CARET_RIGHT_BOLD }).on_select(move |s: &mut S| {
                let id = id2.clone();
                update_mine(acc(s), |a| toggle_collapsed(a, &id));
            }))
            .separator()
            .item(MenuItem::new("delete", "Delete category").icon(&icons::TRASH_FILL).danger().on_select(move |s: &mut S| {
                let id = id3.clone();
                update_mine(acc(s), |a| remove_category(a, &id));
            }))
            .el(&st.popups, pops, p, layer);
    }
    if st.popups.is_open(ROW_MENU)
        && let Some(Target::Channel(c)) = st.target
        && let Some(fav) = a.favorites.iter().find(|f| f.channel == c)
    {
        let guild = store.channels.get(&c).and_then(|ch| ch.guild_id);
        let link = format!("{}/channels/{}/{c}", st.web_base.trim_end_matches('/'), guild.map_or("@me".to_string(), |g| g.to_string()));
        let nick = fav.nickname.clone().unwrap_or_default();
        let mut m = Menu::new(ROW_MENU).item(MenuItem::new("nickname", "Change nickname").icon(&icons::PENCIL_SIMPLE_FILL).on_select(move |s: &mut S| open_edit(acc(s), Edit::Nickname(c), &nick)));
        if let Some(g) = guild {
            let first = Some(c);
            m = m.item(MenuItem::new("open", "Open in community").icon(&icons::ARROW_SQUARE_OUT).on_select(move |s: &mut S| {
                let sh = acc(s);
                sh.nav.favorites = false;
                sh.select_guild(Some(g), first);
            }));
        }
        m = m.item(MenuItem::new("copy-link", "Copy link").icon(&icons::LINK_BOLD).on_select_tx(move |s: &mut S, tx: &mut Tx| {
            tx.copy(link.clone());
            crate::desktop::toast(&mut acc(s).desktop, "Link copied to clipboard", true);
        }));
        // "Move to": Uncategorized (when in a category) and the other categories.
        let move_to = |label: String, target: Option<String>| {
            MenuEntry::Item(MenuItem::new(format!("move-{}", target.as_deref().unwrap_or("none")), label).on_select(move |s: &mut S| {
                let t = target.clone();
                update_mine(acc(s), |a| {
                    let n = a.favorites.iter().filter(|f| f.parent == t && f.channel != c).count();
                    move_channel(&mut a.favorites, c, t, n);
                });
            }))
        };
        let mut targets = Vec::new();
        if fav.parent.is_some() {
            targets.push(move_to("Uncategorized".into(), None));
        }
        targets.extend(a.favorite_categories.iter().filter(|x| Some(&x.id) != fav.parent.as_ref()).map(|x| move_to(x.name.clone(), Some(x.id.clone()))));
        if !targets.is_empty() {
            m = m.submenu("move-to", "Move to", targets);
        }
        if guild.is_some() {
            m = m.separator().entry(crate::social::mute_entry(store, st, guild, Some(c), "channel", acc));
        }
        m.separator()
            .item(MenuItem::new(format!("copy-{c}"), "Copy channel ID").icon(&icons::SNOWFLAKE).on_select_tx(move |_: &mut S, tx: &mut Tx| tx.copy(c.to_string())))
            .separator()
            .item(MenuItem::new("remove", "Remove from favorites").icon(&icons::STAR_FILL).danger().on_select(move |s: &mut S| {
                let sh = acc(s);
                remove(sh, c);
                crate::desktop::toast(&mut sh.desktop, "Channel removed from favorites", true);
            }))
            .el(&st.popups, pops, p, layer);
    }
    if let Some(g) = st.nav.add_favorite {
        let start = layer.len();
        let modal = add_modal(store, st, g, p, acc, layer);
        layer.insert(start, modal);
    }
    if let Some(e) = st.nav.favorite_edit.clone() {
        layer.push(edit_modal(st, e, p, acc));
    }
}

/// `CreateFavoriteCategoryModal` / `RenameChannelModal` (a category's name or a channel's
/// nickname, blank clearing it).
fn edit_modal<S: 'static>(st: &ShellState, e: Edit, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).nav.favorite_edit = None;
    let (title, label, placeholder, action) = match &e {
        Edit::Create => ("Create category", "Category name", "New category", "Create"),
        Edit::Rename(_) => ("Rename category", "Category name", "", "Save"),
        Edit::Nickname(_) => ("Change nickname", "Nickname", "Channel nickname", "Save"),
    };
    let blank = st.nav.favorite_text.text.trim().is_empty();
    let save = move |s: &mut S| {
        let sh = acc(s);
        let v: String = sh.nav.favorite_text.text.trim().chars().take(100).collect();
        let Some(e) = sh.nav.favorite_edit.take() else { return };
        update_mine(sh, |a| match e {
            Edit::Create if !v.is_empty() => a.favorite_categories.push(Category { id: format!("favorite-category-{}", crate::app::now_ms()), name: v }),
            Edit::Rename(id) if !v.is_empty() => {
                if let Some(c) = a.favorite_categories.iter_mut().find(|c| c.id == id) {
                    c.name = v;
                }
            }
            Edit::Nickname(ch) => {
                if let Some(f) = a.favorites.iter_mut().find(|f| f.channel == ch) {
                    f.nickname = Some(v).filter(|v| !v.is_empty());
                }
            }
            _ => {}
        });
    };
    let field = Input::new("favorite-edit", &st.nav.favorite_text, move |s: &mut S| &mut acc(s).nav.favorite_text).label(label).placeholder(placeholder).el(p);
    let footer = row()
        .gap(8.0)
        .child(Button::new("Cancel").variant(ButtonVariant::Secondary).on_press(close).el(p))
        .child(Button::new(action).key("favorite-edit-save").disabled(blank && !matches!(e, Edit::Nickname(_))).on_press(save).el(p));
    Modal::new(title, close).body(field).footer(footer).el(p)
}

/// `AddFavoriteChannelModal`: pick a community, then add or remove its text, announcement,
/// voice and link channels, grouped by category.
fn add_modal<S: 'static>(store: &Store, st: &ShellState, g: Option<Snowflake>, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let close = move |s: &mut S| acc(s).nav.add_favorite = None;
    let names: Vec<(String, String)> = store
        .guild_list()
        .into_iter()
        .flat_map(|i| match i {
            reflux_store::GuildListItem::Guild(g) => vec![g],
            reflux_store::GuildListItem::Folder { guilds, .. } => guilds,
        })
        .map(|id| (id.to_string(), store.guilds.get(&id).and_then(|x| x.info.as_ref()).map_or_else(|| "Unknown community".into(), |i| i.name.clone())))
        .collect();
    let opts: Vec<(&str, &str)> = names.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let value = g.map(|g| g.to_string());
    let search = Input::new("favorite-search", &st.nav.favorite_query, move |s: &mut S| &mut acc(s).nav.favorite_query).placeholder("Search channels").el(p);
    let picker = Select::new("favorite-guild", &opts, value.as_deref())
        .label("Select a community")
        .placeholder("Choose a community")
        .width(392.0)
        .on_choose(move |s: &mut S, v: &str| acc(s).nav.add_favorite = Some(v.parse().ok().map(Snowflake)))
        .el(&st.popups, popup_access(move |s: &mut S| &mut acc(s).popups), p, layer);
    let mut body = col().w_full().gap(16.0).child(search).child(picker);
    if let Some(gid) = g {
        let hide = st.nav.favorite_hide_muted;
        body = body.child(Checkbox::new(hide).label("Hide muted channels").on_toggle(move |s: &mut S| acc(s).nav.favorite_hide_muted = !hide).el(p));
        let q = st.nav.favorite_query.text.trim().to_lowercase();
        let mut rows = col().w_full();
        let mut last_cat: Option<Option<String>> = None;
        let mut n = 0;
        for grp in store.channel_groups(gid) {
            let cat = grp.category.map(|c| c.name.clone().unwrap_or_default());
            for c in grp.text.iter().chain(grp.voice.iter()) {
                if !matches!(c.kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT | ChannelType::GUILD_VOICE | ChannelType::GUILD_LINK) {
                    continue;
                }
                if hide && store.is_muted(c) {
                    continue;
                }
                let name = c.name.clone().unwrap_or_default();
                if !q.is_empty() && !name.to_lowercase().contains(&q) {
                    continue;
                }
                if last_cat.as_ref() != Some(&cat) {
                    rows = rows.child(
                        text(cat.clone().unwrap_or_else(|| "Uncategorized".into()).to_uppercase())
                            .font(font(12.0, 600.0).lh(16.0))
                            .color(p.text_primary_muted)
                            .pt(if n == 0 { 0.0 } else { 12.0 })
                            .pb(4.0),
                    );
                    last_cat = Some(cat.clone());
                }
                n += 1;
                let on = is_favorite(st, c.id);
                let id = c.id;
                rows = rows.child(
                    row()
                        .w_full()
                        .py(6.0)
                        .px(8.0)
                        .gap(8.0)
                        .center_y()
                        .radius(6.0)
                        .hover(p.background_modifier_hover)
                        .child(icon(crate::shell::channel_icon(c.kind), 20.0).color(p.text_primary_muted))
                        .child(text(name).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).truncate().grow(1.0).shrink())
                        .child(
                            Button::new(if on { "Remove" } else { "Add" })
                                .variant(if on { ButtonVariant::Secondary } else { ButtonVariant::Primary })
                                .size(ButtonSize::Small)
                                .on_press(move |s: &mut S| {
                                    let sh = acc(s);
                                    if on {
                                        remove(sh, id)
                                    } else {
                                        let cat = sh.nav.add_favorite_category.clone();
                                        add_to(sh, gid, id, cat)
                                    }
                                })
                                .el(p),
                        ),
                );
            }
        }
        if n == 0 {
            rows = rows.child(text("No channels available").font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).py(16.0));
        }
        let viewport = col().w_full().h(320.0).scroll(ScrollSpec { stick_bottom: false }).child(rows);
        body = body.child(scroller(viewport, false, p));
    }
    Modal::new("Add favorite channels", close).size(ModalSize::Medium).body(body).el(p)
}
