//! Code block and text preview highlighting (`features/code_highlighting`). Fluxer runs
//! Arborium (tree-sitter) with the GitHub Dark / Light themes; this uses syntect's bundled
//! Sublime grammars and maps their scopes onto the same theme colours.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock};

use rune::{Color, Span};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxSet};

use crate::ui::rgb;

/// Arborium's GitHub themes (`--arb-*-dark` / `-light`): keyword, string, comment, function,
/// constant, type, tag. Text without a token keeps the block's own colour.
const DARK: [u32; 7] = [0xff7b72, 0xa5d6ff, 0x8b949e, 0xd2a8ff, 0x79c0ff, 0xffa657, 0x7ee787];
const LIGHT: [u32; 7] = [0xcf222e, 0x0a3069, 0x6e7781, 0x8250df, 0x0550ae, 0x953800, 0x116329];

/// Which theme colour a scope takes, most specific prefixes first.
const RULES: [(&str, usize); 22] = [
    ("comment", 2),
    ("punctuation.definition.comment", 2),
    ("string.regexp", 1),
    ("string", 1),
    ("constant.character.escape", 1),
    ("constant", 4),
    ("support.constant", 4),
    ("variable.other.constant", 4),
    ("entity.name.function", 3),
    ("support.function", 3),
    ("variable.function", 3),
    ("meta.function-call", 3),
    ("entity.name.tag", 6),
    ("entity.other.attribute-name", 4),
    ("entity.name", 5),
    ("support.type", 5),
    ("support.class", 5),
    ("variable.parameter", 5),
    ("storage.type", 0),
    ("storage", 0),
    ("keyword", 0),
    ("variable.language", 4),
];

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The theme slot of the innermost scope a rule covers.
fn slot(stack: &ScopeStack) -> Option<usize> {
    static PARSED: OnceLock<Vec<(Scope, usize)>> = OnceLock::new();
    let rules = PARSED.get_or_init(|| RULES.iter().filter_map(|(s, i)| Some((Scope::new(s).ok()?, *i))).collect());
    stack.as_slice().iter().rev().find_map(|s| rules.iter().find(|(r, _)| r.is_prefix_of(*s)).map(|r| r.1))
}

/// Token ranges (byte offsets into `code`) and their theme slots, for a language name or file
/// extension (`rust`, `rs`, `js`, `py`…). `None` when the language isn't known.
fn tokens(lang: &str, code: &str) -> Option<Vec<(Range<usize>, usize)>> {
    let set = syntaxes();
    let syntax = set.find_syntax_by_token(lang.trim()).filter(|s| s.name != "Plain Text")?;
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut out: Vec<(Range<usize>, usize)> = Vec::new();
    let mut at = 0;
    for line in code.split_inclusive('\n') {
        let ops = state.parse_line(line, set).ok()?;
        let mut from = 0;
        for (i, op) in ops.into_iter().chain([(line.len(), syntect::parsing::ScopeStackOp::Noop)]) {
            if i > from
                && let Some(k) = slot(&stack)
            {
                match out.last_mut() {
                    Some((r, last)) if *last == k && r.end == at + from => r.end = at + i,
                    _ => out.push((at + from..at + i, k)),
                }
            }
            from = i;
            stack.apply(&op).ok()?;
        }
        at += line.len();
    }
    Some(out)
}

type Cached = Arc<Option<Vec<(Range<usize>, usize)>>>;

/// Colour spans for `code` in `lang` on a dark or light theme; cached, since views re-run.
pub fn spans(lang: &str, code: &str, light: bool) -> Vec<Span> {
    static CACHE: OnceLock<Mutex<HashMap<(String, String), Cached>>> = OnceLock::new();
    if lang.is_empty() || code.len() > 64 << 10 {
        return Vec::new();
    }
    let key = (lang.to_ascii_lowercase(), code.to_owned());
    let cache = CACHE.get_or_init(Default::default);
    let hit = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned();
    let toks = hit.unwrap_or_else(|| {
        let t: Cached = Arc::new(tokens(&key.0, code));
        let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
        if c.len() >= 256 {
            c.clear();
        }
        c.insert(key, t.clone());
        t
    });
    let theme = if light { LIGHT } else { DARK };
    let color = |i: usize| -> Color {
        let c = theme[i];
        rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
    };
    toks.iter().flatten().map(|(r, k)| Span::new(r.start, r.end).color(color(*k))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_tokens_take_the_github_slots() {
        let code = "fn main() {\n    let s = \"hi\"; // note\n}\n";
        let t = tokens("rs", code).expect("rust is bundled");
        let of = |needle: &str| t.iter().find(|(r, _)| &code[r.clone()] == needle).map(|t| t.1);
        assert_eq!(of("fn"), Some(0), "keyword");
        assert_eq!(of("main"), Some(3), "function");
        assert!(t.iter().any(|(r, k)| *k == 1 && code[r.clone()].contains("hi")), "string");
        assert!(t.iter().any(|(r, k)| *k == 2 && code[r.clone()].contains("note")), "comment");
        assert!(tokens("no-such-language", code).is_none());
        assert_eq!(spans("rust", code, false).len(), spans("rust", code, true).len());
    }
}
