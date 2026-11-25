//! Settings search highlight (`::highlight(settings-search-highlight)` over
//! `createRangesForSection`): while search results render their inline tabs, `El::highlight_texts`
//! paints every case-insensitive occurrence of the query across `text()` and `rich()` runs alike.

use rune::{El, TextHighlight};

/// Byte ranges of `q` (already lowercased) in `s`, case-insensitively (texts whose lowercase
/// changes byte length are skipped).
fn ranges(s: &str, q: &str) -> Vec<std::ops::Range<usize>> {
    let lower = s.to_lowercase();
    if q.is_empty() || lower.len() != s.len() {
        return Vec::new();
    }
    lower.match_indices(q).map(|(i, m)| i..i + m.len()).collect()
}

/// Highlights every case-insensitive occurrence of `query` in every text element of `tree`.
pub fn highlight(tree: El, query: &str) -> El {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return tree;
    }
    // `rgba(255, 230, 0, 0.4)`, radius 0.125rem.
    let bg = super::rgba(255, 230, 0, 0.4);
    tree.highlight_texts(move |s| ranges(s, &q).into_iter().map(|r| TextHighlight::new(r, bg).pad(0.0)).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_every_case_insensitive_occurrence() {
        assert_eq!(super::ranges("Theme and THEMES", "theme"), vec![0..5, 10..15]);
        assert!(super::ranges("Theme", "").is_empty());
    }
}
