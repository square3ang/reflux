//! `TermsAcceptanceNagbar` and `TermsAcceptanceModal`: after a policy update, a legal-tone bar
//! asks the person to review it, and agreeing posts `/users/@me/terms-acceptance`.

use reflux_store::Store;
use rune::{Color, El, row, text};

use crate::i18n::{t, tf};
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Modal, rgb};

/// `PolicyConstants`: when each policy last changed (ISO 8601); Fluxer ships both as `null`,
/// which keeps the bar away until a date is set.
const TERMS_LAST_UPDATED: Option<&str> = None;
const PRIVACY_LAST_UPDATED: Option<&str> = None;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Terms,
    Privacy,
    Both,
}

/// `getUpdateKind`, `None` when nothing is outdated.
fn kind_for(terms_at: Option<&str>, privacy_at: Option<&str>, terms: Option<&str>, privacy: Option<&str>) -> Option<Kind> {
    let old = |agreed: Option<&str>, updated: Option<&str>| updated.is_some_and(|u| agreed.is_none_or(|a| a < u));
    match (old(terms_at, terms), old(privacy_at, privacy)) {
        (true, true) => Some(Kind::Both),
        (true, false) => Some(Kind::Terms),
        (false, true) => Some(Kind::Privacy),
        _ => None,
    }
}

fn kind(store: &Store, st: &ShellState) -> Option<Kind> {
    if st.nav.terms_demo || crate::shell::devtools::nagbar(st, "forceTermsAcceptance") == Some(true) {
        return Some(Kind::Both);
    }
    let me = store.me.as_ref()?;
    kind_for(me.terms_agreed_at.as_deref(), me.privacy_agreed_at.as_deref(), TERMS_LAST_UPDATED, PRIVACY_LAST_UPDATED)
}

/// The nagbar: `#4338ca` / white (`NagbarToneKind.LEGAL`).
pub fn nagbar<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<crate::desktop::top_bars::Bar> {
    if crate::shell::devtools::nagbar(st, "forceTermsAcceptance") == Some(false) {
        return None;
    }
    kind(store, st)?;
    let review = Button::new(t("Review")).variant(ButtonVariant::Inverted).size(ButtonSize::SuperCompact).key("nagbar-terms").on_press(move |s: &mut S| acc(s).nav.terms = true).el(p);
    Some(crate::desktop::top_bars::Bar::new(-5.0, false,
        row()
            .w_full()
            .min_h(36.0)
            .py(4.0)
            .px(8.0)
            .gap(12.0)
            .center()
            .fill(rgb(0x43, 0x38, 0xca))
            .child(text(t("We've updated our policies. Review them to continue.")).font(font(14.0, 600.0).lh(20.0)).color(Color::WHITE).wrap().align(rune::Align::Center))
            .child(review)
            .role("status"),
    ))
}

pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some(k) = kind(store, st).filter(|_| st.nav.terms) else { return };
    let close = move |s: &mut S| acc(s).nav.terms = false;
    let what = match k {
        Kind::Terms => t("terms of service").to_string(),
        Kind::Privacy => t("privacy policy").to_string(),
        Kind::Both => format!("{} {} {}", t("terms of service"), t("and"), t("privacy policy")),
    };
    let lead = if k == Kind::Privacy { "We've made significant changes to our {policies}. Review it before continuing to use {productName}." } else { "We've made significant changes to our {policies}. Review them before continuing to use {productName}." };
    let mut body = rune::col().w_full().gap(12.0).child(text(tf(lead, &[("policies", &what), ("productName", "Fluxer")])).font(font(16.0, 400.0).lh(24.0)).color(p.text_primary).wrap());
    if k != Kind::Privacy {
        body = body.child(text(tf("By using {productName}, you agree to the updated terms.", &[("productName", "Fluxer")])).font(font(13.6, 400.0).lh(20.0)).color(p.text_primary).opacity(0.7).wrap());
    }
    let accept = move |s: &mut S| {
        let sh = acc(s);
        sh.nav.terms = false;
        sh.nav.terms_demo = false;
        sh.intents.push(Intent::Fetch { key: String::new(), method: reqwest::Method::POST, path: "/users/@me/terms-acceptance".into(), body: Some(serde_json::json!({})) });
    };
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Close")).variant(ButtonVariant::Secondary).on_press(close).el(p))
        .child(Button::new(if k == Kind::Privacy { t("Got it") } else { t("I agree to the updated terms") }).key("terms-accept").on_press(accept).el(p));
    layer.push(Modal::new(t("Updated policies"), close).body(body).footer(footer).el(p));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outdated_agreements_pick_the_kind() {
        let up = Some("2026-05-01T00:00:00Z");
        assert_eq!(kind_for(None, None, None, None), None, "no dates, no bar");
        assert_eq!(kind_for(Some("2026-01-01T00:00:00Z"), None, up, None), Some(Kind::Terms));
        assert_eq!(kind_for(Some("2026-06-01T00:00:00Z"), None, up, up), Some(Kind::Privacy));
        assert_eq!(kind_for(None, None, up, up), Some(Kind::Both));
    }
}
