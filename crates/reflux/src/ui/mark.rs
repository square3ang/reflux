//! Settings search highlight (`::highlight(settings-search-highlight)` over
//! `createRangesForSection`): while the search results render their inline tabs, the shared text
//! helpers paint every case-insensitive occurrence of the query.
// shortcut: Rune keeps the element tree private, so text is marked where it's built: the shared
// helpers and every tab file's `text()` (imported as [`marked`]). `rich()` runs (texts with an
// inline link) aren't.

use std::cell::RefCell;

use rune::{El, TextHighlight};

thread_local! {
    static QUERY: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Marks `q` (lowercased) until [`clear`].
pub fn set(q: &str) {
    QUERY.with(|c| *c.borrow_mut() = q.trim().to_lowercase());
}

pub fn clear() {
    QUERY.with(|c| c.borrow_mut().clear());
}

/// Byte ranges of `q` in `s`, case-insensitively (texts whose lowercase changes byte length are
/// skipped).
fn ranges(s: &str, q: &str) -> Vec<std::ops::Range<usize>> {
    let lower = s.to_lowercase();
    if q.is_empty() || lower.len() != s.len() {
        return Vec::new();
    }
    lower.match_indices(q).map(|(i, m)| i..i + m.len()).collect()
}

/// `e` (a text element showing `s`) with the query's occurrences highlighted.
pub fn text(e: El, s: &str) -> El {
    let r = QUERY.with(|c| ranges(s, &c.borrow()));
    if r.is_empty() {
        return e;
    }
    // `rgba(255, 230, 0, 0.4)`, radius 0.125rem.
    let bg = super::rgba(255, 230, 0, 0.4);
    e.text_highlights(r.into_iter().map(|r| TextHighlight::new(r, bg).pad(0.0)))
}

/// `rune::text(s)`, marked.
pub fn marked(s: impl Into<String>) -> El {
    let s = s.into();
    text(rune::text(s.clone()), &s)
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_every_case_insensitive_occurrence() {
        assert_eq!(super::ranges("Theme and THEMES", "theme"), vec![0..5, 10..15]);
        assert!(super::ranges("Theme", "").is_empty());
    }
}
