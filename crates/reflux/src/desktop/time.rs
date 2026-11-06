//! Message timestamps in the interface language and the user's time format (`timeFormat`:
//! 0 auto, 1 12-hour, 2 24-hour; `LanguageTab.tsx`). Month, weekday and AM/PM names come from
//! chrono's glibc locale tables; the patterns follow what `Intl.DateTimeFormat` prints.

use std::sync::atomic::{AtomicU8, Ordering};

use chrono::{DateTime, Locale, TimeZone};

static FORMAT: AtomicU8 = AtomicU8::new(0);

/// The user setting, refreshed every frame from the store.
pub fn set_format(f: u8) {
    FORMAT.store(f, Ordering::Relaxed);
}

/// `Intl` locales whose default hour cycle is 12-hour.
const TWELVE_HOUR: &[&str] = &["en-US", "ko", "hi", "ar", "es-419", "el"];

pub fn twelve_hour(locale: &str) -> bool {
    match FORMAT.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => TWELVE_HOUR.contains(&locale),
    }
}

/// An interface language as a glibc locale.
fn glibc(code: &str) -> Locale {
    let name = match code {
        "en-US" => "en_US".into(),
        "en-GB" => "en_GB".into(),
        "es-419" => "es_MX".into(),
        "es-ES" => "es_ES".into(),
        "pt-BR" => "pt_BR".into(),
        "sv-SE" => "sv_SE".into(),
        "zh-CN" => "zh_CN".into(),
        "zh-TW" => "zh_TW".into(),
        "ar" => "ar_SA".into(),
        "cs" => "cs_CZ".into(),
        "da" => "da_DK".into(),
        "el" => "el_GR".into(),
        "he" => "he_IL".into(),
        "hi" => "hi_IN".into(),
        "ja" => "ja_JP".into(),
        "ko" => "ko_KR".into(),
        "no" => "nb_NO".into(),
        "uk" => "uk_UA".into(),
        "vi" => "vi_VN".into(),
        other => format!("{other}_{}", other.to_uppercase()),
    };
    Locale::try_from(name.as_str()).unwrap_or(Locale::en_US)
}

/// `hour: numeric, minute: 2-digit`: "3:04 PM", "오후 3:04", "15:04".
pub fn time<Tz: TimeZone>(d: &DateTime<Tz>, locale: &str) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let pat = match (twelve_hour(locale), locale) {
        (false, _) => "%H:%M",
        (true, "ko" | "ja" | "zh-CN" | "zh-TW") => "%p %-I:%M",
        (true, _) => "%-I:%M %p",
    };
    // Locales without AM/PM names (de, fr…) print English ones, as `Intl` does with `hour12`.
    let loc = glibc(locale);
    let am = d.format_localized("%p", loc).to_string();
    let loc = if am.is_empty() { Locale::en_US } else { loc };
    d.format_localized(pat, loc).to_string()
}

/// The numeric date of "11/6/2026, 3:04 PM".
pub fn short_date<Tz: TimeZone>(d: &DateTime<Tz>, locale: &str) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let pat = match locale {
        "en-US" => "%-m/%-d/%Y",
        "ja" | "zh-CN" | "zh-TW" => "%Y/%-m/%-d",
        "ko" => "%Y. %-m. %-d.",
        "hu" => "%Y. %m. %d.",
        "lt" | "sv-SE" => "%Y-%m-%d",
        "nl" => "%-d-%-m-%Y",
        "de" | "fi" | "no" | "da" => "%-d.%-m.%Y",
        "cs" => "%-d. %-m. %Y",
        "ru" | "uk" | "pl" | "ro" | "bg" | "tr" => "%d.%m.%Y",
        "hr" => "%d. %m. %Y.",
        _ => "%d/%m/%Y",
    };
    d.format(pat).to_string()
}

/// `weekday: long, year, month: long, day`: "Friday, November 6, 2026".
pub fn long_date<Tz: TimeZone>(d: &DateTime<Tz>, locale: &str) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let pat = match locale {
        "en-US" => "%A, %B %-d, %Y",
        "ja" | "zh-CN" | "zh-TW" => "%Y年%-m月%-d日%A",
        "ko" => "%Y년 %-m월 %-d일 %A",
        "hu" => "%Y. %B %-d., %A",
        "de" | "da" | "no" | "fi" | "cs" | "hr" => "%A, %-d. %B %Y",
        "es-ES" | "es-419" | "pt-BR" => "%A, %-d de %B de %Y",
        "en-GB" | "fr" | "it" | "nl" | "ro" | "id" | "el" | "he" | "ar" | "hi" | "th" | "vi" => "%A %-d %B %Y",
        _ => "%A, %-d %B %Y",
    };
    d.format_localized(pat, glibc(locale)).to_string()
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    #[test]
    fn follows_locale_and_setting() {
        let d = Utc.with_ymd_and_hms(2026, 11, 6, 15, 4, 0).unwrap();
        set_format(0);
        assert_eq!(time(&d, "en-US"), "3:04 PM");
        assert_eq!(time(&d, "de"), "15:04");
        assert_eq!(time(&d, "ko"), "오후 3:04");
        assert_eq!(short_date(&d, "en-US"), "11/6/2026");
        assert_eq!(short_date(&d, "de"), "6.11.2026");
        assert_eq!(long_date(&d, "en-US"), "Friday, November 6, 2026");
        assert_eq!(long_date(&d, "de"), "Freitag, 6. November 2026");
        assert_eq!(long_date(&d, "ko"), "2026년 11월 6일 금요일");
        set_format(2);
        assert_eq!(time(&d, "en-US"), "15:04");
        set_format(1);
        assert_eq!(time(&d, "de"), "3:04 PM");
        set_format(0);
    }
}
