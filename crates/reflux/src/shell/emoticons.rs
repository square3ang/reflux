//! `convertEmoticonsToEmoji` (`EmoticonConversionUtils.ts`): an emoticon from Fluxer's shortcut
//! table becomes its emoji when it starts the text or follows whitespace and ends the text or is
//! followed by a space. Code spans (matching backtick runs), markdown links and URLs are left
//! alone.

/// Replaces emoticons in an outgoing message.
pub fn convert(text: &str) -> String {
    let table = super::emoji::shortcuts();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(end) = protected_end(text, i) {
            out.push_str(&text[i..end]);
            i = end;
            continue;
        }
        let lead = text[..i].chars().next_back().is_none_or(char::is_whitespace);
        let hit = lead.then(|| table.iter().find(|(k, _)| rest.starts_with(k.as_str()))).flatten();
        if let Some((k, glyph)) = hit
            && matches!(rest[k.len()..].chars().next(), None | Some(' '))
        {
            out.push_str(glyph);
            i += k.len();
            continue;
        }
        let c = rest.chars().next().unwrap_or_default();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// The end of a code span, markdown link or URL starting at `i` (`findProtectedSpanEnd`).
fn protected_end(text: &str, i: usize) -> Option<usize> {
    let rest = &text[i..];
    let run = rest.bytes().take_while(|&b| b == b'`').count();
    if run > 0 {
        // The next run of exactly the same length closes it.
        let mut j = i + run;
        while j < text.len() {
            let n = text[j..].bytes().take_while(|&b| b == b'`').count();
            if n == run {
                return Some(j + n);
            }
            j += n.max(1);
            while !text.is_char_boundary(j) {
                j += 1;
            }
        }
        return None;
    }
    if let Some(end) = link_end(text, i) {
        return Some(end);
    }
    if rest.starts_with("http://") || rest.starts_with("https://") {
        return Some(i + rest.find(char::is_whitespace).unwrap_or(rest.len()));
    }
    None
}

fn escaped(text: &str, i: usize) -> bool {
    text[..i].bytes().rev().take_while(|&b| b == b'\\').count() % 2 == 1
}

/// The first unescaped `close` at depth 0 after `open_at` (`findClosingBracket` /
/// `findMarkdownDestinationEnd`).
fn closing(text: &str, open_at: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0;
    for (j, b) in text.bytes().enumerate().skip(open_at + 1) {
        if escaped(text, j) {
            continue;
        }
        if b == open {
            depth += 1;
        } else if b == close {
            if depth == 0 {
                return Some(j);
            }
            depth -= 1;
        }
    }
    None
}

/// `[text](dest)` or `![text](dest)` (`findMarkdownLinkEnd`).
fn link_end(text: &str, i: usize) -> Option<usize> {
    let b = text.as_bytes();
    let start = if b.get(i) == Some(&b'!') && b.get(i + 1) == Some(&b'[') { i + 1 } else { i };
    if b.get(start) != Some(&b'[') {
        return None;
    }
    let close = closing(text, start, b'[', b']')?;
    if b.get(close + 1) != Some(&b'(') {
        return None;
    }
    closing(text, close + 1, b'(', b')').map(|j| j + 1)
}

#[cfg(test)]
mod tests {
    use super::convert;

    #[test]
    fn converts_like_fluxer() {
        assert_eq!(convert("hi :) <3"), "hi 🙂 ❤️");
        assert_eq!(convert(":D\nok"), ":D\nok", "a newline isn't a trailing boundary");
        assert_eq!(convert("a\n:D"), "a\n😄", "but it is a leading one");
        assert_eq!(convert("a:)b `:)` ``x :) `` :-)"), "a:)b `:)` ``x :) `` 🙂");
        assert_eq!(convert("[:)](https://x.y/:)) https://a.b/ :)"), "[:)](https://x.y/:)) https://a.b/ 🙂");
        assert_eq!(convert(">:( ;-) :'("), "😠 😉 😢");
        assert_eq!(convert("héllo :P"), "héllo 😛");
    }
}
