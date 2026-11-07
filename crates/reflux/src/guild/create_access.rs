//! `ChannelCreateAccessStep`: the second step of creating a private channel, picking the roles
//! and members that may view it.

use reflux_model::{ChannelType, Permissions, Snowflake};
use reflux_store::Store;
use rune::{El, col, row, text};
use serde_json::{Value, json};

use super::channel::search_hits;
use super::settings::{color_dot, sorted_roles};
use crate::i18n::t;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::Checkbox;

const MEMBER_LIMIT: usize = 25;

/// The picked `"type:id"` keys (`form.data["access"]`).
pub fn picked(st: &ShellState) -> Vec<String> {
    st.form.data.get("access").and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
}

/// `buildPrivateChannelOverwrites`: @everyone loses View channel; the picks (and you) get it, plus
/// Connect in voice.
pub fn overwrites(guild: Snowflake, kind: ChannelType, picks: &[String], me: Snowflake) -> Value {
    let access = if kind == ChannelType::GUILD_VOICE { Permissions::VIEW_CHANNEL | Permissions::CONNECT } else { Permissions::VIEW_CHANNEL };
    let mut picks = picks.to_vec();
    let mine = format!("1:{me}");
    if !picks.contains(&mine) {
        picks.push(mine);
    }
    let mut v = vec![json!({ "id": guild.to_string(), "type": 0, "allow": "0", "deny": Permissions::VIEW_CHANNEL.bits().to_string() })];
    v.extend(picks.iter().filter_map(|k| {
        let (ty, id) = k.split_once(':')?;
        Some(json!({ "id": id, "type": ty.parse::<u8>().ok()?, "allow": access.bits().to_string(), "deny": "0" }))
    }));
    Value::Array(v)
}

fn toggle(st: &mut ShellState, key: &str) {
    let mut p = picked(st);
    match p.iter().position(|k| k == key) {
        Some(i) => {
            p.remove(i);
        }
        None => p.push(key.to_string()),
    }
    st.form.data.insert("access".into(), json!(p));
}

fn heading(s: &str, p: &Palette) -> El {
    text(s.to_string()).font(font(12.0, 600.0).lh(16.0)).text_case(rune::TextCase::Uppercase).color(p.text_tertiary)
}

pub fn view<S: 'static>(store: &Store, st: &ShellState, guild: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let q = st.form.value("access-search", "").trim().to_lowercase();
    let hit = |name: &str| q.is_empty() || name.to_lowercase().contains(&q);
    let chosen = picked(st);
    let check = |key: String, lead: El, name: String| {
        let on = chosen.contains(&key);
        row()
            .w_full()
            .gap(8.0)
            .py(4.0)
            .center_y()
            .cursor(rune::Cursor::Pointer)
            .on_press(move |s: &mut S| toggle(acc(s), &key))
            .child(Checkbox::<S>::new(on).size(18.0).el(p))
            .child(lead)
            .child(text(name).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).truncate())
    };
    let roles: Vec<El> = sorted_roles(store, guild).into_iter().filter(|r| r.id != guild && hit(&r.name)).map(|r| check(format!("0:{}", r.id), color_dot(r.color, p), r.name.clone())).collect();
    let mut members: Vec<(String, String)> = store.guilds.get(&guild).map(|gs| gs.members.values().map(|m| (m.user.id.to_string(), m.nick.clone().or_else(|| m.user.global_name.clone()).unwrap_or_else(|| m.user.username.clone()))).collect()).unwrap_or_default();
    members.extend(search_hits(st));
    members.retain(|m| hit(&m.1));
    members.sort_by_key(|m| m.1.to_lowercase());
    members.dedup_by(|a, b| a.0 == b.0);
    let members: Vec<El> = members
        .into_iter()
        .take(MEMBER_LIMIT)
        .map(|(id, name)| {
            let uid = id.parse().unwrap_or(Snowflake(0));
            check(format!("1:{id}"), crate::shell::avatar(store, st, uid, 24.0, None, p.background_secondary, p), name)
        })
        .collect();
    let empty = roles.is_empty() && members.is_empty();
    let mut list = col().w_full().gap(16.0);
    if !roles.is_empty() {
        list = list.child(col().w_full().gap(4.0).child(heading(t("Roles"), p)).children(roles));
    }
    if !members.is_empty() {
        list = list.child(col().w_full().gap(4.0).child(heading(t("Members"), p)).children(members));
    }
    if empty {
        list = list.child(text(t("No matching members or roles")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted));
    }
    col()
        .w_full()
        .gap(12.0)
        .child(super::field(st, acc, "access-search", "", "", t("Search members or roles"), p))
        .child(text(t("Only the members and roles you pick, plus admins, can view this channel.")).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted).wrap())
        .child(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_overwrites_add_you() {
        let v = overwrites(Snowflake(1), ChannelType::GUILD_VOICE, &["0:5".into()], Snowflake(9));
        let a = (Permissions::VIEW_CHANNEL | Permissions::CONNECT).bits().to_string();
        assert_eq!(v, json!([
            { "id": "1", "type": 0, "allow": "0", "deny": Permissions::VIEW_CHANNEL.bits().to_string() },
            { "id": "5", "type": 0, "allow": a, "deny": "0" },
            { "id": "9", "type": 1, "allow": a, "deny": "0" },
        ]));
        assert_eq!(overwrites(Snowflake(1), ChannelType::GUILD_TEXT, &["1:9".into()], Snowflake(9)).as_array().unwrap().len(), 2);
    }
}
