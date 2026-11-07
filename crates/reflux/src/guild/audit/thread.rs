//! `AuditLogThreadPresentation.ts` and `AuditLogForumPresentation.ts` (forum settings rows on a
//! channel update).

use serde_json::Value;

use super::{E, Ph, Presented, Row, Tone, boolean, number, row, s, snowflake, string, with_rows};

const REQUIRE_TAG: i64 = 1 << 4;
const HIDE_MEDIA_DOWNLOAD_OPTIONS: i64 = 1 << 15;

/// Forum tags as (id, name, what an edit compares).
fn tags(v: &Value) -> Vec<(String, String, String)> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let (id, name) = (snowflake(&t["id"])?, string(&t["name"])?);
            let key = serde_json::to_string(&[&t["name"], &t["moderated"], &t["emoji_id"], &t["emoji_name"]]).unwrap_or_default();
            Some((id, name, key))
        })
        .collect()
}

fn emoji(v: &Value) -> Option<Ph> {
    let (id, name) = (snowflake(&v["emoji_id"]), string(&v["emoji_name"]));
    (id.is_some() || name.is_some()).then(|| Ph::Emoji(id, name.unwrap_or_default()))
}

fn changed_number(e: &E, key: &str) -> Option<f64> {
    let (o, n) = e.change(key)?;
    number(n).filter(|a| Some(*a) != number(o))
}

/// `threadParentUpdateRows`.
pub(super) fn parent_rows(e: &E) -> Vec<Row> {
    let mut rows = Vec::new();
    if let Some((o, n)) = e.change("available_tags") {
        let mut before = tags(o);
        for (id, name, key) in tags(n) {
            match before.iter().position(|b| b.0 == id) {
                None => rows.push(row(format!("tag-added-{id}"), Tone::Add, "Added the tag {name}", vec![("name", Ph::Name(name))])),
                Some(i) => {
                    if before[i].2 != key {
                        rows.push(row(format!("tag-edited-{id}"), Tone::Neutral, "Edited the tag {name}", vec![("name", Ph::Name(name))]));
                    }
                    before.remove(i);
                }
            }
        }
        rows.extend(before.into_iter().map(|(id, name, _)| row(format!("tag-removed-{id}"), Tone::Remove, "Removed the tag {name}", vec![("name", Ph::Name(name))])));
    }
    if let Some((o, n)) = e.change("default_reaction_emoji") {
        match emoji(n) {
            Some(em) => rows.push(row("default_reaction_emoji", Tone::Neutral, "Set the default reaction to {emoji}", vec![("emoji", em)])),
            None if emoji(o).is_some() => rows.push(row("default_reaction_emoji", Tone::Remove, "Removed the default reaction", vec![])),
            None => {}
        }
    }
    if let Some(order) = changed_number(e, "default_sort_order") {
        rows.push(row("default_sort_order", Tone::Neutral, "Changed the default sort order to {order}", vec![("order", Ph::Label(if order == 1.0 { "Date posted" } else { "Recently active" }))]));
    }
    if let Some(layout) = changed_number(e, "default_forum_layout").filter(|l| *l != 0.0) {
        rows.push(row("default_forum_layout", Tone::Neutral, "Changed the default layout to {layout}", vec![("layout", Ph::Label(if layout == 2.0 { "Gallery" } else { "List" }))]));
    }
    if let Some(m) = changed_number(e, "default_auto_archive_duration") {
        rows.push(row("default_auto_archive_duration", Tone::Neutral, "Changed the default hide after inactivity time to {duration}", vec![("duration", Ph::Duration(m * 60.0))]));
    }
    if let Some(sm) = changed_number(e, "default_thread_rate_limit_per_user") {
        rows.push(row("default_thread_rate_limit_per_user", Tone::Neutral, "Changed the default slowmode in threads to {duration}", vec![("duration", Ph::Duration(sm))]));
    }
    if let Some((o, n)) = e.change("flags") {
        let (b, a) = (number(o).unwrap_or(0.0) as i64, number(n).unwrap_or(0.0) as i64);
        if (b ^ a) & REQUIRE_TAG != 0 {
            rows.push(if a & REQUIRE_TAG != 0 { row("require_tag", Tone::Add, "Required a tag on new posts", vec![]) } else { row("require_tag", Tone::Remove, "Stopped requiring a tag on new posts", vec![]) });
        }
        if (b ^ a) & HIDE_MEDIA_DOWNLOAD_OPTIONS != 0 {
            rows.push(if a & HIDE_MEDIA_DOWNLOAD_OPTIONS != 0 { row("hide_media_download", Tone::Add, "Hid media download options", vec![]) } else { row("hide_media_download", Tone::Remove, "Showed media download options", vec![]) });
        }
    }
    rows
}

fn thread(e: &E, new: bool) -> Ph {
    Ph::Channel(e.target().unwrap_or_default(), string(if new { e.new("name") } else { e.old("name") }), false)
}

pub fn create(e: &E) -> Presented {
    with_rows(s("{actor} started the thread {channel}", vec![("actor", e.actor()), ("channel", thread(e, true))]), vec![])
}

pub fn update(e: &E) -> Presented {
    let mut rows = Vec::new();
    if let (Some(b), Some(a)) = (string(e.old("name")), string(e.new("name")))
        && b != a
    {
        rows.push(row("name", Tone::Neutral, "Changed the name from {oldName} to {newName}", vec![("oldName", Ph::Name(b)), ("newName", Ph::Name(a))]));
    }
    for (key, on, off) in [("archived", "Closed the thread", "Opened the thread again"), ("locked", "Locked the thread", "Unlocked the thread")] {
        if let Some((o, n)) = e.change(key)
            && let Some(a) = boolean(n).filter(|a| Some(*a) != boolean(o))
        {
            rows.push(if a { row(key, Tone::Remove, on, vec![]) } else { row(key, Tone::Add, off, vec![]) });
        }
    }
    for (key, msg, unit) in [("auto_archive_duration", "Changed the hide after inactivity time to {duration}", 60.0), ("rate_limit_per_user", "Changed slowmode to {duration}", 1.0)] {
        if let Some(a) = changed_number(e, key) {
            rows.push(row(key, Tone::Neutral, msg, vec![("duration", Ph::Duration(a * unit))]));
        }
    }
    with_rows(s("{actor} updated the thread {channel}", vec![("actor", e.actor()), ("channel", thread(e, true))]), rows)
}

pub fn delete(e: &E) -> Presented {
    with_rows(s("{actor} deleted the thread {channel}", vec![("actor", e.actor()), ("channel", thread(e, false))]), vec![])
}
