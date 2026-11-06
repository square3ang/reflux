//! Translations from Fluxer's own catalogs (lingui `.po`, keyed by the English source text).
//! `scripts/i18n-extract.py` copies the entries Reflux uses into `assets/i18n/<locale>.json`, so
//! `t("Welcome back")` is "다시 오신 것을 환영합니다" in Korean and the English text itself when a
//! locale has no entry. Placeholders are lingui's `{name}`; [`tf`] fills them.
// shortcut: UI literals go through `t` where they're drawn (option tables via `RadioGroup`, `Select`,
// settings nav and section headings), but sentences Reflux builds with `format!` and Fluxer has
// no msgid for (dynamic errors, "Delete '{name}'", "Signing in to {host}", some confirm bodies)
// stay English, as do the component gallery and the shell demo. Wrap new literals in `t(...)`
// and re-run scripts/i18n-extract.py.
// shortcut: Arabic and Hebrew get right-to-left text runs (parley's bidi) but the layout isn't
// mirrored: Rune has no layout-direction setting, so rows still run left to right.

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// `SUPPORTED_LOCALES` (`user/utils/LocaleUtils.ts`): (code, native name).
pub const LOCALES: &[(&str, &str)] = &[
    ("ar", "العربية"),
    ("bg", "Български"),
    ("cs", "Čeština"),
    ("da", "Dansk"),
    ("de", "Deutsch"),
    ("el", "Ελληνικά"),
    ("en-GB", "English"),
    ("en-US", "English (US)"),
    ("es-ES", "Español (España)"),
    ("es-419", "Español (Latinoamérica)"),
    ("fi", "Suomi"),
    ("fr", "Français"),
    ("he", "עברית"),
    ("hi", "हिन्दी"),
    ("hr", "Hrvatski"),
    ("hu", "Magyar"),
    ("id", "Bahasa Indonesia"),
    ("it", "Italiano"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("lt", "Lietuvių"),
    ("nl", "Nederlands"),
    ("no", "Norsk"),
    ("pl", "Polski"),
    ("pt-BR", "Português (Brasil)"),
    ("ro", "Română"),
    ("ru", "Русский"),
    ("sv-SE", "Svenska"),
    ("th", "ไทย"),
    ("tr", "Türkçe"),
    ("uk", "Українська"),
    ("vi", "Tiếng Việt"),
    ("zh-CN", "中文 (简体)"),
    ("zh-TW", "中文 (繁體)"),
];

macro_rules! catalogs {
    ($($code:literal),*) => {
        fn raw(code: &str) -> Option<&'static str> {
            match code {
                $($code => Some(include_str!(concat!("../../../assets/i18n/", $code, ".json"))),)*
                _ => None,
            }
        }
    };
}
catalogs!(
    "ar", "bg", "cs", "da", "de", "el", "en-GB", "es-ES", "es-419", "fi", "fr", "he", "hi", "hr", "hu", "id", "it", "ja",
    "ko", "lt", "nl", "no", "pl", "pt-BR", "ro", "ru", "sv-SE", "th", "tr", "uk", "vi", "zh-CN", "zh-TW"
);

type Catalog = HashMap<String, &'static str>;

struct Current {
    code: &'static str,
    /// `None` for English (the keys themselves).
    catalog: Option<&'static Catalog>,
}

static CURRENT: RwLock<Current> = RwLock::new(Current { code: "en-US", catalog: None });

/// Parses a catalog once; it lives for the rest of the process (each is a few KB).
fn load(code: &'static str) -> Option<&'static Catalog> {
    static LOADED: LazyLock<RwLock<HashMap<&'static str, &'static Catalog>>> = LazyLock::new(Default::default);
    if let Some(c) = LOADED.read().unwrap().get(code) {
        return Some(c);
    }
    let json = raw(code)?;
    let map: HashMap<String, String> = serde_json::from_str(json).unwrap_or_default();
    let cat: &'static Catalog = Box::leak(Box::new(map.into_iter().map(|(k, v)| (k, &*v.leak())).collect()));
    LOADED.write().unwrap().insert(code, cat);
    Some(cat)
}

/// A supported locale for a Fluxer locale or a POSIX one (`ko_KR.UTF-8` → `ko`, `pt_BR` → `pt-BR`).
pub fn resolve(tag: &str) -> Option<&'static str> {
    let tag = tag.split(['.', '@']).next()?.replace('_', "-");
    let lang = tag.split('-').next()?;
    let exact = LOCALES.iter().find(|(c, _)| c.eq_ignore_ascii_case(&tag));
    let by_lang = || match lang {
        "en" => Some("en-US"),
        "es" => Some("es-ES"),
        "pt" => Some("pt-BR"),
        "sv" => Some("sv-SE"),
        "zh" => Some("zh-CN"),
        "nb" | "nn" => Some("no"),
        _ => LOCALES.iter().find(|(c, _)| *c == lang).map(|(c, _)| *c),
    };
    exact.map(|(c, _)| *c).or_else(by_lang)
}

/// The system's language (`LC_ALL`, `LC_MESSAGES`, `LANG`), else English.
pub fn system() -> &'static str {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .and_then(|v| resolve(&v))
        .unwrap_or("en-US")
}

pub fn set_locale(tag: &str) {
    let code = resolve(tag).unwrap_or("en-US");
    *CURRENT.write().unwrap() = Current { code, catalog: load(code) };
}

pub fn locale() -> &'static str {
    CURRENT.read().unwrap().code
}

/// The current locale's text for an English source string.
pub fn t(s: &str) -> &str {
    CURRENT.read().unwrap().catalog.and_then(|c| c.get(s).copied()).unwrap_or(s)
}

/// [`t`] with lingui's ICU MessageFormat filled: `{name}` placeholders, `{n, plural, …}` (with
/// `=N`, the locale's CLDR categories and `#`), `{n, selectordinal, …}` and `{x, select, …}`.
pub fn tf(s: &str, args: &[(&str, &str)]) -> String {
    format(t(s), args, locale())
}

/// [`tf`] with `count` set (the usual name of the plural argument).
pub fn tp(s: &str, count: i64, args: &[(&str, &str)]) -> String {
    let n = count.to_string();
    let mut all = vec![("count", n.as_str())];
    all.extend_from_slice(args);
    tf(s, &all)
}

fn format(msg: &str, args: &[(&str, &str)], locale: &str) -> String {
    format_with(msg, args, locale, None)
}

/// `hash`: the number `#` stands for inside a plural branch.
fn format_with(msg: &str, args: &[(&str, &str)], locale: &str, hash: Option<&str>) -> String {
    let mut out = String::with_capacity(msg.len());
    let mut rest = msg;
    while let Some(i) = rest.find(['{', '#']) {
        out.push_str(&rest[..i]);
        if rest.as_bytes()[i] == b'#' {
            out.push_str(hash.unwrap_or("#"));
            rest = &rest[i + 1..];
            continue;
        }
        let Some(end) = matching(&rest[i..]) else {
            out.push_str(&rest[i..]);
            return out;
        };
        let inner = &rest[i + 1..i + end];
        rest = &rest[i + end + 1..];
        let mut parts = inner.splitn(3, ',');
        let name = parts.next().unwrap_or("").trim();
        let value = args.iter().find(|(k, _)| *k == name).map(|(_, v)| *v);
        match (parts.next().map(str::trim), parts.next()) {
            (Some(kind @ ("plural" | "selectordinal" | "select")), Some(options)) => {
                let branches = branches(options);
                let v = value.unwrap_or("");
                let pick = |key: &str| branches.iter().find(|(k, _)| *k == key).map(|(_, m)| *m);
                let chosen = if kind == "select" {
                    pick(v).or_else(|| pick("other"))
                } else {
                    let n: f64 = v.parse().unwrap_or(0.0);
                    let cat = if kind == "plural" { plural(locale, n) } else { "other" };
                    pick(&format!("={v}")).or_else(|| pick(cat)).or_else(|| pick("other"))
                };
                let h = (kind != "select").then_some(v);
                out.push_str(&format_with(chosen.unwrap_or(""), args, locale, h.or(hash)));
            }
            _ => match value {
                Some(v) => out.push_str(v),
                None => {
                    out.push('{');
                    out.push_str(inner);
                    out.push('}');
                }
            },
        }
    }
    out.push_str(rest);
    out
}

/// The length up to the `}` closing the `{` that `s` starts with.
fn matching(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// `key {message} key {message} …`.
fn branches(options: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = options;
    while let Some(open) = rest.find('{') {
        let key = rest[..open].trim();
        let Some(end) = matching(&rest[open..]) else { break };
        out.push((key, &rest[open + 1..open + end]));
        rest = &rest[open + end + 1..];
    }
    out
}

/// CLDR cardinal plural category for the supported locales (integers and simple decimals).
fn plural(locale: &str, n: f64) -> &'static str {
    let lang = locale.split('-').next().unwrap_or(locale);
    let int = n.fract() == 0.0;
    let i = n.trunc() as i64;
    let (m10, m100) = (i % 10, i % 100);
    match lang {
        "ja" | "ko" | "zh" | "th" | "vi" | "id" => "other",
        "fr" | "hi" if i == 0 || i == 1 => "one",
        "fr" | "hi" => "other",
        "pt" if i == 0 || i == 1 => "one",
        "pt" => "other",
        "ar" if int => match (i, m100) {
            (0, _) => "zero",
            (1, _) => "one",
            (2, _) => "two",
            (_, 3..=10) => "few",
            (_, 11..=99) => "many",
            _ => "other",
        },
        "he" if int && i == 1 => "one",
        "he" if int && i == 2 => "two",
        "ru" | "uk" if int => match (m10, m100) {
            (1, m) if m != 11 => "one",
            (2..=4, m) if !(12..=14).contains(&m) => "few",
            _ => "many",
        },
        "hr" if int => match (m10, m100) {
            (1, m) if m != 11 => "one",
            (2..=4, m) if !(12..=14).contains(&m) => "few",
            _ => "other",
        },
        "pl" if int => match (i, m10, m100) {
            (1, _, _) => "one",
            (_, 2..=4, m) if !(12..=14).contains(&m) => "few",
            _ => "many",
        },
        "cs" if int => match i {
            1 => "one",
            2..=4 => "few",
            _ => "other",
        },
        "cs" | "pl" | "ru" | "uk" => "many",
        "lt" if int => match (m10, m100) {
            (1, m) if !(11..=19).contains(&m) => "one",
            (2..=9, m) if !(11..=19).contains(&m) => "few",
            _ => "other",
        },
        "lt" => "many",
        "ro" if int => match (i, m100) {
            (1, _) => "one",
            (0, _) | (_, 2..=19) => "few",
            _ => "other",
        },
        "ro" => "few",
        _ if int && i == 1 => "one",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_posix_and_fluxer_tags() {
        assert_eq!(resolve("ko_KR.UTF-8"), Some("ko"));
        assert_eq!(resolve("pt_BR"), Some("pt-BR"));
        assert_eq!(resolve("en_GB.UTF-8"), Some("en-GB"));
        assert_eq!(resolve("es_MX"), Some("es-ES"));
        assert_eq!(resolve("es-419"), Some("es-419"));
        assert_eq!(resolve("xx"), None);
    }

    #[test]
    fn korean_catalog_translates_and_falls_back() {
        // The only test that switches the locale (it's global).
        set_locale("ko");
        assert_eq!(t("Welcome back"), "다시 오신 것을 환영합니다");
        assert_eq!(t("No such string in Fluxer"), "No such string in Fluxer");
        set_locale("en-US");
        assert_eq!(t("Welcome back"), "Welcome back");
    }

    #[test]
    fn icu_plural_select_and_hash() {
        let m = "{count, plural, one {{authorName} sent an attachment} other {{authorName} sent # attachments}}";
        assert_eq!(format(m, &[("count", "1"), ("authorName", "Ann")], "en-US"), "Ann sent an attachment");
        assert_eq!(format(m, &[("count", "3"), ("authorName", "Ann")], "en-US"), "Ann sent 3 attachments");
        let ru = "{n, plural, one {# файл} few {# файла} many {# файлов} other {# файла}}";
        assert_eq!(format(ru, &[("n", "21")], "ru"), "21 файл");
        assert_eq!(format(ru, &[("n", "23")], "ru"), "23 файла");
        assert_eq!(format(ru, &[("n", "11")], "ru"), "11 файлов");
        assert_eq!(format("{n, plural, =0 {none} other {#}}", &[("n", "0")], "en-US"), "none");
        assert_eq!(format("{g, select, f {she} m {he} other {they}}", &[("g", "x")], "en-US"), "they");
        assert_eq!(format("{n, plural, other {# 개}}", &[("n", "5")], "ko"), "5 개");
        assert_eq!(format("{a} and {b}", &[("a", "x")], "en-US"), "x and {b}");
        assert_eq!(plural("ar", 0.0), "zero");
        assert_eq!(plural("ar", 105.0), "few");
        assert_eq!(plural("pl", 22.0), "few");
        assert_eq!(plural("fr", 0.0), "one");
    }
}
