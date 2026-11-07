//! The Advanced tab (`AdvancedSettingsTab.tsx`): every advanced-audience item from the search
//! index, grouped by category (`AdvancedSettingsCategories.ts`), each with its direct control
//! (`AdvancedSettingDirectControls.tsx`) or an "Open" button to the tab it lives in.
// shortcut: no "New settings" section, no in-tab search field (the settings search filters this
// tab), and the action bar's options sit inline instead of in a configure dialog. Items whose
// feature Reflux doesn't have are left out.

use reflux_store::Store;
use rune::{El, col, el, row, text};
use serde_json::json;

use super::search_index::ITEMS;
use super::tabs::section;
use crate::desktop::Prefs;
use crate::i18n::t;
use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{ButtonVariant, Switch};

/// `ADVANCED_SETTINGS_TAG_ORDER` with `ADVANCED_SETTINGS_TAG_LABELS`.
const CATEGORIES: &[(&str, &str)] = &[
    ("account", "Account"),
    ("privacy", "Privacy"),
    ("appearance", "Appearance"),
    ("accessibility", "Accessibility"),
    ("chat", "Chat"),
    ("media", "Media"),
    ("voice", "Voice"),
    ("notifications", "Notifications"),
    ("desktop", "Desktop"),
    ("developer", "Developer"),
];

/// Fluxer controls these from this tab; Reflux has the ones listed in [`control`].
const DIRECT: &[&str] = &[
    "advanced-windows-fluxer-sans", "accessibility-text-selection", "accessibility-video-seek-thumbnails", "accessibility-smooth-scrolling",
    "accessibility-middle-click-autoscroll", "appearance-show-neko", "appearance-keep-neko-still", "appearance-hide-keyboard-hints",
    "appearance-voice-channel-join-behavior", "appearance-enable-favorites", "appearance-show-help-center-button", "appearance-show-download-button",
    "chat-settings-auto-send-gifs", "chat-settings-save-gif-favorites", "chat-settings-message-action-bar", "chat-settings-media-buttons",
    "chat-settings-strip-tracking", "chat-settings-trust-domains", "chat-settings-search-engines", "chat-settings-translators",
    "chat-settings-reverse-image-search", "chat-settings-expression-autocomplete", "chat-settings-input-buttons", "chat-settings-convert-emoticons",
    "chat-settings-keep-attachments-on-empty-edit", "chat-settings-preupload-attachments", "chat-settings-sequential-file-send",
    "chat-settings-scroll-to-bottom-on-send", "chat-settings-skip-mark-all-as-read-confirmation", "chat-settings-double-click-message-actions",
    "chat-settings-hide-muted-channels", "voice-video-new-device-alerts", "voice-video-connection-volume-controls", "voice-video-screen-share-codec",
    "voice-video-screen-share-av1-opt-in", "voice-video-screen-share-hevc-opt-in", "voice-video-screen-share-preview-behavior",
    "voice-video-screen-share-encoder-controls", "advanced-unread-badge-customization", "advanced-expression-clone-shortcuts",
    "advanced-hide-blocked-messages", "client-developer-mode", "accessibility-stay-interactive-unfocused", "advanced-native-title-bar",
    "advanced-hardware-acceleration", "advanced-automatic-updates", "desktop-source-maps",
];

/// Settings Reflux has no such feature for, so their "Open" would lead nowhere.
const MISSING: &[&str] = &["advanced-minimize-to-tray"];

/// `getAdvancedSettingsCategory`: the item's tag, else its source tab's category.
fn category(id: &str, tab: &str) -> &'static str {
    match id {
        "advanced-windows-fluxer-sans" => "appearance",
        "advanced-unread-badge-customization" => "notifications",
        "advanced-expression-clone-shortcuts" | "advanced-hide-blocked-messages" | "chat-settings-sequential-file-send" => "chat",
        "client-developer-mode" => "developer",
        _ => match tab {
            "privacy_safety" | "authorized_apps" | "blocked_users" => "privacy",
            "appearance" | "themes" => "appearance",
            "accessibility" => "accessibility",
            "chat_settings" => "chat",
            "voice_video" => "voice",
            "notifications" => "notifications",
            "desktop_settings" => "desktop",
            "client_developer_settings" => "developer",
            _ => "account",
        },
    }
}

fn look_switch<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, label: Option<&str>, disabled: bool, get: fn(&Prefs) -> bool, put: fn(&mut Prefs, bool)) -> El {
    let on = get(&st.prefs);
    let mut sw = Switch::new(on).disabled(disabled);
    if let Some(l) = label {
        sw = sw.label(t(l).to_string());
    }
    sw.on_toggle(move |s: &mut S| {
        let sh = acc(s);
        let mut prefs = sh.prefs.clone();
        put(&mut prefs, !on);
        sh.intents.push(Intent::Prefs(Box::new(prefs)));
    })
    .el(p)
}

/// `AdvancedSettingControl` for the items Reflux has.
fn control<S: 'static>(id: &str, store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    Some(match id {
        "advanced-hide-blocked-messages" => look_switch(st, p, acc, None, false, |p| p.look.hide_blocked, |p, v| p.look.hide_blocked = v),
        "chat-settings-double-click-message-actions" => look_switch(st, p, acc, None, false, |p| p.look.double_click_actions, |p, v| p.look.double_click_actions = v),
        "chat-settings-convert-emoticons" => look_switch(st, p, acc, None, false, |p| p.look.convert_emoticons, |p, v| p.look.convert_emoticons = v),
        // The `message-action-bar` switch group.
        "chat-settings-message-action-bar" => {
            let off = !st.prefs.look.action_bar;
            col()
                .w_full()
                .gap(12.0)
                .child(look_switch(st, p, acc, Some("Show message action bar"), false, |p| p.look.action_bar, |p, v| p.look.action_bar = v))
                .child(look_switch(st, p, acc, Some("Show only the more button"), off, |p| p.look.action_bar_more_only, |p, v| p.look.action_bar_more_only = v))
                .child(look_switch(st, p, acc, Some("Show quick reactions"), off || st.prefs.look.action_bar_more_only, |p| p.look.action_bar_quick_reactions, |p, v| p.look.action_bar_quick_reactions = v))
                .child(look_switch(st, p, acc, Some("Enable Shift to expand"), off || st.prefs.look.action_bar_more_only, |p| p.look.action_bar_shift_expand, |p, v| p.look.action_bar_shift_expand = v))
        }
        "client-developer-mode" => {
            let on = store.settings.developer_mode;
            Switch::new(on).on_toggle(move |s: &mut S| acc(s).intents.push(Intent::PatchSettings(json!({ "developer_mode": !on })))).el(p)
        }
        // Fluxer's value is "disable": updates install only when asked. Reflux never installs on
        // its own, so this stops the background check.
        "advanced-automatic-updates" => look_switch(st, p, acc, None, false, |p| p.no_auto_update, |p, v| p.no_auto_update = v),
        // Rune always renders on the GPU (wgpu); there's no software path to fall back to.
        "advanced-hardware-acceleration" => Switch::<S>::new(true).disabled(true).el(p),
        _ => return None,
    })
}

/// The tab; `query` (the settings search) filters it like `filterAdvancedSettingItems`.
pub fn tab<S: 'static>(store: &Store, st: &ShellState, query: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
    let rows: Vec<(&str, El)> = ITEMS
        .iter()
        .filter(|i| i.6 && !MISSING.contains(&i.0))
        .filter(|&&(_, _, _, label, keywords, desc, _)| words.is_empty() || super::search::score(label, keywords, desc, &words) > 0)
        .filter_map(|&(id, tab, _, label, _, desc, _)| {
            let action = match control(id, store, st, p, acc) {
                Some(c) => c,
                None if DIRECT.contains(&id) => return None,
                None if tab != "advanced_settings" && super::visible_id(tab, store, st.premium) => crate::ui::Button::new(t("Open"))
                        .variant(ButtonVariant::Secondary)
                        .icon_right(&icons::ARROW_RIGHT)
                        .key(format!("advanced-open-{id}"))
                    .on_press(move |s: &mut S| super::open_tab(acc(s), tab))
                    .el(p),
                None => return None,
            };
            let full = id == "chat-settings-message-action-bar";
            let mut main = col().grow(1.0).shrink().min_w(0.0).gap(8.0).child(text(t(label).to_string()).font(font(14.0, 600.0).lh(18.9)).color(p.text_primary));
            if !desc.is_empty() {
                main = main.child(text(t(desc).to_string()).font(font(13.0, 400.0).lh(18.2)).color(p.text_secondary).wrap());
            }
            let body = if full { col().w_full().gap(12.0).child(main).child(action) } else { row().w_full().gap(12.0).center_y().child(main).child(action) };
            let r = col()
                .w_full()
                .py(16.0)
                .child(body)
                .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.border_color));
            Some((category(id, tab), r))
        })
        .collect();
    if rows.is_empty() {
        return col().w_full().py(32.0).items_center().child(text(t("No advanced settings found")).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary));
    }
    let mut out = col().w_full();
    let mut first = true;
    let mut rows = rows;
    for (key, label) in CATEGORIES {
        let (items, rest): (Vec<_>, Vec<_>) = rows.into_iter().partition(|(c, _)| c == key);
        rows = rest;
        if items.is_empty() {
            continue;
        }
        let items = items.into_iter().map(|(_, r)| r);
        out = out.child(section(t(label), col().w_full().children(items), first, p));
        first = false;
    }
    out
}
