//! The Stickers tab (`StickersPicker`): favorites, frequently used, then each guild's stickers.

use std::rc::Rc;

use reflux_model::{Channel, Permissions, Snowflake};
use reflux_store::Store;
use rune::{El, Tx, col, el, icon, row, text};

use super::{CellCx, For, Grid, Item, ShellAccess, ShellState, close, grid_key, grid_list, hover_cell, inspector, with_menu, rail, search_bar};
use crate::media::Load;
use crate::shell::Intent;
use crate::icons;
use crate::theme::{Palette, font};
use crate::i18n::t;

/// `STICKER_PICKER_ITEM_SIZE` (+ the 8px gap) over four columns.
const GRID: Grid = Grid { cell: 126.0, row_h: 134.0, cols: 4, kind: "sticker" };

pub(super) fn image(media: &crate::media::Media, id: reflux_model::Snowflake, size: f32) -> El {
    match media.asset(|b, _| crate::media::sticker_url(b, id, false)) {
        Load::Ready(px) => el().size(size, size).pixels_fit(&px, rune::SurfaceFit::Fit),
        _ => el().size(size, size),
    }
}

fn all(store: &Store, st: &ShellState, channel: Option<&Channel>) -> Vec<Item> {
    super::guild_order(store, st)
        .into_iter()
        .filter(|&g| super::can_use(store, st, channel, g, Permissions::USE_EXTERNAL_STICKERS))
        .filter_map(|g| store.guilds.get(&g).map(|s| (g, s)))
        .flat_map(|(g, s)| {
            s.stickers.iter().map(move |x| Item::Sticker {
                guild: g,
                id: x.id,
                name: x.name.clone(),
                description: x.description.clone(),
                tags: x.tags.clone(),
            })
        })
        .collect()
}

/// `handleStickerSelect`: with text or files in the composer the sticker waits beside them
/// (`setPendingSticker`); otherwise it goes at once.
fn pick(st: &mut ShellState, item: &Item, _: bool, _: &mut Tx) {
    let (Some(For::Composer(channel)), Item::Sticker { id, .. }) = (st.expr.target, item) else { return };
    if !st.composer.text.trim().is_empty() || !st.uploads.is_empty() {
        st.pending_sticker = Some((channel, *id));
    } else {
        st.intents.push(Intent::SendExtra { channel, sticker: Some(*id), meme: None });
    }
    st.expr.track(&item.key(), crate::app::now_ms());
    close(st);
}

/// `ChannelStickersArea`: the staged sticker (64px preview, `:name:`, description) and its remove
/// button, above the composer field.
pub fn pending<S: 'static>(store: &Store, st: &ShellState, channel: Snowflake, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let (_, id) = st.pending_sticker.filter(|(c, _)| *c == channel)?;
    let x = store.guilds.values().flat_map(|g| g.stickers.iter()).find(|x| x.id == id);
    let (name, desc) = x.map_or((String::new(), String::new()), |x| (x.name.clone(), x.description.clone()));
    let mut info = col().gap(2.0).child(text(format!(":{name}:")).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary));
    if !desc.is_empty() {
        info = info.child(text(desc).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted));
    }
    let remove = row()
        .size(32.0, 32.0)
        .center()
        .radius(6.0)
        .hover(p.background_modifier_hover)
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(t("Remove sticker"))
        .key("pending-sticker-remove")
        .child(icon(&icons::X_BOLD, 20.0).color(p.text_primary_muted).group_color(p.text_primary))
        .on_press(move |s: &mut S| acc(s).pending_sticker = None);
    Some(
        row().px(16.0).py(8.0).child(
            row()
                .gap(8.0)
                .center_y()
                .child(row().size(64.0, 64.0).center().radius(6.0).fill(p.background_tertiary).child(image(&st.media, id, 56.0)))
                .child(info)
                .child(remove),
        ),
    )
}

fn cell<S: 'static>(cx: &CellCx, item: &Item, g: Grid, selected: bool, acc: ShellAccess<S>) -> El {
    let Item::Sticker { id, .. } = item else { return el() };
    let it = item.clone();
    let mut c = row()
        .size(g.cell, g.cell)
        .center()
        .radius(8.0)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(item.names())
        .child(image(&cx.media, *id, g.cell - 16.0))
        .on_press_tx(move |s: &mut S, tx| {
            let m = tx.mods();
            let st = acc(s);
            if m.alt { st.expr.toggle_favorite(&it.key()) } else { pick(st, &it, m.shift, tx) }
        });
    c = if selected { c.fill(cx.p.background_modifier_selected) } else { c.hover(cx.p.background_modifier_selected) };
    with_menu(hover_cell(c, item.key(), acc), item, acc)
}

pub fn view<S: 'static>(store: &Store, st: &ShellState, channel: Option<&Channel>, _: For, p: &Palette, acc: ShellAccess<S>) -> (El, El) {
    let all = all(store, st, channel);
    let q = st.expr.query.text.trim().to_lowercase();
    let search = (!q.is_empty()).then(|| {
        all.iter()
            .filter(|i| match i {
                Item::Sticker { name, description, tags, .. } => {
                    name.to_lowercase().contains(&q) || description.to_lowercase().contains(&q) || tags.iter().any(|t| t.to_lowercase().contains(&q))
                }
                _ => false,
            })
            .cloned()
            .collect()
    });
    let x = &st.expr.saved;
    let sections = super::sections(store, st, &all, &x.favorite_stickers, &x.sticker_usage, vec![]);
    let rows = Rc::new(super::build_rows(search, sections, GRID.cols, &x.collapsed, "sticker:"));
    let hovered = st.expr.hover.as_ref().and_then(|k| all.iter().find(|i| i.key() == *k).cloned());
    let keys = rows.clone();
    let bar = search_bar(st, t("Find the perfect sticker").into(), p, acc, move |s: &mut S, k, tx| grid_key(acc(s), &keys, k, tx, "sticker-grid", pick));
    let mut grid = col().grow(1.0).min_w(0.0).h_full().child(grid_list(store, st, rows.clone(), GRID, p, acc, cell));
    if rows.is_empty() {
        grid = grid.child(match (all.is_empty(), q.is_empty()) {
            (true, true) => super::empty_state(t("No stickers yet"), Some(t("Stickers added to your communities show up here.")), p),
            _ => super::empty_state(t("No stickers match that search"), None, p),
        });
    }
    let glyph = hovered.as_ref().and_then(|i| if let Item::Sticker { id, .. } = i { Some((i, image(&st.media, *id, 28.0))) } else { None });
    let body = col()
        .w_full()
        .h_full()
        .child(
            row()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(el().w(super::RAIL_W).h_full().child(rail(store, st, &rows, "sticker-grid", vec![], p, acc)))
                .child(grid),
        )
        .child(inspector(store, glyph, p));
    (bar, body)
}
