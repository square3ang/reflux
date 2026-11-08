//! `--shell-demo`: the shell over a hand-written fixture store, for snapshot checks:
//! `cargo run -p reflux -- --shell-demo --snap /tmp/opencode/snaps/shell.png --size 1280x800 --scale 1`
//! (add `--shell-home` for the DM list, `--friends[=all|pending|add]` for the friends page,
//! `--profile` for a profile popout, `--ctx=user|guild|channel` for context menus,
//! `--guild-ui=<scene>` for community management, `--threads[=<scene>]` for threads and forums,
//! `--social=<scene>` for `crate::social`).

use chrono::{Local, TimeZone};
use reflux_model::{DispatchEvent, Snowflake};
use reflux_store::Store;
use rune::{El, Image, ImageCache, Waker};
use serde_json::{Value, json};

use super::{ShellState, load_default_avatars};
use crate::State;
use crate::theme;

pub struct Demo {
    pub store: Store,
    pub shell: ShellState,
    /// `media-long`: older pages served for `LoadOlder`, 0.3 s after it, like a fetch.
    older: Option<(Snowflake, std::time::Instant)>,
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
    // Two users have custom avatars (drawn by `fixture`); the rest show default ones.
    let avatar = matches!(id, 1001 | 1004).then_some("demo");
    json!({"id": id.to_string(), "username": name, "global_name": global, "avatar": avatar})
}

/// Offline stand-ins for media proxy URLs: the Fluxer logo for icons and emoji, gradients for
/// the rest. Waits for the decode so headless snapshots include them.
fn fixture(cache: &ImageCache, url: &str) -> Option<Image> {
    let gradient = |w: u32, h: u32, a: [f32; 3], b: [f32; 3]| {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let t = (x + y) as f32 / (w + h) as f32;
                px.extend((0..3).map(|i| (a[i] + (b[i] - a[i]) * t) as u8));
                px.push(255);
            }
        }
        cache.rgba(w, h, px)
    };
    let img = if url.contains("/stickers/") {
        // A WebP, as the proxy serves stickers, through the app's own decoder.
        let px = image::RgbaImage::from_fn(320, 320, |x, y| {
            let d = ((x as f32 - 160.0).powi(2) + (y as f32 - 160.0).powi(2)).sqrt();
            image::Rgba(if d < 150.0 { [88, 101, 242, 255] } else { [0, 0, 0, 0] })
        });
        let mut webp = Vec::new();
        image::DynamicImage::ImageRgba8(px).write_to(&mut std::io::Cursor::new(&mut webp), image::ImageFormat::WebP).ok()?;
        crate::media::decode(cache, webp).ok()?
    } else if url.contains("/icons/") || url.contains("/emojis/") {
        cache.svg(&include_bytes!("../../../../assets/media/images/fluxer-logo-color.svg")[..], 96.0, 2.0)
    } else if url.contains("/avatars/") {
        gradient(128, 128, [250.0, 166.0, 26.0], [237.0, 66.0, 69.0])
    } else if url.contains("/banners/") {
        gradient(480, 270, [70.0, 65.0, 217.0], [235.0, 69.0, 158.0])
    } else if url.contains("/attachments/") || url.contains("/external/") {
        gradient(640, 360, [0.0, 187.0, 167.0], [43.0, 127.0, 255.0])
    } else {
        return None;
    };
    let _ = img.wait();
    Some(img)
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
    /// `media-long`: answers `LoadOlder` 0.3 s later with 50 older messages (an image every
    /// seventh), through the app's own `prepend_older`, until 500 are loaded.
    pub fn serve_older(&mut self, tx: &mut rune::Tx) -> bool {
        if self.older.is_none()
            && let Some(i) = self.shell.intents.iter().position(|i| matches!(i, super::Intent::LoadOlder { .. }))
            && let super::Intent::LoadOlder { channel } = self.shell.intents.remove(i)
        {
            self.older = Some((channel, std::time::Instant::now()));
        }
        let Some((channel, asked)) = self.older else { return false };
        if asked.elapsed() < std::time::Duration::from_millis(300) || crate::app::older_held() {
            tx.wake_after(std::time::Duration::from_millis(30));
            return false;
        }
        self.older = None;
        let oldest = self.store.messages(channel).first().map_or(at(0, 12, 0), |m| m.id.timestamp_ms());
        let n = if self.store.messages(channel).len() >= 500 { 10 } else { 50 };
        let page = (1..=n as u64)
            .rev()
            .filter_map(|k| {
                let extra = if k % 7 == 0 { json!({"attachments": [{"id": (9900 + k).to_string(), "filename": "old.png", "content_type": "image/png", "size": 1, "width": 1200, "height": 800}]}) } else { json!({}) };
                serde_json::from_value(message(oldest - k * 60_000, 0, [1001, 1002, 1003][(k % 3) as usize], &format!("older message {k}"), extra)).ok()
            })
            .collect();
        crate::app::prepend_older(&mut self.store, tx, channel, page);
        true
    }

    pub fn new() -> Self {
        let now = at(0, 15, 30);
        let mut store = Store::default();
        let mut apply = |t: &str, d: Value| match DispatchEvent::parse(t, &d) {
            Ok(ev) => {
                // Histories count as fetched (their messages go into a loaded window), except the
                // two channels whose only activity is unread pings.
                if let DispatchEvent::MessageCreate(m) = &ev
                    && ![2202, 2203].contains(&m.channel_id.0)
                {
                    store.messages.entry(m.channel_id).or_insert_with(|| reflux_store::MessageWindow { has_more_before: true, ..Default::default() });
                }
                store.apply(&ev, now)
            }
            Err(e) => panic!("fixture {t}: {e}"),
        };

        let staff = 2010;
        let mods = 2011;
        let contrib = 2012;
        let roles = vec![
            json!({"id": HQ.to_string(), "name": "@everyone", "permissions": "0", "position": 0}),
            json!({"id": staff.to_string(), "name": "Staff", "color": 0x4641D9, "position": 3, "hoist": true, "permissions": "0"}),
            json!({"id": mods.to_string(), "name": "Moderators", "color": 0x2ECC71, "unicode_emoji": "🛡️", "position": 2, "hoist": true, "permissions": "0"}),
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
            json!({"id": "2202", "type": 0, "name": "dev", "position": 1, "parent_id": "2200", "rate_limit_per_user": 10,
                   "permission_overwrites": [{"id": HQ.to_string(), "type": 0, "allow": "0", "deny": "2048"},
                                             {"id": "2011", "type": 0, "allow": "2048", "deny": "0"}]}),
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
        let mut hq = guild(
            HQ,
            "Fluxer HQ",
            hq_channels,
            roles,
            hq_members,
            json!({
                "presences": [presence(ME, "online"), presence(1001, "online"), presence(1002, "idle"), presence(1003, "dnd"),
                              presence(1004, "online"), presence(1007, "online")],
                "voice_states": [{"user_id": "1004", "channel_id": "2301", "connection_id": "c1004", "self_stream": true},
                                 {"user_id": "1003", "channel_id": "2301", "self_mute": true, "self_deaf": true}],
            }),
        );
        hq["properties"]["icon"] = json!("demo");
        hq["emojis"] = json!([{"id": "77", "name": "fluxer", "animated": false}, {"id": "78", "name": "flux_love", "animated": false},
                              {"id": "79", "name": "shipit", "animated": false}, {"id": "80", "name": "party_parrot", "animated": true}]);
        hq["stickers"] = json!([{"id": "88", "name": "wave", "description": "Hello there", "tags": ["hi"], "animated": false},
                                {"id": "89", "name": "ship it", "description": "", "tags": ["release"], "animated": false}]);
        hq["properties"]["banner"] = json!("demo");
        hq["properties"]["features"] = json!(["INVITE_SPLASH", "VISITS_ENABLED", "VERIFIED"]);
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
        let mut rust = small(3000, "Rust Lang", 3001, new);
        rust["emojis"] = json!([{"id": "81", "name": "ferris", "animated": false}, {"id": "82", "name": "borrowck", "animated": false}]);
        rust["stickers"] = json!([{"id": "91", "name": "ferris dance", "description": "", "tags": ["crab"], "animated": false}]);
        apply(
            "READY",
            json!({
                "session_id": "demo",
                "user": {"id": ME.to_string(), "username": "square", "global_name": "Square", "email": "square@example.com"},
                "users": [user(1001, "ada", "Ada Lovelace"), user(1002, "linus", "Linus"), user(1003, "grace", "Grace Hopper"),
                          user(1004, "alan", "Alan Turing"), user(1005, "margaret", "Margaret Hamilton"),
                          user(1006, "ken", "Ken Thompson"), user(1007, "fluxbot", "Fluxbot"),
                          user(1008, "hedy", "Hedy Lamarr"), user(1009, "spammer", "Spam Bot")],
                "relationships": [{"id": "1001", "type": 1}, {"id": "1003", "type": 1, "friend_shares_voice_activity": true}, {"id": "1004", "type": 1, "friend_shares_voice_activity": true},
                                  {"id": "1006", "type": 1}, {"id": "1005", "type": 3}, {"id": "1008", "type": 4},
                                  {"id": "1009", "type": 2}],
                "notes": {"1001": "Wrote the first program"},
                "guilds": [hq, rust, small(4000, "Design Crit", 4001, new),
                           small(5000, "Ada's Lab", 5001, old), small(6000, "Weekend Hikes", 6001, old),
                           small(7000, "Retro Computing Club", 7001, old)],
                "private_channels": [
                    {"id": "8001", "type": 1, "recipients": [user(1001, "ada", "Ada Lovelace")], "last_message_id": new.to_string()},
                    {"id": "8002", "type": 1, "recipients": [user(1003, "grace", "Grace Hopper")], "last_message_id": old.to_string()},
                    {"id": "8003", "type": 3, "name": "Compiler nerds", "recipients": [user(1002, "linus", "Linus"), user(1006, "ken", "Ken Thompson")], "last_message_id": old.to_string()},
                ],
                "presences": [{"user": {"id": "1001"}, "status": "online", "custom_status": {"text": "Shipping the gateway"}},
                              presence(1003, "dnd"), presence(1002, "idle"), presence(1004, "online")],
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
                "user_settings": {"status": "online", "render_reactions": true, "guild_folders": [
                    {"id": -1, "guild_ids": ["2000", "3000", "4000"]},
                    {"id": 7, "name": "Friends", "color": 0x2ECC71, "guild_ids": ["5000", "6000"]},
                    {"id": -1, "guild_ids": ["7000"]},
                ]},
                "user_guild_settings": [{"guild_id": HQ.to_string(), "channel_overrides": {"2203": {"muted": true, "message_notifications": 3}, "2200": {"muted": false, "message_notifications": 1}}}],
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
            "attachments": [{"id": "9001", "filename": "screenshot.png", "content_type": "image/png", "size": 482133, "width": 1280, "height": 720,
                             "proxy_url": "https://media.fluxer.app/attachments/2201/9001/screenshot.png"}],
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
                "fields": [{"name": "Duration", "value": "4m 12s", "inline": true}], "footer": {"text": "Fluxer CI"},
                "thumbnail": {"url": "https://ci.fluxer.app/badge.png", "proxy_url": "https://media.fluxer.app/external/sig/ci.png", "width": 128, "height": 128}}]})),
            message(t(11, 2), 0, 1005, "", json!({"type": 7})),
            message(t(11, 3), 0, 1005, "Hi all! Happy to be here. I brought `notes.txt`:", json!({
                "attachments": [{"id": "9002", "filename": "notes.txt", "content_type": "text/plain", "size": 2048}]})),
            message(t(11, 4), 0, 1006, "<:fluxer:77> <:fluxer:77>", json!({})),
            message(t(11, 5), 0, 1002, "Inline emoji: <:fluxer:77> custom, 😀 unicode and 🎉 party, in a sentence long enough to wrap \
                onto a second line of the chat column, where 👍 and <:fluxer:77> have to land on the right spot too. **Bold 🚀** ends it.", json!({})),
            message(t(11, 6), 0, 1004, "", json!({"stickers": [{"id": "88", "name": "wave", "animated": false}]})),
        ];
        let ack = msgs[7]["id"].as_str().unwrap_or_default().to_string();
        for m in msgs {
            apply("MESSAGE_CREATE", m);
        }
        apply("MESSAGE_ACK", json!({"channel_id": GENERAL.to_string(), "message_id": ack, "mention_count": 1, "manual": true}));
        // The lazy member list of #general, as the gateway sends it.
        let groups = [("2010", vec![(1001, "online"), (ME, "online")]), ("2011", vec![(1002, "idle")]),
                      ("online", vec![(1004, "online"), (1007, "online"), (1003, "dnd")]), ("offline", vec![(1006, "offline"), (1005, "offline")])];
        let roles_of = |u: u64| match u { ME => vec![staff], 1001 => vec![staff, contrib], 1002 => vec![mods], 1003 => vec![contrib], _ => vec![] };
        let mut items = Vec::new();
        for (g, us) in &groups {
            items.push(json!({"group": {"id": g, "count": us.len()}}));
            for (u, st) in us {
                let mut m = member(*u, &roles_of(*u));
                m["presence"] = json!({"status": st});
                items.push(json!({"member": m}));
            }
        }
        apply("GUILD_MEMBER_LIST_UPDATE", json!({"guild_id": HQ.to_string(), "id": GENERAL.to_string(), "member_count": 8, "online_count": 6,
            "groups": groups.iter().map(|(g, us)| json!({"id": g, "count": us.len()})).collect::<Vec<_>>(),
            "ops": [{"op": "SYNC", "range": [0, 99], "items": items}]}));
        apply("TYPING_START", json!({"channel_id": GENERAL.to_string(), "user_id": "1002", "guild_id": HQ.to_string(), "timestamp": now / 1000}));

        let mut shell = ShellState { guild: Some(Snowflake(HQ)), channel: Some(Snowflake(GENERAL)), now_ms: now, premium: true, marketing_base: Some("https://fluxer.app".into()), gif_provider: "Klipy".into(), ..ShellState::default() };
        // `--settings[=tab]` opens user settings (`--settings-search=q` types into its search); `--theme=light|coal|dark_legacy|system` sets the
        // account theme; `--compact` the dense message layout.
        for a in rune::extra_args() {
            if let Some(t) = a.strip_prefix("--theme=") {
                store.settings.theme = t.to_owned();
            } else if a == "--compact" {
                store.settings.message_display_compact = true;
            } else if a == "--settings" || a.starts_with("--settings=") {
                // The live test instance grants `feature_higher_video_quality` (16 Mbps ceiling).
                shell.limits.higher_video_quality = true;
                shell.limits.screen_share_bitrate = 16_000_000;
                shell.open_settings(a.strip_prefix("--settings="));
                let sound = |id: u64, name: &str| super::voice::EntranceSound { id: Snowflake(id), name: name.into(), url: String::new(), duration_ms: 1240 };
                let mut lib = super::voice::EntranceLibrary { sounds: vec![sound(1, "airhorn"), sound(2, "door chime")], ..Default::default() };
                lib.selections.insert("global".into(), Snowflake(1));
                shell.voice.entrance = Some(lib);
            } else if a == "--dev-app" {
                // Applications: a bot app's detail (bot profile with no banner yet).
                shell.form.data.insert("dev.apps".into(), json!([{"id": "4242", "name": "Helper", "bot": {"id": "4243", "username": "helper", "discriminator": "0000", "flags": 0}}]));
                shell.nav.dev.app = Some("4242".into());
            } else if let Some(css) = a.strip_prefix("--quick-css=") {
                // The in-tab Quick CSS editor focused with its caret at the end.
                let mut e = rune::TextEdit::new(css.replace("\\n", "\n"));
                e.focused = true;
                e.set_caret(e.text.len());
                shell.form.text.insert("quick-css".into(), e);
            } else if a.starts_with("--popup=") {
                // Applied after the scenes below, so the control exists first.
            } else if let Some(q) = a.strip_prefix("--settings-search=")
                && let Some(u) = shell.settings.as_mut()
            {
                u.search = rune::TextEdit::new(q);
                // Already past the search debounce.
                u.typed = Some((q.trim().to_string(), std::time::Instant::now() - std::time::Duration::from_secs(1)));
            }
        }
        // `REFLUX_LOOK='{"saturation":0.5}'`: appearance / accessibility preferences for snapshots.
        if let Some(look) = std::env::var("REFLUX_LOOK").ok().and_then(|j| serde_json::from_str::<crate::look::Prefs>(&j).ok()) {
            crate::look::set(&look);
            shell.prefs.look = look;
        }
        if let Some(sc) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--threads")) {
            thread_scene(&mut shell, &mut store, sc.trim_start_matches('='));
        }
        if let Some(sc) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--msg-scene=")) {
            stage(&mut shell, &mut store, sc);
        }
        for a in rune::extra_args() {
            if a == "--shell-home" {
                (shell.guild, shell.channel) = (None, Some(Snowflake(8001)));
                // A pinned DM, previews (markdown, an attachment, a system message) and a call
                // running in Ada's DM with its "Join the call" link.
                store.pinned_dms = vec![Snowflake(8002)];
                let dm = |ch: u64, ms: u64, author: u64, content: &str, extra: Value| {
                    let mut m = message(ms, 0, author, content, extra);
                    m["channel_id"] = json!(ch.to_string());
                    m.as_object_mut().map(|o| o.remove("guild_id"));
                    m
                };
                let t = |m| at(0, 15, m);
                let mut msgs = vec![
                    dm(8001, t(1), 1001, "did you see **the new** `gateway` build? <:fluxer:77>", json!({})),
                    dm(8001, t(2), 1001, "", json!({"type": 3, "call": {"participants": ["1001"]}})),
                    dm(8002, t(3), 1003, "", json!({"attachments": [{"id": "9501", "filename": "a.png", "content_type": "image/png", "size": 10}]})),
                    dm(8003, t(4), 1002, "", json!({"type": 1, "mentions": [user(1006, "ken", "Ken Thompson")]})),
                ];
                msgs.push(json!({"channel_id": "8001", "message_id": "1", "region": null, "ringing": [], "voice_states": [], "recipients": ["1001", ME.to_string()]}));
                for (i, m) in msgs.into_iter().enumerate() {
                    let kind = if i == 4 { "CALL_CREATE" } else { "MESSAGE_CREATE" };
                    if let Ok(ev) = DispatchEvent::parse(kind, &m) {
                        if let DispatchEvent::MessageCreate(m) = &ev {
                            store.messages.entry(m.channel_id).or_default();
                        }
                        store.apply(&ev, now);
                    }
                }
            } else if let Some(tab) = a.strip_prefix("--friends").map(|t| t.trim_start_matches('=')) {
                (shell.guild, shell.channel) = (None, None);
                shell.friends_tab = match tab {
                    "all" => super::FriendsTab::All,
                    "pending" => super::FriendsTab::Pending,
                    "add" => super::FriendsTab::Add,
                    "empty" => {
                        shell.friends_query = rune::TextEdit::new("zzz");
                        super::FriendsTab::All
                    }
                    "all-menu" => {
                        shell.popups.open = Some("friend-more-1001".into());
                        super::FriendsTab::All
                    }
                    _ => super::FriendsTab::Online,
                };
                // `--friends=stream`: Active Now with a stream preview (any image will do).
                if tab == "stream" {
                    let bytes = Some(std::sync::Arc::new(include_bytes!("../../../../assets/media/images/app-icon.png").to_vec()));
                    let preview = super::active_now::Preview { bytes, fetched_ms: 1, ..Default::default() };
                    shell.voice.previews.insert(format!("{HQ}:2301:c1004"), preview);
                }
            } else if a == "--profile" {
                let note = store.notes.get(&Snowflake(1001)).cloned().unwrap_or_default();
                shell.open_profile(Snowflake(1001), Some(Snowflake(HQ)), note, "member-1001".into(), crate::ui::Placement::LeftStart);
            } else if let Some(which) = a.strip_prefix("--ctx=") {
                use super::Target;
                let (t, at) = match which {
                    "guild" => (Target::Guild(Snowflake(HQ)), [40.0, 120.0]),
                    "channel" => (Target::Channel(Snowflake(GENERAL)), [180.0, 330.0]),
                    _ => (Target::User { user: Snowflake(1001), guild: Some(Snowflake(HQ)), dm: None }, [1000.0, 160.0]),
                };
                shell.open_user_menu(t, at, "member-1001".into(), crate::ui::Placement::LeftStart);
            }
        }
        account_fixtures(&mut shell, &store, now);
        if let Some(sc) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--account-ui=")) {
            if sc.starts_with("birthday")
                && let Some(me) = &mut store.me
            {
                me.date_of_birth_required = true;
            }
            account_scene(&mut shell, &mut store, sc);
        }
        if let Some(sc) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--guild-ui=")) {
            guild_scene(&mut shell, sc);
        }
        if let Some(sc) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--social=")) {
            crate::social::demo::scene(&mut shell, &mut store, sc);
        }
        if let Some(sc) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--voice")) {
            voice_scene(&mut shell, &mut store, sc.trim_start_matches('='));
        }
        shell.voice.devices = Some((vec!["Built-in Microphone".into(), "USB Headset".into()], vec!["Speakers".into(), "USB Headset".into()]));
        if let Some(sc) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--nav=")) {
            crate::nav::demo(&mut shell, &store, sc);
        }
        if let Some(sc) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--dev=")) {
            super::devtools::demo(&mut shell, &mut store, sc);
        }
        if let Some(sc) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--desktop=")) {
            crate::desktop::demo(&mut shell, &mut store, sc);
        }
        // `--popup=<id>`: opens a popout menu by its ID (snapshots of open selects/menus).
        if let Some(id) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--popup=")) {
            shell.popups.toggle(id);
        }
        Demo { store, shell, older: None }
    }
}

/// What the account tabs and the switcher would have loaded.
fn account_fixtures(sh: &mut ShellState, store: &Store, now: u64) {
    sh.account.me = store.my_id();
    let me = store.me.as_ref().map(|u| &u.base);
    sh.account.current = me.map(|u| crate::session::Account {
        instance: "fluxer.app".into(),
        user_id: u.id.to_string(),
        username: u.username.clone(),
        display_name: u.global_name.clone().unwrap_or_else(|| u.username.clone()),
        ..Default::default()
    });
    sh.account.accounts =
        vec![crate::session::Account { instance: "chat.example.org".into(), user_id: "77".into(), username: "ada_work".into(), display_name: "Ada (work)".into(), ..Default::default() }];
    sh.account.activity = vec![crate::app::accounts::AccountActivity {
        slot: sh.account.accounts[0].slot(),
        state: crate::app::accounts::EngineState::Live,
        summary: crate::app::accounts::Summary {
            mentions: 2,
            unread: 3,
            last_activity: now,
            channels: vec![
                crate::app::accounts::ChannelActivity {
                    channel: reflux_model::Snowflake(9101),
                    guild: Some(reflux_model::Snowflake(9100)),
                    kind: reflux_model::ChannelType::GUILD_TEXT,
                    name: "release-notes".into(),
                    guild_name: Some("Acme".into()),
                    mentions: 2,
                    unread: true,
                    last_message: Some(reflux_model::Snowflake::default()),
                },
                crate::app::accounts::ChannelActivity {
                    channel: reflux_model::Snowflake(9102),
                    guild: None,
                    kind: reflux_model::ChannelType::DM,
                    name: "Mina".into(),
                    guild_name: None,
                    mentions: 0,
                    unread: true,
                    last_message: Some(reflux_model::Snowflake::default()),
                },
            ],
            recent_mentions: vec![crate::app::accounts::MentionActivity {
                message: reflux_model::Snowflake::default(),
                channel: reflux_model::Snowflake(9101),
                guild: Some(reflux_model::Snowflake(9100)),
                channel_name: "#release-notes".into(),
                guild_name: Some("Acme".into()),
                author: "Mina".into(),
                preview: "Can you check the deploy notes?".into(),
                timestamp: now,
            }],
        },
    }];
    let day = |d: u64| iso(now - d * 86_400_000);
    for (k, v) in [
        ("passkeys", json!([{"id": "pk1", "name": "YubiKey 5C", "created_at": day(90), "last_used_at": day(2)}])),
        (
            "sessions",
            json!([
                {"id_hash": "a1", "current": true, "approx_last_used_at": day(0), "client_info": {"os": "Linux", "browser": "Reflux", "location": {"city": "Seoul", "country": "South Korea"}}},
                {"id_hash": "b2", "approx_last_used_at": day(3), "client_info": {"os": "iOS", "platform": "Fluxer", "device": "mobile", "location": {"city": "Busan", "country": "South Korea"}}},
                {"id_hash": "c3", "approx_last_used_at": day(20), "client_info": {"os": "Windows", "browser": "Firefox"}}
            ]),
        ),
        ("apps", json!([{"application": {"id": "900", "name": "Fluxer Stats"}, "scopes": ["identify", "guilds"], "authorized_at": day(12)}])),
        ("connections", json!([{"type": "domain", "id": "c1", "name": "ada.dev", "verified": true, "visibility_flags": 1}])),
        ("harvest", json!({"harvest_id": "h1", "status": "completed", "created_at": day(40), "completed_at": day(40)})),
    ] {
        sh.form.data.insert(k.into(), v);
    }
}

/// `--account-ui=<scene>`: `menu` (user area), `status`, `status-emoji` (with a draft and the picker), `crop` / `crop-banner` / `crop-guild-banner` (the crop modal), `timezone`, `accounts`, `nickname`, `sudo`,
/// `ris` / `ris-add`, `search-engines` / `translators` (Advanced's provider settings), `encoder`,
/// `profile` (the full profile modal), `privacy-review` (the privacy setup modal),
/// `bounced-email` / `bounced-email-code` (the bounced-address modal).
fn account_scene(sh: &mut ShellState, store: &mut Store, scene: &str) {
    use crate::settings::ShellModal;
    match scene {
        "privacy-review" | "privacy-review-alert" | "privacy-review-changed" => {
            sh.open_settings(Some("privacy_safety"));
            if scene != "privacy-review-alert"
                && let Some(u) = sh.settings.as_mut()
            {
                u.privacy_review = Some(crate::settings::privacy::Review::new(store));
            }
            if scene == "privacy-review-changed" {
                store.settings.restricted_guilds = vec![Snowflake(3000)];
                if let Some(r) = sh.settings.as_mut().and_then(|u| u.privacy_review.as_mut()) {
                    r.friends = true;
                }
            }
        }
        "bounced-email" | "bounced-email-code" => {
            sh.tiers.emails_enabled = true;
            sh.tiers.delivery_on = true;
            if let Some(me) = store.me.as_mut() {
                me.email_bounced = true;
                me.verified = false;
            }
            crate::shell::bounced::open(sh);
            if let Some(u) = sh.account_gate.bounced.as_mut() {
                if scene == "bounced-email-code" {
                    (u.ticket, u.code) = (Some("fixture-ticket".into()), rune::TextEdit::new("1234 5678"));
                    u.code.focused = true;
                } else {
                    u.email = rune::TextEdit::new("ada@example.com");
                }
            }
        }
        "menu" => sh.popups.open = Some(super::status::MENU.into()),
        "birthday" | "birthday-logout" => {
            sh.marketing_base = Some("https://fluxer.app".into());
            sh.birthday.dob.month = Some("3".into());
            sh.birthday.error = Some("Something went wrong. Try again.".into());
            sh.birthday.confirm_logout = scene == "birthday-logout";
        }
        "status" => super::status::open(sh, ShellModal::CustomStatus),
        "crop" | "crop-banner" | "crop-guild-banner" => {
            sh.open_settings(Some("my_profile"));
            // The community banner is 16:9 and small enough to keep as is ("Skip cropping").
            let (w, h) = if scene == "crop-guild-banner" { (1280, 720) } else { (900, 600) };
            let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x / 4) as u8, (y / 3) as u8, 160, 255]));
            let mut png = Vec::new();
            let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
            let asset = match scene {
                "crop" => crate::settings::crop::Asset::Avatar,
                "crop-guild-banner" => crate::settings::crop::Asset::GuildBanner,
                _ => crate::settings::crop::Asset::ProfileBanner,
            };
            sh.nav.crop = crate::settings::crop::Crop::new("p.avatar", asset, png);
        }
        "timezone" => {
            sh.open_settings(Some("my_profile"));
            sh.form.data.insert("p.timezone".into(), json!("Asia/Seoul"));
            sh.nav.timezone = true;
        }
        "recovery" | "recovery-kit" => {
            sh.username_sign_in = true;
            sh.web_base = "https://web.fluxer.app".into();
            sh.open_settings(Some("account_security"));
            sh.form.data.insert("recovery_kit".into(), json!({"has_recovery_kit": false, "created_at": null}));
            if scene == "recovery-kit" {
                sh.open_account_modal(crate::settings::AccountModal::RecoveryKit);
                sh.form.data.insert("m.kit".into(), json!({"recovery_key": "7QX2-M4KD-P9RA-W3TB-H6NC-Z8JE", "created_at": "2026-11-07T09:30:00Z"}));
                sh.form.data.insert("m.rk_ack".into(), json!(true));
            }
        }
        "ris" | "ris-add" => {
            sh.open_settings(Some("advanced_settings"));
            sh.prefs.ris_custom.push((
                "custom_1".into(),
                "Kagi".into(),
                "https://kagi.com/images?q={url}".into(),
            ));
            sh.prefs
                .ris_enabled
                .extend(["bing".to_string(), "custom_1".to_string()]);
            if scene == "ris" {
                sh.open_account_modal(crate::settings::AccountModal::Providers(crate::settings::search_engines::Mode::Image));
            } else {
                sh.open_account_modal(crate::settings::AccountModal::ProviderEdit(crate::settings::search_engines::Mode::Image, None));
                sh.form.error =
                    Some(crate::i18n::t("URL pattern must contain the {url} placeholder.").into());
            }
        }
        "search-engines" | "translators" => {
            sh.open_settings(Some("advanced_settings"));
            if scene == "search-engines" {
                sh.prefs.search_custom.push(("custom_1".into(), "Kagi".into(), "https://kagi.com/search?q={query}".into()));
                sh.prefs.search_enabled.extend(["bing".to_string(), "custom_1".to_string()]);
                sh.open_account_modal(crate::settings::AccountModal::Providers(crate::settings::search_engines::Mode::Search));
            } else {
                sh.prefs.translators_custom.push(("custom_1".into(), "LibreTranslate".into(), "https://translate.example.com/?q={query}".into()));
                sh.prefs.translators_enabled.extend(["deepl".to_string(), "custom_1".to_string()]);
                sh.open_account_modal(crate::settings::AccountModal::Providers(crate::settings::search_engines::Mode::Translate));
            }
        }
        "encoder" => {
            sh.open_settings(Some("advanced_settings"));
            sh.open_account_modal(crate::settings::AccountModal::EncoderControls);
        }
        "advanced-media" | "advanced-autocomplete" | "advanced-preview" => {
            sh.open_settings(Some("advanced_settings"));
            let id = match scene {
                "advanced-media" => "chat-settings-media-buttons",
                "advanced-autocomplete" => "chat-settings-expression-autocomplete",
                _ => "voice-video-screen-share-preview-behavior",
            };
            sh.open_account_modal(crate::settings::AccountModal::SwitchGroup(id));
        }
        "advanced-neko" => {
            sh.prefs.look.show_neko = true;
            sh.neko.at = Some((620.0, 420.0));
            crate::look::set(&sh.prefs.look);
        }
        "advanced-trust" => {
            sh.open_settings(Some("advanced_settings"));
            sh.confirm_trust_all = true;
        }
        "bluesky" => {
            sh.bluesky = true;
            sh.open_settings(Some("linked_accounts"));
            sh.form.data.insert("connections".into(), json!([]));
            sh.open_account_modal(crate::settings::AccountModal::AddConnection);
            sh.form.data.insert("m.ctype".into(), json!("bsky"));
        }
        "badges" => {
            sh.open_settings(Some("my_profile"));
            sh.form.data.insert("p.timezone".into(), json!("Asia/Seoul"));
            sh.form.data.insert("p.timezone_privacy_flags".into(), json!(reflux_model::ProfileFieldPrivacyFlags::EVERYONE.bits()));
            let b = |key: &str, label: &str, caption: Option<&str>| json!({"key": key, "kind": key, "label": label, "asset": null, "asset_url": null, "aspect_ratio": null, "caption": caption, "href": null, "hidden": false});
            let own = [b("staff", "Fluxer Staff", Some("S")), b("visionary", "Visionary since March 2025", Some("V")), b("visionary_number", "Visionary #42", Some("#42")), b("operator_pass", "Operator Pass since November 2026", Some("OP"))];
            sh.form.data.insert(crate::settings::badges::PREVIEW.into(), json!({"profile_badges": [own[0], own[1]], "own_badges": own}));
            sh.form.data.insert("p.badges".into(), json!((["visionary_number"], [false, true, false, false])));
        }
        "status-emoji" => {
            super::status::open(sh, ShellModal::CustomStatus);
            sh.form.text.insert("m.status".into(), rune::TextEdit::new("Shipping the status picker"));
            sh.form.data.insert("m.emoji".into(), json!({"emoji_name": "🚀"}));
            super::expr::open_status(sh);
        }
        "accounts" => super::status::open(sh, ShellModal::Accounts),
        "nickname" => super::status::open(sh, ShellModal::Nickname(Snowflake(HQ))),
        "sudo" => {
            let call = crate::guild::Call { method: reqwest::Method::POST, path: "/users/@me/disable".into(), body: None, then: crate::guild::Then::SignOut };
            crate::settings::sudo::require(sh, call, &json!({"has_mfa": true, "methods": {"totp": true, "webauthn": true, "email_code": true}}));
        }
        "profile" => {
            sh.account.profiles.insert((Snowflake(1001), Some(Snowflake(HQ))), Ok(json!({"user_profile": {"bio": "Building **Reflux**.", "pronouns": "she/her"}, "mutual_guilds": [{"id": HQ.to_string()}]})));
            sh.account.profile_modal = Some((Snowflake(1001), Some(Snowflake(HQ)), "overview"));
        }
        // `UserProfileBadges` with ten badges: the popout shows 7 and "+3"; `-tip` hovers the
        // detailed one, `-list` opens `BadgeListModal`, `-full` the full profile (all ten).
        "profile-badges" | "profile-badges-tip" | "profile-badges-list" | "profile-badges-full" => {
            let b = |i: usize| {
                let mut v = json!({"key": format!("custom:{i}"), "kind": "custom", "label": format!("Badge {i}"), "asset": null,
                    "asset_url": format!("https://media.fluxer.app/emojis/{}.webp", 76 + i), "caption": null, "href": null, "quote": null});
                if i == 1 {
                    v["details"] = json!({"title": "Early Supporter", "lines": ["Joined before launch", "Since March 2025"]});
                    v["quote"] = json!("Here from day one");
                }
                if i == 2 {
                    v = json!({"key": "visionary_number", "kind": "visionary_number", "label": "Visionary #42", "asset": null, "asset_url": null, "caption": "#42", "href": null});
                }
                v
            };
            let badges: Vec<Value> = (1..=10).map(b).collect();
            let g = Some(Snowflake(HQ));
            sh.account.profiles.insert((Snowflake(1001), g), Ok(json!({"user_profile": {"bio": "Building **Reflux**.", "pronouns": "she/her"}, "profile_badges": badges})));
            match scene {
                "profile-badges-full" => sh.account.profile_modal = Some((Snowflake(1001), g, "overview")),
                _ => sh.open_profile(Snowflake(1001), g, String::new(), "member-1001".into(), crate::ui::overlay::Placement::LeftStart),
            }
            if scene == "profile-badges-tip" {
                sh.popups.tip = Some("profile-1001-false-badge-0".into());
            }
            if scene == "profile-badges-list" {
                sh.media_ui.badge_list = Some(badges.iter().map(|v| super::badges::from_profile(sh, v)).collect());
            }
        }
        _ => {}
    }
}

/// `--guild-ui=<scene>`: community management mid-use. `menu`, `add|create|join`, `invite[-edit]`,
/// `channel|category|channel-access`, `notif`, `privacy`, `gs-<tab>` (community settings), `role`, `delete`,
/// `cs-overview|cs-permissions|cs-overwrite` (channel settings of #dev), `cs-voice`.
fn guild_scene(st: &mut ShellState, scene: &str) {
    use crate::guild::{AddView, Confirm, Dialog};
    let g = Snowflake(HQ);
    let data = |st: &mut ShellState, k: &str, v: Value| {
        st.form.data.insert(k.into(), v);
    };
    match scene {
        "menu" => st.popups.toggle(crate::guild::HEADER_MENU),
        "voice-chat" => st.select_guild(Some(g), Some(Snowflake(2301))),
        "voice-join-confirm" => {
            st.select_guild(Some(g), Some(Snowflake(2301)));
            st.prefs.look.confirm_join_voice = true;
            st.join_confirm = Some((Some(g), Snowflake(2301)));
        }
        "voice-chat-joined" => {
            st.select_guild(Some(g), Some(Snowflake(2301)));
            st.voice.channel = Some((Some(g), Snowflake(2301)));
            st.voice.connected = true;
        }
        "add" => st.open_dialog(Dialog::Add(AddView::Landing)),
        "create" => st.open_dialog(Dialog::Add(AddView::Create)),
        "join" => st.open_dialog(Dialog::Add(AddView::Join)),
        "template" => st.open_dialog(Dialog::Add(AddView::Import(crate::guild::template::Step::Url))),
        "template-json" => {
            st.open_dialog(Dialog::Add(AddView::Import(crate::guild::template::Step::Json)));
            st.form.data.insert("tpl-api".into(), json!("https://discord.com/api/guilds/templates/abcd1234"));
            st.form.data.insert("tpl-failed".into(), json!(true));
        }
        "template-create" => {
            st.open_dialog(Dialog::Add(AddView::Import(crate::guild::template::Step::Url)));
            let v = json!({"code": "abcd1234", "name": "Gaming Community", "serialized_source_guild": {"name": "Gaming Hub",
                "roles": [{"id": 0, "name": "@everyone"}, {"id": 1, "name": "Mods"}, {"id": 2, "name": "Members"}],
                "channels": [{"id": 3, "type": 4, "name": "Text", "position": 0}, {"id": 4, "type": 0, "name": "general", "position": 0, "parent_id": 3},
                    {"id": 5, "type": 0, "name": "clips", "position": 1, "parent_id": 3}, {"id": 6, "type": 2, "name": "Lounge", "position": 2}]}});
            let (name, g) = crate::guild::template::parse_response(&v).expect("demo template");
            crate::guild::template::apply(st, name, g, true);
        }
        "invite" | "invite-edit" => {
            st.open_dialog(Dialog::Invite(Snowflake(GENERAL), scene == "invite-edit"));
            st.invite_base = "https://fluxer.gg".into();
            data(st, "invite", json!({"code": "AbCd12", "max_age": 604800, "max_uses": 0}));
            data(st, "invite-sent", json!(["1001"]));
        }
        "channel" | "category" => st.open_dialog(Dialog::CreateChannel { guild: g, parent: Some(Snowflake(2200)), category: scene == "category" }),
        "channel-access" => {
            st.open_dialog(Dialog::CreateChannel { guild: g, parent: Some(Snowflake(2200)), category: false });
            data(st, "private", json!(true));
            data(st, "step", json!("access"));
            data(st, "access", json!(["0:2011"]));
        }
        "notif" => st.open_dialog(Dialog::Notifications(g)),
        "privacy" => st.open_dialog(Dialog::Privacy(g)),
        "role" => {
            st.open_dialog(Dialog::Guild { guild: g, tab: "roles", role: Some(Snowflake(2011)) });
            data(st, "role-emoji", json!({"id": null, "name": "🛡️"}));
        }
        "role-emoji" => {
            st.open_dialog(Dialog::Guild { guild: g, tab: "roles", role: Some(Snowflake(2011)) });
            data(st, "role-emoji", json!({"id": "9100", "name": "fluxer"}));
        }
        "invite-page" | "invite-modal" => {
            st.open_dialog(Dialog::Guild { guild: g, tab: "overview", role: None });
            st.form.text.insert("name".into(), rune::TextEdit::new("Fluxer HQ (draft)"));
            data(st, "splash_align", json!(1));
            data(st, "invite-preview", json!(scene.trim_start_matches("invite-")));
        }
        "crop-banner" | "crop-role" => {
            let role = scene == "crop-role";
            st.open_dialog(Dialog::Guild { guild: g, tab: if role { "roles" } else { "overview" }, role: role.then_some(Snowflake(2011)) });
            let img = image::RgbaImage::from_fn(1200, 700, |x, y| image::Rgba([(x / 5) as u8, (y / 3) as u8, 160, 255]));
            let mut png = Vec::new();
            let _ = img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
            let (key, asset) = if role { ("role-icon", crate::settings::crop::Asset::RoleIcon) } else { ("banner", crate::settings::crop::Asset::GuildBanner) };
            st.nav.crop = crate::settings::crop::Crop::new(key, asset, png);
        }
        "delete" => {
            st.open_dialog(Dialog::Guild { guild: g, tab: "overview", role: None });
            st.form.confirm = Some(Confirm::DeleteGuild(g));
        }
        "cs-voice" => {
            st.open_dialog(Dialog::Channel { channel: Snowflake(2301), tab: "overview", target: None });
            data(st, "regions", json!([{"id": "automatic", "name": "Automatic", "emoji": ""}, {"id": "eu-central", "name": "Europe Central", "emoji": ""}, {"id": "us-east", "name": "US East", "emoji": ""}]));
        }
        "cs-overview" | "cs-permissions" | "cs-overwrite" => {
            let tab = if scene == "cs-overview" { "overview" } else { "permissions" };
            st.open_dialog(Dialog::Channel { channel: Snowflake(2202), tab, target: (scene == "cs-overwrite").then_some(Snowflake(2011)) });
        }
        s => {
            let mut tab = crate::guild::tab_ids().find(|t| s.strip_prefix("gs-") == Some(*t)).unwrap_or("overview");
            if s == "gs-emoji-search" {
                tab = "emoji";
            } else if s == "gs-stickers-search" {
                tab = "stickers";
            }
            st.open_dialog(Dialog::Guild { guild: g, tab, role: None });
            if s == "gs-emoji-search" {
                st.form.text.insert("emoji-search".into(), rune::TextEdit::new("flux"));
            } else if s == "gs-stickers-search" {
                st.form.text.insert("sticker-search".into(), rune::TextEdit::new("ship"));
            }
            let m = |u: u64, r: &[u64]| json!({"user": {"id": u.to_string(), "username": format!("user{u}"), "global_name": null}, "roles": r.iter().map(|x| x.to_string()).collect::<Vec<_>>(), "joined_at": "2025-01-01T00:00:00Z"});
            data(st, "members", json!([m(ME, &[2010]), m(1001, &[2010, 2012]), m(1002, &[2011]), m(1003, &[2012]), m(1004, &[])]));
            data(st, "invites", json!([{"code": "AbCd12", "uses": 3, "max_uses": 0, "max_age": 0, "inviter": {"id": "1001", "username": "ada", "global_name": "Ada Lovelace"}, "channel": {"id": GENERAL.to_string(), "name": "general", "type": 0}},
                                      {"code": "Zz99", "uses": 1, "max_uses": 10, "max_age": 86400, "expires_at": "2026-12-01T00:00:00Z", "inviter": {"id": "1000", "username": "square"}, "channel": {"id": "2001", "name": "welcome", "type": 0}}]));
            data(st, "bans", json!([{"user": {"id": "1009", "username": "spammer", "global_name": "Spam Bot"}, "reason": "Spam links"}]));
            let e = |n: u64, user: u64, t: u32, target: &str| json!({"id": id_at(at(0, 9, 0), n), "user_id": user.to_string(), "action_type": t, "target_id": target});
            data(st, "advanced", json!(true));
            if s == "gs-roles-drag" {
                st.dialog = Some(Dialog::Guild { guild: g, tab: "roles", role: None });
                data(st, "role-drag", json!([0, 2]));
            }
            if s.starts_with("gs-threshold") {
                data(st, "mht", json!(true));
                data(st, "mht-on", json!(true));
            }
            if s == "gs-threshold-cal" {
                st.popups.open = Some(format!("mht-date@{}", chrono::Local::now().format("%Y-%m")));
            }
            data(st, "emoji-failed", json!([{"name": "huge.png", "error": "Image is too large to process (9000x9000)"}]));
            let mut role = e(2, ME, 31, "2011");
            role["changes"] = json!([{"key": "name", "old_value": "Mods", "new_value": "Moderators"}, {"key": "hoist", "old_value": false, "new_value": true}, {"key": "icon_hash", "old_value": "a1b2", "new_value": null}, {"key": "permissions", "old_value": "0", "new_value": "8194"}, {"key": "color", "old_value": 0, "new_value": 5793266}]);
            let mut ban = e(1, 1001, 22, "1009");
            ban["reason"] = json!("Spam%20links");
            ban["options"] = json!({"delete_message_seconds": 86400});
            let mut channel = e(3, 1002, 10, "2204");
            channel["changes"] = json!([{"key": "name", "new_value": "announcements"}, {"key": "type", "new_value": 5}, {"key": "topic", "new_value": "News and updates"}]);
            let mut member = e(4, 1001, 24, "1002");
            member["changes"] = json!([{"key": "nick", "old_value": "grace", "new_value": "Admiral"}]);
            data(st, "audit-open", json!(role["id"].clone()));
            let mut emoji = e(6, ME, 60, "77");
            emoji["changes"] = json!([{"key": "name", "new_value": "fluxer"}]);
            data(st, "audit", json!({"audit_log_entries": [emoji, ban, role, channel, member, e(5, 0, 999, "")],
                                     "users": [{"id": "1001", "username": "ada", "global_name": "Ada Lovelace"}, {"id": "1009", "username": "spammer"}]}));
        }
    }
}

/// A gradient standing in for decoded video frames.
fn fake_frames(px: &rune::Pixels, tint: u8) {
    fake_video_frames(px, 320, 180, tint);
}

fn fake_video_frames(px: &rune::Pixels, w: u32, h: u32, tint: u8) {
    let rgba = (0..w * h).flat_map(|i| [(i % w * 255 / w) as u8, (i / w * 255 / h) as u8, tint, 255]).collect();
    px.set(w, h, rgba);
}

/// `--voice=watch*`: the stream viewer's live, buffering, stalled, failed, ended, own-preview and
/// popup scenes. The viewer state is what `watch_step` would derive; the fixture store already
/// marks Alan (1004) as streaming in Lounge.
fn watch_scene(st: &mut ShellState, scene: &str) {
    use super::voice::WatchState;
    fn live(st: &mut ShellState) {
        fake_video_frames(&st.voice.stream, 1280, 720, 160);
        st.voice.stream_frame();
        (st.voice.watch_frame, st.voice.watch_fps, st.voice.watch_state) = (1, 30.0, WatchState::Live);
        st.voice.watch_frame_at = Some(std::time::Instant::now());
    }
    if scene == "watch-self" {
        st.voice.begin_watch(Snowflake(ME));
        st.voice.screen = true;
        return;
    }
    st.voice.begin_watch(Snowflake(1004));
    match scene {
        "watch-buffering" => (),
        "watch-stalled" => {
            live(st);
            st.voice.watch_frame_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(6));
        }
        "watch-failed" => st.voice.watch_state = WatchState::Failed(super::voice::FIRST_FRAME_TIMEOUT_CODE),
        "watch-ended" | "watch-ended-menu" => {
            live(st);
            st.voice.watch_state = WatchState::Ended;
            if scene == "watch-ended-menu" {
                st.voice.stream_pinned = true;
                st.popups.open = Some("stream-settings".into());
            }
        }
        "watch-spectators" => {
            live(st);
            st.popups.open = Some("stream-spectators-btn".into());
        }
        "watch-profile" => {
            live(st);
            st.open_profile(Snowflake(1001), Some(Snowflake(HQ)), String::new(), "stream-spectators-btn".into(), crate::ui::Placement::LeftStart);
        }
        "watch-menu" => {
            live(st);
            st.voice.settings.stream_volumes.insert(1004, 60.0);
            st.popups.open = Some("stream-settings".into());
        }
        _ => {
            live(st);
            if matches!(scene, "watch-full" | "watch-full-menu") {
                st.voice.stream_fullscreen = true;
                if scene == "watch-full-menu" {
                    st.popups.open = Some("stream-settings".into());
                }
            }
        }
    }
}

/// `--voice[=connecting|failed|watch]`: connected to Lounge (Alan speaking, sharing his screen).
/// `call|focus|details|tile-menu|camera|share|full|refused`: the call view and voice dialogs;
/// `menu-input|menu-output|menu-camera|menu-screen|menu-more`: the in-call gear menus;
/// `participants|stats|fullscreen|hud-hidden|audio-only|teleport|hide-camera`: full call view
/// features; `compact|compact-tall|compact-unread|compact-stream`: the compact call surface;
/// `incoming|dm`: a DM call ringing us / joined; `voice-message|meter`: the composer, mic test.
fn voice_scene(st: &mut ShellState, store: &mut Store, scene: &str) {
    use super::voice::Modal;
    let lounge = (Some(Snowflake(HQ)), Snowflake(2301));
    let apply = |store: &mut Store, t: &str, d: Value| {
        if let Ok(ev) = DispatchEvent::parse(t, &d) {
            store.apply(&ev, 0);
        }
    };
    match scene {
        "failed" => {
            st.voice.failed = Some(lounge);
            return;
        }
        // P2P: the start dialog, the join dialog, a call (indicator) and a failed peer.
        "p2p-start" => {
            (st.guild, st.channel) = (None, Some(Snowflake(8001)));
            st.voice.p2p_rollout = (true, 4);
            st.voice.modal = Some(Modal::P2pConsent { guild: None, channel: Snowflake(8001), start: true, allow_standard: true, video: false, always: false });
            return;
        }
        "incoming" | "dm" | "p2p" | "p2p-join" | "p2p-failed" => {
            let p2p = scene.starts_with("p2p");
            let mut states = vec![json!({"guild_id": null, "user_id": "1001", "channel_id": "8001", "connection_id": "c1001", "p2p": p2p})];
            if scene == "dm" || scene == "p2p" || scene == "p2p-failed" {
                states.push(json!({"guild_id": null, "user_id": ME.to_string(), "channel_id": "8001", "connection_id": "demo", "self_mute": true, "p2p": p2p}));
            }
            for v in &states {
                apply(store, "VOICE_STATE_UPDATE", v.clone());
            }
            let ringing = if scene == "incoming" { json!([ME.to_string()]) } else { json!([]) };
            apply(store, "CALL_CREATE", json!({"channel_id": "8001", "message_id": "1", "region": null, "ringing": ringing, "voice_states": states, "recipients": ["1001", ME.to_string()]}));
            (st.guild, st.channel) = (None, Some(Snowflake(8001)));
            if scene == "dm" || scene == "p2p" || scene == "p2p-failed" {
                st.voice.channel = Some((None, Snowflake(8001)));
                (st.voice.connected, st.voice.self_mute, st.voice.encrypted) = (true, true, !p2p);
                st.voice.speaking.insert(Snowflake(1001));
            }
            match scene {
                "p2p-join" => {
                    st.voice.p2p_rollout = (true, 4);
                    st.voice.modal = Some(Modal::P2pConsent { guild: None, channel: Snowflake(8001), start: false, allow_standard: false, video: false, always: true });
                }
                "p2p-failed" => {
                    st.voice.p2p_failed = vec![Snowflake(1001)];
                    let name = store.display_name(None, Snowflake(1001));
                    st.voice.modal = Some(Modal::P2pFailed { names: vec![name], allow_switch: true });
                }
                _ => {}
            }
            return;
        }
        "new-device" => {
            st.voice.modal = Some(Modal::NewDevice(super::voice::NewDevice { name: "E1x2 OTG".into(), input: Some("E1x2 OTG, USB Audio".into()), output: None }));
            return;
        }
        "voice-message-rec" => {
            let bars = (0..24).map(|i| (i as f32 * 0.9).sin().abs() * 0.7 + 0.1).collect();
            st.voice.voice_message = Some(super::voice::VoiceMessage { channel: Snowflake(GENERAL), recording: true, secs: 7.3, bars, ..Default::default() });
            return;
        }
        "voice-message" | "voice-message-play" => {
            let bars = (0..40).map(|i| (i as f32 * 0.7).sin().abs() * 0.8 + 0.1).collect();
            // `-play`: the selection preview 1.2 s in (playhead + Pause).
            let playing = (scene == "voice-message-play").then(|| std::time::Instant::now() - std::time::Duration::from_millis(1200));
            st.voice.voice_message = Some(super::voice::VoiceMessage { channel: Snowflake(GENERAL), recording: false, secs: 4.2, end: 3.6, start: 0.4, bars, playing, ..Default::default() });
            return;
        }
        "meter" | "meter-ptt" => {
            st.open_settings(Some("voice_video"));
            st.voice.settings.push_to_talk = scene == "meter-ptt";
            (st.voice.mic_test, st.voice.level) = (true, -28.0);
            (st.voice.settings.vad_auto, st.voice.settings.processing) = (false, "custom".into());
            return;
        }
        _ => {}
    }
    let me = json!({"guild_id": HQ.to_string(), "user_id": ME.to_string(), "channel_id": "2301", "connection_id": "demo", "self_mute": true});
    if let Ok(ev) = DispatchEvent::parse("VOICE_STATE_UPDATE", &me) {
        store.apply(&ev, 0);
    }
    // The test instance grants `feature_higher_video_quality` (16 Mbps ceiling), like the
    // voice snapshots of a server that sells no Plutonium.
    st.limits.higher_video_quality = true;
    st.limits.screen_share_bitrate = 16_000_000;
    st.voice.channel = Some(lounge);
    st.voice.self_mute = true;
    st.voice.connected = scene != "connecting";
    if st.voice.connected {
        (st.voice.ping, st.voice.pings) = (Some(42), vec![40, 41, 42]);
        st.voice.speaking.insert(Snowflake(1004));
    }
    if scene.starts_with("watch") {
        watch_scene(st, scene);
        if scene == "watch-source-ended" {
            st.voice.share_ended = true;
        }
        if matches!(scene, "watch-spectators" | "watch-profile") {
            let key = format!("{}:2301:c1004", HQ);
            for (u, conn) in [(1001u64, "c1001"), (1003, "c1003")] {
                apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": u.to_string(), "channel_id": "2301", "connection_id": conn, "viewer_stream_keys": [key]}));
            }
        }
    }
    if matches!(scene, "call" | "own-share" | "focus" | "details" | "tile-menu" | "tile-stream" | "share-menu" | "share-menu-res" | "camera" | "share" | "share-fps" | "share-devices" | "share-pick" | "full" | "refused" | "processing" | "trim"
        | "menu-input" | "menu-output" | "menu-camera" | "menu-screen" | "menu-screen-active" | "menu-more" | "participants" | "stats" | "fullscreen" | "hud-hidden" | "audio-only" | "teleport" | "hide-camera"
        | "compact" | "compact-tall" | "compact-unread" | "compact-stream" | "compact-watch") {
        apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": "1001", "channel_id": "2301", "connection_id": "c1001", "self_video": true}));
        fake_frames(st.voice.videos.entry(Snowflake(1001)).or_default(), 90);
        (st.voice.call_view, st.voice.encrypted) = (true, true);
        st.voice.server = Some(("livekit.fluxer.app".into(), "2301".into()));
        st.voice.pings = vec![40, 44, 41, 52, 47, 42, 39, 61, 45, 42];
    }
    match scene {
        // The channel list's voice rows: a user limit, a user on two devices, a LIVE row's
        // hover card, the devices expanded, a collapsed category connected or not.
        "sidebar" | "sidebar-card" | "sidebar-grouped" | "sidebar-collapsed" | "sidebar-collapsed-away" | "sidebar-profile" | "sidebar-tip" => {
            if let Some(c) = store.channels.get_mut(&Snowflake(2301)) {
                c.user_limit = Some(10);
            }
            // Pairing needs a role to connect (@everyone denied CONNECT); the call is encrypted.
            if let Some(c) = store.channels.get_mut(&Snowflake(2302)) {
                c.permission_overwrites = vec![reflux_model::PermissionOverwrite { id: Snowflake(HQ), deny: reflux_model::Permissions::CONNECT, ..Default::default() }];
            }
            st.voice.encrypted = true;
            for (conn, mobile) in [("c1001a", false), ("c1001b", true)] {
                apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": "1001", "channel_id": "2301", "connection_id": conn, "is_mobile": mobile, "self_mute": true}));
            }
            apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": "1002", "channel_id": "2302", "connection_id": "c1002", "mute": true}));
            match scene {
                "sidebar-card" => st.stream_card.0 = Some("vu-2301-1004".into()),
                "sidebar-profile" => st.open_profile(Snowflake(1003), Some(Snowflake(HQ)), String::new(), "vu-2301-1003".into(), crate::ui::Placement::RightStart),
                "sidebar-tip" => st.popups.tip = Some("vu-2302-1002-mute".into()),
                "sidebar-grouped" => drop(st.voice_expanded.insert(Snowflake(1001))),
                "sidebar-collapsed" => drop(st.collapsed.insert(Snowflake(2300))),
                "sidebar-collapsed-away" => {
                    st.collapsed.insert(Snowflake(2300));
                    st.voice.channel = None;
                    apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": ME.to_string(), "channel_id": null, "connection_id": "demo"}));
                }
                _ => {}
            }
        }
        "focus" => {
            st.voice.focus = Some(Snowflake(1004));
            st.voice.watching = None;
            st.call.layout = super::call::LayoutMode::Focus;
        }
        "menu-input" => {
            (st.call.meter_on, st.voice.mic_test, st.voice.level) = (true, true, -28.0);
            st.popups.open = Some(super::call::MENU_INPUT.into());
        }
        "menu-output" => st.popups.open = Some(super::call::MENU_OUTPUT.into()),
        "menu-camera" => {
            st.voice.cameras = Some(vec!["Integrated Camera".into(), "Virtual Camera".into()]);
            st.popups.open = Some(super::call::MENU_CAMERA.into());
        }
        "menu-screen" => st.popups.open = Some(super::call::MENU_SCREEN.into()),
        "menu-screen-active" => {
            st.voice.screen = true;
            apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": ME.to_string(), "channel_id": "2301", "connection_id": "demo", "self_stream": true, "self_mute": true}));
            st.popups.open = Some(super::call::MENU_SCREEN.into());
        }
        "menu-more" => {
            apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": null, "user_id": "1001", "channel_id": "8001", "connection_id": "c1001"}));
            apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": null, "user_id": ME.to_string(), "channel_id": "8001", "connection_id": "demo", "self_mute": true}));
            apply(store, "CALL_CREATE", json!({"channel_id": "8001", "message_id": "1", "region": "eu-central", "ringing": [], "voice_states": [], "recipients": ["1001", ME.to_string()]}));
            (st.guild, st.channel) = (None, Some(Snowflake(8001)));
            st.voice.channel = Some((None, Snowflake(8001)));
            (st.voice.connected, st.voice.self_mute) = (true, true);
            st.call.region = Some("eu-central".into());
            st.expr.fetched.insert(
                "voice-regions-8001".into(),
                super::expr::Fetched::Done(json!([
                    {"id": "automatic", "name": "Automatic"},
                    {"id": "eu-central", "name": "EU Central"},
                    {"id": "us-east", "name": "US East"}
                ])),
            );
            st.popups.open = Some(super::call::MENU_MORE.into());
            st.popups.sub = Some("voice-region".into());
        }
        "participants" => {
            store.messages.entry(Snowflake(2301)).or_default();
            for i in 0..3u64 {
                apply(store, "MESSAGE_CREATE", message(1_700_000_000_000 + i, i, 1001, "see you in the lounge", json!({"channel_id": "2301"})));
            }
            st.popups.open = Some(super::call::PARTICIPANTS.into());
        }
        "stats" => {
            st.call.connected_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(23 * 60 + 41));
            st.call.stats = Some(super::call::CallStats { audio_loss: 0.4, video_loss: 1.2, jitter_ms: 7.0, send_bitrate: 96_000.0, recv_bitrate: 184_000.0 });
            st.popups.open = Some(super::call::DETAILS.into());
        }
        "fullscreen" => {
            st.call.app_fullscreen = true;
            (st.guild, st.channel) = (Some(Snowflake(HQ)), Some(Snowflake(2301)));
        }
        "hud-hidden" => st.call.hud_until = Some(std::time::Instant::now() - std::time::Duration::from_millis(1)),
        "audio-only" => {
            st.voice.videos.clear();
            st.voice.camera = false;
            st.voice.watching = None;
            apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": "1004", "channel_id": "2301", "connection_id": "c1004", "self_stream": false}));
        }
        "teleport" => {
            st.call.teleport_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
            st.call.region = Some("eu-central".into());
        }
        "hide-camera" => st.call.hide_camera_confirm = Some(false),
        "compact" | "compact-tall" | "compact-unread" | "compact-stream" | "compact-watch" => {
            (st.guild, st.channel) = (Some(Snowflake(HQ)), Some(Snowflake(2301)));
            st.voice.call_view = false;
            st.voice.videos.clear();
            apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": "1004", "channel_id": "2301", "connection_id": "c1004", "self_stream": false}));
            if scene == "compact-tall" {
                st.prefs.compact_call_heights.insert(Snowflake(2301), 300.0);
            }
            if scene == "compact-unread" {
                store.messages.entry(Snowflake(2301)).or_default();
                for i in 0..3u64 {
                    apply(store, "MESSAGE_CREATE", message(1_700_000_000_000 + i, i, 1001, "see you in the lounge", json!({"channel_id": "2301"})));
                }
            }
            if scene == "compact-stream" {
                apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": "1004", "channel_id": "2301", "connection_id": "c1004", "self_stream": true}));
                let bytes = Some(std::sync::Arc::new(include_bytes!("../../../../assets/media/images/app-icon.png").to_vec()));
                st.voice.previews.insert(format!("{HQ}:2301:c1004"), super::active_now::Preview { bytes, fetched_ms: 1, ..Default::default() });
                st.prefs.compact_call_heights.insert(Snowflake(2301), 260.0);
            }
            // `-watch`: a stream with no preview thumbnail, being watched.
            if scene == "compact-watch" {
                apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": "1004", "channel_id": "2301", "connection_id": "c1004", "self_stream": true}));
                fake_frames(&st.voice.stream, 200);
                st.voice.watching = Some(Snowflake(1004));
                st.voice.focus = Some(Snowflake(1004));
                st.call.pin(Some(Snowflake(1004)));
                st.prefs.compact_call_heights.insert(Snowflake(2301), 260.0);
            }
        }
        "details" => st.popups.open = Some(super::call::DETAILS.into()),
        "tile-menu" => {
            st.popups.open_at(super::call::TILE_MENU, [700.0, 300.0]);
            st.target = Some(super::Target::User { user: Snowflake(1003), guild: Some(Snowflake(HQ)), dm: None });
        }
        "camera" => {
            st.voice.modal = Some(Modal::CameraPreview);
            st.voice.previewing = true;
            st.voice.cameras = Some(vec!["Integrated Camera".into()]);
            fake_frames(&st.voice.self_view, 220);
        }
        // The drop-downs open, so the snapshots show the offered lists.
        "share" => {
            crate::shell::call::open_screen_picker(st);
            st.popups.open = Some("share-res".into());
        }
        "software-encoder" => {
            st.voice.screen = true;
            st.voice.software_encoder = Some("H264");
        }
        "share-fps" => {
            crate::shell::call::open_screen_picker(st);
            st.popups.open = Some("share-fps".into());
        }
        "share-devices" => {
            st.voice.modal = Some(Modal::ScreenShare);
            st.voice.screen_tab = "devices".into();
            st.voice.cameras = Some(vec!["Integrated Camera".into(), "Virtual Camera".into()]);
            st.voice.screen_device = Some("Integrated Camera".into());
        }
        // The in-app picker (X11 / Windows) with fixture thumbnails and Linux app audio.
        "share-pick" => {
            st.voice.modal = Some(Modal::ScreenShare);
            let src = |window: bool, id: u64, title: &str, shade: u8| {
                let px = rune::Pixels::default();
                px.set(16, 9, (0..16 * 9).flat_map(|i| [shade, (i * 3) as u8, 160, 255]).collect());
                (reflux_voice::ScreenSource { window, id, title: title.into(), thumbnail: None }, px)
            };
            let list = vec![src(false, 1, "Screen 1", 40), src(false, 2, "Screen 2", 90), src(true, 10, "Firefox", 140), src(true, 11, "Terminal", 200)];
            st.voice.screen_pick = Some(list[2].0.clone());
            st.voice.screen_sources = Some(list);
            st.voice.audio_sources = Some(vec![("42".into(), "Firefox — Video".into())]);
            st.voice.settings.stream_audio = Some("42".into());
        }
        // Our own live share in the grid: its tile draws the local track.
        "own-share" => {
            apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": ME.to_string(), "channel_id": "2301", "connection_id": "demo", "self_stream": true, "self_mute": true}));
            st.voice.screen = true;
            fake_frames(&st.voice.own_share, 200);
        }
        "tile-stream" => {
            apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": "1004", "channel_id": "2301", "connection_id": "c1004", "self_stream": true}));
            st.voice.watching = Some(Snowflake(1004));
            fake_frames(&st.voice.stream, 160);
            st.voice.settings.stream_volumes.insert(1004, 60.0);
            st.popups.open_at(super::call::TILE_MENU, [700.0, 200.0]);
            st.target = Some(super::Target::User { user: Snowflake(1004), guild: Some(Snowflake(HQ)), dm: None });
        }
        // Our own live stream's menu (`ActiveScreenShareMenu`): Stop streaming + Stream quality.
        "share-menu" => {
            st.limits.higher_video_quality = true;
            st.limits.screen_share_bitrate = 16_000_000;
            apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": ME.to_string(), "channel_id": "2301", "connection_id": "demo", "self_stream": true, "self_mute": true}));
            st.voice.screen = true;
            st.popups.open_at(super::call::TILE_MENU, [700.0, 300.0]);
            st.popups.sub = Some("stream-quality".into());
            st.target = Some(super::Target::User { user: Snowflake(ME), guild: Some(Snowflake(HQ)), dm: None });
        }
        "share-menu-res" => {
            st.limits.higher_video_quality = true;
            st.limits.screen_share_bitrate = 16_000_000;
            apply(store, "VOICE_STATE_UPDATE", json!({"guild_id": HQ.to_string(), "user_id": ME.to_string(), "channel_id": "2301", "connection_id": "demo", "self_stream": true, "self_mute": true}));
            st.voice.screen = true;
            st.popups.open_at(super::call::TILE_MENU, [700.0, 300.0]);
            st.popups.sub = Some("stream-quality/stream-quality-res".into());
            st.target = Some(super::Target::User { user: Snowflake(ME), guild: Some(Snowflake(HQ)), dm: None });
        }
        "processing" => {
            st.voice.settings.processing = "custom".into();
            st.voice.modal = Some(Modal::AudioProcessing);
        }
        "trim" => {
            let samples: Vec<f32> = (0..48_000 * 8).map(|i| (i as f32 * 0.01).sin() * ((i / 4000) % 7) as f32 / 7.0).collect();
            let mut tr = super::voice::Trim { name: "airhorn".into(), samples: std::sync::Arc::new(samples), ..Default::default() };
            tr.set(true, 1.0);
            tr.set(false, 4.5);
            st.voice.trim = Some(tr);
            st.voice.modal = Some(Modal::Trim);
        }
        "full" => st.voice.modal = Some(Modal::ChannelFull),
        "refused" => st.voice.modal = Some(Modal::Refused("P2P calls are unavailable right now.".into())),
        _ => {}
    }
}

/// The composer's expression picker on a tab, with fixture answers.
fn expr_scene(st: &mut ShellState, scene: &str) {
    use super::expr::{Fetched, Tab};
    let tab = match scene {
        "picker-sticker" => Tab::Stickers,
        "picker-gif" | "picker-gif-search" => Tab::Gifs,
        "picker-memes" | "meme-delete" => Tab::Memes,
        _ => Tab::Emojis,
    };
    st.limits.global_expressions = true;
    st.gif_provider = "Klipy".into();
    let x = &mut st.expr.saved;
    x.favorite_emojis = vec![format!("custom:{HQ}:77"), "unicode:heart".into()];
    for (k, n) in [("unicode:thumbsup", 5), ("unicode:joy", 3), (&*format!("custom:{HQ}:79"), 2), ("unicode:tada", 1)] {
        x.emoji_usage.insert(k.to_string(), super::expr::Usage { count: n, last_used: st.now_ms });
    }
    x.sticker_usage.insert(
        format!("sticker:{HQ}:88"),
        super::expr::Usage {
            count: 2,
            last_used: st.now_ms,
        },
    );
    super::expr::toggle(st, Snowflake(GENERAL), tab);
    st.intents.clear();
    let gif = |i: u32, w: u32, h: u32| {
        json!({"id": i.to_string(), "url": format!("https://klipy.com/gifs/demo-{i}"), "title": "demo", "proxy_src": format!("https://media.fluxer.app/external/gif{i}.gif"),
               "width": w, "height": h, "media": {}})
    };
    let gifs: Vec<Value> = (0..12).map(|i| gif(i, 220, [124, 220, 160, 300][i as usize % 4])).collect();
    let cats: Vec<Value> = ["Happy", "Sad", "Thank you", "Dance", "Yes", "No", "Love", "Facepalm"]
        .iter()
        .enumerate()
        .map(|(i, n)| json!({"name": n, "src": "", "proxy_src": format!("https://media.fluxer.app/external/cat{i}.gif")}))
        .collect();
    st.expr.fetched.insert("gif:featured".into(), Fetched::Done(json!({"categories": cats, "gifs": gifs.clone()})));
    if scene == "picker-gif-search" {
        st.expr.query = rune::TextEdit::new("dance");
        st.expr.gif = super::expr::GifView::Search("dance".into());
        st.expr.fetched.insert("gif:search:dance".into(), Fetched::Done(Value::Array(gifs)));
    }
    if scene == "meme-delete" {
        st.expr.confirm_meme = Some(("501".into(), "meme 1".into()));
    }
    if scene == "gif-import" {
        let mut imp = super::expr::gif::GifImport::default();
        imp.text = rune::TextEdit::new("https://klipy.com/gifs/dance-1\nhttps://klipy.com/gifs/cat-2");
        st.expr.gif_import = Some(imp);
    }
    if scene == "picker-menu" {
        st.expr.menu_item = Some(super::expr::Item::Emoji { guild: Snowflake(HQ), id: Snowflake(77), name: "party".into(), animated: false });
        st.expr.menu.open = Some("expr-menu".into());
        st.expr.menu.at = Some([1000.0, 400.0]);
    }
    let meme = |i: u32, ct: &str| json!({"id": (500 + i).to_string(), "name": format!("meme {i}"), "content_type": ct, "tags": [],
        "url": format!("https://media.fluxer.app/attachments/memes/{i}.png"), "width": 400, "height": ([300, 400, 520][i as usize % 3])});
    st.expr.fetched.insert("memes".into(), Fetched::Done(json!([meme(1, "image/png"), meme(2, "image/png"), meme(3, "image/gif"), meme(4, "image/png"), meme(5, "image/png")])));
    if tab == Tab::Emojis {
        st.expr.hover = Some(format!("custom:{HQ}:77"));
    }
}

/// `--msg-scene=hover|shift|failed|send-error|menu|menu-selection|menu-search-submenu|more|emoji|reply|edit|delete`: message actions mid-use;
/// `uploads|pins|search|search-options|switcher|switcher-people|switcher-link|switcher-account|autocomplete|jump|present|slowmode|counter`: the
/// composer and navigation tools.
/// `picker-emoji|picker-sticker|picker-gif|picker-gif-search|picker-memes|react-picker|
/// reaction-tip|reactions-modal|expr-info|sticker-info`: the expression surfaces.
fn stage(st: &mut ShellState, store: &mut Store, scene: &str) {
    let ids: Vec<Snowflake> = store.messages(Snowflake(GENERAL)).iter().map(|m| m.id).collect();
    let (shot, mine, ada) = (ids[6], ids[7], ids[8]);
    match scene {
        "hover" => st.hover_msg = Some(ada),
        "debug-channel" => st.open_debug(super::debug::Subject::Channel(Snowflake(GENERAL))),
        "debug-ast" => st.debug = Some(super::debug::Debug { subject: super::debug::Subject::Message(Snowflake(GENERAL), ada), tab: 1 }),
        "crash" => crate::crash::show("panicked at crates/reflux/src/shell/mod.rs:1:1:\nTriggered React crash from Developer Tools menu".into()),
        "nagbars" => {
            st.desktop.notice = Some(reflux_model::InstanceNotice { id: "n1".into(), message: "Voice is moving to the new region tonight.".into(), url: Some("https://example.com".into()), ..Default::default() });
            st.desktop.maintenance = Some(crate::desktop::notices::Maintenance { id: "m1".into(), status: crate::desktop::notices::Status::Scheduled, start: "2026-11-08T03:00:00Z".into(), minutes: 120, url: "https://status.example.com".into() });
            st.premium = true;
            if let Some(me) = store.me.as_mut() {
                (me.premium_type, me.premium_until, me.premium_billing_cycle) = (Some(reflux_model::PremiumType::SUBSCRIPTION), Some("2026-12-07T00:00:00Z".into()), Some("monthly".into()));
                (me.has_unread_gift_inventory, me.unread_gift_inventory_count) = (true, 2);
            }
        }
        "shift" => {
            st.hover_msg = store.messages(Snowflake(GENERAL)).last().map(|m| m.id);
            st.shift_held = true;
        }
        // `/nick` answered by a Fluxerbot note.
        "client-system" => store.add_local_message(super::client_system::build(Snowflake(GENERAL), "You changed your nickname in this community from **Square** to **Sq**.".into(), st.now_ms)),
        "failed" | "send-error" => {
            let nonce = Snowflake::from_timestamp_ms(st.now_ms);
            let mut m: reflux_model::Message = serde_json::from_value(message(st.now_ms, 0, ME, "this one **didn't** make it, <@1001>", json!({}))).expect("message");
            (m.id, m.nonce) = (nonce, Some(nonce.to_string()));
            store.add_local_message(m);
            st.failed.insert(nonce, super::queue::Failed { channel: Snowflake(GENERAL), reply: None });
            st.hover_msg = Some(nonce);
            if scene == "send-error" {
                let e = reflux_net::Error::Api { status: reqwest::StatusCode::TOO_MANY_REQUESTS, error: Box::new(reflux_model::ApiError { code: "RATE_LIMITED".into(), retry_after: Some(4.2), ..Default::default() }) };
                st.notice = Some(super::queue::send_error(&e, false, 0));
            }
        }
        "menu" => {
            st.popups.open_at(&format!("msg-menu-{ada}"), [620.0, 330.0]);
            st.active_msg = Some(ada);
        }
        "menu-selection" => {
            st.msg_selection = Some("Release notes".into());
            st.popups.open_at(&format!("msg-menu-{ada}"), [620.0, 330.0]);
            st.active_msg = Some(ada);
        }
        "menu-search-submenu" => {
            st.msg_selection = Some("Release notes".into());
            st.prefs.search_enabled.push("bing".into());
            let mut se = Vec::new();
            crate::synced::bytes(&mut se, 1, b"google");
            store.settings.synced_preferences = crate::synced::set("", crate::shell::search_providers::SEARCH_ENGINES, Some(&se));
            st.popups.open_at(&format!("msg-menu-{ada}"), [620.0, 330.0]);
            st.popups.sub = Some("provider-1".into());
            st.active_msg = Some(ada);
        }
        "more" => {
            st.popups.open = Some(format!("msg-more-{mine}"));
            st.active_msg = Some(mine);
        }
        "emoji" => {
            st.popups.open = Some(format!("emoji-{shot}"));
            st.active_msg = Some(shot);
        }
        "reply" => st.reply = Some(super::ReplyTo { channel: Snowflake(GENERAL), message: ada, mention: true }),
        "edit" => {
            let m = store.messages(Snowflake(GENERAL))[7].clone();
            st.editing = Some((m.id, rune::TextEdit::new(m.content)));
        }
        "delete" => st.confirm_delete = Some(mine),
        "uploads" => {
            // A gradient stands in for a pasted screenshot.
            let img = image::RgbaImage::from_fn(320, 180, |x, y| image::Rgba([(x * 255 / 320) as u8, (y * 255 / 180) as u8, 160, 255]));
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).expect("png");
            super::upload::add_png(st, png);
            let notes = super::upload::Source::Bytes(std::sync::Arc::new(b"notes".to_vec()));
            super::upload::add(st, vec![("release-notes.pdf".into(), 1_572_864, notes)]);
            st.uploads[1].spoiler = true;
            st.composer = rune::TextEdit::new("Here are the files");
        }
        "pins" => {
            // Fluxer's default, so the screenshot shows scaled down to fit the card.
            store.settings.inline_attachment_media = true;
            st.popups.toggle(super::pins::POPOUT);
            let ms = store.messages(Snowflake(GENERAL));
            let items = [&ms[8], &ms[6]].map(|m| (m.clone(), m.timestamp.clone())).to_vec();
            st.pins.insert(Snowflake(GENERAL), super::pins::Pins { items, ..Default::default() });
        }
        "search" => {
            store.settings.inline_attachment_media = true;
            st.search.query = rune::TextEdit::new("from:ada ship");
            let messages = [8, 6].map(|i| store.messages(Snowflake(GENERAL))[i].clone()).to_vec();
            st.search.results = Some(super::search::Results::Done { messages, total: 2 });
            st.search.ran = "from:ada ship".into();
        }
        "search-options" => st.search.query.focused = true,
        // The filter list with the arrow keys on its third row.
        "search-nav" => {
            st.search.query.focused = true;
            st.search.nav = Some((String::new(), 2));
        }
        "search-dates" => {
            st.search.query = rune::TextEdit::new("release from:ada -has:link before:");
            st.search.query.focused = true;
            st.search.nav = Some((st.search.query.text.clone(), 1));
        }
        "search-scope" => st.popups.open = Some("search-scope".into()),
        "switcher" => {
            st.toggle_switcher();
            if let Some(sw) = &mut st.switcher {
                sw.query = rune::TextEdit::new("#");
                sw.query.focused = true;
            }
        }
        "switcher-people" => {
            st.toggle_switcher();
            if let Some(sw) = &mut st.switcher {
                sw.query = rune::TextEdit::new("@a");
                sw.query.focused = true;
            }
        }
        "switcher-link" | "switcher-account" => {
            st.toggle_switcher();
            st.web_base = "https://web.fluxer.app".into();
            if let Some(sw) = &mut st.switcher {
                sw.query = rune::TextEdit::new(if scene == "switcher-link" { "https://web.fluxer.app/channels/1/2/3" } else { "ada" });
                sw.query.focused = true;
            }
        }
        "switcher-empty" => {
            st.prefs.recent = vec![Snowflake(GENERAL), Snowflake(2202), Snowflake(2203)];
            st.toggle_switcher();
        }
        "pending-sticker" => {
            st.composer = rune::TextEdit::new("ship it");
            st.pending_sticker = store.guilds.values().flat_map(|g| g.stickers.first()).next().map(|x| (Snowflake(GENERAL), x.id));
        }
        "format" => {
            st.composer = rune::TextEdit::new("the release goes out on friday after the review");
            st.composer.focused = true;
            st.composer.set_selection(17, 37);
        }
        "autocomplete" => {
            st.composer = rune::TextEdit::new("hey @a");
            st.composer.focused = true;
        }
        // The /ban delete-history choices after a member.
        "command-choices" => {
            st.composer = rune::TextEdit::new("/ban <@1001> ");
            st.composer.focused = true;
        }
        "jump" => {
            st.highlight = Some(ids[4]);
            st.scroll_to = Some(format!("msg-{}", ids[4]));
        }
        "present" => {
            if let Some(w) = store.messages.get_mut(&Snowflake(GENERAL)) {
                w.has_more_after = true;
            }
        }
        m if m.starts_with("media-") => media_scene(st, store, &m[6..]),
        "picker-emoji" | "picker-menu" | "gif-import" | "meme-delete" | "picker-sticker" | "picker-gif" | "picker-gif-search" | "picker-memes" => expr_scene(st, scene),
        // The menu's "Clone emoji to..." open (HQ allows cloning), or the provider picker.
        "picker-clone" | "picker-ris" => {
            expr_scene(st, "picker-menu");
            if let Some(i) = store.guilds.get_mut(&Snowflake(HQ)).and_then(|g| g.info.as_mut()) {
                i.features.push("CLONE_EMOJI_ENABLED".into());
            }
            if scene == "picker-clone" {
                st.prefs.look.clone_shortcuts = true;
                crate::look::set(&st.prefs.look);
                st.expr.menu.sub = Some("expr-clone".into());
            } else {
                st.expr.menu.close();
                st.expr.image_search = Some(("https://fluxerusercontent.com/emojis/77.webp".into(), "google_lens".into()));
            }
        }
        "react-picker" => {
            st.limits.global_expressions = true;
            super::expr::open_react(st, Snowflake(GENERAL), shot);
        }
        "reaction-tip" | "reactions-modal" => {
            use super::expr::Fetched;
            // Linus's message is on screen; give it the screenshot's reactions.
            let shot = ids[ids.len() - 2];
            let rs = store.messages(Snowflake(GENERAL))[6].reactions.clone();
            if let Some(m) = store.messages.get_mut(&Snowflake(GENERAL)).and_then(|w| w.messages.iter_mut().find(|m| m.id == shot)) {
                m.reactions = rs;
            }
            let u = |id: u64, n: &str, g: &str| user(id, n, g);
            let all = json!([u(ME, "square", "Square"), u(1001, "ada", "Ada Lovelace"), u(1003, "grace", "Grace Hopper")]);
            st.expr.fetched.insert(format!("reactors:{shot}:👍:3"), Fetched::Done(all.clone()));
            st.expr.fetched.insert(format!("reactors:{shot}:👍:100"), Fetched::Done(all));
            if scene == "reaction-tip" {
                st.expr.tip = Some((Snowflake(GENERAL), shot, "👍".into()));
            } else {
                st.expr.reactions = Some((Snowflake(GENERAL), shot, "👍".into()));
            }
        }
        "expr-info" | "sticker-info" | "expr-info-clone" | "expr-info-join" | "expr-join" => {
            use super::expr::Fetched;
            st.prefs.look.clone_shortcuts = true;
            crate::look::set(&st.prefs.look);
            if scene == "expr-info-join" || scene == "expr-join" {
                // A foreign emoji from a discoverable community.
                let linus = ids[ids.len() - 2];
                let kind = reflux_markdown::EmojiKind::Custom { name: "partyparrot".into(), id: "99".into(), animated: false };
                super::expr::open_emoji_info(st, &kind, format!("m{linus}-t0-e0-img"));
                st.expr.fetched.insert("meta:emoji:99".into(), Fetched::Done(json!({"guild_id": "4242", "allow_cloning": false})));
                st.expr.fetched.insert("source:emoji:99".into(), Fetched::Done(json!({"id": "4242", "name": "Parrot Lovers", "icon": null, "features": ["DISCOVERABLE"]})));
                if scene == "expr-join" {
                    st.expr.info = None;
                    st.expr.join = Some((Snowflake(4242), "Parrot Lovers".into(), None, ":partyparrot:".into(), false));
                }
            } else if scene != "sticker-info" {
                // Linus's inline custom emoji (the first in his message body).
                let linus = ids[ids.len() - 2];
                let kind = reflux_markdown::EmojiKind::Custom { name: "fluxer".into(), id: "77".into(), animated: false };
                super::expr::open_emoji_info(st, &kind, format!("m{linus}-t0-e0-img"));
                st.expr.fetched.insert("meta:emoji:77".into(), Fetched::Done(json!({"guild_id": HQ.to_string(), "allow_cloning": true})));
                if let Some(i) = st.expr.info.as_mut() {
                    i.cloning = scene == "expr-info-clone";
                }
            } else {
                let m = store.messages(Snowflake(GENERAL)).iter().find(|m| !m.stickers.is_empty()).cloned();
                if let Some(m) = m {
                    super::expr::open_sticker_info(st, &m.stickers[0], format!("sticker-{}-88", m.id));
                    st.scroll_to = Some(format!("msg-{}", m.id));
                }
            }
            st.intents.clear();
        }
        "slowmode" | "counter" => {
            if let Some(c) = store.channels.get_mut(&Snowflake(GENERAL)) {
                c.rate_limit_per_user = 90;
            }
            // Owners bypass slowmode; hand the community to Ada for the shot.
            if let Some(info) = store.guilds.get_mut(&Snowflake(HQ)).and_then(|g| g.info.as_mut()) {
                info.owner_id = Snowflake(1001);
            }
            st.cooldowns.insert(Snowflake(GENERAL), st.now_ms + 75_000);
            if scene == "counter" {
                st.composer = rune::TextEdit::new("lorem ipsum ".repeat(166));
            }
        }
        _ => {}
    }
}

/// `--msg-scene=media-<scene>`: attachments and embeds. `mosaic` (several images, a spoiler,
/// alt text), `files` (video, audio, text preview, file card), `embeds` (rich, YouTube, gifv),
/// `invite`, `viewer` (the lightbox), `viewer-rotate` (turned 90°), `link` (external link
/// warning), `nsfw` (the channel gate), `blur` (flagged media under the guild filter),
/// `system` (system messages).
fn media_scene(st: &mut ShellState, store: &mut Store, scene: &str) {
    let t = |m| at(0, 12, m);
    let att = |id: u64, name: &str, ty: &str, w: u32, h: u32, extra: Value| {
        let mut a = json!({"id": id.to_string(), "filename": name, "content_type": ty, "size": 482133, "width": w, "height": h,
                           "url": format!("https://cdn.fluxer.app/attachments/2201/{id}/{name}"),
                           "proxy_url": format!("https://media.fluxer.app/attachments/2201/{id}/{name}")});
        if let (Some(o), Some(e)) = (a.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        a
    };
    let ext = |w: u32, h: u32| json!({"url": "https://example.com/x.png", "proxy_url": "https://media.fluxer.app/external/sig/x.png", "width": w, "height": h});
    let mut msgs = Vec::new();
    // Fluxer's defaults: spoilers on click, embeds and their media shown, GIFs autoplay.
    store.settings.render_spoilers = 1;
    (store.settings.render_embeds, store.settings.inline_embed_media, store.settings.inline_attachment_media, store.settings.gif_auto_play) = (true, true, true, true);
    match scene {
        // Own media hovered (the toolbar's Delete), then its confirmation.
        "delete" | "delete-confirm" => {
            msgs.push(message(
                t(1),
                0,
                ME,
                "",
                json!({"attachments": [att(9301, "mine.png", "image/png", 1200, 800, json!({}))]}),
            ));
            st.hover_msg = Some(Snowflake::from_timestamp_ms(t(1)));
            if scene == "delete-confirm" {
                st.media_ui.confirm_delete = Some((
                    Snowflake(GENERAL),
                    Snowflake::from_timestamp_ms(t(1)),
                    Snowflake(9301),
                ));
            }
        }
        "mosaic" | "viewer" | "viewer-rotate" => {
            msgs.push(message(t(1), 0, 1001, "Three from the offsite:", json!({"attachments": [
                att(9101, "a.png", "image/png", 1200, 800, json!({})), att(9102, "b.png", "image/png", 800, 800, json!({})),
                att(9103, "c.png", "image/png", 800, 1200, json!({"description": "The team on the stairs"}))]})));
            msgs.push(message(t(2), 0, 1003, "and five more", json!({"attachments": [
                att(9111, "d.png", "image/png", 1200, 800, json!({})), att(9112, "SPOILER_e.png", "image/png", 1200, 800, json!({})),
                att(9113, "f.gif", "image/gif", 400, 400, json!({})), att(9114, "g.png", "image/png", 400, 400, json!({})),
                att(9115, "h.png", "image/png", 400, 400, json!({}))]})));
        }
        "files" | "files-volume" => {
            if scene == "files-volume" {
                (st.media_ui.volume_pop, st.prefs.media_volume) = (Some(("voice-9205".into(), None)), 0.6);
            }
            msgs.push(message(t(1), 0, 1004, "", json!({"attachments": [att(9201, "demo.mp4", "video/mp4", 1280, 720, json!({}))]})));
            msgs.push(message(t(1), 1, 1001, "", json!({"flags": 8192, "attachments": [att(9205, "voice-message.wav", "audio/wav", 0, 0,
                json!({"width": null, "height": null, "size": 403200, "duration": 4.0, "waveform": "IEBgf5+/3/+/n39gQCBAYH+fv9//v59/YEAgQGB/n7/f/7+ff2BAIA=="}))]})));
            msgs.push(message(t(2), 0, 1002, "", json!({"attachments": [
                att(9202, "theme.ogg", "audio/ogg", 0, 0, json!({"width": null, "height": null, "size": 1843200, "duration": 94.0})),
                att(9203, "main.rs", "text/x-rust", 0, 0, json!({"width": null, "height": null, "size": 412})),
                att(9204, "release-0.4.tar.gz", "application/gzip", 0, 0, json!({"width": null, "height": null, "size": 15728640}))]})));
            st.media.seed_text("https://cdn.fluxer.app/attachments/2201/9203/main.rs",
                "use std::io;\n\n/// Greets the world.\nfn main() -> io::Result<()> {\n    let name = \"Fluxer\";\n    println!(\"hello, {name}\");\n    Ok(())\n}\n");
        }
        "embeds" => {
            msgs.push(message(t(0), 0, 1007, "", json!({"embeds": [{"type": "rich", "title": "Deploy ok", "description": "All green.", "color": 0x23A55A},
                {"type": "rich", "title": "Release notes", "description": "Threads, forum tags, the new inbox and a long list of fixes that wraps onto more than one line of the embed.", "url": "https://fluxer.app/notes"}]})));
            msgs.push(message(t(1), 0, 1007, "https://ci.fluxer.app/1843", json!({"embeds": [{"type": "rich", "title": "Build #1843 failed", "url": "https://ci.fluxer.app/1843",
                "description": "2 tests failed on `main`:\n- `store::ack`\n- `gateway::resume`", "color": 0xED4245,
                "author": {"name": "Fluxer CI", "url": "https://ci.fluxer.app", "icon_url": "https://ci.fluxer.app/i.png", "proxy_icon_url": "https://media.fluxer.app/external/sig/ci.png"},
                "fields": [{"name": "Branch", "value": "main", "inline": true}, {"name": "Duration", "value": "5m 03s", "inline": true},
                           {"name": "Commit", "value": "a91782b", "inline": true}, {"name": "Log", "value": "[open](https://ci.fluxer.app/1843/log)", "inline": false}],
                "image": ext(1280, 720),
                "footer": {"text": "Fluxer CI", "icon_url": "https://ci.fluxer.app/i.png", "proxy_icon_url": "https://media.fluxer.app/external/sig/ci.png"},
                "timestamp": iso(t(1))}]})));
            msgs.push(message(t(2), 0, 1002, "https://www.youtube.com/watch?v=dQw4w9WgXcQ", json!({"embeds": [{"type": "video", "url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                "title": "Fluxer launch trailer", "description": "Meet Fluxer.", "color": 0xFF0000,
                "provider": {"name": "YouTube", "url": "https://www.youtube.com"}, "author": {"name": "Fluxer", "url": "https://www.youtube.com/@fluxer"},
                "thumbnail": ext(1280, 720), "video": {"url": "https://www.youtube.com/embed/dQw4w9WgXcQ", "width": 1280, "height": 720}}]})));
            msgs.push(message(t(3), 0, 1003, "https://tenor.com/x.gif", json!({"embeds": [{"type": "gifv", "url": "https://tenor.com/x.gif",
                "thumbnail": ext(498, 280), "video": {"url": "https://media.tenor.com/x.mp4", "proxy_url": "https://media.fluxer.app/external/sig/x.mp4", "width": 498, "height": 280}}]})));
        }
        "invite" | "invite-tip" | "invite-list" => {
            msgs.push(message(t(1), 0, 1001, "come hang out https://fluxer.gg/rustlang and https://fluxer.gg/gone", json!({})));
            msgs.push(message(t(2), 0, 1003, "also https://fluxer.gg/hq and https://fluxer.gg/closed", json!({})));
            let inv = |code: &str, id: u64, name: &str, extra: Value| -> super::invite_embed::Preview {
                let mut g = json!({"id": id.to_string(), "name": name, "icon": "demo"});
                g.as_object_mut().expect("guild").extend(extra.as_object().cloned().unwrap_or_default());
                let v = json!({"code": code, "type": 0, "guild": g, "channel": {"id": "1", "name": "general", "type": 0}, "member_count": 12873, "presence_count": 2041});
                super::invite_embed::Preview::Ready(Box::new(serde_json::from_value(v).expect("invite")))
            };
            st.media_ui.invites.insert("rustlang".into(), inv("rustlang", 9900, "Rust Programming", json!({"features": ["VERIFIED"], "embed_splash": "demo"})));
            st.media_ui.invites.insert("gone".into(), super::invite_embed::Preview::Unknown);
            // Five custom instance badges: three show, then "+2".
            st.media_ui.guild_badges = (1..=5u64)
                .map(|i| serde_json::from_value(json!({"id": (500 + i).to_string(), "name": format!("Badge {i}"), "asset_url": format!("https://media.fluxer.app/emojis/{}.webp", 76 + i)})).expect("badge"))
                .collect();
            st.media_ui.invites.insert("hq".into(), inv("hq", HQ, "Fluxer HQ", json!({"features": ["PARTNERED"], "badge_ids": ["501", "502", "503", "504", "505"]})));
            st.media_ui.invites.insert("closed".into(), inv("closed", 9901, "Quiet Corner", json!({"features": ["INVITES_DISABLED", "DISCOVERABLE"]})));
            if let Some(b) = st.media_ui.guild_badges.get_mut(0) {
                b.description = Some("Awarded to communities that helped test Fluxer".into());
            }
            if scene.ends_with("-tip") {
                st.popups.tip = Some("guild-badge-501".into());
            }
            if scene.ends_with("-list") {
                st.media_ui.badge_list = Some(st.media_ui.guild_badges.iter().map(super::badges::from_guild).collect());
            }
        }
        "system" => {
            let call = |end: Option<u64>, who: &[u64]| json!({"type": 3, "call": {"participants": who.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
                "ended_timestamp": end.map(iso)}});
            msgs.push(message(t(1), 0, 1001, "", json!({"type": 6, "message_reference": {"channel_id": GENERAL.to_string(), "message_id": "1"}})));
            msgs.push(message(t(2), 0, 1002, "", json!({"type": 1, "mentions": [user(1003, "grace", "Grace Hopper")]})));
            msgs.push(message(t(3), 0, 1004, "", json!({"type": 2, "mentions": [user(1004, "alan", "Alan Turing")]})));
            msgs.push(message(t(4), 0, 1003, "release-crew", json!({"type": 4})));
            msgs.push(message(t(5), 0, 1003, "", json!({"type": 5})));
            msgs.push(message(t(6), 0, 1001, "Fluxer News #announcements", json!({"type": 12})));
            msgs.push(message(t(7), 0, 1006, "", call(Some(t(7) + 754_000), &[1006])));
            msgs.push(message(t(9), 0, ME, "", call(Some(t(9) + 30_000), &[ME])));
            msgs.push(message(t(10), 0, 1002, "", call(None, &[1002])));
        }
        "markup" => {
            msgs.push(message(t(30), 0, 1004, "Skin tones: 👍🏽 👋🏿 🧑🏻‍💻", json!({})));
            msgs.push(message(t(1), 0, 1001, "> [!NOTE]\n> Useful information.\n\n> [!TIP]\n> Helpful advice.\n\n> [!IMPORTANT]\n> Key information.\n\n> [!WARNING]\n> Needs attention.\n\n> [!CAUTION]\n> Negative outcomes.", json!({})));
            msgs.push(message(t(2), 0, 1002, "| Name | Role | Lines |\n|:-----|:----:|------:|\n| Ada | **Docs** | 12 |\n| Grace | Compiler | 3400 |\n| Alan | `tests` | 7 |", json!({})));
        }
        "link" => st.media_ui.link_warning = Some(("https://example.com/some/long/path?q=1".into(), false)),
        "nsfw" | "nsfw-check" | "nsfw-geo" => {
            if let Some(c) = store.channels.get_mut(&Snowflake(GENERAL)) {
                c.nsfw_override = Some(true);
            }
            if let Some(me) = store.me.as_mut() {
                me.nsfw_allowed = scene != "nsfw-check";
            }
            if scene == "nsfw-geo" {
                st.media_ui.age_geos = vec![("GB".into(), None)];
                st.expr.fetched.insert(super::attachments::GEO.into(), super::expr::Fetched::Done(json!({"countryCode": "GB"})));
            }
        }
        // A long history of mixed rows: scrolling up mounts rows taller than the list's estimate.
        "long" => {
            for i in 0..300u32 {
                let author = [1001, 1002, 1003, 1004][(i % 4) as usize];
                let extra = if i % 7 == 0 { json!({"attachments": [att(9400 + u64::from(i), "shot.png", "image/png", 1200, 800, json!({}))]}) } else { json!({}) };
                let text = if i % 3 == 0 { format!("message {i}, long enough to wrap onto a second line of the chat column at this width, and then some more words") } else { format!("message {i}") };
                msgs.push(message(at(0, 12 + i / 60, i % 60), 0, author, &text, extra));
            }
        }
        "blur" => {
            store.settings.sensitive_content_guild_filter = 1;
            msgs.push(message(t(1), 0, 1006, "careful", json!({"attachments": [att(9301, "x.png", "image/png", 1200, 800, json!({"flags": 16}))]})));
        }
        _ => {}
    }
    for m in msgs {
        match DispatchEvent::parse("MESSAGE_CREATE", &m) {
            Ok(ev) => store.apply(&ev, st.now_ms),
            Err(e) => panic!("media scene: {e}"),
        }
    }
    if scene == "long"
        && let Some(w) = store.messages.get_mut(&Snowflake(GENERAL))
    {
        w.has_more_before = true;
    }
    if scene == "viewer" || scene == "viewer-rotate" {
        let ms = store.messages(Snowflake(GENERAL));
        let m = ms.last().expect("message");
        let items = m.attachments.iter().map(super::attachments::item).collect();
        super::lightbox::open(st, items, 0, Some((m.channel_id, m.id)));
        if scene == "viewer-rotate"
            && let Some(v) = st.media_ui.viewer.as_mut()
        {
            v.rotation = 90;
        }
    }
}

/// `--threads[=<scene>]`: threads under #general (one from Alan's screenshot, joined; one
/// standalone, joined and unread; one to join) and posts in the #help forum. Scenes: `panel`,
/// `create`, `full`, `browser`, `browser-search`, `panel-wide`, `closed`, `menu`, `settings`,
/// `forum`, `forum-grid`, `forum-compose`, `forum-post`, `forum-settings`, `forum-search`.
fn thread_scene(st: &mut ShellState, store: &mut Store, scene: &str) {
    let now = st.now_ms;
    fn put(store: &mut Store, now: u64, t: &str, d: Value) {
        match DispatchEvent::parse(t, &d) {
            Ok(ev) => {
                if let DispatchEvent::MessageCreate(m) = &ev {
                    store.messages.entry(m.channel_id).or_default();
                }
                store.apply(&ev, now);
            }
            Err(e) => panic!("fixture {t}: {e}"),
        }
    }
    let t = |h, m| at(0, h, m);
    let shot: u64 = store.messages(Snowflake(GENERAL))[6].id.0;
    let thread = |id: u64, parent: u64, kind: u16, name: &str, owner: u64, count: u32, joined: bool, extra: Value| {
        let mut v = json!({"id": id.to_string(), "type": kind, "guild_id": HQ.to_string(), "parent_id": parent.to_string(), "name": name,
            "owner_id": owner.to_string(), "message_count": count, "member_count": 3,
            "thread_metadata": {"archived": false, "auto_archive_duration": 4320, "archive_timestamp": iso(t(9, 0)), "locked": false, "create_timestamp": iso(t(9, 0))}});
        if joined {
            v["member"] = json!({"join_timestamp": iso(t(9, 0)), "flags": 1});
        }
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        v
    };
    let in_ch = |mut m: Value, ch: u64| {
        m["channel_id"] = json!(ch.to_string());
        m
    };
    // From Alan's screenshot (thread ID = message ID), joined and read.
    put(store, now, "THREAD_CREATE", thread(shot, GENERAL, 11, "renderer feedback", 1004, 3, true, json!({})));
    let starter = in_ch(message(t(10, 2), 0, 1004, "", json!({"type": 21, "message_reference": {"channel_id": GENERAL.to_string(), "message_id": shot.to_string()}})), shot);
    put(store, now, "MESSAGE_CREATE", starter);
    let replies = [
        message(t(10, 3), 0, 1003, "The text looks crisper than before. Is that the new hinting?", json!({})),
        message(t(10, 9), 0, 1004, "Yes: grayscale AA with the gamma fix, no subpixel.", json!({})),
        message(t(10, 12), 0, 1002, "Ship it 🚀", json!({})),
    ];
    let mut last = String::new();
    for r in replies {
        last = r["id"].as_str().unwrap_or_default().to_string();
        put(store, now, "MESSAGE_CREATE", in_ch(r, shot));
    }
    put(store, now, "MESSAGE_ACK", json!({"channel_id": shot.to_string(), "message_id": last}));
    // Standalone, joined, unread; and one the user hasn't joined.
    let release = Snowflake::from_timestamp_ms(t(11, 0)).0 + 7;
    put(store, now, "THREAD_CREATE", thread(release, GENERAL, 11, "release-0.4", 1001, 2, true, json!({})));
    put(store, now, "MESSAGE_CREATE", in_ch(message(t(11, 8), 0, 1001, "Tagging tomorrow unless someone objects.", json!({})), release));
    let style = Snowflake::from_timestamp_ms(t(8, 0)).0 + 9;
    put(store, now, "THREAD_CREATE", thread(style, GENERAL, 11, "style guide nits", 1006, 14, false, json!({})));
    put(store, now, "MESSAGE_CREATE", message(t(11, 7), 0, 1001, "release-0.4", json!({"type": 18, "message_reference": {"channel_id": release.to_string(), "guild_id": HQ.to_string()}})));
    // A closed one for the browser.
    let closed = Snowflake::from_timestamp_ms(at(-5, 9, 0)).0 + 3;
    let mut c = thread(closed, GENERAL, 11, "old CI flakes", 1002, 41, false, json!({}));
    c["thread_metadata"]["archived"] = json!(true);
    store.add_threads(&[serde_json::from_value(c).expect("thread")], &[]);
    st.threads.archived.insert((Snowflake(GENERAL), super::threads::Tab::Archived), super::threads::Archived { ids: vec![Snowflake(closed)], ..Default::default() });

    // #help: guidelines, tags, a default reaction and three posts.
    if let Some(help) = store.channels.get_mut(&Snowflake(2204)) {
        help.topic = Some("Search before posting, and pick a **tag** so helpers find you.".into());
        help.available_tags = serde_json::from_value(json!([
            {"id": "2290", "name": "bug", "moderated": false, "emoji_name": "🐛"},
            {"id": "2291", "name": "question", "moderated": false, "emoji_name": "❓"},
            {"id": "2292", "name": "solved", "moderated": true, "emoji_name": "✅"},
            {"id": "2293", "name": "linux", "moderated": false},
        ]))
        .expect("tags");
        help.default_reaction_emoji = Some(reflux_model::DefaultReaction { emoji_id: None, emoji_name: Some("👍".into()) });
    }
    let posts = [
        (Snowflake::from_timestamp_ms(t(7, 0)).0 + 1, "Read this first: how to get help", 1001, 1, vec!["2291"], 2, true, "Include your OS, the build number and what you tried."),
        (Snowflake::from_timestamp_ms(t(9, 30)).0 + 2, "Window goes black after resume on Wayland", 1003, 6, vec!["2290", "2293"], 0, false, "After suspend the whole window is black until I resize it. Screenshot attached."),
        (Snowflake::from_timestamp_ms(at(-1, 16, 0)).0 + 3, "Can I run two accounts side by side?", 1006, 3, vec!["2291", "2292"], 0, false, "Asking for a friend who runs a community and a personal account."),
    ];
    let wayland = Snowflake(posts[1].0);
    let (first_post, two) = (Snowflake(posts[0].0), Snowflake(posts[2].0));
    for (id, name, owner, count, tags, flags, joined, body) in posts {
        let mut p = thread(id, 2204, 11, name, owner, count, joined, json!({"applied_tags": tags, "flags": flags}));
        p["last_message_id"] = json!(Snowflake::from_timestamp_ms(t(11, 30)).0.to_string());
        put(store, now, "THREAD_CREATE", p);
        let mut first = message(Snowflake(id).timestamp_ms(), 0, owner, body, json!({"reactions": [{"emoji": {"name": "👍"}, "count": count, "me": joined}]}));
        first["id"] = json!(id.to_string());
        first["channel_id"] = json!(id.to_string());
        if owner == 1003 {
            first["attachments"] = json!([{"id": "9101", "filename": "black.png", "content_type": "image/png", "size": 1000, "width": 1280, "height": 720,
                "proxy_url": "https://media.fluxer.app/attachments/2204/9101/black.png"}]);
        }
        let m: reflux_model::Message = serde_json::from_value(first).expect("first message");
        st.threads.forum.first.insert(Snowflake(id), Some(m));
    }
    st.threads.archived.insert((Snowflake(2204), super::threads::Tab::Archived), Default::default());
    let general = Snowflake(GENERAL);
    match scene {
        "panel" => super::threads::open(st, Some(general), Snowflake(shot)),
        "create" => {
            let ada = store.messages(general)[8].id;
            super::threads::start_create(st, general, Some(ada));
        }
        "full" => {
            let members = [1006u64, 1001, 1003, 1005].map(|u| serde_json::from_value(json!({"id": style.to_string(), "user_id": u.to_string(), "join_timestamp": iso(t(8, 0)), "flags": 1})).expect("member"));
            store.thread_members.insert(Snowflake(style), members.into());
            super::threads::open_full(st, Snowflake(style));
        }
        "browser" => st.popups.toggle(super::threads::BROWSER),
        "browser-search" => {
            st.popups.toggle(super::threads::BROWSER);
            st.threads.query = rune::TextEdit::new("release");
            st.threads.archived.insert((general, super::threads::Tab::Search), super::threads::Archived { ids: vec![Snowflake(release)], key: "name=release".into(), ..Default::default() });
        }
        "panel-wide" => {
            st.prefs.thread_panel_width = Some(640.0);
            super::threads::open(st, Some(general), Snowflake(shot));
        }
        "closed" => {
            st.popups.toggle(super::threads::BROWSER);
            st.threads.tab = super::threads::Tab::Archived;
        }
        "menu" => super::threads::open_menu(st, Snowflake(release), [120.0, 420.0]),
        "settings" => {
            super::threads::open_menu(st, Snowflake(release), [120.0, 420.0]);
            st.popups.close();
            st.threads.settings = Some(super::threads::Settings { thread: Snowflake(release), name: rune::TextEdit::new("release-0.4"), auto_archive: 4320, invitable: true, slowmode: 0 });
        }
        "forum" | "forum-grid" | "forum-compose" | "forum-post" | "forum-settings" | "forum-tag" | "forum-search" => {
            st.select_channel(Snowflake(2204));
            // The forum was last read before the Wayland post (it gets "New"), and the unjoined
            // two-accounts post has 3 messages past its ack (FORUM_UNREADS).
            st.threads.forum.snapshot = Some((Snowflake(2204), first_post));
            put(store, now, "MESSAGE_ACK", json!({"channel_id": two.to_string(), "message_id": two.to_string()}));
            st.threads.forum.post_unreads.insert(two, (3, two));
            if scene == "forum-search" {
                st.threads.forum.query = rune::TextEdit::new("wayland");
                st.threads.archived.insert((Snowflake(2204), super::threads::Tab::Search), super::threads::Archived { ids: vec![wayland], key: "name=wayland".into(), ..Default::default() });
            }
            if scene == "forum-grid" {
                st.threads.forum.layout.insert(Snowflake(2204), 2);
            }
            if scene == "forum-compose" {
                st.threads.forum.draft = Some(super::forum::Draft {
                    forum: Snowflake(2204),
                    title: rune::TextEdit::new("Crash when dropping a folder into the composer"),
                    body: rune::TextEdit::new("Dragging a folder from Files makes the app hang for a few seconds."),
                    tags: vec![Snowflake(2290)],
                    error: None,
                    preview: false,
                });
            }
            if scene == "forum-settings" || scene == "forum-tag" {
                st.open_dialog(crate::guild::Dialog::Channel { channel: Snowflake(2204), tab: "overview", target: None });
            }
            if scene == "forum-tag" {
                st.form.data.insert("tag-edit".into(), serde_json::json!("2290"));
                st.form.text.insert("tag-name".into(), rune::TextEdit::new("bug"));
                st.form.data.insert("tag-emoji".into(), serde_json::json!({ "id": null, "name": "🐛" }));
            }
            if scene == "forum-post" {
                put(store, now, "MESSAGE_CREATE", in_ch(message(t(11, 30), 0, 1002, "Does it happen with `--disable-gpu` too?", json!({})), wayland.0));
                if let (Some(m), Some(w)) = (st.threads.forum.first.get(&wayland).cloned().flatten(), store.messages.get_mut(&wayland)) {
                    w.messages.insert(0, m);
                }
                super::threads::open(st, Some(Snowflake(2204)), wayland);
            }
        }
        _ => {}
    }
    st.intents.clear();
}

pub fn setup(s: &mut State, waker: Waker) {
    if let Some(d) = s.shell_demo.as_mut() {
        d.shell.default_avatars = load_default_avatars(waker.clone());
        d.shell.media.setup(waker, Some(fixture));
        d.shell.media.set_base("https://media.fluxer.app");
    }
}

fn acc(s: &mut State) -> &mut ShellState {
    &mut s.shell_demo.get_or_insert_with(Demo::new).shell
}

pub fn view(d: &Demo) -> El {
    let look = &d.shell.prefs.look;
    crate::look::set_reduced_motion(look.reduced(false));
    let p = theme::custom::effective(look.theme(&d.store.settings.theme), std::env::var("REFLUX_SYSTEM_LIGHT").is_err(), look.saturation, &look.theme_css());
    super::view(&d.store, &d.shell, &p, acc)
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
        assert_eq!(s.messages(Snowflake(GENERAL)).len(), 15);
        assert!(s.has_unread(Snowflake(GENERAL)));
        assert!(s.has_unread(Snowflake(2202)));
        assert!(s.is_muted(&s.channels[&Snowflake(2203)]));
        let list = &s.member_lists[&(Snowflake(HQ), GENERAL.to_string())];
        let names: Vec<String> = list.groups.iter().map(|g| super::super::members::group_label(s, Snowflake(HQ), g)).collect();
        assert_eq!(names, ["Staff", "Moderators", "Online", "Offline"]);
        assert_eq!(list.rows.len(), 12);
        assert_eq!(s.relationships.len(), 7);
        assert_eq!(s.typing_users(Snowflake(GENERAL), d.shell.now_ms), [Snowflake(1002)]);
    }
}
