//! The content of each settings tab.

use reflux_store::Store;
use rune::{El, col, el, row, text};
use serde_json::json;

use super::SettingsUi;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::Switch;
use crate::i18n::t;

pub fn body<S: 'static>(store: &Store, st: &ShellState, ui: &SettingsUi, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    match ui.tab {
        "my_profile" => super::profile::tab(store, st, ui, p, acc, layer),
        "account_security" if st.desktop.streamer && st.prefs.streamer.hide_personal => super::desktop_tabs::streamer_gate(p, acc),
        "account_security" => super::account::tab(store, st, ui, p, acc),
        "privacy_safety" => super::privacy::tab(store, st, p, acc, layer),
        "devices" => super::privacy::devices(st, p, acc),
        "authorized_apps" => super::privacy::apps(st, p, acc),
        "linked_accounts" => super::privacy::connections(st, p, acc),
        "blocked_users" => section(crate::i18n::t("Blocked users"), super::account::blocked(store, st, p, acc), true, p),
        "appearance" => super::look_tabs::appearance(store, st, p, acc, layer),
        "themes" => super::look_tabs::themes(store, st, p, acc),
        "chat_settings" => super::look_tabs::chat(store, st, p, acc),
        "accessibility" => super::look_tabs::accessibility(store, st, p, acc, layer),
        "advanced_settings" => advanced(store, p, acc),
        "voice_video" => super::voice_tabs::tab(st, p, acc, layer),
        "notifications" => super::look_tabs::notifications(store, st, p, acc),
        "keybinds" => crate::nav::keys::tab(st, p, acc, layer),
        "language" => super::desktop_tabs::language(store, st, p, acc),
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
        .child(text(crate::i18n::t(title)).font(font(18.0, 600.0).lh(23.4)).color(p.text_primary))
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

/// `AdvancedSettingRow` for developer mode, under its "Developer" category.
fn advanced<S: 'static>(store: &Store, p: &Palette, acc: ShellAccess<S>) -> El {
    let on = store.settings.developer_mode;
    let row = row()
        .w_full()
        .py(16.0)
        .gap(12.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(8.0)
                .child(text(t("Developer mode")).font(font(14.0, 600.0).lh(18.9)).color(p.text_primary))
                .child(text(t("Enable developer mode")).font(font(13.0, 400.0).lh(18.2)).color(p.text_secondary)),
        )
        .child(
            Switch::new(on)
                .on_toggle(move |s: &mut S| acc(s).intents.push(Intent::PatchSettings(json!({ "developer_mode": !on }))))
                .el(p),
        )
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.border_color));
    // shortcut: only developer mode; Fluxer's other advanced settings aren't implemented.
    section(t("Developer"), row, true, p)
}
