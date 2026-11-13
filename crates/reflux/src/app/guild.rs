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
    // A retry from the sudo prompt reports back there.
    let sudo = sh.account.sudo.as_ref().is_some_and(|x| x.call.path == c.path);
    spawn(async move {
        let r = inst.rest.request::<Value, Value>(c.method.clone(), &c.path, c.body.as_ref()).await;
        let reload = match (&r, &c.then) {
            (Ok(_), Then::Reload(_, path) | Then::Saved { reload: Some((_, path)), .. }) => Some(inst.rest.get::<Value>(path).await),
            _ => None,
        };
        // A DM opened to send into: send.
        let sent = match (&r, &c.then) {
            (Ok(v), Then::SendDm { content, .. }) => {
                let id = v.get("id").and_then(Value::as_str).unwrap_or_default();
                Some(inst.rest.request::<Value, Value>(reqwest::Method::POST, &format!("/channels/{id}/messages"), Some(&serde_json::json!({ "content": content }))).await)
            }
            _ => None,
        };
        let c_reload = match &c.then {
            Then::Saved { reload, .. } => reload.clone(),
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
                Err(reflux_net::Error::Api { error, .. }) if error.code == "SUDO_MODE_REQUIRED" => {
                    if same {
                        sh.form.busy = false;
                    }
                    let methods = Value::Object(error.extra.into_iter().collect());
                    return crate::settings::sudo::require(sh, c, &methods);
                }
                Err(e) => {
                    eprintln!("reflux: {} {}: {e}", c.method, c.path);
                    if let Then::Toast(_, err) = c.then {
                        return crate::desktop::toast(&mut sh.desktop, crate::i18n::t(err), false);
                    }
                    if sudo && let Some(x) = sh.account.sudo.as_mut() {
                        x.fail(describe(&e));
                    } else if same {
                        sh.form.error = Some(describe(&e));
                    }
                    return;
                }
            };
            if sudo {
                sh.account.sudo = None;
            }
            if same {
                // A confirmed action is done.
                sh.form.confirm = None;
            }
            match c.then {
                Then::Nothing => {}
                Then::Close => {
                    if same {
                        sh.dialog = None;
                        sh.close_account_modals();
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
                Then::SendDm { user, mark, .. } => match sent {
                    Some(Ok(_)) if same => {
                        let list = sh.form.data.entry(mark).or_insert_with(|| Value::Array(vec![]));
                        if let Some(a) = list.as_array_mut() {
                            a.push(Value::String(user.to_string()));
                        }
                    }
                    Some(Err(e)) if same => sh.form.error = Some(describe(&e)),
                    _ => {}
                },
                Then::Append(key, page) => {
                    if same {
                        let end = append(sh.form.data.entry(key.clone()).or_insert(Value::Null), v) < page;
                        if end {
                            sh.form.data.insert(format!("{key}-end"), Value::Bool(true));
                        }
                    }
                }
                Then::Enter => {
                    sh.dialog = None;
                    if let Some((g, ch)) = entered(&v) {
                        sh.select_guild(Some(g), ch);
                    }
                }
                Then::Saved { clear, .. } => {
                    // Our cached profiles (banner, bio, …) are stale after a profile save; cards fetch them again.
                    if c.path == "/users/@me" || c.path.ends_with("/members/@me") {
                        let me = s.app.store.my_id();
                        sh.account.profiles.retain(|(u, _), _| *u != me);
                    }
                    if same {
                        sh.close_account_modals();
                        sh.form.text.retain(|k, _| !k.starts_with(&clear));
                        sh.form.data.retain(|k, _| !k.starts_with(&clear));
                        match reload {
                            Some(Ok(v)) => {
                                if let Some((key, _)) = &c_reload {
                                    sh.form.data.insert(key.clone(), v);
                                }
                            }
                            Some(Err(e)) => sh.form.error = Some(describe(&e)),
                            None => {}
                        }
                    }
                }
                Then::Token => {
                    sh.close_account_modals();
                    if let Some(t) = v.get("token").and_then(Value::as_str) {
                        super::replace_token(s, t.to_owned());
                    }
                }
                Then::SignOut => super::sign_out(s, None),
                // An empty success text toasts only failures.
                Then::Browse(field) => {
                    if let Some(url) = v.get(field).and_then(Value::as_str) {
                        crate::settings::open_url(url);
                    }
                }
                Then::Toast(ok, _) => {
                    if !ok.is_empty() {
                        crate::desktop::toast(&mut sh.desktop, crate::i18n::t(ok), true)
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

/// Appends `new`'s arrays (or `new` itself, when an array) to `old`'s; returns the longest
/// array appended, so a short page ends paging.
fn append(old: &mut Value, new: Value) -> usize {
    match (old, new) {
        (Value::Array(o), Value::Array(n)) => {
            let len = n.len();
            o.extend(n);
            len
        }
        (Value::Object(o), Value::Object(n)) => n.into_iter().map(|(k, v)| append(o.entry(k).or_insert_with(|| Value::Array(vec![])), v)).max().unwrap_or(0),
        (o, n) => {
            let len = n.as_array().map_or(0, Vec::len);
            *o = n;
            len
        }
    }
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

    #[test]
    fn append_extends_pages() {
        let mut old = json!({ "entries": [1, 2], "users": [] });
        assert_eq!(append(&mut old, json!({ "entries": [3], "users": [7, 8] })), 2);
        assert_eq!(old, json!({ "entries": [1, 2, 3], "users": [7, 8] }));
        let mut list = json!([1]);
        assert_eq!(append(&mut list, json!([])), 0);
        assert_eq!(list, json!([1]));
    }
}
