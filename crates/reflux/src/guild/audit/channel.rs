//! `AuditLogChannelPresentation.ts`.

use serde_json::Value;

use super::{CATEGORY, E, Ph, Presented, Row, Sentence, Tone, bitfield, boolean, flags_in, noun, number, row, s, snowflake, string, with_rows};

/// Per noun: text, announcement, voice, link, category, forum, media, generic.
const CREATE: [&str; 8] = [
    "{actor} created the text channel {channel}",
    "{actor} created the announcement channel {channel}",
    "{actor} created the voice channel {channel}",
    "{actor} created the link channel {channel}",
    "{actor} created the category {channel}",
    "{actor} created the forum channel {channel}",
    "{actor} created the media channel {channel}",
    "{actor} created the channel {channel}",
];
const CREATE_IN: [&str; 8] = [
    "{actor} created the text channel {channel} in {category}",
    "{actor} created the announcement channel {channel} in {category}",
    "{actor} created the voice channel {channel} in {category}",
    "{actor} created the link channel {channel} in {category}",
    "{actor} created the category {channel}",
    "{actor} created the forum channel {channel} in {category}",
    "{actor} created the media channel {channel} in {category}",
    "{actor} created the channel {channel} in {category}",
];
const RENAME: [&str; 8] = [
    "{actor} renamed the text channel {oldName} to {newName}",
    "{actor} renamed the announcement channel {oldName} to {newName}",
    "{actor} renamed the voice channel {oldName} to {newName}",
    "{actor} renamed the link channel {oldName} to {newName}",
    "{actor} renamed the category {oldName} to {newName}",
    "{actor} renamed the forum channel {oldName} to {newName}",
    "{actor} renamed the media channel {oldName} to {newName}",
    "{actor} renamed the channel {oldName} to {newName}",
];
const OVERRIDES: [&str; 8] = [
    "{actor} changed the permission overrides of the text channel {channel}",
    "{actor} changed the permission overrides of the announcement channel {channel}",
    "{actor} changed the permission overrides of the voice channel {channel}",
    "{actor} changed the permission overrides of the link channel {channel}",
    "{actor} changed the permission overrides of the category {channel}",
    "{actor} changed the permission overrides of the forum channel {channel}",
    "{actor} changed the permission overrides of the media channel {channel}",
    "{actor} changed the permission overrides of the channel {channel}",
];
const UPDATE: [&str; 8] = [
    "{actor} updated the text channel {channel}",
    "{actor} updated the announcement channel {channel}",
    "{actor} updated the voice channel {channel}",
    "{actor} updated the link channel {channel}",
    "{actor} updated the category {channel}",
    "{actor} updated the forum channel {channel}",
    "{actor} updated the media channel {channel}",
    "{actor} updated the channel {channel}",
];
const DELETE: [&str; 8] = [
    "{actor} deleted the text channel {channel}",
    "{actor} deleted the announcement channel {channel}",
    "{actor} deleted the voice channel {channel}",
    "{actor} deleted the link channel {channel}",
    "{actor} deleted the category {channel}",
    "{actor} deleted the forum channel {channel}",
    "{actor} deleted the media channel {channel}",
    "{actor} deleted the channel {channel}",
];
/// role, member, everyone.
const OW_CREATE: [&str; 3] = [
    "{actor} added a permission override for the role {role} in {channel}",
    "{actor} added a permission override for {user} in {channel}",
    "{actor} added a permission override for @everyone in {channel}",
];
const OW_UPDATE: [&str; 3] = [
    "{actor} updated the permission override for the role {role} in {channel}",
    "{actor} updated the permission override for {user} in {channel}",
    "{actor} updated the permission override for @everyone in {channel}",
];
const OW_DELETE: [&str; 3] = [
    "{actor} removed the permission override for the role {role} from {channel}",
    "{actor} removed the permission override for {user} from {channel}",
    "{actor} removed the permission override for @everyone from {channel}",
];

const WARNING_ON: f64 = 1.0;
const BITRATE_DEFAULT: f64 = 64_000.0;
const CONNECTION_LIMIT_DEFAULT: f64 = 5.0;

fn chan(id: Option<String>, recorded: Option<String>, noun: usize) -> Ph {
    Ph::Channel(id.unwrap_or_default(), recorded, noun == CATEGORY)
}

fn category(id: String) -> Ph {
    Ph::Channel(id, None, true)
}

/// `Transition`.
pub(super) enum Tr<T> {
    Set(T),
    Changed(T, T),
    Removed(T),
}

pub(super) fn transition<T: PartialEq>(e: &E, key: &str, read: impl Fn(&Value) -> Option<T>) -> Option<Tr<T>> {
    let (o, n) = e.change(key)?;
    match (read(o), read(n)) {
        (None, None) => None,
        (None, Some(a)) => Some(Tr::Set(a)),
        (Some(b), None) => Some(Tr::Removed(b)),
        (Some(b), Some(a)) if b == a => None,
        (Some(b), Some(a)) => Some(Tr::Changed(b, a)),
    }
}

fn maturity(nsfw: bool) -> Row {
    if nsfw { row("nsfw", Tone::Add, "Marked the channel as containing mature content", vec![]) } else { row("nsfw", Tone::Remove, "Marked the channel as not containing mature content", vec![]) }
}

fn added_warning(text: Option<String>) -> Row {
    match text {
        None => row("content_warning", Tone::Add, "Added a content warning", vec![]),
        Some(t) => row("content_warning", Tone::Add, "Added a content warning with the text {text}", vec![("text", Ph::Text(t))]),
    }
}

pub fn create(e: &E) -> Presented {
    let n = noun(number(e.newer("type")).or_else(|| number(e.option("type"))));
    let actor = e.actor();
    let channel = chan(e.target(), string(e.newer("name")), n);
    let summary = match snowflake(e.newer("parent_id")).filter(|_| n != CATEGORY) {
        None => s(CREATE[n], vec![("actor", actor), ("channel", channel)]),
        Some(c) => s(CREATE_IN[n], vec![("actor", actor), ("channel", channel), ("category", category(c))]),
    };
    let (voice, text_like, forum) = (n == 2, n <= 1, n == 5 || n == 6);
    let mut rows = Vec::new();
    if n == 3
        && let Some(u) = string(e.newer("url"))
    {
        rows.push(row("url", Tone::Neutral, "Set the link to {url}", vec![("url", Ph::Text(u))]));
    }
    if (text_like || voice || n == 3 || forum)
        && let Some(t) = string(e.newer("topic"))
    {
        rows.push(row("topic", Tone::Neutral, "Set the topic to {text}", vec![("text", Ph::Text(t))]));
    }
    if let Some(nsfw) = boolean(e.newer("nsfw")) {
        rows.push(maturity(nsfw));
    }
    if n != CATEGORY && number(e.newer("content_warning_level")) == Some(WARNING_ON) {
        rows.push(added_warning(string(e.newer("content_warning_text"))));
    }
    if (text_like || voice || forum)
        && let Some(sm) = number(e.newer("rate_limit_per_user")).filter(|v| *v > 0.0)
    {
        rows.push(row("rate_limit_per_user", Tone::Neutral, "Set slowmode to {duration}", vec![("duration", Ph::Duration(sm))]));
    }
    if voice {
        if let Some(b) = number(e.newer("bitrate")).filter(|b| *b > 0.0 && *b != BITRATE_DEFAULT) {
            rows.push(row("bitrate", Tone::Neutral, "Set the audio quality to {kbps} kbps", vec![("kbps", Ph::Num(b / 1000.0))]));
        }
        if let Some(l) = number(e.newer("user_limit")).filter(|l| *l > 0.0) {
            rows.push(row("user_limit", Tone::Neutral, "Limited the channel to {count, plural, one {# user} other {# users}}", vec![("count", Ph::Num(l))]));
        }
        if let Some(l) = number(e.newer("voice_connection_limit")).filter(|l| *l != CONNECTION_LIMIT_DEFAULT) {
            rows.push(row("voice_connection_limit", Tone::Neutral, "Allowed up to {count, plural, one {# connection} other {# connections}} per member", vec![("count", Ph::Num(l))]));
        }
        if let Some(r) = string(e.newer("rtc_region")) {
            rows.push(row("rtc_region", Tone::Neutral, "Set the voice region to {region}", vec![("region", Ph::Name(r))]));
        }
    }
    with_rows(summary, rows)
}

fn update_rows(e: &E, name: &Option<Tr<String>>) -> Vec<Row> {
    let mut rows = Vec::new();
    if let Some(Tr::Changed(b, a)) = name {
        rows.push(row("name", Tone::Neutral, "Changed the name from {oldName} to {newName}", vec![("oldName", Ph::Name(b.clone())), ("newName", Ph::Name(a.clone()))]));
    }
    if let Some(Tr::Changed(b, a)) = transition(e, "type", number) {
        if a == 5.0 {
            rows.push(row("type", Tone::Neutral, "Converted the channel to an announcement channel", vec![]));
        } else if a == 0.0 && b == 5.0 {
            rows.push(row("type", Tone::Neutral, "Converted the channel to a text channel", vec![]));
        }
    }
    rows.extend(transition(e, "parent_id", snowflake).map(|p| match p {
        Tr::Set(a) => row("parent_id", Tone::Neutral, "Moved the channel into the category {category}", vec![("category", category(a))]),
        Tr::Removed(b) => row("parent_id", Tone::Neutral, "Removed the channel from the category {category}", vec![("category", category(b))]),
        Tr::Changed(b, a) => row("parent_id", Tone::Neutral, "Moved the channel from the category {oldCategory} to the category {newCategory}", vec![("oldCategory", category(b)), ("newCategory", category(a))]),
    }));
    rows.extend(transition(e, "url", string).map(|u| match u {
        Tr::Set(a) => row("url", Tone::Add, "Set the link to {url}", vec![("url", Ph::Text(a))]),
        Tr::Changed(b, a) => row("url", Tone::Neutral, "Changed the link from {oldUrl} to {newUrl}", vec![("oldUrl", Ph::Text(b)), ("newUrl", Ph::Text(a))]),
        Tr::Removed(_) => row("url", Tone::Remove, "Removed the link", vec![]),
    }));
    rows.extend(transition(e, "topic", string).map(|t| match t {
        Tr::Set(a) => row("topic", Tone::Add, "Set the topic to {text}", vec![("text", Ph::Text(a))]),
        Tr::Changed(b, a) => row("topic", Tone::Neutral, "Changed the topic from {oldText} to {newText}", vec![("oldText", Ph::Text(b)), ("newText", Ph::Text(a))]),
        Tr::Removed(b) => row("topic", Tone::Remove, "Removed the topic {text}", vec![("text", Ph::Text(b))]),
    }));
    // `readNsfwOverride`: null inherits; anything unreadable is skipped.
    if let Some((o, n)) = e.change("nsfw") {
        let read = |v: &Value| if v.is_null() { Some(None) } else { boolean(v).map(Some) };
        if let Some(after) = read(n).filter(|a| Some(*a) != read(o)) {
            rows.push(match after {
                None => row("nsfw", Tone::Neutral, "Made the channel inherit its mature content setting", vec![]),
                Some(v) => maturity(v),
            });
        }
    }
    let (lb, la) = (number(e.old("content_warning_level")), number(e.newer("content_warning_level")));
    if la == Some(WARNING_ON) && lb != Some(WARNING_ON) {
        rows.push(added_warning(string(e.newer("content_warning_text"))));
    } else if lb == Some(WARNING_ON) && la != Some(WARNING_ON) {
        rows.push(row("content_warning", Tone::Remove, "Removed the content warning", vec![]));
    } else if let Some(t) = transition(e, "content_warning_text", string) {
        rows.push(match t {
            Tr::Set(a) => row("content_warning", Tone::Neutral, "Set the content warning text to {text}", vec![("text", Ph::Text(a))]),
            Tr::Changed(b, a) => row("content_warning", Tone::Neutral, "Changed the content warning text from {oldText} to {newText}", vec![("oldText", Ph::Text(b)), ("newText", Ph::Text(a))]),
            Tr::Removed(_) => row("content_warning", Tone::Neutral, "Removed the content warning text", vec![]),
        });
    }
    if let Some(Tr::Changed(b, a)) = transition(e, "rate_limit_per_user", number) {
        if b == 0.0 && a > 0.0 {
            rows.push(row("rate_limit_per_user", Tone::Add, "Set slowmode to {duration}", vec![("duration", Ph::Duration(a))]));
        } else if b > 0.0 && a == 0.0 {
            rows.push(row("rate_limit_per_user", Tone::Remove, "Turned off slowmode", vec![]));
        } else if b > 0.0 && a > 0.0 {
            rows.push(row("rate_limit_per_user", Tone::Neutral, "Changed slowmode from {oldDuration} to {newDuration}", vec![("oldDuration", Ph::Duration(b)), ("newDuration", Ph::Duration(a))]));
        }
    }
    if let Some(Tr::Changed(b, a)) = transition(e, "bitrate", number).filter(|t| matches!(t, Tr::Changed(b, a) if *b > 0.0 && *a > 0.0)) {
        rows.push(row("bitrate", Tone::Neutral, "Changed the audio quality from {oldKbps} kbps to {newKbps} kbps", vec![("oldKbps", Ph::Num(b / 1000.0)), ("newKbps", Ph::Num(a / 1000.0))]));
    }
    if let Some(Tr::Changed(b, a)) = transition(e, "user_limit", number) {
        if b == 0.0 && a > 0.0 {
            rows.push(row("user_limit", Tone::Neutral, "Limited the channel to {count, plural, one {# user} other {# users}}", vec![("count", Ph::Num(a))]));
        } else if b > 0.0 && a == 0.0 {
            rows.push(row("user_limit", Tone::Neutral, "Removed the user limit", vec![]));
        } else if b > 0.0 && a > 0.0 {
            rows.push(row("user_limit", Tone::Neutral, "Changed the user limit from {oldCount} to {newCount}", vec![("oldCount", Ph::Num(b)), ("newCount", Ph::Num(a))]));
        }
    }
    if let Some(Tr::Changed(b, a)) = transition(e, "voice_connection_limit", number) {
        rows.push(row("voice_connection_limit", Tone::Neutral, "Changed the connections per member from {oldCount} to {newCount}", vec![("oldCount", Ph::Num(b)), ("newCount", Ph::Num(a))]));
    }
    rows.extend(transition(e, "rtc_region", string).map(|r| match r {
        Tr::Set(a) => row("rtc_region", Tone::Neutral, "Set the voice region to {region}", vec![("region", Ph::Name(a))]),
        Tr::Changed(b, a) => row("rtc_region", Tone::Neutral, "Changed the voice region from {oldRegion} to {newRegion}", vec![("oldRegion", Ph::Name(b)), ("newRegion", Ph::Name(a))]),
        Tr::Removed(_) => row("rtc_region", Tone::Neutral, "Set the voice region to automatic", vec![]),
    }));
    rows.extend(super::thread::parent_rows(e));
    rows
}

pub fn update(e: &E) -> Presented {
    let n = noun(number(e.option("type")));
    let actor = e.actor();
    let name = transition(e, "name", string);
    let rows = update_rows(e, &name);
    let overrides = matches!(transition(e, "permission_overwrite_count", number), Some(Tr::Changed(..)));
    if let Some(Tr::Changed(b, a)) = &name
        && rows.len() == 1
        && !overrides
    {
        return with_rows(s(RENAME[n], vec![("actor", actor), ("oldName", Ph::Name(b.clone())), ("newName", Ph::Name(a.clone()))]), vec![]);
    }
    let channel = chan(e.target(), string(e.newer("name")), n);
    if rows.is_empty() && overrides {
        return with_rows(s(OVERRIDES[n], vec![("actor", actor), ("channel", channel)]), vec![]);
    }
    with_rows(s(UPDATE[n], vec![("actor", actor), ("channel", channel)]), rows)
}

pub fn delete(e: &E) -> Presented {
    let n = noun(number(e.option("type")).or_else(|| number(e.old("type"))));
    with_rows(s(DELETE[n], vec![("actor", e.actor()), ("channel", chan(e.target(), string(e.old("name")), n))]), vec![])
}

/// `overwriteSummary`.
fn overwrite_summary(e: &E, msgs: [&'static str; 3]) -> Sentence {
    let actor = e.actor();
    let target = e.target().unwrap_or_default();
    let channel = Ph::Channel(snowflake(e.option("channel_id")).unwrap_or_default(), None, false);
    let kind = number(e.option("type")).or_else(|| number(e.newer("type"))).or_else(|| number(e.old("type")));
    if kind == Some(1.0) {
        s(msgs[1], vec![("actor", actor), ("user", Ph::User(target)), ("channel", channel)])
    } else if e.v["target_id"].as_str() == Some(e.guild.to_string().as_str()) {
        s(msgs[2], vec![("actor", actor), ("channel", channel)])
    } else {
        s(msgs[0], vec![("actor", actor), ("role", Ph::Role(target, string(e.option("role_name")))), ("channel", channel)])
    }
}

/// `readOverwriteMask`: missing is 0.
fn mask(v: &Value) -> Option<u64> {
    if v.is_null() { Some(0) } else { bitfield(v) }
}

fn perms_row(id: &str, tone: Tone, msg: &'static str, flags: Vec<u64>) -> Option<Row> {
    (!flags.is_empty()).then(|| row(id, tone, msg, vec![("permissions", Ph::Perms(flags))]))
}

pub fn overwrite_create(e: &E) -> Presented {
    let flags = |v: &Value| mask(v).map(flags_in).unwrap_or_default();
    let rows = [perms_row("allow", Tone::Add, "Allowed {permissions}", flags(e.newer("allow"))), perms_row("deny", Tone::Remove, "Denied {permissions}", flags(e.newer("deny")))];
    with_rows(overwrite_summary(e, OW_CREATE), rows.into_iter().flatten().collect())
}

pub fn overwrite_update(e: &E) -> Presented {
    let masks = (mask(e.old("allow")), mask(e.old("deny")), mask(e.newer("allow")), mask(e.newer("deny")));
    let rows = match masks {
        (Some(pa), Some(pd), Some(na), Some(nd)) => {
            let all = flags_in(u64::MAX);
            let has = |m: u64, f: u64| m & f == f;
            let allowed = all.iter().copied().filter(|f| has(na, *f) && !has(pa, *f)).collect();
            let denied = all.iter().copied().filter(|f| has(nd, *f) && !has(pd, *f)).collect();
            let cleared = all.iter().copied().filter(|f| (has(pa, *f) || has(pd, *f)) && !has(na, *f) && !has(nd, *f)).collect();
            [
                perms_row("allowed", Tone::Add, "Allowed {permissions}", allowed),
                perms_row("denied", Tone::Remove, "Denied {permissions}", denied),
                perms_row("cleared", Tone::Neutral, "Removed the override for {permissions}", cleared),
            ]
            .into_iter()
            .flatten()
            .collect()
        }
        _ => vec![],
    };
    with_rows(overwrite_summary(e, OW_UPDATE), rows)
}

pub fn overwrite_delete(e: &E) -> Presented {
    let flags = |v: &Value| mask(v).map(flags_in).unwrap_or_default();
    let rows = [perms_row("allow", Tone::Neutral, "The override allowed {permissions}", flags(e.old("allow"))), perms_row("deny", Tone::Neutral, "The override denied {permissions}", flags(e.old("deny")))];
    with_rows(overwrite_summary(e, OW_DELETE), rows.into_iter().flatten().collect())
}
