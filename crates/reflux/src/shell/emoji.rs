//! The reaction emoji picker: Fluxer's unicode emoji data (`assets/media/data/emojis.json`) in a
//! searchable grid, opened from a message's add-reaction button (`EmojiPickerPopout`).
// shortcut: unicode only, glyphs drawn from the font rather than the twemoji spritesheet, no
// custom emoji, recents, skin tones or category tabs; port `EmojiPicker` fully later.

use std::sync::OnceLock;

use reflux_model::{ReactionEmoji, Snowflake};
use rune::{El, col, icon, row, text, text_input};

use super::{Intent, ShellAccess, ShellState};
use crate::icons;
use crate::theme::{Palette, font};
use crate::ui::overlay::popout_shadow;
use crate::ui::{over, scroller};

pub struct Emoji {
    pub name: String,
    pub glyph: String,
    keywords: Vec<String>,
}

const CATEGORIES: [(&str, &str); 8] = [
    ("people", "People"),
    ("nature", "Nature"),
    ("food", "Food & Drink"),
    ("activity", "Activities"),
    ("travel", "Travel & Places"),
    ("objects", "Objects"),
    ("symbols", "Symbols"),
    ("flags", "Flags"),
];

/// (category label, emoji), in Fluxer's order.
pub fn all() -> &'static [(&'static str, Vec<Emoji>)] {
    static DATA: OnceLock<Vec<(&'static str, Vec<Emoji>)>> = OnceLock::new();
    DATA.get_or_init(|| {
        let v: serde_json::Value = serde_json::from_str(include_str!("../../../../assets/media/data/emojis.json")).unwrap_or_default();
        let strs = |v: &serde_json::Value| -> Vec<String> {
            v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_owned)).collect()
        };
        CATEGORIES
            .iter()
            .map(|&(key, label)| {
                let list = v["categories"][key].as_array().into_iter().flatten();
                let emoji = list
                    .filter_map(|e| {
                        Some(Emoji {
                            name: strs(&e["names"]).into_iter().next()?,
                            glyph: e["surrogates"].as_str()?.to_owned(),
                            keywords: strs(&e["keywords"]),
                        })
                    })
                    .collect();
                (label, emoji)
            })
            .collect()
    })
}

/// Emoji whose name contains `q` or a keyword starts with it (case-insensitive).
pub fn search(q: &str) -> Vec<&'static Emoji> {
    let q = q.trim().to_lowercase();
    all()
        .iter()
        .flat_map(|(_, v)| v)
        .filter(|e| e.name.contains(&q) || e.keywords.iter().any(|k| k.starts_with(&q)))
        .collect()
}

const COLS: usize = 9;
const CELL: f32 = 40.0;

enum Row {
    Head(&'static str),
    Cells(Vec<&'static Emoji>),
    Empty,
}

fn cell<S: 'static>(e: &'static Emoji, p: &Palette, channel: Snowflake, message: Snowflake, acc: ShellAccess<S>) -> El {
    row()
        .size(CELL, CELL)
        .center()
        .radius(6.0)
        .hover(p.surface_interactive_hover_bg)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(format!(":{}:", e.name))
        .child(text(e.glyph.clone()).font(font(28.0, 400.0).lh(32.0)))
        .on_press(move |s: &mut S| {
            let st = acc(s);
            st.popups.close();
            st.active_msg = None;
            let emoji = ReactionEmoji { name: e.glyph.clone(), ..Default::default() };
            st.intents.push(Intent::React { channel, message, emoji, add: true });
        })
}

fn build_row<S: 'static>(r: &Row, p: &Palette, channel: Snowflake, message: Snowflake, acc: ShellAccess<S>) -> El {
    match r {
        Row::Head(label) => text(label.to_uppercase()).font(font(12.0, 600.0).lh(16.0).tracking(0.24)).color(p.text_tertiary).px(4.0).pt(8.0).pb(4.0),
        Row::Cells(c) => row().children(c.iter().map(|e| cell(e, p, channel, message, acc))),
        Row::Empty => text("No emoji match that search.").font(font(14.0, 400.0)).color(p.text_tertiary).padding(16.0),
    }
}

/// The picker surface for reacting to `message`; the caller anchors it.
pub fn picker<S: 'static>(st: &ShellState, channel: Snowflake, message: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let bg = p.form_surface_background;
    let cells = |list: Vec<&'static Emoji>| list.chunks(COLS).map(|c| Row::Cells(c.to_vec())).collect::<Vec<_>>();
    let mut rows = Vec::new();
    if st.emoji_query.text.trim().is_empty() {
        for (label, list) in all() {
            rows.push(Row::Head(label));
            rows.extend(cells(list.iter().collect()));
        }
    } else {
        let found = search(&st.emoji_query.text);
        if found.is_empty() {
            rows.push(Row::Empty);
        }
        rows.extend(cells(found));
    }
    let (n, pal, q) = (rows.len(), *p, st.emoji_query.text.trim().to_lowercase());
    let list = col().w_full().key("emoji-grid").growth(rune::GrowthSpec::default()).virtual_rows(
        n,
        CELL,
        move |_: &S, i| format!("emoji-{q}-{i}").into(),
        move |_: &S, i| build_row(&rows[i], &pal, channel, message, acc),
    );
    let search = row()
        .w_full()
        .h(36.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .fill(p.background_textarea)
        .border(1.0, over(p.background_modifier_accent, p.background_textarea))
        .child(icon(&icons::MAGNIFYING_GLASS, 16.0).color(p.text_tertiary))
        .child(
            text_input(&st.emoji_query, font(14.0, 400.0).lh(20.0))
                .key("emoji-search")
                .grow(1.0)
                .single_line()
                .color(p.text_primary)
                .placeholder("Find the perfect emoji", p.text_tertiary)
                .caret(p.text_primary, p.text_selection)
                .on_edit(move |s: &mut S| -> &mut rune::TextEdit { &mut acc(s).emoji_query }),
        );
    col()
        .w(COLS as f32 * CELL + 2.0 * 9.0)
        .h(420.0)
        .padding(8.0)
        .gap(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .radius(8.0)
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .role("dialog")
        .label("Emoji picker")
        .child(search)
        .child(col().w_full().grow(1.0).min_h(0.0).child(scroller(col().w_full().h_full().scroll(rune::ScrollSpec { stick_bottom: false }).child(list), false, p)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn data_loads_and_searches() {
        let all = super::all();
        assert_eq!(all.len(), 8);
        assert!(all[0].1.len() > 500, "people");
        let hits = super::search("thumbs");
        assert!(hits.iter().any(|e| e.glyph == "👍"), "thumbsup by name");
        assert!(super::search("THUMBS").len() == hits.len(), "case-insensitive");
    }
}
