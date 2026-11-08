//! The content of each settings tab.

use reflux_store::Store;
use rune::{El, col, el, icon, row};
use crate::ui::mark::marked as text;

use super::SettingsUi;
use crate::i18n::t;
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};

/// Tab `tab`'s content (the open tab, or one shown inline in search results).
pub fn body<S: 'static>(store: &Store, st: &ShellState, ui: &SettingsUi, tab: &str, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    match tab {
        "my_profile" => super::profile::tab(store, st, ui, p, acc, layer),
        "account_security" | "linked_accounts" if st.desktop.streamer && st.prefs.streamer.hide_personal => super::desktop_tabs::streamer_gate(p, acc).key("streamer-gate"),
        "account_security" => super::account::tab(store, st, ui, p, acc),
        "privacy_safety" => super::privacy::tab(store, st, p, acc, layer),
        "devices" => super::privacy::devices(st, p, acc),
        "authorized_apps" => super::privacy::apps(st, p, acc),
        "linked_accounts" => super::privacy::connections(st, p, acc),
        "blocked_users" => section(crate::i18n::t("Blocked users"), super::account::blocked(store, st, p, acc), true, p),
        "appearance" => super::look_tabs::appearance(store, st, p, acc, layer),
        "themes" => super::look_tabs::themes(store, st, p, acc, layer),
        "chat_settings" => super::look_tabs::chat(store, st, p, acc),
        "accessibility" => super::look_tabs::accessibility(store, st, p, acc, layer),
        "advanced_settings" => super::advanced::tab(store, st, &ui.searched, p, acc, layer),
        "voice_video" => super::voice_tabs::tab(store, st, p, acc, layer),
        "notifications" => super::look_tabs::notifications(store, st, p, acc),
        "keybinds" => crate::nav::keys::tab(st, p, acc, layer),
        "language" => super::desktop_tabs::language(store, st, p, acc),
        "plutonium" => super::premium::plutonium(store, st, p, acc),
        "gift_inventory" => section(crate::i18n::t("Purchased gifts"), super::premium::gifts(st, p, acc), true, p),
        "desktop_settings" => super::desktop_tabs::desktop(st, p, acc),
        "applications" => super::dev::applications(st, p, acc, layer),
        "embed_debugger" => super::dev::embed_debugger(store, st, p, acc),
        "component_gallery" => {
            let (body, mut over) = crate::ui::gallery::parts(&st.nav.gallery, true);
            layer.append(&mut over);
            body
        }
        _ => not_available(p),
    }
}

thread_local! {
    /// The tab being rendered, for the section deep links (`buildUserSettingsDeepLink`).
    static TAB: std::cell::Cell<&'static str> = const { std::cell::Cell::new("") };
}

pub(crate) fn set_current_tab(tab: &'static str) {
    TAB.with(|t| t.set(tab));
}

/// `buildUserSettingsDeepLink`: `fluxer://settings/user?tab=…[&section=…]`.
pub(crate) fn deep_link(tab: &str, section: Option<&str>) -> String {
    match section {
        Some(s) => format!("fluxer://settings/user?tab={tab}&section={s}"),
        None => format!("fluxer://settings/user?tab={tab}"),
    }
}

/// `SettingsHeadingLinkButton`: a 28px link that copies `href`, turning into a check for two
/// seconds (`COPY_LINK_TO_PAGE` / `COPY_LINK_TO_SECTION`).
pub(crate) fn link_button(href: String, page: bool, p: &Palette) -> El {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static COPIED: Mutex<Option<(String, Instant)>> = Mutex::new(None);
    let copied = COPIED.lock().unwrap().as_ref().is_some_and(|(h, at)| *h == href && at.elapsed() < Duration::from_secs(2));
    let label = if copied { t("Copied") } else if page { t("Copy link to page") } else { t("Copy link to section") };
    let press = href.clone();
    row()
        .size(28.0, 28.0)
        .center()
        .radius(6.0)
        .role("button")
        .label(label.to_string())
        .cursor(rune::Cursor::Pointer)
        .hover_group()
        .on_press_tx(move |_: &mut crate::State, tx: &mut rune::Tx| {
            tx.copy(press.clone());
            *COPIED.lock().unwrap() = Some((press.clone(), Instant::now()));
            tx.wake_after(Duration::from_secs(2));
        })
        .child(icon(if copied { &icons::CHECK_BOLD } else { &icons::LINK_BOLD }, 15.0).color(p.text_tertiary).group_color(p.text_primary))
}

/// The section's deep-link id: Fluxer's ids are the lowercased title (`audio`, `video`, …).
fn section_id(title: &str) -> String {
    title.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect::<String>().trim_matches('_').to_string()
}

/// `SettingsSection`: title 18/600 with its deep-link button, content gap 20; later sections get a
/// top border and 24 padding.
pub(crate) fn section(title: &str, content: El, first: bool, p: &Palette) -> El {
    // Keyed by title: tabs add sections conditionally, which would shift the ones after them.
    let link = deep_link(TAB.with(|t| t.get()), Some(&section_id(crate::i18n::t(title))));
    let mut s = col()
        .key(format!("sec-{title}"))
        .w_full()
        .gap(16.0)
        .pb(24.0)
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(crate::ui::mark::text(text(crate::i18n::t(title)).font(font(18.0, 600.0).lh(23.4)).color(p.text_primary), crate::i18n::t(title)))
                .child(link_button(link, false, p)),
        )
        .child(col().w_full().gap(20.0).child(content));
    if !first {
        s = s.pt(24.0).child(el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(1.0).fill(p.background_modifier_accent));
    }
    s
}

pub(crate) fn not_available(p: &Palette) -> El {
    col()
        .w_full()
        .py(48.0)
        .gap(8.0)
        .items_center()
        .child(text("Not available yet").font(font(16.0, 600.0).lh(20.0)).color(p.text_primary))
        .child(
            text("Reflux doesn't support this part of settings yet.")
                .font(font(14.0, 400.0).lh(20.0))
                .color(p.text_primary_muted),
        )
}


#[cfg(test)]
mod tests {
    /// `buildUserSettingsDeepLink`: the tab, and the section when one is named.
    #[test]
    fn section_deep_links_match_fluxer() {
        assert_eq!(super::deep_link("voice_video", None), "fluxer://settings/user?tab=voice_video");
        assert_eq!(super::deep_link("voice_video", Some("audio")), "fluxer://settings/user?tab=voice_video&section=audio");
        assert_eq!(super::section_id("Audio"), "audio");
        assert_eq!(super::section_id("Video"), "video");
    }
}
