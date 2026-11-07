//! Moderation modals (`moderation/components/modals/*`, `TransferOwnershipModal`,
//! `ChangeNicknameModal`, `report_flow/ReportFlowModal`) and the member and voice moderation
//! items of the user context menu (`GuildMemberContextMenu`, `VoiceParticipantMenuItems`).

use reflux_model::{ChannelType, Permissions as P, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, col, row, text};
use serde_json::{Value, json};

use super::{Done, Modal, Req, body, close, desc, error, field, footer, iso, label, user_name};
use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, MenuEntry, MenuItem, Modal as Dialog, RadioGroup, Select, popup_access};
use crate::i18n::t;

/// Queues a request carrying an audit-log reason (blank = none).
fn send(st: &mut ShellState, method: Method, path: String, body: Option<Value>, reason: &str, done: Done) {
    let reason = Some(reason.trim().to_string()).filter(|r| !r.is_empty());
    st.intents.push(Intent::Social(Req { method, path, body, reason, done }));
}

/// The highest role position of a member (the owner outranks everyone).
fn rank(store: &Store, g: Snowflake, u: Snowflake) -> i64 {
    let Some(gs) = store.guilds.get(&g) else { return 0 };
    if gs.info.as_ref().is_some_and(|i| i.owner_id == u) {
        return i64::MAX;
    }
    let roles = store.member(g, u).map(|m| m.roles.clone()).unwrap_or_default();
    gs.roles.iter().filter(|r| roles.contains(&r.id)).map(|r| r.position as i64).max().unwrap_or(0)
}

/// `useGuildMemberModerationCapabilities`: the permission, and a higher role than the target.
pub fn can_moderate(store: &Store, g: Snowflake, target: Snowflake, perm: P) -> bool {
    let me = store.my_id();
    target != me && store.guild_permissions(g).contains(perm) && rank(store, g, me) > rank(store, g, target)
}

fn timed_out(store: &Store, g: Snowflake, u: Snowflake, now_iso: &str) -> bool {
    store.member(g, u).and_then(|m| m.communication_disabled_until.as_deref()).is_some_and(|t| t.get(..19).unwrap_or(t) > now_iso.get(..19).unwrap_or(now_iso))
}

/// The user context menu's community items: nickname, roles, moderation, voice and report.
pub fn member_items<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, u: Snowflake, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let me = store.my_id();
    let have = store.guild_permissions(g);
    let mut out = Vec::new();
    let open = move |m: Modal| move |s: &mut S| acc(s).open_social(m.clone());
    let member = store.member(g, u).is_some();
    // Your own nickname is the shell's (`status::open`); this is MANAGE_NICKNAMES on others.
    let nick_ok = u != me && can_moderate(store, g, u, P::MANAGE_NICKNAMES);
    if nick_ok && member {
        out.push(MenuEntry::Item(MenuItem::new("change-nickname", t("Change nickname")).icon(&icons::IDENTIFICATION_CARD_FILL).on_select(open(Modal::Nickname { guild: g, user: u }))));
    }
    if have.contains(P::MANAGE_ROLES) && member {
        let mine = rank(store, g, me);
        let has: Vec<Snowflake> = store.member(g, u).map(|m| m.roles.clone()).unwrap_or_default();
        let roles: Vec<MenuEntry<S>> = crate::guild::sorted_roles(store, g)
            .into_iter()
            .filter(|r| r.id != g && (r.position as i64) < mine)
            .map(|r| {
                let (rid, on) = (r.id, has.contains(&r.id));
                MenuEntry::Item(MenuItem::new(format!("role-{rid}"), r.name.clone()).checkbox(on).on_select(move |s: &mut S| {
                    acc(s).social(if on { Method::DELETE } else { Method::PUT }, format!("/guilds/{g}/members/{u}/roles/{rid}"), None, Done::Nothing)
                }))
            })
            .collect();
        if !roles.is_empty() {
            out.push(MenuEntry::Separator);
            out.push(MenuEntry::Submenu("roles".into(), "Roles".into(), roles));
        }
    }
    let owner = store.guilds.get(&g).and_then(|gs| gs.info.as_ref()).is_some_and(|i| i.owner_id == me);
    let mut mods = Vec::new();
    if owner && u != me && member && store.users.get(&u).is_none_or(|x| !x.bot) {
        mods.push(MenuItem::new("transfer", t("Transfer ownership")).icon(&icons::CROWN_FILL).danger().on_select(open(Modal::TransferOwnership { guild: g, user: u })));
    }
    if member && can_moderate(store, g, u, P::MODERATE_MEMBERS) {
        let now = iso(st.now_ms);
        mods.push(if timed_out(store, g, u, &now) {
            MenuItem::new("remove-timeout", t("Remove timeout")).icon(&icons::TIMER_FILL).danger().on_select(open(Modal::RemoveTimeout { guild: g, user: u }))
        } else {
            MenuItem::new("timeout", t("Timeout")).icon(&icons::TIMER_FILL).danger().on_select(open(Modal::Timeout { guild: g, user: u }))
        });
    }
    if member && can_moderate(store, g, u, P::KICK_MEMBERS) {
        mods.push(MenuItem::new("kick", t("Kick member")).icon(&icons::SIGN_OUT_FILL).danger().on_select(open(Modal::Kick { guild: g, user: u })));
    }
    if can_moderate(store, g, u, P::BAN_MEMBERS) {
        mods.push(MenuItem::new("ban", t("Ban member")).icon(&icons::GAVEL_FILL).danger().on_select(open(Modal::Ban { guild: g, user: u })));
    }
    if !mods.is_empty() {
        out.push(MenuEntry::Separator);
        out.extend(mods.into_iter().map(MenuEntry::Item));
    }
    out.extend(voice_items(store, g, u, acc));
    if u != me {
        out.push(MenuEntry::Separator);
        let m = Modal::Report { target: "user", guild: Some(g), channel: None, id: u };
        out.push(MenuEntry::Item(MenuItem::new("report-user", t("Report profile")).icon(&icons::FLAG_FILL).danger().on_select(move |s: &mut S| open_report(acc(s), m.clone()))));
    }
    out
}

/// Opens a report flow and fetches its screens.
pub fn open_report(st: &mut ShellState, m: Modal) {
    let Modal::Report { target, .. } = m else { return };
    st.open_social(m);
    st.social(Method::GET, format!("/reports/flows/{target}?surface=in_app"), None, Done::Form("flow".into()));
}

/// `VoiceParticipantMenuItems` for someone in a voice channel of the guild: community mute and
/// deafen, move, disconnect.
pub fn voice_items<S: 'static>(store: &Store, g: Snowflake, u: Snowflake, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let Some(vs) = store.voice_states.get(&(Some(g), u)).filter(|v| v.channel_id.is_some()) else { return Vec::new() };
    let have = store.guild_permissions(g);
    let me = store.my_id();
    // `canManageTarget`: yourself, or someone below your highest role.
    let manage = u == me || rank(store, g, me) > rank(store, g, u);
    let patch = move |b: Value| move |s: &mut S| acc(s).social(Method::PATCH, format!("/guilds/{g}/members/{u}"), Some(b.clone()), Done::Nothing);
    let mut out = vec![MenuEntry::Separator];
    if have.contains(P::MUTE_MEMBERS) && manage {
        out.push(MenuEntry::Item(MenuItem::new("server-mute", t("Community mute")).checkbox(vs.mute).on_select(patch(json!({ "mute": !vs.mute })))));
    }
    if have.contains(P::DEAFEN_MEMBERS) && manage {
        out.push(MenuEntry::Item(MenuItem::new("server-deafen", t("Community deafen")).checkbox(vs.deaf).on_select(patch(json!({ "deaf": !vs.deaf })))));
    }
    if can_moderate(store, g, u, P::MOVE_MEMBERS) {
        let mut chans: Vec<_> = store.channels.values().filter(|c| c.guild_id == Some(g) && c.kind == ChannelType::GUILD_VOICE && Some(c.id) != vs.channel_id && store.permissions(c.id).contains(P::CONNECT)).collect();
        chans.sort_by_key(|c| (c.position, c.id));
        let moves: Vec<MenuEntry<S>> = chans
            .into_iter()
            .map(|c| MenuEntry::Item(MenuItem::new(format!("move-{}", c.id), c.name.clone().unwrap_or_default()).icon(crate::shell::channel_icon(c.kind)).on_select(patch(json!({ "channel_id": c.id })))))
            .collect();
        if !moves.is_empty() {
            out.push(MenuEntry::Submenu("move".into(), t("Move to…").into(), moves));
        }
        let conn = vs.connection_id.clone();
        out.push(MenuEntry::Item(MenuItem::new("disconnect", t("Disconnect")).icon(&icons::PHONE_X_FILL).danger().on_select(patch(json!({ "channel_id": null, "connection_id": conn })))));
    }
    if out.len() == 1 { Vec::new() } else { out }
}

// ---- Kick / ban / timeout -------------------------------------------------------------------

pub fn kick<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, u: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let tag = user_name(store, u);
    let mut items = vec![desc(crate::i18n::tf_plain("Are you sure you want to kick <0>{targetUserTag}</0> from the community? They will be able to rejoin with a new invite.", &[("targetUserTag", &tag)]), p)];
    items.extend(error(st, p));
    let go = move |s: &mut S| send(acc(s), Method::DELETE, format!("/guilds/{g}/members/{u}"), None, "", Done::Close);
    Dialog::new(t("Kick member"), close(acc)).body(body(items)).footer(footer(st, t("Kick"), true, "kick-confirm", acc, go, p)).el(p)
}

const BAN_DURATIONS: [(&str, &str); 9] = [
    ("0", "Permanent"),
    ("3600", "1 hour"),
    ("43200", "12 hours"),
    ("86400", "1 day"),
    ("259200", "3 days"),
    ("432000", "5 days"),
    ("604800", "1 week"),
    ("1209600", "2 weeks"),
    ("2592000", "1 month"),
];
/// `BAN_DELETE_MESSAGE_OPTIONS`.
pub(crate) const BAN_DELETE: [(&str, &str, Option<&str>); 7] = [
    ("0", "Don't delete any", None),
    ("3600", "Previous hour", None),
    ("21600", "Previous 6 hours", None),
    ("43200", "Previous 12 hours", None),
    ("86400", "Previous 24 hours", None),
    ("259200", "Previous 3 days", None),
    ("604800", "Previous 7 days", None),
];

fn pick(st: &ShellState, key: &str, default: &str) -> String {
    st.form.data.get(key).and_then(Value::as_str).unwrap_or(default).to_string()
}

fn set<S: 'static>(acc: ShellAccess<S>, key: &'static str) -> impl Fn(&mut S, &str) + 'static {
    move |s: &mut S, v: &str| {
        acc(s).form.data.insert(key.into(), json!(v));
    }
}

pub fn ban<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, u: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let (dur, del) = (pick(st, "duration", "0"), pick(st, "delete", "86400"));
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let duration = col()
        .w_full()
        .gap(8.0)
        .child(Select::new("ban-duration", &BAN_DURATIONS, Some(&dur)).label(t("Ban duration")).width(406.0).on_choose(set(acc, "duration")).el(&st.popups, pops, p, layer))
        .child(text(t("How long they stay banned.")).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted));
    let history = col().w_full().gap(8.0).child(label(t("Delete message history"), p)).child(RadioGroup::new(&BAN_DELETE, Some(&del)).on_change(set(acc, "delete")).el(p));
    let mut items = vec![duration, history, field(st, acc, "reason", "", "Reason (optional)", "Enter a reason for the ban", p)];
    items.extend(error(st, p));
    let go = move |s: &mut S| {
        let sh = acc(s);
        let n = |k: &str, d: &str| pick(sh, k, d).parse::<u64>().unwrap_or(0);
        let reason = sh.form.value("reason", "").trim().to_string();
        let mut b = json!({ "delete_message_seconds": n("delete", "86400"), "ban_duration_seconds": n("duration", "0") });
        if !reason.is_empty() {
            b["reason"] = json!(reason);
        }
        send(sh, Method::PUT, format!("/guilds/{g}/bans/{u}"), Some(b), &reason, Done::Close);
    };
    Dialog::new(format!("Ban {}", user_name(store, u)), close(acc)).body(body(items)).footer(footer(st, t("Ban"), true, "ban-confirm", acc, go, p)).el(p)
}

/// `TimeoutMemberOptions`: seconds.
const TIMEOUTS: [(&str, &str); 6] = [("60", "60 seconds"), ("300", "5 minutes"), ("600", "10 minutes"), ("3600", "1 hour"), ("86400", "1 day"), ("604800", "1 week")];

pub fn timeout<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, u: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let tag = user_name(store, u);
    let dur = pick(st, "duration", "3600");
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let select = col()
        .w_full()
        .gap(8.0)
        .child(Select::new("timeout-duration", &TIMEOUTS, Some(&dur)).label(t("Timeout duration")).width(406.0).on_choose(set(acc, "duration")).el(&st.popups, pops, p, layer))
        .child(text(t("How long they stay timed out.")).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted));
    let mut items = vec![
        desc(format!("Prevent {tag} from sending messages, reacting, and joining voice channels for the specified duration."), p),
        select,
        field(st, acc, "reason", "", t("Reason (optional)"), "", p),
        text("This reason will be displayed in the activity log in Community settings > Activity log.").font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).wrap(),
    ];
    items.extend(error(st, p));
    let now = st.now_ms;
    let go = move |s: &mut S| {
        let sh = acc(s);
        let secs = pick(sh, "duration", "3600").parse::<u64>().unwrap_or(3600);
        let reason = sh.form.value("reason", "").trim().to_string();
        let mut b = json!({ "communication_disabled_until": iso(now + secs * 1000) });
        if !reason.is_empty() {
            b["timeout_reason"] = json!(reason);
        }
        send(sh, Method::PATCH, format!("/guilds/{g}/members/{u}"), Some(b), &reason, Done::Close);
    };
    Dialog::new(crate::i18n::tf("Time out {tag}", &[("tag", &tag.to_string())]), close(acc)).body(body(items)).footer(footer(st, t("Timeout"), true, "timeout-confirm", acc, go, p)).el(p)
}

pub fn remove_timeout<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, u: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let tag = user_name(store, u);
    let mut items = vec![desc(crate::i18n::tf_plain("Removing the timeout will allow <0>{targetUserTag}</0> to send messages, react, and join voice channels again.", &[("targetUserTag", &tag)]), p)];
    items.extend(error(st, p));
    let go = move |s: &mut S| send(acc(s), Method::PATCH, format!("/guilds/{g}/members/{u}"), Some(json!({ "communication_disabled_until": null })), "", Done::Close);
    Dialog::new(t("Remove timeout"), close(acc)).body(body(items)).footer(footer(st, t("Remove timeout"), true, "untimeout-confirm", acc, go, p)).el(p)
}

/// `BanDetailsModal`, from the Bans tab (its list is in `form.data["bans"]`).
pub fn ban_details<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, u: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let ban = st.form.data.get("ban").cloned().unwrap_or(Value::Null);
    let row_of = |k: &str, v: String| col().gap(4.0).child(text(k.to_uppercase()).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary)).child(text(v).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).wrap());
    let reason = ban["reason"].as_str().filter(|r| !r.is_empty()).map_or(t("No reason provided").to_string(), str::to_string);
    let on = ban["banned_at"].as_str().map_or(t("Unknown").to_string(), |t| t.get(..10).unwrap_or(t).to_string());
    let by = ban["moderator_id"].as_str().and_then(|i| i.parse().ok()).map_or(t("Unknown").to_string(), |m| user_name(store, m));
    let name = ban["user"]["global_name"].as_str().or(ban["user"]["username"].as_str()).map_or_else(|| user_name(store, u), str::to_string);
    let head = row()
        .gap(12.0)
        .center_y()
        .child(crate::shell::avatar(store, st, u, 40.0, None, p.background_secondary, p))
        .child(col().child(text(name).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary)).child(text(ban["user"]["username"].as_str().unwrap_or_default().to_string()).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted)));
    let mut items = vec![head, row_of(t("Reason"), reason), row().gap(24.0).child(row_of(t("Banned on"), on)).child(row_of(t("Banned by"), by))];
    items.extend(error(st, p));
    let revoke = Button::new(t("Revoke ban")).variant(ButtonVariant::Danger).submitting(st.form.busy).key("revoke-ban").on_press(move |s: &mut S| {
        let sh = acc(s);
        sh.social(Method::DELETE, format!("/guilds/{g}/bans/{u}"), None, Done::Close);
    });
    let back = move |s: &mut S| acc(s).open_guild_settings(g, "bans");
    Dialog::new(t("Ban details"), back).body(body(items)).footer(revoke.el(p)).el(p)
}

/// `BaseChangeNicknameModal`.
pub fn nickname<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, u: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let cur = store.member(g, u).and_then(|m| m.nick.clone()).unwrap_or_default();
    let display = user_name(store, u);
    let mut items = vec![field(st, acc, "nick", &cur, "Nickname", &display, p)];
    items.extend(error(st, p));
    let me = store.my_id();
    let go = move |s: &mut S| {
        let sh = acc(s);
        let nick = sh.form.value("nick", &cur).trim().to_string();
        if nick.chars().count() > 32 {
            sh.form.error = Some(t("Nickname must not exceed 32 characters").into());
            return;
        }
        let who = if u == me { "@me".to_string() } else { u.to_string() };
        sh.social(Method::PATCH, format!("/guilds/{g}/members/{who}"), Some(json!({ "nick": nick })), Done::Close);
    };
    Dialog::new(t("Change nickname"), close(acc)).body(body(items)).footer(footer(st, t("Save"), false, "nick-save", acc, go, p)).el(p)
}

/// `TransferOwnershipModal` (sudo: the password).
pub fn transfer<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, u: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let who = store.display_name(Some(g), u);
    let mut items = vec![
        desc(crate::i18n::tf_plain("You are about to transfer ownership of this community to <0>{0}</0>. This action is <1>irreversible</1> and you will lose all owner privileges.", &[("0", &who)]), p),
        field(st, acc, "password", "", t("Password"), "", p),
    ];
    items.extend(error(st, p));
    let go = move |s: &mut S| {
        let sh = acc(s);
        let b = json!({ "new_owner_id": u, "password": sh.form.value("password", "") });
        sh.social(Method::POST, format!("/guilds/{g}/transfer-ownership"), Some(b), Done::Close);
    };
    Dialog::new(t("Transfer community ownership"), close(acc)).body(body(items)).footer(footer(st, t("Transfer ownership"), true, "transfer-confirm", acc, go, p)).el(p)
}

// ---- Report flow ----------------------------------------------------------------------------

/// Where an outcome leads (`report flow outcome object`).
#[derive(Debug, PartialEq)]
pub enum Next {
    Screen(String),
    Submit(String),
    End(Option<String>),
    Link(String),
}

pub fn outcome(o: &Value) -> Next {
    let s = |k: &str| o[k].as_str().map(str::to_string);
    match o["type"].as_str() {
        Some("screen") => Next::Screen(s("screen_id").unwrap_or_default()),
        Some("submit") => Next::Submit(s("reason").unwrap_or_default()),
        Some("link") => Next::Link(s("url").unwrap_or_default()),
        _ => Next::End(s("notice_id")),
    }
}

/// Answers the current screen: records its step and follows the outcome. `answer` is an option
/// ID, ticked checklist items, or nothing (an info screen). Returns where the walk goes.
pub fn answer(flow: &Value, steps: &mut Vec<Value>, screen: &str, option: Option<&str>, items: &[String]) -> Option<Next> {
    let sc = flow["screens"].as_array()?.iter().find(|s| s["id"] == screen)?;
    let mut step = json!({ "screen_id": screen });
    let next = if let Some(opt) = option {
        step["option_id"] = json!(opt);
        outcome(&sc["options"].as_array()?.iter().find(|o| o["id"] == opt)?["outcome"])
    } else if sc["checklist"].is_object() {
        if (items.len() as u64) < sc["checklist"]["min_checked"].as_u64().unwrap_or(1) {
            return None;
        }
        step["item_ids"] = json!(items);
        outcome(&sc["checklist"]["outcome"])
    } else {
        Next::Screen(sc["next_screen_id"].as_str()?.to_string())
    };
    // Links leave the walk as it was.
    if !matches!(next, Next::Link(_)) {
        steps.push(step);
    }
    Some(next)
}

pub fn report<S: 'static>(st: &ShellState, target: &'static str, g: Option<Snowflake>, ch: Option<Snowflake>, id: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let title = match target {
        "message" => t("Report message"),
        "guild" => "Report community",
        _ => t("Report profile"),
    };
    let f = &st.form;
    let Some(flow) = f.data.get("flow").cloned() else {
        let mut items = vec![desc(if f.error.is_some() { "" } else { t("Loading…") }, p)];
        items.extend(error(st, p));
        return Dialog::new(title, close(acc)).body(body(items)).el(p);
    };
    if let Some(done) = f.data.get("done") {
        // The thank-you screen, or the notice an `end` outcome named.
        let notice = done.as_str().and_then(|n| flow["notices"].as_array()?.iter().find(|x| x["id"] == n).cloned());
        let (t, b) = match &notice {
            Some(n) => (n["title"].as_str().unwrap_or_default().to_string(), n["body"].as_str().unwrap_or_default().to_string()),
            None => (t("Report sent").to_string(), "Thanks for letting us know. Our safety team will review it.".to_string()),
        };
        return Dialog::new(title, close(acc)).body(body(vec![label(t, p), desc(b, p)])).footer(Button::new(crate::i18n::t("Done")).key("report-done").on_press(close(acc)).el(p)).el(p);
    }
    let screen = f.data.get("screen").and_then(Value::as_str).or(flow["start_screen_id"].as_str()).unwrap_or_default().to_string();
    let Some(sc) = flow["screens"].as_array().and_then(|a| a.iter().find(|s| s["id"] == screen.as_str())).cloned() else {
        return Dialog::new(title, close(acc)).body(desc("This report form isn't available.", p)).el(p);
    };
    let checked: Vec<String> = f.data.get("checked").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let flow2 = flow.clone();
    let step = move |s: &mut S, option: Option<String>, tx: Option<&mut rune::Tx>| {
        let sh = acc(s);
        let mut steps: Vec<Value> = sh.form.data.get("steps").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        let cur = sh.form.data.get("screen").and_then(Value::as_str).or(flow2["start_screen_id"].as_str()).unwrap_or_default().to_string();
        let items: Vec<String> = sh.form.data.get("checked").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        let Some(next) = answer(&flow2, &mut steps, &cur, option.as_deref(), &items) else {
            sh.form.error = Some("Pick at least one.".into());
            return;
        };
        sh.form.error = None;
        sh.form.data.insert("steps".into(), json!(steps));
        sh.form.data.remove("checked");
        match next {
            Next::Screen(id) => {
                sh.form.data.insert("screen".into(), json!(id));
            }
            Next::End(n) => {
                sh.form.data.insert("done".into(), json!(n));
            }
            Next::Link(url) => {
                if let Some(tx) = tx {
                    let _ = tx.open_url(&url);
                }
            }
            Next::Submit(_) => {
                let mut b = json!({ "revision_hash": flow2["revision_hash"], "steps": steps, "locale": flow2["locale"] });
                match target {
                    "message" => (b["channel_id"], b["message_id"]) = (json!(ch), json!(id)),
                    "guild" => b["guild_id"] = json!(id),
                    _ => {
                        b["user_id"] = json!(id);
                        if let Some(g) = g {
                            b["guild_id"] = json!(g);
                        }
                    }
                }
                sh.social(Method::POST, format!("/reports/flows/{target}/submissions"), Some(b), Done::Form("done".into()));
            }
        }
    };
    let step = std::rc::Rc::new(step);
    let mut items = vec![col().gap(4.0).child(label(sc["title"].as_str().unwrap_or_default(), p)).children(sc["subtitle"].as_str().map(|t| desc(t, p)))];
    if let Some(h) = sc["options_heading"].as_str() {
        items.push(text(h.to_uppercase()).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary));
    }
    let mut footer_btn = None;
    if let Some(opts) = sc["options"].as_array().filter(|o| !o.is_empty()) {
        let list = col().w_full().gap(4.0).children(opts.iter().map(|o| {
            let oid = o["id"].as_str().unwrap_or_default().to_string();
            let s2 = step.clone();
            row()
                .w_full()
                .py(10.0)
                .px(12.0)
                .gap(8.0)
                .center_y()
                .radius(8.0)
                .fill(p.background_secondary_alt)
                .hover(p.background_modifier_hover)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(o["label"].as_str().unwrap_or_default().to_string())
                .on_press_tx(move |s: &mut S, tx| s2(s, Some(oid.clone()), Some(tx)))
                .child(text(o["label"].as_str().unwrap_or_default().to_string()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0).shrink().wrap())
                .child(rune::icon(&icons::CARET_RIGHT_BOLD, 16.0).color(p.text_tertiary))
        }));
        items.push(list);
    } else {
        if let Some(cl) = sc["checklist"]["items"].as_array() {
            items.push(col().w_full().gap(8.0).children(cl.iter().map(|it| {
                let iid = it["id"].as_str().unwrap_or_default().to_string();
                let on = checked.contains(&iid);
                row()
                    .w_full()
                    .gap(10.0)
                    .items_start()
                    .cursor(rune::Cursor::Pointer)
                    .role("checkbox")
                    .checked(on)
                    .on_press(move |s: &mut S| {
                        let f = &mut acc(s).form;
                        let mut c: Vec<String> = f.data.get("checked").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                        if let Some(i) = c.iter().position(|x| *x == iid) {
                            c.remove(i);
                        } else {
                            c.push(iid.clone());
                        }
                        f.data.insert("checked".into(), json!(c));
                    })
                    .child(crate::ui::Checkbox::<S>::new(on).el(p))
                    .child(col().grow(1.0).shrink().child(text(it["label"].as_str().unwrap_or_default().to_string()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary)).children(it["description"].as_str().map(|d| text(d.to_string()).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).wrap())))
            })));
        }
        let s2 = step.clone();
        footer_btn = Some(Button::new(t("Next")).submitting(st.form.busy).key("report-next").on_press(move |s: &mut S| s2(s, None, None)).el(p));
    }
    if sc["urgent"].as_bool() == Some(true) {
        items.push(desc("If someone is in immediate danger, contact your local emergency services.", p));
    }
    items.extend(error(st, p));
    let mut d = Dialog::new(title, close(acc)).body(body(items));
    if let Some(b) = footer_btn {
        d = d.footer(b);
    }
    d.el(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow() -> Value {
        json!({
            "revision_hash": "abc", "start_screen_id": "root",
            "screens": [
                {"id": "root", "title": "Why?", "options": [
                    {"id": "spam", "label": "Spam", "outcome": {"type": "submit", "reason": "spam"}},
                    {"id": "parts", "label": "Profile", "outcome": {"type": "screen", "screen_id": "parts"}},
                    {"id": "dislike", "label": "I don't like it", "outcome": {"type": "end", "notice_id": "block"}},
                    {"id": "law", "label": "Illegal", "outcome": {"type": "link", "url": "https://x"}}]},
                {"id": "parts", "title": "Which?", "options": [], "checklist": {"items": [{"id": "photo"}, {"id": "name"}], "min_checked": 1,
                    "outcome": {"type": "screen", "screen_id": "info"}}},
                {"id": "info", "title": "Ok", "options": [], "next_screen_id": "root"}
            ]
        })
    }

    #[test]
    fn report_walks_follow_outcomes() {
        let f = flow();
        let mut steps = Vec::new();
        assert_eq!(answer(&f, &mut steps, "root", Some("parts"), &[]), Some(Next::Screen("parts".into())));
        assert_eq!(answer(&f, &mut steps, "parts", None, &[]), None, "the checklist needs one tick");
        assert_eq!(answer(&f, &mut steps, "parts", None, &["photo".into()]), Some(Next::Screen("info".into())));
        assert_eq!(answer(&f, &mut steps, "info", None, &[]), Some(Next::Screen("root".into())));
        assert_eq!(answer(&f, &mut steps, "root", Some("law"), &[]), Some(Next::Link("https://x".into())));
        assert_eq!(answer(&f, &mut steps, "root", Some("spam"), &[]), Some(Next::Submit("spam".into())));
        assert_eq!(steps, vec![
            json!({"screen_id": "root", "option_id": "parts"}),
            json!({"screen_id": "parts", "item_ids": ["photo"]}),
            json!({"screen_id": "info"}),
            json!({"screen_id": "root", "option_id": "spam"}),
        ]);
        assert_eq!(answer(&f, &mut Vec::new(), "root", Some("dislike"), &[]), Some(Next::End(Some("block".into()))));
    }
}
