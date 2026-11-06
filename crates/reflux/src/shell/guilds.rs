//! The guild rail (`GuildsLayout.module.css`, `sidebar_nav/*`, `guild/components/GuildAcronym`).

use reflux_model::Snowflake;
use reflux_store::{GuildListItem, Store};
use rune::{Align, El, ScrollSpec, Shadow, col, el, icon, row, text};

use super::{ShellAccess, ShellState, USER_AREA_OVERLAY, first_channel, role_color};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{mention_badge, mix, scroller};

const ICON: f32 = dim::GUILD_ICON_SIZE;
/// `.guildListReorderTarget`: box size plus the item gap.
const ROW: f32 = dim::GUILD_LIST_ITEM_TARGET_SIZE;

pub fn rail<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let mut list = col()
        .w_full()
        .pt(dim::SPACING_1)
        .pb(USER_AREA_OVERLAY + dim::SPACING_2)
        .child(home(store, st, p, acc))
        .child(divider(p));
    for item in store.guild_list() {
        list = match item {
            GuildListItem::Guild(id) => list.child(guild(store, st, id, p, acc)),
            GuildListItem::Folder { folder, guilds } => list.child(folder_item(store, folder.color, &guilds, p)),
        };
    }
    list = list.child(add_button(p));
    let viewport = col().w(dim::LAYOUT_GUILD_LIST_WIDTH).h_full().fill(p.background_secondary).scroll(ScrollSpec { stick_bottom: false });
    scroller(viewport.child(list), false, p).key("guild-rail")
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
    p: &Palette,
    acc: ShellAccess<S>,
) -> El {
    let hovered = st.hover_guild == Some(target);
    let mut item = row().w_full().h(ROW).center();
    if selected || hovered || unread {
        item = item.child(indicator(if selected { 40.0 } else if hovered { 20.0 } else { 8.0 }, p));
    }
    let mut frame = el().size(ICON, ICON).child(face);
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
    item.child(frame)
        .cursor(rune::Cursor::Pointer)
        .role("button")
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
    let selected = st.guild.is_none();
    let lit = selected || st.hover_guild == Some(None);
    let (bg, fg) = if lit { (p.brand_primary, p.brand_primary_fill) } else { (p.guilds_layout_item_bg, p.text_primary) };
    let face = row()
        .size(ICON, ICON)
        .center()
        .radius(if lit { ICON * 0.3 } else { ICON / 2.0 })
        .fill(bg)
        .child(icon(&icons::CHAT_CIRCLE, 28.0).color(fg));
    let mentions: u32 = store.private_channels().iter().map(|c| store.mention_count(c.id)).sum();
    let first = store.private_channels().first().map(|c| c.id);
    slot(None, selected, false, st, face, mention_badge(mentions as u64, true, p), p, acc)
        .label("Direct Messages")
        .on_press(move |s: &mut S| acc(s).select_guild(None, first))
}

/// `.guildDividerSlot`: 6 + 2 + 6, a 32-wide line.
fn divider(p: &Palette) -> El {
    row().w_full().py(dim::GUILD_LIST_DIVIDER_BASE_GAP).justify_center().child(
        el().size(32.0, dim::GUILD_LIST_DIVIDER_LINE_SIZE).radius(1.0).fill(p.background_modifier_hover),
    )
}

fn guild<S: 'static>(store: &Store, st: &ShellState, id: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let selected = st.guild == Some(id);
    let lit = selected || st.hover_guild == Some(Some(id));
    let name = store.guilds.get(&id).and_then(|g| g.info.as_ref()).map_or("", |i| i.name.as_str());
    let (unread, mentions) = store.guild_unread(id);
    // shortcut: guild icons aren't fetched yet, so every guild shows its acronym.
    let face = acronym_face(name, ICON, lit, p);
    let first = first_channel(store, id);
    slot(Some(id), selected, unread, st, face, mention_badge(mentions as u64, true, p), p, acc)
        .label(name.to_string())
        .key(format!("guild-{id}"))
        .on_press(move |s: &mut S| acc(s).select_guild(Some(id), first))
}

fn acronym_face(name: &str, size: f32, lit: bool, p: &Palette) -> El {
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

/// `getGuildAcronymFontScale`: 18 18 16 14 12 10 px per 48px icon by length.
// shortcut: skips the canvas width fit that shrinks wide acronyms further.
fn acronym_font_size(a: &str, size: f32) -> f32 {
    const SIZES: [f32; 6] = [18.0, 18.0, 16.0, 14.0, 12.0, 10.0];
    let n = a.chars().count().clamp(1, 6);
    (SIZES[n - 1] / 48.0 * size).max(8.0)
}

/// A closed folder: up to four mini acronyms on the folder colour.
// shortcut: folders always show closed and don't open; add expand state when folders matter.
fn folder_item(store: &Store, color: Option<u32>, guilds: &[Snowflake], p: &Palette) -> El {
    let bg = color.map_or(mix(p.brand_primary, p.background_secondary, 40.0), |c| mix(role_color(c), p.background_secondary, 40.0));
    let mini = |id: &Snowflake| {
        let name = store.guilds.get(id).and_then(|g| g.info.as_ref()).map_or("", |i| i.name.as_str());
        acronym_face(name, 16.0, false, p)
    };
    let cells: Vec<El> = guilds.iter().take(4).map(mini).collect();
    let grid = col()
        .gap(2.0)
        .child(row().gap(2.0).children(cells.iter().take(2).cloned()))
        .child(row().gap(2.0).children(cells.iter().skip(2).cloned()));
    row().w_full().h(ROW).center().child(
        row().size(ICON, ICON).center().radius(dim::FOLDER_RADIUS).fill(bg).child(grid),
    )
}

/// `.createGuildButton`: a dashed ring with a plus.
fn add_button(p: &Palette) -> El {
    row().w_full().h(dim::GUILD_LIST_ITEM_BOX_SIZE).center().child(
        row()
            .size(ICON, ICON)
            .center()
            .radius(ICON / 2.0)
            .border(2.0, p.background_modifier_accent)
            .border_dash(4.0, 3.0)
            .hover(mix(p.text_primary, rune::Color::TRANSPARENT, 4.0))
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label("Add a Community")
            .child(icon(&icons::PLUS_BOLD, 20.0).color(p.text_primary)),
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
    }
}
