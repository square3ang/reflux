//! `AuditLogGuildPresentation.ts`.

use serde_json::Value;

use super::{E, Ph, Presented, Row, Sentence, Tone, bitfield, boolean, channel, number, row, s, snowflake, string, strings, timestamp, with_rows};

/// `GuildChange`: the row, and the summary used when it's the only change.
type Change = (Row, Option<Sentence>);

fn setting(id: &str, tone: Tone, msg: &'static str, vals: Vec<(&'static str, Ph)>) -> Option<Change> {
    Some((row(id, tone, msg, vals), None))
}

fn summarized(id: &str, tone: Tone, msg: &'static str, sum: &'static str, actor: &Ph, vals: Vec<(&'static str, Ph)>) -> Option<Change> {
    let mut sv = vec![("actor", actor.clone())];
    sv.extend(vals.clone());
    Some((row(id, tone, msg, vals), Some(s(sum, sv))))
}

/// `readDiff`: both readable and different.
fn diff<T: PartialEq>(e: &E, key: &str, read: impl Fn(&Value) -> Option<T>) -> Option<(T, T)> {
    let (o, n) = e.change(key)?;
    let (o, n) = (read(o)?, read(n)?);
    (o != n).then_some((o, n))
}

enum Nullable<T> {
    Added(T),
    Changed(T, T),
    Removed(T),
}

/// `readNullableChange`: `read` gives `None` for unreadable, `Some(None)` for null.
fn nullable<T: PartialEq>(e: &E, key: &str, read: impl Fn(&Value) -> Option<Option<T>>) -> Option<Nullable<T>> {
    let (o, n) = e.change(key)?;
    match (read(o)?, read(n)?) {
        (None, None) => None,
        (None, Some(n)) => Some(Nullable::Added(n)),
        (Some(o), None) => Some(Nullable::Removed(o)),
        (Some(o), Some(n)) if o == n => None,
        (Some(o), Some(n)) => Some(Nullable::Changed(o, n)),
    }
}

fn nullable_text(v: &Value) -> Option<Option<String>> {
    match v {
        Value::Null => Some(None),
        Value::String(_) => Some(string(v)),
        _ => None,
    }
}

fn nullable_snowflake(v: &Value) -> Option<Option<String>> {
    if v.is_null() { Some(None) } else { snowflake(v).map(Some) }
}

fn nullable_time(v: &Value) -> Option<Option<i64>> {
    if v.is_null() { Some(None) } else { timestamp(v).map(Some) }
}

fn switch(v: &Value, off: f64, on: f64) -> Option<bool> {
    match number(v) {
        Some(x) if x == off => Some(false),
        Some(x) if x == on => Some(true),
        _ => None,
    }
}

fn label(v: &Value, labels: &[(f64, &'static str)]) -> Option<&'static str> {
    let x = number(v)?;
    labels.iter().find(|l| l.0 == x).map(|l| l.1)
}

/// `featureDelta`: Some(true) when `feature` was added, Some(false) when removed.
fn feature(lists: &Option<(Vec<String>, Vec<String>)>, f: &str) -> Option<bool> {
    let (prev, next) = lists.as_ref()?;
    let (had, has) = (prev.iter().any(|x| x == f), next.iter().any(|x| x == f));
    (had != has).then_some(has)
}

fn toggle(id: &str, on: Option<bool>, [(ton, mon), (toff, moff)]: [(Tone, &'static str); 2]) -> Option<Change> {
    let on = on?;
    if on { setting(id, ton, mon, vec![]) } else { setting(id, toff, moff, vec![]) }
}

fn image(e: &E, key: &str, [added, changed, removed]: [&'static str; 3]) -> Option<Change> {
    match nullable(e, key, nullable_text)? {
        Nullable::Added(_) => setting(key, Tone::Add, added, vec![]),
        Nullable::Changed(..) => setting(key, Tone::Neutral, changed, vec![]),
        Nullable::Removed(_) => setting(key, Tone::Remove, removed, vec![]),
    }
}

fn labelled(e: &E, key: &str, labels: &[(f64, &'static str)], msg: &'static str, old: &'static str, new: &'static str) -> Option<Change> {
    let (o, n) = diff(e, key, |v| label(v, labels))?;
    setting(key, Tone::Neutral, msg, vec![(old, Ph::Label(o)), (new, Ph::Label(n))])
}

fn channel_setting(e: &E, key: &str, [set, changed, removed]: [&'static str; 3]) -> Option<Change> {
    match nullable(e, key, nullable_snowflake)? {
        Nullable::Added(c) => setting(key, Tone::Add, set, vec![("channel", channel(c))]),
        Nullable::Changed(o, n) => setting(key, Tone::Neutral, changed, vec![("oldChannel", channel(o)), ("newChannel", channel(n))]),
        Nullable::Removed(_) => setting(key, Tone::Remove, removed, vec![]),
    }
}

pub fn update(e: &E) -> Presented {
    let actor = e.actor();
    let features = e.change("features").and_then(|(o, n)| Some((strings(o)?, strings(n)?)));
    let cloning = |on: &str, off: &str| feature(&features, on).or_else(|| feature(&features, off).map(|a| !a));
    let turned_on = |key: &str, read: &dyn Fn(&Value) -> Option<bool>| diff(e, key, read).map(|d| d.1);
    let seconds = |v: &Value| number(v).filter(|s| *s >= 0.0).map(|s| s.round() as i64);
    let changes: Vec<Change> = [
        diff(e, "name", string).and_then(|(o, n)| summarized("name", Tone::Neutral, "Renamed the community from {oldName} to {newName}", "{actor} renamed the community from {oldName} to {newName}", &actor, vec![("oldName", Ph::Name(o)), ("newName", Ph::Name(n))])),
        image(e, "icon_hash", ["Added a community icon", "Changed the community icon", "Removed the community icon"]),
        image(e, "banner_hash", ["Added a community banner", "Changed the community banner", "Removed the community banner"]),
        toggle("detached_banner", feature(&features, "DETACHED_BANNER"), [(Tone::Add, "Turned on the detached banner"), (Tone::Remove, "Turned off the detached banner")]),
        image(e, "splash_hash", ["Added an invite background", "Changed the invite background", "Removed the invite background"]),
        labelled(e, "splash_card_alignment", &[(0.0, "Centered"), (1.0, "Left aligned"), (2.0, "Right aligned")], "Changed the invite background alignment from {oldAlignment} to {newAlignment}", "oldAlignment", "newAlignment"),
        image(e, "embed_splash_hash", ["Added a chat embed background", "Changed the chat embed background", "Removed the chat embed background"]),
        channel_setting(e, "system_channel_id", ["Set the system channel to {channel}", "Changed the system channel from {oldChannel} to {newChannel}", "Removed the system channel"]),
        toggle("system_channel_flags", turned_on("system_channel_flags", &|v| bitfield(v).map(|f| f & 1 != 0)), [(Tone::Neutral, "Hid join messages in the system channel"), (Tone::Neutral, "Stopped hiding join messages in the system channel")]),
        labelled(e, "default_message_notifications", &[(0.0, "All messages"), (1.0, "Mentions only")], "Changed the default notification setting from {oldSetting} to {newSetting}", "oldSetting", "newSetting"),
        channel_setting(e, "afk_channel_id", ["Set the AFK channel to {channel}", "Changed the AFK channel from {oldChannel} to {newChannel}", "Removed the AFK channel"]),
        diff(e, "afk_timeout", seconds).and_then(|(o, n)| setting("afk_timeout", Tone::Neutral, "Changed the AFK timeout from {oldDuration} to {newDuration}", vec![("oldDuration", Ph::Duration(o as f64)), ("newDuration", Ph::Duration(n as f64))])),
        toggle("text_channel_flexible_names", feature(&features, "TEXT_CHANNEL_FLEXIBLE_NAMES"), [(Tone::Add, "Allowed flexible text channel names"), (Tone::Remove, "Stopped allowing flexible text channel names")]),
        toggle("hide_owner_crown", feature(&features, "HIDE_OWNER_CROWN"), [(Tone::Neutral, "Hid the community owner crown"), (Tone::Neutral, "Stopped hiding the community owner crown")]),
        nullable(e, "message_history_cutoff", nullable_time).and_then(|c| match c {
            Nullable::Added(d) => setting("message_history_cutoff", Tone::Add, "Set the message history threshold to {date}", vec![("date", Ph::Date(d))]),
            Nullable::Changed(o, n) => setting("message_history_cutoff", Tone::Neutral, "Changed the message history threshold from {oldDate} to {newDate}", vec![("oldDate", Ph::Date(o)), ("newDate", Ph::Date(n))]),
            Nullable::Removed(_) => setting("message_history_cutoff", Tone::Remove, "Removed the message history threshold", vec![]),
        }),
        feature(&features, "INVITES_DISABLED").and_then(|paused| {
            if paused {
                summarized("invites_disabled", Tone::Remove, "Paused invites", "{actor} paused invites", &actor, vec![])
            } else {
                summarized("invites_disabled", Tone::Add, "Resumed invites", "{actor} resumed invites", &actor, vec![])
            }
        }),
        toggle("emoji_cloning", cloning("CLONE_EMOJI_ENABLED", "CLONE_EMOJI_DISABLED"), [(Tone::Add, "Allowed others to clone this community's emojis"), (Tone::Remove, "Stopped allowing others to clone this community's emojis")]),
        toggle("sticker_cloning", cloning("CLONE_STICKER_ENABLED", "CLONE_STICKER_DISABLED"), [(Tone::Add, "Allowed others to clone this community's stickers"), (Tone::Remove, "Stopped allowing others to clone this community's stickers")]),
        labelled(e, "verification_level", &[(0.0, "None"), (1.0, "Low"), (2.0, "Medium"), (3.0, "High"), (4.0, "High")], "Changed the verification level from {oldLevel} to {newLevel}", "oldLevel", "newLevel"),
        labelled(e, "explicit_content_filter", &[(0.0, "Off"), (1.0, "Filter members without roles"), (2.0, "Filter everyone")], "Changed the explicit content filter from {oldFilter} to {newFilter}", "oldFilter", "newFilter"),
        toggle(
            "mature_content",
            if e.change("nsfw").is_some() { turned_on("nsfw", &boolean) } else { turned_on("nsfw_level", &|v| switch(v, 0.0, 3.0)) },
            [(Tone::Add, "Turned on mature content"), (Tone::Remove, "Turned off mature content")],
        ),
        toggle("content_warning_level", turned_on("content_warning_level", &|v| switch(v, 0.0, 1.0)), [(Tone::Add, "Turned on the content warning"), (Tone::Remove, "Turned off the content warning")]),
        nullable(e, "content_warning_text", nullable_text).and_then(|c| match c {
            Nullable::Added(t) => setting("content_warning_text", Tone::Add, "Set the custom warning text to {text}", vec![("text", Ph::Text(t))]),
            Nullable::Changed(o, n) => setting("content_warning_text", Tone::Neutral, "Changed the custom warning text from {oldText} to {newText}", vec![("oldText", Ph::Text(o)), ("newText", Ph::Text(n))]),
            Nullable::Removed(_) => setting("content_warning_text", Tone::Remove, "Removed the custom warning text", vec![]),
        }),
        toggle("mfa_level", turned_on("mfa_level", &|v| switch(v, 0.0, 1.0)), [(Tone::Add, "Required 2FA for moderation actions"), (Tone::Remove, "Stopped requiring 2FA for moderation actions")]),
        e.change("owner_id").and_then(|(o, n)| {
            let owner = snowflake(n).filter(|n| snowflake(o).as_ref() != Some(n))?;
            summarized("owner_id", Tone::Neutral, "Transferred community ownership to {user}", "{actor} transferred community ownership to {user}", &actor, vec![("user", Ph::User(owner))])
        }),
        nullable(e, "vanity_url_code", nullable_text).and_then(|c| match c {
            Nullable::Added(v) => summarized("vanity_url_code", Tone::Add, "Set the vanity URL to {code}", "{actor} set the vanity URL to {code}", &actor, vec![("code", Ph::Text(v))]),
            Nullable::Changed(o, n) => summarized("vanity_url_code", Tone::Neutral, "Changed the vanity URL from {oldCode} to {newCode}", "{actor} changed the vanity URL from {oldCode} to {newCode}", &actor, vec![("oldCode", Ph::Text(o)), ("newCode", Ph::Text(n))]),
            Nullable::Removed(v) => summarized("vanity_url_code", Tone::Remove, "Removed the vanity URL {code}", "{actor} removed the vanity URL {code}", &actor, vec![("code", Ph::Text(v))]),
        }),
    ]
    .into_iter()
    .flatten()
    .collect();
    if let [(_, Some(sum))] = changes.as_slice() {
        return with_rows(sum.clone(), vec![]);
    }
    with_rows(s("{actor} updated the community settings", vec![("actor", actor)]), changes.into_iter().map(|c| c.0).collect())
}
