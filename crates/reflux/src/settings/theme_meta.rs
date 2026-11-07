//! A theme's metadata in its CSS header comment (`ThemeCssUtils.parseThemeMetadata`,
//! `upsertThemeCssHeader`): `@name`, `@description`, `@author`, `@version`, `@tags`, or
//! `key: value` lines, in the first `/* … */` of the file.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub tags: Vec<String>,
}

/// `getFallbackThemeName`: the file name without `.css`, `-`/`_` as spaces, first letter upper.
pub fn fallback_name(file: &str) -> String {
    let base = file.len().checked_sub(4).filter(|&i| file.is_char_boundary(i) && file[i..].eq_ignore_ascii_case(".css")).map_or(file, |i| &file[..i]);
    let spaced = base.replace(['-', '_'], " ");
    let words = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut c = words.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// `parseTags` / `parseTagInput`.
pub fn tags(v: &str) -> Vec<String> {
    v.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

/// `HEADER_COMMENT_PATTERN`: the leading comment's byte range and its inside.
fn header(css: &str) -> Option<(std::ops::Range<usize>, &str)> {
    let start = css.len() - css.trim_start().len();
    let body = css[start..].strip_prefix("/*")?;
    let end = body.find("*/")?;
    Some((0..start + 2 + end + 2, &body[..end]))
}

fn key(k: &str) -> Option<&'static str> {
    let n: String = k.trim().to_lowercase().chars().filter(|c| !matches!(c, ' ' | '_' | '-' | '\t')).collect();
    Some(match n.as_str() {
        "name" | "title" => "name",
        "description" | "desc" | "summary" => "description",
        "author" | "authors" => "author",
        "version" | "ver" => "version",
        "tags" | "tag" => "tags",
        _ => return None,
    })
}

/// One header line as `(key, value)`: `@key value`, `key: value` or `key = value`.
fn line(l: &str) -> Option<(&str, &str)> {
    let l = l.trim_start();
    let l = l.strip_prefix('*').map_or(l, |r| r.strip_prefix(' ').unwrap_or(r)).trim();
    let ident = |s: &str, spaces: bool| {
        let mut it = s.char_indices();
        let first = it.next().is_some_and(|(_, c)| c.is_ascii_alphabetic());
        let end = it.find(|&(_, c)| !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || (spaces && c == ' '))).map_or(s.len(), |(i, _)| i);
        first.then_some(end)
    };
    // `METADATA_LINE_PATTERN` first, then `AT_RULE_METADATA_PATTERN`.
    let at = l.strip_prefix('@');
    let (s, spaces) = match at {
        Some(r) => (r, false),
        None => (l, true),
    };
    if let Some(end) = ident(s, spaces) {
        let rest = s[end..].trim_start();
        if let Some(v) = rest.strip_prefix([':', '=']) {
            return Some((s[..end].trim_end(), v.trim()));
        }
        if at.is_some() && (rest.len() < s[end..].len() || rest.is_empty()) {
            return Some((&s[..end], rest.trim()));
        }
    }
    None
}

pub fn parse(css: &str, fallback: &str) -> Meta {
    let mut m = Meta { name: fallback.to_string(), ..Default::default() };
    let Some((_, inner)) = header(css) else { return m };
    for raw in inner.lines() {
        let Some((k, v)) = line(raw) else { continue };
        match key(k) {
            Some("name") => m.name = v.into(),
            Some("description") => m.description = v.into(),
            Some("author") => m.author = v.into(),
            Some("version") => m.version = v.into(),
            Some("tags") => m.tags = tags(v),
            _ => {}
        }
    }
    m
}

/// `buildThemeCssHeader` in place of the header comment, or before the CSS.
pub fn upsert(css: &str, m: &Meta) -> String {
    let mut lines = vec!["/**".to_string(), format!(" * @name {}", if m.name.is_empty() { "Untitled theme" } else { &m.name })];
    for (k, v) in [("description", &m.description), ("author", &m.author), ("version", &m.version)] {
        if !v.trim().is_empty() {
            lines.push(format!(" * @{k} {}", v.trim()));
        }
    }
    if !m.tags.is_empty() {
        lines.push(format!(" * @tags {}", m.tags.join(", ")));
    }
    lines.push(" */".into());
    let head = lines.join("\n");
    match header(css) {
        Some((r, _)) => format!("{head}{}", &css[r.end..]),
        None => format!("{head}\n\n{css}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_the_header() {
        assert_eq!(fallback_name("my_dark-theme.CSS"), "My dark theme");
        let css = "/**\n * @name Midnight\n * @author Ada\n * Tags: dark, blue \n * version = 1.2\n */\n:root{}";
        let m = parse(css, "x");
        assert_eq!((m.name.as_str(), m.author.as_str(), m.version.as_str()), ("Midnight", "Ada", "1.2"));
        assert_eq!(m.tags, ["dark", "blue"]);
        assert_eq!(parse(":root{}", "Fallback").name, "Fallback");
        let m2 = Meta { name: "New".into(), description: "d".into(), tags: vec!["a".into()], ..Default::default() };
        assert_eq!(upsert(css, &m2), "/**\n * @name New\n * @description d\n * @tags a\n */\n:root{}");
        assert_eq!(upsert(":root{}", &m2), "/**\n * @name New\n * @description d\n * @tags a\n */\n\n:root{}");
        assert_eq!(parse(&upsert(":root{}", &m2), "x"), m2);
    }
}
