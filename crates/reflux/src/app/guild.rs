//! Runs the community management calls [`crate::guild`] queues and applies their [`Then`].

use reflux_model::{DispatchEvent, Snowflake, UserGuildSettings};
use serde_json::Value;

use super::{now_ms, spawn, update};
use crate::State;
use crate::guild::{Call, Dialog, Then, describe};

pub(super) fn run(s: &mut State, c: Call) {
    let Some(inst) = s.app.instance.clone() else { return };
    let sh = &mut s.app.shell;
    // Answers only touch the form of the dialog they were sent from.
    let from = sh.dialog.clone();
    sh.form.busy = true;
    sh.form.error = None;
    spawn(async move {
        let r = inst.rest.request::<Value, Value>(c.method.clone(), &c.path, c.body.as_ref()).await;
        let reload = match (&r, &c.then) {
            (Ok(_), Then::Reload(_, path)) => Some(inst.rest.get::<Value>(path).await),
            _ => None,
        };
        update(move |s| {
            let sh = &mut s.app.shell;
            let same = sh.dialog == from;
            if same {
                sh.form.busy = false;
            }
            let v = match r {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("reflux: {} {}: {e}", c.method, c.path);
                    if same {
                        sh.form.error = Some(describe(&e));
                    }
                    return;
                }
            };
            if same {
                // A confirmed action is done.
                sh.form.confirm = None;
            }
            match c.then {
                Then::Nothing => {}
                Then::Close => {
                    if same {
                        sh.dialog = None;
                    }
                }
                Then::Keep(key) => {
                    if same {
                        sh.form.data.insert(key, v);
                    }
                }
                Then::Reload(key, _) => match reload {
                    Some(Ok(list)) if same => {
                        sh.form.data.insert(key, list);
                    }
                    Some(Err(e)) if same => sh.form.error = Some(describe(&e)),
                    _ => {}
                },
                Then::Enter => {
                    sh.dialog = None;
                    if let Some((g, ch)) = entered(&v) {
                        sh.select_guild(Some(g), ch);
                    }
                }
                Then::Select => {
                    let id = v.get("id").and_then(Value::as_str).and_then(|i| i.parse::<Snowflake>().ok());
                    match (&mut sh.dialog, id) {
                        (Some(Dialog::Guild { role, .. }), Some(id)) if same => *role = Some(id),
                        (Some(Dialog::Channel { target, .. }), Some(id)) if same => *target = Some(id),
                        _ => {}
                    }
                }
            }
        });
    });
}

/// The (guild, channel) a response leads to: an accepted invite, a created channel, or a created
/// guild (its system channel).
pub fn entered(v: &Value) -> Option<(Snowflake, Option<Snowflake>)> {
    let id = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).and_then(|s| s.parse::<Snowflake>().ok());
    if let Some(g) = v.get("guild").filter(|g| g.is_object()) {
        return Some((id(g, "id")?, v.get("channel").and_then(|c| id(c, "id"))));
    }
    if v.get("type").is_some() {
        return Some((id(v, "guild_id")?, id(v, "id")));
    }
    Some((id(v, "id")?, id(v, "system_channel_id")))
}

/// Applies a partial user guild settings object now and sends it; the response (or the
/// gateway) settles it, a failure restores the old settings.
pub(super) fn patch_settings(s: &mut State, guild: Option<Snowflake>, patch: Value) {
    let cur = s.app.store.guild_settings.get(&guild).cloned().unwrap_or_else(|| UserGuildSettings { guild_id: guild, ..Default::default() });
    let mut new = cur.clone();
    if !crate::settings::apply(&mut new, &patch) {
        return eprintln!("reflux: guild settings patch doesn't fit: {patch}");
    }
    s.app.store.apply(&DispatchEvent::UserGuildSettingsUpdate(new), now_ms());
    let Some(inst) = s.app.instance.clone() else { return };
    spawn(async move {
        let r = inst.rest.update_guild_settings(guild, &patch).await;
        update(move |s| match r {
            Ok(u) => s.app.store.apply(&DispatchEvent::UserGuildSettingsUpdate(u), now_ms()),
            Err(e) => {
                eprintln!("reflux: guild settings update: {e}");
                s.app.store.apply(&DispatchEvent::UserGuildSettingsUpdate(cur), now_ms());
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn entered_reads_invites_channels_and_guilds() {
        let s = |n: u64| Some(Snowflake(n));
        assert_eq!(entered(&json!({ "code": "x", "guild": { "id": "1" }, "channel": { "id": "2" } })), Some((Snowflake(1), s(2))));
        assert_eq!(entered(&json!({ "id": "5", "type": 0, "guild_id": "1" })), Some((Snowflake(1), s(5))));
        assert_eq!(entered(&json!({ "id": "1", "name": "x", "system_channel_id": "3" })), Some((Snowflake(1), s(3))));
        assert_eq!(entered(&json!({ "id": "1", "system_channel_id": null })), Some((Snowflake(1), None)));
        assert_eq!(entered(&json!(null)), None);
    }
}
