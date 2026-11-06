//! `--social=<scene>` for `--shell-demo`: this module's dialogs, popouts and pages mid-use.
//! Scenes: `kick ban timeout untimeout nickname transfer ban-details report create-dm add-group
//! edit-group friend-nick group-nick forward publish follow topic duplicate invite webhook`
//! (dialogs), `inbox inbox-bookmarks inbox-unread bell mute user-menu group-menu msg-menu`
//! (popouts and menus), `gs-moderation gs-vanity_url gs-webhooks gs-explore cs-invites
//! cs-webhooks` (settings tabs), `discovery preview welcome dm-welcome group-welcome notes
//! blocked mass` (pages and channel pieces).

use reflux_model::{DispatchEvent, Snowflake};
use reflux_store::Store;
use serde_json::{Value, json};

use super::Modal;
use crate::shell::{ShellState, Target};

const ME: u64 = 1000;
const HQ: u64 = 2000;
const GENERAL: u64 = 2201;

fn s(n: u64) -> Snowflake {
    Snowflake(n)
}

fn msgs(store: &Store) -> Vec<Value> {
    store.messages(s(GENERAL)).iter().map(|m| serde_json::to_value(m).unwrap_or_default()).collect()
}

pub fn scene(st: &mut ShellState, store: &mut Store, scene: &str) {
    let g = s(HQ);
    let ms = msgs(store);
    let mid = |i: usize| ms[i]["id"].as_str().and_then(|x| x.parse().ok()).unwrap_or_default();
    let data = |st: &mut ShellState, k: &str, v: Value| {
        st.form.data.insert(k.into(), v);
    };
    st.invite_base = "https://fluxer.gg".into();
    st.social.api_base = "https://api.fluxer.app/v1".into();
    match scene {
        "kick" => st.open_social(Modal::Kick { guild: g, user: s(1004) }),
        "ban" => st.open_social(Modal::Ban { guild: g, user: s(1004) }),
        "timeout" => st.open_social(Modal::Timeout { guild: g, user: s(1004) }),
        "untimeout" => st.open_social(Modal::RemoveTimeout { guild: g, user: s(1004) }),
        "nickname" => st.open_social(Modal::Nickname { guild: g, user: s(1004) }),
        "transfer" => st.open_social(Modal::TransferOwnership { guild: g, user: s(1001) }),
        "ban-details" => {
            st.open_social(Modal::BanDetails { guild: g, user: s(1009) });
            data(st, "ban", json!({"user": {"id": "1009", "username": "spammer", "global_name": "Spam Bot"}, "reason": "Spam links in every channel", "banned_at": "2026-11-02T10:00:00Z", "moderator_id": "1001"}));
        }
        "report" => {
            st.open_social(Modal::Report { target: "user", guild: Some(g), channel: None, id: s(1009) });
            data(st, "flow", json!({"revision_hash": "x", "start_screen_id": "root", "locale": "en-US", "screens": [
                {"id": "root", "title": "What's going on with this profile?", "subtitle": "Pick the option that fits best.", "options": [
                    {"id": "spam", "label": "Spam or scam", "outcome": {"type": "submit", "reason": "spam"}},
                    {"id": "imp", "label": "Impersonation", "outcome": {"type": "screen", "screen_id": "x"}},
                    {"id": "hate", "label": "Hateful or abusive", "outcome": {"type": "screen", "screen_id": "x"}},
                    {"id": "other", "label": "Something else", "outcome": {"type": "end"}}]}]}));
        }
        "create-dm" => {
            st.open_social(Modal::CreateDm);
            data(st, "picked", json!(["1001", "1003"]));
        }
        "add-group" => {
            st.open_social(Modal::AddToGroup(s(8003)));
            data(st, "invite", json!({"code": "GrP42x"}));
        }
        "edit-group" => st.open_social(Modal::EditGroup(s(8003))),
        "friend-nick" => st.open_social(Modal::FriendNickname(s(1001))),
        "group-nick" => st.open_social(Modal::GroupNickname { channel: s(8003), user: s(1002) }),
        "forward" => {
            st.open_social(Modal::Forward { channel: s(GENERAL), message: mid(8) });
            data(st, "picked", json!(["8001", "2202"]));
        }
        "publish" => st.open_social(Modal::Publish { channel: s(GENERAL), message: mid(8) }),
        "follow" => {
            st.open_social(Modal::Follow(s(2101)));
            data(st, "guild", json!("2000"));
        }
        "topic" => st.open_social(Modal::Topic(s(GENERAL))),
        "duplicate" => st.open_social(Modal::Duplicate(s(2202))),
        "invite" => {
            st.open_social(Modal::InviteAccept("rust".into()));
            data(st, "invite", json!({"code": "rust", "guild": {"id": "3000", "name": "Rust Lang"}, "inviter": {"id": "1001", "username": "ada", "global_name": "Ada Lovelace"}, "member_count": 12840, "presence_count": 2310}));
        }
        "webhook" => {
            st.open_social(Modal::Webhook { guild: g, id: s(9100) });
            data(st, "webhook", json!({"id": "9100", "name": "CI", "channel_id": "2202", "token": "t"}));
        }
        "inbox" | "inbox-bookmarks" | "inbox-unread" => {
            st.social.inbox_open = true;
            st.social.inbox_tab = match scene {
                "inbox-unread" => 0,
                "inbox-bookmarks" => 1,
                _ => 2,
            };
            st.social.data.insert("mentions".into(), json!([ms[8].clone(), ms[7].clone()]));
            st.social.data.insert("bookmarks".into(), json!([{"id": ms[6]["id"], "channel_id": GENERAL.to_string(), "message_id": ms[6]["id"], "status": "available", "message": ms[6].clone()}]));
        }
        "bell" => st.popups.toggle(super::channel::BELL),
        "mute" => {
            st.open_menu(Target::Channel(s(GENERAL)), [180.0, 330.0]);
            st.popups.sub = Some("mute".into());
        }
        "user-menu" => st.open_user_menu(Target::User { user: s(1004), guild: Some(g), dm: None }, [900.0, 60.0], "member-1004".into(), crate::ui::Placement::LeftStart),
        "group-menu" => {
            (st.guild, st.channel) = (None, Some(s(8003)));
            st.open_user_menu(Target::User { user: s(1002), guild: None, dm: None }, [900.0, 60.0], "member-1002".into(), crate::ui::Placement::LeftStart);
        }
        "msg-menu" => {
            let id = mid(8);
            st.popups.open_at(&format!("msg-menu-{id}"), [620.0, 200.0]);
            st.active_msg = Some(id);
        }
        "gs-moderation" | "gs-vanity_url" | "gs-webhooks" | "gs-explore" => {
            let tab = crate::guild::tab_ids().chain(["explore"]).find(|t| scene.strip_prefix("gs-") == Some(*t)).unwrap_or("moderation");
            st.open_dialog(crate::guild::Dialog::Guild { guild: g, tab, role: None });
            if let Some(info) = store.guilds.get_mut(&g).and_then(|x| x.info.as_mut()) {
                info.features.push("VANITY_URL".into());
            }
            data(st, "vanity", json!({"code": "fluxer", "uses": 1284}));
            data(st, "webhooks", json!([{"id": "9100", "name": "CI", "channel_id": "2202", "token": "t", "user": {"id": "1001", "username": "ada", "global_name": "Ada Lovelace"}},
                                        {"id": "9101", "name": "Release bot", "channel_id": "2101", "user": {"id": "1000", "username": "square"}}]));
            data(st, "discovery", json!({"application": {"status": "pending", "description": "The home of Fluxer development: releases, RFCs and help.", "category_type": 4}, "eligible": true, "min_member_count": 5}));
        }
        "cs-invites" | "cs-webhooks" => {
            let tab = if scene == "cs-invites" { "invites" } else { "webhooks" };
            st.open_dialog(crate::guild::Dialog::Channel { channel: s(GENERAL), tab, target: None });
            data(st, "invites", json!([{"code": "AbCd12", "uses": 3, "max_uses": 0, "max_age": 0, "inviter": {"id": "1001", "username": "ada", "global_name": "Ada Lovelace"}, "channel": {"id": GENERAL.to_string(), "name": "general", "type": 0}}]));
            data(st, "webhooks", json!([{"id": "9100", "name": "CI", "channel_id": "2201", "token": "t", "user": {"id": "1001", "username": "ada", "global_name": "Ada Lovelace"}}]));
        }
        "discovery" => {
            (st.guild, st.channel) = (None, None);
            st.social.discovery = true;
            let card = |id: u64, name: &str, d: &str, cat: u8, m: u64, o: u64| json!({"id": id.to_string(), "name": name, "description": d, "category_type": cat, "member_count": m, "online_count": o, "features": [], "verification_level": 1});
            st.social.data.insert("discovery".into(), json!({"total": 4, "guilds": [
                card(3000, "Rust Lang", "Official community for the Rust programming language. Help, news and showcase.", 4, 12840, 2310),
                card(9001, "Pixel Art Club", "Share your sprites, get feedback and join weekly challenges.", 2, 3402, 412),
                card(9002, "Lo-fi Lounge", "", 1, 980, 77),
                card(9003, "Speedrunners", "Routes, splits and races for every game you can think of. Beginners welcome!", 0, 25110, 4021)]}));
        }
        "preview" => {
            if let Some(gs) = store.guilds.get_mut(&s(3000)) {
                gs.visiting = true;
            }
            (st.guild, st.channel) = (Some(s(3000)), Some(s(3001)));
        }
        "welcome" => {
            st.channel = Some(s(2001));
            store.messages.entry(s(2001)).or_default().has_more_before = false;
        }
        "dm-welcome" | "group-welcome" | "notes" => {
            let c = match scene {
                "dm-welcome" => s(8002),
                "group-welcome" => s(8003),
                _ => s(ME),
            };
            (st.guild, st.channel) = (None, Some(c));
            store.messages.entry(c).or_default().has_more_before = false;
        }
        "blocked" => {
            let base = ms[10]["id"].as_str().and_then(|x| x.parse::<u64>().ok()).unwrap_or_default();
            for n in 1..=3 {
                let m = json!({"id": (base + n).to_string(), "channel_id": GENERAL.to_string(), "author": {"id": "1009", "username": "spammer"}, "content": "buy now", "timestamp": ms[10]["timestamp"]});
                if let Ok(ev) = DispatchEvent::parse("MESSAGE_CREATE", &m) {
                    store.apply(&ev, 0);
                }
            }
            st.scroll_to = Some(format!("msg-{}", base + 1));
        }
        "mass" => {
            st.composer = rune::TextEdit::new("@everyone release is out!");
            st.social.mass = Some((s(GENERAL), "@everyone", 120));
        }
        _ => {}
    }
}
