//! `AuditLogRolePresentation.ts`.

use reflux_model::Permissions;
use serde_json::Value;

use super::{E, Ph, Presented, Tone, bitfield, boolean, color, flags_in, number, only, row, s, snowflake, string, strings, with_rows};

fn role(e: &E, recorded: Option<String>) -> Ph {
    Ph::Role(snowflake(&e.v["target_id"]).unwrap_or_default(), recorded)
}

fn named(v: &Value) -> Vec<u64> {
    let mask = strings(v).unwrap_or_default().iter().filter_map(|n| Permissions::from_name(n)).fold(0, |m, p| m | p.bits());
    flags_in(mask)
}

/// (added, removed) permission bits.
fn permission_changes(e: &E) -> Option<(Vec<u64>, Vec<u64>)> {
    let c = match e.change("permissions") {
        Some((o, n)) => {
            let (o, n) = (bitfield(o)?, bitfield(n)?);
            (flags_in(n & !o), flags_in(o & !n))
        }
        None => {
            let d = e.newer("permissions_diff");
            if !d.is_object() {
                return None;
            }
            (named(&d["added"]), named(&d["removed"]))
        }
    };
    (!c.0.is_empty() || !c.1.is_empty()).then_some(c)
}

fn both<T: PartialEq>(e: &E, key: &str, read: impl Fn(&Value) -> Option<T>) -> Option<(T, T)> {
    let (o, n) = e.change(key)?;
    let (o, n) = (read(o)?, read(n)?);
    (o != n).then_some((o, n))
}

/// `readOptionalTextChange`: null or text on both sides, and different.
fn optional_text(e: &E, key: &str) -> Option<(Option<String>, Option<String>)> {
    let (o, n) = e.change(key)?;
    let ok = |v: &Value| v.is_null() || v.is_string();
    if !ok(o) || !ok(n) {
        return None;
    }
    let (o, n) = (string(o), string(n));
    (o != n).then_some((o, n))
}

/// up / down, or for the member list also set / reset.
fn direction(b: f64, a: f64) -> &'static str {
    if a > b { "up" } else { "down" }
}

fn member_list_position(e: &E) -> Option<&'static str> {
    let (o, n) = e.change("hoist_position")?;
    let (b, a) = (number(o), number(n));
    if (!o.is_null() && b.is_none()) || (!n.is_null() && a.is_none()) {
        return None;
    }
    match (b, a) {
        (None, None) => None,
        (None, Some(_)) => Some("set"),
        (Some(_), None) => Some("reset"),
        (Some(b), Some(a)) if b == a => None,
        (Some(b), Some(a)) => Some(direction(b, a)),
    }
}

pub fn create(e: &E) -> Presented {
    let perms = flags_in(bitfield(e.newer("permissions")).unwrap_or(0));
    let mut rows = Vec::new();
    if !perms.is_empty() {
        rows.push(row("permissions-granted", Tone::Add, "Granted {permissions}", vec![("permissions", Ph::Perms(perms))]));
    }
    if let Some(c) = color(e.newer("color")).filter(|c| *c > 0) {
        rows.push(row("color", Tone::Neutral, "Set the role color to {color}", vec![("color", Ph::Color(c))]));
    }
    if boolean(e.newer("hoist")) == Some(true) {
        rows.push(row("hoist", Tone::Add, "Showed members with this role in their own section in the member list", vec![]));
    }
    if boolean(e.newer("mentionable")) == Some(true) {
        rows.push(row("mentionable", Tone::Add, "Allowed all members to mention this role", vec![]));
    }
    if let Some(em) = string(e.newer("unicode_emoji")) {
        rows.push(row("unicode-emoji", Tone::Add, "Set the role icon to {emoji}", vec![("emoji", Ph::Name(em))]));
    }
    if string(e.newer("icon_hash")).is_some() {
        rows.push(row("icon-hash", Tone::Add, "Set a custom role icon", vec![]));
    }
    with_rows(s("{actor} created the role {role}", vec![("actor", e.actor()), ("role", role(e, string(e.newer("name"))))]), rows)
}

pub fn update(e: &E) -> Presented {
    let name = both(e, "name", string);
    let perms = permission_changes(e);
    let col = both(e, "color", color);
    let hoist = both(e, "hoist", boolean).map(|c| c.1);
    let mentionable = both(e, "mentionable", boolean).map(|c| c.1);
    let emoji = optional_text(e, "unicode_emoji");
    let icon = optional_text(e, "icon_hash");
    let list_move = both(e, "position", number).map(|(b, a)| direction(b, a));
    let member_pos = member_list_position(e);
    let actor = e.actor();
    let r = role(e, string(e.newer("name")).or_else(|| string(e.option("role_name"))));
    let count = [name.is_some(), perms.is_some(), col.is_some(), hoist.is_some(), mentionable.is_some(), emoji.is_some(), icon.is_some(), list_move.is_some(), member_pos.is_some()].iter().filter(|x| **x).count();
    if count == 1 {
        if let Some(d) = list_move {
            let msg = if d == "up" { "{actor} moved the role {role} up in the role list" } else { "{actor} moved the role {role} down in the role list" };
            return only(s(msg, vec![("actor", actor), ("role", r)]));
        }
        if let Some(d) = member_pos {
            let msg = match d {
                "set" => "{actor} set a member list position for the role {role}",
                "reset" => "{actor} reset the member list position for the role {role}",
                "up" => "{actor} moved the role {role} up in the member list",
                _ => "{actor} moved the role {role} down in the member list",
            };
            return only(s(msg, vec![("actor", actor), ("role", r)]));
        }
        if let Some((o, n)) = &name {
            return only(s("{actor} renamed the role {oldName} to {newName}", vec![("actor", actor), ("oldName", Ph::Name(o.clone())), ("newName", Ph::Name(n.clone()))]));
        }
    }
    let mut rows = Vec::new();
    if let Some((o, n)) = name {
        rows.push(row("name", Tone::Neutral, "Changed the name from {oldName} to {newName}", vec![("oldName", Ph::Name(o)), ("newName", Ph::Name(n))]));
    }
    if let Some((added, removed)) = perms {
        if !added.is_empty() {
            rows.push(row("permissions-granted", Tone::Add, "Granted {permissions}", vec![("permissions", Ph::Perms(added))]));
        }
        if !removed.is_empty() {
            rows.push(row("permissions-revoked", Tone::Remove, "Revoked {permissions}", vec![("permissions", Ph::Perms(removed))]));
        }
    }
    if let Some((b, a)) = col {
        rows.push(if b == 0 {
            row("color", Tone::Add, "Set the role color to {color}", vec![("color", Ph::Color(a))])
        } else if a == 0 {
            row("color", Tone::Remove, "Removed the role color", vec![])
        } else {
            row("color", Tone::Neutral, "Changed the role color from {oldColor} to {newColor}", vec![("oldColor", Ph::Color(b)), ("newColor", Ph::Color(a))])
        });
    }
    if let Some(h) = hoist {
        rows.push(if h { row("hoist", Tone::Add, "Started displaying members with this role separately", vec![]) } else { row("hoist", Tone::Remove, "Stopped displaying members with this role separately", vec![]) });
    }
    if let Some(m) = mentionable {
        rows.push(if m { row("mentionable", Tone::Add, "Allowed all members to mention this role", vec![]) } else { row("mentionable", Tone::Remove, "Limited mentions of this role to members with permission to mention any role", vec![]) });
    }
    match emoji {
        Some((_, None)) => rows.push(row("unicode-emoji", Tone::Remove, "Removed the role icon", vec![])),
        Some((None, Some(n))) => rows.push(row("unicode-emoji", Tone::Add, "Set the role icon to {emoji}", vec![("emoji", Ph::Name(n))])),
        Some((Some(o), Some(n))) => rows.push(row("unicode-emoji", Tone::Neutral, "Changed the role icon from {oldEmoji} to {newEmoji}", vec![("oldEmoji", Ph::Name(o)), ("newEmoji", Ph::Name(n))])),
        None => {}
    }
    match icon {
        Some((_, None)) => rows.push(row("icon-hash", Tone::Remove, "Removed the custom role icon", vec![])),
        Some((None, Some(_))) => rows.push(row("icon-hash", Tone::Add, "Set a custom role icon", vec![])),
        Some((Some(_), Some(_))) => rows.push(row("icon-hash", Tone::Neutral, "Changed the custom role icon", vec![])),
        None => {}
    }
    if let Some(d) = list_move {
        rows.push(row("position", Tone::Neutral, if d == "up" { "Moved the role up in the role list" } else { "Moved the role down in the role list" }, vec![]));
    }
    if let Some(d) = member_pos {
        let msg = match d {
            "set" => "Set a member list position for the role",
            "reset" => "Reset the member list position for the role",
            "up" => "Moved the role up in the member list",
            _ => "Moved the role down in the member list",
        };
        rows.push(row("hoist-position", Tone::Neutral, msg, vec![]));
    }
    with_rows(s("{actor} updated the role {role}", vec![("actor", actor), ("role", r)]), rows)
}

pub fn delete(e: &E) -> Presented {
    let perms = flags_in(bitfield(e.old("permissions")).unwrap_or(0));
    let rows = if perms.is_empty() { vec![] } else { vec![row("permissions", Tone::Remove, "The role granted {permissions}", vec![("permissions", Ph::Perms(perms))])] };
    with_rows(s("{actor} deleted the role {role}", vec![("actor", e.actor()), ("role", role(e, string(e.old("name"))))]), rows)
}
