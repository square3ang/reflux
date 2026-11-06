//! Settings tabs: the community's Moderation (`GuildModerationTab`), Vanity URL
//! (`GuildVanityURLTab`), Webhooks (`GuildWebhooksTab`) and Explore listing tabs, the channel
//! Invites and Webhooks tabs, the webhook editor, and pausing invites.
// shortcut: the webhook editor returns to the community's Webhooks tab even when opened from a
// channel's.

use reflux_model::{ChannelType, Invite, Permissions as P, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, Tx, col, row, text};
use serde_json::{Value, json};

use super::{Done, Modal, body, error, field, label};
use crate::guild::{card, empty, loaded, save_bar, small, title};
use crate::settings::tabs::section;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Modal as Dialog, RadioGroup, Select, Switch, popup_access};

/// Fetches what a community tab of this module lists.
pub fn load_guild_tab(st: &mut ShellState, g: Snowflake, tab: &str) {
    match tab {
        "vanity_url" => st.social(Method::GET, format!("/guilds/{g}/vanity-url"), None, Done::Form("vanity".into())),
        "webhooks" => st.social(Method::GET, format!("/guilds/{g}/webhooks"), None, Done::Form("webhooks".into())),
        "explore" => {
            st.social(Method::GET, format!("/guilds/{g}/discovery"), None, Done::Form("discovery".into()));
            st.social(Method::GET, "/discovery/categories", None, Done::Form("categories".into()));
        }
        _ => {}
    }
}

pub fn load_channel_tab(st: &mut ShellState, c: Snowflake, tab: &str) {
    match tab {
        "invites" => st.social(Method::GET, format!("/channels/{c}/invites"), None, Done::Form("invites".into())),
        "webhooks" => st.social(Method::GET, format!("/channels/{c}/webhooks"), None, Done::Form("webhooks".into())),
        _ => {}
    }
}

/// A community tab body, if this module draws it.
pub fn guild_tab<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, tab: &str, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> Option<El> {
    Some(match tab {
        "moderation" => moderation(store, st, g, p, acc),
        "vanity_url" => vanity(store, st, g, p, acc),
        "webhooks" => webhooks(store, st, g, None, p, acc),
        "explore" => explore(st, g, p, acc, layer),
        _ => return None,
    })
}

pub fn channel_tab<S: 'static>(store: &Store, st: &ShellState, c: Snowflake, tab: &str, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let g = store.channels.get(&c)?.guild_id?;
    Some(match tab {
        "invites" => channel_invites(store, st, c, p, acc),
        "webhooks" => webhooks(store, st, g, Some(c), p, acc),
        _ => return None,
    })
}

fn heading(t: &str, d: &str, p: &Palette) -> El {
    col().w_full().gap(4.0).child(label(t, p)).children((!d.is_empty()).then(|| text(d.to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap()))
}

// ---- Moderation -----------------------------------------------------------------------------

const VERIFICATION: [(&str, &str, Option<&str>); 4] = [
    ("0", "None", Some("No verification is required.")),
    ("1", "Low", Some("Requires a verified email address.")),
    ("2", "Medium", Some("Requires a verified email address and an account that's at least 5 minutes old.")),
    ("3", "High", Some("Requires everything in medium, plus being a member of the community for at least 10 minutes.")),
];
const FILTER: [(&str, &str, Option<&str>); 3] = [
    ("0", "Off", Some("Let the community self-moderate")),
    ("1", "Filter members without roles", Some("Suggested for most communities")),
    ("2", "Filter everyone", Some("Maximum protection for family-friendly spaces")),
];
const MATURE: [(&str, &str); 2] = [("on", "On"), ("off", "Off")];

/// The edited value of `key` (`form.data`), else the guild's.
fn cur(st: &ShellState, key: &str, saved: Value) -> Value {
    st.form.data.get(key).cloned().unwrap_or(saved)
}

fn moderation<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let Some(info) = store.guilds.get(&g).and_then(|gs| gs.info.as_ref()) else { return empty("Loading…", p) };
    let me = store.my_id();
    let can = store.guild_permissions(g).contains(P::MANAGE_GUILD);
    let owner = info.owner_id == me;
    let discoverable = info.features.iter().any(|f| f == "DISCOVERABLE");
    let set = move |k: &'static str| move |s: &mut S, v: &str| {
        acc(s).form.data.insert(k.into(), json!(v.parse::<u8>().unwrap_or(0)));
    };
    let vl = cur(st, "verification_level", json!(info.verification_level)).as_u64().unwrap_or(0).to_string();
    let ef = if discoverable { "2".to_string() } else { cur(st, "explicit_content_filter", json!(info.explicit_content_filter)).as_u64().unwrap_or(0).to_string() };
    let nsfw = cur(st, "nsfw", json!(info.nsfw)).as_bool().unwrap_or(false);
    let cw = cur(st, "content_warning_level", json!(info.content_warning_level)).as_u64().unwrap_or(0) == 1;
    let mfa = cur(st, "mfa_level", json!(info.mfa_level)).as_u64().unwrap_or(0) == 1;
    let mut verification = col()
        .w_full()
        .gap(12.0)
        .child(heading("Member verification", "Choose what members must have before they can post or DM community members. Members with roles can bypass these checks. For public spaces, we recommend enabling verification.", p));
    if discoverable {
        verification = verification.child(small("Communities listed in Explore need at least email verification. You can't pick \"None\" while your community is listed in Explore.", p).wrap());
    }
    verification = verification.child(RadioGroup::new(&VERIFICATION, Some(&vl)).disabled(!can).on_change(set("verification_level")).el(p));
    let filter = col()
        .w_full()
        .gap(12.0)
        .child(heading("Content filtering", "Automatically check images and videos for explicit content in channels not marked for mature content.", p))
        .child(RadioGroup::new(&FILTER, Some(&ef)).disabled(!can || discoverable).on_change(set("explicit_content_filter")).el(p));
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let mut mature = col()
        .w_full()
        .gap(12.0)
        .child(heading("Mature content & content warnings", "", p))
        .child(
            row().w_full().center_y().child(text("Mature content").font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0)).child(
                Select::new("mod-mature", &MATURE, Some(if nsfw { "on" } else { "off" }))
                    .width(160.0)
                    .disabled(!can)
                    .on_choose(move |s: &mut S, v: &str| {
                        acc(s).form.data.insert("nsfw".into(), json!(v == "on"));
                    })
                    .el(&st.popups, pops, p, &mut Vec::new()),
            ),
        )
        .child(Switch::new(cw).label("Show a content warning").disabled(!can).on_toggle(move |s: &mut S| {
            acc(s).form.data.insert("content_warning_level".into(), json!(if cw { 0 } else { 1 }));
        }).el(p));
    if cw {
        mature = mature.child(field(st, acc, "content_warning_text", info.content_warning_text.as_deref().unwrap_or(""), "Custom warning text", "This contains sensitive content.", p));
    }
    let mut content = col().w_full().gap(32.0).child(verification).child(filter).child(mature);
    if owner {
        content = content.child(
            col()
                .w_full()
                .gap(12.0)
                .child(heading("2FA requirement", "Require two-factor authentication for moderators before they can ban, kick, time out, or remove messages.", p))
                .child(Switch::new(mfa).label("Require 2FA for moderation actions").on_toggle(move |s: &mut S| {
                    acc(s).form.data.insert("mfa_level".into(), json!(if mfa { 0 } else { 1 }));
                }).el(p)),
        );
    }
    let cw_text = info.content_warning_text.clone().unwrap_or_default();
    let dirty = ["verification_level", "explicit_content_filter", "nsfw", "content_warning_level", "mfa_level"].iter().any(|k| st.form.data.contains_key(*k)) || st.form.value("content_warning_text", &cw_text) != cw_text;
    if dirty {
        let saved = info.mfa_level;
        content = content.child(save_bar(st, p, acc, move |s: &mut S| {
            let sh = acc(s);
            let mut b = json!({});
            for k in ["verification_level", "explicit_content_filter", "nsfw", "content_warning_level"] {
                if let Some(v) = sh.form.data.get(k) {
                    b[k] = v.clone();
                }
            }
            let t = sh.form.value("content_warning_text", &cw_text);
            if t != cw_text {
                b["content_warning_text"] = if t.trim().is_empty() { Value::Null } else { json!(t) };
            }
            let mfa = sh.form.data.get("mfa_level").and_then(Value::as_u64).filter(|&m| m != saved as u64);
            if b.as_object().is_some_and(|o| !o.is_empty()) {
                sh.social(Method::PATCH, format!("/guilds/{g}"), Some(b), Done::Nothing);
            }
            if let Some(level) = mfa {
                sh.social(Method::POST, format!("/guilds/{g}/mfa"), Some(json!({ "level": level })), Done::Nothing);
            }
            sh.form.data.retain(|k, _| !matches!(k.as_str(), "verification_level" | "explicit_content_filter" | "nsfw" | "content_warning_level" | "mfa_level"));
            sh.form.text.remove("content_warning_text");
        }));
    } else if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    section("Moderation", content, true, p)
}

// ---- Vanity URL -----------------------------------------------------------------------------

fn vanity<S: 'static>(_store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let v = match st.form.data.get("vanity") {
        Some(v) => v.clone(),
        None => return section("Vanity URL", error(st, p).unwrap_or_else(|| empty("Loading…", p)), true, p),
    };
    let code = v["code"].as_str().unwrap_or_default().to_string();
    let uses = v["uses"].as_u64().unwrap_or(0);
    let base = st.invite_base.trim_end_matches('/').to_string();
    let edit = st.form.value("vanity_code", &code);
    let mut content = col()
        .w_full()
        .gap(16.0)
        .child(small("Give your community a short, memorable invite link.", p).wrap())
        .child(row().w_full().gap(8.0).items_end().child(text(format!("{base}/")).font(font(14.0, 500.0).lh(40.0)).color(p.text_primary_muted)).child(col().grow(1.0).child(field(st, acc, "vanity_code", &code, "", "your-community", p))));
    if !code.is_empty() {
        content = content.child(small(format!("Current uses: {uses}"), p));
    }
    let mut buttons = row().gap(8.0);
    let code2 = code.clone();
    if !code.is_empty() {
        buttons = buttons.child(Button::new("Remove").variant(ButtonVariant::Secondary).key("vanity-remove").on_press(move |s: &mut S| {
            acc(s).social(Method::PATCH, format!("/guilds/{g}/vanity-url"), Some(json!({ "code": null })), Done::Reload("vanity".into(), format!("/guilds/{g}/vanity-url")))
        }).el(p));
    }
    buttons = buttons.child(Button::new("Save").submitting(st.form.busy).disabled(edit.trim() == code2 || edit.trim().is_empty()).key("vanity-save").on_press(move |s: &mut S| {
        let sh = acc(s);
        let c = sh.form.value("vanity_code", &code).trim().to_string();
        sh.social(Method::PATCH, format!("/guilds/{g}/vanity-url"), Some(json!({ "code": c })), Done::Reload("vanity".into(), format!("/guilds/{g}/vanity-url")));
    }).el(p));
    content = content.child(buttons).children(error(st, p));
    section("Vanity URL", content, true, p)
}

// ---- Webhooks -------------------------------------------------------------------------------

fn webhook_url(st: &ShellState, w: &Value) -> Option<String> {
    let token = w["token"].as_str()?;
    Some(format!("{}/webhooks/{}/{token}", st.social.api_base.trim_end_matches('/'), w["id"].as_str()?))
}

fn webhooks<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, channel: Option<Snowflake>, p: &Palette, acc: ShellAccess<S>) -> El {
    let list: Vec<Value> = match loaded(st, "webhooks", p) {
        Ok(l) => l,
        Err(e) => return section("Webhooks", e, true, p),
    };
    let reload = match channel {
        Some(c) => format!("/channels/{c}/webhooks"),
        None => format!("/guilds/{g}/webhooks"),
    };
    let rows: Vec<El> = list
        .iter()
        .map(|w| {
            let id: Snowflake = w["id"].as_str().and_then(|s| s.parse().ok()).unwrap_or_default();
            let ch = w["channel_id"].as_str().and_then(|s| s.parse::<Snowflake>().ok()).and_then(|c| store.channels.get(&c)).and_then(|c| c.name.clone()).unwrap_or_default();
            let by = w["user"]["global_name"].as_str().or(w["user"]["username"].as_str()).unwrap_or("Unknown").to_string();
            let url = webhook_url(st, w);
            let w2 = w.clone();
            let reload = reload.clone();
            let mut r = row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(col().grow(1.0).min_w(0.0).child(title(w["name"].as_str().unwrap_or("Webhook").to_string(), p)).child(small(format!("#{ch} · created by {by}"), p)));
            if let Some(url) = url {
                r = r.child(Button::<S>::new("Copy URL").variant(ButtonVariant::Secondary).size(ButtonSize::Compact).key(format!("webhook-copy-{id}")).el(p).on_press_tx(move |_: &mut S, tx: &mut Tx| tx.copy(url.clone())));
            }
            r.child(Button::new("Edit").variant(ButtonVariant::Secondary).size(ButtonSize::Compact).key(format!("webhook-edit-{id}")).on_press(move |s: &mut S| {
                let sh = acc(s);
                sh.open_social(Modal::Webhook { guild: g, id });
                sh.form.data.insert("webhook".into(), w2.clone());
            }).el(p))
                .child(Button::new("Delete").variant(ButtonVariant::Danger).size(ButtonSize::Compact).key(format!("webhook-delete-{id}")).on_press(move |s: &mut S| {
                    acc(s).social(Method::DELETE, format!("/webhooks/{id}"), None, Done::Reload("webhooks".into(), reload.clone()))
                }).el(p))
        })
        .collect();
    let mut content = col().w_full().gap(16.0).child(small(if channel.is_some() { "Webhooks post messages to this channel from other apps." } else { "View and manage every webhook configured across your community." }, p).wrap());
    if let Some(c) = channel {
        content = content.child(row().child(Button::new("Create webhook").size(ButtonSize::Small).key("webhook-create").on_press(move |s: &mut S| {
            acc(s).social(Method::POST, format!("/channels/{c}/webhooks"), Some(json!({ "name": "New webhook" })), Done::Reload("webhooks".into(), format!("/channels/{c}/webhooks")))
        }).el(p)));
    }
    content = content.children(error(st, p)).child(if rows.is_empty() { empty("No webhooks", p) } else { card(rows, p) });
    section("Webhooks", content, true, p)
}

/// Name, channel and avatar of one webhook (`form.data["webhook"]`).
pub fn webhook_editor<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, id: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let back = move |s: &mut S| acc(s).open_guild_settings(g, "webhooks");
    let w = st.form.data.get("webhook").cloned().unwrap_or(Value::Null);
    let name = w["name"].as_str().unwrap_or_default().to_string();
    let ch = w["channel_id"].as_str().unwrap_or_default().to_string();
    let mut chans: Vec<(String, String)> = store
        .channels
        .values()
        .filter(|c| c.guild_id == Some(g) && matches!(c.kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT))
        .map(|c| (c.id.to_string(), format!("#{}", c.name.clone().unwrap_or_default())))
        .collect();
    chans.sort_by(|a, b| a.1.cmp(&b.1));
    let opts: Vec<(&str, &str)> = chans.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let pick = st.form.data.get("channel").and_then(Value::as_str).unwrap_or(&ch).to_string();
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let avatar = match st.form.data.get("avatar") {
        Some(Value::String(_)) => "New avatar picked",
        Some(_) => "Avatar removed",
        None if w["avatar"].is_string() => "Custom avatar",
        None => "Default avatar",
    };
    let mut items = vec![
        field(st, acc, "name", &name, "Name", "Webhook", p),
        Select::new("webhook-channel", &opts, Some(&pick)).label("Channel").width(406.0).on_choose(move |s: &mut S, v: &str| {
            acc(s).form.data.insert("channel".into(), json!(v));
        }).el(&st.popups, pops, p, layer),
        col().gap(8.0).child(label("Avatar", p)).child(
            row()
                .gap(8.0)
                .center_y()
                .child(small(avatar, p))
                .child(Button::<S>::new("Change avatar").variant(ButtonVariant::Secondary).size(ButtonSize::Small).el(p).on_press_tx(crate::guild::pick_image(acc, "avatar")))
                .child(Button::new("Remove avatar").variant(ButtonVariant::Secondary).size(ButtonSize::Small).on_press(move |s: &mut S| {
                    acc(s).form.data.insert("avatar".into(), Value::Null);
                }).el(p)),
        ),
    ];
    items.extend(error(st, p));
    let save = move |s: &mut S| {
        let sh = acc(s);
        let n = sh.form.value("name", &name).trim().to_string();
        if n.is_empty() {
            sh.form.error = Some("Name is required".into());
            return;
        }
        let mut b = json!({ "name": n });
        if let Some(c) = sh.form.data.get("channel") {
            b["channel_id"] = c.clone();
        }
        if let Some(a) = sh.form.data.get("avatar") {
            b["avatar"] = a.clone();
        }
        sh.social(Method::PATCH, format!("/webhooks/{id}"), Some(b), Done::GuildTab(g, "webhooks"));
    };
    let footer = row()
        .gap(8.0)
        .child(Button::new("Cancel").variant(ButtonVariant::Secondary).on_press(back).el(p))
        .child(Button::new("Save").submitting(st.form.busy).key("webhook-save").on_press(save).el(p));
    Dialog::new("Edit webhook", back).body(body(items)).footer(footer).el(p)
}

// ---- Explore --------------------------------------------------------------------------------

const CATEGORIES: [(&str, &str); 9] = [
    ("0", "Gaming"),
    ("1", "Music"),
    ("2", "Entertainment"),
    ("3", "Education"),
    ("4", "Science & Technology"),
    ("5", "Content Creator"),
    ("6", "Anime & Manga"),
    ("7", "Movies & TV"),
    ("8", "Other"),
];

fn explore<S: 'static>(st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some(status) = st.form.data.get("discovery").cloned() else {
        return section("Explore", error(st, p).unwrap_or_else(|| empty("Loading…", p)), true, p);
    };
    let app = &status["application"];
    let state = app["status"].as_str().unwrap_or("");
    let eligible = status["eligible"].as_bool().unwrap_or(false);
    let min = status["min_member_count"].as_u64().unwrap_or(0);
    let mut content = col().w_full().gap(16.0).child(small("List your community in Explore so people can find and join it.", p).wrap());
    let (badge, line) = match state {
        "pending" => ("Pending review", "An instance admin will review your application."),
        "approved" => ("Listed", "Your community is listed in Explore."),
        "rejected" => ("Rejected", "You can change your listing and apply again."),
        "removed" => ("Removed", "An instance admin removed the listing. You can apply again."),
        _ if eligible => ("Not listed", "Fill in a description and category to apply."),
        _ => ("Not eligible", ""),
    };
    content = content.child(row().gap(8.0).center_y().child(text(badge).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary)).child(small(line, p)));
    if !eligible && app.is_null() {
        content = content.child(small(format!("Communities need at least {min} members to apply."), p));
        return section("Explore", content, true, p);
    }
    let desc0 = app["description"].as_str().unwrap_or_default().to_string();
    let cat0 = app["category_type"].as_u64().map_or("8".to_string(), |c| c.to_string());
    let cat = st.form.data.get("category").and_then(Value::as_str).unwrap_or(&cat0).to_string();
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    content = content
        .child(field(st, acc, "description", &desc0, "Description", "What is your community about? (10-300 characters)", p))
        .child(Select::new("explore-category", &CATEGORIES, Some(&cat)).label("Category").width(320.0).on_choose(move |s: &mut S, v: &str| {
            acc(s).form.data.insert("category".into(), json!(v));
        }).el(&st.popups, pops, p, layer))
        .children(error(st, p));
    let reload = Done::Reload("discovery".into(), format!("/guilds/{g}/discovery"));
    let r2 = reload.clone();
    let apply = !matches!(state, "pending" | "approved");
    let submit = move |s: &mut S| {
        let sh = acc(s);
        let d = sh.form.value("description", &desc0).trim().to_string();
        let n = d.chars().count();
        if !(10..=300).contains(&n) {
            sh.form.error = Some("The description must be 10-300 characters.".into());
            return;
        }
        let c = sh.form.data.get("category").and_then(Value::as_str).unwrap_or(&cat0).parse::<u8>().unwrap_or(8);
        let method = if apply { Method::POST } else { Method::PATCH };
        sh.social(method, format!("/guilds/{g}/discovery"), Some(json!({ "description": d, "category_type": c })), r2.clone());
    };
    let mut buttons = row().gap(8.0).child(Button::new(if apply { "Apply" } else { "Save" }).submitting(st.form.busy).key("explore-submit").on_press(submit).el(p));
    if matches!(state, "pending" | "approved") {
        buttons = buttons.child(Button::new(if state == "approved" { "Remove listing" } else { "Withdraw" }).variant(ButtonVariant::Danger).key("explore-delete").on_press(move |s: &mut S| {
            acc(s).social(Method::DELETE, format!("/guilds/{g}/discovery"), None, reload.clone())
        }).el(p));
    }
    section("Explore", content.child(buttons), true, p)
}

// ---- Invites --------------------------------------------------------------------------------

fn channel_invites<S: 'static>(store: &Store, st: &ShellState, c: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let list: Vec<Invite> = match loaded(st, "invites", p) {
        Ok(l) => l,
        Err(e) => return section("Invites", e, true, p),
    };
    let rows: Vec<El> = list
        .iter()
        .map(|i| {
            let code = i.code.clone();
            let by = i.inviter.as_ref().map(|u| u.global_name.clone().unwrap_or_else(|| u.username.clone())).unwrap_or_else(|| "Unknown".into());
            let uses = if i.max_uses == 0 { format!("{} uses", i.uses) } else { format!("{}/{} uses", i.uses, i.max_uses) };
            let expires = i.expires_at.as_deref().map_or("Never expires".to_string(), |t| format!("Expires {}", t.get(..10).unwrap_or(t)));
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(col().grow(1.0).min_w(0.0).child(title(code.clone(), p)).child(small(format!("by {by} · {uses} · {expires}"), p)))
                .child(Button::new("Revoke").variant(ButtonVariant::Danger).size(ButtonSize::Compact).key(format!("invite-revoke-{code}")).on_press(move |s: &mut S| {
                    acc(s).social(Method::DELETE, format!("/invites/{code}"), None, Done::Reload("invites".into(), format!("/channels/{c}/invites")))
                }).el(p))
        })
        .collect();
    let mut content = col().w_full().gap(16.0);
    if let Some(g) = store.channels.get(&c).and_then(|ch| ch.guild_id) {
        content = content.children(pause_invites(store, g, p, acc));
    }
    content = content.children(error(st, p)).child(if rows.is_empty() { empty("No invites yet.", p) } else { card(rows, p) });
    section("Invites", content, true, p)
}

/// "Pause invites": toggles the `INVITES_DISABLED` feature (needs `MANAGE_GUILD`).
pub fn pause_invites<S: 'static>(store: &Store, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if !store.guild_permissions(g).contains(P::MANAGE_GUILD) {
        return None;
    }
    let features = store.guilds.get(&g)?.info.as_ref()?.features.clone();
    let paused = features.iter().any(|f| f == "INVITES_DISABLED");
    let next: Vec<String> = if paused { features.into_iter().filter(|f| f != "INVITES_DISABLED").collect() } else { features.into_iter().chain(["INVITES_DISABLED".to_string()]).collect() };
    Some(
        Switch::new(paused)
            .label("Pause invites")
            .description("Nobody can join with an invite while this is on.")
            .on_toggle(move |s: &mut S| acc(s).social(Method::PATCH, format!("/guilds/{g}"), Some(json!({ "features": next.clone() })), Done::Nothing))
            .el(p),
    )
}
