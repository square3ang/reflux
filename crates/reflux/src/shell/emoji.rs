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
    names: Vec<String>,
    /// The five skin-tone variants, when it has them.
    skins: Vec<String>,
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
                        let names = strs(&e["names"]);
                        Some(Emoji {
                            name: names.first()?.clone(),
                            glyph: e["surrogates"].as_str()?.to_owned(),
                            keywords: strs(&e["keywords"]),
                            names,
                            skins: e["skins"].as_array().into_iter().flatten().filter_map(|s| s["surrogates"].as_str().map(str::to_owned)).collect(),
                        })
                    })
                    .collect();
                (label, emoji)
            })
            .collect()
    })
}

/// The emoji's cell in `spritesheet-emoji@2x.png`: its place in Fluxer's emoji list
/// (`setSpriteSheetIndex`), matched without variation selectors. Skin-tone variants have none.
pub fn sprite_index(raw: &str) -> Option<usize> {
    static INDEX: OnceLock<std::collections::HashMap<String, usize>> = OnceLock::new();
    let canon = |s: &str| s.replace('\u{FE0F}', "");
    INDEX.get_or_init(|| all().iter().flat_map(|(_, v)| v).enumerate().map(|(i, e)| (canon(&e.glyph), i)).collect()).get(&canon(raw)).copied()
}

/// `convertToCodePoints`: lowercase hex code points joined by `-`, without U+FE0F unless it's
/// a ZWJ sequence.
fn codepoints(raw: &str) -> String {
    let keep = raw.contains('\u{200D}');
    raw.chars().filter(|&c| keep || c != '\u{FE0F}').map(|c| format!("{:x}", c as u32)).collect::<Vec<_>>().join("-")
}

/// Surrogates (as written and without U+FE0F) to the emoji's name, and the longest in bytes.
fn surrogates() -> &'static (std::collections::HashMap<String, &'static str>, usize) {
    static MAP: OnceLock<(std::collections::HashMap<String, &'static str>, usize)> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        for e in all().iter().flat_map(|(_, v)| v) {
            for s in std::iter::once(&e.glyph).chain(&e.skins) {
                map.entry(s.replace('\u{FE0F}', "")).or_insert(e.name.as_str());
                map.insert(s.clone(), e.name.as_str());
            }
        }
        let longest = map.keys().map(String::len).max().unwrap_or(0);
        (map, longest)
    })
}

/// `buildEmojiContext` (`MarkdownParserWasm.ts`): where `input` has unicode emoji (longest
/// match first; ™ © ® stay text), and the emoji its `:shortcodes:` and their skin tones name.
pub fn context(input: &str) -> reflux_markdown::EmojiContext {
    let (map, longest) = surrogates();
    let mut lines = String::new();
    let mut at = 0;
    while at < input.len() {
        let rest = &input[at..];
        // Only keycaps (`1️⃣`, `#️⃣`, `*️⃣`) start with ASCII; skip the rest cheaply.
        if rest.as_bytes()[0].is_ascii() && !matches!(rest.as_bytes()[0], b'0'..=b'9' | b'#' | b'*') {
            at += 1;
            continue;
        }
        let ends = rest.char_indices().map(|(i, c)| i + c.len_utf8()).take_while(|&e| e <= *longest);
        let hit = ends.collect::<Vec<_>>().into_iter().rev().find_map(|e| map.get(&rest[..e]).map(|n| (e, *n)));
        match hit {
            Some((len, name)) if !matches!(rest[..len].trim_end_matches('\u{FE0F}'), "™" | "©" | "®") => {
                let raw = &rest[..len];
                lines += &format!("S\t{at}\t{len}\t{raw}\t{name}\t{}\n", codepoints(raw));
                at += len;
            }
            _ => at += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    let mut seen = std::collections::HashSet::new();
    for name in input.split(':').skip(1).filter(|n| !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')) {
        if matches!(name, "tm" | "copyright" | "registered") || !seen.insert(name) {
            continue;
        }
        let Some(e) = all().iter().flat_map(|(_, v)| v).find(|e| e.names.iter().any(|n| n == name)) else { continue };
        lines += &format!("C\t{name}\t{}\t{}\n", e.glyph, codepoints(&e.glyph));
        for (tone, skin) in e.skins.iter().enumerate() {
            lines += &format!("K\t{name}\t{}\t{skin}\t{}\n", tone + 1, codepoints(skin));
        }
    }
    reflux_markdown::EmojiContext::parse(&lines)
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

    #[test]
    fn unicode_emoji_and_shortcodes_parse_as_emoji() {
        use reflux_markdown::{EmojiKind, Node, Options, parse, presets};
        let src = "a 😀 b ❤ c 👍🏽 :tada: ™";
        let opts = Options { flags: presets::STANDARD, emoji: super::context(src) };
        let raws: Vec<String> = parse(src, &opts)
            .iter()
            .filter_map(|n| match n {
                Node::Emoji { kind: EmojiKind::Standard { raw, .. } } => Some(raw.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(raws, ["😀", "❤", "👍🏽", "🎉"]);
        assert_eq!(super::sprite_index("😀"), Some(0));
        assert_eq!(super::sprite_index("❤"), super::sprite_index("❤️"));
        assert_eq!(super::sprite_index("👍🏽"), None, "skin tones aren't on the base sheet");
    }
}
