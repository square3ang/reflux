//! `AuditLogMemberPresentation.ts`.

use super::{E, Ph, Presented, Row, Sentence, Tone, boolean, channel, color, number, only, reason, row, s, snowflake, string, strings, timestamp, with_rows};

/// `readMemberSubject`: (actor, target, acting on themselves).
fn subject(e: &E) -> (Ph, Ph, bool) {
    let target = e.target();
    let me = snowflake(&e.v["user_id"]);
    (e.actor(), Ph::User(target.clone().unwrap_or_default()), me.is_some() && me == target)
}

enum Change<T> {
    Set(T),
    Changed(T),
    Removed,
}

/// `readValueChange`.
fn value_change<T: PartialEq>(e: &E, key: &str, read: impl Fn(&serde_json::Value) -> Option<T>) -> Option<Change<T>> {
    let (o, n) = e.change(key)?;
    match (read(o), read(n)) {
        (a, b) if a == b => None,
        (None, Some(n)) => Some(Change::Set(n)),
        (Some(_), None) => Some(Change::Removed),
        (Some(_), Some(n)) => Some(Change::Changed(n)),
        (None, None) => None,
    }
}

fn toggle(e: &E, key: &str) -> Option<bool> {
    let (o, n) = e.change(key)?;
    match (boolean(o), boolean(n)) {
        (Some(a), Some(b)) if a != b => Some(b),
        _ => None,
    }
}

/// `readRoleChanges`: (added, removed) role IDs.
fn role_changes(e: &E) -> (Vec<String>, Vec<String>) {
    let (Some(prev), Some(next)) = (strings(e.old("roles")), strings(e.newer("roles"))) else { return Default::default() };
    let ok = |id: &String| snowflake(&serde_json::Value::String(id.clone())).is_some();
    let mut added: Vec<String> = next.iter().filter(|i| !prev.contains(i) && ok(i)).cloned().collect();
    let mut removed: Vec<String> = prev.iter().filter(|i| !next.contains(i) && ok(i)).cloned().collect();
    added.dedup();
    removed.dedup();
    (added, removed)
}

fn single(c: &(Vec<String>, Vec<String>)) -> Option<(bool, String)> {
    match (c.0.as_slice(), c.1.as_slice()) {
        ([a], []) => Some((true, a.clone())),
        ([], [r]) => Some((false, r.clone())),
        _ => None,
    }
}

fn role_rows(c: &(Vec<String>, Vec<String>)) -> Vec<Row> {
    let role = |id: &String| vec![("role", Ph::Role(id.clone(), None))];
    c.0.iter()
        .map(|id| row(format!("role_added_{id}"), Tone::Add, "Added the role {role}", role(id)))
        .chain(c.1.iter().map(|id| row(format!("role_removed_{id}"), Tone::Remove, "Removed the role {role}", role(id))))
        .collect()
}

fn temporary(e: &E) -> Vec<Row> {
    match e.change("temporary") {
        Some((o, n)) if boolean(o) == Some(true) && boolean(n) == Some(false) => vec![row("temporary", Tone::Neutral, "The member is no longer temporary", vec![])],
        _ => vec![],
    }
}

fn role_summary(actor: Ph, target: Ph, added: bool, role: Ph) -> Sentence {
    if added {
        s("{actor} gave {target} the role {role}", vec![("actor", actor), ("target", target), ("role", role)])
    } else {
        s("{actor} removed the role {role} from {target}", vec![("actor", actor), ("target", target), ("role", role)])
    }
}

/// `MemberChangeGroup`: (summary, rows shown under that summary, rows).
type Group = (Option<Sentence>, Vec<Row>, Vec<Row>);

fn nickname(e: &E, (actor, target, me): &(Ph, Ph, bool)) -> Option<Group> {
    let (o, n) = e.change("nick")?;
    let (o, n) = (string(o), string(n));
    if o == n {
        return None;
    }
    let (a, t) = (actor.clone(), target.clone());
    Some(match (o, n) {
        (None, Some(n)) => {
            let nick = Ph::Name(n);
            let sum = if *me { s("{actor} set their nickname to {nick}", vec![("actor", a), ("nick", nick.clone())]) } else { s("{actor} set the nickname of {target} to {nick}", vec![("actor", a), ("target", t), ("nick", nick.clone())]) };
            (Some(sum), vec![], vec![row("nick", Tone::Add, "Set the nickname to {nick}", vec![("nick", nick)])])
        }
        (Some(o), Some(n)) => {
            let (on, nn) = (Ph::Name(o), Ph::Name(n));
            let sum = if *me {
                s("{actor} changed their nickname from {oldNick} to {newNick}", vec![("actor", a), ("oldNick", on.clone()), ("newNick", nn.clone())])
            } else {
                s("{actor} changed the nickname of {target} from {oldNick} to {newNick}", vec![("actor", a), ("target", t), ("oldNick", on.clone()), ("newNick", nn.clone())])
            };
            (Some(sum), vec![], vec![row("nick", Tone::Neutral, "Changed the nickname from {oldNick} to {newNick}", vec![("oldNick", on), ("newNick", nn)])])
        }
        (Some(o), None) => {
            let nick = Ph::Name(o);
            let sum = if *me { s("{actor} removed their nickname {nick}", vec![("actor", a), ("nick", nick.clone())]) } else { s("{actor} removed the nickname {nick} from {target}", vec![("actor", a), ("target", t), ("nick", nick.clone())]) };
            (Some(sum), vec![], vec![row("nick", Tone::Remove, "Removed the nickname {nick}", vec![("nick", nick)])])
        }
        (None, None) => return None,
    })
}

fn roles(e: &E, (actor, target, _): &(Ph, Ph, bool)) -> Option<Group> {
    let c = role_changes(e);
    let rows = role_rows(&c);
    if rows.is_empty() {
        return None;
    }
    Some(match single(&c) {
        Some((added, id)) => (Some(role_summary(actor.clone(), target.clone(), added, Ph::Role(id, None))), vec![], rows),
        None => (Some(s("{actor} updated the roles of {target}", vec![("actor", actor.clone()), ("target", target.clone())])), rows.clone(), rows),
    })
}

fn timeout(e: &E, (actor, target, _): &(Ph, Ph, bool)) -> Option<Group> {
    let (o, n) = e.change("communication_disabled_until")?;
    let now = e.time();
    let (prev, next) = (timestamp(o), timestamp(n));
    let was = prev.is_some_and(|p| p > now);
    let (a, t) = (actor.clone(), target.clone());
    match next.filter(|n| *n > now) {
        Some(until) if !was => Some((
            Some(s("{actor} timed out {target} until {date}", vec![("actor", a), ("target", t), ("date", Ph::Date(until))])),
            vec![],
            vec![row("communication_disabled_until", Tone::Add, "Blocked the member from sending messages, reacting and joining voice until {date}", vec![("date", Ph::Date(until))])],
        )),
        Some(until) if prev != next => Some((
            Some(s("{actor} changed {target}'s block on sending messages, reacting and joining voice to end at {date}", vec![("actor", a), ("target", t), ("date", Ph::Date(until))])),
            vec![],
            vec![row("communication_disabled_until", Tone::Neutral, "Changed the block on sending messages, reacting and joining voice to end at {date}", vec![("date", Ph::Date(until))])],
        )),
        Some(_) => None,
        None if was => Some((Some(s("{actor} removed the timeout for {target}", vec![("actor", a), ("target", t)])), vec![], vec![row("communication_disabled_until", Tone::Remove, "Removed the timeout", vec![])])),
        None => None,
    }
}

/// `voiceStateGroup`: [applied summary, removed summary, applied row, removed row].
fn voice(e: &E, (actor, target, _): &(Ph, Ph, bool), key: &'static str, m: [&'static str; 4]) -> Option<Group> {
    let on = toggle(e, key)?;
    let vals = vec![("actor", actor.clone()), ("target", target.clone())];
    Some((Some(s(if on { m[0] } else { m[1] }, vals)), vec![], vec![if on { row(key, Tone::Add, m[2], vec![]) } else { row(key, Tone::Remove, m[3], vec![]) }]))
}

/// `profileGroup`: (key, value name, tones, summaries, rows) for set / changed / removed.
struct Profile {
    key: &'static str,
    value: Option<&'static str>,
    color: bool,
    tones: [Tone; 3],
    summaries: [&'static str; 3],
    rows: [&'static str; 3],
}

const PROFILES: [Profile; 5] = [
    Profile {
        key: "avatar_hash",
        value: None,
        color: false,
        tones: [Tone::Add, Tone::Neutral, Tone::Remove],
        summaries: ["{actor} set their community profile avatar", "{actor} changed their community profile avatar", "{actor} removed their community profile avatar"],
        rows: ["Added the member's community profile avatar", "Changed the member's community profile avatar", "Removed the member's community profile avatar"],
    },
    Profile {
        key: "banner_hash",
        value: None,
        color: false,
        tones: [Tone::Add, Tone::Neutral, Tone::Remove],
        summaries: ["{actor} set their community profile banner", "{actor} changed their community profile banner", "{actor} removed their community profile banner"],
        rows: ["Added the member's community profile banner", "Changed the member's community profile banner", "Removed the member's community profile banner"],
    },
    Profile {
        key: "bio",
        value: Some("text"),
        color: false,
        tones: [Tone::Add, Tone::Neutral, Tone::Remove],
        summaries: ["{actor} set their community profile bio to {text}", "{actor} changed their community profile bio to {text}", "{actor} removed their community profile bio"],
        rows: ["Set the member's community profile bio to {text}", "Changed the member's community profile bio to {text}", "Removed the member's community profile bio"],
    },
    Profile {
        key: "pronouns",
        value: Some("text"),
        color: false,
        tones: [Tone::Neutral, Tone::Neutral, Tone::Remove],
        summaries: ["{actor} set their community profile pronouns to {text}", "{actor} set their community profile pronouns to {text}", "{actor} removed their community profile pronouns"],
        rows: ["Set the member's community profile pronouns to {text}", "Set the member's community profile pronouns to {text}", "Removed the member's community profile pronouns"],
    },
    Profile {
        key: "accent_color",
        value: Some("color"),
        color: true,
        tones: [Tone::Neutral, Tone::Neutral, Tone::Remove],
        summaries: ["{actor} set their community profile accent color to {color}", "{actor} set their community profile accent color to {color}", "{actor} removed their community profile accent color"],
        rows: ["Set the member's community profile accent color to {color}", "Set the member's community profile accent color to {color}", "Removed the member's community profile accent color"],
    },
];

fn profile(e: &E, (actor, _, me): &(Ph, Ph, bool), spec: &Profile) -> Option<Group> {
    let (i, next) = if spec.color {
        match value_change(e, spec.key, color)? {
            Change::Set(n) => (0, Some(Ph::Color(n))),
            Change::Changed(n) => (1, Some(Ph::Color(n))),
            Change::Removed => (2, None),
        }
    } else {
        match value_change(e, spec.key, string)? {
            Change::Set(n) => (0, Some(Ph::Text(n))),
            Change::Changed(n) => (1, Some(Ph::Text(n))),
            Change::Removed => (2, None),
        }
    };
    let vals: Vec<(&'static str, Ph)> = spec.value.zip(next).into_iter().collect();
    let mut sum_vals = vec![("actor", actor.clone())];
    sum_vals.extend(vals.clone());
    Some(((*me).then(|| s(spec.summaries[i], sum_vals)), vec![], vec![row(spec.key, spec.tones[i], spec.rows[i], vals)]))
}

pub fn kick(e: &E) -> Presented {
    let (a, t, _) = subject(e);
    only(s("{actor} kicked {target}", vec![("actor", a), ("target", t)]))
}

pub fn prune(e: &E) -> Presented {
    only(s("{actor} pruned inactive members", vec![("actor", e.actor())]))
}

pub fn ban_add(e: &E) -> Presented {
    let (a, t, _) = subject(e);
    let duration = timestamp(e.newer("expires_at")).and_then(|exp| {
        let at = timestamp(e.newer("banned_at")).unwrap_or_else(|| e.time());
        let secs = ((exp - at) as f64 / 1000.0).round();
        (secs > 0.0).then_some(secs)
    });
    let deleted = number(e.option("delete_message_seconds")).filter(|s| *s > 0.0).or_else(|| number(e.option("delete_member_days")).filter(|d| d.fract() == 0.0 && *d > 0.0).map(|d| d * 86_400.0));
    let ban_reason = reason(e.newer("reason"));
    Presented {
        summary: match duration {
            None => s("{actor} banned {target}", vec![("actor", a), ("target", t)]),
            Some(d) => s("{actor} banned {target} for {duration}", vec![("actor", a), ("target", t), ("duration", Ph::Duration(d))]),
        },
        rows: deleted.map(|d| row("delete_messages", Tone::Remove, "Deleted {duration} of the member's recent message history", vec![("duration", Ph::Duration(d))])).into_iter().collect(),
        blocks: ban_reason.filter(|r| Some(r) != reason(&e.v["reason"]).as_ref()).map(|r| (true, r)).into_iter().collect(),
    }
}

pub fn ban_remove(e: &E) -> Presented {
    let (a, t, _) = subject(e);
    let mut rows = Vec::new();
    let moderator = snowflake(e.old("moderator_id")).filter(|m| Some(m.as_str()) != e.user_id());
    match (moderator, timestamp(e.old("banned_at"))) {
        (Some(m), Some(at)) => rows.push(row("original_ban", Tone::Neutral, "Originally banned by {user} on {date}", vec![("user", Ph::User(m)), ("date", Ph::Date(at))])),
        (None, Some(at)) => rows.push(row("original_ban", Tone::Neutral, "Originally banned on {date}", vec![("date", Ph::Date(at))])),
        (Some(m), None) => rows.push(row("original_ban", Tone::Neutral, "Originally banned by {user}", vec![("user", Ph::User(m))])),
        (None, None) => {}
    }
    if let Some(exp) = timestamp(e.old("expires_at")) {
        let msg = if exp > e.time() { "The ban was scheduled to expire on {date}" } else { "The ban had already expired on {date}" };
        rows.push(row("ban_expiry", Tone::Neutral, msg, vec![("date", Ph::Date(exp))]));
    }
    Presented { summary: s("{actor} unbanned {target}", vec![("actor", a), ("target", t)]), rows, blocks: reason(e.old("reason")).map(|r| (true, r)).into_iter().collect() }
}

pub fn update(e: &E) -> Presented {
    let sub = subject(e);
    let mut groups: Vec<Group> = [
        nickname(e, &sub),
        roles(e, &sub),
        timeout(e, &sub),
        voice(e, &sub, "mute", [
            "{actor} turned off {target}'s microphone in this community",
            "{actor} turned {target}'s microphone back on in this community",
            "Turned off the member's microphone in this community",
            "Turned the member's microphone back on in this community",
        ]),
        voice(e, &sub, "deaf", [
            "{actor} stopped {target} from hearing others in this community",
            "{actor} let {target} hear others in this community again",
            "Stopped the member from hearing others in this community",
            "Let the member hear others in this community again",
        ]),
    ]
    .into_iter()
    .flatten()
    .collect();
    groups.extend(PROFILES.iter().filter_map(|p| profile(e, &sub, p)));
    let temp = temporary(e);
    if let [(Some(sum), rows, _)] = groups.as_slice() {
        return with_rows(sum.clone(), rows.iter().cloned().chain(temp).collect());
    }
    let (a, t, me) = sub;
    let summary = if me { s("{actor} updated their community profile", vec![("actor", a)]) } else { s("{actor} updated {target}", vec![("actor", a), ("target", t)]) };
    with_rows(summary, groups.into_iter().flat_map(|g| g.2).chain(temp).collect())
}

pub fn role_update(e: &E) -> Presented {
    let (a, t, _) = subject(e);
    let c = role_changes(e);
    let temp = temporary(e);
    let Some((added, id)) = single(&c) else {
        let mut rows = role_rows(&c);
        rows.extend(temp);
        return with_rows(s("{actor} updated the roles of {target}", vec![("actor", a), ("target", t)]), rows);
    };
    let role = Ph::Role(id, string(e.option("role_name")));
    if a != Ph::System {
        return with_rows(role_summary(a, t, added, role), temp);
    }
    let msg = if added { "The system gave {target} the role {role}" } else { "The system removed the role {role} from {target}" };
    with_rows(s(msg, vec![("target", t), ("role", role)]), temp)
}

pub fn moved(e: &E) -> Presented {
    let (a, t, me) = subject(e);
    let old = snowflake(e.old("channel_id"));
    let Some(new) = snowflake(e.newer("channel_id")).or_else(|| snowflake(e.option("channel_id"))) else {
        return only(if me { s("{actor} moved to a different voice channel", vec![("actor", a)]) } else { s("{actor} moved {target} to a different voice channel", vec![("actor", a), ("target", t)]) });
    };
    only(match (old, me) {
        (None, true) => s("{actor} moved to {channel}", vec![("actor", a), ("channel", channel(new))]),
        (None, false) => s("{actor} moved {target} to {channel}", vec![("actor", a), ("target", t), ("channel", channel(new))]),
        (Some(o), true) => s("{actor} moved from {oldChannel} to {newChannel}", vec![("actor", a), ("oldChannel", channel(o)), ("newChannel", channel(new))]),
        (Some(o), false) => s("{actor} moved {target} from {oldChannel} to {newChannel}", vec![("actor", a), ("target", t), ("oldChannel", channel(o)), ("newChannel", channel(new))]),
    })
}

pub fn disconnect(e: &E) -> Presented {
    let (a, t, me) = subject(e);
    only(match (snowflake(e.old("channel_id")).or_else(|| snowflake(e.option("channel_id"))), me) {
        (None, true) => s("{actor} disconnected from voice", vec![("actor", a)]),
        (None, false) => s("{actor} disconnected {target} from voice", vec![("actor", a), ("target", t)]),
        (Some(c), true) => s("{actor} disconnected from {channel}", vec![("actor", a), ("channel", channel(c))]),
        (Some(c), false) => s("{actor} disconnected {target} from {channel}", vec![("actor", a), ("target", t), ("channel", channel(c))]),
    })
}

pub fn bot_add(e: &E) -> Presented {
    let (a, t, _) = subject(e);
    let msg = if boolean(e.option("temporary")) == Some(true) { "{actor} added the bot {target} as a temporary member" } else { "{actor} added the bot {target}" };
    only(s(msg, vec![("actor", a), ("target", t)]))
}
