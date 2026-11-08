//! Community settings (`GuildSettingsModal`, `DesktopGuildSettingsView`, `guild_tabs/*`) in the
//! user settings frame: permission-gated tabs, the community name over the nav, and Delete
//! community at its foot for the owner.

use reflux_model::{Ban, ChannelType, Emoji, Invite, PartialUser, Permissions as P, Role, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, Icon, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use super::modals::confirm;
use super::{Confirm, Dialog, Form, Then, error, field, hex, parse_hex};
use crate::icons;
use crate::media::{self, Load};
use crate::settings::tabs::{not_available, section};
use crate::settings::{colors, frame, nav_group, nav_item};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, RadioGroup, Select, Switch, over, popup_access};
use crate::i18n::{t, tf};

/// (id, category, label, icon, any of these permissions).
type TabDef = (&'static str, &'static str, &'static str, &'static Icon, P);

/// `GUILD_SETTINGS_TABS_DESCRIPTORS`. Explore needs the instance's discovery config, which Reflux
/// doesn't read, so it isn't listed.
const TABS: &[TabDef] = &[
    ("overview", "", "Overview", &icons::GEAR_FILL, P::MANAGE_GUILD),
    ("roles", "", "Roles", &icons::SHIELD_FILL, P::MANAGE_ROLES),
    ("moderation", "", "Moderation", &icons::HAMMER_FILL, P::MANAGE_GUILD),
    ("audit_log", "", "Activity log", &icons::BOOK_OPEN_FILL, P::VIEW_AUDIT_LOG),
    ("emoji", "Expressions", "Emoji", &icons::SMILEY_FILL, P::CREATE_EXPRESSIONS.union(P::MANAGE_EXPRESSIONS)),
    ("stickers", "Expressions", "Stickers", &icons::STICKER_FILL, P::CREATE_EXPRESSIONS.union(P::MANAGE_EXPRESSIONS)),
    ("vanity_url", "Community", "Vanity URL", &icons::LINK_BOLD, P::MANAGE_GUILD),
    ("explore", "Community", "Explore", &icons::COMPASS_FILL, P::MANAGE_GUILD),
    ("webhooks", "Integrations", "Webhooks", &icons::WEBHOOKS_LOGO_FILL, P::MANAGE_WEBHOOKS),
    ("members", "People", "Members", &icons::USER_FILL, P::MANAGE_GUILD),
    ("invites", "People", "Invites", &icons::TICKET_FILL, P::MANAGE_GUILD),
    ("bans", "People", "Bans", &icons::PROHIBIT_FILL, P::BAN_MEMBERS),
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
        "invites" => st.load("invites", format!("/guilds/{guild}/invites")),
        "bans" => st.load("bans", format!("/guilds/{guild}/bans")),
        "audit_log" => st.load("audit", format!("/guilds/{guild}/audit-logs?limit=50")),
        _ => crate::social::load_guild_tab(st, guild, tab),
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
        nav = nav.child(nav_item(&icons::TRASH_FILL, t("Delete community"), false, true, p, &c).on_press(move |s: &mut S| acc(s).form.confirm = Some(Confirm::DeleteGuild(guild))));
    }
    nav = nav.children(super::overview_extra::threshold_button(store, guild, p, acc));
    let label = TABS.iter().find(|t| t.0 == tab).map_or(t("Community settings"), |x| t(x.2));
    let body = match tab {
        "overview" => overview(store, st, guild, p, acc, &mut popups),
        "roles" => match role {
            Some(r) => role_editor(store, st, guild, r, p, acc),
            None => roles(store, st, guild, p, acc),
        },
        "emoji" => emoji(store, st, guild, p, acc),
        "stickers" => super::stickers::view(store, st, guild, p, acc),
        "invites" => invites(store, st, guild, p, acc),
        "bans" => bans(store, st, guild, p, acc),
        "audit_log" => audit_log(store, st, guild, p, acc, &mut popups),
        _ => crate::social::guild_tab(store, st, guild, tab, p, acc, &mut popups).unwrap_or_else(|| not_available(p)),
    };
    super::overview_extra::threshold_modal(st, guild, info, p, acc, &mut popups);
    let mut modal = frame(t("Community settings"), label, vec![head], nav, body, close, p);
    if tab == "audit_log" {
        // The window's height, for the activity log's scroll paging.
        modal = modal.on_layout(move |s: &mut S, [_, _, _, h]: [f32; 4]| {
            acc(s).form.data.insert("audit-win-h".into(), json!(h));
        });
    }
    if let Some(cf) = st.form.confirm {
        modal = modal.child(confirm(store, st, cf, p, acc));
    }
    layer.push(modal);
    layer.extend(popups);
    layer.extend(super::invite_preview::view(store, st, guild, info, p, acc));
    if let Some(c) = &st.nav.crop {
        layer.push(crate::settings::crop::modal(st, c, p, acc));
    }
}

fn select_tab(st: &mut ShellState, guild: Snowflake, tab: &'static str) {
    st.popups.close();
    // `handleDesktopTabSelect`: Members leaves settings for the community's members page.
    if tab == "members" {
        st.dialog = None;
        if st.guild != Some(guild) {
            st.select_guild(Some(guild), None);
        }
        return st.open_members_page(guild);
    }
    st.form = Form::default();
    st.dialog = Some(Dialog::Guild { guild, tab, role: None });
    load_tab(st, guild, tab);
}

// ---- Shared bits ----------------------------------------------------------------------------

pub(crate) fn small(t: impl Into<String>, p: &Palette) -> El {
    text(t.into()).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted)
}

pub(crate) fn title(t: impl Into<String>, p: &Palette) -> El {
    text(t.into()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).truncate()
}

/// A bordered card of rows split by hairlines (`AccountSecurityCard`-style lists).
pub(crate) fn card(rows: Vec<El>, p: &Palette) -> El {
    let n = rows.len();
    col().w_full().radius(12.0).border(1.0, p.background_modifier_accent).children(rows.into_iter().enumerate().map(move |(i, r)| {
        let r = row().w_full().px(16.0).py(12.0).gap(12.0).center_y().child(r);
        if i + 1 < n { r.child(el().pin(None, Some(0.0), Some(0.0), Some(0.0)).h(1.0).fill(p.background_modifier_accent)) } else { r }
    }))
}

pub(crate) fn empty(t: &str, p: &Palette) -> El {
    text(t.to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).py(24.0).w_full().align(rune::Align::Center)
}

type Typed = std::collections::HashMap<(String, std::any::TypeId), (Value, std::rc::Rc<dyn std::any::Any>)>;

thread_local! {
    /// The typed list last read from each `form.data` entry, with the JSON it came from; it's
    /// parsed again only once that JSON changes (the many writers all go through `form.data`).
    static TYPED: std::cell::RefCell<Typed> = Default::default();
}

/// A list loaded into `form.data[key]`, or a loading / failed line.
#[allow(clippy::result_large_err)] // the Err is the El shown instead, built once a frame
pub(crate) fn loaded<T: serde::de::DeserializeOwned + 'static>(st: &ShellState, key: &str, p: &Palette) -> Result<std::rc::Rc<T>, El> {
    let v = match st.form.data.get(key) {
        Some(v) => v,
        None if st.form.error.is_some() => return Err(error(st, p).unwrap_or_else(el)),
        None => return Err(empty(t("Loading…"), p)),
    };
    let slot = (key.to_string(), std::any::TypeId::of::<T>());
    let hit = TYPED.with_borrow(|m| m.get(&slot).filter(|(src, _)| src == v).and_then(|(_, t)| t.clone().downcast::<T>().ok()));
    if let Some(t) = hit {
        return Ok(t);
    }
    let typed = std::rc::Rc::new(serde_json::from_value::<T>(v.clone()).map_err(|e| empty(&format!("Couldn't read the list: {e}"), p))?);
    TYPED.with_borrow_mut(|m| m.insert(slot, (v.clone(), typed.clone())));
    Ok(typed)
}

fn user_name(u: &PartialUser) -> String {
    u.global_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| u.username.clone())
}

fn date(ts: &str) -> String {
    ts.get(..10).unwrap_or(ts).to_string()
}

/// The sticky "unsaved changes" bar with Reset and Save.
pub(crate) fn save_bar<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, save: impl Fn(&mut S) + 'static) -> El {
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
        .child(text(t("You have unsaved changes.")).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0))
        .child(
            Button::new(t("Reset"))
                .variant(ButtonVariant::Ghost)
                .size(ButtonSize::Small)
                .on_press(move |s: &mut S| {
                    let f = &mut acc(s).form;
                    (f.text, f.error) = (Default::default(), None);
                    f.data.retain(|k, _| matches!(k.as_str(), "members" | "invites" | "bans" | "audit" | "vanity" | "webhooks" | "discovery" | "categories"));
                })
                .el(p),
        )
        .child(Button::new(t("Save changes")).size(ButtonSize::Small).submitting(st.form.busy).key("save-changes").on_press(save).el(p));
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
pub(super) fn image_field<S: 'static>(st: &ShellState, label: &str, key: &'static str, current: Load, w: f32, h: f32, radius: f32, p: &Palette, acc: ShellAccess<S>) -> El {
    let state = picked(&st.form, key);
    // `current` is `Failed` when there's no image.
    let has = matches!(state, Some(Some(_))) || (state.is_none() && !matches!(current, Load::Failed));
    let preview = match (&state, current) {
        (Some(Some(_)), _) => row().size(w, h).center().radius(radius).fill(p.brand_primary).child(icon(&icons::IMAGE_FILL, 28.0).color(p.brand_primary_fill)),
        (None, Load::Ready(px)) => el().size(w, h).radius(radius).pixels_fit(&px, rune::SurfaceFit::Fill),
        _ => row().size(w, h).center().radius(radius).fill(p.background_tertiary).border(1.0, over(p.background_modifier_accent, p.background_tertiary)).child(icon(&icons::IMAGE_FILL, 28.0).color(p.text_tertiary)),
    };
    let mut buttons = row().gap(8.0).child(Button::<S>::new(format!("Change {}", label.to_lowercase())).variant(ButtonVariant::Secondary).size(ButtonSize::Small).el(p).on_press_tx(crate::settings::crop::pick(acc, key, crate::settings::crop::Asset::for_key(key))));
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
    // `BrandingSection`: icon, name, detached banner, banner, splashes.
    let mut branding = col().w_full().gap(20.0).child(image_field(st, t("Icon"), "icon", icon_load, 80.0, 80.0, 40.0, p, acc)).child(field(st, acc, "name", &info.name, t("Name"), t("My awesome community"), p));
    branding = branding.children(super::overview_extra::detached(st, info, p, acc));
    if info.features.iter().any(|f| f == "BANNER") {
        branding = branding.child(image_field(st, t("Banner"), "banner", banner_load, 240.0, 135.0, 8.0, p, acc));
    }
    branding = branding.children(super::overview_extra::splashes(st, g, info, p, acc));

    // Channel pickers keep their choice in `form.data` as a string ID ("" = none).
    let chosen = |key: &str, cur: Option<Snowflake>| f.data.get(key).and_then(Value::as_str).map_or_else(|| cur.map_or(String::new(), |c| c.to_string()), str::to_string);
    let channels = |kind: ChannelType| -> Vec<(String, String)> {
        let mut v: Vec<_> = store.channels.values().filter(|c| c.guild_id == Some(g) && c.kind == kind).map(|c| (c.position, c.id.to_string(), c.name.clone().unwrap_or_else(|| t("Unnamed channel").into()))).collect();
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
        .child(pick("afk", t("AFK / idle channel"), t("No AFK channel"), channels(ChannelType::GUILD_VOICE), afk.clone(), layer))
        .child(
            Select::new("afk_timeout", &AFK_TIMEOUTS, Some(&afk_timeout))
                .label(t("AFK timeout"))
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
        .child(pick("system", t("Destination channel"), t("No system channel"), texts, system.clone(), layer))
        .child(
            Switch::new(hide_joins)
                .label(t("Hide join messages"))
                .on_toggle(move |s: &mut S| {
                    acc(s).form.data.insert("hide_joins".into(), json!(!hide_joins));
                })
                .el(p),
        );
    let notif = f.data.get("notif").and_then(Value::as_str).map_or(info.default_message_notifications.to_string(), str::to_string);
    let notif_opts = [("0", "All messages", None), ("1", "Mentions only", None)];
    let notifications = col().w_full().gap(8.0).child(title(t("Default notification settings"), p)).child(
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
    super::overview_extra::patch(st, info, &mut patch);
    let mut body = col()
        .w_full()
        .child(section(t("Branding"), branding, true, p))
        .child(section(t("Idle settings"), idle, false, p))
        .child(section(t("System & welcome"), welcome, false, p))
        .child(section(t("Previewing"), super::overview_extra::previewing(store, g, info, p, acc), false, p))
        .child(section(t("Default notifications"), notifications, false, p))
        .child(super::overview_extra::advanced(st, info, p, acc));
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

pub(crate) fn sorted_roles(store: &Store, g: Snowflake) -> Vec<&Role> {
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
    let drag = super::role_extra::drag_state(st);
    let rows: Vec<El> = all
        .iter()
        .map(|r| {
            let id = r.id;
            let i = movable.iter().position(|&x| x == id);
            let name = if id == g { "@everyone".to_string() } else { r.name.clone() };
            let sub = if id == g { "Default permissions for all members".to_string() } else { format!("{} members", counts(id)) };
            let mut line = row().w_full().gap(12.0).center_y();
            if i.is_some() {
                line = line.child(icon(&icons::DOTS_SIX_VERTICAL_BOLD, 16.0).color(p.text_tertiary)).cursor(rune::Cursor::Grab);
            }
            line = line.child(color_dot(r.color, p)).child(col().grow(1.0).min_w(0.0).child(title(name, p)).child(small(sub, p)));
            if let Some(i) = i {
                // The landing spot gets a 2px line on the side the row will go.
                if let Some((from, to)) = drag.filter(|d| d.1 == i && d.0 != d.1) {
                    let mark = if to < from { el().pin(Some(0.0), Some(-11.0), Some(0.0), None) } else { el().pin(Some(0.0), None, Some(0.0), Some(-11.0)) };
                    line = line.child(mark.h(2.0).radius(1.0).fill(p.brand_primary_light).inert());
                }
                if drag.is_some_and(|d| d.0 == i) {
                    line = line.opacity(0.5);
                }
                line = super::role_extra::draggable(line, g, i, movable.clone(), 24.0, acc);
            }
            line.child(Button::new(t("Edit")).variant(ButtonVariant::Secondary).size(ButtonSize::Compact).on_press(move |s: &mut S| open_role(acc(s), g, id)).el(p))
        })
        .collect();
    let create = Button::new(t("Create role")).icon(&icons::PLUS_BOLD).size(ButtonSize::Small).key("create-role").on_press(move |s: &mut S| {
        acc(s).api(Method::POST, format!("/guilds/{g}/roles"), Some(json!({ "name": "new role" })), Then::Select);
    });
    let mut content = col().w_full().gap(16.0).child(row().w_full().justify_between().center_y().child(small("Members use the colour of their highest role. Drag roles to reorder them.", p).wrap().shrink()).child(create.el(p)));
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    section(t("Roles"), content.child(card(rows, p)), true, p)
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
        .child(text(t("Back to roles")).font(font(14.0, 500.0).lh(20.0)).color(p.text_link));
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
            .child(field(st, acc, "rname", &r.name, t("Role name"), "", p))
            .child(col().gap(8.0).child(title("Role colour", p)).child(row().gap(12.0).center_y().child(el().size(40.0, 40.0).radius(8.0).fill(if color == 0 { p.text_tertiary } else { crate::shell::role_color(color) })).child(col().w(160.0).child(field(st, acc, "color", &if r.color == 0 { String::new() } else { hex(r.color) }, "", "#000000", p))).child(swatches)))
            .child(super::role_extra::field(store, st, g, r, p, acc))
            .child(Switch::new(hoist).label("Display role members separately").on_toggle(set("hoist", json!(!hoist))).el(p))
            .child(Switch::new(mention).label("Allow anyone to @mention this role").on_toggle(set("mentionable", json!(!mention))).el(p));
    }
    let mut groups = col().w_full().gap(24.0);
    for (gt, entries) in super::perms::GUILD {
        let mut list = col().w_full().gap(4.0).child(text(crate::i18n::t(gt)).font(font(12.0, 600.0).lh(16.0)).text_case(rune::TextCase::Uppercase).color(p.text_tertiary).mb(4.0));
        for &(bit, name, d) in entries.iter() {
            let on = bits.contains(bit);
            let next = if on { bits - bit } else { bits | bit };
            list = list.child(Switch::new(on).label(crate::i18n::t(name)).description(crate::i18n::t(d)).on_toggle(set("perms", json!(next.bits()))).el(p));
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
    super::role_extra::patch(st, &mut patch);
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
        Button::new(t("Delete role")).variant(ButtonVariant::Danger).size(ButtonSize::Small).on_press(move |s: &mut S| acc(s).form.confirm = Some(Confirm::DeleteRole(g, rid))).el(p)
    });
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(header)
        .child(row().gap(8.0).center_y().child(color_dot(r.color, p)).child(text(if everyone { "@everyone".to_string() } else { r.name.clone() }).font(font(20.0, 600.0).lh(24.0)).color(p.text_primary)));
    if !everyone {
        body = body.child(section(t("Display"), display, true, p));
    }
    body = body.child(section(t("Permissions"), groups, everyone, p));
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

/// `filteredEmojis`: `matchSorter(emojis, searchQuery, {keys: ['name']})`.
fn emoji_search_matches(name: &str, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || name.to_lowercase().contains(&query)
}

#[allow(clippy::too_many_arguments)]
fn emoji_row<S: 'static>(st: &ShellState, e: &Emoji, g: Snowflake, manage: bool, me: Snowflake, editing: Option<Snowflake>, p: &Palette, acc: ShellAccess<S>) -> El {
    let id = e.id;
    let img = crate::shell::picture(st.media.asset(|b, _| media::emoji_url(b, &id.to_string())), 32.0, 32.0, 4.0, p);
    let by = e.user.as_ref().map(user_name).unwrap_or_default();
    let mine = manage || e.user.as_ref().is_some_and(|u| u.id == me);
    if editing == Some(id) {
        return row().w_full().gap(12.0).center_y().child(img).child(col().grow(1.0).min_w(0.0).child(field(st, acc, "emoji-rename", &e.name, "", "", p))).child(
            Button::<S>::new(t("Save")).size(ButtonSize::Small).key("emoji-save").el(p).on_press(move |s: &mut S| {
                let sh = acc(s);
                // `handleSave`: strip what the API refuses, then 2-32 characters.
                let name: String = sh.form.value("emoji-rename", "").chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                match name.len() {
                    0..2 => sh.form.error = Some(t("Emoji name must be at least 2 characters.").into()),
                    33.. => sh.form.error = Some(t("Emoji name must be at most 32 characters.").into()),
                    _ => {
                        sh.form.data.remove("emoji-edit");
                        sh.form.text.remove("emoji-rename");
                        sh.api(Method::PATCH, format!("/guilds/{g}/emojis/{id}"), Some(json!({ "name": name })), Then::Nothing);
                    }
                }
            }),
        ).child(Button::<S>::new(t("Cancel")).size(ButtonSize::Small).variant(ButtonVariant::Secondary).el(p).on_press(move |s: &mut S| {
            let f = &mut acc(s).form;
            f.data.remove("emoji-edit");
            f.text.remove("emoji-rename");
        }));
    }
    let mut name = title(format!(":{}:", e.name), p);
    if mine {
        name = name.cursor(rune::Cursor::Text).on_press(move |s: &mut S| {
            let f = &mut acc(s).form;
            f.data.insert("emoji-edit".into(), json!(id.to_string()));
            f.text.remove("emoji-rename");
        });
    }
    let mut line = row().w_full().gap(12.0).center_y().child(img).child(col().grow(1.0).min_w(0.0).child(name).child(small(by, p)));
    if mine {
        line = line.child(
            row()
                .size(28.0, 28.0)
                .center()
                .radius(6.0)
                .hover(p.background_modifier_hover)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Delete emoji"))
                .on_press(move |s: &mut S| acc(s).api(Method::DELETE, format!("/guilds/{g}/emojis/{id}"), None, Then::Nothing))
                .child(icon(&icons::X_BOLD, 16.0).color(p.text_primary_muted)),
        );
    }
    line
}

/// `GuildEmojiTab`: search by name, then the non-animated and animated sections.
fn emoji<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let list: Vec<&Emoji> = store.guilds.get(&g).map(|gs| gs.emojis.iter().collect()).unwrap_or_default();
    let manage = store.guild_permissions(g).contains(P::MANAGE_EXPRESSIONS);
    let me = store.my_id();
    let upload = Button::<S>::new(t("Upload emoji")).size(ButtonSize::Small).key("upload-emoji").el(p).on_press_tx(move |_: &mut S, tx: &mut Tx| {
        crate::file_picker::open_files(tx, "Upload emoji", Some((t("Images"), &["png", "jpg", "jpeg", "gif", "webp", "avif"])), acc, move |sh, r| match r {
            Ok(paths) if !paths.is_empty() => super::expr_upload::upload_emojis(sh, g, &paths),
            Ok(_) => {}
            Err(e) => sh.form.error = Some(e),
        });
    });
    let editing = st.form.data.get("emoji-edit").and_then(Value::as_str).and_then(|s| s.parse::<Snowflake>().ok());
    let q = st.form.value("emoji-search", "");
    let found: Vec<&Emoji> = list.iter().copied().filter(|e| emoji_search_matches(&e.name, &q)).collect();
    let rows = |items: &[&Emoji]| -> Vec<El> { items.iter().map(|e| emoji_row(st, e, g, manage, me, editing, p, acc)).collect() };
    let still: Vec<&Emoji> = found.iter().copied().filter(|e| !e.animated).collect();
    let animated: Vec<&Emoji> = found.iter().copied().filter(|e| e.animated).collect();
    let heading = |label: String, p: &Palette| text(label).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary);
    let mut groups = col().w_full().gap(24.0);
    if !still.is_empty() {
        groups = groups.child(
            col()
                .w_full()
                .gap(8.0)
                .child(heading(tf("Non-animated emoji ({emojiCount})", &[("emojiCount", &still.len().to_string())]), p))
                .child(card(rows(&still), p)),
        );
    }
    if !animated.is_empty() {
        groups = groups.child(
            col()
                .w_full()
                .gap(8.0)
                .child(heading(tf("Animated emoji ({emojiCount})", &[("emojiCount", &animated.len().to_string())]), p))
                .child(card(rows(&animated), p)),
        );
    }
    let mut content = col()
        .w_full()
        .gap(16.0)
        .child(row().w_full().center_y().gap(12.0).child(field(st, acc, "emoji-search", "", "", t("Search emojis"), p).w(280.0)).child(el().grow(1.0)).child(upload));
    if store.guild_permissions(g).intersects(P::MANAGE_EXPRESSIONS | P::CREATE_EXPRESSIONS) {
        content = content.child(small("Names come from the file name: letters, digits and underscores. PNG, JPEG, GIF, WebP or AVIF up to 512 KB.", p).wrap());
    }
    if store.guild_permissions(g).intersects(P::MANAGE_EXPRESSIONS | P::CREATE_EXPRESSIONS) {
        content = content.child(super::expr_upload::drop_zone(st, t("Drag and drop emoji files here"), true, p, acc, move |sh, files| super::expr_upload::upload_emojis(sh, g, files)));
    }
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    let failed = super::expr_upload::failures(st);
    if !failed.is_empty() {
        content = content.child(col().w_full().gap(4.0).child(title(t("Some emojis could not be added"), p)).children(failed.into_iter().map(|(n, e)| small(format!("{n}: {e}"), p).wrap())));
    }
    content = content.child(if found.is_empty() {
        empty(if q.is_empty() { "No emoji yet." } else { "No emojis found matching your search." }, p)
    } else {
        groups
    });
    section(&format!("Emoji — {}", list.len()), content, true, p)
}

// ---- Invites and bans -----------------------------------------------------------------------

fn invites<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let list: std::rc::Rc<Vec<Invite>> = match loaded(st, "invites", p) {
        Ok(l) => l,
        Err(e) => return section(t("Invites"), e, true, p),
    };
    let rows: Vec<El> = list
        .iter()
        .map(|i| {
            let code = i.code.clone();
            let by = i.inviter.as_ref().map(user_name).unwrap_or_else(|| t("Unknown").into());
            let ch = i.channel.as_ref().and_then(|c| c.name.clone()).or_else(|| i.channel_id.and_then(|c| store.channels.get(&c)?.name.clone())).unwrap_or_default();
            let uses = if i.max_uses == 0 { format!("{} uses", i.uses) } else { format!("{}/{} uses", i.uses, i.max_uses) };
            let expires = i.expires_at.as_deref().map_or("Never expires".to_string(), |t| format!("Expires {}", date(t)));
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(col().grow(1.0).min_w(0.0).child(title(code.clone(), p)).child(small(format!("#{ch} · by {by} · {uses} · {expires}"), p)))
                .child(Button::new(t("Revoke")).variant(ButtonVariant::Danger).size(ButtonSize::Compact).on_press(move |s: &mut S| {
                    acc(s).api(Method::DELETE, format!("/invites/{code}"), None, Then::Reload("invites".into(), format!("/guilds/{g}/invites")))
                }).el(p))
        })
        .collect();
    let mut content = col().w_full().gap(16.0).children(crate::social::pause_invites(store, g, p, acc));
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    section(t("Invites"), content.child(if rows.is_empty() { empty("No invites yet.", p) } else { card(rows, p) }), true, p)
}

fn bans<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let list: std::rc::Rc<Vec<Ban>> = match loaded(st, "bans", p) {
        Ok(l) => l,
        Err(e) => return section(t("Bans"), e, true, p),
    };
    let q = st.form.value("search", "").trim().to_lowercase();
    let rows: Vec<El> = list
        .iter()
        .filter(|b| q.is_empty() || b.user.username.to_lowercase().contains(&q))
        .map(|b| {
            let u = b.user.id;
            let ban = serde_json::to_value(b).unwrap_or_default();
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .cursor(rune::Cursor::Pointer)
                .key(format!("ban-{u}"))
                .on_press(move |s: &mut S| {
                    let sh = acc(s);
                    sh.open_social(crate::social::Modal::BanDetails { guild: g, user: u });
                    sh.form.data.insert("ban".into(), ban.clone());
                })
                .child(crate::shell::avatar(store, st, u, 32.0, None, p.background_secondary, p))
                .child(col().grow(1.0).min_w(0.0).child(title(user_name(&b.user), p)).child(small(b.reason.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| "No reason given".into()), p)))
                .child(Button::new("Unban").variant(ButtonVariant::Secondary).size(ButtonSize::Compact).on_press(move |s: &mut S| {
                    acc(s).api(Method::DELETE, format!("/guilds/{g}/bans/{u}"), None, Then::Reload("bans".into(), format!("/guilds/{g}/bans")))
                }).el(p))
        })
        .collect();
    let mut content = col().w_full().gap(16.0).child(field(st, acc, "search", "", "", t("Search bans"), p));
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    section(&format!("Bans — {}", list.len()), content.child(if rows.is_empty() { empty("No bans.", p) } else { card(rows, p) }), true, p)
}

// ---- Activity log ---------------------------------------------------------------------------

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct AuditLog {
    audit_log_entries: Vec<Value>,
    users: Vec<PartialUser>,
    webhooks: Vec<Value>,
}

/// `getActionIcon` and `getActionKind` (0 create, 1 update, 2 delete).
fn action_look(t: u64) -> (&'static Icon, u8) {
    let icon = match t {
        1 => &icons::GEAR_BOLD,
        10..=15 => &icons::HASH_BOLD,
        20..=28 => &icons::USER_GEAR_BOLD,
        30..=32 => &icons::TAG_BOLD,
        40..=42 => &icons::LINK_BOLD,
        50..=52 => &icons::PLUG_BOLD,
        60..=62 => &icons::SMILEY_BOLD,
        90..=92 => &icons::STAMP_BOLD,
        72 | 73 => &icons::TRASH_BOLD,
        74 | 75 => &icons::PENCIL_SIMPLE_BOLD,
        110..=112 => &icons::CHATS_BOLD,
        _ => &icons::BUILDINGS_BOLD,
    };
    let kind = match t {
        10 | 13 | 30 | 40 | 50 | 60 | 90 | 28 | 74 | 110 => 0,
        1 | 11 | 14 | 24 | 25 | 31 | 41 | 51 | 61 | 91 | 26 | 27 | 111 => 1,
        _ => 2,
    };
    (icon, kind)
}

/// The "Filter by action" options (`AuditLogConstants.ts`); threads only when threads are on.
const AUDIT_ACTIONS: [(&str, &str); 36] = [
    ("1", "Community updated"),
    ("10", "Channel created"),
    ("11", "Channel updated"),
    ("12", "Channel deleted"),
    ("13", "Permission override added"),
    ("14", "Permission override updated"),
    ("15", "Permission override removed"),
    ("20", "Member kicked"),
    ("22", "Member banned"),
    ("23", "Member unbanned"),
    ("24", "Member updated"),
    ("25", "Member roles updated"),
    ("26", "Member moved"),
    ("27", "Member disconnected"),
    ("28", "Bot added"),
    ("30", "Role created"),
    ("31", "Role updated"),
    ("32", "Role deleted"),
    ("40", "Invite created"),
    ("42", "Invite deleted"),
    ("50", "Webhook created"),
    ("51", "Webhook updated"),
    ("52", "Webhook deleted"),
    ("60", "Emoji created"),
    ("61", "Emoji updated"),
    ("62", "Emoji deleted"),
    ("90", "Sticker created"),
    ("91", "Sticker updated"),
    ("92", "Sticker deleted"),
    ("72", "Message deleted"),
    ("73", "Messages deleted"),
    ("74", "Message pinned"),
    ("75", "Message unpinned"),
    ("110", "Thread created"),
    ("111", "Thread updated"),
    ("112", "Thread deleted"),
];
const AUDIT_PAGE: usize = 50;

/// The audit log query for the form's filters, after `before` when paging.
fn audit_path(g: Snowflake, f: &Form, before: Option<Snowflake>) -> String {
    let mut q = format!("/guilds/{g}/audit-logs?limit={AUDIT_PAGE}");
    for (key, param) in [("audit-user", "user_id"), ("audit-action", "action_type")] {
        if let Some(v) = f.data.get(key).and_then(Value::as_str).filter(|v| !v.is_empty()) {
            q.push_str(&format!("&{param}={v}"));
        }
    }
    if let Some(b) = before {
        q.push_str(&format!("&before={b}"));
    }
    q
}

/// The sentence's links: `user:<id>` opens the full profile, `copy:<id>` copies the ID.
fn audit_links<S: 'static>(e: El, g: Snowflake, acc: ShellAccess<S>) -> El {
    e.on_link(move |s: &mut S, id: &str, tx: &mut rune::Tx| {
        let sh = acc(s);
        if let Some(u) = id.strip_prefix("user:").and_then(|u| u.parse().ok()) {
            sh.account.profile_modal = Some((Snowflake(u), Some(g), "overview"));
            sh.fetch_profile(Snowflake(u), Some(g));
        } else if let Some(c) = id.strip_prefix("copy:") {
            tx.copy(c.to_string());
            crate::desktop::toast(&mut sh.desktop, t("Copied to clipboard"), true);
        }
    })
}

fn audit_log<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let refilter = move |key: &'static str| {
        move |s: &mut S, v: &str| {
            let sh = acc(s);
            sh.form.data.insert(key.into(), json!(v));
            for k in ["audit", "audit-end", "audit-open", "audit-paging"] {
                sh.form.data.remove(k);
            }
            let path = audit_path(g, &sh.form, None);
            sh.load("audit", path);
        }
    };
    let cur = |k: &str| st.form.data.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    // "Filter by user": the community's known members (Fluxer lists the member store too).
    let mut users: Vec<(String, String)> = store.guilds.get(&g).map(|gs| gs.members.values().map(|m| (m.user.id.to_string(), m.nick.clone().unwrap_or_else(|| user_name(&m.user)))).collect()).unwrap_or_default();
    // `ensureMembersLoaded` for the log's actors and targets: they join the list.
    for u in st.form.data.get("audit").and_then(|a| a["users"].as_array()).into_iter().flatten() {
        if let Ok(u) = serde_json::from_value::<PartialUser>(u.clone())
            && !users.iter().any(|x| x.0 == u.id.to_string())
        {
            users.push((u.id.to_string(), user_name(&u)));
        }
    }
    users.sort_by_key(|u| u.1.to_lowercase());
    users.insert(0, (String::new(), t("All users").into()));
    let user_refs: Vec<(&str, &str)> = users.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let threads = store.guilds.get(&g).and_then(|gs| gs.info.as_ref()).is_some_and(|i| i.threads_active);
    let actions: Vec<(&str, &str)> = std::iter::once(("", t("All actions"))).chain(AUDIT_ACTIONS.iter().filter(|a| threads || a.0.len() < 3).map(|&(v, l)| (v, t(l)))).collect();
    let filters = row()
        .w_full()
        .gap(12.0)
        .child(Select::new("audit-user", &user_refs, Some(&cur("audit-user"))).label(t("Filter by user")).width(300.0).on_choose(refilter("audit-user")).el(&st.popups, pops, p, layer))
        .child(Select::new("audit-action", &actions, Some(&cur("audit-action"))).label(t("Filter by action")).width(300.0).on_choose(refilter("audit-action")).el(&st.popups, pops, p, layer));
    let head = col().w_full().gap(16.0).child(small(t("Track moderator actions across the community."), p)).child(filters);
    let log: std::rc::Rc<AuditLog> = match loaded(st, "audit", p) {
        Ok(l) => l,
        Err(e) => return section(t("Activity log"), head.child(e), true, p),
    };
    let user = |id: &str| {
        let id: Snowflake = id.parse().ok()?;
        let me = store.me.as_ref().filter(|m| m.id == id).map(|m| &m.base);
        log.users.iter().find(|u| u.id == id).or(store.users.get(&id)).or(me).map(user_name)
    };
    let chan = |id: &str| {
        let c = store.channels.get(&id.parse().ok()?)?;
        Some((c.name.clone()?, matches!(c.kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT | ChannelType::GUILD_LINK)))
    };
    let role = |id: &str| store.guilds.get(&g)?.roles.iter().find(|r| r.id.to_string() == id).map(|r| r.name.clone());
    // `Emoji.getEmojiById`: any community's emoji this client has.
    let emoji = |id: &str| {
        let e = store.guilds.values().flat_map(|gd| gd.emojis.iter()).find(|e| e.id.to_string() == id)?;
        let animate = e.animated && store.settings.animate_emoji && !crate::look::reduced_motion();
        let load = st.media.asset(|b, _| if animate { crate::media::emoji_url_animated(b, id) } else { crate::media::emoji_url(b, id) });
        Some(match load {
            crate::media::Load::Ready(px) => Some(px),
            _ => None,
        })
    };
    let names = super::audit::Names { user: &user, channel: &chan, role: &role, guild: g, emoji: &emoji };
    let known = |kind: &str, id: &str| match kind {
        "webhook" => log.webhooks.iter().find(|w| w["id"] == id).and_then(|w| w["name"].as_str()).map(str::to_string),
        "emoji" => store.guilds.get(&g)?.emojis.iter().find(|e| e.id.to_string() == id).map(|e| e.name.clone()),
        "sticker" => store.guilds.get(&g)?.stickers.iter().find(|e| e.id.to_string() == id).map(|e| e.name.clone()),
        _ => None,
    };
    let c = colors(p);
    let rows: Vec<El> = log
        .audit_log_entries
        .iter()
        .map(|v| {
            let e = super::audit::E { v, guild: g, known: &known };
            let pres = super::audit::present(&e);
            let id = v["id"].as_str().unwrap_or_default().to_string();
            let open = cur("audit-open") == id;
            let expandable = !pres.rows.is_empty() || !pres.blocks.is_empty();
            let (glyph, kind) = action_look(v["action_type"].as_u64().unwrap_or(0));
            let tint = [p.status_online, p.status_warning, p.status_danger][kind as usize];
            let badge = row()
                .size(32.0, 32.0)
                .center()
                .radius(16.0)
                .fill(crate::ui::mix(tint, c.hover, 8.0))
                .child(icon(glyph, 20.0).color(crate::ui::mix(tint, p.text_primary_muted, 70.0)));
            let actor: Option<Snowflake> = v["user_id"].as_str().and_then(|u| u.parse().ok());
            let face = match actor {
                Some(a) => crate::shell::avatar(store, st, a, 32.0, None, p.background_secondary, p),
                None => row().size(32.0, 32.0).radius(16.0).fill(c.hover),
            };
            let when = id.parse().map(|i| crate::shell::format_relative(Snowflake(i).timestamp_ms(), st.now_ms)).unwrap_or_default();
            let words = col().grow(1.0).min_w(0.0).gap(2.0).child(super::audit::sentence(&pres.summary, &names, 14.0, p.text_primary_muted, p, &format!("audit-{id}"), |e| audit_links(e, g, acc))).child(small(when, p));
            let mut header = row().w_full().min_h(56.0).px(16.0).py(8.0).gap(12.0).center_y().child(badge).child(face).child(words);
            if expandable {
                let id = id.clone();
                header = header
                    .child(icon(if open { &icons::CARET_UP_BOLD } else { &icons::CARET_DOWN_BOLD }, 20.0).color(p.text_tertiary))
                    .hover(c.hover)
                    .cursor(rune::Cursor::Pointer)
                    .role("button")
                    .expanded(open)
                    .on_press(move |s: &mut S| {
                        let f = &mut acc(s).form.data;
                        if open { f.remove("audit-open") } else { f.insert("audit-open".into(), json!(id)) };
                    });
                if open {
                    header = header.fill(c.hover);
                }
            }
            let mut entry = col().w_full().radius(12.0).clip_rounded(12.0).border(1.0, p.background_modifier_accent).fill(p.background_textarea).child(header);
            if open {
                let mut details = col().w_full().px(16.0).py(12.0).gap(12.0).child(el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(1.0).fill(p.background_modifier_accent));
                for (ban, text_) in &pres.blocks {
                    let label = if *ban { t("Ban reason") } else { t("Reason") };
                    details = details.child(
                        col().w_full().gap(4.0).child(text(label).font(font(11.0, 600.0).lh(14.0)).text_case(rune::TextCase::Uppercase).color(p.text_primary_muted)).child(text(text_.clone()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).wrap()),
                    );
                }
                if !pres.rows.is_empty() {
                    details = details.child(col().w_full().gap(8.0).children(pres.rows.iter().enumerate().map(|(ri, r)| {
                        use super::audit::Tone;
                        let (glyph, tint) = match r.tone {
                            Tone::Add => (&icons::PLUS_BOLD, Some(p.status_online)),
                            Tone::Remove => (&icons::MINUS_BOLD, Some(p.status_danger)),
                            Tone::Neutral => (&icons::DOT_BOLD, None),
                        };
                        let (bg, border, fg) = match tint {
                            Some(k) => (crate::ui::mix(k, c.hover, 18.0), crate::ui::mix(k, p.background_modifier_accent, 55.0), k),
                            None => (c.hover, p.background_modifier_accent, p.text_primary),
                        };
                        let bullet = row().size(20.0, 20.0).center().radius(10.0).fill(bg).border(1.0, over(border, bg)).child(icon(glyph, 12.0).color(fg));
                        row().w_full().min_h(28.0).gap(8.0).center_y().child(bullet).child(super::audit::sentence(&r.s, &names, 14.0, p.text_primary_muted, p, &format!("audit-{id}-r{ri}"), |e| audit_links(e, g, acc)).grow(1.0).min_w(0.0))
                    })));
                }
                entry = entry.child(details);
            }
            entry
        })
        .collect();
    let mut body = head.child(if rows.is_empty() { empty(t("No logs yet"), p) } else { col().w_full().gap(8.0).children(rows) });
    let last = log.audit_log_entries.last().and_then(|e| e["id"].as_str()).map(str::to_string);
    if let Some(last) = last.filter(|_| !st.form.data.contains_key("audit-end") && log.audit_log_entries.len() >= AUDIT_PAGE) {
        // `INFINITE_SCROLL_OVERSCAN_PX`: the next page loads once this sentinel comes within
        // 1200px of the bottom of the window (the modal's height, recorded as it lays out).
        let loading = st.form.data.get("audit-paging").and_then(Value::as_str) == Some(last.as_str());
        let sentinel = col().w_full().key("audit-sentinel").on_layout(move |s: &mut S, [_, y, _, _]: [f32; 4]| {
            let sh = acc(s);
            let win = sh.form.data.get("audit-win-h").and_then(Value::as_f64).unwrap_or(800.0) as f32;
            if y > win + 1200.0 || sh.form.data.get("audit-paging").and_then(Value::as_str) == Some(last.as_str()) {
                return;
            }
            sh.form.data.insert("audit-paging".into(), json!(last));
            let path = audit_path(g, &sh.form, last.parse().ok().map(Snowflake));
            sh.api(Method::GET, path, None, Then::Append("audit".into(), AUDIT_PAGE));
        });
        body = body.child(if loading { sentinel.h(48.0).center().child(crate::ui::button::spinner(p.text_primary_muted)) } else { sentinel.h(1.0) });
    }
    section(t("Activity log"), body, true, p)
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
    fn emoji_search_matches_names() {
        assert!(emoji_search_matches("PartyParrot", "parrot"));
        assert!(emoji_search_matches("party_parrot", "PAR"));
        assert!(emoji_search_matches("party_parrot", ""));
        assert!(!emoji_search_matches("party_parrot", "cat"));
    }

    #[test]
    fn audit_query() {
        let mut f = Form::default();
        assert_eq!(audit_path(Snowflake(1), &f, None), "/guilds/1/audit-logs?limit=50");
        f.data.insert("audit-user".into(), json!("7"));
        f.data.insert("audit-action".into(), json!(""));
        assert_eq!(audit_path(Snowflake(1), &f, Some(Snowflake(9))), "/guilds/1/audit-logs?limit=50&user_id=7&before=9");
    }
}
