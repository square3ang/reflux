//! The user area's popout (`UserAreaPopout`: status, custom status, edit profile, switch
//! accounts), `CustomStatusModal`, `AccountSwitcherModal` and `ChangeNicknameModal`.

use reflux_model::Snowflake;
use reflux_store::Store;
use reqwest::Method;
use rune::{El, Tx, col, el, row, text};
use serde_json::{Value, json};

use super::{Intent, ShellAccess, ShellState, pops};
use crate::guild::Then;
use crate::i18n::t;
use crate::icons;
use crate::settings::{Op, ShellModal, input};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Modal, Select, mix};

/// The id of the user-area menu; the user row is keyed with it so the menu anchors there.
pub const MENU: &str = "user-area";
/// `TimeWindowPresets`: (key, label, minutes; `None` = don't clear).
pub const CLEAR_AFTER: [(&str, &str, Option<u64>); 9] = [
    ("15m", "15 minutes", Some(15)),
    ("30m", "30 minutes", Some(30)),
    ("1h", "1 hour", Some(60)),
    ("3h", "3 hours", Some(180)),
    ("4h", "4 hours", Some(240)),
    ("8h", "8 hours", Some(480)),
    ("24h", "24 hours", Some(1440)),
    ("3d", "3 days", Some(4320)),
    ("never", "Don't clear", None),
];
const MAX_STATUS: usize = 128;

/// `UserAreaPopout` as a menu over the user row.
pub fn menu<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !st.popups.is_open(MENU) {
        return;
    }
    use reflux_model::Status as S_;
    let me = store.my_id();
    let cur = store.settings.status;
    let dot = |s: S_| match s {
        S_::Online => Dot::Online,
        S_::Idle => Dot::Idle,
        S_::Dnd => Dot::Dnd,
        _ => Dot::Offline,
    };
    let label = |s: S_| match s {
        S_::Online => t("Online"),
        S_::Idle => t("Idle"),
        S_::Dnd => "Do Not Disturb",
        _ => t("Invisible"),
    };
    // `.actionButton`: 22px icon, 13px/500 label, optional 11px hint and a caret.
    let action = |key: &'static str, icon_el: El, text_: String, hint: Option<String>, chevron: bool| {
        let mut words = col().grow(1.0).min_w(0.0).child(text(text_).font(font(13.0, 500.0).lh(16.25)).color(p.text_primary).truncate());
        if let Some(h) = hint {
            words = words.child(text(h).font(font(11.0, 400.0).lh(13.75)).color(p.text_tertiary).truncate());
        }
        let mut r = row()
            .key(key)
            .w_full()
            .gap(10.0)
            .px(12.0)
            .py(10.0)
            .center_y()
            .hover(p.background_modifier_hover)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .child(row().size(22.0, 22.0).center().child(icon_el))
            .child(words);
        if chevron {
            r = r.child(rune::icon(&icons::CARET_RIGHT_BOLD, 14.0).color(p.text_tertiary));
        }
        r
    };
    let divider = || el().mx(12.0).h(1.0).fill(p.background_header_secondary).opacity(0.3);
    let surface = p.background_secondary_alt;
    let status_row = action(STATUS_ROW, crate::ui::avatar::status_indicator(dot(cur), 14.0, surface, p), label(cur).to_string(), None, true)
        .on_hover_change(move |s: &mut S, inside, _| {
            if inside {
                acc(s).popups.sub = Some(STATUS_ROW.into());
            }
        })
        .on_press(move |s: &mut S| acc(s).popups.sub = Some(STATUS_ROW.into()));
    let accounts = action("user-area-accounts", rune::icon(&icons::USERS_THREE_BOLD, 16.0).color(p.text_secondary), t("Switch accounts").to_string(), None, true)
        .on_hover_change(move |s: &mut S, inside, _| {
            if inside {
                acc(s).popups.sub = None;
            }
        })
        // shortcut: Fluxer's hover submenu (`UserAreaAccountSwitchMenu`) isn't built; the row's
        // click opens the account switcher, as Fluxer's does.
        .on_press(move |s: &mut S| open(acc(s), ShellModal::Accounts));
    let copy = action("user-area-copy-id", rune::icon(&icons::IDENTIFICATION_BADGE_BOLD, 16.0).color(p.text_secondary), t("Copy user ID").to_string(), None, false)
        .on_hover_change(move |s: &mut S, inside, _| {
            if inside {
                acc(s).popups.sub = None;
            }
        })
        .on_press_tx(move |s: &mut S, tx: &mut Tx| {
            acc(s).popups.close();
            tx.copy(me.to_string());
        });
    let group = col().w_full().radius(12.0).clip_rounded(12.0).fill(surface).child(status_row).child(divider()).child(accounts).child(divider()).child(copy);
    let edit = Button::new(t("Edit profile"))
        .icon(&icons::PENCIL_BOLD)
        .fit_container()
        .key("user-area-edit-profile")
        .on_press(move |s: &mut S| acc(s).open_settings(Some("my_profile")))
        .el(p);
    // The custom status, or `.customStatusPlaceholder` ("Set a custom status").
    let has_custom = store.presences.get(&me).and_then(|pr| pr.custom_status.as_ref()).is_some_and(|c| c.text.as_deref().is_some_and(|t| !t.is_empty()) || c.emoji_name.is_some());
    let c = super::profile::card_for(store, st, me, None, 80.0, p);
    let accent = c.accent;
    let placeholder = (!has_custom).then(|| {
        row()
            .gap(3.0)
            .center_y()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .child(rune::icon(&icons::SMILEY, 14.0).color(p.text_tertiary))
            .child(text(t("Set a custom status")).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary))
            .on_press(move |s: &mut S| open(acc(s), ShellModal::CustomStatus))
    });
    let c = super::profile::with_badges(store, st, c, false, p, acc);
    let footer = col().w_full().gap(12.0).px(12.0).pt(8.0).child(group).child(edit);
    // shortcut: a set custom status shows read-only (`CustomStatusDisplay isEditable` edits it on
    // click in Fluxer); change it from the custom status modal.
    let extra: Vec<El> = placeholder.into_iter().collect();
    let body = super::profile::card_body(store, st, c, super::profile::WIDTH, extra, p);
    let card = super::profile::frame(body, accent, super::profile::WIDTH, p).key("user-area-popout").child(footer).modal(true, move |s: &mut S| acc(s).popups.close());
    layer.push(crate::ui::overlay::anchor(card, MENU, crate::ui::Placement::Top, 8.0));
    if st.popups.sub.as_deref() == Some(STATUS_ROW) {
        layer.push(crate::ui::overlay::anchor(status_menu(cur, p, acc), STATUS_ROW, crate::ui::Placement::RightStart, 8.0));
    }
}

const STATUS_ROW: &str = "user-area-status";
use crate::ui::avatar::Status as Dot;

/// `StatusMenu`: the four statuses with their descriptions.
// shortcut: the per-status expiry submenus ("For 15 minutes" …) aren't offered; a status set
// here lasts until changed.
fn status_menu<S: 'static>(cur: reflux_model::Status, p: &Palette, acc: ShellAccess<S>) -> El {
    use reflux_model::Status as S_;
    let mut m = col().w(220.0).padding(6.0).radius(8.0).fill(p.form_surface_background).border(1.0, p.background_modifier_accent).key("user-area-status-menu");
    for (s, dot, label, hint) in [
        (S_::Online, Dot::Online, t("Online"), None),
        (S_::Idle, Dot::Idle, t("Idle"), None),
        (S_::Dnd, Dot::Dnd, "Do Not Disturb", Some(t("You won't receive notifications on desktop"))),
        (S_::Invisible, Dot::Offline, t("Invisible"), Some(t("You'll appear offline"))),
    ] {
        let v = serde_json::to_value(s).unwrap_or(Value::Null);
        let mut words = col().grow(1.0).min_w(0.0).child(text(t(label)).font(font(14.0, 500.0).lh(18.0)).color(p.text_primary));
        if let Some(h) = hint {
            words = words.child(text(h).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary).wrap());
        }
        let _ = cur;
        m = m.child(
            row()
                .w_full()
                .gap(10.0)
                .px(8.0)
                .py(6.0)
                .center_y()
                .radius(4.0)
                .hover(p.background_modifier_hover)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .child(row().size(16.0, 16.0).center().child(crate::ui::avatar::status_indicator(dot, 14.0, p.form_surface_background, p)))
                .child(words)
                .on_press(move |st: &mut S| {
                    let sh = acc(st);
                    sh.popups.close();
                    sh.intents.push(Intent::PatchSettings(json!({ "status": v })));
                }),
        );
    }
    m
}

/// Opens a user-area modal with fresh `m.` fields.
pub fn open(sh: &mut ShellState, m: ShellModal) {
    sh.popups.close();
    sh.close_account_modals();
    sh.account.modal = Some(m);
}

fn close<S: 'static>(acc: ShellAccess<S>) -> impl Fn(&mut S) + Clone + 'static {
    move |s: &mut S| acc(s).close_account_modals()
}

fn error(st: &ShellState, p: &Palette) -> Option<El> {
    st.form.error.as_ref().map(|e| text(e.clone()).font(font(14.0, 400.0).lh(20.0)).color(mix(p.status_danger, p.text_primary, 78.0)).wrap())
}

/// The `custom_status` a save sends: `None` clears it.
pub fn custom_status(text: &str, emoji: Option<&Value>, clear_after: Option<u64>, now_ms: u64) -> Value {
    let text: String = text.trim().chars().take(MAX_STATUS).collect();
    let emoji = emoji.filter(|e| e["emoji_name"].as_str().is_some_and(|n| !n.is_empty()));
    // `buildDraftStatus`: nothing to show clears the status.
    if text.is_empty() && emoji.is_none() {
        return Value::Null;
    }
    let expires = clear_after.map(|m| crate::app::iso8601(now_ms + m * 60_000));
    let mut v = json!({ "text": (!text.is_empty()).then_some(text), "expires_at": expires, "emoji_name": null });
    if let Some(e) = emoji {
        v["emoji_name"] = e["emoji_name"].clone();
        if let Some(id) = e["emoji_id"].as_str() {
            v["emoji_id"] = json!(id);
            v["emoji_animated"] = e["emoji_animated"].clone();
        }
    }
    v
}

/// The draft's emoji: picked in the modal (`Null` = cleared), else the saved status's.
fn draft_emoji(store: &Store, st: &ShellState) -> Option<Value> {
    match st.form.data.get("m.emoji") {
        Some(Value::Null) => None,
        Some(v) => Some(v.clone()),
        None => store.settings.custom_status.as_ref().filter(|c| c.emoji_name.is_some()).map(|c| {
            json!({ "emoji_id": c.emoji_id.map(|i| i.to_string()), "emoji_name": c.emoji_name, "emoji_animated": c.emoji_animated })
        }),
    }
}

/// `CustomStatusModal`: a live profile preview, the text with its emoji button and clear
/// button, then when it clears and "Update my status".
pub fn custom_status_modal<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let initial = store.settings.custom_status.as_ref().and_then(|c| c.text.clone()).unwrap_or_default();
    let typed = st.form.text.get("m.status").map_or(initial.as_str(), |e| e.text.as_str());
    let count = typed.chars().count();
    let emoji = draft_emoji(store, st);
    let emoji_el = |size: f32| {
        let e = emoji.as_ref()?;
        super::forum::forum_emoji(st, e["emoji_id"].as_str().and_then(|i| i.parse().ok()), e["emoji_name"].as_str(), size)
    };
    let exp = st.form.data.get("m.clear").and_then(Value::as_str).unwrap_or("24h").to_string();
    let opts: Vec<(&str, &str)> = CLEAR_AFTER.iter().map(|(k, l, _)| (*k, t(l))).collect();
    let select = Select::new("clear-after", &opts, Some(&exp))
        .width(406.0)
        .on_choose(move |s: &mut S, v| {
            acc(s).form.data.insert("m.clear".into(), Value::String(v.into()));
        })
        .el(&st.popups, pops(acc), p, layer);
    // `ProfilePreview` with the draft as its custom status.
    let me = store.my_id();
    let mut card = super::profile::card_for(store, st, me, None, 80.0, p);
    card.custom_status = Some(typed.trim().to_string()).filter(|s| !s.is_empty());
    card.status_emoji = emoji_el(16.0);
    let preview = row().w_full().justify_center().mb(16.0).child(super::profile::preview(store, st, card, p));
    let picking = st.popups.is_open(super::expr::POPOUT);
    let has_emoji = emoji.is_some();
    let trigger = row()
        .size(32.0, 32.0)
        .center()
        .radius(999.0)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(if has_emoji { t("Change emoji") } else { t("Choose an emoji") })
        .expanded(picking)
        .focusable(true)
        .focus_ring(4.0, 0.0, p.focus_primary)
        .key("status-emoji")
        .hover(p.background_modifier_hover)
        .on_press(move |s: &mut S| super::expr::open_status(acc(s)))
        .child(emoji_el(22.0).unwrap_or_else(|| rune::icon(&icons::SMILEY_FILL, 22.0).color(p.text_primary_muted)));
    let trigger = if picking { trigger.fill(p.background_modifier_hover) } else { trigger };
    let edit = st.form.text.get("m.status").cloned().unwrap_or_else(|| rune::TextEdit::new(&initial));
    let init = initial.clone();
    let mut field = crate::ui::Input::new("form-m.status", &edit, move |s: &mut S| -> &mut rune::TextEdit {
        acc(s).form.text.entry("m.status".into()).or_insert_with(|| rune::TextEdit::new(init.clone()))
    })
    .placeholder(t("What's happening?"))
    .left(trigger);
    if count > 0 || has_emoji {
        // `handleClearDraft`.
        field = field.right(
            row()
                .size(32.0, 32.0)
                .center()
                .radius(999.0)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Clear custom status"))
                .focusable(true)
                .focus_ring(4.0, 0.0, p.focus_primary)
                .hover(p.background_modifier_hover)
                .on_press(move |s: &mut S| {
                    let sh = acc(s);
                    sh.form.text.insert("m.status".into(), rune::TextEdit::new(""));
                    sh.form.data.insert("m.emoji".into(), Value::Null);
                })
                .child(rune::icon(&icons::X_BOLD, 16.0).color(p.text_primary_muted)),
        );
    }
    let mut body = col()
        .w_full()
        .child(preview)
        .child(
            col()
                .w_full()
                .gap(6.0)
                .child(
                    text(format!("{count}/{MAX_STATUS}"))
                        .font(font(12.0, 400.0).lh(16.0))
                        .color(if count > MAX_STATUS { p.status_danger } else { p.text_tertiary })
                        .self_end(),
                )
                .child(field.el(p)),
        );
    if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    let save = move |s: &mut S, store_emoji: Option<Value>| {
        let sh = acc(s);
        let text = sh.form.text.get("m.status").map_or(initial.clone(), |e| e.text.clone());
        let key = sh.form.data.get("m.clear").and_then(Value::as_str).unwrap_or("24h");
        let mins = CLEAR_AFTER.iter().find(|(k, ..)| *k == key).and_then(|(_, _, m)| *m);
        sh.intents.push(Intent::PatchSettings(json!({ "custom_status": custom_status(&text, store_emoji.as_ref(), mins, crate::app::now_ms()) })));
        sh.close_account_modals();
    };
    let footer = col()
        .w_full()
        .gap(12.0)
        .child(col().w_full().gap(6.0).child(text(t("Clear after")).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary)).child(select))
        .child(Button::new(t("Update my status")).fit_container().disabled(count > MAX_STATUS).key("modal-primary").on_press(move |s: &mut S| save(s, emoji.clone())).el(p));
    layer.push(Modal::new(t("Set your status"), close(acc)).body(body).footer(footer).el(p));
    if let Some(picker) = super::expr::status_picker(store, st, p, acc) {
        layer.push(picker);
    }
}

/// `AccountSwitcherModal`: the signed-in account, the others saved on this device, then
/// "Add an account".
pub fn accounts_modal<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let me = store.my_id();
    let entry = |name: &str, sub: String, face: El, actions: El| {
        row()
            .w_full()
            .gap(12.0)
            .padding(12.0)
            .radius(8.0)
            .fill(p.background_tertiary)
            .center_y()
            .child(face)
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(text(name.to_string()).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).truncate())
                    .child(text(sub).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted).truncate()),
            )
            .child(actions)
    };
    let letter = |n: &str| {
        row()
            .size(40.0, 40.0)
            .center()
            .radius(20.0)
            .fill(p.brand_primary)
            .child(text(n.chars().next().unwrap_or('?').to_uppercase().to_string()).font(font(16.0, 600.0)).color(p.brand_primary_fill))
    };
    let mut list = col().w_full().gap(8.0);
    if let Some(c) = &st.account.current {
        let face = super::avatar(store, st, me, 40.0, None, p.background_tertiary, p);
        let mentions: u32 = store
            .channels
            .values()
            .filter(|c| c.kind != reflux_model::ChannelType::GUILD_CATEGORY)
            .map(|c| store.mention_count(c.id))
            .sum();
        let active = row()
            .gap(8.0)
            .children(crate::ui::mention_badge(mentions as u64, true, p))
            .child(text(t("Active")).font(font(12.0, 600.0).lh(16.0)).color(p.status_online));
        list = list.child(entry(&c.display_name, format!("{} · {}", c.username, host(&c.instance)), face, active));
    }
    for (i, a) in st.account.accounts.iter().enumerate() {
        let act = st.account.activity.iter().find(|x| x.slot == a.slot());
        let mentions = act.map_or(0, |x| x.summary.mentions);
        let unread = act.is_some_and(|x| x.summary.unread > 0);
        let state = match act.map(|x| x.state) {
            Some(crate::app::accounts::EngineState::Reconnecting) => Some(t("Reconnecting")),
            Some(crate::app::accounts::EngineState::Offline) => Some(t("Offline")),
            _ => None,
        };
        let sub = match state {
            Some(s) => format!("{} · {} · {s}", a.username, host(&a.instance)),
            None => format!("{} · {}", a.username, host(&a.instance)),
        };
        let mut actions = row().gap(8.0);
        if let Some(b) = crate::ui::mention_badge(mentions as u64, true, p) {
            actions = actions.child(b);
        } else if unread {
            actions = actions.child(row().size(16.0, 16.0).center().child(rune::el().size(8.0, 8.0).radius(4.0).fill(p.text_primary_muted)).label(t("Unread messages")));
        }
        actions = actions
            .child(Button::new(t("Switch")).key(format!("switch-{i}")).on_press(move |s: &mut S| acc(s).intents.push(Intent::Account(Op::SwitchAccount(i)))).el(p))
            .child(
                Button::new(t("Remove"))
                    .variant(ButtonVariant::Secondary)
                    .key(format!("remove-{i}"))
                    .on_press(move |s: &mut S| acc(s).intents.push(Intent::Account(Op::RemoveAccount(i))))
                    .el(p),
            );
        list = list.child(entry(&a.display_name, sub, letter(&a.display_name), actions));
    }
    if st.account.accounts.is_empty() {
        list = list.child(text(t("No other accounts on this device.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted));
    }
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Close")).variant(ButtonVariant::Secondary).on_press(close(acc)).el(p))
        .child(Button::new(t("Add an account")).key("modal-primary").on_press(move |s: &mut S| acc(s).intents.push(Intent::Account(Op::AddAccount))).el(p));
    layer.push(Modal::new(t("Manage accounts"), close(acc)).body(list).footer(footer).el(p));
}

fn host(instance: &str) -> &str {
    instance.trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/')
}

/// `ChangeNicknameModal`: `PATCH /guilds/{g}/members/@me {nick}`; empty resets it.
pub fn nickname_modal<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let me = store.my_id();
    let nick = store.member(g, me).and_then(|m| m.nick.clone()).unwrap_or_default();
    let mut body = col().w_full().gap(16.0).child(input(st, acc, "m.nick", &nick, t("Nickname"), store.me.as_ref().map_or("", |u| u.base.username.as_str()), false, p));
    body = body.child(
        el().child(
            text(t("Reset nickname"))
                .font(font(14.0, 500.0).lh(20.0))
                .color(p.text_link)
                .cursor(rune::Cursor::Pointer)
                .on_press(move |s: &mut S| acc(s).api(Method::PATCH, format!("/guilds/{g}/members/@me"), Some(json!({ "nick": null })), Then::Close)),
        ),
    );
    if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    let save = move |s: &mut S| {
        let sh = acc(s);
        let v = sh.form.text.get("m.nick").map_or(nick.clone(), |e| e.text.trim().to_string());
        let v = if v.is_empty() { Value::Null } else { Value::String(v) };
        sh.api(Method::PATCH, format!("/guilds/{g}/members/@me"), Some(json!({ "nick": v })), Then::Close);
    };
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close(acc)).el(p))
        .child(Button::new(t("Save")).submitting(st.form.busy).key("modal-primary").on_press(save).el(p));
    layer.push(Modal::new(t("Change nickname"), close(acc)).body(body).footer(footer).el(p));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_status_body() {
        assert_eq!(custom_status("  ", None, Some(15), 0), Value::Null);
        let v = custom_status(" hi ", None, Some(15), 0);
        assert_eq!(v["text"], "hi");
        assert_eq!(v["expires_at"], "1970-01-01T00:15:00.000Z");
        assert_eq!(custom_status("x", None, None, 0)["expires_at"], Value::Null);
        assert_eq!(custom_status(&"a".repeat(200), None, None, 0)["text"].as_str().unwrap().len(), MAX_STATUS);
        let e = json!({ "emoji_id": "42", "emoji_name": "blob", "emoji_animated": true });
        let v = custom_status(" ", Some(&e), None, 0);
        assert_eq!((v["text"].clone(), v["emoji_id"].clone(), v["emoji_name"].clone()), (Value::Null, json!("42"), json!("blob")), "an emoji alone is a status");
    }
}
