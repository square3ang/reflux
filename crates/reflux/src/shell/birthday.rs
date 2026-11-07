//! `DateOfBirthGate`: everyone without a date of birth on file enters one before using the app.
//! The modal can't be dismissed; a date below the minimum age removes the account
//! (`ACCOUNT_BELOW_MINIMUM_AGE`), which the app answers by signing out (`crate::app`).

use rune::{Align, Color, El, Shadow, col, el, row, text};

use super::{Intent, ShellAccess, ShellState, pops};
use crate::i18n::{t, tf};
use crate::login::Dob;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Modal, over, rgba};
use reflux_store::Store;

#[derive(Default)]
pub struct Gate {
    pub dob: Dob,
    pub submitting: bool,
    pub error: Option<String>,
    /// The `LogoutModal` over the gate.
    pub confirm_logout: bool,
}

pub fn view<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !store.me.as_ref().is_some_and(|u| u.date_of_birth_required) {
        return;
    }
    let g = &st.birthday;
    let today = chrono::Local::now().date_naive();
    let iso = g.dob.iso(today);
    let title = t("When is your birthday?");
    let mut desc = tf(
        "{productName} only asks for your date of birth, never for ID or a face scan. It's used to check the minimum age for your country and for age-restricted content, and it isn't shown to anyone. You can't change it later, so please make sure it's right.",
        &[("productName", "Fluxer")],
    );
    // "Why we ask": the help article (`HelpCenterArticleSlug.AgeVerification`), not on self-hosted instances.
    let why = st.marketing_base.as_ref().map(|b| format!("{}/help/age-verification#date-of-birth", b.trim_end_matches('/')));
    let mut spans = Vec::new();
    let desc_el = |desc: String, spans: Vec<rune::Span>| rune::rich(desc, spans).w_full().align(Align::Center).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap();
    let desc = match &why {
        Some(url) => {
            desc.push(' ');
            let start = desc.len();
            desc.push_str(t("Why we ask"));
            spans.push(rune::Span::new(start, desc.len()).color(p.text_link));
            desc_el(desc.clone(), spans).link(rune::Link::new(start..desc.len(), url.clone())).on_link(|_: &mut S, url: &str, tx: &mut rune::Tx| drop(tx.open_url(url)))
        }
        None => desc_el(desc, spans),
    };
    let mut select_layer = Vec::new();
    let field = crate::login::date_of_birth("birthday-dob", &g.dob, g.error.clone(), 374.0, &st.popups, pops(acc), move |s: &mut S| &mut acc(s).birthday.dob, p, &mut select_layer);
    let submit = Button::new(t("Continue")).fit_container().submitting(g.submitting).disabled(iso.is_none()).key("birthday-continue").on_press(move |s: &mut S| {
        let sh = acc(s);
        if let Some(d) = sh.birthday.dob.iso(chrono::Local::now().date_naive()).filter(|_| !sh.birthday.submitting) {
            sh.birthday.submitting = true;
            sh.birthday.error = None;
            sh.intents.push(Intent::SetDateOfBirth(d));
        }
    });
    let sign_out = text(t("Sign out"))
        .key("birthday-sign-out")
        .py(2.0)
        .px(4.0)
        .font(font(14.0, 600.0).lh(20.0))
        .color(p.text_link)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .on_press(move |s: &mut S| acc(s).birthday.confirm_logout = true);
    // `.form`: padding 32, gap 18; title 20/600, description 14/20 secondary, both centred.
    let bg = p.background_secondary;
    let card = col()
        .w(440.0)
        .padding(33.0)
        .gap(18.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_header_secondary, bg))
        .box_shadow(Shadow { x: 0.0, y: 8.0, blur: 24.0, spread: -4.0, color: rgba(0, 0, 0, 0.25), inset: false })
        .role("dialog")
        .label(title)
        .child(text(title).w_full().align(Align::Center).font(font(20.0, 600.0).lh(26.0)).color(p.text_primary))
        .child(desc)
        .child(field)
        .child(submit.el(p).default_button())
        .child(row().w_full().justify_center().child(sign_out));
    // `backdropTone="strong"`, and no way to dismiss it.
    layer.push(
        el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(Color::BLACK).opacity(0.9))
            .child(col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).padding(16.0).center().child(card.modal(true, |_: &mut S| {}))),
    );
    layer.extend(select_layer);
    if g.confirm_logout {
        layer.push(logout_modal(store, st, p, acc));
    }
}

/// `LogoutModal`: an unclaimed account (they're the ones asked for a birthday) can't sign back in.
fn logout_modal<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let cancel = move |s: &mut S| acc(s).birthday.confirm_logout = false;
    let unclaimed = super::devtools::me(store, st).is_some_and(|m| m.account_tier.as_deref() == Some("unclaimed"));
    let (body, primary, variant) = if unclaimed {
        (t("This account is unclaimed, so you can't sign back in to it."), t("Sign out anyway"), ButtonVariant::Danger)
    } else {
        (t("You can sign back in at any time."), t("Sign out"), ButtonVariant::Primary)
    };
    Modal::new(t("Sign out?"), cancel)
        .body(text(body).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap())
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(t("Stay signed in")).variant(ButtonVariant::Secondary).key("birthday-stay").on_press(cancel).el(p))
                .child(Button::new(primary).variant(variant).key("birthday-sign-out-confirm").on_press(move |s: &mut S| acc(s).intents.push(Intent::Logout)).el(p)),
        )
        .el(p)
}
