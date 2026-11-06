//! Translations from Fluxer's own catalogs (lingui `.po`, keyed by the English source text).
//! `scripts/i18n-extract.py` copies the entries Reflux uses into `assets/i18n/<locale>.json`, so
//! `t("Welcome back")` is "다시 오신 것을 환영합니다" in Korean and the English text itself when a
//! locale has no entry. Placeholders are lingui's `{name}`; [`tf`] fills them.
// shortcut: only the login screens, settings chrome and the desktop tabs/tray/notifications call
// `t` so far. Still English: shell/*.rs (chat, sidebar, members, menus, friends, voice, profile),
// guild/*.rs, settings/tabs.rs bodies, ui/*, and app.rs/login.rs messages Fluxer has no msgid for
// (dynamic errors, "Signing in to {host}"). Wrap literals in `t(...)` and re-run
// scripts/i18n-extract.py when converting them.
// shortcut: ICU plurals (`{count, plural, ...}`) aren't interpreted; strings with them stay English.

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

/// [`t`] with lingui `{name}` placeholders filled.
pub fn tf(s: &str, args: &[(&str, &str)]) -> String {
    args.iter().fold(t(s).to_string(), |out, (k, v)| out.replace(&format!("{{{k}}}"), v))
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
}
