//! Community settings (`GuildSettingsModal`, `DesktopGuildSettingsView`, `guild_tabs/*`) in the
//! user settings frame: permission-gated tabs, the community name over the nav, and Delete
//! community at its foot for the owner.

use reflux_model::{AuditLogEntry, Ban, ChannelType, Emoji, GuildMember, Invite, PartialUser, Permissions as P, Role, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, Icon, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use super::modals::confirm;
use super::{Confirm, Dialog, Form, Then, error, field, hex, parse_hex, pick_image};
use crate::icons;
use crate::media::{self, Load};
use crate::settings::tabs::{not_available, section};
use crate::settings::{colors, frame, nav_group, nav_item};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Menu, MenuItem, RadioGroup, Select, Switch, over, popup_access};

/// (id, category, label, icon, any of these permissions).
type TabDef = (&'static str, &'static str, &'static str, &'static Icon, P);

/// `GUILD_SETTINGS_TABS_DESCRIPTORS`. Explore needs the instance's discovery config, which Reflux
/// doesn't read, so it isn't listed.
const TABS: &[TabDef] = &[
    ("overview", "", "Overview", &icons::GEAR, P::MANAGE_GUILD),
    ("roles", "", "Roles", &icons::SHIELD, P::MANAGE_ROLES),
    ("moderation", "", "Moderation", &icons::HAMMER, P::MANAGE_GUILD),
    ("audit_log", "", "Activity log", &icons::BOOK_OPEN, P::VIEW_AUDIT_LOG),
    ("emoji", "Expressions", "Emoji", &icons::SMILEY, P::CREATE_EXPRESSIONS.union(P::MANAGE_EXPRESSIONS)),
    ("stickers", "Expressions", "Stickers", &icons::STICKER, P::CREATE_EXPRESSIONS.union(P::MANAGE_EXPRESSIONS)),
    ("vanity_url", "Community", "Vanity URL", &icons::LINK_BOLD, P::MANAGE_GUILD),
    ("webhooks", "Integrations", "Webhooks", &icons::WEBHOOKS_LOGO, P::MANAGE_WEBHOOKS),
    ("members", "People", "Members", &icons::USER, P::MANAGE_GUILD),
    ("invites", "People", "Invites", &icons::TICKET, P::MANAGE_GUILD),
    ("bans", "People", "Bans", &icons::PROHIBIT, P::BAN_MEMBERS),
];

/// Every tab id (for the demo).
pub fn tab_ids() -> impl Iterator<Item = &'static str> {
    TABS.iter().map(|t| t.0)
}

/// The tabs you may open in a guild.
pub fn tabs(store: &Store, guild: Snowflake) -> impl Iterator<Item = &'static TabDef> {
    let have = store.guild_permissions(guild);
    let vanity = store.guilds.get(&guild).and_then(|g| g.info.as_ref()).is_some_and(|i| i.features.iter().any(|f| f == "VANITY_URL"));
    TABS.iter().filter(move |t| have.intersects(t.4) && (t.0 != "vanity_url" || vanity))
}

/// Fetches what a tab lists.
pub fn load_tab(st: &mut ShellState, guild: Snowflake, tab: &str) {
    match tab {
        "members" => st.load("members", format!("/guilds/{guild}/members?limit=1000")),
        "invites" => st.load("invites", format!("/guilds/{guild}/invites")),
        "bans" => st.load("bans", format!("/guilds/{guild}/bans")),
        "audit_log" => st.load("audit", format!("/guilds/{guild}/audit-logs?limit=50")),
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
pub fn view<S: 'static>(store: &Store, st: &ShellState, guild: Snowflake, tab: &'static str, role: Option<Snowflake>, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some(info) = store.guilds.get(&guild).and_then(|g| g.info.as_ref()) else { return };
    let c = colors(p);
    let mut popups = Vec::new();
    let close = move |s: &mut S| acc(s).dialog = None;
    // `.sidebarHeader` / `.guildName`: the community name, 17/500, padding 6 4 6 10, mb 24.
    let head = row()
        .w_full()
        .pt(6.0)
        .pb(6.0)
        .pl(10.0)
        .pr(4.0)
        .mb(24.0)
        .child(text(info.name.clone()).font(font(17.07, 500.0).lh(22.0)).color(p.text_primary).truncate().grow(1.0).shrink());
    let mut nav = col().w_full().gap(3.0);
    let shown: Vec<&TabDef> = tabs(store, guild).collect();
    for cat in ["", "Expressions", "Community", "Integrations", "People"] {
        let items: Vec<El> = shown
            .iter()
            .filter(|t| t.1 == cat)
            .map(|t| {
                let id = t.0;
                nav_item(t.3, t.2, tab == id, false, p, &c).on_press(move |s: &mut S| select_tab(acc(s), guild, id))
            })
            .collect();
        if !items.is_empty() {
            nav = nav.child(nav_group((!cat.is_empty()).then_some(cat), items, p));
        }
    }
    if info.owner_id == store.my_id() {
        nav = nav.child(nav_item(&icons::TRASH, "Delete community", false, true, p, &c).on_press(move |s: &mut S| acc(s).form.confirm = Some(Confirm::DeleteGuild(guild))));
    }
    let label = TABS.iter().find(|t| t.0 == tab).map_or("Community settings", |t| t.2);
    let body = match tab {
        "overview" => overview(store, st, guild, p, acc, &mut popups),
        "roles" => match role {
            Some(r) => role_editor(store, st, guild, r, p, acc),
            None => roles(store, st, guild, p, acc),
        },
        "emoji" => emoji(store, st, guild, p, acc),
        "members" => members(store, st, guild, p, acc, &mut popups),
        "invites" => invites(store, st, guild, p, acc),
        "bans" => bans(store, st, guild, p, acc),
        "audit_log" => audit_log(store, st, p),
        _ => not_available(p),
    };
    let mut modal = frame("Community settings", label, vec![head], nav, body, close, p);
    if let Some(cf) = st.form.confirm {
        modal = modal.child(confirm(store, st, cf, p, acc));
    }
    layer.push(modal);
    layer.extend(popups);
}

fn select_tab(st: &mut ShellState, guild: Snowflake, tab: &'static str) {
    st.popups.close();
    st.form = Form::default();
    st.dialog = Some(Dialog::Guild { guild, tab, role: None });
    load_tab(st, guild, tab);
}

// ---- Shared bits ----------------------------------------------------------------------------

pub(super) fn small(t: impl Into<String>, p: &Palette) -> El {
    text(t.into()).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted)
}

pub(super) fn title(t: impl Into<String>, p: &Palette) -> El {
    text(t.into()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).truncate()
}

/// A bordered card of rows split by hairlines (`AccountSecurityCard`-style lists).
pub(super) fn card(rows: Vec<El>, p: &Palette) -> El {
    let n = rows.len();
    col().w_full().radius(12.0).border(1.0, p.background_modifier_accent).children(rows.into_iter().enumerate().map(move |(i, r)| {
        let r = row().w_full().px(16.0).py(12.0).gap(12.0).center_y().child(r);
        if i + 1 < n { r.child(el().pin(None, Some(0.0), Some(0.0), Some(0.0)).h(1.0).fill(p.background_modifier_accent)) } else { r }
    }))
}

pub(super) fn empty(t: &str, p: &Palette) -> El {
    text(t.to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).py(24.0).w_full().align(rune::Align::Center)
}

/// A list loaded into `form.data[key]`, or a loading / failed line.
#[allow(clippy::result_large_err)] // the Err is the El shown instead, built once a frame
pub(super) fn loaded<T: serde::de::DeserializeOwned>(st: &ShellState, key: &str, p: &Palette) -> Result<T, El> {
    // shortcut: parses the kept JSON every frame; keep typed lists if they get large.
    match st.form.data.get(key) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| empty(&format!("Couldn't read the list: {e}"), p)),
        None if st.form.error.is_some() => Err(error(st, p).unwrap_or_else(el)),
        None => Err(empty("Loading…", p)),
    }
}

fn user_name(u: &PartialUser) -> String {
    u.global_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| u.username.clone())
}

fn date(ts: &str) -> String {
    ts.get(..10).unwrap_or(ts).to_string()
}

/// The sticky "unsaved changes" bar with Reset and Save.
pub(super) fn save_bar<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, save: impl Fn(&mut S) + 'static) -> El {
    let bg = p.form_surface_background;
    let mut bar = row()
        .w_full()
        .mt(24.0)
        .py(10.0)
        .pl(17.0)
        .pr(11.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .child(text("You have unsaved changes.").font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0))
        .child(
            Button::new("Reset")
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Small)
                .on_press(move |s: &mut S| {
                    let f = &mut acc(s).form;
                    (f.text, f.error) = (Default::default(), None);
                    f.data.retain(|k, _| matches!(k.as_str(), "members" | "invites" | "bans" | "audit"));
                })
                .el(p),
        )
        .child(Button::new("Save changes").size(ButtonSize::Small).submitting(st.form.busy).key("save-changes").on_press(save).el(p));
    if let Some(e) = error(st, p) {
        bar = col().w_full().gap(8.0).child(bar).child(e);
    }
    bar
}

// ---- Overview -------------------------------------------------------------------------------

const AFK_TIMEOUTS: [(&str, &str); 5] = [("60", "1 minute"), ("300", "5 minutes"), ("900", "15 minutes"), ("1800", "30 minutes"), ("3600", "1 hour")];

/// The image a form field picked: `Some(Some(url))` new, `Some(None)` removed, `None` unchanged.
fn picked(f: &Form, key: &str) -> Option<Option<String>> {
    f.data.get(key).map(|v| v.as_str().map(str::to_string))
}

/// An icon or banner field: the current image (or the picked one's placeholder) and Change /
/// Remove buttons.
#[allow(clippy::too_many_arguments)]
fn image_field<S: 'static>(st: &ShellState, label: &str, key: &'static str, current: Load, w: f32, h: f32, radius: f32, p: &Palette, acc: ShellAccess<S>) -> El {
    let state = picked(&st.form, key);
    // `current` is `Failed` when there's no image.
    let has = matches!(state, Some(Some(_))) || (state.is_none() && !matches!(current, Load::Failed));
    let preview = match (&state, current) {
        (Some(Some(_)), _) => row().size(w, h).center().radius(radius).fill(p.brand_primary).child(icon(&icons::IMAGE_FILL, 28.0).color(p.brand_primary_fill)),
        (None, Load::Ready(px)) => el().size(w, h).radius(radius).pixels_fit(&px, rune::SurfaceFit::Fill),
        _ => row().size(w, h).center().radius(radius).fill(p.background_tertiary).border(1.0, over(p.background_modifier_accent, p.background_tertiary)).child(icon(&icons::IMAGE, 28.0).color(p.text_tertiary)),
    };
    let mut buttons = row().gap(8.0).child(Button::<S>::new(format!("Change {}", label.to_lowercase())).variant(ButtonVariant::Secondary).size(ButtonSize::Small).el(p).on_press_tx(pick_image(acc, key)));
    if has {
        buttons = buttons.child(
            Button::new(format!("Remove {}", label.to_lowercase()))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Small)
                .on_press(move |s: &mut S| {
                    acc(s).form.data.insert(key.into(), Value::Null);
                })
                .el(p),
        );
    }
    col().gap(8.0).child(title(label, p)).child(row().gap(16.0).center_y().child(preview).child(buttons))
}

fn overview<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some(info) = store.guilds.get(&g).and_then(|gs| gs.info.as_ref()) else { return not_available(p) };
    let f = &st.form;
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let icon_load = info.icon.as_deref().map_or(Load::Failed, |h| st.media.asset(|b, s| media::guild_icon_url(b, g, h, 80.0, s)));
    let banner_load = info.banner.as_deref().map_or(Load::Failed, |h| st.media.asset(|b, s| media::guild_banner_url(b, g, h, s)));
    let mut branding = col().w_full().gap(20.0).child(image_field(st, "Icon", "icon", icon_load, 80.0, 80.0, 40.0, p, acc));
    if info.features.iter().any(|f| f == "BANNER") {
        branding = branding.child(image_field(st, "Banner", "banner", banner_load, 240.0, 135.0, 8.0, p, acc));
    }
    branding = branding.child(field(st, acc, "name", &info.name, "Name", "My awesome community", p));

    // Channel pickers keep their choice in `form.data` as a string ID ("" = none).
    let chosen = |key: &str, cur: Option<Snowflake>| f.data.get(key).and_then(Value::as_str).map_or_else(|| cur.map_or(String::new(), |c| c.to_string()), str::to_string);
    let channels = |kind: ChannelType| -> Vec<(String, String)> {
        let mut v: Vec<_> = store.channels.values().filter(|c| c.guild_id == Some(g) && c.kind == kind).map(|c| (c.position, c.id.to_string(), c.name.clone().unwrap_or_else(|| "Unnamed channel".into()))).collect();
        v.sort();
        v.into_iter().map(|(_, id, n)| (id, n)).collect()
    };
    let pick = |id: &'static str, label: &'static str, none: &'static str, opts: Vec<(String, String)>, value: String, layer: &mut Vec<El>| {
        let all: Vec<(String, String)> = std::iter::once((String::new(), none.to_string())).chain(opts).collect();
        let refs: Vec<(&str, &str)> = all.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        Select::new(id, &refs, Some(&value))
            .label(label)
            .width(340.0)
            .on_choose(move |s: &mut S, v| {
                acc(s).form.data.insert(id.into(), json!(v));
            })
            .el(&st.popups, pops, p, layer)
    };
    let afk = chosen("afk", info.afk_channel_id);
    let afk_timeout = f.data.get("afk_timeout").and_then(Value::as_str).map_or(info.afk_timeout.to_string(), str::to_string);
    let idle = col()
        .w_full()
        .gap(16.0)
        .child(pick("afk", "AFK / idle channel", "No AFK channel", channels(ChannelType::GUILD_VOICE), afk.clone(), layer))
        .child(
            Select::new("afk_timeout", &AFK_TIMEOUTS, Some(&afk_timeout))
                .label("AFK timeout")
                .placeholder("Choose a timeout")
                .width(340.0)
                .on_choose(move |s: &mut S, v| {
                    acc(s).form.data.insert("afk_timeout".into(), json!(v));
                })
                .el(&st.popups, pops, p, layer),
        );
    let system = chosen("system", info.system_channel_id);
    let mut texts = channels(ChannelType::GUILD_TEXT);
    texts.extend(channels(ChannelType::GUILD_ANNOUNCEMENT));
    let hide_joins = f.data.get("hide_joins").and_then(Value::as_bool).unwrap_or(info.system_channel_flags.bits() & 1 != 0);
    let welcome = col()
        .w_full()
        .gap(16.0)
        .child(pick("system", "Destination channel", "No system channel", texts, system.clone(), layer))
        .child(
            Switch::new(hide_joins)
                .label("Hide join messages")
                .on_toggle(move |s: &mut S| {
                    acc(s).form.data.insert("hide_joins".into(), json!(!hide_joins));
                })
                .el(p),
        );
    let notif = f.data.get("notif").and_then(Value::as_str).map_or(info.default_message_notifications.to_string(), str::to_string);
    let notif_opts = [("0", "All messages", None), ("1", "Mentions only", None)];
    let notifications = col().w_full().gap(8.0).child(title("Default notification settings", p)).child(
        RadioGroup::new(&notif_opts, Some(&notif))
            .on_change(move |s: &mut S, v| {
                acc(s).form.data.insert("notif".into(), json!(v));
            })
            .el(p),
    );

    // The PATCH body: only what changed.
    let mut patch = serde_json::Map::new();
    let name = f.value("name", &info.name);
    if name.trim() != info.name {
        patch.insert("name".into(), json!(name.trim()));
    }
    for key in ["icon", "banner"] {
        if let Some(v) = picked(f, key) {
            patch.insert(key.into(), json!(v));
        }
    }
    let id = |s: &str| (!s.is_empty()).then(|| s.to_string());
    if id(&afk) != info.afk_channel_id.map(|c| c.to_string()) {
        patch.insert("afk_channel_id".into(), json!(id(&afk)));
    }
    if afk_timeout != info.afk_timeout.to_string() {
        patch.insert("afk_timeout".into(), json!(afk_timeout.parse::<u32>().unwrap_or(300)));
    }
    if id(&system) != info.system_channel_id.map(|c| c.to_string()) {
        patch.insert("system_channel_id".into(), json!(id(&system)));
    }
    if hide_joins != (info.system_channel_flags.bits() & 1 != 0) {
        patch.insert("system_channel_flags".into(), json!((info.system_channel_flags.bits() & !1) | u64::from(hide_joins)));
    }
    if notif != info.default_message_notifications.to_string() {
        patch.insert("default_message_notifications".into(), json!(notif.parse::<u8>().unwrap_or(0)));
    }
    let mut body = col()
        .w_full()
        .child(section("Branding", branding, true, p))
        .child(section("Idle settings", idle, false, p))
        .child(section("System & welcome", welcome, false, p))
        .child(section("Default notifications", notifications, false, p));
    // shortcut: the detached banner, invite splash, visits, message history and Advanced
    // sections aren't here yet.
    if !patch.is_empty() {
        let patch = Value::Object(patch);
        body = body.child(save_bar(st, p, acc, move |s: &mut S| {
            let sh = acc(s);
            sh.api(Method::PATCH, format!("/guilds/{g}"), Some(patch.clone()), Then::Keep("saved".into()));
            // Picked values are dropped once saved; the gateway's GUILD_UPDATE brings the result.
            sh.form.text.clear();
            sh.form.data.retain(|k, _| k == "saved");
        }));
    } else if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    body
}

// ---- Roles ----------------------------------------------------------------------------------

pub(super) fn sorted_roles(store: &Store, g: Snowflake) -> Vec<&Role> {
    let mut roles: Vec<&Role> = store.guilds.get(&g).map(|gs| gs.roles.iter().collect()).unwrap_or_default();
    roles.sort_by(|a, b| b.position.cmp(&a.position).then(a.id.cmp(&b.id)));
    roles
}

pub(super) fn color_dot(c: u32, p: &Palette) -> El {
    el().size(12.0, 12.0).radius(6.0).fill(if c == 0 { p.text_tertiary } else { crate::shell::role_color(c) })
}

fn roles<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let all = sorted_roles(store, g);
    let movable: Vec<Snowflake> = all.iter().filter(|r| r.id != g).map(|r| r.id).collect();
    let counts = |r: Snowflake| store.guilds.get(&g).map_or(0, |gs| gs.members.values().filter(|m| m.roles.contains(&r)).count());
    let rows: Vec<El> = all
        .iter()
        .map(|r| {
            let id = r.id;
            let i = movable.iter().position(|&x| x == id);
            let order = movable.clone();
            let mv = move |d: isize| {
                let order = order.clone();
                move |s: &mut S| {
                    let Some(i) = i else { return };
                    let j = i as isize + d;
                    if j < 0 || j as usize >= order.len() {
                        return;
                    }
                    let mut o = order.clone();
                    o.swap(i, j as usize);
                    // Top of the list ranks highest; the server renumbers from 1.
                    let n = o.len();
                    let body: Vec<Value> = o.iter().enumerate().map(|(k, r)| json!({ "id": r, "position": n - k })).collect();
                    acc(s).api(Method::PATCH, format!("/guilds/{g}/roles"), Some(Value::Array(body)), Then::Nothing);
                }
            };
            let arrow = |glyph: &'static Icon, label: &str, ok: bool, f: Box<dyn Fn(&mut S)>| {
                let b = row().size(28.0, 28.0).center().radius(6.0).role("button").label(label.to_string()).child(icon(glyph, 16.0).color(p.text_primary_muted));
                if ok { b.hover(p.background_modifier_hover).cursor(rune::Cursor::Pointer).on_press(f) } else { b.opacity(0.3).disabled(true) }
            };
            let name = if id == g { "@everyone".to_string() } else { r.name.clone() };
            let sub = if id == g { "Default permissions for all members".to_string() } else { format!("{} members", counts(id)) };
            let mut line = row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(color_dot(r.color, p))
                .child(col().grow(1.0).min_w(0.0).child(title(name, p)).child(small(sub, p)));
            if let Some(i) = i {
                line = line.child(arrow(&icons::ARROW_UP_BOLD, "Move up", i > 0, Box::new(mv(-1)))).child(arrow(&icons::ARROW_DOWN_BOLD, "Move down", i + 1 < movable.len(), Box::new(mv(1))));
            }
            line.child(Button::new("Edit").variant(ButtonVariant::Secondary).size(ButtonSize::Compact).on_press(move |s: &mut S| open_role(acc(s), g, id)).el(p))
        })
        .collect();
    let create = Button::new("Create role").icon(&icons::PLUS_BOLD).size(ButtonSize::Small).key("create-role").on_press(move |s: &mut S| {
        acc(s).api(Method::POST, format!("/guilds/{g}/roles"), Some(json!({ "name": "new role" })), Then::Select);
    });
    let mut content = col().w_full().gap(16.0).child(row().w_full().justify_between().center_y().child(small("Members use the colour of their highest role. Use the arrows to reorder roles.", p).wrap().shrink()).child(create.el(p)));
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    section("Roles", content.child(card(rows, p)), true, p)
}

fn open_role(st: &mut ShellState, g: Snowflake, r: Snowflake) {
    st.form = Form::default();
    st.dialog = Some(Dialog::Guild { guild: g, tab: "roles", role: Some(r) });
}

const SWATCHES: [u32; 10] = [0x1abc9c, 0x2ecc71, 0x3498db, 0x9b59b6, 0xe91e63, 0xf1c40f, 0xe67e22, 0xe74c3c, 0x95a5a6, 0x607d8b];

fn role_editor<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, rid: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let Some(r) = store.guilds.get(&g).and_then(|gs| gs.roles.iter().find(|x| x.id == rid)) else { return empty("This role no longer exists.", p) };
    let f = &st.form;
    let everyone = rid == g;
    let back = row()
        .gap(6.0)
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .role("link")
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.form = Form::default();
            sh.dialog = Some(Dialog::Guild { guild: g, tab: "roles", role: None });
        })
        .child(icon(&icons::ARROW_LEFT_BOLD, 16.0).color(p.text_link))
        .child(text("Back to roles").font(font(14.0, 500.0).lh(20.0)).color(p.text_link));
    let flag = |k: &str, cur: bool| f.data.get(k).and_then(Value::as_bool).unwrap_or(cur);
    let (hoist, mention) = (flag("hoist", r.hoist), flag("mentionable", r.mentionable));
    let bits = f.data.get("perms").and_then(Value::as_u64).map_or(r.permissions, P::from_bits_retain);
    let color_text = f.value("color", &if r.color == 0 { String::new() } else { hex(r.color) });
    let color = parse_hex(&color_text).unwrap_or(0);
    let set = move |k: &'static str, v: Value| move |s: &mut S| {
        acc(s).form.data.insert(k.into(), v.clone());
    };
    let mut display = col().w_full().gap(16.0);
    if !everyone {
        let swatches = row().gap(8.0).center_y().children(SWATCHES.iter().map(|&c| {
            el().size(28.0, 28.0)
                .radius(6.0)
                .fill(crate::shell::role_color(c))
                .border(2.0, if c == color { p.text_primary } else { crate::shell::role_color(c) })
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(hex(c))
                .on_press(move |s: &mut S| {
                    acc(s).form.text.insert("color".into(), rune::TextEdit::new(hex(c)));
                })
        }));
        display = display
            .child(field(st, acc, "rname", &r.name, "Role name", "", p))
            .child(col().gap(8.0).child(title("Role colour", p)).child(row().gap(12.0).center_y().child(el().size(40.0, 40.0).radius(8.0).fill(if color == 0 { p.text_tertiary } else { crate::shell::role_color(color) })).child(col().w(160.0).child(field(st, acc, "color", &if r.color == 0 { String::new() } else { hex(r.color) }, "", "#000000", p))).child(swatches)))
            .child(Switch::new(hoist).label("Display role members separately").on_toggle(set("hoist", json!(!hoist))).el(p))
            .child(Switch::new(mention).label("Allow anyone to @mention this role").on_toggle(set("mentionable", json!(!mention))).el(p));
    }
    let mut groups = col().w_full().gap(24.0);
    for (gt, entries) in super::perms::GUILD {
        let mut list = col().w_full().gap(4.0).child(text(gt.to_string()).font(font(12.0, 600.0).lh(16.0)).text_case(rune::TextCase::Uppercase).color(p.text_tertiary).mb(4.0));
        for &(bit, t, d) in entries.iter() {
            let on = bits.contains(bit);
            let next = if on { bits - bit } else { bits | bit };
            list = list.child(Switch::new(on).label(t).description(d).on_toggle(set("perms", json!(next.bits()))).el(p));
        }
        groups = groups.child(list);
    }
    let mut patch = serde_json::Map::new();
    let name = f.value("rname", &r.name);
    if !everyone && name.trim() != r.name && !name.trim().is_empty() {
        patch.insert("name".into(), json!(name.trim()));
    }
    if !everyone && color != r.color && (color_text.trim().is_empty() || parse_hex(&color_text).is_some()) {
        patch.insert("color".into(), json!(color));
    }
    if hoist != r.hoist {
        patch.insert("hoist".into(), json!(hoist));
    }
    if mention != r.mentionable {
        patch.insert("mentionable".into(), json!(mention));
    }
    if bits != r.permissions {
        patch.insert("permissions".into(), json!(bits.bits().to_string()));
    }
    let header = row().w_full().justify_between().center_y().child(back).child(if everyone {
        el()
    } else {
        Button::new("Delete role").variant(ButtonVariant::Danger).size(ButtonSize::Small).on_press(move |s: &mut S| acc(s).form.confirm = Some(Confirm::DeleteRole(g, rid))).el(p)
    });
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(header)
        .child(row().gap(8.0).center_y().child(color_dot(r.color, p)).child(text(if everyone { "@everyone".to_string() } else { r.name.clone() }).font(font(20.0, 600.0).lh(24.0)).color(p.text_primary)));
    if !everyone {
        body = body.child(section("Display", display, true, p));
    }
    body = body.child(section("Permissions", groups, everyone, p));
    if !patch.is_empty() {
        let patch = Value::Object(patch);
        body = body.child(save_bar(st, p, acc, move |s: &mut S| {
            let sh = acc(s);
            sh.api(Method::PATCH, format!("/guilds/{g}/roles/{rid}"), Some(patch.clone()), Then::Nothing);
            sh.form.text.clear();
            sh.form.data.clear();
        }));
    } else if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    body
}

// ---- Emoji ----------------------------------------------------------------------------------

/// An emoji name from a file name: ASCII letters, digits and `_`, 2-32 long (`guild-emojis.mdx`).
pub fn emoji_name(path: &std::path::Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let mut n: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).take(32).collect();
    while n.len() < 2 {
        n.push('_');
    }
    n
}

fn emoji<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let list: Vec<&Emoji> = store.guilds.get(&g).map(|gs| gs.emojis.iter().collect()).unwrap_or_default();
    let manage = store.guild_permissions(g).contains(P::MANAGE_EXPRESSIONS);
    let me = store.my_id();
    let upload = Button::<S>::new("Upload emoji").size(ButtonSize::Small).key("upload-emoji").el(p).on_press_tx(move |_: &mut S, tx: &mut Tx| {
        let options = rune::FileDialogOptions { title: "Upload emoji".into(), ..Default::default() }.filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "avif"]);
        tx.open_file(options, move |s: &mut S, r, _| {
            let sh = acc(s);
            match r {
                Ok(Some(path)) => match std::fs::read(&path) {
                    Ok(b) => sh.api(Method::POST, format!("/guilds/{g}/emojis"), Some(json!({ "name": emoji_name(&path), "image": super::data_url(&path, &b) })), Then::Nothing),
                    Err(e) => sh.form.error = Some(format!("Couldn't open the image: {e}")),
                },
                Ok(None) => {}
                Err(e) => sh.form.error = Some(e.to_string()),
            }
        });
    });
    let rows: Vec<El> = list
        .iter()
        .map(|e| {
            let id = e.id;
            let img = crate::shell::picture(st.media.asset(|b, _| media::emoji_url(b, &id.to_string())), 32.0, 32.0, 4.0, p);
            let by = e.user.as_ref().map(user_name).unwrap_or_default();
            let mut line = row().w_full().gap(12.0).center_y().child(img).child(col().grow(1.0).min_w(0.0).child(title(format!(":{}:", e.name), p)).child(small(by, p)));
            if manage || e.user.as_ref().is_some_and(|u| u.id == me) {
                line = line.child(
                    row()
                        .size(28.0, 28.0)
                        .center()
                        .radius(6.0)
                        .hover(p.background_modifier_hover)
                        .cursor(rune::Cursor::Pointer)
                        .role("button")
                        .label("Delete emoji")
                        .on_press(move |s: &mut S| acc(s).api(Method::DELETE, format!("/guilds/{g}/emojis/{id}"), None, Then::Nothing))
                        .child(icon(&icons::X_BOLD, 16.0).color(p.text_primary_muted)),
                );
            }
            line
        })
        .collect();
    let mut content = col()
        .w_full()
        .gap(16.0)
        .child(row().w_full().justify_between().center_y().gap(12.0).child(small("Names come from the file name: letters, digits and underscores. PNG, JPEG, GIF, WebP or AVIF up to 512 KB.", p).wrap().shrink()).child(upload));
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    content = content.child(if rows.is_empty() { empty("No emoji yet.", p) } else { card(rows, p) });
    section(&format!("Emoji — {}", list.len()), content, true, p)
}

// ---- Members --------------------------------------------------------------------------------

fn members<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let list: Vec<GuildMember> = match loaded(st, "members", p) {
        Ok(l) => l,
        Err(e) => return section("Members", e, true, p),
    };
    let have = store.guild_permissions(g);
    let owner = store.guilds.get(&g).and_then(|gs| gs.info.as_ref()).map(|i| i.owner_id);
    let q = st.form.value("search", "").trim().to_lowercase();
    let roles = sorted_roles(store, g);
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let shown: Vec<&GuildMember> = list
        .iter()
        .filter(|m| q.is_empty() || [Some(&m.user.username), m.user.global_name.as_ref(), m.nick.as_ref()].iter().flatten().any(|n| n.to_lowercase().contains(&q)))
        .take(200)
        .collect();
    let rows: Vec<El> = shown
        .iter()
        .map(|m| {
            let u = m.user.id;
            let name = m.nick.clone().unwrap_or_else(|| user_name(&m.user));
            let chips = row().gap(4.0).wrap().children(m.roles.iter().filter_map(|r| roles.iter().find(|x| x.id == *r)).map(|r| {
                row().gap(4.0).px(6.0).py(2.0).center_y().radius(4.0).fill(p.background_modifier_hover).child(color_dot(r.color, p).size(8.0, 8.0)).child(text(r.name.clone()).font(font(12.0, 500.0).lh(16.0)).color(p.text_primary))
            }));
            let mut line = row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(crate::shell::avatar(store, st, u, 32.0, None, p.background_secondary, p))
                .child(col().grow(1.0).min_w(0.0).gap(2.0).child(title(name, p)).child(small(m.user.username.clone(), p)).child(chips));
            let target = Some(u) != owner && u != store.my_id();
            if have.contains(P::MANAGE_ROLES) {
                let id = format!("member-roles-{u}");
                let mut menu = Menu::new(id.clone());
                for r in roles.iter().filter(|r| r.id != g) {
                    let (rid, on) = (r.id, m.roles.contains(&r.id));
                    menu = menu.item(MenuItem::new(format!("role-{rid}"), r.name.clone()).checkbox(on).on_select(move |s: &mut S| {
                        acc(s).api(
                            if on { Method::DELETE } else { Method::PUT },
                            format!("/guilds/{g}/members/{u}/roles/{rid}"),
                            None,
                            Then::Reload("members".into(), format!("/guilds/{g}/members?limit=1000")),
                        )
                    }));
                }
                menu.el(&st.popups, pops, p, layer);
                let toggle = id.clone();
                line = line.child(Button::new("Roles").variant(ButtonVariant::Secondary).size(ButtonSize::Compact).key(id).on_press(move |s: &mut S| acc(s).popups.toggle(&toggle)).el(p));
            }
            if target && have.contains(P::KICK_MEMBERS) {
                line = line.child(Button::new("Kick").variant(ButtonVariant::Secondary).size(ButtonSize::Compact).on_press(move |s: &mut S| acc(s).form.confirm = Some(Confirm::Kick(g, u))).el(p));
            }
            if target && have.contains(P::BAN_MEMBERS) {
                line = line.child(Button::new("Ban").variant(ButtonVariant::Danger).size(ButtonSize::Compact).on_press(move |s: &mut S| acc(s).form.confirm = Some(Confirm::Ban(g, u))).el(p));
            }
            line
        })
        .collect();
    let mut content = col().w_full().gap(16.0).child(field(st, acc, "search", "", "", "Search members", p));
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    content = content.child(if rows.is_empty() { empty("No members match.", p) } else { card(rows, p) });
    // shortcut: lists the first 1000 members (one page of `GET /guilds/{id}/members`).
    section(&format!("Members — {}", list.len()), content, true, p)
}

// ---- Invites and bans -----------------------------------------------------------------------

fn invites<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let list: Vec<Invite> = match loaded(st, "invites", p) {
        Ok(l) => l,
        Err(e) => return section("Invites", e, true, p),
    };
    let rows: Vec<El> = list
        .iter()
        .map(|i| {
            let code = i.code.clone();
            let by = i.inviter.as_ref().map(user_name).unwrap_or_else(|| "Unknown".into());
            let ch = i.channel.as_ref().and_then(|c| c.name.clone()).or_else(|| i.channel_id.and_then(|c| store.channels.get(&c)?.name.clone())).unwrap_or_default();
            let uses = if i.max_uses == 0 { format!("{} uses", i.uses) } else { format!("{}/{} uses", i.uses, i.max_uses) };
            let expires = i.expires_at.as_deref().map_or("Never expires".to_string(), |t| format!("Expires {}", date(t)));
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(col().grow(1.0).min_w(0.0).child(title(code.clone(), p)).child(small(format!("#{ch} · by {by} · {uses} · {expires}"), p)))
                .child(Button::new("Revoke").variant(ButtonVariant::Danger).size(ButtonSize::Compact).on_press(move |s: &mut S| {
                    acc(s).api(Method::DELETE, format!("/invites/{code}"), None, Then::Reload("invites".into(), format!("/guilds/{g}/invites")))
                }).el(p))
        })
        .collect();
    let mut content = col().w_full().gap(16.0);
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    section("Invites", content.child(if rows.is_empty() { empty("No invites yet.", p) } else { card(rows, p) }), true, p)
}

fn bans<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let list: Vec<Ban> = match loaded(st, "bans", p) {
        Ok(l) => l,
        Err(e) => return section("Bans", e, true, p),
    };
    let q = st.form.value("search", "").trim().to_lowercase();
    let rows: Vec<El> = list
        .iter()
        .filter(|b| q.is_empty() || b.user.username.to_lowercase().contains(&q))
        .map(|b| {
            let u = b.user.id;
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(crate::shell::avatar(store, st, u, 32.0, None, p.background_secondary, p))
                .child(col().grow(1.0).min_w(0.0).child(title(user_name(&b.user), p)).child(small(b.reason.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| "No reason given".into()), p)))
                .child(Button::new("Unban").variant(ButtonVariant::Secondary).size(ButtonSize::Compact).on_press(move |s: &mut S| {
                    acc(s).api(Method::DELETE, format!("/guilds/{g}/bans/{u}"), None, Then::Reload("bans".into(), format!("/guilds/{g}/bans")))
                }).el(p))
        })
        .collect();
    let mut content = col().w_full().gap(16.0).child(field(st, acc, "search", "", "", "Search bans", p));
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    section(&format!("Bans — {}", list.len()), content.child(if rows.is_empty() { empty("No bans.", p) } else { card(rows, p) }), true, p)
}

// ---- Activity log ---------------------------------------------------------------------------

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct AuditLog {
    audit_log_entries: Vec<AuditLogEntry>,
    users: Vec<PartialUser>,
}

/// What an audit action did, as a phrase after the actor (`audit-logs.mdx` "Audit actions").
pub fn action_phrase(t: u32) -> &'static str {
    match t {
        1 => "updated the community",
        10 => "created a channel",
        11 => "updated a channel",
        12 => "deleted a channel",
        13 => "added a permission overwrite",
        14 => "updated a permission overwrite",
        15 => "removed a permission overwrite",
        20 => "kicked a member",
        21 => "pruned members",
        22 => "banned a member",
        23 => "unbanned a member",
        24 => "updated a member",
        25 => "updated a member's roles",
        26 => "moved a member",
        27 => "disconnected a member",
        28 => "added a bot",
        30 => "created a role",
        31 => "updated a role",
        32 => "deleted a role",
        40 => "created an invite",
        41 => "updated an invite",
        42 => "deleted an invite",
        50 => "created a webhook",
        51 => "updated a webhook",
        52 => "deleted a webhook",
        60 => "created an emoji",
        61 => "updated an emoji",
        62 => "deleted an emoji",
        72 => "deleted a message",
        73 => "deleted messages",
        74 => "pinned a message",
        75 => "unpinned a message",
        90 => "created a sticker",
        91 => "updated a sticker",
        92 => "deleted a sticker",
        110 => "created a thread",
        111 => "updated a thread",
        112 => "deleted a thread",
        _ => "made a change",
    }
}

fn audit_log(store: &Store, st: &ShellState, p: &Palette) -> El {
    let log: AuditLog = match loaded(st, "audit", p) {
        Ok(l) => l,
        Err(e) => return section("Activity log", e, true, p),
    };
    let known = |id: Snowflake| {
        let me = store.me.as_ref().filter(|m| m.id == id).map(|m| &m.base);
        log.users.iter().find(|u| u.id == id).or(store.users.get(&id)).or(me).map(user_name)
    };
    let who = |id: Snowflake| known(id).unwrap_or_else(|| if id.0 == 0 { "System".into() } else { id.to_string() });
    let target = |e: &AuditLogEntry| -> Option<String> {
        let t = e.target_id.as_deref()?;
        let id: Option<Snowflake> = t.parse().ok();
        id.and_then(|id| store.channels.get(&id).and_then(|c| c.name.clone()).map(|n| format!("#{n}")))
            .or_else(|| id.and_then(|id| store.guilds.values().flat_map(|g| g.roles.iter()).find(|r| r.id == id).map(|r| format!("@{}", r.name))))
            .or_else(|| id.and_then(known))
            .or_else(|| Some(t.to_string()))
    };
    let rows: Vec<El> = log
        .audit_log_entries
        .iter()
        .map(|e| {
            let mut line = format!("{} {}", who(e.user_id), action_phrase(e.action_type));
            if let Some(t) = target(e).filter(|_| e.action_type != 1) {
                line.push_str(&format!(" — {t}"));
            }
            let when = crate::shell::format_relative(e.id.timestamp_ms(), st.now_ms);
            let mut c = col().grow(1.0).min_w(0.0).child(text(line).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).wrap()).child(small(when, p));
            if let Some(r) = e.reason.as_ref().filter(|r| !r.is_empty()) {
                c = c.child(small(format!("Reason: {r}"), p));
            }
            row().w_full().gap(12.0).center_y().child(crate::shell::avatar(store, st, e.user_id, 32.0, None, p.background_secondary, p)).child(c)
        })
        .collect();
    // shortcut: the newest 50 entries, no filters or paging, and no per-field change details.
    section("Activity log", if rows.is_empty() { empty("Nothing has happened yet.", p) } else { card(rows, p) }, true, p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_names_follow_the_api_rules() {
        assert_eq!(emoji_name(std::path::Path::new("/x/party parrot!.gif")), "party_parrot_");
        assert_eq!(emoji_name(std::path::Path::new("a.png")), "a_");
        assert_eq!(emoji_name(std::path::Path::new(&"x".repeat(40))).len(), 32);
    }

    #[test]
    fn audit_phrases() {
        assert_eq!(action_phrase(22), "banned a member");
        assert_eq!(action_phrase(999), "made a change");
    }
}
