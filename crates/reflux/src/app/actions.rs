//! Message actions the shell queues: typing notices, read-state acks, reactions, edits, deletes
//! and pins. Each applies to the store first where Fluxer does (optimistic), then calls the API
//! and undoes the local change if the call fails.

use std::collections::HashMap;

use reflux_model::{DispatchEvent, MessageAck, MessageDelete, MessageReaction, ReactionEmoji, Snowflake};

use super::{Route, now_ms, spawn, update};
use crate::State;
use crate::shell::Intent;

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
        _ => {}
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
/// changed.
// shortcut: acks even while the window is unfocused; Fluxer waits for focus.
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
    }
    let now = now_ms();
    let sh = &s.app.shell;
    if let Some(ch) = sh.channel.filter(|&c| sh.at_bottom && sh.hold_ack != Some(c) && s.app.store.has_unread(c)) {
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
        assert!(s.app.store.has_unread(ch));

        s.app.shell.at_bottom = false;
        assert!(!tick(&mut s) && s.app.store.has_unread(ch), "scrolled up: no ack");

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
