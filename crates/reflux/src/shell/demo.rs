//! `--shell-demo`: the shell over a hand-written fixture store, for snapshot checks:
//! `cargo run -p reflux -- --shell-demo --snap /tmp/opencode/snaps/shell.png --size 1280x800 --scale 1`
//! (add `--shell-home` for the DM list, `--friends[=all|pending|add]` for the friends page,
//! `--profile` for a profile popout, `--ctx=user|guild|channel` for context menus,
//! `--guild-ui=<scene>` for community management).

use chrono::{Local, TimeZone};
use reflux_model::{DispatchEvent, Snowflake};
use reflux_store::Store;
use rune::{El, Image, ImageCache, Waker};
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
                "voice_states": [{"user_id": "1004", "channel_id": "2301", "self_stream": true},
                                 {"user_id": "1003", "channel_id": "2301", "self_mute": true, "self_deaf": true}],
            }),
        );
        hq["properties"]["icon"] = json!("demo");
        hq["properties"]["banner"] = json!("demo");
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
                "user": {"id": ME.to_string(), "username": "square", "global_name": "Square", "email": "square@example.com"},
                "users": [user(1001, "ada", "Ada Lovelace"), user(1002, "linus", "Linus"), user(1003, "grace", "Grace Hopper"),
                          user(1004, "alan", "Alan Turing"), user(1005, "margaret", "Margaret Hamilton"),
                          user(1006, "ken", "Ken Thompson"), user(1007, "fluxbot", "Fluxbot"),
                          user(1008, "hedy", "Hedy Lamarr"), user(1009, "spammer", "Spam Bot")],
                "relationships": [{"id": "1001", "type": 1}, {"id": "1003", "type": 1}, {"id": "1004", "type": 1},
                                  {"id": "1006", "type": 1}, {"id": "1005", "type": 3}, {"id": "1008", "type": 4},
                                  {"id": "1009", "type": 2}],
                "notes": {"1001": "Wrote the first program"},
                "guilds": [hq, small(3000, "Rust Lang", 3001, new), small(4000, "Design Crit", 4001, new),
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

        let mut shell = ShellState { guild: Some(Snowflake(HQ)), channel: Some(Snowflake(GENERAL)), now_ms: now, premium: true, ..ShellState::default() };
        // `--settings[=tab]` opens user settings; `--theme=light|coal` sets the account theme.
        for a in rune::extra_args() {
            if let Some(t) = a.strip_prefix("--theme=") {
                store.settings.theme = t.to_owned();
            } else if a == "--settings" || a.starts_with("--settings=") {
                shell.settings = Some(crate::settings::SettingsUi::new(a.strip_prefix("--settings=")));
            }
        }
        if let Some(sc) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--msg-scene=")) {
            stage(&mut shell, &mut store, sc);
        }
        for a in rune::extra_args() {
            if a == "--shell-home" {
                (shell.guild, shell.channel) = (None, Some(Snowflake(8001)));
            } else if let Some(tab) = a.strip_prefix("--friends").map(|t| t.trim_start_matches('=')) {
                (shell.guild, shell.channel) = (None, None);
                shell.friends_tab = match tab {
                    "all" => super::FriendsTab::All,
                    "pending" => super::FriendsTab::Pending,
                    "add" => super::FriendsTab::Add,
                    _ => super::FriendsTab::Online,
                };
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
        if let Some(sc) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--guild-ui=")) {
            guild_scene(&mut shell, sc);
        }
        if let Some(sc) = rune::extra_args().iter().find_map(|a| a.strip_prefix("--voice")) {
            voice_scene(&mut shell, &mut store, sc.trim_start_matches('='));
        }
        shell.voice.devices = Some((vec!["Built-in Microphone".into(), "USB Headset".into()], vec!["Speakers".into(), "USB Headset".into()]));
        Demo { store, shell }
    }
}

/// `--guild-ui=<scene>`: community management mid-use. `menu`, `add|create|join`, `invite[-edit]`,
/// `channel|category`, `notif`, `privacy`, `gs-<tab>` (community settings), `role`, `delete`,
/// `cs-overview|cs-permissions|cs-overwrite` (channel settings of #dev).
fn guild_scene(st: &mut ShellState, scene: &str) {
    use crate::guild::{AddView, Confirm, Dialog};
    let g = Snowflake(HQ);
    let data = |st: &mut ShellState, k: &str, v: Value| {
        st.form.data.insert(k.into(), v);
    };
    match scene {
        "menu" => st.popups.toggle(crate::guild::HEADER_MENU),
        "add" => st.open_dialog(Dialog::Add(AddView::Landing)),
        "create" => st.open_dialog(Dialog::Add(AddView::Create)),
        "join" => st.open_dialog(Dialog::Add(AddView::Join)),
        "invite" | "invite-edit" => {
            st.open_dialog(Dialog::Invite(Snowflake(GENERAL), scene == "invite-edit"));
            st.invite_base = "https://fluxer.gg".into();
            data(st, "invite", json!({"code": "AbCd12", "max_age": 604800, "max_uses": 0}));
        }
        "channel" | "category" => st.open_dialog(Dialog::CreateChannel { guild: g, parent: Some(Snowflake(2200)), category: scene == "category" }),
        "notif" => st.open_dialog(Dialog::Notifications(g)),
        "privacy" => st.open_dialog(Dialog::Privacy(g)),
        "role" => st.open_dialog(Dialog::Guild { guild: g, tab: "roles", role: Some(Snowflake(2011)) }),
        "delete" => {
            st.open_dialog(Dialog::Guild { guild: g, tab: "overview", role: None });
            st.form.confirm = Some(Confirm::DeleteGuild(g));
        }
        "cs-overview" | "cs-permissions" | "cs-overwrite" => {
            let tab = if scene == "cs-overview" { "overview" } else { "permissions" };
            st.open_dialog(Dialog::Channel { channel: Snowflake(2202), tab, target: (scene == "cs-overwrite").then_some(Snowflake(2011)) });
        }
        s => {
            let tab = crate::guild::tab_ids().find(|t| s.strip_prefix("gs-") == Some(*t)).unwrap_or("overview");
            st.open_dialog(Dialog::Guild { guild: g, tab, role: None });
            let m = |u: u64, r: &[u64]| json!({"user": {"id": u.to_string(), "username": format!("user{u}"), "global_name": null}, "roles": r.iter().map(|x| x.to_string()).collect::<Vec<_>>(), "joined_at": "2025-01-01T00:00:00Z"});
            data(st, "members", json!([m(ME, &[2010]), m(1001, &[2010, 2012]), m(1002, &[2011]), m(1003, &[2012]), m(1004, &[])]));
            data(st, "invites", json!([{"code": "AbCd12", "uses": 3, "max_uses": 0, "max_age": 0, "inviter": {"id": "1001", "username": "ada", "global_name": "Ada Lovelace"}, "channel": {"id": GENERAL.to_string(), "name": "general", "type": 0}},
                                      {"code": "Zz99", "uses": 1, "max_uses": 10, "max_age": 86400, "expires_at": "2026-12-01T00:00:00Z", "inviter": {"id": "1000", "username": "square"}, "channel": {"id": "2001", "name": "welcome", "type": 0}}]));
            data(st, "bans", json!([{"user": {"id": "1009", "username": "spammer", "global_name": "Spam Bot"}, "reason": "Spam links"}]));
            let e = |n: u64, user: u64, t: u32, target: &str| json!({"id": id_at(at(0, 9, 0), n), "user_id": user.to_string(), "action_type": t, "target_id": target});
            data(st, "audit", json!({"audit_log_entries": [e(1, 1001, 22, "1009"), e(2, ME, 31, "2011"), e(3, 1002, 10, "2204")],
                                     "users": [{"id": "1001", "username": "ada", "global_name": "Ada Lovelace"}, {"id": "1009", "username": "spammer"}]}));
        }
    }
}

/// `--voice[=connecting|failed|watch]`: connected to Lounge (Alan speaking, sharing his screen).
fn voice_scene(st: &mut ShellState, store: &mut Store, scene: &str) {
    let lounge = (Some(Snowflake(HQ)), Snowflake(2301));
    if scene == "failed" {
        st.voice.failed = Some(lounge);
        return;
    }
    let me = json!({"guild_id": HQ.to_string(), "user_id": ME.to_string(), "channel_id": "2301", "connection_id": "demo", "self_mute": true});
    if let Ok(ev) = DispatchEvent::parse("VOICE_STATE_UPDATE", &me) {
        store.apply(&ev, 0);
    }
    st.voice.channel = Some(lounge);
    st.voice.self_mute = true;
    st.voice.connected = scene != "connecting";
    if st.voice.connected {
        (st.voice.ping, st.voice.pings) = (Some(42), vec![40, 41, 42]);
        st.voice.speaking.insert(Snowflake(1004));
    }
    if scene == "watch" {
        st.voice.watching = Some(Snowflake(1004));
        // A gradient stands in for the decoded frames.
        let (w, h) = (320u32, 180u32);
        let rgba = (0..w * h).flat_map(|i| [(i % w * 255 / w) as u8, (i / w * 255 / h) as u8, 160, 255]).collect();
        st.voice.stream.set(w, h, rgba);
    }
}

/// `--msg-scene=hover|menu|more|emoji|reply|edit|delete`: message actions mid-use;
/// `uploads|pins|search|search-options|switcher|autocomplete|jump|present|slowmode|counter`: the
/// composer and navigation tools.
fn stage(st: &mut ShellState, store: &mut Store, scene: &str) {
    let ids: Vec<Snowflake> = store.messages(Snowflake(GENERAL)).iter().map(|m| m.id).collect();
    let (shot, mine, ada) = (ids[6], ids[7], ids[8]);
    match scene {
        "hover" => st.hover_msg = Some(ada),
        "menu" => {
            st.popups.open_at(&format!("msg-menu-{ada}"), [620.0, 330.0]);
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
            st.popups.toggle(super::pins::POPOUT);
            let ms = store.messages(Snowflake(GENERAL));
            let items = [&ms[8], &ms[6]].map(|m| (m.clone(), m.timestamp.clone())).to_vec();
            st.pins.insert(Snowflake(GENERAL), super::pins::Pins { items, ..Default::default() });
        }
        "search" => {
            st.search.query = rune::TextEdit::new("from:ada ship");
            let messages = [8, 3].map(|i| store.messages(Snowflake(GENERAL))[i].clone()).to_vec();
            st.search.results = Some(super::search::Results::Done { messages, total: 2 });
            st.search.ran = "from:ada ship".into();
        }
        "search-options" => st.search.query.focused = true,
        "switcher" => {
            st.toggle_switcher();
            if let Some(sw) = &mut st.switcher {
                sw.query = rune::TextEdit::new("#");
                sw.query.focused = true;
            }
        }
        "autocomplete" => {
            st.composer = rune::TextEdit::new("hey @a");
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
/// `invite`, `viewer` (the lightbox), `link` (external link warning), `nsfw` (the channel gate),
/// `blur` (flagged media under the guild filter).
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
    // Fluxer's defaults: spoilers on click.
    store.settings.render_spoilers = 1;
    match scene {
        "mosaic" | "viewer" => {
            msgs.push(message(t(1), 0, 1001, "Three from the offsite:", json!({"attachments": [
                att(9101, "a.png", "image/png", 1200, 800, json!({})), att(9102, "b.png", "image/png", 800, 800, json!({})),
                att(9103, "c.png", "image/png", 800, 1200, json!({"description": "The team on the stairs"}))]})));
            msgs.push(message(t(2), 0, 1003, "and five more", json!({"attachments": [
                att(9111, "d.png", "image/png", 1200, 800, json!({})), att(9112, "SPOILER_e.png", "image/png", 1200, 800, json!({})),
                att(9113, "f.gif", "image/gif", 400, 400, json!({})), att(9114, "g.png", "image/png", 400, 400, json!({})),
                att(9115, "h.png", "image/png", 400, 400, json!({}))]})));
        }
        "files" => {
            msgs.push(message(t(1), 0, 1004, "", json!({"attachments": [att(9201, "demo.mp4", "video/mp4", 1280, 720, json!({}))]})));
            msgs.push(message(t(2), 0, 1002, "", json!({"attachments": [
                att(9202, "theme.ogg", "audio/ogg", 0, 0, json!({"width": null, "height": null, "size": 1843200, "duration": 94.0})),
                att(9203, "main.rs", "text/x-rust", 0, 0, json!({"width": null, "height": null, "size": 412})),
                att(9204, "release-0.4.tar.gz", "application/gzip", 0, 0, json!({"width": null, "height": null, "size": 15728640}))]})));
            st.media.seed_text("https://cdn.fluxer.app/attachments/2201/9203/main.rs",
                "use std::io;\n\n/// Greets the world.\nfn main() -> io::Result<()> {\n    let name = \"Fluxer\";\n    println!(\"hello, {name}\");\n    Ok(())\n}\n");
        }
        "embeds" => {
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
        "invite" => {
            msgs.push(message(t(1), 0, 1001, "come hang out https://fluxer.gg/rustlang and https://fluxer.gg/gone", json!({})));
            msgs.push(message(t(2), 0, 1003, "also https://fluxer.gg/hq", json!({})));
            let inv = |code: &str, id: u64, name: &str| -> super::invite_embed::Preview {
                let v = json!({"code": code, "type": 0, "guild": {"id": id.to_string(), "name": name, "icon": "demo"},
                               "channel": {"id": "1", "name": "general", "type": 0}, "member_count": 12873, "presence_count": 2041});
                super::invite_embed::Preview::Ready(Box::new(serde_json::from_value(v).expect("invite")))
            };
            st.media_ui.invites.insert("rustlang".into(), inv("rustlang", 9900, "Rust Programming"));
            st.media_ui.invites.insert("gone".into(), super::invite_embed::Preview::Unknown);
            st.media_ui.invites.insert("hq".into(), inv("hq", HQ, "Fluxer HQ"));
        }
        "link" => st.media_ui.link_warning = Some(("https://example.com/some/long/path?q=1".into(), false)),
        "nsfw" => {
            if let Some(c) = store.channels.get_mut(&Snowflake(GENERAL)) {
                c.nsfw_override = Some(true);
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
    if scene == "viewer" {
        let ms = store.messages(Snowflake(GENERAL));
        let m = ms.last().expect("message");
        let items = m.attachments.iter().map(super::attachments::item).collect();
        super::lightbox::open(st, items, 0, Some((m.channel_id, m.id)));
    }
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
    super::view(&d.store, &d.shell, theme::palette(Kind::from_setting(&d.store.settings.theme)), acc)
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
