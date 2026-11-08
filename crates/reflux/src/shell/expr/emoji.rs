//! The Emojis tab (`EmojiPicker`): favorites, frequently used, each guild's custom emoji, then
//! the unicode categories; search, skin tones and the inspector.

use std::rc::Rc;

use reflux_model::{Channel, Permissions, ReactionEmoji};
use reflux_store::Store;
use rune::{El, Tx, col, el, row, text};

use super::{CellCx, For, Grid, Item, Row, Section, ShellAccess, ShellState, close, grid_key, grid_list, hover_cell, inspector, with_menu, rail, search_bar};
use crate::icons;
use crate::media::Load;
use crate::shell::Intent;
use crate::shell::emoji::{self as data, Emoji};
use crate::theme::{Palette, font};
use crate::i18n::t;

/// `EMOJI_PICKER_CELL_SIZE` and columns at the default width.
const GRID: Grid = Grid { cell: 48.0, row_h: 48.0, cols: 11, kind: "emoji" };

/// The unicode categories' rail icons, in [`data::CATEGORIES`] order.
const CAT_ICONS: [&rune::Icon; 8] =
    [&icons::SMILEY_FILL, &icons::LEAF_FILL, &icons::BOWL_FOOD_FILL, &icons::GAME_CONTROLLER_FILL, &icons::BICYCLE_FILL, &icons::MAGNET_FILL, &icons::HEART_FILL, &icons::FLAG_FILL];

/// The surrogate a unicode emoji sends at skin tone `tone`.
fn surrogate(e: &Emoji, tone: u8) -> &str {
    match tone {
        1..=5 => e.skins.get(tone as usize - 1).unwrap_or(&e.glyph),
        _ => &e.glyph,
    }
}

/// What reacting with `item` sends.
pub fn reaction_emoji(item: &Item, tone: u8) -> ReactionEmoji {
    match item {
        Item::Unicode(e) => ReactionEmoji { name: surrogate(e, tone).to_string(), ..Default::default() },
        Item::Emoji { id, name, animated, .. } => ReactionEmoji { id: Some(*id), name: name.clone(), animated: Some(*animated) },
        Item::Sticker { .. } => ReactionEmoji::default(),
    }
}

/// What the composer gets: `<:name:id>`, `<a:name:id>` or the surrogate.
fn wire(item: &Item, tone: u8) -> String {
    match item {
        Item::Unicode(e) => surrogate(e, tone).to_string(),
        Item::Emoji { id, name, animated, .. } => format!("<{}:{name}:{id}>", if *animated { "a" } else { "" }),
        Item::Sticker { .. } => String::new(),
    }
}

/// An emoji drawn at `size`: custom from the CDN, unicode from the (skin-tone) spritesheet.
pub(super) fn glyph(media: &crate::media::Media, item: &Item, tone: u8, size: f32) -> El {
    let load = match item {
        Item::Unicode(e) => match data::skin_index(e).filter(|_| tone > 0) {
            Some(i) => media.skin_sprite(i, tone),
            None => data::sprite_index(&e.glyph).map_or(Load::Failed, |i| media.sprite(i)),
        },
        Item::Emoji { id, .. } => media.asset(|b, _| crate::media::emoji_url(b, &id.to_string())),
        Item::Sticker { .. } => Load::Failed,
    };
    match load {
        Load::Ready(px) => el().size(size, size).pixels_fit(&px, rune::SurfaceFit::Fit),
        _ => match item {
            Item::Unicode(e) => text(surrogate(e, tone).to_string()).font(font(size * 0.8, 400.0).lh(size)),
            _ => el().size(size, size),
        },
    }
}

/// Every custom emoji usable in `channel`, guild-list order.
fn custom(store: &Store, st: &ShellState, channel: Option<&Channel>) -> Vec<Item> {
    super::guild_order(store, st)
        .into_iter()
        .filter(|&g| super::can_use(store, st, channel, g, Permissions::USE_EXTERNAL_EMOJIS))
        .filter_map(|g| store.guilds.get(&g).map(|s| (g, s)))
        .flat_map(|(g, s)| s.emojis.iter().map(move |e| Item::Emoji { guild: g, id: e.id, name: e.name.clone(), animated: e.animated }))
        .collect()
}

fn rows(store: &Store, st: &ShellState, channel: Option<&Channel>) -> Vec<Row> {
    let custom = custom(store, st, channel);
    let q = st.expr.query.text.trim().to_lowercase();
    let search = (!q.is_empty()).then(|| {
        let mine = custom.iter().filter(|i| matches!(i, Item::Emoji { name, .. } if name.to_lowercase().contains(&q))).cloned();
        mine.chain(data::search(&q).into_iter().map(Item::Unicode)).collect()
    });
    let unicode: Vec<Item> = data::all().iter().flat_map(|(_, v)| v.iter().map(Item::Unicode)).collect();
    let all: Vec<Item> = custom.iter().cloned().chain(unicode).collect();
    let cats = data::CATEGORIES
        .iter()
        .zip(data::all())
        .map(|((key, label), (_, list))| Section { cat: key.to_string(), name: label.to_string(), guild: None, items: list.iter().map(Item::Unicode).collect() })
        .collect();
    let x = &st.expr.saved;
    let sections = super::sections(store, st, &all, &x.favorite_emojis, &x.emoji_usage, cats);
    super::build_rows(search, sections, GRID.cols, &x.collapsed, "")
}

/// Puts `item` where it goes, counts the use, and closes unless Shift is held.
fn pick(st: &mut ShellState, item: &Item, shift: bool, _: &mut Tx) {
    let tone = st.expr.saved.skin_tone;
    match st.expr.target {
        Some(For::Composer(_)) => {
            let end = st.composer.anchor.max(st.composer.focus);
            let after = st.composer.text.get(end..).and_then(|s| s.chars().next());
            let space = if after.is_none_or(|c| !c.is_whitespace()) { " " } else { "" };
            match item {
                // `ComposerCustomEmojiNode`: the emoji's image in the draft, sent as its token.
                Item::Emoji { .. } => {
                    let (a, b) = st.composer.range();
                    st.composer.undo_group(|e| {
                        e.replace_with_pill(a..b, wire(item, tone), &crate::compose::emoji_label());
                        e.insert(space);
                    });
                }
                _ => st.composer.insert(&format!("{}{space}", wire(item, tone))),
            }
        }
        Some(For::React { channel, message }) => st.intents.push(Intent::React { channel, message, emoji: reaction_emoji(item, tone), add: true }),
        Some(For::Form { key, .. }) => {
            let e = reaction_emoji(item, tone);
            st.form.data.insert(key.into(), serde_json::json!({ "id": e.id.map(|i| i.to_string()), "name": e.name }));
            if key == "role-emoji" {
                crate::guild::role_extra::emoji_picked(st, e.id.map(|i| i.to_string()));
            }
        }
        // `useCustomStatusDraft.handleEmojiSelect`: a custom emoji's id and name, else the
        // skin-toned surrogate.
        Some(For::Status) => {
            let e = reaction_emoji(item, tone);
            let v = serde_json::json!({ "emoji_id": e.id.map(|i| i.to_string()), "emoji_name": e.name, "emoji_animated": e.animated.unwrap_or(false) });
            st.form.data.insert("m.emoji".into(), v);
            st.expr.track(&item.key(), crate::app::now_ms());
            return close(st);
        }
        None => return,
    }
    st.expr.track(&item.key(), crate::app::now_ms());
    if !shift {
        close(st);
    }
}

fn cell<S: 'static>(cx: &CellCx, item: &Item, g: Grid, selected: bool, acc: ShellAccess<S>) -> El {
    let size = if matches!(item, Item::Unicode(_)) { 32.0 } else { 40.0 };
    let it = item.clone();
    let mut c = row()
        .size(g.cell, g.cell)
        .center()
        .radius(8.0)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(item.names())
        .child(glyph(&cx.media, item, cx.tone, size))
        // Alt toggles the favorite; Shift keeps the picker open.
        .on_press_tx(move |s: &mut S, tx| {
            let m = tx.mods();
            let st = acc(s);
            if m.alt {
                st.expr.toggle_favorite(&it.key());
            } else {
                pick(st, &it, m.shift, tx);
            }
        });
    c = if selected { c.fill(cx.p.background_modifier_selected) } else { c.hover(cx.p.background_modifier_selected) };
    with_menu(hover_cell(c, item.key(), acc), item, acc)
}

/// The clap at the current tone and, when open, every tone (current first) in a column.
fn skin_tones<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let Some(clap) = data::by_name("clap") else { return el() };
    let tone = st.expr.saved.skin_tone;
    let one = |t: u8, size: f32| glyph(&st.media, &Item::Unicode(clap), t, size);
    let button = row().size(24.0, 24.0).center().cursor(rune::Cursor::Pointer).role("button").label(t("Skin tone")).child(one(tone, 24.0)).on_press(move |s: &mut S| {
        let x = &mut acc(s).expr;
        x.skin_open = !x.skin_open;
    });
    if !st.expr.skin_open {
        return button.key("skin");
    }
    const LABELS: [&str; 6] = ["Default skin tone", "Light skin tone", "Medium-light skin tone", "Medium skin tone", "Medium-dark skin tone", "Dark skin tone"];
    let order = std::iter::once(tone).chain((0..=5).filter(|&t| t != tone));
    let options = col().pin(None, Some(-6.0), Some(-6.0), None).w(36.0).h(192.0).radius(8.0).fill(p.background_secondary).children(order.map(|t| {
        row()
            .size(36.0, 32.0)
            .center()
            .radius(6.0)
            .hover(p.background_modifier_hover)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(LABELS[t as usize])
            .child(one(t, 24.0))
            .on_press(move |s: &mut S| {
                let x = &mut acc(s).expr;
                x.saved.skin_tone = t;
                x.skin_open = false;
                x.save();
            })
    }));
    el().size(24.0, 24.0).key("skin-open").child(options)
}

/// (search bar, body).
pub fn view<S: 'static>(store: &Store, st: &ShellState, channel: Option<&Channel>, _: For, p: &Palette, acc: ShellAccess<S>) -> (El, El) {
    let rows = Rc::new(rows(store, st, channel));
    let hovered = st.expr.hover.as_ref().and_then(|k| super::item_rows(&rows).into_iter().flat_map(|r| r.1.iter()).find(|i| i.key() == *k).cloned());
    let placeholder = hovered.as_ref().map_or_else(|| t("Find the emoji of your dreams").to_string(), Item::names);
    let keys = rows.clone();
    let bar = row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(search_bar(st, placeholder, p, acc, move |s: &mut S, k, tx| grid_key(acc(s), &keys, k, tx, "emoji-grid", pick)))
        .child(skin_tones(st, p, acc));
    let empty = rows.is_empty();
    let extra = data::CATEGORIES.iter().zip(CAT_ICONS).map(|((k, l), i)| (k.to_string(), i, *l)).collect();
    let mut grid = col().grow(1.0).min_w(0.0).h_full().child(grid_list(store, st, rows.clone(), GRID, p, acc, cell));
    if empty {
        grid = grid.child(super::empty_state(t("No emojis match that search"), None, p));
    }
    let tone = st.expr.saved.skin_tone;
    let body = col()
        .w_full()
        .h_full()
        .child(
            row()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(el().w(super::RAIL_W).h_full().child(rail(store, st, &rows, "emoji-grid", extra, p, acc)))
                .child(grid),
        )
        .child(inspector(store, hovered.as_ref().map(|i| (i, glyph(&st.media, i, tone, 28.0))), p));
    (bar, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_forms() {
        let clap = data::by_name("clap").unwrap();
        assert_eq!(wire(&Item::Unicode(clap), 0), "👏");
        assert_eq!(wire(&Item::Unicode(clap), 3), "👏🏽");
        let e = Item::Emoji { guild: reflux_model::Snowflake(1), id: reflux_model::Snowflake(9), name: "party".into(), animated: true };
        assert_eq!(wire(&e, 0), "<a:party:9>");
        assert_eq!(reaction_emoji(&e, 0).id, Some(reflux_model::Snowflake(9)));
    }

    #[test]
    fn composer_pick_inserts_with_trailing_space() {
        let mut st = ShellState::default();
        st.expr.target = Some(For::Composer(reflux_model::Snowflake(1)));
        pick(&mut st, &Item::Unicode(data::by_name("tada").unwrap()), true, &mut Tx::default());
        assert_eq!(st.composer.text, "🎉 ");
        assert!(st.expr.saved.emoji_usage.contains_key("unicode:tada"));
        assert!(st.expr.target.is_some(), "Shift keeps it open");
    }
}
