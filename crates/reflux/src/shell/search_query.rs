//! Fluxer's message search syntax (`search/utils/SearchQueryParser.ts`): `from:`, `mentions:`,
//! `in:`, `has:`, `before:`/`after:`/`during:`/`on:`, `pinned:` and friends, `-key:` to exclude,
//! and "quoted phrases", turned into a `POST /search/messages` body.
// shortcut: no search chips or confusable-skeleton matching.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone};
use reflux_model::{ChannelType, Snowflake};
use reflux_store::Store;
use serde_json::{Map, Value, json};

pub const KNOWN_KEYS: [&str; 26] = [
    "from", "mentions", "in", "before", "during", "on", "after", "has", "pinned", "author-type", "sort", "order", "mature",
    "embed-type", "embed-provider", "link", "filename", "ext", "last", "beforeid", "afterid", "any", "scope", "-from", "-in", "-has",
];

fn is_quote(c: char) -> bool {
    matches!(c, '"' | '\u{201c}' | '\u{201d}' | '\u{201f}' | '\u{2033}' | '\u{ab}' | '\u{bb}')
}

/// A `key:` at byte `i`: (key without `-`, excluded, where its value starts).
fn key_at(q: &str, i: usize) -> Option<(String, bool, usize)> {
    let rest = &q[i..];
    let colon = rest.find([':', ' '])?;
    if !rest[colon..].starts_with(':') || colon == 0 {
        return None;
    }
    let raw = rest[..colon].to_lowercase();
    let (key, exclude) = match raw.strip_prefix('-') {
        Some(k) => (k.to_string(), true),
        None => (raw.clone(), false),
    };
    (KNOWN_KEYS.contains(&raw.as_str()) || KNOWN_KEYS.contains(&key.as_str())).then_some((key, exclude, i + colon + 1))
}

/// `(key, excluded, value)` tokens and the free text (content, exact phrases).
pub fn tokenize(q: &str) -> (Vec<(String, bool, String)>, String, Vec<String>) {
    let mut tokens = Vec::new();
    let mut rest = String::new();
    let mut i = 0;
    while i < q.len() {
        let at_word = i == 0 || q[..i].ends_with(' ');
        if let Some((key, exclude, vs)) = at_word.then(|| key_at(q, i)).flatten() {
            let mut j = vs;
            while q[j..].starts_with(' ') {
                j += 1;
            }
            // A bare `key:` (at the end, or followed by another key) has no value.
            if j >= q.len() || (j > vs && key_at(q, j).is_some()) {
                i = vs;
                continue;
            }
            let (mut end, mut quoted, mut escaped) = (j, false, false);
            for (k, c) in q[j..].char_indices() {
                end = j + k;
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if is_quote(c) {
                    quoted = !quoted;
                } else if c == ' ' && !quoted {
                    break;
                }
                end = j + k + c.len_utf8();
            }
            tokens.push((key, exclude, q[j..end].to_string()));
            i = end;
            continue;
        }
        let c = q[i..].chars().next().unwrap_or(' ');
        rest.push(c);
        i += c.len_utf8();
    }
    // Quoted runs of the rest are exact phrases.
    let (mut content, mut phrases, mut chars) = (String::new(), Vec::new(), rest.chars());
    while let Some(c) = chars.next() {
        if is_quote(c) {
            let p: String = chars.by_ref().take_while(|&c| !is_quote(c)).collect();
            if !p.trim().is_empty() {
                phrases.push(p.trim().to_string());
            }
        } else {
            content.push(c);
        }
    }
    (tokens, content.split_whitespace().collect::<Vec<_>>().join(" "), phrases)
}

/// `splitCSV`: comma-separated, quotes allowed.
fn csv(v: &str) -> Vec<String> {
    v.split(',').map(|s| s.trim().trim_matches(is_quote).trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// `isSearchFilterValueChippable`: a value the filter can use. Keys with a fixed vocabulary or a
/// date / duration check it; names (users, channels) and free text always pass. Values that fail
/// show as unapplied (`SearchChipRole.UNAPPLIED_VALUE`).
pub fn applies(key: &str, value: &str, now: DateTime<Local>) -> bool {
    let v = value.trim();
    if v.is_empty() {
        return false;
    }
    let lower = v.to_lowercase();
    let every = |ok: &dyn Fn(&str) -> bool| {
        let items = csv(&lower);
        !items.is_empty() && items.iter().all(|i| ok(i))
    };
    let date = |d: &str| match d.split_once("..") {
        Some((a, b)) => period(a, now).is_some() && period(b, now).is_some(),
        None => period(d, now).is_some(),
    };
    match key.trim_start_matches('-') {
        "has" => every(&|i| ["image", "sound", "video", "file", "sticker", "embed", "link", "poll", "snapshot", "forward"].contains(&i)),
        "author-type" => every(&|i| ["user", "bot", "webhook"].contains(&i)),
        "embed-type" => every(&|i| ["image", "video", "sound", "article"].contains(&i)),
        "pinned" => ["true", "false", "yes", "no", "1", "0"].contains(&lower.as_str()),
        "mature" => ["true", "false"].contains(&lower.as_str()),
        "sort" => ["timestamp", "relevance"].contains(&lower.as_str()),
        "order" => ["asc", "desc"].contains(&lower.as_str()),
        "scope" => csv(&lower).iter().any(|s| ["current", "open_dms", "all_dms", "all_guilds", "all", "open_dms_and_all_guilds"].contains(&s.as_str())),
        "last" => duration_ms(v).is_some(),
        "before" | "after" | "during" | "on" => date(v),
        _ => true,
    }
}

/// What a parse resolves names against.
pub struct Ctx<'a> {
    pub store: &'a Store,
    pub guild: Option<Snowflake>,
    pub now: DateTime<Local>,
}

fn user(ctx: &Ctx, tag: &str) -> Option<Snowflake> {
    if let Ok(id) = tag.parse::<u64>() {
        return Some(Snowflake(id));
    }
    if tag.eq_ignore_ascii_case("@me") {
        return Some(ctx.store.my_id());
    }
    let (name, disc) = tag.split_once('#').unwrap_or((tag, ""));
    let me = ctx.store.me.as_ref().map(|m| &m.base);
    ctx.store.users.values().chain(me).find(|u| u.username.eq_ignore_ascii_case(name) && (disc.is_empty() || u.discriminator == disc)).map(|u| u.id)
}

fn channel(ctx: &Ctx, name: &str) -> Option<Snowflake> {
    let target = name.trim_start_matches('#').to_lowercase();
    let pick = |list: Vec<(Snowflake, String)>| {
        list.iter().find(|(_, n)| *n == target).or_else(|| list.iter().find(|(_, n)| n.contains(&target))).map(|(id, _)| *id)
    };
    match ctx.guild {
        Some(g) => {
            let mut list: Vec<_> = ctx.store.channels.values().filter(|c| c.guild_id == Some(g)).map(|c| (c.id, c.name.clone().unwrap_or_default().to_lowercase())).collect();
            list.sort_by_key(|c| c.0);
            pick(list)
        }
        None => {
            let me = ctx.store.my_id();
            let list = ctx
                .store
                .private_channels()
                .into_iter()
                .filter(|c| c.kind == ChannelType::DM)
                .filter_map(|c| c.recipients.iter().find(|u| u.id != me).map(|u| (c.id, u.username.to_lowercase())))
                .collect();
            pick(list)
        }
    }
}

const MONTHS: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
const WEEKDAYS: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

/// `Intl.Locale#getWeekInfo().firstDay` of each interface language.
fn first_day(locale: &str) -> chrono::Weekday {
    match locale {
        "ar" => chrono::Weekday::Sat,
        "en-US" | "he" | "hi" | "id" | "ja" | "ko" | "pt-BR" | "th" | "zh-TW" => chrono::Weekday::Sun,
        _ => chrono::Weekday::Mon,
    }
}

fn week_start(today: NaiveDate, first: chrono::Weekday) -> NaiveDate {
    today - Duration::days((today.weekday().num_days_from_monday() as i64 - first.num_days_from_monday() as i64).rem_euclid(7))
}

fn midnight(d: NaiveDate) -> Option<DateTime<Local>> {
    Local.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).earliest()
}

/// `resolveSearchDatePeriod`: [start, end) of a date word, a day or an instant (start == end).
/// Weeks start where luxon's `useLocaleWeeks` puts them for the system locale.
pub fn period(v: &str, now: DateTime<Local>) -> Option<(DateTime<Local>, DateTime<Local>)> {
    let w = v.trim().to_lowercase();
    let today = now.date_naive();
    let day = |d: NaiveDate| Some((midnight(d)?, midnight(d.succ_opt()?)?));
    let month = |y: i32, m: u32| {
        let a = NaiveDate::from_ymd_opt(y, m, 1)?;
        let b = if m == 12 { NaiveDate::from_ymd_opt(y + 1, 1, 1)? } else { NaiveDate::from_ymd_opt(y, m + 1, 1)? };
        Some((midnight(a)?, midnight(b)?))
    };
    let week_start = week_start(today, first_day(crate::i18n::system()));
    match w.as_str() {
        "now" => return Some((now, now)),
        "today" => return day(today),
        "yesterday" => return day(today.pred_opt()?),
        "week" => return Some((midnight(week_start)?, midnight(week_start + Duration::days(7))?)),
        "month" => return month(today.year(), today.month()),
        "year" => return Some((midnight(NaiveDate::from_ymd_opt(today.year(), 1, 1)?)?, midnight(NaiveDate::from_ymd_opt(today.year() + 1, 1, 1)?)?)),
        _ => {}
    }
    if let Some(m) = MONTHS.iter().position(|m| *m == w) {
        return month(today.year(), m as u32 + 1);
    }
    if let Some(d) = WEEKDAYS.iter().position(|d| *d == w) {
        let offset = (d as i64 - week_start.weekday().num_days_from_monday() as i64).rem_euclid(7);
        return day(week_start + Duration::days(offset));
    }
    if w.len() == 4
        && let Ok(y) = w.parse::<i32>()
    {
        return (2015..=today.year()).contains(&y).then(|| Some((midnight(NaiveDate::from_ymd_opt(y, 1, 1)?)?, midnight(NaiveDate::from_ymd_opt(y + 1, 1, 1)?)?))).flatten();
    }
    if let Ok(d) = NaiveDate::parse_from_str(&w, "%Y-%m-%d") {
        return day(d);
    }
    for f in ["%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M", "%Y-%m-%d_%H:%M", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(t) = chrono::NaiveDateTime::parse_from_str(&w, f) {
            let t = Local.from_local_datetime(&t).earliest()?;
            return Some((t, t));
        }
    }
    None
}

fn snowflake_at(t: DateTime<Local>) -> String {
    Snowflake::from_timestamp_ms(t.timestamp_millis().max(0) as u64).to_string()
}

/// `parseDuration`: `30m`, `2h`, `7d`, `1w`.
fn duration_ms(v: &str) -> Option<i64> {
    let v = v.trim().to_lowercase();
    let split = v.find(|c: char| !c.is_ascii_digit())?;
    let n: i64 = v[..split].parse().ok()?;
    let unit = match &v[split..] {
        "ms" => 1,
        "s" => 1000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        "w" => 7 * 86_400_000,
        _ => return None,
    };
    Some(n * unit)
}

/// The search body's filters; context and paging are added by the caller.
pub fn parse(q: &str, ctx: &Ctx) -> Map<String, Value> {
    let (tokens, content, phrases) = tokenize(q);
    let mut b = Map::new();
    if !content.is_empty() {
        b.insert("content".into(), json!(content));
    }
    if !phrases.is_empty() {
        b.insert("exact_phrases".into(), json!(phrases));
    }
    let push = |b: &mut Map<String, Value>, k: &str, v: Value| {
        let a = b.entry(k).or_insert_with(|| json!([]));
        if let Some(a) = a.as_array_mut().filter(|a| !a.contains(&v)) {
            a.push(v);
        }
    };
    let ex = |exclude: bool, k: &str| if exclude { format!("exclude_{k}") } else { k.to_string() };
    for (key, exclude, value) in tokens {
        let key = if key == "on" { "during".to_string() } else { key };
        let lower = value.trim().to_lowercase();
        match key.as_str() {
            "from" => csv(&value).iter().filter_map(|t| user(ctx, t)).for_each(|id| push(&mut b, &ex(exclude, "author_id"), json!(id))),
            "mentions" => {
                for v in csv(&value) {
                    if matches!(v.to_lowercase().as_str(), "everyone" | "here") {
                        b.insert("mention_everyone".into(), json!(true));
                    } else if let Some(id) = user(ctx, &v) {
                        push(&mut b, &ex(exclude, "mentions"), json!(id));
                    }
                }
            }
            "in" => csv(&value).iter().filter_map(|n| channel(ctx, n)).for_each(|id| push(&mut b, if exclude { "exclude_channel_id" } else { "channel_ids" }, json!(id))),
            "has" => {
                for v in csv(&lower) {
                    let v = if v == "forward" { "snapshot".to_string() } else { v };
                    if ["image", "sound", "video", "file", "sticker", "embed", "link", "poll", "snapshot"].contains(&v.as_str()) {
                        push(&mut b, &ex(exclude, "has"), json!(v));
                    }
                }
            }
            "pinned" if ["true", "false", "yes", "no", "1", "0"].contains(&lower.as_str()) => {
                b.insert("pinned".into(), json!(matches!(lower.as_str(), "true" | "yes" | "1")));
            }
            "author-type" => csv(&lower).into_iter().filter(|v| ["user", "bot", "webhook"].contains(&v.as_str())).for_each(|v| push(&mut b, "author_type", json!(v))),
            "sort" if ["timestamp", "relevance"].contains(&lower.as_str()) => drop(b.insert("sort_by".into(), json!(lower))),
            "order" if ["asc", "desc"].contains(&lower.as_str()) => drop(b.insert("sort_order".into(), json!(lower))),
            "mature" if ["true", "false"].contains(&lower.as_str()) => drop(b.insert("include_nsfw".into(), json!(lower == "true"))),
            "embed-type" => csv(&lower).into_iter().filter(|v| ["image", "video", "sound", "article"].contains(&v.as_str())).for_each(|v| push(&mut b, &ex(exclude, "embed_type"), json!(v))),
            "embed-provider" => csv(&value).into_iter().for_each(|v| push(&mut b, &ex(exclude, "embed_provider"), json!(v))),
            "link" => csv(&value).into_iter().for_each(|v| push(&mut b, &ex(exclude, "link_hostname"), json!(v))),
            "filename" => csv(&value).into_iter().for_each(|v| push(&mut b, &ex(exclude, "attachment_filename"), json!(v))),
            "ext" => csv(&value).into_iter().for_each(|v| push(&mut b, &ex(exclude, "attachment_extension"), json!(v.trim_start_matches('.')))),
            "scope" => {
                if let Some(s) = csv(&lower).into_iter().find(|s| ["current", "open_dms", "all_dms", "all_guilds", "all", "open_dms_and_all_guilds"].contains(&s.as_str())) {
                    b.insert("scope".into(), json!(s));
                }
            }
            "last" => {
                if let Some(ms) = duration_ms(&value) {
                    b.insert("min_id".into(), json!(snowflake_at(ctx.now - Duration::milliseconds(ms))));
                }
            }
            "beforeid" if !lower.is_empty() => drop(b.insert("max_id".into(), json!(value.trim()))),
            "afterid" if !lower.is_empty() => drop(b.insert("min_id".into(), json!(value.trim()))),
            "any" => csv(&value).into_iter().for_each(|v| push(&mut b, "contents", json!(v))),
            "before" | "after" | "during" => {
                if let Some((lo, hi)) = value.split_once("..") {
                    if let (Some(a), Some(z)) = (period(lo, ctx.now), period(hi, ctx.now)) {
                        b.insert("min_id".into(), json!(snowflake_at(a.0)));
                        b.insert("max_id".into(), json!(snowflake_at(z.1)));
                    }
                } else if let Some((start, end)) = period(&value, ctx.now) {
                    match key.as_str() {
                        "before" => drop(b.insert("max_id".into(), json!(snowflake_at(start)))),
                        "after" => drop(b.insert("min_id".into(), json!(snowflake_at(end)))),
                        _ => {
                            // An instant searches its whole day.
                            let end = if end > start { end } else { midnight(start.date_naive().succ_opt().unwrap_or_default()).unwrap_or(end) };
                            b.insert("min_id".into(), json!(snowflake_at(start)));
                            b.insert("max_id".into(), json!(snowflake_at(end)));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    b
}

/// Filter keys the search bar offers while it's empty (`MessageSearchBar`'s options).
pub const OPTIONS: [(&str, &str); 8] = [
    ("from:", "user"),
    ("mentions:", "user"),
    ("has:", "link, embed or file"),
    ("before:", "specific date"),
    ("during:", "specific date"),
    ("after:", "specific date"),
    ("in:", "channel"),
    ("pinned:", "true or false"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weeks_start_by_locale() {
        let sat = NaiveDate::from_ymd_opt(2026, 11, 7).unwrap();
        assert_eq!(week_start(sat, first_day("de")), NaiveDate::from_ymd_opt(2026, 11, 2).unwrap());
        assert_eq!(week_start(sat, first_day("en-US")), NaiveDate::from_ymd_opt(2026, 11, 1).unwrap());
        assert_eq!(week_start(sat, first_day("ar")), sat);
    }

    #[test]
    fn values_apply_like_fluxer() {
        let now = Local::now();
        assert!(applies("has", "image,Link", now) && !applies("has", "image,pdf", now));
        assert!(applies("-pinned", "yes", now) && !applies("pinned", "maybe", now));
        assert!(applies("before", "2026-01-02", now) && applies("during", "yesterday..today", now));
        assert!(!applies("after", "soon", now) && !applies("last", "x", now));
        assert!(applies("from", "anyone", now), "names always chip");
        assert!(!applies("from", " ", now));
    }

    fn store() -> Store {
        let mut s = Store::default();
        let ready = json!({"session_id": "s", "user": {"id": "5", "username": "me"},
            "users": [{"id": "6", "username": "ada"}],
            "guilds": [{"id": "100", "properties": {"id": "100", "name": "G", "owner_id": "5"},
                        "channels": [{"id": "101", "type": 0, "name": "general"}, {"id": "102", "type": 0, "name": "dev-talk"}]}]});
        s.apply(&reflux_model::DispatchEvent::parse("READY", &ready).unwrap(), 0);
        s
    }

    #[test]
    fn tokens_phrases_and_filters() {
        let (t, content, phrases) = tokenize(r#"hello from:ada "exact words" -has:link in: world"#);
        assert_eq!(t, [("from".into(), false, "ada".into()), ("has".into(), true, "link".into()), ("in".into(), false, "world".into())], "a bare key followed by text takes it as its value");
        assert_eq!(content, "hello");
        assert_eq!(phrases, ["exact words"]);
        let (t, _, _) = tokenize(r#"from:"ada lovelace""#);
        assert_eq!(t[0].2, r#""ada lovelace""#, "quoted values keep spaces");
    }

    #[test]
    fn builds_the_api_body() {
        let s = store();
        let now = Local.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap();
        let ctx = Ctx { store: &s, guild: Some(Snowflake(100)), now };
        let b = parse("bug from:ada,@me -from:6 in:dev has:image,forward pinned:yes mentions:everyone", &ctx);
        assert_eq!(b["content"], "bug");
        assert_eq!(b["author_id"], json!(["6", "5"]));
        assert_eq!(b["exclude_author_id"], json!(["6"]));
        assert_eq!(b["channel_ids"], json!(["102"]), "partial channel name");
        assert_eq!(b["has"], json!(["image", "snapshot"]));
        assert_eq!(b["pinned"], true);
        assert_eq!(b["mention_everyone"], true);
        assert!(!b.contains_key("mentions"));

        let day = parse("during:2026-11-05", &ctx);
        let (lo, hi) = (day["min_id"].as_str().unwrap().parse::<u64>().unwrap(), day["max_id"].as_str().unwrap().parse::<u64>().unwrap());
        assert_eq!(Snowflake(hi).timestamp_ms() - Snowflake(lo).timestamp_ms(), 86_400_000);
        let before = parse("before:today", &ctx);
        assert_eq!(Snowflake(before["max_id"].as_str().unwrap().parse().unwrap()).timestamp_ms(), Local.with_ymd_and_hms(2026, 11, 6, 0, 0, 0).unwrap().timestamp_millis() as u64);
        assert!(parse("after:nonsense", &ctx).is_empty(), "unresolvable values are dropped");
    }
}
