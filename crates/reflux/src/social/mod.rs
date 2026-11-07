//! People and moderation: kick/ban/timeout and the report flow, DMs and group DMs, the inbox
//! (recent mentions and bookmarks), forwarding and publishing, webhooks, discovery, and the
//! channel-level bits around them (welcome sections, blocked message groups, the notification
//! bell, mute durations, the @everyone confirmation).
//!
//! Like [`crate::guild`], views only queue requests ([`Intent::Social`]); [`run`] sends them
//! (with an optional audit-log reason) and applies their [`Done`]. Most effects come back over the
//! gateway.

mod channel;
pub mod demo;
mod discovery;
mod dm;
mod inbox;
pub(crate) mod moderation;
mod tabs;

use std::collections::{HashMap, HashSet};

use reflux_model::{Snowflake, UserGuildSettings};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, TextEdit, col, row, text};
use serde_json::{Value, json};

use crate::guild::Dialog;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Input, mix};
use crate::i18n::t;

pub use channel::{bell_button, blocked_bar, blocked_runs, mass_mention, mute_entry, welcome};
pub use discovery::{discovery_page, invite_code, open as open_discovery, open_invite, preview_nagbar};
pub use dm::{group_items, open_create_dm};
pub use inbox::{inbox_button, message_items};
pub use moderation::{member_items, open_report};
pub use tabs::{channel_tab, guild_tab, load_channel_tab, load_guild_tab, pause_invites};

/// Fluxer's inbox and page-level state that outlives a dialog.
#[derive(Default)]
pub struct Social {
    /// Lists fetched for views that aren't dialogs (mentions, bookmarks, discovery results).
    pub data: HashMap<String, Value>,
    /// The inbox popout is open, and its tab: 0 unread, 1 bookmarks, 2 mentions.
    pub inbox_open: bool,
    pub inbox_tab: u8,
    /// Recent mention filters: (role mentions, @everyone/@here, all communities).
    pub mention_filter: [bool; 3],
    /// Bookmarked message IDs known this session (the menu's Bookmark / Remove bookmark).
    pub saved: HashSet<Snowflake>,
    /// Blocked-message runs shown anyway, by their first message.
    pub show_blocked: HashSet<Snowflake>,
    /// The discovery page is open (over the home view).
    pub discovery: bool,
    pub discovery_query: TextEdit,
    pub discovery_category: Option<u8>,
    /// A message held back by the @everyone confirmation: (channel, mention, members).
    pub mass: Option<(Snowflake, &'static str, u64)>,
    /// The API base (`…/api/v1`'s parent), for webhook URLs.
    pub api_base: String,
    /// The composer's (length limit, slowmode) for a send held by the confirmation.
    pub pending_send: (usize, u32),
}

impl Social {
    pub fn new() -> Self {
        Self { mention_filter: [true, true, true], ..Default::default() }
    }
}

/// The social dialogs (inside [`Dialog::Social`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Modal {
    Kick { guild: Snowflake, user: Snowflake },
    Ban { guild: Snowflake, user: Snowflake },
    Timeout { guild: Snowflake, user: Snowflake },
    RemoveTimeout { guild: Snowflake, user: Snowflake },
    BanDetails { guild: Snowflake, user: Snowflake },
    /// A member's community nickname (`user` = me for my own).
    Nickname { guild: Snowflake, user: Snowflake },
    TransferOwnership { guild: Snowflake, user: Snowflake },
    /// `ReportFlowModal`: `target` is "message", "user" or "guild"; `ids` the subject.
    Report { target: &'static str, guild: Option<Snowflake>, channel: Option<Snowflake>, id: Snowflake },
    CreateDm,
    EditGroup(Snowflake),
    AddToGroup(Snowflake),
    FriendNickname(Snowflake),
    GroupNickname { channel: Snowflake, user: Snowflake },
    Forward { channel: Snowflake, message: Snowflake },
    Publish { channel: Snowflake, message: Snowflake },
    Follow(Snowflake),
    Topic(Snowflake),
    Duplicate(Snowflake),
    /// `InviteAcceptModal` for an invite code.
    InviteAccept(String),
    /// A webhook being edited.
    Webhook { guild: Snowflake, id: Snowflake },
}

/// A request a view asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct Req {
    pub method: Method,
    pub path: String,
    pub body: Option<Value>,
    /// `X-Audit-Log-Reason`.
    pub reason: Option<String>,
    pub done: Done,
}

/// What to do once a [`Req`] succeeds; a failure shows on the dialog as `form.error`.
#[derive(Clone, Debug, PartialEq)]
pub enum Done {
    Nothing,
    /// Close the dialog it came from.
    Close,
    /// Keep the response as `form.data[key]`.
    Form(String),
    /// Keep the response as `social.data[key]`.
    Data(String),
    /// GET `path` into `form.data[key]` (a list the request changed).
    Reload(String, String),
    /// Open the guild or channel the response is, and close the dialog.
    Enter,
    /// Open the DM or group DM channel the response is, and close the dialog.
    OpenChannel,
    /// Show this community (joined or visited), closing Explore and the dialog.
    EnterGuild(Snowflake),
    /// Back to a community settings tab.
    GuildTab(Snowflake, &'static str),
}

impl ShellState {
    pub fn social(&mut self, method: Method, path: impl Into<String>, body: Option<Value>, done: Done) {
        self.intents.push(Intent::Social(Req { method, path: path.into(), body, reason: None, done }));
    }

    pub fn open_social(&mut self, m: Modal) {
        self.open_dialog(Dialog::Social(m));
    }
}

/// Sends a [`Req`] and applies its [`Done`] (runs on the UI thread; the request on the runtime).
pub fn run(s: &mut crate::State, r: Req) {
    let Some(inst) = s.app.instance.clone() else { return };
    let sh = &mut s.app.shell;
    let from = sh.dialog.clone();
    sh.form.busy = true;
    sh.form.error = None;
    crate::app::spawn(async move {
        let res = inst.rest.request_with_reason::<Value, Value>(r.method.clone(), &r.path, r.body.as_ref(), r.reason.as_deref()).await;
        let reload = match (&res, &r.done) {
            (Ok(_), Done::Reload(_, path)) => Some(inst.rest.get::<Value>(path).await),
            _ => None,
        };
        crate::app::update(move |s| {
            let sh = &mut s.app.shell;
            let same = sh.dialog == from;
            if same {
                sh.form.busy = false;
            }
            let v = match res {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("reflux: {} {}: {e}", r.method, r.path);
                    if same {
                        sh.form.error = Some(crate::guild::describe(&e));
                    }
                    return;
                }
            };
            apply(sh, r.done, v, reload, same);
        });
    });
}

fn apply(sh: &mut ShellState, done: Done, v: Value, reload: Option<reflux_net::Result<Value>>, same: bool) {
    match done {
        Done::Nothing => {}
        Done::Close if same => sh.dialog = None,
        Done::Form(k) if same => {
            sh.form.data.insert(k, v);
        }
        Done::Data(k) => {
            sh.social.data.insert(k, v);
        }
        Done::Reload(k, _) if same => match reload {
            Some(Ok(list)) => {
                sh.form.data.insert(k, list);
            }
            Some(Err(e)) => sh.form.error = Some(crate::guild::describe(&e)),
            None => {}
        },
        Done::Enter => {
            sh.dialog = None;
            sh.social.discovery = false;
            if let Some((g, ch)) = crate::app::entered(&v) {
                sh.select_guild(Some(g), ch);
            }
        }
        Done::EnterGuild(g) => {
            sh.dialog = None;
            sh.social.discovery = false;
            sh.select_guild(Some(g), None);
        }
        Done::GuildTab(g, tab) if same => sh.open_guild_settings(g, tab),
        Done::OpenChannel => {
            sh.dialog = None;
            if let Some(id) = id_of(&v, "id") {
                sh.select_guild(None, Some(id));
            }
        }
        _ => {}
    }
}

/// Every overlay this module draws for the shell.
pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    inbox::popout(store, st, p, acc, layer);
    channel::bell_layer(store, st, p, acc, layer);
    channel::mass_popout(st, p, acc, layer);
    inbox::filter_menu(st, p, acc, layer);
    if let Some(Dialog::Social(m)) = &st.dialog {
        let start = layer.len();
        let modal = match m.clone() {
            Modal::Kick { guild, user } => moderation::kick(store, st, guild, user, p, acc),
            Modal::Ban { guild, user } => moderation::ban(store, st, guild, user, p, acc, layer),
            Modal::Timeout { guild, user } => moderation::timeout(store, st, guild, user, p, acc, layer),
            Modal::RemoveTimeout { guild, user } => moderation::remove_timeout(store, st, guild, user, p, acc),
            Modal::BanDetails { guild, user } => moderation::ban_details(store, st, guild, user, p, acc),
            Modal::Nickname { guild, user } => moderation::nickname(store, st, guild, user, p, acc),
            Modal::TransferOwnership { guild, user } => moderation::transfer(store, st, guild, user, p, acc),
            Modal::Report { target, guild, channel, id } => moderation::report(st, target, guild, channel, id, p, acc),
            Modal::CreateDm => dm::create_dm(store, st, None, p, acc),
            Modal::AddToGroup(c) => dm::create_dm(store, st, Some(c), p, acc),
            Modal::EditGroup(c) => dm::edit_group(store, st, c, p, acc),
            Modal::FriendNickname(u) => dm::friend_nickname(store, st, u, p, acc),
            Modal::GroupNickname { channel, user } => dm::group_nickname(store, st, channel, user, p, acc),
            Modal::Forward { channel, message } => inbox::forward(store, st, channel, message, p, acc),
            Modal::Publish { channel, message } => inbox::publish(store, st, channel, message, p, acc),
            Modal::Follow(c) => inbox::follow(store, st, c, p, acc, layer),
            Modal::Topic(c) => channel::topic(store, st, c, p, acc),
            Modal::Duplicate(c) => channel::duplicate(store, st, c, p, acc),
            Modal::InviteAccept(code) => discovery::invite_accept(store, st, &code, p, acc),
            Modal::Webhook { guild, id } => tabs::webhook_editor(store, st, guild, id, p, acc, layer),
        };
        // Under any select list it opened.
        layer.insert(start, modal);
    }
}

// ---- Shared bits ---------------------------------------------------------------------------

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` (UTC), the API's timestamp format.
pub fn iso(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64).unwrap_or_default().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn id_of(v: &Value, k: &str) -> Option<Snowflake> {
    v.get(k).and_then(Value::as_str).and_then(|s| s.parse().ok())
}

fn close<S: 'static>(acc: ShellAccess<S>) -> impl Fn(&mut S) + Clone + 'static {
    move |s: &mut S| acc(s).dialog = None
}

/// `Modal.Description`: 14/400, line-height 1.4, `text-primary-muted`.
fn desc(t: impl Into<String>, p: &Palette) -> El {
    text(t.into()).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap()
}

/// `.sectionTitle`: 14/600 primary.
fn label(t: impl Into<String>, p: &Palette) -> El {
    text(t.into()).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary)
}

/// Cancel + the primary action, as `ConfirmModal` lays them out.
fn footer<S: 'static>(st: &ShellState, action: &str, danger: bool, key: &str, acc: ShellAccess<S>, go: impl Fn(&mut S) + 'static, p: &Palette) -> El {
    row()
        .gap(8.0)
        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close(acc)).el(p))
        .child(Button::new(action).variant(if danger { ButtonVariant::Danger } else { ButtonVariant::Primary }).submitting(st.form.busy).key(key.to_string()).on_press(go).el(p))
}

/// A form text field (`form.text[key]`, else `initial`).
fn field<S: 'static>(st: &ShellState, acc: ShellAccess<S>, key: &'static str, initial: &str, label: &str, placeholder: &str, p: &Palette) -> El {
    let edit = st.form.text.get(key).cloned().unwrap_or_else(|| TextEdit::new(initial));
    let init = initial.to_string();
    let el_key = format!("form-{key}");
    let mut i = Input::new(&el_key, &edit, move |s: &mut S| -> &mut TextEdit { acc(s).form.text.entry(key.to_string()).or_insert_with(|| TextEdit::new(init.clone())) }).placeholder(placeholder);
    if !label.is_empty() {
        i = i.label(label);
    }
    i.el(p)
}

/// `form.error` in the danger colour, if any.
fn error(st: &ShellState, p: &Palette) -> Option<El> {
    st.form.error.as_ref().map(|e| text(e.clone()).font(font(14.0, 400.0).lh(20.0)).color(mix(p.status_danger, p.text_primary, 78.0)).wrap())
}

fn body(items: Vec<El>) -> El {
    col().w_full().gap(16.0).children(items)
}

/// A user's tag-ish name: global name, else username.
fn user_name(store: &Store, u: Snowflake) -> String {
    let me = store.me.as_ref().filter(|m| m.id == u).map(|m| &m.base);
    store.users.get(&u).or(me).map_or_else(|| u.to_string(), |x| x.global_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| x.username.clone()))
}

fn guild_name(store: &Store, g: Snowflake) -> String {
    store.guilds.get(&g).and_then(|gs| gs.info.as_ref()).map_or(String::new(), |i| i.name.clone())
}

/// A user guild settings patch that changes one channel override (the API replaces the whole
/// `channel_overrides` map).
pub fn override_patch(store: &Store, guild: Option<Snowflake>, channel: Snowflake, f: impl FnOnce(&mut Value)) -> Value {
    let cur = store.guild_settings.get(&guild).cloned().unwrap_or_else(|| UserGuildSettings { guild_id: guild, ..Default::default() });
    let mut all = serde_json::to_value(cur.channel_overrides.unwrap_or_default()).unwrap_or_else(|_| json!({}));
    let key = channel.to_string();
    let mut ov = all.get(&key).cloned().unwrap_or_else(|| json!({ "muted": false, "message_notifications": 3 }));
    f(&mut ov);
    all[key] = ov;
    json!({ "channel_overrides": all })
}

/// Clears mutes whose `mute_config.end_time` has passed (`now_iso`: the API's UTC timestamp
/// format); true when any did. Fluxer unmutes locally when the timer runs out.
pub fn expire_mutes(store: &mut Store, now_iso: &str) -> bool {
    let past = |c: &Option<reflux_model::MuteConfig>| c.as_ref().and_then(|c| c.end_time.as_deref()).is_some_and(|t| t.get(..19).unwrap_or(t) <= now_iso.get(..19).unwrap_or(now_iso));
    let mut any = false;
    for s in store.guild_settings.values_mut() {
        if s.muted && past(&s.mute_config) {
            (s.muted, s.mute_config) = (false, None);
            any = true;
        }
        for o in s.channel_overrides.iter_mut().flat_map(|m| m.values_mut()) {
            if o.muted && past(&o.mute_config) {
                (o.muted, o.mute_config) = (false, None);
                any = true;
            }
        }
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_mutes_expire() {
        let mut store = Store::default();
        let s: UserGuildSettings = serde_json::from_value(json!({
            "guild_id": "1", "muted": true, "mute_config": {"end_time": "2026-11-07T10:00:00.000Z", "selected_time_window": 900000},
            "channel_overrides": {"5": {"muted": true, "mute_config": {"end_time": "2026-11-07T12:00:00Z", "selected_time_window": 3600000}},
                                  "6": {"muted": true, "mute_config": null}}
        }))
        .unwrap();
        store.guild_settings.insert(Some(Snowflake(1)), s);
        assert!(!expire_mutes(&mut store, "2026-11-07T09:59:59.000Z"));
        assert!(expire_mutes(&mut store, "2026-11-07T11:00:00.000Z"));
        let s = &store.guild_settings[&Some(Snowflake(1))];
        assert!(!s.muted);
        let ov = s.channel_overrides.as_ref().unwrap();
        assert!(ov[&Snowflake(5)].muted && ov[&Snowflake(6)].muted, "later and untimed mutes stay");
        assert!(expire_mutes(&mut store, "2026-11-07T12:00:01.000Z"));
        assert!(!store.guild_settings[&Some(Snowflake(1))].channel_overrides.as_ref().unwrap()[&Snowflake(5)].muted);
    }

    #[test]
    fn override_patches_keep_other_channels() {
        let mut store = Store::default();
        let s: UserGuildSettings = serde_json::from_value(json!({"guild_id": "1", "channel_overrides": {"5": {"muted": true}}})).unwrap();
        store.guild_settings.insert(Some(Snowflake(1)), s);
        let p = override_patch(&store, Some(Snowflake(1)), Snowflake(6), |o| o["message_notifications"] = json!(2));
        assert_eq!(p["channel_overrides"]["5"]["muted"], json!(true));
        assert_eq!(p["channel_overrides"]["6"]["message_notifications"], json!(2));
        assert_eq!(p["channel_overrides"]["6"]["muted"], json!(false));
    }
}
