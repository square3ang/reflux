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

/// `dateStyle: medium` (+ `timeStyle: short`) as `Intl.DateTimeFormat` prints it: "Nov 7, 2026",
/// "07.11.2026", "2026年11月7日". Per locale: the date pattern, its short month names, how date
/// and time join, and the year offset (Thai counts Buddhist years). Generated from V8's ICU.
fn medium_parts(locale: &str) -> (&'static str, &'static [&'static str], &'static str, i32) {
    match locale {
        "ar" => ("{dd}‏/{MM}‏/{y}", &[], "{date}، {time}", 0),
        "bg" => ("{d}.{MM}.{y} г.", &[], "{date}, {time}", 0),
        "cs" => ("{d}. {M}. {y}", &[], "{date} {time}", 0),
        "da" => ("{d}. {MMM} {y}", &["jan.", "feb.", "mar.", "apr.", "maj", "jun.", "jul.", "aug.", "sep.", "okt.", "nov.", "dec."], "{date}, {time}", 0),
        "de" => ("{dd}.{MM}.{y}", &[], "{date}, {time}", 0),
        "el" => ("{d} {MMM} {y}", &["Ιαν", "Φεβ", "Μαρ", "Απρ", "Μαΐ", "Ιουν", "Ιουλ", "Αυγ", "Σεπ", "Οκτ", "Νοε", "Δεκ"], "{date}, {time}", 0),
        "en-GB" => ("{d} {MMM} {y}", &["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sept", "Oct", "Nov", "Dec"], "{date}, {time}", 0),
        "es-419" => ("{d} {MMM} {y}", &["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"], "{date}, {time}", 0),
        "es-ES" => ("{d} {MMM} {y}", &["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"], "{date}, {time}", 0),
        "fi" => ("{d}.{M}.{y}", &[], "{date} klo {time}", 0),
        "fr" => ("{d} {MMM} {y}", &["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."], "{date}, {time}", 0),
        "he" => ("{d} ב{MMM} {y}", &["ינו׳", "פבר׳", "מרץ", "אפר׳", "מאי", "יוני", "יולי", "אוג׳", "ספט׳", "אוק׳", "נוב׳", "דצמ׳"], "{date}, {time}", 0),
        "hi" => ("{d} {MMM} {y}", &["जन॰", "फ़र॰", "मार्च", "अप्रैल", "मई", "जून", "जुल॰", "अग॰", "सित॰", "अक्टू॰", "नव॰", "दिस॰"], "{date}, {time}", 0),
        "hr" => ("{d}. {MMM} {y}.", &["sij", "velj", "ožu", "tra", "svi", "lip", "srp", "kol", "ruj", "lis", "stu", "pro"], "{date} {time}", 0),
        "hu" => ("{y}. {MMM} {d}.", &["jan.", "febr.", "márc.", "ápr.", "máj.", "jún.", "júl.", "aug.", "szept.", "okt.", "nov.", "dec."], "{date} {time}", 0),
        "id" => ("{d} {MMM} {y}", &["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"], "{date}, {time}", 0),
        "it" => ("{d} {MMM} {y}", &["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"], "{date}, {time}", 0),
        "ja" => ("{y}/{MM}/{dd}", &[], "{date} {time}", 0),
        "ko" => ("{y}. {M}. {d}.", &[], "{date} {time}", 0),
        "lt" => ("{y}-{MM}-{dd}", &[], "{date} {time}", 0),
        "nl" => ("{d} {MMM} {y}", &["jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec"], "{date}, {time}", 0),
        "no" => ("{d}. {MMM} {y}", &["jan.", "feb.", "mars", "apr.", "mai", "juni", "juli", "aug.", "sep.", "okt.", "nov.", "des."], "{date}, {time}", 0),
        "pl" => ("{d} {MMM} {y}", &["sty", "lut", "mar", "kwi", "maj", "cze", "lip", "sie", "wrz", "paź", "lis", "gru"], "{date}, {time}", 0),
        "pt-BR" => ("{d} de {MMM} de {y}", &["jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez."], "{date}, {time}", 0),
        "ro" => ("{d} {MMM} {y}", &["ian.", "feb.", "mar.", "apr.", "mai", "iun.", "iul.", "aug.", "sept.", "oct.", "nov.", "dec."], "{date}, {time}", 0),
        "ru" => ("{d} {MMM} {y} г.", &["янв.", "февр.", "мар.", "апр.", "мая", "июн.", "июл.", "авг.", "сент.", "окт.", "нояб.", "дек."], "{date}, {time}", 0),
        "sv-SE" => ("{d} {MMM} {y}", &["jan.", "feb.", "mars", "apr.", "maj", "juni", "juli", "aug.", "sep.", "okt.", "nov.", "dec."], "{date} {time}", 0),
        "th" => ("{d} {MMM} {y}", &["ม.ค.", "ก.พ.", "มี.ค.", "เม.ย.", "พ.ค.", "มิ.ย.", "ก.ค.", "ส.ค.", "ก.ย.", "ต.ค.", "พ.ย.", "ธ.ค."], "{date} {time}", 543),
        "tr" => ("{d} {MMM} {y}", &["Oca", "Şub", "Mar", "Nis", "May", "Haz", "Tem", "Ağu", "Eyl", "Eki", "Kas", "Ara"], "{date} {time}", 0),
        "uk" => ("{d} {MMM} {y} р.", &["січ.", "лют.", "бер.", "квіт.", "трав.", "черв.", "лип.", "серп.", "вер.", "жовт.", "лист.", "груд."], "{date}, {time}", 0),
        "vi" => ("{d} {MMM}, {y}", &["thg 1", "thg 2", "thg 3", "thg 4", "thg 5", "thg 6", "thg 7", "thg 8", "thg 9", "thg 10", "thg 11", "thg 12"], "{time} {date}", 0),
        "zh-CN" => ("{y}年{M}月{d}日", &[], "{date} {time}", 0),
        "zh-TW" => ("{y}年{M}月{d}日", &[], "{date} {time}", 0),
        "en-US" => ("{MMM} {d}, {y}", &["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"], "{date}, {time}", 0),
        _ => ("{MMM} {d}, {y}", &["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"], "{date}, {time}", 0),
    }
}

/// `dateStyle: medium`.
pub fn medium_date(d: chrono::NaiveDate, locale: &str) -> String {
    use chrono::Datelike;
    let (pat, months, _, years) = medium_parts(locale);
    let m = d.month0() as usize;
    pat.replace("{dd}", &format!("{:02}", d.day()))
        .replace("{d}", &d.day().to_string())
        .replace("{MMM}", months.get(m).copied().unwrap_or_default())
        .replace("{MM}", &format!("{:02}", m + 1))
        .replace("{M}", &(m + 1).to_string())
        .replace("{y}", &(d.year() + years).to_string())
}

/// `dateStyle: medium, timeStyle: short`, the time from [`time`].
pub fn medium_date_time<Tz: TimeZone>(d: &DateTime<Tz>, locale: &str) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let (_, _, join, _) = medium_parts(locale);
    join.replace("{date}", &medium_date(d.date_naive(), locale)).replace("{time}", &time(d, locale))
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
        assert_eq!(medium_date_time(&d, "en-US"), "Nov 6, 2026, 3:04 PM");
        assert_eq!(medium_date_time(&d, "de"), "06.11.2026, 15:04");
        assert_eq!(medium_date_time(&d, "vi"), "15:04 6 thg 11, 2026");
        assert_eq!(medium_date(d.date_naive(), "th"), "6 พ.ย. 2569");
        assert_eq!(medium_date(d.date_naive(), "zh-CN"), "2026年11月6日");
        set_format(2);
        assert_eq!(time(&d, "en-US"), "15:04");
        set_format(1);
        assert_eq!(time(&d, "de"), "3:04 PM");
        set_format(0);
    }
}
