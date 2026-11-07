use reflux_model::{DispatchEvent, Message, Snowflake as S};
use serde_json::{Value, json};

use super::*;

const ME: u64 = 5;
const G: u64 = 100;

fn ev(t: &str, d: Value) -> DispatchEvent {
    DispatchEvent::parse(t, &d).unwrap()
}

fn apply(s: &mut Store, t: &str, d: Value) {
    s.apply(&ev(t, d), 0);
}

fn role(id: u64, perms: u64, color: u32, position: i32) -> Value {
    json!({"id": id.to_string(), "name": "r", "permissions": perms.to_string(), "color": color, "position": position})
}

fn chan(id: u64, kind: u16, position: i32, parent: Option<u64>) -> Value {
    json!({"id": id.to_string(), "type": kind, "position": position, "parent_id": parent.map(|p| p.to_string())})
}

/// A store with one guild: @everyone can view; channel 104 hides itself, category 110 is hidden.
fn store() -> Store {
    let view = Permissions::VIEW_CHANNEL.bits();
    let deny_view = json!([{"id": G.to_string(), "type": 0, "allow": "0", "deny": view.to_string()}]);
    let mut hidden = chan(110, 4, 0, None);
    hidden["permission_overwrites"] = deny_view.clone();
    let mut secret = chan(104, 0, 9, Some(120));
    secret["permission_overwrites"] = deny_view;
    let mut s = Store::default();
    apply(
        &mut s,
        "READY",
        json!({
            "session_id": "s", "user": {"id": ME.to_string(), "username": "me"},
            "guilds": [{
                "id": G.to_string(), "properties": {"id": G.to_string(), "name": "G", "owner_id": "1"},
                "joined_at": "2020-01-01T00:00:00.000Z",
                "roles": [role(G, view, 0, 0), role(7, 0, 0xff0000, 2), role(8, 0, 0x00ff00, 5), role(9, 0, 0, 9)],
                "channels": [
                    chan(101, 0, 1, None), chan(102, 2, 0, None), chan(120, 4, 1, None),
                    chan(103, 0, 2, Some(120)), chan(105, 2, 0, Some(120)), chan(106, 0, 0, Some(120)),
                    secret, hidden, chan(111, 0, 0, Some(110)), chan(130, 15, 3, None),
                ],
                "members": [{"user": {"id": ME.to_string()}, "roles": ["7", "8", "9"], "nick": "Nick"}],
            }],
            "private_channels": [
                {"id": "300", "type": 1, "recipients": [{"id": "6", "username": "bob"}], "last_message_id": "310"},
                {"id": "301", "type": 1, "recipients": [{"id": "7", "username": "eve"}], "last_message_id": "320"},
            ],
            "read_states": [{"id": "101", "last_message_id": "1000", "mention_count": 0}],
        }),
    );
    s
}

fn msg(id: u64, channel: u64, author: u64) -> Value {
    json!({"id": id.to_string(), "channel_id": channel.to_string(), "author": {"id": author.to_string(), "username": "u"}})
}

fn ids(v: &[&Channel]) -> Vec<u64> {
    v.iter().map(|c| c.id.0).collect()
}

#[test]
fn channel_groups_sorted_and_filtered() {
    let s = store();
    let groups = s.channel_groups(S(G));
    assert_eq!(groups.len(), 2, "hidden category 110 is dropped with its child");
    assert_eq!(groups[0].category, None);
    assert_eq!(ids(&groups[0].text), [101, 130], "forum counts as text");
    assert_eq!(ids(&groups[0].voice), [102]);
    assert_eq!(groups[1].category.unwrap().id, S(120));
    assert_eq!(ids(&groups[1].text), [106, 103], "104 is hidden by an overwrite");
    assert_eq!(ids(&groups[1].voice), [105]);
    assert!(!s.can_view(S(104)) && s.can_view(S(101)));
}

#[test]
fn guild_list_folders() {
    let mut s = store();
    for (id, name) in [(200, "b"), (201, "a"), (202, "c"), (203, "z")] {
        apply(&mut s, "GUILD_CREATE", json!({"id": id.to_string(), "properties": {"name": name}}));
    }
    apply(&mut s, "GUILD_CREATE", json!({"id": "204", "properties": {"name": "v"}, "visiting": true}));
    apply(&mut s, "USER_SETTINGS_UPDATE", json!({"guild_folders": [
        {"id": -1, "guild_ids": ["202"]},
        {"id": 9, "guild_ids": ["200", "999", "204"]},
        {"id": -1, "guild_ids": ["201", "202"]},
        {"id": 9, "guild_ids": ["201"]},
    ]}));
    let list = s.guild_list();
    let flat: Vec<_> = list
        .iter()
        .map(|i| match i {
            GuildListItem::Guild(g) => g.0.to_string(),
            GuildListItem::Folder { guilds, .. } => format!("f{guilds:?}"),
        })
        .collect();
    // Unplaced (G="G", 203="z") come first in reverse name order, then folders in order;
    // the duplicate folder 9 is skipped, guild 202 only placed once, visiting 204 excluded.
    assert_eq!(flat, ["203", "100", "202", "f[Snowflake(200)]", "201"]);
}

#[test]
fn member_name_colour_and_partial_update() {
    let mut s = store();
    assert_eq!(s.member_color(S(G), S(ME)), Some(0x00ff00), "highest coloured role");
    assert_eq!(s.display_name(Some(S(G)), S(ME)), "Nick");
    assert_eq!(s.display_name(None, S(ME)), "me");
    s.mask_names = true;
    assert_eq!(s.display_name(Some(S(G)), S(ME)), "N…", "streaming privacy");
    s.mask_names = false;
    apply(&mut s, "GUILD_MEMBER_UPDATE", json!({"guild_id": G.to_string(), "user": {"id": "6"}, "roles": []}));
    assert_eq!(s.member(S(G), S(6)).unwrap().user.username, "bob", "user filled from cache");
    assert_eq!(s.member_color(S(G), S(6)), None);
    apply(&mut s, "GUILD_ROLE_UPDATE", json!({"guild_id": G.to_string(), "role": role(G, 1024, 0x123456, 0)}));
    assert_eq!(s.member_color(S(G), S(6)), Some(0x123456), "@everyone colour fallback");
    apply(&mut s, "GUILD_ROLE_DELETE", json!({"guild_id": G.to_string(), "role_id": "8"}));
    assert_eq!(s.member_color(S(G), S(ME)), Some(0xff0000));
}

#[test]
fn should_notify_follows_levels_mutes_and_dnd() {
    let mut s = store();
    let m = |channel: u64, author: u64| -> Message { serde_json::from_value(msg(1, channel, author)).unwrap() };
    let mut mention = m(101, 6);
    mention.mentions = vec![serde_json::from_value(json!({"id": ME.to_string()})).unwrap()];
    // Guild default is all messages (0); DMs always notify.
    assert!(s.should_notify(&m(101, 6)) && s.should_notify(&m(300, 6)));
    assert!(!s.should_notify(&m(101, ME)), "own messages never notify");
    // Guild set to mentions only; the category 120 override brings channel 103 back to all.
    apply(&mut s, "USER_GUILD_SETTINGS_UPDATE", json!({"guild_id": G.to_string(), "message_notifications": 1,
        "channel_overrides": {"120": {"message_notifications": 0}}}));
    assert!(!s.should_notify(&m(101, 6)) && s.should_notify(&mention));
    assert!(s.should_notify(&m(103, 6)), "category override wins over the guild level");
    // Muting the guild silences even mentions; DND silences everything.
    apply(&mut s, "USER_GUILD_SETTINGS_UPDATE", json!({"guild_id": G.to_string(), "muted": true}));
    assert!(!s.should_notify(&mention));
    s.settings.status = reflux_model::Status::Dnd;
    assert!(!s.should_notify(&m(300, 6)));
}

#[test]
fn unread_and_mentions() {
    let mut s = store();
    assert!(!s.has_unread(S(101)), "no messages yet");
    // No read state: unread only when newer than joined_at.
    let old = Snowflake::from_timestamp_ms(1_500_000_000_000);
    let new = Snowflake::from_timestamp_ms(1_700_000_000_000);
    apply(&mut s, "MESSAGE_CREATE", msg(old.0, 106, 6));
    assert!(!s.has_unread(S(106)));
    let mut m = msg(new.0, 106, 6);
    m["mentions"] = json!([{"id": ME.to_string()}]);
    m["guild_id"] = json!(G.to_string());
    apply(&mut s, "MESSAGE_CREATE", m);
    assert!(s.has_unread(S(106)));
    assert_eq!(s.mention_count(S(106)), 1);
    assert_eq!(s.guild_unread(S(G)), (true, 1));

    // Muting the channel hides the unread dot but keeps mentions.
    apply(&mut s, "USER_GUILD_SETTINGS_UPDATE", json!({"guild_id": G.to_string(),
        "channel_overrides": {"120": {"muted": true}}}));
    assert_eq!(s.guild_unread(S(G)), (false, 1));

    // Another session acks it.
    apply(&mut s, "MESSAGE_ACK", json!({"channel_id": "106", "message_id": new.0.to_string(), "mention_count": 0}));
    assert!(!s.has_unread(S(106)));
    assert_eq!(s.guild_unread(S(G)), (false, 0));

    // Own message acks; others in a DM count as mentions; blocked authors are ignored.
    apply(&mut s, "MESSAGE_CREATE", msg(new.0 + 1, 101, ME));
    assert!(!s.has_unread(S(101)));
    apply(&mut s, "MESSAGE_CREATE", msg(new.0 + 2, 300, 6));
    assert_eq!((s.has_unread(S(300)), s.mention_count(S(300))), (true, 1));
    apply(&mut s, "RELATIONSHIP_ADD", json!({"id": "6", "type": 2}));
    apply(&mut s, "MESSAGE_CREATE", msg(new.0 + 3, 300, 6));
    assert!(!s.has_unread(S(300)), "blocked DM never unread");
    assert_eq!(s.private_channels().iter().map(|c| c.id.0).collect::<Vec<_>>(), [300, 301]);
}

#[test]
fn message_window_and_nonce() {
    let mut s = store();
    let page: Vec<Message> = (1..=PAGE_SIZE as u64)
        .rev()
        .map(|i| serde_json::from_value(msg(1000 + i, 101, 6)).unwrap())
        .collect();
    s.set_latest_messages(S(101), page, PAGE_SIZE);
    let w = &s.messages[&S(101)];
    assert!(w.has_more_before && !w.has_more_after);
    assert_eq!(s.messages(S(101))[0].id, S(1001));

    // Optimistic send replaced by the real message (different ID).
    let mut local: Message = serde_json::from_value(msg(9999, 101, ME)).unwrap();
    local.nonce = Some("9999".into());
    local.content = "pending".into();
    s.add_local_message(local);
    let mut real = msg(2000, 101, ME);
    real["nonce"] = json!("9999");
    real["content"] = json!("sent");
    apply(&mut s, "MESSAGE_CREATE", real);
    let last = s.messages(S(101)).last().unwrap();
    assert_eq!((last.id, last.content.as_str()), (S(2000), "sent"));
    assert_eq!(s.messages(S(101)).len(), PAGE_SIZE + 1);

    // Update keeps reactions; delete removes.
    apply(&mut s, "MESSAGE_REACTION_ADD", json!({"user_id": "6", "channel_id": "101", "message_id": "2000",
        "emoji": {"name": "🔥"}}));
    let mut upd = msg(2000, 101, ME);
    upd["content"] = json!("edited");
    apply(&mut s, "MESSAGE_UPDATE", upd);
    let last = s.messages(S(101)).last().unwrap();
    assert_eq!((last.content.as_str(), last.reactions.len()), ("edited", 1));
    apply(&mut s, "MESSAGE_DELETE_BULK", json!({"ids": ["2000", "1001"], "channel_id": "101"}));
    assert_eq!(s.messages(S(101)).len(), PAGE_SIZE - 1);

    // Live messages are capped at MAX_WINDOW.
    for i in 0..MAX_WINDOW as u64 {
        apply(&mut s, "MESSAGE_CREATE", msg(3000 + i, 101, 6));
    }
    assert_eq!(s.messages(S(101)).len(), MAX_WINDOW);
    assert_eq!(s.messages(S(101))[0].id, S(3000));

    // Older page while at the cap drops the newest and detaches from live.
    let older: Vec<Message> = (0..10).map(|i| serde_json::from_value(msg(500 + i, 101, 6)).unwrap()).collect();
    s.prepend_messages(S(101), older, PAGE_SIZE);
    let w = &s.messages[&S(101)];
    assert!(!w.has_more_before && w.has_more_after);
    assert_eq!((w.messages.len(), w.messages[0].id), (MAX_WINDOW, S(500)));
    apply(&mut s, "MESSAGE_CREATE", msg(9000, 101, 6));
    assert!(s.messages(S(101)).iter().all(|m| m.id != S(9000)), "not appended while detached");
}

/// Found live: a message in a channel never opened made a one-message window, so opening the
/// channel later skipped fetching its history.
#[test]
fn live_message_in_an_unloaded_channel_keeps_it_unloaded() {
    let mut s = store();
    apply(&mut s, "MESSAGE_CREATE", msg(Snowflake::from_timestamp_ms(1_700_000_000_000).0, 106, 6));
    assert!(!s.messages.contains_key(&S(106)), "no window until the channel's history is fetched");
    assert!(s.has_unread(S(106)), "read state and last message still follow it");
}

#[test]
fn reactions() {
    let mut s = store();
    s.set_latest_messages(S(101), vec![serde_json::from_value(msg(1, 101, 6)).unwrap()], PAGE_SIZE);
    let r = |s: &mut Store, t: &str, user: u64, emoji: Value| {
        apply(s, t, json!({"user_id": user.to_string(), "channel_id": "101", "message_id": "1", "emoji": emoji}))
    };
    let fire = json!({"name": "🔥"});
    let custom = json!({"id": "77", "name": "x"});
    r(&mut s, "MESSAGE_REACTION_ADD", 6, fire.clone());
    r(&mut s, "MESSAGE_REACTION_ADD", ME, fire.clone());
    r(&mut s, "MESSAGE_REACTION_ADD", ME, fire.clone()); // duplicate of our own add
    r(&mut s, "MESSAGE_REACTION_ADD", 6, custom.clone());
    r(&mut s, "MESSAGE_REACTION_ADD", 6, json!({"id": "78", "name": "x"}));
    let rs = &s.messages(S(101))[0].reactions;
    assert_eq!((rs.len(), rs[0].count, rs[0].me), (3, 2, true));
    r(&mut s, "MESSAGE_REACTION_REMOVE", ME, fire.clone());
    r(&mut s, "MESSAGE_REACTION_REMOVE", ME, fire.clone()); // we no longer have it
    let rs = &s.messages(S(101))[0].reactions;
    assert_eq!((rs[0].count, rs[0].me), (1, false));
    r(&mut s, "MESSAGE_REACTION_REMOVE", 6, fire);
    assert_eq!(s.messages(S(101))[0].reactions.len(), 2);
    apply(&mut s, "MESSAGE_REACTION_REMOVE_EMOJI", json!({"channel_id": "101", "message_id": "1", "emoji": custom}));
    assert_eq!(s.messages(S(101))[0].reactions[0].emoji.id, Some(S(78)));
    apply(&mut s, "MESSAGE_REACTION_REMOVE_ALL", json!({"channel_id": "101", "message_id": "1"}));
    assert!(s.messages(S(101))[0].reactions.is_empty());
}

#[test]
fn typing_expiry() {
    let mut s = store();
    let t = |user: u64| ev("TYPING_START", json!({"channel_id": "101", "user_id": user.to_string(), "timestamp": 1}));
    s.apply(&t(6), 1_000);
    s.apply(&t(ME), 1_000);
    s.apply(&t(7), 5_000);
    assert_eq!(s.typing_users(S(101), 6_000), [S(6), S(7)], "self excluded");
    assert_eq!(s.typing_users(S(101), 11_000), [S(7)]);
    s.apply(&ev("MESSAGE_CREATE", msg(1, 101, 7)), 6_000);
    assert!(s.typing_users(S(101), 6_000).iter().all(|&u| u != S(7)), "message ends typing");
}

#[test]
fn guild_lifecycle_and_member_list() {
    let mut s = store();
    apply(&mut s, "GUILD_DELETE", json!({"id": G.to_string(), "unavailable": true}));
    assert!(s.guilds[&S(G)].unavailable && s.channels.contains_key(&S(101)));
    apply(&mut s, "GUILD_MEMBER_LIST_UPDATE", json!({"guild_id": G.to_string(), "id": "101",
        "member_count": 2, "online_count": 1,
        "groups": [{"id": "online", "count": 1}, {"id": "offline", "count": 1}],
        "ops": [{"op": "SYNC", "range": [0, 99], "items": [
            {"group": {"id": "online", "count": 1}},
            {"member": {"user": {"id": "6"}, "roles": [], "presence": {"status": "online"}}},
            {"group": {"id": "offline", "count": 1}},
            {"member": {"user": {"id": "8", "username": "zed"}, "roles": []}},
        ]}]}));
    let l = &s.member_lists[&(S(G), "101".to_string())];
    assert_eq!(l.rows.len(), 4);
    assert_eq!(l.rows[1], Some(MemberRow::Member(S(6))));
    assert_eq!(s.presences[&S(6)].status, reflux_model::Status::Online);
    // A later window only replaces its own range; a shrunk list drops rows past the new total.
    apply(&mut s, "GUILD_MEMBER_LIST_UPDATE", json!({"guild_id": G.to_string(), "id": "101",
        "member_count": 2, "online_count": 2, "groups": [{"id": "online", "count": 2}],
        "ops": [{"op": "SYNC", "range": [2, 3], "items": [
            {"member": {"user": {"id": "8"}, "roles": [], "presence": {"status": "idle"}}},
        ]}]}));
    let l = &s.member_lists[&(S(G), "101".to_string())];
    assert_eq!(l.rows, [Some(MemberRow::Group(reflux_model::MemberListGroup { id: "online".into(), count: 1 })),
        Some(MemberRow::Member(S(6))), Some(MemberRow::Member(S(8)))]);
    assert_eq!((l.online_count, l.groups.len()), (2, 1));
    assert_eq!(s.users[&S(8)].username, "zed", "partial member keeps the known user");
    apply(&mut s, "GUILD_DELETE", json!({"id": G.to_string()}));
    assert!(s.guilds.is_empty() && !s.channels.contains_key(&S(101)) && s.member_lists.is_empty());
}

#[test]
fn iso_parse() {
    assert_eq!(parse_iso_ms("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_iso_ms("2026-01-01T00:00:00.5Z"), Some(1_767_225_600_500));
    assert_eq!(parse_iso_ms("2024-02-29T12:34:56.789+00:00"), Some(1_709_210_096_789));
    assert_eq!(parse_iso_ms("bad"), None);
}

#[test]
fn around_window_knows_if_it_is_the_newest() {
    let mut s = store();
    let page = |ids: &[u64]| ids.iter().map(|&i| serde_json::from_value::<Message>(msg(i, 300, 6)).unwrap()).collect::<Vec<_>>();
    s.set_around_messages(S(300), page(&[290, 300]), 50);
    let w = &s.messages[&S(300)];
    assert!(w.has_more_after && !w.has_more_before, "300 < last 310: history");
    apply(&mut s, "MESSAGE_CREATE", msg(311, 300, 6));
    assert_eq!(s.messages(S(300)).len(), 2, "live messages wait while in history");
    s.set_around_messages(S(300), page(&[305, 311]), 50);
    assert!(!s.messages[&S(300)].has_more_after, "reaches the last message");
}

/// A reply's original deleted while loaded shows as deleted, as in Fluxer.
#[test]
fn deleting_an_original_marks_its_replies() {
    let mut s = store();
    let t = Snowflake::from_timestamp_ms(1_700_000_000_000).0;
    s.set_latest_messages(S(101), vec![], 50);
    apply(&mut s, "MESSAGE_CREATE", msg(t, 101, 6));
    let mut reply = msg(t + 1, 101, 6);
    reply["message_reference"] = json!({"channel_id": "101", "message_id": t.to_string()});
    reply["referenced_message"] = msg(t, 101, 6);
    apply(&mut s, "MESSAGE_CREATE", reply);
    apply(&mut s, "MESSAGE_DELETE", json!({"id": t.to_string(), "channel_id": "101"}));
    let r = s.messages(S(101)).iter().find(|m| m.id.0 == t + 1).expect("reply stays");
    assert_eq!(r.referenced_message, Some(None));
}

#[test]
fn calls_follow_create_update_delete() {
    let mut s = store();
    let vs = json!({"guild_id":null,"channel_id":"50","user_id":"7","connection_id":"c","session_id":"s","member":null,"mute":false,"deaf":false,"self_mute":false,"self_deaf":false,"self_video":false,"self_stream":false,"is_mobile":false,"suppress":false,"viewer_stream_keys":[],"e2ee_capable":true,"p2p":false,"version":1});
    apply(&mut s, "CALL_CREATE", json!({"channel_id":"50","message_id":"60","region":null,"ringing":["1"],"voice_states":[vs],"recipients":["1","7"],"created_at":5}));
    assert_eq!(s.calls[&Snowflake(50)].ringing, [Snowflake(1)]);
    assert!(s.voice_states.contains_key(&(None, Snowflake(7))));
    apply(&mut s, "CALL_UPDATE", json!({"channel_id":"50","message_id":"60","region":"us","ringing":[],"voice_states":[]}));
    let c = &s.calls[&Snowflake(50)];
    assert!(c.ringing.is_empty());
    assert_eq!(c.recipients.as_deref(), Some(&[Snowflake(1), Snowflake(7)][..]), "update keeps the recipients");
    apply(&mut s, "CALL_DELETE", json!({"channel_id":"50"}));
    assert!(s.calls.is_empty() && !s.voice_states.contains_key(&(None, Snowflake(7))));
}

#[test]
fn pinned_dms_sort_first_in_pin_order() {
    let mut s = Store::default();
    for (id, last) in [(1u64, 30u64), (2, 20), (3, 10)] {
        apply(&mut s, "CHANNEL_CREATE", json!({"id": id.to_string(), "type": 1, "last_message_id": last.to_string()}));
    }
    assert_eq!(ids(&s.private_channels()), [1, 2, 3], "newest first");
    s.pinned_dms = vec![S(3), S(2)];
    assert_eq!(ids(&s.private_channels()), [3, 2, 1]);
}
