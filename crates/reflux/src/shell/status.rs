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
use crate::ui::{Button, ButtonVariant, Menu, MenuItem, Modal, Select, mix};

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
    let cur = store.settings.status;
    let status = |id: &str, label: &str, hint: Option<&str>, s: S_| {
        let v = serde_json::to_value(s).unwrap_or(Value::Null);
        let mut i = MenuItem::new(format!("status-{id}"), t(label)).radio(cur == s).on_select(move |st: &mut S| {
            let sh = acc(st);
            sh.popups.close();
            sh.intents.push(Intent::PatchSettings(json!({ "status": v })));
        });
        if let Some(h) = hint {
            i = i.hint(t(h));
        }
        i
    };
    let me = store.my_id();
    let custom = store.settings.custom_status.as_ref().and_then(|c| c.text.clone()).filter(|t| !t.is_empty());
    let mut m = Menu::new(MENU)
        .placement(crate::ui::Placement::Top)
        .min_w(320.0)
        .item(status("online", t("Online"), None, S_::Online))
        .item(status("idle", t("Idle"), None, S_::Idle))
        .item(status("dnd", "Do Not Disturb", Some(t("You won't receive notifications on desktop")), S_::Dnd))
        .item(status("invisible", t("Invisible"), Some(t("You'll appear offline")), S_::Invisible))
        .separator()
        .item(
            MenuItem::new("custom-status", custom.clone().unwrap_or_else(|| t("Set a custom status").into()))
                .icon(&icons::SMILEY)
                .on_select(move |s: &mut S| open(acc(s), ShellModal::CustomStatus)),
        );
    if custom.is_some() {
        m = m.item(
            MenuItem::new("clear-status", t("Clear custom status"))
                .icon(&icons::X_BOLD)
                .on_select(move |s: &mut S| acc(s).intents.push(Intent::PatchSettings(json!({ "custom_status": null })))),
        );
    }
    m = m
        .separator()
        .item(MenuItem::new("edit-profile", t("Edit profile")).icon(&icons::PENCIL_SIMPLE).on_select(move |s: &mut S| acc(s).open_settings(Some("my_profile"))))
        .item(MenuItem::new("switch-accounts", t("Switch accounts")).icon(&icons::USERS).on_select(move |s: &mut S| open(acc(s), ShellModal::Accounts)))
        .separator()
        .item(MenuItem::new("copy-me", t("Copy user ID")).icon(&icons::SNOWFLAKE).on_select_tx(move |_: &mut S, tx: &mut Tx| tx.copy(me.to_string())));
    m.el(&st.popups, pops(acc), p, layer);
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
pub fn custom_status(text: &str, clear_after: Option<u64>, now_ms: u64) -> Value {
    let text: String = text.trim().chars().take(MAX_STATUS).collect();
    if text.is_empty() {
        return Value::Null;
    }
    let expires = clear_after.map(|m| crate::app::iso8601(now_ms + m * 60_000));
    json!({ "text": text, "expires_at": expires, "emoji_name": null })
}

/// `CustomStatusModal`: the text, when it clears, then "Update my status".
// shortcut: no emoji picker or live profile preview; an existing status emoji is dropped on save.
pub fn custom_status_modal<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let initial = store.settings.custom_status.as_ref().and_then(|c| c.text.clone()).unwrap_or_default();
    let typed = st.form.text.get("m.status").map_or(initial.as_str(), |e| e.text.as_str());
    let count = typed.chars().count();
    let exp = st.form.data.get("m.clear").and_then(Value::as_str).unwrap_or("24h").to_string();
    let opts: Vec<(&str, &str)> = CLEAR_AFTER.iter().map(|(k, l, _)| (*k, t(l))).collect();
    let select = Select::new("clear-after", &opts, Some(&exp))
        .label(t("Clear after"))
        .width(408.0)
        .on_choose(move |s: &mut S, v| {
            acc(s).form.data.insert("m.clear".into(), Value::String(v.into()));
        })
        .el(&st.popups, pops(acc), p, layer);
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(
            col()
                .w_full()
                .gap(6.0)
                .child(input(st, acc, "m.status", &initial, "", t("What's happening?"), false, p))
                .child(
                    text(format!("{count}/{MAX_STATUS}"))
                        .font(font(12.0, 400.0).lh(16.0))
                        .color(if count > MAX_STATUS { p.status_danger } else { p.text_tertiary })
                        .self_end(),
                ),
        )
        .child(select);
    if let Some(e) = error(st, p) {
        body = body.child(e);
    }
    let save = move |s: &mut S| {
        let sh = acc(s);
        let text = sh.form.text.get("m.status").map_or(initial.clone(), |e| e.text.clone());
        let key = sh.form.data.get("m.clear").and_then(Value::as_str).unwrap_or("24h");
        let mins = CLEAR_AFTER.iter().find(|(k, ..)| *k == key).and_then(|(_, _, m)| *m);
        sh.intents.push(Intent::PatchSettings(json!({ "custom_status": custom_status(&text, mins, crate::app::now_ms()) })));
        sh.close_account_modals();
    };
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close(acc)).el(p))
        .child(Button::new(t("Update my status")).disabled(count > MAX_STATUS).key("modal-primary").on_press(save).el(p));
    layer.push(Modal::new(t("Set your status"), close(acc)).body(body).footer(footer).el(p));
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
        let active = text(t("Active")).font(font(12.0, 600.0).lh(16.0)).color(p.status_online);
        list = list.child(entry(&c.display_name, format!("{} · {}", c.username, host(&c.instance)), face, active));
    }
    for (i, a) in st.account.accounts.iter().enumerate() {
        let actions = row()
            .gap(8.0)
            .child(Button::new(t("Switch")).key(format!("switch-{i}")).on_press(move |s: &mut S| acc(s).intents.push(Intent::Account(Op::SwitchAccount(i)))).el(p))
            .child(
                Button::new(t("Remove"))
                    .variant(ButtonVariant::Secondary)
                    .key(format!("remove-{i}"))
                    .on_press(move |s: &mut S| acc(s).intents.push(Intent::Account(Op::RemoveAccount(i))))
                    .el(p),
            );
        list = list.child(entry(&a.display_name, format!("{} · {}", a.username, host(&a.instance)), letter(&a.display_name), actions));
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
        assert_eq!(custom_status("  ", Some(15), 0), Value::Null);
        let v = custom_status(" hi ", Some(15), 0);
        assert_eq!(v["text"], "hi");
        assert_eq!(v["expires_at"], "1970-01-01T00:15:00.000Z");
        assert_eq!(custom_status("x", None, 0)["expires_at"], Value::Null);
        assert_eq!(custom_status(&"a".repeat(200), None, 0)["text"].as_str().unwrap().len(), MAX_STATUS);
    }
}
