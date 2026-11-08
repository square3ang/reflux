//! Plutonium and gifts, read-only: the Plutonium tab's `PlutoniumPageLinkCard` plus the
//! subscription's status, the Gifts tab's purchased gifts (`GiftInventoryTab`), and
//! `GiftAcceptModal` for gift links. Buying and managing a plan happen in the browser.
// shortcut: no in-app Plutonium page, checkout, plan changes or gift code redeem form (the web
// app has them); gift cards don't collapse or "Share with a friend".

use reflux_model::PremiumType;
use reflux_store::Store;
use reqwest::Method;
use rune::{El, Tx, col, icon, row};
use crate::ui::mark::marked as text;
use serde_json::Value;

use crate::i18n::{t, tf};
use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::social::{Done, Modal};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Modal as Dialog, spinner};

const PRODUCT: &str = "Plutonium";

fn open_web<S: 'static>(acc: ShellAccess<S>, path: &'static str) -> impl Fn(&mut S) + 'static {
    move |s: &mut S| {
        let sh = acc(s);
        let url = format!("{}{path}", sh.web_base.trim_end_matches('/'));
        sh.intents.push(Intent::Desktop(crate::desktop::Request::OpenUrl(url)));
    }
}

/// A card: icon, title + body, and an action on the right (`PlutoniumPageLinkCard`).
fn card(glyph: El, title: String, body: String, action: El, p: &Palette) -> El {
    row()
        .w_full()
        .padding(16.0)
        .gap(16.0)
        .items_center()
        .radius(12.0)
        .border(1.0, p.background_modifier_accent)
        .fill(p.background_secondary)
        .child(glyph)
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(title).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary))
                .child(text(body).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap()),
        )
        .child(action)
}

/// The account's plan, from `premium_type` / `premium_until`.
fn status(store: &Store, st: &ShellState) -> String {
    let Some(me) = crate::shell::devtools::me(store, st) else { return String::new() };
    match me.premium_type.unwrap_or_default() {
        PremiumType::LIFETIME => tf("You have lifetime {premiumProductName}.", &[("premiumProductName", PRODUCT)]),
        PremiumType::SUBSCRIPTION => match me.premium_until.as_deref() {
            Some(until) => tf("Your {premiumProductName} subscription is active until {date}.", &[("premiumProductName", PRODUCT), ("date", super::account::day(until).as_str())]),
            None => tf("Your {premiumProductName} subscription is active.", &[("premiumProductName", PRODUCT)]),
        },
        _ => tf("You don't have {premiumProductName}.", &[("premiumProductName", PRODUCT)]),
    }
}

pub fn plutonium<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let atom = icon(&icons::ATOM_FILL, 32.0).color(p.plutonium_icon);
    let open = Button::new(tf("Open {premiumProductFullName}", &[("premiumProductFullName", "Fluxer Plutonium")]))
        .size(ButtonSize::Small)
        .key("plutonium-open")
        .on_press(open_web(acc, "/channels/@premium"))
        .el(p);
    let link = card(atom, "Fluxer Plutonium".into(), t("Plans, perks, gifts and your subscription are all on one page.").into(), open, p);
    let manage = Button::new(t("Manage in browser")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).key("plutonium-manage").on_press(open_web(acc, "/channels/@premium")).el(p);
    let sub = card(icon(&icons::CROWN_FILL, 24.0).color(p.text_secondary), t("Subscription").into(), status(store, st), manage, p);
    col().w_full().gap(16.0).child(link).child(sub)
}

/// `getGiftDurationText`.
pub fn duration(g: &Value) -> String {
    let n = g["duration_quantity"].as_u64().unwrap_or(0);
    if n == 0 {
        return tf("Visionary (lifetime {premiumProductName})", &[("premiumProductName", PRODUCT)]);
    }
    let unit = match g["duration_type"].as_str().unwrap_or("months") {
        "days" if n == 1 => "day",
        "days" => "days",
        "weeks" if n == 1 => "week",
        "weeks" => "weeks",
        "years" if n == 1 => "year",
        "years" => "years",
        _ if n == 1 => "month",
        _ => "months",
    };
    format!("{n} {} of {PRODUCT}", t(unit))
}

fn gift_glyph(active: bool) -> El {
    let a = if active { 1.0 } else { 0.5 };
    row()
        .size(48.0, 48.0)
        .center()
        .radius(999.0)
        .linear_gradient(3.0 * std::f32::consts::FRAC_PI_4, [(0.0, crate::ui::rgba(168, 85, 247, a)), (1.0, crate::ui::rgba(236, 72, 153, a))])
        .child(icon(&icons::GIFT_FILL, 24.0).color(rune::Color::WHITE))
}

fn user_name(u: &Value) -> Option<String> {
    u["global_name"].as_str().filter(|s| !s.is_empty()).or(u["username"].as_str()).map(str::to_string)
}

/// `GiftInventoryTab`'s "Purchased gifts" (`GET /users/@me/gifts`).
pub fn gifts<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let list = match super::loaded::<Vec<Value>>(st, "gifts", p) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let intro = text(tf("Gifts you buy show up here. Buy a {premiumProductName} gift on the {premiumProductName} page to share with friends.", &[("premiumProductName", PRODUCT)]))
        .font(font(14.0, 400.0).lh(20.0))
        .color(p.text_primary_muted)
        .wrap();
    let mut out = col().w_full().gap(12.0).child(intro);
    if list.is_empty() {
        out = out.child(
            col()
                .w_full()
                .py(32.0)
                .gap(8.0)
                .items_center()
                .child(icon(&icons::GIFT_FILL, 40.0).color(p.text_tertiary))
                .child(text(t("No gifts yet")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary)),
        );
        return out.child(Button::new(t("Manage in browser")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).key("gifts-browser").on_press(open_web(acc, "/channels/@premium")).el(p));
    }
    let base = st.gift_base.trim_end_matches('/').to_string();
    for g in &list {
        let code = g["code"].as_str().unwrap_or_default().to_string();
        let redeemed = g["redeemed_at"].as_str();
        let when = match redeemed {
            Some(at) => tf("Redeemed {date}", &[("date", super::account::day(at).as_str())]),
            None => tf("Purchased {date}", &[("date", super::account::day(g["created_at"].as_str().unwrap_or_default()).as_str())]),
        };
        let mut c = col()
            .key(format!("gift-{code}"))
            .w_full()
            .padding(16.0)
            .gap(12.0)
            .radius(8.0)
            .fill(p.background_secondary)
            .child(
                row()
                    .w_full()
                    .gap(12.0)
                    .items_center()
                    .child(gift_glyph(redeemed.is_none()))
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .child(text(duration(g)).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).truncate())
                            .child(text(when).font(font(13.0, 400.0).lh(18.0)).color(p.text_tertiary)),
                    ),
            );
        c = if redeemed.is_some() {
            let by = user_name(&g["redeemed_by"]).map_or_else(|| t("This gift has been redeemed").to_string(), |n| tf("Redeemed by {redeemerTag}", &[("redeemerTag", &n)]));
            c.child(text(by).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted))
        } else {
            let url = format!("{base}/{code}");
            let shown = url.clone();
            let copy = Button::<S>::new(t("Copy")).size(ButtonSize::Compact).key(format!("gift-copy-{code}")).el(p).on_press_tx(move |_: &mut S, tx: &mut Tx| tx.copy(url.clone()));
            c.child(
                row()
                    .w_full()
                    .min_h(40.0)
                    .pl(12.0)
                    .pr(4.0)
                    .gap(8.0)
                    .items_center()
                    .radius(8.0)
                    .fill(p.form_surface_background)
                    .child(text(shown).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).truncate().grow(1.0).min_w(0.0))
                    .child(copy),
            )
        };
        out = out.child(c);
    }
    out
}

/// Opens `GiftAcceptModal` and loads the gift.
pub fn open_gift(st: &mut ShellState, code: String) {
    st.open_social(Modal::GiftAccept(code.clone()));
    st.social(Method::GET, format!("/gifts/{code}"), None, Done::Form("gift".into()));
}

/// `GiftAcceptModal`: loading, unknown, already claimed, or "Claim gift".
pub fn gift_accept<S: 'static>(st: &ShellState, code: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).dialog = None;
    let gift = st.form.data.get("gift");
    let line = |glyph: El, title: String, title_color, from: Option<String>, help: String| {
        row()
            .w_full()
            .gap(12.0)
            .items_center()
            .child(glyph)
            .child(
                col()
                    .min_w(0.0)
                    .child(text(title).font(font(16.0, 600.0).lh(20.0)).color(title_color).truncate())
                    .children(from.map(|f| text(f).font(font(12.8, 400.0).lh(16.0)).color(p.text_secondary)))
                    .child(text(help).font(font(12.8, 400.0).lh(16.0)).color(p.text_tertiary).mt(2.0).wrap()),
            )
    };
    let dismiss = || Button::new(t("Close")).variant(ButtonVariant::Secondary).on_press(close).el(p);
    let (content, footer) = match gift {
        None if st.form.error.is_none() => (row().key("loading").w_full().py(16.0).center().child(spinner(p.text_primary_muted)), None),
        None => {
            let q = row().size(48.0, 48.0).center().radius(999.0).fill(p.background_tertiary).child(icon(&icons::QUESTION_FILL, 24.0).color(p.text_tertiary));
            (line(q, t("Unknown gift").into(), p.status_danger, None, t("This gift code is invalid or already claimed.").into()), Some(dismiss()))
        }
        Some(g) => {
            let from = user_name(&g["created_by"]).map(|n| tf("From {creatorTag}", &[("creatorTag", &n)]));
            if g["redeemed"].as_bool() == Some(true) {
                (line(gift_glyph(false), duration(g), p.text_tertiary, from, t("This gift has already been claimed.").into()), Some(dismiss()))
            } else {
                let help = tf("Claim your gift to activate your {premiumProductName} subscription!", &[("premiumProductName", PRODUCT)]);
                let code = code.to_string();
                let claim = Button::new(t("Claim gift")).submitting(st.form.busy).key("gift-claim").on_press(move |s: &mut S| {
                    acc(s).social(Method::POST, format!("/gifts/{code}/redeem"), None, Done::Toast("Gift redeemed!".into()))
                });
                let later = Button::new(t("Maybe later")).variant(ButtonVariant::Secondary).disabled(st.form.busy).on_press(close).el(p);
                let mut items = line(gift_glyph(true), duration(g), p.text_primary, from, help);
                if let Some(e) = &st.form.error {
                    items = col().key("with-error").w_full().gap(12.0).child(items).child(text(e.clone()).font(font(14.0, 400.0).lh(20.0)).color(p.status_danger).wrap());
                }
                (items, Some(row().gap(8.0).child(later).child(claim.el(p))))
            }
        }
    };
    let mut d = Dialog::new(t("Gift"), close).body(content);
    if let Some(f) = footer {
        d = d.footer(f);
    }
    d.el(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn gift_durations() {
        assert_eq!(duration(&json!({"duration_type": "months", "duration_quantity": 1})), "1 month of Plutonium");
        assert_eq!(duration(&json!({"duration_type": "years", "duration_quantity": 2})), "2 years of Plutonium");
        assert_eq!(duration(&json!({"duration_type": "days", "duration_quantity": 0})), "Visionary (lifetime Plutonium)");
    }
}
