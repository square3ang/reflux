//! Channel settings (`ChannelSettingsModal`, `channel_tabs/ChannelOverviewTab`,
//! `ChannelPermissionsTab`) in the user settings frame, with Delete channel at the nav's foot.

use reflux_model::{ChannelType, OverwriteType, PermissionOverwrite, Permissions as P, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, Icon, col, el, icon, row, text};
use serde_json::{Value, json};

use super::modals::confirm;
use super::settings::{card, color_dot, empty, save_bar, small, sorted_roles, title};
use super::{Confirm, Dialog, Form, Then, error, field, perms, textarea};
use crate::icons;
use crate::settings::tabs::{not_available, section};
use crate::settings::{colors, frame, nav_group, nav_item};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Select, Slider, mix, popup_access};
use crate::i18n::t;

/// (id, label, icon, permission needed).
const TABS: &[(&str, &str, &Icon, P)] = &[
    ("overview", "Overview", &icons::GEAR, P::MANAGE_CHANNELS),
    ("permissions", "Permissions", &icons::SHIELD, P::MANAGE_ROLES),
    ("invites", "Invites", &icons::TICKET, P::MANAGE_CHANNELS),
    ("webhooks", "Webhooks", &icons::WEBHOOKS_LOGO, P::MANAGE_WEBHOOKS),
];

/// The first tab you may open on a channel, if any.
pub fn first_tab(store: &Store, channel: Snowflake) -> Option<&'static str> {
    let have = store.permissions(channel);
    TABS.iter().find(|t| have.contains(t.3)).map(|t| t.0)
}

/// Fetches what a tab lists: invites and webhooks, or a voice channel's regions.
pub(super) fn load_tab(st: &mut ShellState, channel: Snowflake, tab: &str, voice: bool) {
    if tab == "overview" && voice {
        st.load("regions", format!("/channels/{channel}/rtc-regions"));
    }
    crate::social::load_channel_tab(st, channel, tab);
}

/// `VOICE_CHANNEL_BITRATE_*` in kbps: min, default, and the max a community's features allow
/// (`getMaxVoiceChannelBitrate`).
const BITRATE_MIN: u32 = 8;
const BITRATE_DEFAULT: u32 = 64;
pub(crate) fn max_bitrate(features: &[String]) -> u32 {
    features.iter().fold(96, |m, f| match f.as_str() {
        "AUDIO_BITRATE_384_KBPS" => m.max(384),
        "AUDIO_BITRATE_256_KBPS" => m.max(256),
        "AUDIO_BITRATE_128_KBPS" => m.max(128),
        _ => m,
    })
}

/// `MAX_TOPIC_LENGTH`.
const TOPIC_MAX: usize = 1024;

#[allow(clippy::too_many_arguments)]
pub fn view<S: 'static>(store: &Store, st: &ShellState, channel: Snowflake, tab: &'static str, target: Option<Snowflake>, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some(ch) = store.channels.get(&channel) else { return };
    let c = colors(p);
    let mut popups = Vec::new();
    let close = move |s: &mut S| acc(s).dialog = None;
    let have = store.permissions(channel);
    let name = ch.name.clone().unwrap_or_default();
    let voice = ch.kind == ChannelType::GUILD_VOICE;
    let head = row()
        .w_full()
        .pt(6.0)
        .pb(6.0)
        .pl(10.0)
        .pr(4.0)
        .mb(24.0)
        .gap(6.0)
        .center_y()
        .child(icon(crate::shell::channel_icon(ch.kind), 16.0).color(p.text_primary_muted))
        .child(text(name).font(font(17.07, 500.0).lh(22.0)).color(p.text_primary).truncate().grow(1.0).shrink());
    let items: Vec<El> = TABS
        .iter()
        .filter(|t| have.contains(t.3))
        .map(|&(id, label, glyph, _)| {
            nav_item(glyph, label, tab == id, false, p, &c).on_press(move |s: &mut S| {
                let sh = acc(s);
                sh.popups.close();
                sh.form = Form::default();
                sh.dialog = Some(Dialog::Channel { channel, tab: id, target: None });
                load_tab(sh, channel, id, voice);
            })
        })
        .collect();
    let mut nav = col().w_full().gap(3.0).child(nav_group(None, items, p));
    if have.contains(P::MANAGE_CHANNELS) {
        let label = if ch.kind == ChannelType::GUILD_CATEGORY { t("Delete category") } else { t("Delete channel") };
        nav = nav.child(nav_item(&icons::TRASH, label, false, true, p, &c).on_press(move |s: &mut S| acc(s).form.confirm = Some(Confirm::DeleteChannel(channel))));
    }
    let label = TABS.iter().find(|t| t.0 == tab).map_or(t("Channel settings"), |x| t(x.1));
    let body = match tab {
        "overview" => overview(store, st, channel, p, acc, &mut popups),
        "permissions" => match target {
            Some(t) => overwrite_editor(store, st, channel, t, p, acc),
            None => overwrites(store, st, channel, p, acc, &mut popups),
        },
        _ => crate::social::channel_tab(store, st, channel, tab, p, acc).unwrap_or_else(|| not_available(p)),
    };
    let mut modal = frame(t("Channel settings"), label, vec![head], nav, body, close, p);
    if let Some(cf) = st.form.confirm {
        modal = modal.child(confirm(store, st, cf, p, acc));
    }
    if let Some(m) = super::channel_opts::tag_modal(st, ch, p, acc) {
        modal = modal.child(m);
    }
    layer.push(modal);
    layer.extend(popups);
}

// ---- Overview -------------------------------------------------------------------------------

fn overview<S: 'static>(store: &Store, st: &ShellState, id: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some(ch) = store.channels.get(&id) else { return not_available(p) };
    let f = &st.form;
    let cur_name = ch.name.clone().unwrap_or_default();
    let cur_topic = ch.topic.clone().unwrap_or_default();
    let texty = matches!(ch.kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT);
    let category = ch.kind == ChannelType::GUILD_CATEGORY;
    let (name_label, name_hint) = if category { (t("Category name"), t("My category")) } else { (t("Channel name"), "general") };
    let mut content = col().w_full().gap(20.0).child(field(st, acc, "name", &cur_name, name_label, name_hint, p));
    let mut patch = serde_json::Map::new();
    let name = f.value("name", &cur_name);
    if name.trim() != cur_name && !name.trim().is_empty() {
        patch.insert("name".into(), json!(name.trim()));
    }
    if ch.kind == ChannelType::GUILD_LINK {
        let cur_url = ch.url.clone().unwrap_or_default();
        content = content.child(field(st, acc, "url", &cur_url, t("URL"), "https://example.com", p));
        let url = f.value("url", &cur_url);
        if url.trim() != cur_url && !url.trim().is_empty() {
            patch.insert("url".into(), json!(url.trim()));
        }
    }
    if texty {
        let topic = f.value("topic", &cur_topic);
        let over_limit = topic.chars().count() > TOPIC_MAX;
        let count = text(format!("{}/{TOPIC_MAX}", topic.chars().count())).font(font(12.0, 400.0).lh(16.0)).color(if over_limit { mix(p.status_danger, p.text_primary, 78.0) } else { p.text_tertiary });
        content = content.child(
            col()
                .w_full()
                .gap(4.0)
                .child(row().w_full().justify_between().child(text(t("Topic")).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary)).child(count))
                .child(textarea(st, acc, "topic", &cur_topic, "", t("Add a topic to this channel"), 88.0, p)),
        );
        if topic.trim() != cur_topic && !over_limit {
            patch.insert("topic".into(), if topic.trim().is_empty() { Value::Null } else { json!(topic.trim()) });
        }
        content = content.child(super::channel_opts::slowmode(st, acc, "slowmode", "rate_limit_per_user", t("Slowmode"), ch.rate_limit_per_user, &mut patch, p));
        let threads = ch.guild_id.and_then(|g| store.guilds.get(&g)?.info.as_ref().map(|i| i.threads_active)).unwrap_or(false);
        if threads {
            content = content.child(super::channel_opts::thread_defaults(st, ch, p, acc, &mut patch));
        }
    }
    if reflux_store::threads::is_forum(ch.kind) {
        content = super::forum::fields(st, ch, p, acc, layer, content, &mut patch);
    } else if texty || category || matches!(ch.kind, ChannelType::GUILD_VOICE | ChannelType::GUILD_LINK) {
        content = content.child(super::channel_opts::mature(st, ch, p, acc, layer, &mut patch));
    }
    let mut body = col().w_full().child(section(t("Overview"), content, true, p));
    if ch.kind == ChannelType::GUILD_VOICE {
        body = body.child(section(t("Voice"), voice_settings(store, st, ch, p, acc, layer, &mut patch), false, p));
    }
    if !patch.is_empty() {
        let patch = Value::Object(patch);
        body = body.child(save_bar(st, p, acc, move |s: &mut S| {
            let sh = acc(s);
            sh.api(Method::PATCH, format!("/channels/{id}"), Some(patch.clone()), Then::Nothing);
            sh.form.text.clear();
            sh.form.data.retain(|k, _| k == "regions");
        }));
    } else if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    body
}

/// `VoiceSettings` + `RtcRegionSelect`: voice quality, participant limit and the voice region.
fn voice_settings<S: 'static>(store: &Store, st: &ShellState, ch: &reflux_model::Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>, patch: &mut serde_json::Map<String, Value>) -> El {
    let f = &st.form;
    let features = ch.guild_id.and_then(|g| store.guilds.get(&g)?.info.as_ref().map(|i| i.features.clone())).unwrap_or_default();
    let max = max_bitrate(&features);
    let cur_kbps = ch.bitrate.map_or(BITRATE_DEFAULT, |b| (b + 500) / 1000).min(max);
    let kbps = f.data.get("bitrate").and_then(Value::as_u64).map_or(cur_kbps, |v| v as u32);
    let cur_limit = ch.user_limit.unwrap_or(0);
    let limit = f.data.get("user_limit").and_then(Value::as_u64).map_or(cur_limit, |v| v as u32);
    let have = store.permissions(ch.id);
    let set = move |k: &'static str| move |s: &mut S, v: f32| {
        acc(s).form.data.insert(k.into(), json!(v.round() as u32));
    };
    let control = |label: &str, value: String, slider: El| {
        col().w_full().gap(8.0).child(row().w_full().justify_between().child(title(label, p)).child(small(value, p))).child(slider)
    };
    let mut out = col().w_full().gap(20.0);
    if have.contains(P::MANAGE_CHANNELS) {
        out = out
            .child(control(t("Voice quality"), crate::i18n::tf("{kilobits} kbps", &[("kilobits", &kbps.to_string())]), Slider::new("voice-bitrate", kbps as f32, BITRATE_MIN as f32, max as f32).on_change(set("bitrate")).el(p)))
            .child(control(
                t("Participant limit"),
                crate::i18n::tp("{count, plural, =0 {∞ No limit} one {# participant} other {# participants}}", limit as i64, &[]),
                Slider::new("voice-user-limit", limit as f32, 0.0, 99.0).on_change(set("user_limit")).el(p),
            ));
        if kbps != cur_kbps {
            patch.insert("bitrate".into(), json!(kbps * 1000));
        }
        if limit != cur_limit {
            patch.insert("user_limit".into(), json!(limit));
        }
    }
    if have.intersects(P::UPDATE_RTC_REGION | P::MANAGE_CHANNELS) {
        let regions: Vec<reflux_model::RtcRegion> = f.data.get("regions").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        let mut opts: Vec<(String, String)> = regions.iter().filter(|r| r.id != "automatic").map(|r| (r.id.clone(), r.name.clone())).collect();
        opts.sort_by(|a, b| a.1.cmp(&b.1));
        opts.insert(0, (String::new(), t("Automatic").to_string()));
        let cur = ch.rtc_region.clone().filter(|r| regions.is_empty() || regions.iter().any(|x| &x.id == r)).unwrap_or_default();
        let value = f.data.get("rtc_region").and_then(Value::as_str).map_or(cur.clone(), str::to_string);
        let refs: Vec<(&str, &str)> = opts.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
        out = out.child(
            Select::new("rtc_region", &refs, Some(&value))
                .label(t("Voice region"))
                .width(340.0)
                .on_choose(move |s: &mut S, v| {
                    acc(s).form.data.insert("rtc_region".into(), json!(v));
                })
                .el(&st.popups, pops, p, layer),
        );
        if value != cur {
            patch.insert("rtc_region".into(), if value.is_empty() { Value::Null } else { json!(value) });
        }
    }
    out
}

// ---- Permissions ----------------------------------------------------------------------------

/// Whose overwrite `id` is: a role of the channel's guild, else a member.
fn kind_of(store: &Store, guild: Snowflake, id: Snowflake) -> OverwriteType {
    let role = id == guild || store.guilds.get(&guild).is_some_and(|g| g.roles.iter().any(|r| r.id == id));
    if role { OverwriteType::ROLE } else { OverwriteType::MEMBER }
}

fn subject_name(store: &Store, guild: Snowflake, id: Snowflake) -> String {
    if id == guild {
        return "@everyone".into();
    }
    if let Some(r) = store.guilds.get(&guild).and_then(|g| g.roles.iter().find(|r| r.id == id)) {
        return r.name.clone();
    }
    store.users.get(&id).map_or_else(|| id.to_string(), |u| u.global_name.clone().unwrap_or_else(|| u.username.clone()))
}

fn edit_target(st: &mut ShellState, channel: Snowflake, target: Option<Snowflake>) {
    st.form = Form::default();
    st.dialog = Some(Dialog::Channel { channel, tab: "permissions", target });
}

fn overwrites<S: 'static>(store: &Store, st: &ShellState, id: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some(ch) = store.channels.get(&id) else { return not_available(p) };
    let Some(g) = ch.guild_id else { return not_available(p) };
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let roles = sorted_roles(store, g);
    let mut list: Vec<&PermissionOverwrite> = ch.permission_overwrites.iter().collect();
    // Roles first in rank order, then members.
    list.sort_by_key(|o| (o.kind != OverwriteType::ROLE, roles.iter().position(|r| r.id == o.id).unwrap_or(usize::MAX)));
    let rows: Vec<El> = list
        .iter()
        .map(|o| {
            let t = o.id;
            let lead = if o.kind == OverwriteType::ROLE {
                color_dot(roles.iter().find(|r| r.id == t).map_or(0, |r| r.color), p)
            } else {
                crate::shell::avatar(store, st, t, 24.0, None, p.background_secondary, p)
            };
            let sub = format!("{} allowed · {} denied", o.allow.bits().count_ones(), o.deny.bits().count_ones());
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(lead)
                .child(col().grow(1.0).min_w(0.0).child(title(subject_name(store, g, t), p)).child(small(sub, p)))
                .child(Button::new(crate::i18n::t("Edit")).variant(ButtonVariant::Secondary).size(ButtonSize::Compact).on_press(move |s: &mut S| edit_target(acc(s), id, Some(t))).el(p))
        })
        .collect();
    // Roles, and members the client knows or the search found, without an overwrite yet.
    let q = st.form.value("ow-search", "").trim().to_lowercase();
    let mut add: Vec<(String, String)> = roles.iter().filter(|r| !list.iter().any(|o| o.id == r.id)).map(|r| (r.id.to_string(), if r.id == g { "@everyone".into() } else { format!("@{}", r.name) })).collect();
    let mut members: Vec<(String, String)> = store.guilds.get(&g).map(|gs| gs.members.keys().map(|&u| (u.to_string(), subject_name(store, g, u))).collect()).unwrap_or_default();
    members.extend(search_hits(st));
    members.retain(|m| !list.iter().any(|o| o.id.to_string() == m.0));
    members.sort_by_key(|m| m.1.to_lowercase());
    members.dedup_by(|a, b| a.0 == b.0);
    add.extend(members);
    add.retain(|a| q.is_empty() || a.1.to_lowercase().contains(&q));
    let refs: Vec<(&str, &str)> = add.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let picker = Select::new("add-overwrite", &refs, None)
        .placeholder("Add a role or member")
        .width(280.0)
        .on_choose(move |s: &mut S, v| {
            if let Ok(t) = v.parse::<Snowflake>() {
                // Edits start from an empty overwrite; saving creates it.
                edit_target(acc(s), id, Some(t));
            }
        })
        .el(&st.popups, pops, p, layer);
    let mut content = col()
        .w_full()
        .gap(16.0)
        .children(ch.parent_id.and_then(|c| store.channels.get(&c)).filter(|c| c.kind == ChannelType::GUILD_CATEGORY).map(|parent| sync_banner(store, ch, parent, p, acc)))
        .child(row().w_full().gap(12.0).justify_between().center_y().child(small("Overwrites change what roles and members can do in this channel.", p).wrap().shrink()))
        .child(row().w_full().gap(12.0).center_y().child(el().grow(1.0).child(super::field(st, acc, "ow-search", "", "", t("Search roles or members…"), p))).child(picker));
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    section(t("Permissions"), content.child(if rows.is_empty() { empty("No overwrites yet.", p) } else { card(rows, p) }), true, p)
}

/// The overwrites as comparable rows (`isSyncedWithParent` compares id, type, allow, deny).
fn overwrite_set(c: &reflux_model::Channel) -> Vec<(Snowflake, u8, u64, u64)> {
    let mut v: Vec<_> = c.permission_overwrites.iter().map(|o| (o.id, o.kind.0, o.allow.bits(), o.deny.bits())).collect();
    v.sort_unstable();
    v
}

/// `SyncWithParentBanner`: whether the channel matches its category, and a Sync button if not.
fn sync_banner<S: 'static>(store: &Store, ch: &reflux_model::Channel, parent: &reflux_model::Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let synced = overwrite_set(ch) == overwrite_set(parent);
    let name = parent.name.clone().unwrap_or_default();
    let line = if synced { crate::i18n::tf_plain("This channel is synced with the parent category <0>{0}</0>.", &[("0", &name)]) } else { crate::i18n::tf_plain("This channel is not synced with the parent category <0>{0}</0>.", &[("0", &name)]) };
    let mut b = row()
        .w_full()
        .gap(8.0)
        .px(12.0)
        .py(8.0)
        .center_y()
        .justify_between()
        .radius(6.0)
        .border(1.0, p.background_modifier_accent)
        .child(text(line).font(font(13.0, 400.0).lh(18.0)).color(if synced { p.status_online } else { p.status_idle }).wrap().shrink());
    if !synced {
        let id = ch.id;
        let body = json!({ "permission_overwrites": parent.permission_overwrites.iter().map(|o| json!({ "id": o.id.to_string(), "type": o.kind.0, "allow": o.allow.bits().to_string(), "deny": o.deny.bits().to_string() })).collect::<Vec<_>>() });
        let can = ch.guild_id.is_some_and(|g| store.guild_permissions(g).contains(P::MANAGE_CHANNELS | P::MANAGE_ROLES));
        b = b.child(Button::new(t("Sync with category")).size(ButtonSize::Small).disabled(!can).on_press(move |s: &mut S| {
            acc(s).api(Method::PATCH, format!("/channels/{id}"), Some(body.clone()), Then::Toast("Channel synced with parent category", "Failed to update channel permissions"));
        }).el(p));
    }
    b
}

/// The members `members-search` found for the add-override search (`form.data["ow-hits"]`).
pub(super) fn search_hits(st: &ShellState) -> Vec<(String, String)> {
    let hits = st.form.data.get("ow-hits").and_then(|v| v.get("members")).and_then(Value::as_array);
    hits.into_iter()
        .flatten()
        .filter_map(|m| {
            let name = ["nickname", "global_name", "username"].iter().find_map(|k| m[*k].as_str().filter(|n| !n.is_empty()))?;
            Some((m["user_id"].as_str()?.to_string(), name.to_string()))
        })
        .collect()
}

/// `AddOverridePopout` / `ChannelCreateAccessStep`: once the search text rests 300 ms, asks the
/// server for up to 25 matching members.
pub fn poll(store: &Store, st: &mut ShellState, tx: &mut rune::Tx) -> bool {
    let (g, key) = match st.dialog {
        Some(Dialog::Channel { channel, tab: "permissions", target: None }) => (store.channels.get(&channel).and_then(|c| c.guild_id), "ow-search"),
        Some(Dialog::CreateChannel { guild, .. }) => (Some(guild), "access-search"),
        _ => return false,
    };
    let Some(g) = g else { return false };
    let q = st.form.value(key, "").trim().to_string();
    let now = crate::app::now_ms();
    let f = &mut st.form.data;
    if f.get("ow-seen").and_then(Value::as_str) != Some(&q) {
        f.insert("ow-seen".into(), json!(q));
        f.insert("ow-at".into(), json!(now));
    }
    if q.is_empty() || f.get("ow-q").and_then(Value::as_str) == Some(&q) {
        return false;
    }
    let at = f.get("ow-at").and_then(Value::as_u64).unwrap_or(0);
    if now < at + 300 {
        tx.wake_after(std::time::Duration::from_millis(at + 300 - now));
        return false;
    }
    f.insert("ow-q".into(), json!(q));
    st.api(Method::POST, format!("/guilds/{g}/members-search"), Some(json!({ "query": q, "limit": 25 })), Then::Keep("ow-hits".into()));
    true
}

/// Deny / inherit / allow for one bit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tri {
    Deny,
    Inherit,
    Allow,
}

pub fn tri(allow: P, deny: P, bit: P) -> Tri {
    if allow.contains(bit) {
        Tri::Allow
    } else if deny.contains(bit) {
        Tri::Deny
    } else {
        Tri::Inherit
    }
}

pub fn set_tri(allow: P, deny: P, bit: P, t: Tri) -> (P, P) {
    let (a, d) = (allow - bit, deny - bit);
    match t {
        Tri::Allow => (a | bit, d),
        Tri::Deny => (a, d | bit),
        Tri::Inherit => (a, d),
    }
}

fn overwrite_editor<S: 'static>(store: &Store, st: &ShellState, id: Snowflake, t: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let Some(ch) = store.channels.get(&id) else { return not_available(p) };
    let Some(g) = ch.guild_id else { return not_available(p) };
    let existing = ch.permission_overwrites.iter().find(|o| o.id == t);
    let (cur_allow, cur_deny) = existing.map_or((P::empty(), P::empty()), |o| (o.allow, o.deny));
    let f = &st.form;
    let allow = f.data.get("allow").and_then(Value::as_u64).map_or(cur_allow, P::from_bits_retain);
    let deny = f.data.get("deny").and_then(Value::as_u64).map_or(cur_deny, P::from_bits_retain);
    let kind = existing.map_or_else(|| kind_of(store, g, t), |o| o.kind);
    let back = row()
        .gap(6.0)
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .role("link")
        .on_press(move |s: &mut S| edit_target(acc(s), id, None))
        .child(icon(&icons::ARROW_LEFT_BOLD, 16.0).color(p.text_link))
        .child(text("Back to permissions").font(font(14.0, 500.0).lh(20.0)).color(p.text_link));
    let mut header = row().w_full().justify_between().center_y().child(back);
    if existing.is_some() {
        header = header.child(
            Button::new("Remove overwrite")
                .variant(ButtonVariant::Danger)
                .size(ButtonSize::Small)
                .on_press(move |s: &mut S| {
                    let sh = acc(s);
                    sh.api(Method::DELETE, format!("/channels/{id}/permissions/{t}"), None, Then::Nothing);
                    edit_target(sh, id, None);
                })
                .el(p),
        );
    }
    let mut groups = col().w_full().gap(24.0);
    let threads = store.guilds.get(&g).and_then(|gs| gs.info.as_ref()).is_some_and(|i| i.threads_active);
    for (gt, entries) in perms::channel(ch.kind, threads) {
        let mut list = col().w_full().gap(12.0).child(text(crate::i18n::t(gt)).font(font(12.0, 600.0).lh(16.0)).text_case(rune::TextCase::Uppercase).color(p.text_tertiary));
        for &(bit, name, desc) in &entries {
            let (name, desc) = (crate::i18n::t(name), crate::i18n::t(desc));
            let now = tri(allow, deny, bit);
            let seg = |state: Tri, glyph: &'static Icon, on_color: rune::Color, label: &str| {
                let on = now == state;
                let (a, d) = set_tri(allow, deny, bit, state);
                row()
                    .size(32.0, 28.0)
                    .center()
                    .fill(if on { on_color } else { p.background_tertiary })
                    .hover(if on { on_color } else { p.background_modifier_hover })
                    .cursor(rune::Cursor::Pointer)
                    .role("radio")
                    .checked(on)
                    .label(format!("{label} {name}"))
                    .on_press(move |s: &mut S| {
                        let d_ = &mut acc(s).form.data;
                        d_.insert("allow".into(), json!(a.bits()));
                        d_.insert("deny".into(), json!(d.bits()));
                    })
                    .child(icon(glyph, 14.0).color(if on { p.text_on_brand_primary } else { p.text_primary_muted }))
            };
            let control = row()
                .radius(6.0)
                .border(1.0, p.background_modifier_accent)
                .child(seg(Tri::Deny, &icons::X_BOLD, p.status_danger, crate::i18n::t("Deny")))
                .child(seg(Tri::Inherit, &icons::MINUS_BOLD, mix(p.text_primary_muted, p.background_tertiary, 60.0), crate::i18n::t("Inherit")))
                .child(seg(Tri::Allow, &icons::CHECK_BOLD, p.status_online, crate::i18n::t("Allow")));
            list = list.child(
                row()
                    .w_full()
                    .gap(16.0)
                    .center_y()
                    .child(col().grow(1.0).min_w(0.0).child(title(name, p)).child(small(desc, p).wrap()))
                    .child(control),
            );
        }
        groups = groups.child(list);
    }
    let lead = if kind == OverwriteType::ROLE { color_dot(store.guilds.get(&g).and_then(|gs| gs.roles.iter().find(|r| r.id == t)).map_or(0, |r| r.color), p) } else { crate::shell::avatar(store, st, t, 24.0, None, p.background_secondary, p) };
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(header)
        .child(row().gap(8.0).center_y().child(lead).child(text(subject_name(store, g, t)).font(font(20.0, 600.0).lh(24.0)).color(p.text_primary)))
        .child(section(crate::i18n::t("Permissions"), groups, true, p));
    // An overwrite that doesn't exist yet is saved even when empty, so it shows in the list.
    if existing.is_none() || (allow, deny) != (cur_allow, cur_deny) {
        body = body.child(save_bar(st, p, acc, move |s: &mut S| {
            let sh = acc(s);
            let b = json!({ "type": kind.0, "allow": allow.bits().to_string(), "deny": deny.bits().to_string() });
            sh.api(Method::PUT, format!("/channels/{id}/permissions/{t}"), Some(b), Then::Nothing);
            sh.form.data.clear();
        }));
    } else if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tri_state_moves_a_bit_between_allow_and_deny() {
        let (a, d) = (P::SEND_MESSAGES, P::VIEW_CHANNEL);
        assert_eq!(tri(a, d, P::SEND_MESSAGES), Tri::Allow);
        assert_eq!(tri(a, d, P::VIEW_CHANNEL), Tri::Deny);
        assert_eq!(tri(a, d, P::CONNECT), Tri::Inherit);
        assert_eq!(set_tri(a, d, P::VIEW_CHANNEL, Tri::Allow), (P::SEND_MESSAGES | P::VIEW_CHANNEL, P::empty()));
        assert_eq!(set_tri(a, d, P::SEND_MESSAGES, Tri::Inherit), (P::empty(), P::VIEW_CHANNEL));
    }
}
