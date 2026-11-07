//! Activity log presentation (`features/guild/utils/guild_tabs/audit_log/*`): each entry becomes
//! a summary sentence, detail rows with a tone and text blocks (reasons), rendered like
//! `GuildAuditLogSentence` with the placeholders in bold (free text as inline code).
//! Known users open their profile (`ClickableUser`) and channels / roles copy their ID
//! (`CopyIdInline`): the sentence's links are `user:<id>` and `copy:<id>`.
// shortcut: custom emojis show as `:name:` without the image.

mod channel;
mod expr;
mod guild;
mod invite;
mod member;
mod role;
mod thread;

use reflux_model::{Permissions, Snowflake};
use rune::{Color, El, Span, rich};
use serde_json::Value;

use crate::i18n::{t, tf};
use crate::theme::{Palette, font};

/// `AuditLogPlaceholder` (plus plain numbers).
#[derive(Clone, Debug, PartialEq)]
pub enum Ph {
    User(String),
    System,
    /// id, recorded name, falls back to "deleted-category".
    Channel(String, Option<String>, bool),
    Role(String, Option<String>),
    Name(String),
    Text(String),
    Emoji(Option<String>, String),
    Date(i64),
    Duration(f64),
    Perms(Vec<u64>),
    Color(u32),
    Label(&'static str),
    Num(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Add,
    Remove,
    Neutral,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sentence {
    pub msg: &'static str,
    pub vals: Vec<(&'static str, Ph)>,
}

pub fn s(msg: &'static str, vals: Vec<(&'static str, Ph)>) -> Sentence {
    Sentence { msg, vals }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub tone: Tone,
    pub s: Sentence,
}

pub fn row(id: impl Into<String>, tone: Tone, msg: &'static str, vals: Vec<(&'static str, Ph)>) -> Row {
    Row { id: id.into(), tone, s: s(msg, vals) }
}

/// `AuditLogBlock`: (is the ban reason, text).
pub type Block = (bool, String);

/// `AuditLogEntryPresentation`.
#[derive(Debug, PartialEq)]
pub struct Presented {
    pub summary: Sentence,
    pub rows: Vec<Row>,
    pub blocks: Vec<Block>,
}

fn only(summary: Sentence) -> Presented {
    Presented { summary, rows: Vec::new(), blocks: Vec::new() }
}

fn with_rows(summary: Sentence, rows: Vec<Row>) -> Presented {
    Presented { summary, rows, blocks: Vec::new() }
}

// ---- Readers (`AuditLogValues.ts`) ---------------------------------------------------------------

const NULL: Value = Value::Null;

/// One audit entry, as the API returns it.
pub struct E<'a> {
    pub v: &'a Value,
    pub guild: Snowflake,
    /// A webhook's, emoji's or sticker's current name by (kind, ID) (`AuditLogPresentationContext`).
    pub known: &'a dyn Fn(&str, &str) -> Option<String>,
}

impl<'a> E<'a> {
    /// `readChange`: (old, new), null when absent.
    fn change(&self, key: &str) -> Option<(&'a Value, &'a Value)> {
        let c = self.v["changes"].as_array()?.iter().find(|c| c["key"] == key)?;
        Some((c.get("old_value").unwrap_or(&NULL), c.get("new_value").unwrap_or(&NULL)))
    }
    fn old(&self, key: &str) -> &'a Value {
        self.change(key).map_or(&NULL, |c| c.0)
    }
    fn newer(&self, key: &str) -> &'a Value {
        self.change(key).map_or(&NULL, |c| c.1)
    }
    fn option(&self, key: &str) -> &'a Value {
        self.v["options"].get(key).unwrap_or(&NULL)
    }
    fn target(&self) -> Option<String> {
        snowflake(&self.v["target_id"])
    }
    fn user_id(&self) -> Option<&'a str> {
        self.v["user_id"].as_str()
    }
    /// `actorPlaceholder`.
    fn actor(&self) -> Ph {
        match self.user_id() {
            Some("0") => Ph::System,
            u => Ph::User(u.unwrap_or_default().to_string()),
        }
    }
    /// `entryTime`.
    fn time(&self) -> i64 {
        self.v["id"].as_str().and_then(|i| i.parse().ok()).map_or(0, |i| Snowflake(i).timestamp_ms() as i64)
    }
}

fn string(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        Value::String(s) => {
            let s = s.trim();
            let digits = s.strip_prefix('-').unwrap_or(s);
            let ok = !digits.is_empty() && digits.split_once('.').map_or(digits.bytes().all(|b| b.is_ascii_digit()), |(a, b)| !a.is_empty() && !b.is_empty() && a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit()));
            ok.then(|| s.parse().ok()).flatten()
        }
        _ => None,
    }
}

fn boolean(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::String(s) if s == "true" => Some(true),
        Value::String(s) if s == "false" => Some(false),
        _ => None,
    }
}

fn timestamp(v: &Value) -> Option<i64> {
    let s = string(v)?;
    chrono::DateTime::parse_from_rfc3339(&s).ok().map(|d| d.timestamp_millis())
}

fn bitfield(v: &Value) -> Option<u64> {
    match v {
        Value::String(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => s.parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

fn snowflake(v: &Value) -> Option<String> {
    v.as_str().filter(|s| (1..=20).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())).map(str::to_string)
}

fn strings(v: &Value) -> Option<Vec<String>> {
    v.as_array()?.iter().map(|i| i.as_str().map(str::to_string)).collect()
}

fn color(v: &Value) -> Option<u32> {
    number(v).filter(|c| c.fract() == 0.0 && (0.0..=16_777_215.0).contains(c)).map(|c| c as u32)
}

/// `decodeAuditReason`: percent-decoded when it holds `%XX`.
pub fn reason(v: &Value) -> Option<String> {
    let s = string(v)?;
    let b = s.as_bytes();
    if !(0..b.len().saturating_sub(2)).any(|i| b[i] == b'%' && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit()) {
        return Some(s);
    }
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            match b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(x) => {
                    out.push(x);
                    i += 3;
                    continue;
                }
                None => return Some(s),
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).ok().map_or(Some(s), |d| string(&Value::String(d)))
}

/// `permissionFlagsIn`: the set bits, lowest first.
fn flags_in(mask: u64) -> Vec<u64> {
    Permissions::all().iter().map(|p| p.bits()).filter(|b| mask & b == *b).collect()
}

fn channel(id: String) -> Ph {
    Ph::Channel(id, None, false)
}

/// `channelNoun`, as an index into the per-noun message tables:
/// text, announcement, voice, link, category, forum, media, generic.
fn noun(kind: Option<f64>) -> usize {
    match kind.map(|k| k as i64) {
        Some(0) => 0,
        Some(5) => 1,
        Some(2) => 2,
        Some(998) => 3,
        Some(4) => 4,
        Some(15) => 5,
        Some(16) => 6,
        _ => 7,
    }
}
const CATEGORY: usize = 4;

/// `presentAuditLogEntry`.
pub fn present(e: &E) -> Presented {
    let mut p = match e.v["action_type"].as_u64().unwrap_or(0) {
        1 => guild::update(e),
        10 => channel::create(e),
        11 => channel::update(e),
        12 => channel::delete(e),
        13 => channel::overwrite_create(e),
        14 => channel::overwrite_update(e),
        15 => channel::overwrite_delete(e),
        20 => member::kick(e),
        21 => member::prune(e),
        22 => member::ban_add(e),
        23 => member::ban_remove(e),
        24 => member::update(e),
        25 => member::role_update(e),
        26 => member::moved(e),
        27 => member::disconnect(e),
        28 => member::bot_add(e),
        30 => role::create(e),
        31 => role::update(e),
        32 => role::delete(e),
        40 => invite::invite_create(e),
        41 => invite::invite_update(e),
        42 => invite::invite_delete(e),
        50 => invite::webhook_create(e),
        51 => invite::webhook_update(e),
        52 => invite::webhook_delete(e),
        60 => expr::emoji_create(e),
        61 => expr::emoji_update(e),
        62 => expr::emoji_delete(e),
        90 => expr::sticker_create(e),
        91 => expr::sticker_update(e),
        92 => expr::sticker_delete(e),
        72 => expr::message_delete(e),
        73 => expr::message_bulk_delete(e),
        74 => expr::message_pin(e),
        75 => expr::message_unpin(e),
        110 => thread::create(e),
        111 => thread::update(e),
        112 => thread::delete(e),
        _ => only(s("{actor} made a change that this version of the app cannot show", vec![("actor", e.actor())])),
    };
    if let Some(r) = reason(&e.v["reason"]) {
        p.blocks.insert(0, (false, r));
    }
    p
}

// ---- Rendering (`GuildAuditLogSentence`) ----------------------------------------------------------

/// What the sentence needs from the app: names for users, channels and roles.
pub struct Names<'a> {
    pub user: &'a dyn Fn(&str) -> Option<String>,
    /// (name, shows a `#`).
    pub channel: &'a dyn Fn(&str) -> Option<(String, bool)>,
    pub role: &'a dyn Fn(&str) -> Option<String>,
    pub guild: Snowflake,
}

const TEXT_MAX: usize = 200;

fn duration(secs: f64) -> String {
    let r = secs.round() as i64;
    let (msg, key, v) = if r == 0 {
        ("{seconds, plural, one {# second} other {# seconds}}", "seconds", 0)
    } else if r % 86_400 == 0 {
        ("{days, plural, one {# day} other {# days}}", "days", r / 86_400)
    } else if r % 3_600 == 0 {
        ("{hours, plural, one {# hour} other {# hours}}", "hours", r / 3_600)
    } else if r % 60 == 0 {
        ("{minutes, plural, one {# minute} other {# minutes}}", "minutes", r / 60)
    } else {
        ("{seconds, plural, one {# second} other {# seconds}}", "seconds", r)
    };
    tf(msg, &[(key, &v.to_string())])
}

/// `formatPermissions`: up to 8 labels, else 7 and "N more permissions", as a list.
fn permissions(flags: &[u64]) -> String {
    let shown = if flags.len() > 8 { &flags[..7] } else { flags };
    let mut items: Vec<String> = shown
        .iter()
        .map(|b| {
            let f = Permissions::from_bits_retain(*b);
            crate::guild::perms::GUILD.iter().flat_map(|(_, e)| e.iter()).find(|(p, ..)| *p == f).map_or_else(|| format!("{f:?}"), |(_, l, _)| t(l).to_string())
        })
        .collect();
    if shown.len() < flags.len() {
        items.push(crate::i18n::tp("{count, plural, one {# more permission} other {# more permissions}}", (flags.len() - shown.len()) as i64, &[]));
    }
    list(&items)
}

/// `formatList` (long conjunction).
fn list(items: &[String]) -> String {
    crate::i18n::list_and(items)
}

fn number_text(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n}") }
}

/// A placeholder's text and whether it reads as code.
fn placeholder(p: &Ph, n: &Names) -> (String, bool, Option<Color>) {
    match p {
        Ph::User(id) => ((n.user)(id).unwrap_or_else(|| t("Unknown user").to_string()), false, None),
        Ph::System => (t("System").to_string(), false, None),
        Ph::Channel(id, recorded, category) => {
            let found = (n.channel)(id);
            match (recorded.clone().or_else(|| found.as_ref().map(|f| f.0.clone())), found.is_some_and(|f| f.1)) {
                (Some(name), true) => (format!("#{name}"), false, None),
                (Some(name), false) => (name, false, None),
                (None, _) if *category => (t("deleted-category").to_string(), false, None),
                (None, _) => (format!("#{}", t("deleted-channel")), false, None),
            }
        }
        Ph::Role(id, recorded) => {
            let label = if *id == n.guild.to_string() { "@everyone".to_string() } else { recorded.clone().or_else(|| (n.role)(id)).unwrap_or_else(|| t("deleted-role").to_string()) };
            (label, false, None)
        }
        Ph::Name(v) => (v.clone(), false, None),
        Ph::Text(v) => {
            let mut c: String = v.chars().take(TEXT_MAX).collect();
            if v.chars().count() > TEXT_MAX {
                c.push('…');
            }
            (c, true, None)
        }
        Ph::Emoji(_, name) => (format!(":{name}:"), false, None),
        Ph::Date(ms) => (crate::shell::format_full_date_time((*ms).max(0) as u64), false, None),
        Ph::Duration(secs) => (duration(*secs), false, None),
        Ph::Perms(f) => (permissions(f), false, None),
        Ph::Color(c) => (format!("#{c:06X}"), false, Some(crate::ui::rgba((c >> 16) as u8, (c >> 8) as u8, *c as u8, 1.0))),
        Ph::Label(l) => (t(l).to_string(), false, None),
        Ph::Num(v) => (number_text(*v), false, None),
    }
}

/// The sentence as text plus its styled spans: plurals resolved from `count`, then every other
/// `{name}` replaced with its placeholder in bold (text as inline code, colours with a dot).
/// A styled range of a composed sentence: (bytes, reads as code, dot colour, link id).
pub type Styled = (std::ops::Range<usize>, bool, Option<Color>, Option<String>);

/// What pressing a placeholder does: open a known user's profile, copy a channel's or role's ID.
fn link_of(p: &Ph, n: &Names) -> Option<String> {
    match p {
        Ph::User(id) if (n.user)(id).is_some() => Some(format!("user:{id}")),
        Ph::Channel(id, ..) | Ph::Role(id, _) => Some(format!("copy:{id}")),
        _ => None,
    }
}

pub fn compose(st: &Sentence, n: &Names) -> (String, Vec<Styled>) {
    let count = st.vals.iter().find_map(|(k, v)| match v {
        Ph::Num(c) if *k == "count" => Some(number_text(*c)),
        _ => None,
    });
    let msg = match &count {
        Some(c) => tf(st.msg, &[("count", c)]),
        None => t(st.msg).to_string(),
    };
    let mut out = String::with_capacity(msg.len() + 32);
    let mut spans = Vec::new();
    let mut rest = msg.as_str();
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let Some(end) = rest[i..].find('}') else { break };
        let key = &rest[i + 1..i + end];
        match st.vals.iter().find(|(k, _)| *k == key) {
            Some((_, p)) => {
                let (txt, code, dot) = placeholder(p, n);
                let start = out.len();
                out.push_str(&txt);
                spans.push((start..out.len(), code, None, link_of(p, n)));
                if let Some(c) = dot {
                    out.push_str(" ●");
                    spans.push((out.len() - "●".len()..out.len(), false, Some(c), None));
                }
            }
            None => out.push_str(&rest[i..=i + end]),
        }
        rest = &rest[i + end + 1..];
    }
    out.push_str(rest);
    (out, spans)
}

/// The sentence as a wrapping text element.
pub fn sentence(st: &Sentence, n: &Names, size: f32, color: Color, p: &Palette) -> El {
    let (txt, ranges) = compose(st, n);
    let spans = ranges
        .iter()
        .map(|(r, code, dot, _)| match (code, dot) {
            (_, Some(c)) => Span::new(r.start, r.end).color(*c),
            (true, _) => Span::new(r.start, r.end).mono().size(size - 1.0).bg(p.bg_code_block, 2.0),
            _ => Span::new(r.start, r.end).bold().color(p.text_primary),
        })
        .collect();
    let mut e = rich(txt, spans).font(font(size, 400.0).lh(size * 1.45)).color(color).wrap();
    for (r, _, _, link) in ranges {
        if let Some(id) = link {
            e = e.link(rune::Link::new(r, id).hover(rune::LinkPaint::new().underline(p.text_primary)));
        }
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn line(v: Value) -> (String, Vec<String>) {
        let u = |id: &str| (id == "7").then(|| "Ana".to_string());
        let c = |id: &str| (id == "9").then(|| ("general".to_string(), true));
        let r = |_: &str| None;
        let n = Names { user: &u, channel: &c, role: &r, guild: Snowflake(1) };
        let p = present(&E { v: &v, guild: Snowflake(1), known: &|_, _| None });
        (compose(&p.summary, &n).0, p.rows.iter().map(|r| compose(&r.s, &n).0).collect())
    }

    #[test]
    fn sentences_follow_fluxer() {
        assert_eq!(line(json!({"id": "1", "action_type": 20, "user_id": "7", "target_id": "8"})).0, "Ana kicked Unknown user");
        assert_eq!(line(json!({"id": "1", "action_type": 999, "user_id": "0"})).0, "System made a change that this version of the app cannot show");
        let (sum, rows) = line(json!({"id": "1", "action_type": 11, "user_id": "7", "target_id": "9", "options": {"type": 0},
            "changes": [{"key": "name", "old_value": "a", "new_value": "b"}, {"key": "rate_limit_per_user", "old_value": 0, "new_value": 3600}]}));
        assert_eq!(sum, "Ana updated the text channel #b");
        assert_eq!(rows, ["Changed the name from a to b", "Set slowmode to 1 hour"]);
        let (sum, rows) = line(json!({"id": "1", "action_type": 11, "user_id": "7", "target_id": "9", "options": {"type": 0}, "changes": [{"key": "name", "old_value": "a", "new_value": "b"}]}));
        assert_eq!((sum.as_str(), rows.len()), ("Ana renamed the text channel a to b", 0));
        let (_, rows) = line(json!({"id": "1", "action_type": 31, "user_id": "7", "target_id": "5", "changes": [{"key": "permissions", "old_value": "0", "new_value": "10"}, {"key": "color", "old_value": 0, "new_value": 255}]}));
        assert_eq!(rows[1], "Set the role color to #0000FF ●");
        assert!(rows[0].starts_with("Granted "), "{}", rows[0]);
        assert_eq!(line(json!({"id": "1", "action_type": 22, "user_id": "7", "target_id": "7", "options": {"delete_message_seconds": 86400}})).1, ["Deleted 1 day of the member's recent message history"]);
    }

    #[test]
    fn readers() {
        assert_eq!(reason(&json!("spam%20bot")).as_deref(), Some("spam bot"));
        assert_eq!(reason(&json!("100%")).as_deref(), Some("100%"));
        assert_eq!(number(&json!("1.5")), Some(1.5));
        assert_eq!(number(&json!("x")), None);
        assert_eq!(duration(90.0), "90 seconds");
        assert_eq!(list(&["a".into(), "b".into(), "c".into()]), "a, b, and c");
    }
}
