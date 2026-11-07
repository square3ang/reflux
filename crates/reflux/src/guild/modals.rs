//! The small modals: add or join a community (`AddGuildModal`), invites (`InviteModal`), channel
//! and category creation, notification and privacy settings, and the confirmation the settings
//! overlays share.

use reflux_model::{ChannelType, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, Icon, Tx, col, icon, row, text};
use serde_json::{Value, json};

use super::menu::create_invite;
use super::{AddView, Confirm, Dialog, Then, error, field, pick_image};
use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Modal, ModalSize, RadioGroup, Select, Switch, over, radio_dot};
use crate::i18n::t;

pub fn view<S: 'static>(store: &Store, st: &ShellState, d: &Dialog, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let start = layer.len();
    let modal = match *d {
        Dialog::Add(v) => add(st, v, p, acc),
        Dialog::Invite(ch, edit) => invite(store, st, ch, edit, p, acc, layer),
        Dialog::CreateChannel { guild, parent, category } => create_channel(store, st, guild, parent, category, p, acc),
        Dialog::Notifications(g) => notifications(store, st, g, p, acc, layer),
        Dialog::Privacy(g) => privacy(store, g, p, acc),
        _ => return,
    };
    // Under any select list it opened.
    layer.insert(start, modal);
}

fn close<S: 'static>(acc: ShellAccess<S>) -> impl Fn(&mut S) + Clone + 'static {
    move |s: &mut S| acc(s).dialog = None
}

/// `Modal.Description`: 14/400, line-height 1.4, `text-primary-muted`.
fn desc(t: &str, p: &Palette) -> El {
    text(t.to_string()).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap()
}

/// `.sectionLabel` / `.sectionTitle`: 14/500–600 primary.
fn label(t: &str, weight: f32, p: &Palette) -> El {
    text(t.to_string()).font(font(14.0, weight).lh(20.0)).color(p.text_primary)
}

fn buttons(items: Vec<El>) -> El {
    row().gap(8.0).children(items)
}

// ---- Add a community ------------------------------------------------------------------------

/// `.actionButton`: a bordered tile with a 48px brand disc and a 14/600 label.
fn action<S: 'static>(glyph: &'static Icon, title: &str, p: &Palette, f: impl Fn(&mut S) + 'static) -> El {
    col()
        .grow(1.0)
        .w(0.0)
        .py(25.0)
        .px(17.0)
        .gap(12.0)
        .items_center()
        .radius(8.0)
        .fill(p.background_secondary)
        .border(1.0, over(p.background_modifier_accent, p.background_secondary))
        .hover(p.background_secondary_alt)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(title.to_string())
        .on_press(f)
        .child(row().size(48.0, 48.0).center().radius(24.0).fill(p.brand_primary).child(icon(glyph, 24.0).color(p.brand_primary_fill)))
        .child(text(title.to_string()).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary).align(rune::Align::Center))
}

fn add<S: 'static>(st: &ShellState, v: AddView, p: &Palette, acc: ShellAccess<S>) -> El {
    let go = move |v: AddView| move |s: &mut S| {
        let sh = acc(s);
        sh.form.error = None;
        sh.dialog = Some(Dialog::Add(v));
    };
    let back = Button::new(t("Back")).variant(ButtonVariant::Secondary).on_press(go(AddView::Landing)).el(p);
    match v {
        // shortcut: "Import template" (a third-party platform's template API) isn't offered.
        AddView::Landing => Modal::new(t("Add a community"), close(acc))
            .body(
                col()
                    .w_full()
                    .gap(24.0)
                    .child(desc(t("Create a new community or join an existing one."), p))
                    .child(
                        row()
                            .w_full()
                            .gap(12.0)
                            .child(action(&icons::HOUSE, t("Create community"), p, go(AddView::Create)))
                            .child(action(&icons::LINK_BOLD, t("Join community"), p, go(AddView::Join))),
                    ),
            )
            .el(p),
        AddView::Create => {
            let name = st.form.value("name", "");
            let icon_set = st.form.data.get("icon").is_some_and(Value::is_string);
            // `.iconPlaceholder`: an 80px ring with the acronym of the name typed so far.
            let mut face = row()
                .size(80.0, 80.0)
                .center()
                .radius(40.0)
                .fill(p.background_tertiary)
                .border(1.0, over(p.background_modifier_accent, p.background_tertiary));
            face = if icon_set {
                // shortcut: a picked icon shows as a picture glyph, not a preview of the image.
                face.fill(p.brand_primary).child(icon(&icons::IMAGE_FILL, 32.0).color(p.brand_primary_fill))
            } else if !name.trim().is_empty() {
                let a = crate::shell::acronym(&name);
                face.child(text(a).font(font(24.0, 500.0)).color(p.text_primary))
            } else {
                face
            };
            let mut icon_buttons = row().gap(8.0).child(
                Button::<S>::new(if icon_set { t("Change icon") } else { t("Upload icon") }).variant(ButtonVariant::Secondary).size(ButtonSize::Small).el(p).on_press_tx(pick_image(acc, "icon")),
            );
            if icon_set {
                icon_buttons = icon_buttons.child(
                    Button::new(t("Remove icon"))
                        .variant(ButtonVariant::Secondary)
                        .size(ButtonSize::Small)
                        .on_press(move |s: &mut S| {
                            acc(s).form.data.remove("icon");
                        })
                        .el(p),
                );
            }
            let mut body = col()
                .w_full()
                .gap(16.0)
                .child(col().gap(8.0).child(label(t("Community icon"), 500.0, p)).child(row().gap(16.0).center_y().child(face).child(icon_buttons)))
                .child(field(st, acc, "name", "", t("Community name"), "", p))
                .child(text("By creating a community, you agree to follow and uphold the community guidelines.").font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).wrap());
            if let Some(e) = error(st, p) {
                body = body.child(e);
            }
            let create = move |s: &mut S| {
                let sh = acc(s);
                let name = sh.form.value("name", "").trim().to_string();
                if name.is_empty() {
                    sh.form.error = Some("Community name is required.".into());
                    return;
                }
                let mut b = json!({ "name": name });
                if let Some(i) = sh.form.data.get("icon") {
                    b["icon"] = i.clone();
                }
                sh.api(Method::POST, "/guilds", Some(b), Then::Enter);
            };
            Modal::new(t("Create a community"), close(acc))
                .body(body)
                .footer(buttons(vec![back, Button::new(t("Create community")).submitting(st.form.busy).key("create-guild").on_press(create).el(p)]))
                .el(p)
        }
        AddView::Join => {
            let mut body = col()
                .w_full()
                .gap(12.0)
                .child(desc(t("Enter the invite link to join a community."), p))
                .child(field(st, acc, "invite", "", t("Invite link"), &format!("{}/AbCdEf", st.invite_base.trim_end_matches('/')), p));
            if let Some(e) = error(st, p) {
                body = body.child(e);
            }
            let join = move |s: &mut S| {
                let sh = acc(s);
                match parse_invite(&sh.form.value("invite", "")) {
                    Some(code) => sh.api(Method::POST, format!("/invites/{code}"), None, Then::Enter),
                    None => sh.form.error = Some("Enter an invite link or code.".into()),
                }
            };
            Modal::new(t("Join a community"), close(acc))
                .body(body)
                .footer(buttons(vec![back, Button::new(t("Join community")).submitting(st.form.busy).key("join-guild").on_press(join).el(p)]))
                .el(p)
        }
    }
}

/// The code of an invite link (`https://fluxer.gg/AbCd`, `…/invite/AbCd`) or a bare code.
pub fn parse_invite(s: &str) -> Option<String> {
    let s = s.trim().split(['?', '#']).next()?.trim_end_matches('/');
    let code = s.rsplit('/').next()?;
    (!code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then(|| code.to_string())
}

// ---- Invite ---------------------------------------------------------------------------------

const EXPIRY: [(&str, &str); 7] =
    [("1800", "30 minutes"), ("3600", "1 hour"), ("21600", "6 hours"), ("43200", "12 hours"), ("86400", "1 day"), ("604800", "7 days"), ("0", "Never")];
const USES: [(&str, &str); 7] = [("0", "No limit"), ("1", "1 use"), ("5", "5 uses"), ("10", "10 uses"), ("25", "25 uses"), ("50", "50 uses"), ("100", "100 uses")];

fn num(st: &ShellState, key: &str, default: u64) -> u64 {
    st.form.data.get(key).and_then(Value::as_u64).unwrap_or(default)
}

/// "Your invite link expires in 7 days." (`getExpirationText`).
fn expiry_text(max_age: u64) -> String {
    let plural = |n: u64, msg: &str, arg: &str| crate::i18n::tf(msg, &[(arg, &n.to_string())]);
    match max_age {
        0 => t("This invite link never expires.").into(),
        s if s % 86_400 == 0 => plural(s / 86_400, "{days, plural, one {Your invite link expires in # day.} other {Your invite link expires in # days.}}", "days"),
        s if s % 3600 == 0 => plural(s / 3600, "{hours, plural, one {Your invite link expires in # hour.} other {Your invite link expires in # hours.}}", "hours"),
        s if s % 60 == 0 => plural(s / 60, "{minutes, plural, one {Your invite link expires in # minute.} other {Your invite link expires in # minutes.}}", "minutes"),
        s => plural(s, "{seconds, plural, one {Your invite link expires in # second.} other {Your invite link expires in # seconds.}}", "seconds"),
    }
}

fn invite<S: 'static>(store: &Store, st: &ShellState, ch: Snowflake, edit: bool, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let guild = store.channels.get(&ch).and_then(|c| c.guild_id);
    let gname = guild.and_then(|g| store.guilds.get(&g)?.info.as_ref()).map_or(String::new(), |i| i.name.clone());
    let created = st.form.data.get("invite");
    let (max_age, max_uses, temporary) = (num(st, "max_age", 604_800), num(st, "max_uses", 0), st.form.data.get("temporary").and_then(Value::as_bool).unwrap_or(false));
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    if edit {
        let set = move |key: &'static str| move |s: &mut S, v: &str| {
            acc(s).form.data.insert(key.into(), json!(v.parse::<u64>().unwrap_or(0)));
        };
        let (age_s, uses_s) = (max_age.to_string(), max_uses.to_string());
        let body = col()
            .w_full()
            .gap(16.0)
            .child(Select::new("invite-expiry", &EXPIRY, Some(&age_s)).label(t("Expire after")).width(406.0).on_choose(set("max_age")).el(&st.popups, pops, p, layer))
            .child(Select::new("invite-uses", &USES, Some(&uses_s)).label(t("Max number of uses")).width(406.0).on_choose(set("max_uses")).el(&st.popups, pops, p, layer))
            .child(
                Switch::new(temporary)
                    .label(t("Temporary membership"))
                    .description(t("Members are removed when offline unless given a role."))
                    .on_toggle(move |s: &mut S| {
                        acc(s).form.data.insert("temporary".into(), json!(!temporary));
                    })
                    .el(p),
            );
        let generate = move |s: &mut S| {
            let sh = acc(s);
            sh.dialog = Some(Dialog::Invite(ch, false));
            sh.form.data.remove("invite");
            create_invite(sh, ch, max_age as u32, max_uses as u32, temporary);
        };
        return Modal::new(t("Invite link settings"), close(acc))
            .body(body)
            .footer(buttons(vec![
                Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(move |s: &mut S| acc(s).dialog = Some(Dialog::Invite(ch, false))).el(p),
                Button::new(t("Create new link")).key("invite-generate").on_press(generate).el(p),
            ]))
            .el(p);
    }
    let link = created.and_then(|v| v["code"].as_str()).map(|c| format!("{}/{c}", st.invite_base.trim_end_matches('/')));
    let friends = super::invite_friends::view(store, st, link.clone(), p, acc);
    let hide = st.desktop.streamer && st.prefs.streamer.hide_invites && link.is_some();
    let shown = if hide { crate::i18n::t("Link hidden while sharing").to_string() } else { link.clone().unwrap_or_default() };
    // `CopyLinkSection`: the link in a read-only field with a Copy button inside it.
    let bg = p.form_surface_background;
    let copy = Button::<S>::new(t("Copy")).size(ButtonSize::Compact).disabled(link.is_none()).key("invite-copy").el(p).on_press_tx(move |_: &mut S, tx: &mut Tx| {
        if let Some(l) = &link {
            tx.copy(l.clone());
        }
    });
    let field = row()
        .w_full()
        .min_h(44.0)
        .pl(17.0)
        .pr(5.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .child(
            text(if shown.is_empty() { "Creating invite…".into() } else { shown })
                .font(font(14.0, 400.0).lh(20.0))
                .color(if created.is_some() { p.text_primary } else { p.text_tertiary })
                .selectable(true)
                .truncate()
                .grow(1.0)
                .shrink(),
        )
        .child(copy);
    let edit_link = text(t("Edit invite link"))
        .font(font(12.0, 400.0).lh(16.0))
        .color(p.text_link)
        .cursor(rune::Cursor::Pointer)
        .role("link")
        .on_press(move |s: &mut S| acc(s).dialog = Some(Dialog::Invite(ch, true)));
    let has_friends = friends.is_some();
    let mut body = col()
        .w_full()
        .gap(8.0)
        .children(friends.map(|f| f.mb(16.0)))
        .child(label(if has_friends { t("Or send an invite link to a friend:") } else { t("Send an invite link to a friend:") }, 500.0, p))
        .child(field)
        .child(row().gap(4.0).child(text(expiry_text(max_age)).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted)).child(edit_link));
    if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    Modal::new(crate::i18n::tf("Invite friends to {guildName}", &[("guildName", &gname.to_string())]), close(acc)).body(body).el(p)
}

// ---- Create channel / category --------------------------------------------------------------

const TYPES: [(ChannelType, &str, &str); 4] = [
    (ChannelType::GUILD_TEXT, "Text", "Send messages, images, GIFs and emoji"),
    (ChannelType::GUILD_VOICE, "Voice", "Hang out with voice, video and screen share"),
    (ChannelType::GUILD_ANNOUNCEMENT, "Announcement", "Post updates other communities can follow"),
    (ChannelType::GUILD_LINK, "Link", "Shortcut to an external website"),
];

#[allow(clippy::too_many_arguments)]
fn create_channel<S: 'static>(store: &Store, st: &ShellState, guild: Snowflake, parent: Option<Snowflake>, category: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let kind = ChannelType(st.form.data.get("type").and_then(Value::as_u64).unwrap_or(0) as u16);
    let private = st.form.data.get("private").and_then(Value::as_bool).unwrap_or(false) && kind != ChannelType::GUILD_LINK;
    let access = st.form.data.get("step").and_then(Value::as_str) == Some("access");
    let me = store.my_id();
    let mut body = col().w_full().gap(20.0);
    if access {
        body = body.child(super::create_access::view(store, st, guild, p, acc));
    } else if !category {
        let options = TYPES.iter().map(|&(t, name, sub)| {
            let (name, sub) = (crate::i18n::t(name), crate::i18n::t(sub));
            let on = t == kind;
            row()
                .w_full()
                .py(4.0)
                .gap(8.0)
                .center_y()
                .role("radio")
                .checked(on)
                .label(name)
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .on_press(move |s: &mut S| {
                    acc(s).form.data.insert("type".into(), json!(t.0));
                })
                .child(radio_dot(on, p))
                .child(
                    row()
                        .gap(12.0)
                        .center_y()
                        .child(row().size(32.0, 32.0).center().radius(8.0).fill(p.background_modifier_hover).child(
                            icon(crate::shell::channel_icon(t), 20.0).color(if on { p.text_primary } else { p.text_primary_muted }),
                        ))
                        .child(
                            col()
                                .child(text(name).font(font(15.0, 600.0).lh(20.0)).color(p.text_primary))
                                .child(text(sub).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted)),
                        ),
                )
        });
        body = body.child(col().w_full().gap(8.0).child(label(t("Channel type"), 500.0, p)).child(col().w_full().gap(6.0).role("radiogroup").children(options)));
    }
    if !access {
        body = body.child(field(st, acc, "name", "", if category { t("Name") } else { t("Channel name") }, if category { t("New category") } else { "" }, p));
    }
    let link = kind == ChannelType::GUILD_LINK;
    if link && !category && !access {
        body = body.child(field(st, acc, "url", "", "URL", "https://", p));
    }
    if !link && !access {
        body = body.child(
            Switch::new(private)
                .label(t("Private channel"))
                .description(t("Only selected members and roles will be able to view this channel."))
                .on_toggle(move |s: &mut S| {
                    acc(s).form.data.insert("private".into(), json!(!private));
                })
                .el(p),
        );
    }
    if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    let create = move |s: &mut S| {
        let sh = acc(s);
        let name = sh.form.value("name", "").trim().to_string();
        if name.is_empty() {
            sh.form.error = Some(t("Name is required.").into());
            return;
        }
        // A private channel goes on to pick who may view it.
        if private && !access {
            sh.form.data.insert("step".into(), json!("access"));
            return;
        }
        let kind = if category { ChannelType::GUILD_CATEGORY } else { kind };
        let mut b = json!({ "type": kind.0, "name": name });
        if let Some(pid) = parent.filter(|_| !category) {
            b["parent_id"] = json!(pid);
        }
        if kind == ChannelType::GUILD_LINK {
            b["url"] = json!(sh.form.value("url", "").trim());
        }
        if private {
            b["permission_overwrites"] = super::create_access::overwrites(guild, kind, &super::create_access::picked(sh), me);
        }
        sh.api(Method::POST, format!("/guilds/{guild}/channels"), Some(b), if category { Then::Close } else { Then::Enter });
    };
    let title = if category { t("Create category") } else { t("Create channel") };
    let footer = if access {
        let go = if super::create_access::picked(st).is_empty() { t("Skip") } else { t("Create channel") };
        vec![
            Button::new(t("Go back")).variant(ButtonVariant::Secondary).on_press(move |s: &mut S| {
                acc(s).form.data.remove("step");
            }).el(p),
            Button::new(go).submitting(st.form.busy).key("create-channel").on_press(create).el(p),
        ]
    } else {
        vec![
            Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close(acc)).el(p),
            Button::new(if private { t("Next") } else { title }).submitting(st.form.busy).key("create-channel").on_press(create).el(p),
        ]
    };
    Modal::new(if access { t("Add members or roles") } else { title }, close(acc)).body(body).footer(buttons(footer)).el(p)
}

// ---- Notification settings ------------------------------------------------------------------

/// Mute durations (`getMuteDurationOptions`): ms, or 0 for "until I turn it back on".
const MUTE_FOR: [(&str, &str); 9] = [
    ("900000", "For 15 minutes"),
    ("1800000", "For 30 minutes"),
    ("3600000", "For 1 hour"),
    ("10800000", "For 3 hours"),
    ("14400000", "For 4 hours"),
    ("28800000", "For 8 hours"),
    ("86400000", "For 24 hours"),
    ("259200000", "For 3 days"),
    ("0", "Until I turn it back on"),
];

/// The `mute_config` for a duration (`createMuteConfig`).
pub fn mute_config(ms: u64, now_ms: u64) -> Value {
    if ms == 0 {
        return Value::Null;
    }
    json!({ "selected_time_window": ms, "end_time": chrono::DateTime::from_timestamp_millis((now_ms + ms) as i64).unwrap_or_default().to_rfc3339_opts(chrono::SecondsFormat::Millis, true) })
}

fn notifications<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let name = store.guilds.get(&g).and_then(|gs| gs.info.as_ref()).map_or(String::new(), |i| i.name.clone());
    let s = store.guild_settings.get(&Some(g)).cloned().unwrap_or_default();
    let patch = move |v: Value| move |sh: &mut S| acc(sh).intents.push(Intent::PatchGuildSettings { guild: Some(g), patch: v.clone() });
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    let now = st.now_ms;
    let mut mute = col().w_full().gap(12.0).child(
        Switch::new(s.muted)
            .label(crate::i18n::tf("Mute {displayLabel}", &[("displayLabel", &name.to_string())]))
            .description(t("Muting a community hides its unread badge and prevents notifications from appearing"))
            .on_toggle(patch(json!({ "muted": !s.muted, "mute_config": null })))
            .el(p),
    );
    if s.muted {
        let current = s.mute_config.as_ref().map_or("0".to_string(), |c| c.selected_time_window.to_string());
        mute = mute.child(
            Select::new("mute-duration", &MUTE_FOR, Some(&current))
                .label("Mute duration")
                .width(568.0)
                .on_choose(move |sh: &mut S, v| {
                    let ms = v.parse().unwrap_or(0);
                    acc(sh).intents.push(Intent::PatchGuildSettings { guild: Some(g), patch: json!({ "muted": true, "mute_config": mute_config(ms, now) }) });
                })
                .el(&st.popups, pops, p, layer),
        );
    }
    // `MessageNotifications`: 0 all, 1 mentions, 2 nothing, 3 inherit (the guild default).
    let level = match s.message_notifications.0 {
        3 => store.guilds.get(&g).and_then(|gs| gs.info.as_ref()).map_or(1, |i| i.default_message_notifications as u32),
        l => l as u32,
    };
    let lv = level.to_string();
    let opts = [("0", "All messages", None), ("1", "Only mentions", None), ("2", "Nothing", None)];
    let levels = RadioGroup::new(&opts, Some(&lv))
        .on_change(move |sh: &mut S, v| acc(sh).intents.push(Intent::PatchGuildSettings { guild: Some(g), patch: json!({ "message_notifications": v.parse::<u8>().unwrap_or(0) }) }))
        .el(p);
    let body = col()
        .w_full()
        .gap(24.0)
        .child(mute)
        .child(col().w_full().gap(12.0).child(label(t("Community notification settings"), 600.0, p)).child(levels))
        .child(
            col()
                .w_full()
                .gap(16.0)
                .child(Switch::new(s.suppress_everyone).label("Suppress @everyone and @here").on_toggle(patch(json!({ "suppress_everyone": !s.suppress_everyone }))).el(p))
                .child(Switch::new(s.suppress_roles).label(t("Suppress all role mentions")).on_toggle(patch(json!({ "suppress_roles": !s.suppress_roles }))).el(p)),
        )
        .child(Switch::new(s.mobile_push).label(t("Mobile push notifications")).on_toggle(patch(json!({ "mobile_push": !s.mobile_push }))).el(p))
        .child(overrides(store, st, g, level, p, acc, layer));
    // shortcut: unread badges and the reply mention preference aren't here.
    Modal::new(t("Notification settings"), close(acc)).size(ModalSize::Medium).body(body).el(p)
}

/// The "Notification overrides" section: a picker that adds a channel or category, then a
/// 2fr/1fr×4 grid (name, All, Mentions, Nothing, Mute) with a remove button on hover.
fn overrides<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, guild_level: u32, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let ov = store.guild_settings.get(&Some(g)).and_then(|s| s.channel_overrides.clone()).unwrap_or_default();
    // The API replaces the whole `channel_overrides` map, so each patch is built from the store now.
    let send = move |v: Value| move |s: &mut S| acc(s).intents.push(Intent::PatchGuildSettings { guild: Some(g), patch: v.clone() });
    let patch_ov = |c: Snowflake, body: Value| {
        send(crate::social::override_patch(store, Some(g), c, |o| match body.as_object() {
            Some(m) => m.iter().for_each(|(k, v)| o[k] = v.clone()),
            None => *o = Value::Null,
        }))
    };
    let mut chans: Vec<_> = store.channels.values().filter(|c| c.guild_id == Some(g) && !c.kind.is_thread() && (c.kind == ChannelType::GUILD_CATEGORY || store.can_view(c.id))).collect();
    chans.sort_by_key(|c| (c.position, c.id));
    let opts: Vec<(String, String)> = chans.iter().filter(|c| !ov.contains_key(&c.id)).map(|c| (c.id.to_string(), c.name.clone().unwrap_or_default())).collect();
    let opts: Vec<(&str, &str)> = opts.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let adds: Vec<(String, Value)> = chans.iter().filter(|c| !ov.contains_key(&c.id)).map(|c| (c.id.to_string(), crate::social::override_patch(store, Some(g), c.id, |_| {}))).collect();
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    let picker = Select::new("override-add", &opts, None)
        .placeholder(t("Select a channel or category"))
        .width(568.0)
        .on_choose(move |s: &mut S, v| {
            if let Some(patch) = adds.iter().find(|(id, _)| id == v).map(|(_, p)| p.clone()) {
                acc(s).intents.push(Intent::PatchGuildSettings { guild: Some(g), patch });
            }
        })
        .el(&st.popups, pops, p, layer);
    let mut section = col().w_full().gap(12.0).child(label(t("Notification overrides"), 600.0, p)).child(picker);
    let mut rows: Vec<_> = ov.iter().filter_map(|(id, o)| Some((store.channels.get(id)?, o))).collect();
    rows.sort_by_key(|(c, _)| (c.position, c.id));
    if rows.is_empty() {
        return section;
    }
    let cell = |e: El| row().grow(1.0).w(0.0).center().child(e);
    let head = |s: &str| cell(text(s.to_string()).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary));
    section = section.child(
        row()
            .w_full()
            .mt(8.0)
            .px(12.0)
            .py(8.0)
            .gap(8.0)
            .child(row().grow(2.0).w(0.0).child(text(t("Channel or category").to_uppercase()).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary)))
            .child(head(&t("All").to_uppercase()))
            .child(head(&t("Mentions").to_uppercase()))
            .child(head(&t("Nothing").to_uppercase()))
            .child(head(&t("Mute").to_uppercase())),
    );
    for (c, o) in rows {
        let id = c.id;
        let level = match o.message_notifications.0 {
            3 => guild_level,
            l => l as u32,
        };
        let is_cat = c.kind == ChannelType::GUILD_CATEGORY;
        let sub = (!is_cat).then(|| c.parent_id.and_then(|pid| store.channels.get(&pid)?.name.clone()).unwrap_or_else(|| t("No category").to_string()));
        let name = col().min_w(0.0).child(text(c.name.clone().unwrap_or_default()).font(font(14.0, 500.0).lh(19.6)).color(p.text_primary).truncate()).children(sub.map(|s| text(s).font(font(12.0, 400.0).lh(16.8)).color(p.text_tertiary)));
        let glyph: &'static Icon = if is_cat { &icons::FOLDER } else { crate::shell::channel_icon(c.kind) };
        let check = |n: u32| cell(crate::ui::Checkbox::new(level == n).on_toggle(patch_ov(id, json!({ "message_notifications": n }))).el(p));
        let remove = row()
            .pin(None, Some(-12.0), None, None)
            .h_full()
            .center_y()
            .group_opacity(0.0, 1.0)
            .child(
                row()
                    .size(24.0, 24.0)
                    .center()
                    .radius(999.0)
                    .fill(p.status_danger)
                    .cursor(rune::Cursor::Pointer)
                    .role("button")
                    .label(t("Remove override").to_string())
                    .key(format!("override-remove-{id}"))
                    .on_press(patch_ov(id, Value::Null))
                    .child(icon(&icons::X_BOLD, 14.0).color(rune::Color::WHITE)),
            );
        section = section.child(
            row()
                .w_full()
                .min_h(64.0)
                .padding(12.0)
                .gap(8.0)
                .center_y()
                .radius(8.0)
                .border(1.0, p.background_header_secondary)
                .hover(p.background_secondary)
                .hover_group()
                .child(row().grow(2.0).w(0.0).min_w(0.0).gap(8.0).center_y().child(icon(glyph, 20.0).color(p.text_tertiary)).child(name))
                .child(check(0))
                .child(check(1))
                .child(check(2))
                .child(cell(crate::ui::Checkbox::new(o.muted).on_toggle(patch_ov(id, json!({ "muted": !o.muted }))).el(p)))
                .child(remove),
        );
    }
    section
}

// ---- Privacy --------------------------------------------------------------------------------

fn privacy<S: 'static>(store: &Store, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let toggle = |list: &[Snowflake], key: &'static str| {
        let allowed = !list.contains(&g);
        let next: Vec<Snowflake> = if allowed { list.iter().copied().chain([g]).collect() } else { list.iter().copied().filter(|&x| x != g).collect() };
        (allowed, move |s: &mut S| acc(s).intents.push(Intent::PatchSettings(json!({ key: next.clone() }))))
    };
    let (dms, dm_toggle) = toggle(&store.settings.restricted_guilds, "restricted_guilds");
    let (bots, bot_toggle) = toggle(&store.settings.bot_restricted_guilds, "bot_restricted_guilds");
    let body = col()
        .w_full()
        .gap(16.0)
        .child(Switch::new(dms).label(t("Direct messages")).description(t("Let members of this community DM you")).on_toggle(dm_toggle).el(p))
        .child(Switch::new(bots).label(t("Bot direct messages")).description(t("Let bots in this community DM you")).on_toggle(bot_toggle).el(p));
    Modal::new(t("Privacy settings"), close(acc)).body(body).el(p)
}

// ---- Confirmation ---------------------------------------------------------------------------

/// The confirmation for `form.confirm`, over a settings overlay.
pub fn confirm<S: 'static>(store: &Store, st: &ShellState, c: Confirm, p: &Palette, acc: ShellAccess<S>) -> El {
    let cancel = move |s: &mut S| {
        let f = &mut acc(s).form;
        f.confirm = None;
        f.error = None;
    };
    let gname = |g: Snowflake| store.guilds.get(&g).and_then(|gs| gs.info.as_ref()).map_or(String::new(), |i| i.name.clone());
    let (title, msg, action, method, path, body, then): (String, String, &str, Method, String, Option<Value>, Then) = match c {
        Confirm::DeleteGuild(g) => (
            format!("Delete '{}'", gname(g)),
            "This deletes the community with its channels, messages and roles. It can't be undone.".into(),
            "Delete community",
            Method::POST,
            format!("/guilds/{g}/delete"),
            Some(json!({})),
            Then::Close,
        ),
        Confirm::DeleteChannel(ch) => {
            let c = store.channels.get(&ch);
            let kind = t(if c.is_some_and(|c| c.kind == ChannelType::GUILD_VOICE) { "voice channel" } else { "text channel" });
            let label = c.and_then(|c| c.name.clone()).map_or_else(|| t("this channel").to_string(), |n| format!("#{n}"));
            let (title, body) = (crate::i18n::tf("Delete {channelType}", &[("channelType", kind)]), crate::i18n::tf("Are you sure you want to delete {channelLabel}? This cannot be undone.", &[("channelLabel", &label)]));
            (title, body, "Delete channel", Method::DELETE, format!("/channels/{ch}"), None, Then::Close)
        }
        Confirm::DeleteRole(g, r) => {
            let n = store.guilds.get(&g).and_then(|gs| gs.roles.iter().find(|x| x.id == r)).map_or(String::new(), |x| x.name.clone());
            (t("Delete role").to_string(), crate::i18n::tf_plain("Are you sure you want to delete the <0>{0}</0> role? Any members with this role will no longer have it.", &[("0", &n)]), "Delete role", Method::DELETE, format!("/guilds/{g}/roles/{r}"), None, Then::Nothing)
        }
    };
    // `POST /guilds/{id}/delete` needs sudo mode: its `SUDO_MODE_REQUIRED` answer opens the
    // sudo prompt (password, authenticator, passkey or emailed code) and retries.
    let mut content = col().w_full().gap(16.0).child(desc(&msg, p));
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    let go = move |s: &mut S| acc(s).api(method.clone(), path.clone(), body.clone(), then.clone());
    Modal::new(title, cancel)
        .body(content)
        .footer(buttons(vec![
            Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(cancel).el(p),
            Button::new(action).variant(ButtonVariant::Danger).submitting(st.form.busy).key("confirm-action").on_press(go).el(p),
        ]))
        .el(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_codes_from_links() {
        assert_eq!(parse_invite("https://fluxer.gg/AbCd12").as_deref(), Some("AbCd12"));
        assert_eq!(parse_invite(" https://web.fluxer.app/invite/xyz/?ref=1 ").as_deref(), Some("xyz"));
        assert_eq!(parse_invite("AbCd").as_deref(), Some("AbCd"));
        assert_eq!(parse_invite("not a code"), None);
        assert_eq!(parse_invite(""), None);
    }

    #[test]
    fn expiry_texts() {
        assert_eq!(expiry_text(604_800), "Your invite link expires in 7 days.");
        assert_eq!(expiry_text(3600), "Your invite link expires in 1 hour.");
        assert_eq!(expiry_text(0), "This invite link never expires.");
    }

    #[test]
    fn mute_configs() {
        assert_eq!(mute_config(0, 5), Value::Null);
        assert_eq!(mute_config(900_000, 0), json!({ "selected_time_window": 900_000, "end_time": "1970-01-01T00:15:00.000Z" }));
    }
}
