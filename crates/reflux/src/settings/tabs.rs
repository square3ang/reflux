//! The content of each settings tab.

use reflux_store::Store;
use rune::{El, col, el};
use crate::ui::mark::marked as text;

use super::SettingsUi;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};

/// Tab `tab`'s content (the open tab, or one shown inline in search results).
pub fn body<S: 'static>(store: &Store, st: &ShellState, ui: &SettingsUi, tab: &str, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    match tab {
        "my_profile" => super::profile::tab(store, st, ui, p, acc, layer),
        "account_security" | "linked_accounts" if st.desktop.streamer && st.prefs.streamer.hide_personal => super::desktop_tabs::streamer_gate(p, acc),
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
        "advanced_settings" => super::advanced::tab(store, st, &ui.searched, p, acc),
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

/// `SettingsSection`: title 18/600, content gap 20; later sections get a top border and 24 padding.
pub(crate) fn section(title: &str, content: El, first: bool, p: &Palette) -> El {
    let mut s = col()
        .w_full()
        .gap(16.0)
        .pb(24.0)
        .child(crate::ui::mark::text(text(crate::i18n::t(title)).font(font(18.0, 600.0).lh(23.4)).color(p.text_primary), crate::i18n::t(title)))
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

