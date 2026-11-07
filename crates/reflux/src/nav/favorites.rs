//! Favorites (`messaging/state/Favorites.ts`, `sidebar_nav/FavoritesButton`,
//! `FavoritesGuildHeader`, `FavoritesChannelListContent`, `AddFavoriteChannelModal`).
// Synced through `synced_preferences` by [`super::favorites_sync`].
// shortcut: categories and nicknames are kept in the snapshot but not shown; no drag reordering.

use reflux_model::{ChannelType, Snowflake};
use reflux_store::Store;
use rune::{El, ScrollSpec, TextEdit, Tx, col, el, icon, row, text};
use serde::{Deserialize, Serialize};

use super::{mine, update_mine};
use crate::icons;
use crate::shell::{ShellAccess, ShellState, Target};
use crate::theme::{Palette, dim, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Checkbox, Input, Menu, MenuItem, Modal, ModalSize, Select, mention_badge, over, popup_access, scroller};

const HEADER_MENU: &str = "favorites-menu";
const ROW_MENU: &str = "favorite-row-menu";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Favorite {
    pub guild: Option<Snowflake>,
    pub channel: Snowflake,
}

pub fn is_favorite(st: &ShellState, channel: Snowflake) -> bool {
    st.prefs.accounts.get(&st.account.me.to_string()).is_some_and(|a| a.favorites.iter().any(|f| f.channel == channel))
}

/// `Favorites.addChannel` (no duplicates).
pub fn add(st: &mut ShellState, guild: Snowflake, channel: Snowflake) {
    update_mine(st, |a| {
        if !a.favorites.iter().any(|f| f.channel == channel) {
            a.favorites.push(Favorite { guild: Some(guild), channel });
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
    for f in &a.favorites {
        let Some(c) = store.channels.get(&f.channel) else { continue };
        let selected = st.channel == Some(c.id);
        if hide && store.is_muted(c) && !selected {
            continue;
        }
        any = true;
        list = list.child(fav_row(store, st, c, selected, p, acc));
    }
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
    let viewport = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(list);
    col().w(dim::LAYOUT_SIDEBAR_WIDTH).h_full().min_h(0.0).fill(p.background_secondary).child(header).child(scroller(viewport, false, p))
}

/// `FavoriteChannelItem`: the community icon (24px) with the channel glyph in a corner badge,
/// then the name.
fn fav_row<S: 'static>(store: &Store, st: &ShellState, c: &reflux_model::Channel, selected: bool, p: &Palette, acc: ShellAccess<S>) -> El {
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
        .label(c.name.clone().unwrap_or_default())
        .hover_group()
        .child(el().size(24.0, 24.0).child(face).child(badge))
        .child(text(c.name.clone().unwrap_or_default()).font(font(16.0, 500.0).lh(20.0)).color(fg).group_color(if selected { fg } else { p.text_chat }).truncate().grow(1.0).shrink())
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
    container.key(format!("fav-{id}"))
}

pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    if st.popups.is_open(HEADER_MENU) {
        let hide = mine(st).hide_muted_favorites;
        Menu::new(HEADER_MENU)
            .item(MenuItem::new("add", "Add channel").icon(&icons::PLUS_CIRCLE_FILL).on_select(move |s: &mut S| {
                let sh = acc(s);
                sh.nav.add_favorite = Some(sh.guild);
                sh.nav.favorite_query = TextEdit::new("");
            }))
            .separator()
            .item(MenuItem::new("hide-muted", "Hide muted channels").checkbox(hide).on_select(move |s: &mut S| update_mine(acc(s), |a| a.hide_muted_favorites = !hide)))
            .el(&st.popups, pops, p, layer);
    }
    if st.popups.is_open(ROW_MENU)
        && let Some(Target::Channel(c)) = st.target
    {
        let guild = store.channels.get(&c).and_then(|ch| ch.guild_id);
        let link = format!("{}/channels/{}/{c}", st.web_base.trim_end_matches('/'), guild.map_or("@me".to_string(), |g| g.to_string()));
        let mut m = Menu::new(ROW_MENU);
        if let Some(g) = guild {
            let first = Some(c);
            m = m.item(MenuItem::new("open", "Open in community").icon(&icons::ARROW_SQUARE_OUT).on_select(move |s: &mut S| {
                let sh = acc(s);
                sh.nav.favorites = false;
                sh.select_guild(Some(g), first);
            }));
        }
        m.item(MenuItem::new("copy-link", "Copy link").icon(&icons::LINK_BOLD).on_select_tx(move |s: &mut S, tx: &mut Tx| {
            tx.copy(link.clone());
            crate::desktop::toast(&mut acc(s).desktop, "Link copied to clipboard", true);
        }))
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
                                    if on { remove(sh, id) } else { add(sh, gid, id) }
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
