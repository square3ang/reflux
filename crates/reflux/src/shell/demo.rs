//! `--shell-demo`: the shell over a hand-written fixture store, for snapshot checks:
//! `cargo run -p reflux -- --shell-demo --snap /tmp/opencode/snaps/shell.png --size 1280x800 --scale 1`
//! (add `--shell-home` for the DM list).

use chrono::{Local, TimeZone};
use reflux_model::{DispatchEvent, Snowflake};
use reflux_store::Store;
use rune::{El, Waker};
use serde_json::{Value, json};

use super::{ShellState, load_default_avatars};
use crate::State;
use crate::theme::{self, Kind};

pub struct Demo {
    pub store: Store,
    pub shell: ShellState,
}

const ME: u64 = 1000;
const HQ: u64 = 2000;
const GENERAL: u64 = 2201;

/// Local wall-clock time on the fixture's "today" (6 Nov 2026) plus `day` days.
fn at(day: i64, h: u32, m: u32) -> u64 {
    let d = Local.with_ymd_and_hms(2026, 11, 6, h, m, 0).single().expect("valid local time");
    (d.timestamp_millis() + day * 86_400_000) as u64
}

fn iso(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64).unwrap_or_default().to_rfc3339()
}

fn id_at(ms: u64, n: u64) -> String {
    (Snowflake::from_timestamp_ms(ms).0 + n).to_string()
}

fn user(id: u64, name: &str, global: &str) -> Value {
    json!({"id": id.to_string(), "username": name, "global_name": global})
}

fn chan(id: u64, kind: u16, name: &str, position: i32, parent: Option<u64>) -> Value {
    json!({"id": id.to_string(), "type": kind, "name": name, "position": position, "parent_id": parent.map(|p| p.to_string())})
}

fn guild(id: u64, name: &str, channels: Vec<Value>, roles: Vec<Value>, members: Vec<Value>, extra: Value) -> Value {
    let mut g = json!({
        "id": id.to_string(),
        "properties": {"id": id.to_string(), "name": name, "owner_id": ME.to_string()},
        "joined_at": "2025-01-01T00:00:00.000Z",
        "roles": roles, "channels": channels, "members": members,
    });
    if let (Some(g), Some(e)) = (g.as_object_mut(), extra.as_object()) {
        g.extend(e.clone());
    }
    g
}

fn member(id: u64, roles: &[u64]) -> Value {
    json!({"user": {"id": id.to_string()}, "roles": roles.iter().map(|r| r.to_string()).collect::<Vec<_>>(), "joined_at": "2025-01-01T00:00:00.000Z"})
}

fn presence(id: u64, status: &str) -> Value {
    json!({"user": {"id": id.to_string()}, "status": status})
}

/// A MESSAGE_CREATE payload; `extra` is merged in.
fn message(ms: u64, n: u64, author: u64, content: &str, extra: Value) -> Value {
    let users = [(1001, "ada", "Ada Lovelace"), (1002, "linus", "Linus"), (1003, "grace", "Grace Hopper"), (1004, "alan", "Alan Turing"), (1005, "margaret", "Margaret Hamilton"), (1006, "ken", "Ken Thompson"), (1007, "fluxbot", "Fluxbot"), (ME, "square", "Square")];
    let (_, un, gn) = users.iter().find(|u| u.0 == author).copied().unwrap_or((author, "user", "User"));
    let mut m = json!({
        "id": id_at(ms, n), "channel_id": GENERAL.to_string(), "guild_id": HQ.to_string(),
        "author": user(author, un, gn), "type": 0, "content": content, "timestamp": iso(ms),
    });
    if let (Some(o), Some(e)) = (m.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    m
}

impl Demo {
    pub fn new() -> Self {
        let now = at(0, 15, 30);
        let mut store = Store::default();
        let mut apply = |t: &str, d: Value| match DispatchEvent::parse(t, &d) {
            Ok(ev) => store.apply(&ev, now),
            Err(e) => panic!("fixture {t}: {e}"),
        };

        let staff = 2010;
        let mods = 2011;
        let contrib = 2012;
        let roles = vec![
            json!({"id": HQ.to_string(), "name": "@everyone", "permissions": "0", "position": 0}),
            json!({"id": staff.to_string(), "name": "Staff", "color": 0x4641D9, "position": 3, "hoist": true, "permissions": "0"}),
            json!({"id": mods.to_string(), "name": "Moderators", "color": 0x2ECC71, "position": 2, "hoist": true, "permissions": "0"}),
            json!({"id": contrib.to_string(), "name": "Contributor", "color": 0xE67E22, "position": 1, "permissions": "0"}),
        ];
        let hq_channels = vec![
            chan(2001, 0, "welcome", 0, None),
            chan(2100, 4, "Information", 0, None),
            chan(2101, 5, "announcements", 0, Some(2100)),
            chan(2102, 0, "rules", 1, Some(2100)),
            json!({"id": "2103", "type": 998, "name": "docs", "position": 2, "parent_id": "2100", "url": "https://docs.fluxer.app"}),
            chan(2200, 4, "Text Channels", 1, None),
            json!({"id": GENERAL.to_string(), "type": 0, "name": "general", "position": 0, "parent_id": "2200",
                   "topic": "Talk about anything — be nice, and keep **memes** in #off-topic"}),
            chan(2202, 0, "dev", 1, Some(2200)),
            chan(2203, 0, "off-topic", 2, Some(2200)),
            chan(2204, 15, "help", 3, Some(2200)),
            chan(2300, 4, "Voice", 2, None),
            chan(2301, 2, "Lounge", 0, Some(2300)),
            chan(2302, 2, "Pairing", 1, Some(2300)),
            chan(2400, 4, "Archive", 3, None),
            chan(2401, 0, "2025-events", 0, Some(2400)),
        ];
        let hq_members = vec![
            member(ME, &[staff]),
            member(1001, &[staff, contrib]),
            member(1002, &[mods]),
            member(1003, &[contrib]),
            member(1004, &[]),
            member(1005, &[]),
            member(1006, &[]),
            member(1007, &[]),
        ];
        let hq = guild(
            HQ,
            "Fluxer HQ",
            hq_channels,
            roles,
            hq_members,
            json!({
                "presences": [presence(ME, "online"), presence(1001, "online"), presence(1002, "idle"), presence(1003, "dnd"),
                              presence(1004, "online"), presence(1007, "online")],
                "voice_states": [{"user_id": "1004", "channel_id": "2301"}, {"user_id": "1003", "channel_id": "2301"}],
            }),
        );
        let small = |id: u64, name: &str, ch: u64, last: u64| {
            guild(
                id,
                name,
                vec![json!({"id": ch.to_string(), "type": 0, "name": "general", "position": 0, "last_message_id": last.to_string()})],
                vec![json!({"id": id.to_string(), "name": "@everyone", "permissions": "0", "position": 0})],
                vec![member(ME, &[])],
                json!({}),
            )
        };
        let old = Snowflake::from_timestamp_ms(at(-3, 12, 0)).0;
        let new = Snowflake::from_timestamp_ms(at(0, 12, 0)).0;
        apply(
            "READY",
            json!({
                "session_id": "demo",
                "user": {"id": ME.to_string(), "username": "square", "global_name": "Square"},
                "users": [user(1001, "ada", "Ada Lovelace"), user(1002, "linus", "Linus"), user(1003, "grace", "Grace Hopper"),
                          user(1004, "alan", "Alan Turing"), user(1005, "margaret", "Margaret Hamilton"),
                          user(1006, "ken", "Ken Thompson"), user(1007, "fluxbot", "Fluxbot")],
                "guilds": [hq, small(3000, "Rust Lang", 3001, new), small(4000, "Design Crit", 4001, new),
                           small(5000, "Ada's Lab", 5001, old), small(6000, "Weekend Hikes", 6001, old),
                           small(7000, "Retro Computing Club", 7001, old)],
                "private_channels": [
                    {"id": "8001", "type": 1, "recipients": [user(1001, "ada", "Ada Lovelace")], "last_message_id": new.to_string()},
                    {"id": "8002", "type": 1, "recipients": [user(1003, "grace", "Grace Hopper")], "last_message_id": old.to_string()},
                    {"id": "8003", "type": 3, "name": "Compiler nerds", "recipients": [user(1002, "linus", "Linus"), user(1006, "ken", "Ken Thompson")], "last_message_id": old.to_string()},
                ],
                "presences": [presence(1001, "online"), presence(1003, "dnd"), presence(1002, "idle")],
                "sessions": [{"session_id": "all", "status": "online"}],
                "read_states": [
                    {"id": "3001", "last_message_id": old.to_string(), "mention_count": 3},
                    {"id": "4001", "last_message_id": old.to_string(), "mention_count": 0},
                    {"id": "5001", "last_message_id": old.to_string()}, {"id": "6001", "last_message_id": old.to_string()},
                    {"id": "7001", "last_message_id": old.to_string()},
                    {"id": "8001", "last_message_id": old.to_string(), "mention_count": 2},
                    {"id": "2202", "last_message_id": "1"}, {"id": "2203", "last_message_id": "1"},
                    {"id": "2001", "last_message_id": new.to_string()}, {"id": "2101", "last_message_id": new.to_string()},
                    {"id": "2102", "last_message_id": new.to_string()}, {"id": "2204", "last_message_id": new.to_string()},
                    {"id": "2401", "last_message_id": new.to_string()},
                ],
                "user_settings": {"status": "online", "guild_folders": [
                    {"id": -1, "guild_ids": ["2000", "3000", "4000"]},
                    {"id": 7, "name": "Friends", "color": 0x2ECC71, "guild_ids": ["5000", "6000"]},
                    {"id": -1, "guild_ids": ["7000"]},
                ]},
                "user_guild_settings": [{"guild_id": HQ.to_string(), "channel_overrides": {"2203": {"muted": true}}}],
            }),
        );
        // Unread activity in other HQ channels (they have no loaded window; this bumps last IDs).
        for (ch, n) in [(2202u64, 1u64), (2203, 2)] {
            let mut m = message(at(0, 11, 0), n, 1002, "ping", json!({}));
            m["channel_id"] = json!(ch.to_string());
            apply("MESSAGE_CREATE", m);
        }

        let y = |h, m| at(-1, h, m);
        let t = |h, m| at(0, h, m);
        let alan_shot = message(t(10, 1), 0, 1004, "First light from the new renderer:", json!({
            "attachments": [{"id": "9001", "filename": "screenshot.png", "content_type": "image/png", "size": 482133, "width": 1280, "height": 720}],
            "reactions": [{"emoji": {"name": "👍"}, "count": 3, "me": true}, {"emoji": {"name": "🎉"}, "count": 1, "me": false},
                          {"emoji": {"id": "77", "name": "fluxer"}, "count": 2, "me": false}],
        }));
        let msgs = vec![
            message(y(18, 2), 0, 1001, "Shipping the **gateway** rewrite tonight", json!({})),
            message(y(18, 3), 0, 1001, "it now speaks `zstd-stream` end to end, with resume", json!({})),
            message(y(18, 40), 0, 1006, "nice. benchmarks?", json!({"edited_timestamp": iso(y(18, 41))})),
            message(t(9, 12), 0, 1002, "Morning! Here's the diff for the store:\n```rust\nfn apply(&mut self, ev: &DispatchEvent) {\n    match ev {\n        E::Ready(r) => self.ready(r),\n        _ => {}\n    }\n}\n```\nreviews welcome", json!({})),
            message(t(9, 14), 0, 1003, "> the store owns every channel\nYes, and ||threads too||. See [the docs](https://docs.fluxer.app) or <#2202>.", json!({})),
            message(t(9, 15), 0, 1003, "Plan:\n- port the member list\n- *virtualize* the message list\n- ~~ship it~~ test it first", json!({})),
            alan_shot.clone(),
            message(t(10, 5), 0, ME, "Looks great <@1004>! The squircles are **pixel**-identical.", json!({
                "type": 19,
                "message_reference": {"channel_id": GENERAL.to_string(), "message_id": alan_shot["id"]},
                "referenced_message": alan_shot,
            })),
            message(t(10, 6), 0, 1001, "## Release notes\n<@1000> can you review the __changelog__ before we tag 0.4?", json!({"mentions": [user(ME, "square", "Square")]})),
            message(t(10, 20), 0, 1007, "", json!({"embeds": [{"type": "rich", "title": "Build #1842 passed", "url": "https://ci.fluxer.app/1842",
                "description": "All **312** tests green on `main`.", "color": 0x2ECC71,
                "fields": [{"name": "Duration", "value": "4m 12s", "inline": true}], "footer": {"text": "Fluxer CI"}}]})),
            message(t(11, 2), 0, 1005, "", json!({"type": 7})),
            message(t(11, 3), 0, 1005, "Hi all! Happy to be here. I brought `notes.txt`:", json!({
                "attachments": [{"id": "9002", "filename": "notes.txt", "content_type": "text/plain", "size": 2048}]})),
        ];
        let ack = msgs[7]["id"].as_str().unwrap_or_default().to_string();
        for m in msgs {
            apply("MESSAGE_CREATE", m);
        }
        apply("MESSAGE_ACK", json!({"channel_id": GENERAL.to_string(), "message_id": ack, "mention_count": 1, "manual": true}));
        apply("TYPING_START", json!({"channel_id": GENERAL.to_string(), "user_id": "1002", "guild_id": HQ.to_string(), "timestamp": now / 1000}));

        let mut shell = ShellState { guild: Some(Snowflake(HQ)), channel: Some(Snowflake(GENERAL)), now_ms: now, ..ShellState::default() };
        if rune::extra_args().iter().any(|a| a == "--shell-home") {
            (shell.guild, shell.channel) = (None, Some(Snowflake(8001)));
        }
        Demo { store, shell }
    }
}

pub fn setup(s: &mut State, waker: Waker) {
    if let Some(d) = s.shell_demo.as_mut() {
        d.shell.default_avatars = load_default_avatars(waker);
    }
}

fn acc(s: &mut State) -> &mut ShellState {
    &mut s.shell_demo.get_or_insert_with(Demo::new).shell
}

pub fn view(d: &Demo) -> El {
    super::view(&d.store, &d.shell, theme::palette(Kind::Dark), acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::first_channel;

    #[test]
    fn fixture_builds_a_browsable_store() {
        let d = Demo::new();
        let s = &d.store;
        assert_eq!(s.guilds.len(), 6);
        assert_eq!(first_channel(s, Snowflake(HQ)), Some(Snowflake(2001)));
        assert_eq!(s.messages(Snowflake(GENERAL)).len(), 12);
        assert!(s.has_unread(Snowflake(GENERAL)));
        assert!(s.has_unread(Snowflake(2202)));
        assert!(s.is_muted(&s.channels[&Snowflake(2203)]));
        let groups = super::super::members::groups(s, Snowflake(HQ));
        let names: Vec<&str> = groups.iter().map(|g| g.0.as_str()).collect();
        assert_eq!(names, ["Staff", "Moderators", "Online", "Offline"]);
        assert_eq!(s.typing_users(Snowflake(GENERAL), d.shell.now_ms), [Snowflake(1002)]);
    }
}
