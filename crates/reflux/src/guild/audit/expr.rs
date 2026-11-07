//! `AuditLogExpressionMessagePresentation.ts`.

use super::{E, Ph, Presented, Row, Tone, channel, number, only, row, s, snowflake, string, with_rows};

fn name_change(e: &E) -> Option<(String, String)> {
    let (o, n) = (string(e.old("name"))?, string(e.new("name"))?);
    (o != n).then_some((o, n))
}

/// `readCurrentName`: the recorded name, else what the app knows by ID.
fn current_name(e: &E, kind: &str) -> Option<String> {
    string(e.new("name")).or_else(|| e.target().and_then(|id| (e.known)(kind, &id)).and_then(|n| string(&serde_json::Value::String(n))))
}

fn uploader(e: &E, msg: &'static str) -> Vec<Row> {
    snowflake(e.old("creator_id")).filter(|c| Some(c.as_str()) != e.user_id()).map(|u| row("uploader", Tone::Neutral, msg, vec![("user", Ph::User(u))])).into_iter().collect()
}

fn in_channel(e: &E, with: &'static str, without: &'static str) -> Presented {
    only(match snowflake(e.option("channel_id")) {
        None => s(without, vec![("actor", e.actor())]),
        Some(c) => s(with, vec![("actor", e.actor()), ("channel", channel(c))]),
    })
}

pub fn emoji_create(e: &E) -> Presented {
    only(match string(e.new("name")) {
        None => s("{actor} added an emoji", vec![("actor", e.actor())]),
        Some(n) => s("{actor} added the emoji {emoji}", vec![("actor", e.actor()), ("emoji", Ph::Emoji(e.target(), n))]),
    })
}

pub fn emoji_update(e: &E) -> Presented {
    let actor = e.actor();
    if let Some((o, n)) = name_change(e) {
        return only(s("{actor} renamed the emoji {oldEmoji} to {newEmoji}", vec![("actor", actor), ("oldEmoji", Ph::Emoji(None, o)), ("newEmoji", Ph::Emoji(e.target(), n))]));
    }
    only(match current_name(e, "emoji") {
        None => s("{actor} updated an emoji", vec![("actor", actor)]),
        Some(n) => s("{actor} updated the emoji {emoji}", vec![("actor", actor), ("emoji", Ph::Emoji(e.target(), n))]),
    })
}

pub fn emoji_delete(e: &E) -> Presented {
    let summary = match string(e.old("name")) {
        None => s("{actor} deleted an emoji", vec![("actor", e.actor())]),
        Some(n) => s("{actor} deleted the emoji {emoji}", vec![("actor", e.actor()), ("emoji", Ph::Emoji(None, n))]),
    };
    with_rows(summary, uploader(e, "The emoji was uploaded by {user}"))
}

pub fn sticker_create(e: &E) -> Presented {
    let summary = match string(e.new("name")) {
        None => s("{actor} added a sticker", vec![("actor", e.actor())]),
        Some(n) => s("{actor} added the sticker {name}", vec![("actor", e.actor()), ("name", Ph::Name(n))]),
    };
    let rows = string(e.new("description")).map(|d| row("description", Tone::Neutral, "Set the description to {text}", vec![("text", Ph::Text(d))])).into_iter().collect();
    with_rows(summary, rows)
}

pub fn sticker_update(e: &E) -> Presented {
    let actor = e.actor();
    let name = name_change(e);
    let desc = match (string(e.old("description")), string(e.new("description"))) {
        (None, None) => None,
        (None, Some(n)) => Some(row("description", Tone::Add, "Set the description to {text}", vec![("text", Ph::Text(n))])),
        (Some(o), None) => Some(row("description", Tone::Remove, "Removed the description {text}", vec![("text", Ph::Text(o))])),
        (Some(o), Some(n)) if o == n => None,
        (Some(o), Some(n)) => Some(row("description", Tone::Neutral, "Changed the description from {oldText} to {newText}", vec![("oldText", Ph::Text(o)), ("newText", Ph::Text(n))])),
    };
    if let (Some((o, n)), None) = (&name, &desc) {
        return only(s("{actor} renamed the sticker {oldName} to {newName}", vec![("actor", actor), ("oldName", Ph::Name(o.clone())), ("newName", Ph::Name(n.clone()))]));
    }
    let current = current_name(e, "sticker");
    let mut rows = Vec::new();
    if let Some((o, n)) = name {
        rows.push(row("name", Tone::Neutral, "Changed the name from {oldName} to {newName}", vec![("oldName", Ph::Name(o)), ("newName", Ph::Name(n))]));
    }
    rows.extend(desc);
    let summary = match current {
        None => s("{actor} updated a sticker", vec![("actor", actor)]),
        Some(n) => s("{actor} updated the sticker {name}", vec![("actor", actor), ("name", Ph::Name(n))]),
    };
    with_rows(summary, rows)
}

pub fn sticker_delete(e: &E) -> Presented {
    let summary = match string(e.old("name")) {
        None => s("{actor} deleted a sticker", vec![("actor", e.actor())]),
        Some(n) => s("{actor} deleted the sticker {name}", vec![("actor", e.actor()), ("name", Ph::Name(n))]),
    };
    with_rows(summary, uploader(e, "The sticker was uploaded by {user}"))
}

pub fn message_delete(e: &E) -> Presented {
    in_channel(e, "{actor} deleted a message in {channel}", "{actor} deleted a message")
}

pub fn message_bulk_delete(e: &E) -> Presented {
    let Some(n) = number(e.option("count")).filter(|c| c.fract() == 0.0 && *c > 0.0) else {
        return in_channel(e, "{actor} deleted some messages in {channel}", "{actor} deleted some messages");
    };
    only(match snowflake(e.option("channel_id")) {
        None => s("{actor} deleted {count, plural, one {# message} other {# messages}}", vec![("actor", e.actor()), ("count", Ph::Num(n))]),
        Some(c) => s("{actor} deleted {count, plural, one {# message} other {# messages}} in {channel}", vec![("actor", e.actor()), ("count", Ph::Num(n)), ("channel", channel(c))]),
    })
}

pub fn message_pin(e: &E) -> Presented {
    in_channel(e, "{actor} pinned a message in {channel}", "{actor} pinned a message")
}

pub fn message_unpin(e: &E) -> Presented {
    in_channel(e, "{actor} unpinned a message in {channel}", "{actor} unpinned a message")
}
