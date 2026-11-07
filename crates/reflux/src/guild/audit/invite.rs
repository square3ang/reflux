//! `AuditLogInviteWebhookPresentation.ts`.

use serde_json::Value;

use super::{E, Ph, Presented, Row, Sentence, Tone, boolean, channel, number, row, s, snowflake, string, timestamp, with_rows};

fn side<'a>(e: &E<'a>, key: &str, new: bool) -> &'a Value {
    if new { e.newer(key) } else { e.old(key) }
}

fn count(v: &Value) -> Option<f64> {
    number(v).filter(|c| c.fract() == 0.0 && *c >= 0.0)
}

fn snapshot_channel(e: &E, new: bool) -> Option<String> {
    snowflake(e.option("channel_id")).or_else(|| snowflake(side(e, "channel_id", new)))
}

fn code(e: &E, new: bool) -> Ph {
    Ph::Text(string(&e.v["target_id"]).or_else(|| string(side(e, "code", new))).unwrap_or_default())
}

/// (max age, max uses, temporary).
fn settings(e: &E, new: bool) -> (Option<f64>, Option<f64>, Option<bool>) {
    (
        count(e.option("max_age")).or_else(|| count(side(e, "max_age", new))),
        count(e.option("max_uses")).or_else(|| count(side(e, "max_uses", new))),
        boolean(e.option("temporary")).or_else(|| boolean(side(e, "temporary", new))),
    )
}

fn snapshot_summary(e: &E, new: bool, in_channel: &'static str, without: &'static str) -> Sentence {
    match snapshot_channel(e, new) {
        None => s(without, vec![("actor", e.actor()), ("code", code(e, new))]),
        Some(c) => s(in_channel, vec![("actor", e.actor()), ("code", code(e, new)), ("channel", channel(c))]),
    }
}

fn max_age_row(age: f64) -> Row {
    if age == 0.0 { row("max_age", Tone::Neutral, "The invite was set to never expire", vec![]) } else { row("max_age", Tone::Neutral, "The invite was set to expire after {duration}", vec![("duration", Ph::Duration(age))]) }
}

fn max_uses_row(uses: f64) -> Row {
    if uses == 0.0 {
        row("max_uses", Tone::Neutral, "The invite was set to allow unlimited uses", vec![])
    } else {
        row("max_uses", Tone::Neutral, "{count, plural, one {The invite was limited to # use} other {The invite was limited to # uses}}", vec![("count", Ph::Num(uses))])
    }
}

fn temporary_row() -> Row {
    row("temporary", Tone::Neutral, "The invite was set to grant temporary membership", vec![])
}

pub fn invite_create(e: &E) -> Presented {
    let (age, uses, temp) = settings(e, true);
    let mut rows: Vec<Row> = age.map(max_age_row).into_iter().chain(uses.map(max_uses_row)).collect();
    if temp == Some(true) {
        rows.push(temporary_row());
    }
    with_rows(snapshot_summary(e, true, "{actor} created the invite {code} for {channel}", "{actor} created the invite {code}"), rows)
}

pub fn invite_update(e: &E) -> Presented {
    with_rows(s("{actor} updated the invite {code}", vec![("actor", e.actor()), ("code", code(e, true))]), vec![])
}

pub fn invite_delete(e: &E) -> Presented {
    let (age, uses, temp) = settings(e, false);
    let creator = snowflake(e.option("inviter_id")).or_else(|| snowflake(e.old("inviter_id"))).filter(|c| Some(c.as_str()) != e.user_id());
    let created = timestamp(e.old("created_at"));
    let mut rows = Vec::new();
    match (creator, created) {
        (Some(u), Some(d)) => rows.push(row("creator", Tone::Neutral, "The invite was created by {user} on {date}", vec![("user", Ph::User(u)), ("date", Ph::Date(d))])),
        (Some(u), None) => rows.push(row("creator", Tone::Neutral, "The invite was created by {user}", vec![("user", Ph::User(u))])),
        (None, Some(d)) => rows.push(row("creator", Tone::Neutral, "The invite was created on {date}", vec![("date", Ph::Date(d))])),
        (None, None) => {}
    }
    if let Some(used) = count(e.old("uses")) {
        rows.push(if used == 0.0 {
            row("uses", Tone::Neutral, "The invite was never used", vec![])
        } else {
            row("uses", Tone::Neutral, "{count, plural, one {The invite was used # time} other {The invite was used # times}}", vec![("count", Ph::Num(used))])
        });
    }
    match (age, created) {
        (Some(a), Some(c)) if a > 0.0 => rows.push(row("max_age", Tone::Neutral, "The invite was scheduled to expire on {date}", vec![("date", Ph::Date(c + (a * 1000.0) as i64))])),
        (Some(a), _) => rows.push(max_age_row(a)),
        _ => {}
    }
    rows.extend(uses.map(max_uses_row));
    if temp == Some(true) {
        rows.push(temporary_row());
    }
    with_rows(snapshot_summary(e, false, "{actor} deleted the invite {code} for {channel}", "{actor} deleted the invite {code}"), rows)
}

/// follow into channel, named in channel, named, unnamed.
fn webhook_summary(e: &E, new: bool, m: [&'static str; 4]) -> Sentence {
    let actor = e.actor();
    let ch = snapshot_channel(e, new);
    let kind = number(e.option("type")).or_else(|| number(side(e, "type", new)));
    if kind == Some(2.0)
        && let Some(c) = &ch
    {
        return s(m[0], vec![("actor", actor), ("channel", channel(c.clone()))]);
    }
    match (string(side(e, "name", new)), ch) {
        (None, _) => s(m[3], vec![("actor", actor)]),
        (Some(n), None) => s(m[2], vec![("actor", actor), ("name", Ph::Name(n))]),
        (Some(n), Some(c)) => s(m[1], vec![("actor", actor), ("name", Ph::Name(n)), ("channel", channel(c))]),
    }
}

pub fn webhook_create(e: &E) -> Presented {
    with_rows(webhook_summary(e, true, ["{actor} followed an announcement channel into {channel}", "{actor} created the webhook {name} in {channel}", "{actor} created the webhook {name}", "{actor} created a webhook"]), vec![])
}

pub fn webhook_update(e: &E) -> Presented {
    let actor = e.actor();
    let (old, new) = (string(e.old("name")), string(e.newer("name")));
    let rename = match (&old, &new) {
        (Some(o), Some(n)) if o != n => Some(vec![("oldName", Ph::Name(o.clone())), ("newName", Ph::Name(n.clone()))]),
        _ => None,
    };
    let mut rows = Vec::new();
    if let Some(r) = &rename {
        rows.push(row("name", Tone::Neutral, "Changed the name from {oldName} to {newName}", r.clone()));
    }
    if let (Some(o), Some(n)) = (snowflake(e.old("channel_id")), snowflake(e.newer("channel_id")))
        && o != n
    {
        rows.push(row("channel_id", Tone::Neutral, "Moved from {oldChannel} to {newChannel}", vec![("oldChannel", channel(o)), ("newChannel", channel(n))]));
    }
    if let Some((o, n)) = e.change("avatar_hash")
        && (o.is_null() || o.is_string())
        && (n.is_null() || n.is_string())
    {
        match (string(o), string(n)) {
            (a, b) if a == b => {}
            (None, _) => rows.push(row("avatar_hash", Tone::Add, "Added an avatar", vec![])),
            (_, None) => rows.push(row("avatar_hash", Tone::Remove, "Removed the avatar", vec![])),
            _ => rows.push(row("avatar_hash", Tone::Neutral, "Changed the avatar", vec![])),
        }
    }
    if let Some(r) = rename
        && rows.len() == 1
    {
        let mut v = vec![("actor", actor)];
        v.extend(r);
        return with_rows(s("{actor} renamed the webhook {oldName} to {newName}", v), vec![]);
    }
    let name = new.or_else(|| e.target().and_then(|id| (e.known)("webhook", &id)));
    let summary = match name {
        None => s("{actor} updated a webhook", vec![("actor", actor)]),
        Some(n) => s("{actor} updated the webhook {name}", vec![("actor", actor), ("name", Ph::Name(n))]),
    };
    with_rows(summary, rows)
}

pub fn webhook_delete(e: &E) -> Presented {
    let creator = snowflake(e.old("creator_id")).filter(|c| Some(c.as_str()) != e.user_id());
    let rows = creator.map(|u| row("creator", Tone::Neutral, "The webhook was created by {user}", vec![("user", Ph::User(u))])).into_iter().collect();
    with_rows(webhook_summary(e, false, ["{actor} stopped following an announcement channel in {channel}", "{actor} deleted the webhook {name} from {channel}", "{actor} deleted the webhook {name}", "{actor} deleted a webhook"]), rows)
}
