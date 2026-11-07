//! The Advanced tab (`AdvancedSettingsTab.tsx`): every advanced-audience item from the search
//! index, grouped by category (`AdvancedSettingsCategories.ts`), each with its direct control
//! (`AdvancedSettingDirectControls.tsx`) or an "Open" button to the tab it lives in.
//! Items added in the last 30 days lead in a "New settings" section (`isSettingsItemNew`).
//! Items whose feature Reflux doesn't have are left out.

use reflux_store::Store;
use rune::{El, TextEdit, col, el, row};
use crate::ui::mark::marked as text;
use serde_json::json;

use super::search_index::{ADDED, ITEMS};
use super::tabs::section;
use crate::desktop::Prefs;
use crate::i18n::t;
use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{ButtonSize, ButtonVariant, Modal, ModalSize, Select, Switch};

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
        "advanced-expression-clone-shortcuts" => look_switch(
            st,
            p,
            acc,
            None,
            false,
            |p| p.look.clone_shortcuts,
            |p, v| p.look.clone_shortcuts = v,
        ),
        "voice-video-screen-share-encoder-controls" => crate::ui::Button::new(t("Configure"))
            .variant(ButtonVariant::Secondary)
            .size(ButtonSize::Compact)
            .icon(&icons::GEAR_BOLD)
            .key("advanced-encoder-configure")
            .on_press(move |s: &mut S| {
                acc(s).open_account_modal(super::AccountModal::EncoderControls)
            })
            .el(p),
        "chat-settings-reverse-image-search" => crate::ui::Button::new(t("Configure"))
            .variant(ButtonVariant::Secondary)
            .size(ButtonSize::Compact)
            .icon(&icons::GEAR_BOLD)
            .key("advanced-ris-configure")
            .on_press(move |s: &mut S| {
                acc(s).open_account_modal(super::AccountModal::ReverseImageSearch)
            })
            .el(p),
        "advanced-unread-badge-customization" => look_switch(
            st,
            p,
            acc,
            None,
            false,
            |p| p.look.unread_badge_customization,
            |p, v| p.look.unread_badge_customization = v,
        ),
        "advanced-hide-blocked-messages" => look_switch(
            st,
            p,
            acc,
            None,
            false,
            |p| p.look.hide_blocked,
            |p, v| p.look.hide_blocked = v,
        ),
        "chat-settings-double-click-message-actions" => look_switch(
            st,
            p,
            acc,
            None,
            false,
            |p| p.look.double_click_actions,
            |p, v| p.look.double_click_actions = v,
        ),
        "chat-settings-convert-emoticons" => look_switch(
            st,
            p,
            acc,
            None,
            false,
            |p| p.look.convert_emoticons,
            |p, v| p.look.convert_emoticons = v,
        ),
        // `SwitchGroupSettingsButton`: the group opens in a dialog.
        "chat-settings-message-action-bar" | "chat-settings-input-buttons" => {
            let id: &'static str = if id == "chat-settings-input-buttons" { "chat-settings-input-buttons" } else { "chat-settings-message-action-bar" };
            crate::ui::Button::new(t("Configure"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Compact)
                .icon(&icons::GEAR_BOLD)
                .key(format!("advanced-configure-{id}"))
                .on_press(move |s: &mut S| acc(s).open_account_modal(super::AccountModal::SwitchGroup(id)))
                .el(p)
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

/// `isSettingsItemNew`: added at most 30 days before `now`, not in the future, and after the
/// account was created (all ISO 8601 UTC with milliseconds, so they compare as strings).
fn is_new(id: &str, now: &str, month_ago: &str, created: &str) -> bool {
    ADDED.iter().find(|a| a.0 == id).is_some_and(|&(_, at)| at <= now && at >= month_ago && at > created)
}

/// `SwitchGroupSettingsModal`: the item's switches (`MessageActionBarContent`, `InputButtonsContent`).
pub fn switch_group_modal<S: 'static>(st: &ShellState, id: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let title = ITEMS.iter().find(|i| i.0 == id).map_or("", |i| i.3);
    let body = if id == "chat-settings-input-buttons" {
        col()
            .w_full()
            .gap(12.0)
            .child(look_switch(st, p, acc, Some("Show GIFs button"), false, |p| p.look.show_gif_button, |p, v| p.look.show_gif_button = v))
            .child(look_switch(st, p, acc, Some("Show media button"), false, |p| p.look.show_memes_button, |p, v| p.look.show_memes_button = v))
            .child(look_switch(st, p, acc, Some("Show stickers button"), false, |p| p.look.show_stickers_button, |p, v| p.look.show_stickers_button = v))
            .child(look_switch(st, p, acc, Some("Show emoji button"), false, |p| p.look.show_emoji_button, |p, v| p.look.show_emoji_button = v))
            .child(look_switch(st, p, acc, Some("Show send button"), false, |p| p.look.show_send_button, |p, v| p.look.show_send_button = v))
    } else {
        let off = !st.prefs.look.action_bar;
        col()
            .w_full()
            .gap(12.0)
            .child(look_switch(st, p, acc, Some("Show message action bar"), false, |p| p.look.action_bar, |p, v| p.look.action_bar = v))
            .child(look_switch(st, p, acc, Some("Show only the more button"), off, |p| p.look.action_bar_more_only, |p, v| p.look.action_bar_more_only = v))
            .child(look_switch(st, p, acc, Some("Show quick reactions"), off || st.prefs.look.action_bar_more_only, |p| p.look.action_bar_quick_reactions, |p, v| p.look.action_bar_quick_reactions = v))
            .child(look_switch(st, p, acc, Some("Enable Shift to expand"), off || st.prefs.look.action_bar_more_only, |p| p.look.action_bar_shift_expand, |p, v| p.look.action_bar_shift_expand = v))
    };
    Modal::new(t(title), move |s: &mut S| acc(s).close_account_modals()).size(ModalSize::Medium).body(body).el(p)
}

/// `SettingsSearch` over the tab (`AdvancedSettingsTab`'s toolbar): 34 tall, a 16 bold magnifier,
/// 13/18 text, a clear button once there's a query.
fn search_field<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    const KEY: &str = "advanced-search";
    let empty = TextEdit::new("");
    let edit = st.form.text.get(KEY).unwrap_or(&empty);
    let c = super::colors(p);
    let border = if edit.focused { p.background_modifier_accent_focus } else { c.border };
    row()
        .w_full()
        .h(34.0)
        .pl(36.0)
        .pr(4.0)
        .center_y()
        .radius(8.0)
        .fill(c.surface)
        .border(1.0, crate::ui::over(border, c.surface))
        .cursor(rune::Cursor::Text)
        .child(rune::icon(&icons::MAGNIFYING_GLASS_BOLD, 16.0).color(p.text_tertiary).pin(Some(12.0), Some(8.0), None, None))
        .child(
            rune::text_input(edit, font(13.0, 400.0).lh(18.0))
                .single_line()
                .key(KEY)
                .label(t("Search advanced settings"))
                .grow(1.0)
                .shrink()
                .color(p.text_primary)
                .placeholder(t("Search advanced settings"), p.text_tertiary)
                .caret(p.text_primary, p.background_modifier_selected)
                .on_input_key(move |s: &mut S, k, _| {
                    let blur = k.key == rune::Key::Escape;
                    if blur && let Some(e) = acc(s).form.text.get_mut(KEY) {
                        e.focused = false;
                    }
                    blur
                })
                .on_edit(move |s: &mut S| -> &mut TextEdit { acc(s).form.text.entry(KEY.into()).or_insert_with(|| TextEdit::new("")) }),
        )
        .children((!edit.text.is_empty()).then(|| {
            row()
                .size(28.0, 28.0)
                .center()
                .radius(6.0)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Clear search"))
                .hover(c.hover)
                .on_press(move |s: &mut S| {
                    let mut e = TextEdit::new("");
                    e.focused = true;
                    acc(s).form.text.insert(KEY.into(), e);
                })
                .child(rune::icon(&icons::X_BOLD, 14.0).color(p.text_tertiary))
        }))
}

/// The tab; `query` (the settings search) filters it like `filterAdvancedSettingItems`. As its own
/// tab (no settings search), its search field filters it instead.
pub fn tab<S: 'static>(store: &Store, st: &ShellState, query: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let inline = !query.is_empty();
    let own = st.form.text.get("advanced-search").map_or("", |e| e.text.as_str());
    let query = if inline { query } else { own };
    let toolbar = (!inline).then(|| col().w_full().mb(20.0).child(search_field(st, p, acc)));
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
    let iso = crate::social::iso;
    let (now, month_ago, created) = (iso(st.now_ms), iso(st.now_ms.saturating_sub(30 * 86_400_000)), iso(store.my_id().timestamp_ms()));
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
            let mut main = col().grow(1.0).shrink().min_w(0.0).gap(8.0).child(text(t(label).to_string()).font(font(14.0, 600.0).lh(18.9)).color(p.text_primary));
            if !desc.is_empty() {
                main = main.child(text(t(desc).to_string()).font(font(13.0, 400.0).lh(18.2)).color(p.text_secondary).wrap());
            }
            let body = row().w_full().gap(12.0).center_y().child(main).child(action);
            let r = col()
                .w_full()
                .py(16.0)
                .child(body)
                .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.border_color));
            Some((if is_new(id, &now, &month_ago, &created) { "new" } else { category(id, tab) }, r))
        })
        .collect();
    if rows.is_empty() {
        return col().w_full().children(toolbar).child(col().w_full().py(32.0).items_center().child(text(t("No advanced settings found")).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary)));
    }
    let mut out = col().w_full().children(toolbar);
    let mut first = true;
    let mut rows = rows;
    for (key, label) in [("new", "New settings")].iter().chain(CATEGORIES) {
        let (items, rest): (Vec<_>, Vec<_>) = rows.into_iter().partition(|(c, _)| c == key);
        rows = rest;
        if items.is_empty() {
            continue;
        }
        let mut body = col().w_full();
        if *key == "new" {
            body = body.child(text(t("New settings stay here for 30 days.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap());
        }
        out = out.child(section(t(label), body.children(items.into_iter().map(|(_, r)| r)), first, p));
        first = false;
    }
    out
}

/// `ScreenShareEncoderControlsModal`: encoder path, SVC mode and content hint
/// (`CompactComboboxRow`s), saved with the voice settings and used by the next screen share.
// shortcut: Fluxer's "No hardware encoder was found" description needs webrtc-sys's factory
// probe, which isn't exposed; the plain description always shows.
pub fn encoder_modal<S: 'static>(
    st: &ShellState,
    p: &Palette,
    acc: ShellAccess<S>,
    layer: &mut Vec<El>,
) -> El {
    let v = &st.voice.settings;
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    let encoder = match v.encoder.as_str() {
        "software" => "software",
        "auto" => "auto",
        _ => "hardware",
    };
    let mut popups = Vec::new();
    let mut combo = |id: &'static str,
                     label: &str,
                     desc: &str,
                     options: &[(&str, &str)],
                     value: &str,
                     width: f32,
                     put: fn(&mut crate::shell::voice::Settings, String)| {
        let select = Select::new(id, options, Some(value))
            .compact()
            .width(width)
            .on_choose(move |s: &mut S, val| {
                let sh = acc(s);
                put(&mut sh.voice.settings, val.to_string());
                sh.intents
                    .push(Intent::Voice(crate::shell::voice::Op::Settings));
            })
            .el(&st.popups, pops, p, &mut popups);
        let words = col()
            .grow(1.0)
            .min_w(0.0)
            .gap(4.0)
            .child(
                text(t(label).to_string())
                    .font(font(14.0, 500.0).lh(20.0))
                    .color(p.text_primary),
            )
            .child(
                text(t(desc).to_string())
                    .font(font(13.0, 400.0).lh(20.0))
                    .color(p.text_primary_muted)
                    .wrap(),
            );
        row()
            .w_full()
            .min_h(44.0)
            .py(4.0)
            .gap(16.0)
            .center_y()
            .child(words)
            .child(select)
    };
    let body = col()
        .w_full()
        .gap(12.0)
        .child(combo(
            "advanced-encoder-path",
            "Encoder path",
            "Encoder preference for new screen shares.",
            &[
                ("auto", "Automatic"),
                ("hardware", "Prefer hardware"),
                ("software", "Prefer software"),
            ],
            encoder,
            160.0,
            |s, v| s.encoder = v,
        ))
        .child(combo(
            "advanced-svc-mode",
            "SVC mode",
            "Layering for AV1 and VP9 when available.",
            &[
                ("auto", "Automatic"),
                ("single_layer", "Single layer"),
                ("temporal", "Temporal layers"),
            ],
            &v.screen_svc,
            224.0,
            |s, v| s.screen_svc = v,
        ))
        .child(combo(
            "advanced-content-hint",
            "Content hint",
            "Used in Custom mode. The Gaming and Screen share presets set their own hint.",
            &[
                ("auto", "Automatic"),
                ("motion", "Motion"),
                ("detail", "Detail"),
                ("text", "Text"),
            ],
            &v.screen_content_hint,
            128.0,
            |s, v| s.screen_content_hint = v,
        ));
    layer.append(&mut popups);
    Modal::new(t("Screen share encoder controls"), move |s: &mut S| {
        acc(s).close_account_modals()
    })
    .size(ModalSize::Medium)
    .body(body)
    .el(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_settings_window() {
        let (now, ago) = ("2026-11-07T00:00:00.000Z", "2026-10-08T00:00:00.000Z");
        assert!(is_new("advanced-hide-blocked-messages", now, ago, "2020-01-01T00:00:00.000Z"));
        assert!(!is_new("advanced-hide-blocked-messages", now, ago, "2026-10-25T00:00:00.000Z"), "the account is newer");
        assert!(!is_new("advanced-expression-clone-shortcuts", now, ago, "2020-01-01T00:00:00.000Z"), "older than 30 days");
        assert!(!is_new("client-developer-mode", now, ago, ""), "no addedAt");
    }
}
