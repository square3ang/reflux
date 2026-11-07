//! The slash commands that report back (`CommandUtils.executeCommand`): `/nick` and `/msg`, and
//! the Fluxerbot notes they post ([`crate::shell::client_system`]).

use reflux_model::{ChannelType, DispatchEvent, Snowflake};
use reqwest::Method;
use serde_json::{Value, json};

use super::{now_ms, spawn, update};
use crate::State;
use crate::i18n::tf;
use crate::shell::Intent;
use crate::shell::client_system;

/// Adds a Fluxerbot note to `channel`.
pub fn post(s: &mut State, channel: Snowflake, content: String) {
    s.app.store.add_local_message(client_system::build(channel, content, now_ms()));
}

/// `/nick`: `updateProfile`, then "You changed your nickname in this community from **old** to
/// **new**." (`getCurrentUserNicknameLabels`: the nickname, else the display name).
pub fn nick(s: &mut State, guild: Snowflake, channel: Snowflake, nick: Option<String>) {
    let Some(inst) = s.app.instance.clone() else { return };
    let store = &s.app.store;
    let me = store.my_id();
    let fallback = store.display_name(None, me);
    let previous = store.member(guild, me).and_then(|m| m.nick.clone()).map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| fallback.clone());
    let new = nick.clone().unwrap_or(fallback);
    let path = format!("/guilds/{guild}/members/@me");
    spawn(async move {
        let r = inst.rest.request_with_reason::<Value, Value>(Method::PATCH, &path, Some(&json!({ "nick": nick })), None).await;
        update(move |s| match r {
            Ok(_) => {
                let text = tf("You changed your nickname in this community from **{previousNickname}** to **{newNickname}**.", &[("previousNickname", &previous), ("newNickname", &new)]);
                post(s, channel, text);
            }
            Err(e) => {
                eprintln!("reflux: /nick: {e}");
                s.app.shell.notice = Some((crate::i18n::t("Something went wrong").into(), crate::guild::describe(&e)));
            }
        });
    });
}

/// `/msg`: `ensureDMChannel`, send, then show the DM; a DM that can't open is noted in `from`.
pub fn msg(s: &mut State, from: Snowflake, user: Snowflake, message: String) {
    let Some(inst) = s.app.instance.clone() else { return };
    let me = s.app.store.my_id();
    let existing = s.app.store.private_channels().into_iter().find(|c| c.kind == ChannelType::DM && c.recipients.iter().any(|u| u.id == user) && user != me).map(|c| c.id);
    let send = move |s: &mut State, channel: Snowflake| {
        s.app.shell.intents.push(Intent::SendMessage { channel, content: message, reply: None, attachments: Vec::new() });
        s.app.shell.select_guild(None, Some(channel));
    };
    if let Some(c) = existing {
        return send(s, c);
    }
    spawn(async move {
        let r = inst.rest.open_dm(user).await;
        update(move |s| match r {
            Ok(c) => {
                let id = c.id;
                s.app.store.apply(&DispatchEvent::ChannelCreate(Box::new(c)), now_ms());
                send(s, id);
            }
            Err(e) => {
                eprintln!("reflux: /msg open DM with {user}: {e}");
                let name = s.app.store.users.get(&user).map_or_else(|| user.to_string(), |_| s.app.store.display_name(None, user));
                post(s, from, format!("Failed to send a message to **{name}**. They may have DMs disabled or you may be blocked."));
            }
        });
    });
}
