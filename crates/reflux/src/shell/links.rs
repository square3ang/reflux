//! Opening links: `ExternalLinkWarningModal` before untrusted sites, and the trusted domain
//! rules (`features/trusted_domain/state/TrustedDomain.ts`).

use rune::{El, Tx, col, icon, row, text};

use super::{Intent, ShellAccess, ShellState};
use crate::icons;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Checkbox, Modal, rgb};
use crate::i18n::t;

/// `BUILT_IN_TRUST_PATTERNS`.
const BUILT_IN: [&str; 10] =
    ["fluxer.app", "*.fluxer.app", "fluxer.com", "*.fluxer.com", "fluxer.dev", "*.fluxer.dev", "fluxerstatus.com", "*.fluxerstatus.com", "fluxer.gg", "fluxer.gift"];

/// `TRUST_EVERYTHING_PATTERN` in the user's `trusted_domains`.
const EVERYTHING: &str = "*";

fn comparable(host: &str) -> String {
    host.trim().to_ascii_lowercase().trim_end_matches('.').to_owned()
}

/// `coversHost`: `*.example.com` covers the domain and its subdomains.
fn covers(pattern: &str, host: &str) -> bool {
    let p = comparable(pattern);
    match p.strip_prefix("*.") {
        _ if p == EVERYTHING => true,
        Some(base) => host == base || host.ends_with(&format!(".{base}")),
        None => host == p,
    }
}

pub fn host(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(comparable)
}

/// `isTrustedDomain`: built-in Fluxer domains, the instance's own endpoints, and what the user
/// trusted (or `*` for everything).
pub fn is_trusted(url: &str, user: &[String], instance: &[&str]) -> bool {
    let Some(h) = host(url) else { return false };
    let own = instance.iter().filter_map(|u| host(u));
    BUILT_IN.iter().map(|s| s.to_string()).chain(own).chain(user.iter().cloned()).any(|p| covers(&p, &h))
}

/// `openExternalUrlWithWarning`: trusted URLs open at once, others ask first.
pub fn open(st: &mut ShellState, url: &str, tx: &mut Tx) {
    let media = st.media.base();
    if is_trusted(url, &st.media_ui.trusted, &[&st.web_base, &st.invite_base, &media]) {
        let _ = tx.open_url(url);
    } else {
        st.media_ui.link_warning = Some((url.to_owned(), false));
    }
}

/// The warning dialog for `st.link_warning`.
pub fn modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let (url, trust) = st.media_ui.link_warning.clone()?;
    let close = move |s: &mut S| acc(s).media_ui.link_warning = None;
    let product = "Fluxer";
    let head = col()
        .items_center()
        .gap(12.0)
        .child(row().size(48.0, 48.0).center().radius(24.0).fill(p.background_modifier_accent).child(icon(&icons::WARNING_FILL, 24.0).color(rgb(0xea, 0xb3, 0x08))))
        .child(
            col()
                .items_center()
                .child(text(crate::i18n::tf("You are about to leave {productName}", &[("productName", product)])).font(font(16.0, 600.0).lh(24.0)).color(p.text_primary))
                .child(text(t("External links can be dangerous. Be careful.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).mt(4.0)),
        );
    let dest = col()
        .w_full()
        .gap(8.0)
        .child(text(t("Destination URL:")).font(font(14.0, 500.0).lh(20.0)).color(p.text_secondary))
        .child(
            col()
                .w_full()
                .radius(8.0)
                .border(1.0, crate::ui::over(p.background_modifier_accent, p.background_tertiary))
                .fill(p.background_tertiary)
                .py(12.0)
                .px(16.0)
                .child(text(url.clone()).font(font(14.0, 400.0).mono().lh(20.0)).color(p.text_primary).wrap_anywhere()),
        );
    let check = Checkbox::new(trust)
        .size(18.0)
        .label("skip this warning next time")
        .on_toggle(move |s: &mut S| {
            if let Some(w) = &mut acc(s).media_ui.link_warning {
                w.1 = !w.1;
            }
        })
        .el(p);
    let body = col().w_full().gap(24.0).pb(24.0).child(head).child(dest).child(check);
    let visit = move |s: &mut S, tx: &mut Tx| {
        let st = acc(s);
        let Some((url, trust)) = st.media_ui.link_warning.take() else { return };
        // `addTrustedDomain`: the hostname, unless everything is trusted already.
        let mut domains = st.media_ui.trusted.clone();
        if let Some(h) = host(&url).filter(|h| trust && !domains.iter().any(|d| d == EVERYTHING || d == h)) {
            domains.push(h);
            st.media_ui.trusted = domains.clone();
            st.intents.push(Intent::PatchSettings(serde_json::json!({ "trusted_domains": domains })));
        }
        let _ = tx.open_url(&url);
    };
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("link-cancel").on_press(close).el(p))
        .child(
            Button::new(t("Visit site"))
                .icon_right(&icons::ARROW_RIGHT_BOLD)
                .key("link-visit")
                .on_press_tx(visit)
                .el(p),
        );
    Some(Modal::new(t("External link warning"), close).body(body).footer(footer).el(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_follows_fluxer_patterns() {
        let none: [String; 0] = [];
        assert!(is_trusted("https://web.fluxer.app/x", &none, &[]));
        assert!(is_trusted("https://fluxer.gg/abc", &none, &[]));
        assert!(!is_trusted("https://evil.fluxer.gg/abc", &none, &[]), "fluxer.gg has no wildcard");
        assert!(!is_trusted("https://example.com", &none, &[]));
        assert!(is_trusted("https://chat.example.org/a", &none, &["https://chat.example.org"]), "instance endpoints");
        assert!(is_trusted("https://Docs.Example.com./a", &["*.example.com".into()], &[]));
        assert!(is_trusted("https://anything.test", &["*".into()], &[]));
        assert!(!is_trusted("not a url", &["*".into()], &[]));
    }
}
