//! Fluxer's unicode emoji data (`assets/media/data/emojis.json`, `UnicodeEmojis.ts`): names,
//! categories, spritesheet cells and the markdown emoji context. The picker is `expr::emoji`.

use std::sync::OnceLock;

#[derive(Debug)]
pub struct Emoji {
    pub name: String,
    pub glyph: String,
    pub keywords: Vec<String>,
    pub names: Vec<String>,
    /// The five skin-tone variants, when it has them.
    pub skins: Vec<String>,
}

/// (key, label) of each category, in Fluxer's order.
pub const CATEGORIES: [(&str, &str); 8] = [
    ("people", "People"),
    ("nature", "Nature"),
    ("food", "Food & Drink"),
    ("activity", "Activities"),
    ("travel", "Travel & Places"),
    ("objects", "Objects"),
    ("symbols", "Symbols"),
    ("flags", "Flags"),
];

const DATA_JSON: &str = include_str!("../../../../assets/media/data/emojis.json");

/// `emojiData.shortcuts` (emoticon → emoji name) resolved to glyphs, longest emoticon first.
pub fn shortcuts() -> &'static [(String, String)] {
    static DATA: OnceLock<Vec<(String, String)>> = OnceLock::new();
    DATA.get_or_init(|| {
        let v: serde_json::Value = serde_json::from_str(DATA_JSON).unwrap_or_default();
        let mut out: Vec<(String, String)> = v["shortcuts"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, n)| Some((k.clone(), by_name(n.as_str()?)?.glyph.clone())))
            .collect();
        out.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
        out
    })
}

/// (category label, emoji), in Fluxer's order.
pub fn all() -> &'static [(&'static str, Vec<Emoji>)] {
    static DATA: OnceLock<Vec<(&'static str, Vec<Emoji>)>> = OnceLock::new();
    DATA.get_or_init(|| {
        let v: serde_json::Value = serde_json::from_str(DATA_JSON).unwrap_or_default();
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
pub fn codepoints(raw: &str) -> String {
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
pub fn context(input: &str) -> std::sync::Arc<reflux_markdown::EmojiContext> {
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
    reflux_markdown::EmojiContext::parse(&lines).into()
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

/// The emoji's cell on the skin-tone sheets (`skinToneIndex`): its place among the emoji that
/// have skin tones.
pub fn skin_index(e: &Emoji) -> Option<usize> {
    static INDEX: OnceLock<std::collections::HashMap<&'static str, usize>> = OnceLock::new();
    INDEX.get_or_init(|| all().iter().flat_map(|(_, v)| v).filter(|e| !e.skins.is_empty()).enumerate().map(|(i, e)| (e.name.as_str(), i)).collect()).get(e.name.as_str()).copied()
}

/// A skin-tone variant's cell on its tone's sheet: ([`skin_index`], tone 1..=5).
pub fn skin_sprite(raw: &str) -> Option<(usize, u8)> {
    let canon = |s: &str| s.replace('\u{FE0F}', "");
    let raw = canon(raw);
    let e = by_name(name_of(&raw)?)?;
    let tone = e.skins.iter().position(|s| canon(s) == raw)?;
    Some((skin_index(e)?, tone as u8 + 1))
}

/// The name of the emoji a surrogate (or a skin-tone variant) draws.
pub fn name_of(glyph: &str) -> Option<&'static str> {
    surrogates().0.get(glyph).copied()
}

/// A unicode emoji by any of its names.
pub fn by_name(name: &str) -> Option<&'static Emoji> {
    all().iter().flat_map(|(_, v)| v).find(|e| e.names.iter().any(|n| n == name))
}

#[cfg(test)]
mod tests {
    #[test]
    fn skin_tone_variants_find_their_sheet_cell() {
        let base = super::by_name("thumbsup").expect("thumbsup");
        let (i, tone) = super::skin_sprite(&base.skins[2]).expect("a skin-tone variant");
        assert_eq!((Some(i), tone), (super::skin_index(base), 3));
        assert_eq!(super::skin_sprite(&base.glyph), None, "the base glyph is on the base sheet");
        assert_eq!(super::codepoints("👍🏽"), "1f44d-1f3fd");
    }

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
        let clap = super::by_name("clap").expect("clap");
        assert_eq!(clap.skins.len(), 5);
        assert!(super::skin_index(clap).is_some());
        assert_eq!(super::skin_index(super::by_name("grinning").expect("grinning")), None);
    }
}
