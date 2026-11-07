//! The Quick CSS editors' syntax colours (`QuickCssEditor` `highlightStyle` over lezer-css's tags,
//! `TOKEN_COLORS`) and theme-variable completions (`CssCompletions` `variableSource`), for the
//! in-tab editor and the editor window.
// shortcut: only the variable completions; selector, `data-flx`, property and snippet completions,
// hover info, swatches, lint and the outline target the DOM, which Reflux doesn't have (only
// `:root` variables apply). Ctrl+Space doesn't force the list open.

use std::cell::Cell;
use std::ops::Range;

use rune::{Color, El, Key, Span, TextEdit, col, row, text};

use crate::theme::{Palette, font};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tok {
    Comment,
    /// Property and variable names.
    Property,
    /// At-rules, `!important`, function names, `and`/`not`.
    Keyword,
    Tag,
    /// Classes, ids and pseudo-classes.
    Selector,
    /// Numbers, units and keyword values.
    Number,
    /// Strings and colours.
    Str,
    Punct,
}

#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    Selector,
    AtPrelude,
    Property,
    Value,
}

fn ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c >= 0x80
}

fn ident_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && (ident_char(b[i]) || b[i] == b'\\') {
        i += if b[i] == b'\\' { 2 } else { 1 };
    }
    i.min(b.len())
}

/// What the statement starting at `i` is: in a declaration block, a selector when a `{` comes
/// before the next `;` or `}` (a nested rule), else a declaration.
fn statement(css: &str, i: usize, decls: bool) -> Ctx {
    let rest = css[i..].trim_start();
    if rest.starts_with('@') {
        return Ctx::AtPrelude;
    }
    if !decls {
        return Ctx::Selector;
    }
    match rest.find(['{', ';', '}']) {
        Some(n) if rest.as_bytes()[n] == b'{' => Ctx::Selector,
        _ => Ctx::Property,
    }
}

/// The highlighted ranges of `css`; text outside them is plain.
pub fn tokens(css: &str) -> Vec<(Range<usize>, Tok)> {
    let b = css.as_bytes();
    let mut out = Vec::new();
    // Per open block: whether it holds declarations (else rules, like `@media`).
    let mut stack: Vec<bool> = Vec::new();
    let mut ctx = Ctx::Selector;
    let mut at_rule = String::new();
    let mut in_attr = false;
    let mut i = 0;
    let decls = |s: &[bool]| s.last().copied().unwrap_or(false);
    while i < b.len() {
        let c = b[i];
        let start = i;
        if css[i..].starts_with("/*") {
            i = css[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
            out.push((start..i, Tok::Comment));
            continue;
        }
        if c == b'"' || c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != c && b[i] != b'\n' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(b.len());
            out.push((start..i, Tok::Str));
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let digit_at = |j: usize| b.get(j).is_some_and(u8::is_ascii_digit);
        let number = c.is_ascii_digit() || (c == b'.' && digit_at(i + 1)) || ((c == b'-' || c == b'+') && (digit_at(i + 1) || (b.get(i + 1) == Some(&b'.') && digit_at(i + 2))));
        match c {
            b'{' => {
                out.push((i..i + 1, Tok::Punct));
                let holds = match ctx {
                    Ctx::AtPrelude => !matches!(at_rule.as_str(), "media" | "supports" | "layer" | "container" | "document" | "keyframes" | "scope" | "starting-style"),
                    _ => true,
                };
                stack.push(holds);
                i += 1;
                ctx = statement(css, i, decls(&stack));
            }
            b'}' => {
                out.push((i..i + 1, Tok::Punct));
                stack.pop();
                i += 1;
                ctx = statement(css, i, decls(&stack));
            }
            b';' => {
                out.push((i..i + 1, Tok::Punct));
                i += 1;
                ctx = statement(css, i, decls(&stack));
            }
            b'@' => {
                i = ident_end(b, i + 1);
                at_rule = css[start + 1..i].to_ascii_lowercase();
                out.push((start..i, Tok::Keyword));
                ctx = Ctx::AtPrelude;
            }
            b':' if ctx == Ctx::Property => {
                out.push((i..i + 1, Tok::Punct));
                i += 1;
                ctx = Ctx::Value;
            }
            b'!' if ctx == Ctx::Value && css[i + 1..].trim_start().get(..9).is_some_and(|w| w.eq_ignore_ascii_case("important")) => {
                i = css[i + 1..].find(|ch: char| ch.is_ascii_alphabetic()).map_or(b.len(), |n| i + 1 + n + 9);
                out.push((start..i, Tok::Keyword));
            }
            b'#' if ctx == Ctx::Value => {
                i = ident_end(b, i + 1);
                out.push((start..i, Tok::Str));
            }
            b'#' | b':' if ctx == Ctx::Selector => {
                while i < b.len() && (b[i] == b':' || b[i] == b'#') {
                    i += 1;
                }
                out.push((start..i, Tok::Punct));
                let end = ident_end(b, i);
                if end > i {
                    out.push((i..end, Tok::Selector));
                    i = end;
                }
            }
            b'.' if ctx == Ctx::Selector && b.get(i + 1).is_some_and(|&n| ident_char(n) && !n.is_ascii_digit()) => {
                let end = ident_end(b, i + 1);
                out.push((i + 1..end, Tok::Selector));
                i = end;
            }
            _ if number && !(ctx == Ctx::Selector && !c.is_ascii_digit()) => {
                i += 1;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                    i += 1;
                }
                i = if b.get(i) == Some(&b'%') { i + 1 } else { ident_end(b, i) };
                out.push((start..i, Tok::Number));
            }
            _ if ident_char(c) || c == b'\\' => {
                i = ident_end(b, i);
                let word = &css[start..i];
                let call = b.get(i) == Some(&b'(');
                let tok = match ctx {
                    _ if in_attr => None,
                    Ctx::Selector if matches!(word, "from" | "to") => Some(Tok::Keyword),
                    Ctx::Selector => Some(Tok::Tag),
                    Ctx::Property => Some(Tok::Property),
                    Ctx::AtPrelude if matches!(word, "and" | "or" | "not" | "only") => Some(Tok::Keyword),
                    Ctx::AtPrelude if css[i..].trim_start().starts_with(':') => Some(Tok::Property),
                    _ if call => Some(Tok::Keyword),
                    _ if word.starts_with("--") => Some(Tok::Property),
                    _ => Some(Tok::Number),
                };
                if let Some(t) = tok {
                    out.push((start..i, t));
                }
                // `url(…)` without quotes is `ParenthesizedContent`.
                if call && word.eq_ignore_ascii_case("url") && !css[i + 1..].trim_start().starts_with(['"', '\'']) {
                    out.push((i..i + 1, Tok::Punct));
                    let end = css[i..].find(')').map_or(b.len(), |n| i + n);
                    out.push((i + 1..end, Tok::Str));
                    i = end;
                }
            }
            _ => {
                in_attr = match c {
                    b'[' => true,
                    b']' => false,
                    _ => in_attr,
                };
                // Multi-byte characters other than identifiers stay plain.
                i += css[i..].chars().next().map_or(1, char::len_utf8);
                if c.is_ascii_punctuation() && c != b'.' {
                    out.push((start..i, Tok::Punct));
                }
            }
        }
    }
    out
}

/// `color-mix(in oklab, a p%, b)` on linear colours.
fn mix_oklab(a: Color, b: Color, p: f32) -> Color {
    let lab = |c: Color| {
        let l = (0.412_221_46 * c.r + 0.536_332_55 * c.g + 0.051_445_995 * c.b).cbrt();
        let m = (0.211_903_5 * c.r + 0.680_699_5 * c.g + 0.107_396_96 * c.b).cbrt();
        let s = (0.088_302_46 * c.r + 0.281_718_85 * c.g + 0.629_978_7 * c.b).cbrt();
        [l, m, s]
    };
    let (x, y, t) = (lab(a), lab(b), p / 100.0);
    let [l, m, s] = [0, 1, 2].map(|k| (x[k] * t + y[k] * (1.0 - t)).powi(3));
    Color {
        r: (4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s).clamp(0.0, 1.0),
        g: (-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s).clamp(0.0, 1.0),
        b: (-0.0041960863 * l - 0.703_418_6 * m + 1.707_614_7 * s).clamp(0.0, 1.0),
        a: a.a * t + b.a * (1.0 - t),
    }
}

/// `TOKEN_COLORS`, mixed into the text colour on light themes (`LIGHT_TOKEN_SHARE`).
fn colour(t: Tok, p: &Palette) -> Color {
    let (c, share) = match t {
        Tok::Comment => (p.text_tertiary, 65.0),
        Tok::Property => (p.text_link, 55.0),
        Tok::Keyword => (p.accent_purple, 65.0),
        Tok::Tag => (p.accent_success, 45.0),
        Tok::Selector => (p.accent_warning, 30.0),
        Tok::Number => (p.accent_success, 45.0),
        Tok::Str => (p.accent_danger, 65.0),
        Tok::Punct => (p.text_secondary, 75.0),
    };
    if crate::ui::is_light(p) { mix_oklab(c, p.text_primary, share) } else { c }
}

/// The editor's decorations.
pub fn spans(css: &str, p: &Palette) -> Vec<Span> {
    tokens(css)
        .into_iter()
        .map(|(r, t)| {
            let s = Span::new(r.start, r.end).color(colour(t, p));
            if t == Tok::Comment { s.italic() } else { s }
        })
        .collect()
}

// ---- completions ----

/// An open completion list: where the typed name starts, the names, and whether picking one
/// adds `: ` (a declaration rather than `var(…)`).
#[derive(Clone, Debug, PartialEq)]
pub struct List {
    pub from: usize,
    pub items: Vec<String>,
    pub decl: bool,
}

/// The highlighted row, and the name start the list was dismissed at (Escape).
#[derive(Clone, Copy, Debug, Default)]
pub struct Pick {
    pub selected: usize,
    pub dismissed: Option<usize>,
}

thread_local! {
    /// Row state per editor: the in-tab one (0) and the window's (1).
    pub static PICK: [Cell<Pick>; 2] = [Cell::new(Pick::default()), Cell::new(Pick::default())];
    /// The in-tab editor's caret `[x, y, w, h]`, read by the root view.
    pub static CARET: Cell<Option<[f32; 4]>> = const { Cell::new(None) };
}

/// `variableSource`: inside `var(` or at a declaration start (`{` or `;` then `--`).
pub fn list(e: &TextEdit, which: usize) -> Option<List> {
    let (a, caret) = e.range();
    if !e.focused || a != caret {
        return None;
    }
    let before = &e.text[..caret];
    let token_len = before.bytes().rev().take_while(|&c| ident_char(c)).count();
    let token = &before[caret - token_len..];
    let head = before[..caret - token_len].trim_end_matches([' ', '\t']);
    let decl = if head.ends_with("var(") {
        false
    } else if token.starts_with("--") && (head.is_empty() || head.ends_with(['{', ';', '\n'])) {
        let line_start = before.rfind('\n').map_or(0, |n| n + 1);
        // `DECLARATION_START` is per line: a `--` at the line start, or after `{`/`;` on it.
        if !(head.len() <= line_start || head.ends_with(['{', ';'])) {
            return None;
        }
        true
    } else {
        return None;
    };
    let from = caret - token_len;
    if PICK.with(|p| p[which].get().dismissed) == Some(from) {
        return None;
    }
    let mut names: Vec<String> = crate::theme::NAMES.iter().chain(&crate::theme::custom::FONTS).map(|s| s.to_string()).collect();
    // Variables the CSS declares itself (`getFileVariables`).
    for (n, _) in e.text.match_indices("--") {
        let end = ident_end(e.text.as_bytes(), n);
        let name = &e.text[n..end];
        if n != from && e.text[end..].trim_start().starts_with(':') && !names.iter().any(|x| x == name) {
            names.push(name.to_string());
        }
    }
    let q = token.to_lowercase();
    let rank = |n: &str| {
        if n.starts_with(&q) {
            Some(0)
        } else if n.contains(&q) {
            Some(1)
        } else {
            let mut it = n.chars();
            q.chars().all(|c| it.any(|x| x == c)).then_some(2)
        }
    };
    let mut items: Vec<(u8, String)> = names.into_iter().filter_map(|n| Some((rank(&n)?, n))).collect();
    items.sort();
    let items: Vec<String> = items.into_iter().map(|(_, n)| n).filter(|n| *n != token).collect();
    (!items.is_empty()).then_some(List { from, items, decl })
}

/// Puts the picked name in.
pub fn accept(e: &mut TextEdit, l: &List, i: usize) {
    let Some(name) = l.items.get(i) else { return };
    let caret = e.range().1;
    e.set_selection(l.from, caret);
    e.insert(&if l.decl { format!("{name}: ") } else { name.clone() });
}

/// Arrow keys, Enter/Tab and Escape while the list is open; `true` when handled.
pub fn key(e: &mut TextEdit, which: usize, k: &rune::KeyEvent) -> bool {
    let Some(l) = list(e, which) else { return false };
    let n = l.items.len();
    PICK.with(|p| {
        let mut pk = p[which].get();
        pk.selected = pk.selected.min(n - 1);
        let handled = match k.key {
            Key::Down => {
                pk.selected = (pk.selected + 1) % n;
                true
            }
            Key::Up => {
                pk.selected = (pk.selected + n - 1) % n;
                true
            }
            Key::Enter | Key::Tab if !k.mods.shift => {
                accept(e, &l, pk.selected);
                pk.selected = 0;
                true
            }
            Key::Escape => {
                pk.dismissed = Some(l.from);
                true
            }
            _ => {
                pk.selected = 0;
                false
            }
        };
        p[which].set(pk);
        handled
    })
}

/// The list (`.cm-tooltip-autocomplete`), to anchor under the caret; rows pick on press.
pub fn popup<S: 'static>(l: &List, which: usize, p: &Palette, edit: impl Fn(&mut S, &mut dyn FnMut(&mut TextEdit)) + Clone + 'static) -> El {
    let selected = PICK.with(|pk| pk[which].get().selected).min(l.items.len() - 1);
    let mut rows = col().w_full();
    for (i, name) in l.items.iter().take(100).enumerate() {
        let on = i == selected;
        let (edit, l) = (edit.clone(), l.clone());
        rows = rows.child(
            row()
                .w_full()
                .px(3.0)
                .py(1.0)
                .fill(if on { p.brand_primary } else { Color::TRANSPARENT })
                .cursor(rune::Cursor::Pointer)
                .on_press(move |s: &mut S| edit(s, &mut |e| accept(e, &l, i)))
                .child(text(name.clone()).font(font(13.0, 400.0).mono().lh(16.0)).color(if on { p.brand_primary_fill } else { p.text_primary })),
        );
    }
    col()
        .min_w(250.0)
        .max_h(10.0 * 16.0)
        .radius(6.0)
        .clip_rounded(6.0)
        .fill(p.background_primary)
        .border(1.0, p.background_modifier_accent)
        .box_shadow(rune::Shadow { x: 0.0, y: 8.0, blur: 24.0, spread: 0.0, color: crate::ui::rgba(0, 0, 0, 0.28), inset: false })
        .child(crate::ui::scroller(rows.scroll(rune::ScrollSpec { stick_bottom: false }), false, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(css: &str) -> Vec<(&str, Tok)> {
        tokens(css).into_iter().map(|(r, t)| (&css[r], t)).collect()
    }

    #[test]
    fn colours_like_lezer_css() {
        let k = kinds(":root { --text-link: #00a8fc; } /* c */ a.b:hover > p { color: var(--x) !important; margin: -2px 1.5em }");
        let has = |s: &str, t: Tok| k.contains(&(s, t));
        assert!(has(":", Tok::Punct) && has("root", Tok::Selector));
        assert!(has("--text-link", Tok::Property) && has("#00a8fc", Tok::Str));
        assert!(has("/* c */", Tok::Comment));
        assert!(has("a", Tok::Tag) && has("b", Tok::Selector) && has("hover", Tok::Selector) && has(">", Tok::Punct));
        assert!(has("color", Tok::Property) && has("var", Tok::Keyword) && has("--x", Tok::Property));
        assert!(has("!important", Tok::Keyword) && has("-2px", Tok::Number) && has("1.5em", Tok::Number));
        let m = kinds("@media (max-width: 600px) { .a { display: none } }");
        assert!(m.contains(&("@media", Tok::Keyword)) && m.contains(&("max-width", Tok::Property)) && m.contains(&("600px", Tok::Number)));
        assert!(m.contains(&("a", Tok::Selector)) && m.contains(&("display", Tok::Property)) && m.contains(&("none", Tok::Number)));
        let n = kinds("a { color: red; &:hover { color: blue } }");
        assert!(n.contains(&("hover", Tok::Selector)), "a nested rule is a selector: {n:?}");
    }

    #[test]
    fn completes_variables() {
        let at = |s: &str| {
            let mut e = TextEdit::new(s);
            e.focused = true;
            e.set_caret(s.len());
            e
        };
        let l = list(&at(":root { --text-li"), 0).unwrap();
        assert!(l.decl && l.items[0] == "--text-link", "{:?}", &l.items[..3]);
        let l = list(&at("a { color: var(--brand-pr"), 0).unwrap();
        assert!(!l.decl && l.items[0].starts_with("--brand-primary"));
        assert!(list(&at("a { color: red"), 0).is_none());
        let mut e = at(":root { --text-li");
        let l = list(&e, 0).unwrap();
        accept(&mut e, &l, 0);
        assert_eq!(e.text, ":root { --text-link: ");
    }
}
