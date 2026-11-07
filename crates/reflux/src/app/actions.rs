//! Message actions the shell queues: typing notices, read-state acks, reactions, edits, deletes
//! and pins. Each applies to the store first where Fluxer does (optimistic), then calls the API
//! and undoes the local change if the call fails.

use std::collections::HashMap;

use std::future::Future;
use std::sync::Arc;

use reflux_model::{
    ChannelType, DispatchEvent, GatewayPayload, GuildSubscription, LazyRequest, MessageAck, MessageDelete, MessageReaction, OpCode,
    ReactionEmoji, Snowflake,
};
use reflux_net::{Command, ConnectionState};

use super::{Instance, Route, now_ms, spawn, update};
use crate::State;
use crate::shell::Intent;
use crate::i18n::t;

/// `ACK_BATCH_DELAY_MS`: automatic acks wait this long (mentions go at once).
const ACK_DELAY_MS: u64 = 3000;

/// Acks waiting to be sent: channel → (message, due at ms).
pub type PendingAcks = HashMap<Snowflake, (Snowflake, u64)>;

/// Runs a message intent; others are the caller's.
pub(super) fn run(s: &mut State, i: Intent) {
    let Some(inst) = s.app.instance.clone() else { return };
    match i {
        Intent::Typing(channel) => spawn(async move {
            if let Err(e) = inst.rest.typing(channel).await {
                eprintln!("reflux: typing in {channel}: {e}");
            }
        }),
        Intent::MarkUnread { channel, message } => {
            // Read through the message before it, by hand, and stop auto-acking until reopened.
            let before = Snowflake(message.0.saturating_sub(1));
            ack_locally(s, channel, before, true);
            s.app.shell.hold_ack = Some(channel);
            s.app.shell.sticky_unread = None;
            s.app.acks.remove(&channel);
            spawn(async move {
                if let Err(e) = inst.rest.ack_message(channel, before, true).await {
                    eprintln!("reflux: mark {channel} unread: {e}");
                }
            });
        }
        Intent::React { channel, message, emoji, add } => {
            react_locally(s, channel, message, &emoji, add);
            spawn(async move {
                if let Err(e) = inst.rest.set_reaction(channel, message, &emoji, add).await {
                    eprintln!("reflux: reaction on {message}: {e}");
                    update(move |s| react_locally(s, channel, message, &emoji, !add));
                }
            });
        }
        Intent::Edit { channel, message, content } => {
            let Some(old) = s.app.store.messages(channel).iter().find(|m| m.id == message).cloned() else { return };
            let mut new = old.clone();
            new.content = content.clone();
            s.app.store.apply(&DispatchEvent::MessageUpdate(Box::new(new)), now_ms());
            spawn(async move {
                let r = inst.rest.edit_message(channel, message, &content).await;
                update(move |s| match r {
                    Ok(m) => s.app.store.apply(&DispatchEvent::MessageUpdate(Box::new(m)), now_ms()),
                    Err(e) => {
                        eprintln!("reflux: edit {message}: {e}");
                        s.app.store.apply(&DispatchEvent::MessageUpdate(Box::new(old)), now_ms());
                    }
                });
            });
        }
        Intent::Delete { channel, message } => spawn(async move {
            // Not optimistic, as in Fluxer: the message goes when the server says so.
            match inst.rest.delete_message(channel, message).await {
                Ok(()) => update(move |s| {
                    let d = MessageDelete { id: message, channel_id: channel, ..Default::default() };
                    s.app.store.apply(&DispatchEvent::MessageDelete(Box::new(d)), now_ms());
                }),
                Err(e) => eprintln!("reflux: delete {message}: {e}"),
            }
        }),
        Intent::Pin { channel, message, pin } => spawn(async move {
            // The gateway's MESSAGE_UPDATE flips `pinned`.
            if let Err(e) = inst.rest.set_pinned(channel, message, pin).await {
                eprintln!("reflux: pin {message}: {e}");
            }
        }),
        Intent::OpenDm(user) => with_dm(s, inst, user, |s, c| s.app.shell.select_guild(None, Some(c))),
        Intent::DmCall(user, video) => with_dm(s, inst, user, move |s, c| {
            s.app.shell.select_guild(None, Some(c));
            crate::shell::call::request_call(&s.app.store, &mut s.app.shell, c, video);
        }),
        Intent::InviteToGuild { user, guild, channel } => {
            let store = &s.app.store;
            let name = store.guilds.get(&guild).and_then(|g| g.info.as_ref()).map_or_else(String::new, |i| i.name.clone());
            // `getInviteCapability`: a vanity link when you can't create invites.
            let vanity = (!store.permissions(channel).contains(reflux_model::Permissions::CREATE_INSTANT_INVITE))
                .then(|| store.guilds.get(&guild).and_then(|g| g.info.as_ref()?.vanity_url_code.clone()))
                .flatten();
            let base = s.app.shell.invite_base.trim_end_matches('/').to_string();
            spawn(async move {
                let code = match vanity {
                    Some(v) => Ok(v),
                    None => inst
                        .rest
                        .request::<serde_json::Value, _>(reqwest::Method::POST, &format!("/channels/{channel}/invites"), Some(&serde_json::json!({ "max_age": 604_800, "max_uses": 0, "temporary": false })))
                        .await
                        .map(|v| v["code"].as_str().unwrap_or_default().to_string()),
                };
                let Ok(code) = code else { return eprintln!("reflux: invite to {guild}: {code:?}") };
                update(move |s| {
                    let Some(inst) = s.app.instance.clone() else { return };
                    with_dm(s, inst, user, move |s, c| {
                        super::chat::send(s, c, format!("{base}/{code}"), None, vec![]);
                        crate::desktop::toast(&mut s.app.shell.desktop, crate::i18n::tf("Invite sent for {guildName}", &[("guildName", &name)]), true);
                    })
                });
            });
        }
        Intent::CloseDm(channel) => call(inst, move |i| async move { i.rest.close_channel(channel).await }, "close DM"),
        Intent::AddFriend(user) => call(inst, move |i| async move { i.rest.add_friend(user).await.map(drop) }, "add friend"),
        Intent::RemoveRelationship(user) => call(inst, move |i| async move { i.rest.remove_relationship(user).await }, "remove relationship"),
        Intent::Block(user) => call(inst, move |i| async move { i.rest.block(user).await.map(drop) }, "block"),
        Intent::SetNote { user, note } => call(inst, move |i| async move { i.rest.set_note(user, &note).await }, "set note"),
        Intent::LeaveGuild(guild) => call(inst, move |i| async move { i.rest.leave_guild(guild).await }, "leave guild"),
        Intent::FriendRequest(tag) => {
            let Some((name, disc)) = crate::shell::friends::parse_tag(&tag).map(|(n, d)| (n.to_string(), d.to_string())) else { return };
            spawn(async move {
                let r = inst.rest.send_friend_request(&name, &disc).await;
                update(move |s| {
                    let sh = &mut s.app.shell;
                    sh.add_friend_result = Some(match r {
                        Ok(_) => {
                            sh.add_friend = rune::TextEdit::new("");
                            Ok(())
                        }
                        Err(reflux_net::Error::Api { error, .. }) if error.code == "NO_USERS_WITH_FLUXERTAG_EXIST" => {
                            Err(t("No user found with that username.").into())
                        }
                        Err(reflux_net::Error::Api { error, .. }) => Err(error.message),
                        Err(e) => Err(e.to_string()),
                    });
                });
            });
        }
        Intent::ResendVerification => call(inst, |i| async move { i.rest.resend_verification().await }, "resend verification email"),
        Intent::SetDateOfBirth(date) => spawn(async move {
            let body = serde_json::json!({ "date_of_birth": date });
            let r = inst.rest.request::<serde_json::Value, _>(reqwest::Method::PUT, "/users/@me/date-of-birth", Some(&body)).await;
            update(move |s| {
                s.app.shell.birthday.submitting = false;
                match r {
                    Ok(_) => {
                        if let Some(me) = &mut s.app.store.me {
                            me.date_of_birth_required = false;
                        }
                    }
                    // The server removes the account: sign out without telling it, and refuse
                    // sign-up here for a day.
                    Err(e) if e.code() == Some("ACCOUNT_BELOW_MINIMUM_AGE") => {
                        crate::login::mark_age_blocked(s);
                        s.app.saved.token = None;
                        super::sign_out(s, None);
                        s.login.age_removed = true;
                    }
                    Err(e) => s.app.shell.birthday.error = Some(crate::login::Errors::from_error(&e).form.unwrap_or_else(|| e.to_string())),
                }
            });
        }),
        Intent::MarkRead(acks) => {
            for &(c, m) in &acks {
                ack_locally(s, c, m, false);
                s.app.acks.remove(&c);
            }
            for chunk in acks.chunks(100).map(<[_]>::to_vec) {
                call(inst.clone(), move |i| async move { i.rest.ack_bulk(&chunk).await }, "mark as read");
            }
        }
        _ => {}
    }
}

/// Runs a call whose effects arrive over the gateway; failures are logged.
fn call<F, Fut>(inst: Arc<Instance>, f: F, what: &'static str)
where
    F: FnOnce(Arc<Instance>) -> Fut + Send + 'static,
    Fut: Future<Output = reflux_net::Result<()>> + Send,
{
    spawn(async move {
        if let Err(e) = f(inst).await {
            eprintln!("reflux: {what}: {e}");
        }
    });
}

/// Keeps the lazy member list subscription (opcode 14) on the open guild channel and the rows
/// on screen, like `MemberSidebar.applyChannelSubscription` (the first one per guild in a
/// session asks for `sync: true`). Closing the list sends nothing, as in Fluxer.
pub(super) fn subscribe_member_list(s: &mut State) {
    let sh = &s.app.shell;
    let want = match (sh.guild, sh.channel.and_then(|c| s.app.store.channels.get(&c))) {
        (Some(g), Some(c)) if sh.member_list && c.guild_id == Some(g) => Some((g, c.id, crate::shell::member_ranges(sh.member_rows.clone()))),
        _ => None,
    };
    if want.is_none() || want == s.app.member_sub || s.app.connection != Some(ConnectionState::Connected) {
        return;
    }
    let Some((guild, channel, ranges)) = want.clone() else { return };
    let Some(gw) = &s.app.gateway else { return };
    let sync = (!s.app.member_synced.contains(&guild)).then_some(true);
    let sub = GuildSubscription { active: Some(true), sync, member_list_channels: [(channel, ranges)].into(), ..Default::default() };
    let req = LazyRequest { subscriptions: [(guild, sub)].into() };
    if gw.send(Command::Send(GatewayPayload::command(OpCode::LAZY_REQUEST, req))).is_ok() {
        s.app.member_sub = want;
        s.app.member_synced.insert(guild);
    }
}

fn react_locally(s: &mut State, channel: Snowflake, message: Snowflake, emoji: &ReactionEmoji, add: bool) {
    let r = MessageReaction { user_id: s.app.store.my_id(), channel_id: channel, message_id: message, emoji: emoji.clone(), ..Default::default() };
    let ev = if add { DispatchEvent::MessageReactionAdd(Box::new(r)) } else { DispatchEvent::MessageReactionRemove(Box::new(r)) };
    s.app.store.apply(&ev, now_ms());
}

fn ack_locally(s: &mut State, channel: Snowflake, message: Snowflake, manual: bool) {
    let a = MessageAck { channel_id: channel, message_id: message, manual, ..Default::default() };
    s.app.store.apply(&DispatchEvent::MessageAck(a), now_ms());
}

/// Every frame: acks the open channel while its newest message is on screen (locally at once,
/// on the server after [`ACK_DELAY_MS`]), and sends acks that are due. True when the store
/// changed. `canAutoAck`: only while the window is focused (headless runs count as focused) and
/// the media viewer is closed.
pub(super) fn tick(s: &mut State) -> bool {
    if s.app.route != Route::Main {
        s.app.acks.clear();
        return false;
    }
    let mut changed = false;
    if let Some(inst) = &s.app.instance
        && s.app.shell.web_base.is_empty()
    {
        s.app.shell.web_base = inst.discovery.endpoints.webapp.clone();
        s.app.shell.marketing_base = (!inst.discovery.features.self_hosted).then(|| inst.discovery.endpoints.marketing.clone());
        s.app.shell.media.set_base(&inst.discovery.endpoints.media);
        s.app.shell.invite_base = inst.discovery.endpoints.invite.clone();
        s.app.shell.gift_base = inst.discovery.endpoints.gift.clone();
        s.app.shell.static_base = inst.discovery.endpoints.static_cdn.clone();
        s.app.shell.username_sign_in = inst.discovery.features.account_identity == "username";
        s.app.shell.bluesky = inst.discovery.services.bluesky_enabled;
        s.app.shell.media_ui.age_geos = inst.discovery.age_policy.geos.iter().map(|g| (g.country_code.clone(), g.region_code.clone())).collect();
        crate::shell::expr::fetch(&mut s.app.shell, crate::shell::attachments::GEO, "/ip");
        s.app.shell.media_ui.hidden_badges = inst.discovery.hidden_built_in_guild_badges.clone();
        let gif = &inst.discovery.gif;
        // Picker frecency, favorites and skin tone, once per sign-in.
        if !s.app.shell.expr.persist {
            s.app.shell.expr.saved = crate::shell::expr::load();
            s.app.shell.expr.persist = true;
        }
        s.app.shell.gif_provider = if inst.discovery.services.gif_enabled { gif.display_name.clone() } else { String::new() };
        s.app.shell.social.api_base = inst.rest.base().to_string();
    }
    // Timed mutes run out locally (Fluxer's mute timers).
    if crate::social::expire_mutes(&mut s.app.store, &crate::social::iso(now_ms())) {
        changed = true;
    }
    if let Some(inst) = &s.app.instance {
        let traits = s.app.store.me.as_ref().map_or(&[][..], |m| &m.traits[..]);
        s.app.shell.limits = crate::shell::upload::resolve(&inst.discovery.limits, traits);
    }
    let now = now_ms();
    let sh = &s.app.shell;
    let can_ack = (s.desktop.focused || rune::is_headless()) && sh.media_ui.viewer.is_none();
    if let Some(ch) = sh.channel.filter(|&c| can_ack && sh.at_bottom && sh.hold_ack != Some(c) && s.app.store.has_unread(c)) {
        let store = &s.app.store;
        let last = store.channels.get(&ch).and_then(|c| c.last_message_id);
        let loaded = store.messages(ch).last().map(|m| m.id);
        // Only what's loaded has been seen.
        if let (Some(last), Some(loaded)) = (last, loaded)
            && loaded >= last
        {
            let ack = store.read_states.get(&ch).and_then(|r| r.last_message_id).unwrap_or_default();
            let me = store.my_id();
            let first = store.messages(ch).iter().find(|m| m.id > ack && m.author.id != me).map(|m| m.id);
            let delay = if store.mention_count(ch) > 0 { 0 } else { ACK_DELAY_MS };
            if let Some(first) = first
                && s.app.shell.sticky_unread.is_none_or(|(c, _)| c != ch)
            {
                s.app.shell.sticky_unread = Some((ch, first));
            }
            ack_locally(s, ch, last, false);
            changed = true;
            let pending = s.app.acks.get(&ch).copied();
            s.app.acks.insert(ch, (last, pending.map_or(now + delay, |(_, d)| d.min(now + delay))));
            if pending.is_none() && delay > 0 {
                // Wake a frame when it's due, in case nothing else does.
                spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    update(|_| {});
                });
            }
        }
    }
    let due: Vec<_> = s.app.acks.iter().filter(|(_, (_, at))| *at <= now).map(|(&c, &(m, _))| (c, m)).collect();
    let Some(inst) = s.app.instance.clone() else { return changed };
    for (channel, message) in due {
        s.app.acks.remove(&channel);
        let inst = inst.clone();
        spawn(async move {
            if let Err(e) = inst.rest.ack_message(channel, message, false).await {
                eprintln!("reflux: ack {channel}: {e}");
            }
        });
    }
    changed
}

/// Runs `then` with the DM channel of `user`, opening one first if there's none.
fn with_dm(s: &mut State, inst: std::sync::Arc<super::Instance>, user: Snowflake, then: impl FnOnce(&mut State, Snowflake) + Send + 'static) {
    let me = s.app.store.my_id();
    let existing = s.app.store.private_channels().into_iter().find(|c| c.kind == ChannelType::DM && c.recipients.iter().any(|u| u.id == user) && user != me).map(|c| c.id);
    match existing {
        Some(c) => then(s, c),
        None => spawn(async move {
            match inst.rest.open_dm(user).await {
                Ok(c) => update(move |s| {
                    let id = c.id;
                    s.app.store.apply(&DispatchEvent::ChannelCreate(Box::new(c)), now_ms());
                    then(s, id);
                }),
                Err(e) => eprintln!("reflux: open DM with {user}: {e}"),
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_acks_the_open_channel_at_the_bottom() {
        let demo = crate::shell::demo::Demo::new();
        let mut s = State::default();
        let ch = demo.shell.channel.expect("demo opens #general");
        (s.app.store, s.app.route) = (demo.store, Route::Main);
        s.app.shell.channel = Some(ch);
        s.desktop.focused = true;
        assert!(s.app.store.has_unread(ch));

        s.app.shell.at_bottom = false;
        assert!(!tick(&mut s) && s.app.store.has_unread(ch), "scrolled up: no ack");
        s.desktop.focused = false;
        s.app.shell.at_bottom = true;
        assert!(!tick(&mut s) && s.app.store.has_unread(ch), "window unfocused: no ack");
        s.desktop.focused = true;

        s.app.shell.at_bottom = true;
        assert!(tick(&mut s));
        assert!(!s.app.store.has_unread(ch), "read locally at once");
        assert!(s.app.acks.contains_key(&ch), "server ack queued");
        assert_eq!(s.app.shell.sticky_unread.map(|u| u.0), Some(ch), "NEW line kept");

        s.app.shell.hold_ack = Some(ch);
        ack_locally(&mut s, ch, Snowflake(1), true);
        assert!(!tick(&mut s) && s.app.store.has_unread(ch), "marked unread by hand: not re-acked");
    }
}
