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

/// Direct controls the desktop build can't honour; [docs/WORKAROUNDS.md] explains each.
const UNAVAILABLE: &[&str] = &[
    "accessibility-video-seek-thumbnails", // no frame-at-time snapshot API in the players
    "accessibility-smooth-scrolling", // Rune's wheel scroll is immediate
    "accessibility-middle-click-autoscroll", // no browser autoscroll
    "accessibility-stay-interactive-unfocused", // the OS gives clicks to the focused window
    "chat-settings-preupload-attachments", // uploads start when the message is sent
    "voice-video-connection-volume-controls", // the in-call volume menu is in shell/call.rs
    "advanced-native-title-bar", // Rune always draws the system titlebar
    "desktop-source-maps", // a Rust build has no JS source maps
];

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

/// A bare switch for a voice setting (the tab row already shows its label).
fn voice_switch<S: 'static>(
    st: &ShellState,
    p: &Palette,
    acc: ShellAccess<S>,
    get: fn(&crate::shell::voice::Settings) -> bool,
    put: fn(&mut crate::shell::voice::Settings, bool),
) -> El {
    let on = get(&st.voice.settings);
    Switch::new(on)
        .on_toggle(move |s: &mut S| {
            let sh = acc(s);
            put(&mut sh.voice.settings, !on);
            sh.intents.push(Intent::Voice(crate::shell::voice::Op::Settings));
        })
        .el(p)
}

/// `AdvancedSettingControl` for the items Reflux has.
fn control<S: 'static>(id: &str, store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> Option<El> {
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
        "chat-settings-reverse-image-search" | "chat-settings-search-engines" | "chat-settings-translators" => {
            let mode = match id {
                "chat-settings-search-engines" => super::search_engines::Mode::Search,
                "chat-settings-translators" => super::search_engines::Mode::Translate,
                _ => super::search_engines::Mode::Image,
            };
            crate::ui::Button::new(t("Configure"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Compact)
                .icon(&icons::GEAR_BOLD)
                .key(format!("advanced-configure-{id}"))
                .on_press(move |s: &mut S| acc(s).open_account_modal(super::AccountModal::Providers(mode)))
                .el(p)
        }
        "appearance-voice-channel-join-behavior" => crate::ui::Button::new(t("Configure"))
            .variant(ButtonVariant::Secondary)
            .size(ButtonSize::Compact)
            .icon(&icons::GEAR_BOLD)
            .key("advanced-voice-join-configure")
            .on_press(move |s: &mut S| acc(s).open_account_modal(super::AccountModal::SwitchGroup("appearance-voice-channel-join-behavior")))
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
        "accessibility-text-selection" => look_switch(st, p, acc, None, false, |p| p.look.text_selection, |p, v| p.look.text_selection = v),
        "appearance-show-neko" => look_switch(st, p, acc, None, false, |p| p.look.show_neko, |p, v| p.look.show_neko = v),
        "appearance-keep-neko-still" => look_switch(st, p, acc, None, !st.prefs.look.show_neko, |p| p.look.keep_neko_still, |p, v| p.look.keep_neko_still = v),
        "appearance-hide-keyboard-hints" => look_switch(st, p, acc, None, false, |p| !p.look.hide_keyboard_hints, |p, v| p.look.hide_keyboard_hints = !v),
        "appearance-enable-favorites" => look_switch(st, p, acc, None, false, |p| p.look.show_favorites, |p, v| p.look.show_favorites = v),
        "appearance-show-help-center-button" => look_switch(st, p, acc, None, false, |p| p.look.show_help_button, |p, v| p.look.show_help_button = v),
        "appearance-show-download-button" => look_switch(st, p, acc, None, false, |p| p.look.show_download_button, |p, v| p.look.show_download_button = v),
        "chat-settings-auto-send-gifs" => look_switch(st, p, acc, None, false, |p| p.look.auto_send_gifs, |p, v| p.look.auto_send_gifs = v),
        "chat-settings-save-gif-favorites" => look_switch(st, p, acc, None, false, |p| p.look.save_gif_favorites, |p, v| p.look.save_gif_favorites = v),
        "chat-settings-strip-tracking" => look_switch(st, p, acc, None, false, |p| p.look.strip_tracking, |p, v| p.look.strip_tracking = v),
        "chat-settings-keep-attachments-on-empty-edit" => look_switch(st, p, acc, None, false, |p| p.look.keep_attachments_on_empty_edit, |p, v| p.look.keep_attachments_on_empty_edit = v),
        "chat-settings-scroll-to-bottom-on-send" => look_switch(st, p, acc, None, false, |p| p.look.scroll_to_bottom_on_send, |p, v| p.look.scroll_to_bottom_on_send = v),
        "chat-settings-skip-mark-all-as-read-confirmation" => look_switch(st, p, acc, None, false, |p| p.look.skip_mark_all_read, |p, v| p.look.skip_mark_all_read = v),
        "voice-video-new-device-alerts" => look_switch(st, p, acc, None, false, |p| p.look.new_device_alerts, |p, v| p.look.new_device_alerts = v),
        "chat-settings-trust-domains" => {
            let on = store.settings.trusted_domains.iter().any(|d| d == "*");
            Switch::new(on)
                .on_toggle(move |s: &mut S| {
                    let sh = acc(s);
                    if on {
                        sh.media_ui.trusted.clear();
                        sh.intents.push(Intent::PatchSettings(json!({ "trusted_domains": [] })));
                    } else {
                        sh.confirm_trust_all = true;
                    }
                })
                .el(p)
        }
        // `UserSettings.defaultHideMutedChannels`: the server applies it to communities joined
        // from now on; an existing community's toggle stays in its own settings.
        "chat-settings-hide-muted-channels" => {
            let on = store.settings.default_hide_muted_channels;
            Switch::new(on)
                .on_toggle(move |s: &mut S| {
                    acc(s).intents.push(Intent::PatchSettings(json!({ "default_hide_muted_channels": !on })));
                })
                .el(p)
        }
        "chat-settings-media-buttons" | "chat-settings-expression-autocomplete" | "voice-video-screen-share-preview-behavior" => {
            let id: &'static str = match id {
                "chat-settings-media-buttons" => "chat-settings-media-buttons",
                "chat-settings-expression-autocomplete" => "chat-settings-expression-autocomplete",
                _ => "voice-video-screen-share-preview-behavior",
            };
            crate::ui::Button::new(t("Configure"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Compact)
                .icon(&icons::GEAR_BOLD)
                .key(format!("advanced-configure-{id}"))
                .on_press(move |s: &mut S| acc(s).open_account_modal(super::AccountModal::SwitchGroup(id)))
                .el(p)
        }
        "voice-video-screen-share-codec" => {
            let v = &st.voice.settings;
            Select::new(
                "advanced-codec-direct",
                &[
                    ("auto", "Automatic"),
                    ("av1", "AV1 (requires opt-in)"),
                    ("h265", "H.265 (requires opt-in)"),
                    ("h264", "H.264"),
                    ("vp9", "VP9"),
                    ("vp8", "VP8"),
                ],
                Some(&v.screen_codec),
            )
            .compact()
            .width(200.0)
            .on_choose(move |s: &mut S, val| {
                let sh = acc(s);
                sh.voice.settings.screen_codec = val.to_string();
                sh.intents.push(Intent::Voice(crate::shell::voice::Op::Settings));
            })
            .el(&st.popups, crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups), p, layer)
        }
        "voice-video-screen-share-av1-opt-in" => voice_switch(st, p, acc, |s| s.screen_av1_opt_in, |s, v| s.screen_av1_opt_in = v),
        "voice-video-screen-share-hevc-opt-in" => voice_switch(st, p, acc, |s| s.screen_hevc_opt_in, |s, v| s.screen_hevc_opt_in = v),
        // Reflux always draws in Fluxer Sans, so the Windows fallback is already off.
        "advanced-windows-fluxer-sans" => Switch::<S>::new(true).disabled(true).el(p),
        // One message carries every attachment, so their order is already the pick order.
        "chat-settings-sequential-file-send" => Switch::<S>::new(true).disabled(true).el(p),
        _ => return None,
    })
}

/// `isSettingsItemNew`: added at most 30 days before `now`, not in the future, and after the
/// account was created (all ISO 8601 UTC with milliseconds, so they compare as strings).
fn is_new(id: &str, now: &str, month_ago: &str, created: &str) -> bool {
    ADDED.iter().find(|a| a.0 == id).is_some_and(|&(_, at)| at <= now && at >= month_ago && at > created)
}

/// `TrustAllExternalLinksControl`'s confirmation before every domain is trusted.
pub fn trust_all_modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if !st.confirm_trust_all {
        return None;
    }
    let cancel = move |s: &mut S| acc(s).confirm_trust_all = false;
    let go = move |s: &mut S| {
        let sh = acc(s);
        sh.confirm_trust_all = false;
        sh.media_ui.trusted = vec!["*".into()];
        sh.intents.push(Intent::PatchSettings(json!({ "trusted_domains": ["*"] })));
    };
    Some(
        Modal::new(t("Trust all external links?"), cancel)
            .body(
                text("This skips the external-link warning for every domain and replaces your existing trusted domains. This is less secure.")
                    .font(font(14.0, 400.0).lh(20.0))
                    .color(p.text_primary_muted)
                    .wrap(),
            )
            .footer(
                row()
                    .gap(8.0)
                    .child(crate::ui::Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("trust-all-cancel").on_press(cancel).el(p))
                    .child(crate::ui::Button::new(t("Trust all")).variant(ButtonVariant::Danger).key("trust-all-confirm").on_press(go).el(p)),
            )
            .el(p),
    )
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
    } else if id == "chat-settings-media-buttons" {
        col()
            .w_full()
            .gap(12.0)
            .child(look_switch(st, p, acc, Some("Show GIF indicator"), false, |p| p.look.media_gif_indicator, |p, v| p.look.media_gif_indicator = v))
            .child(look_switch(st, p, acc, Some("Show attachment expiration indicator"), false, |p| p.look.media_expiry_indicator, |p, v| p.look.media_expiry_indicator = v))
            .child(look_switch(st, p, acc, Some("Show delete button"), false, |p| p.look.media_delete_button, |p, v| p.look.media_delete_button = v))
            .child(look_switch(st, p, acc, Some("Show download button"), false, |p| p.look.media_download_button, |p, v| p.look.media_download_button = v))
            .child(look_switch(st, p, acc, Some("Show favorite button"), false, |p| p.look.media_favorite_button, |p, v| p.look.media_favorite_button = v))
            .child(look_switch(st, p, acc, Some("Show suppress embeds button"), false, |p| p.look.media_suppress_embeds, |p, v| p.look.media_suppress_embeds = v))
    } else if id == "chat-settings-expression-autocomplete" {
        // shortcut: only default and custom emoji are in the composer's `:` autocomplete; the
        // sticker and saved-media switches are kept for Fluxer parity.
        col()
            .w_full()
            .gap(12.0)
            .child(look_switch(st, p, acc, Some("Show default emojis in expression autocomplete"), false, |p| p.look.autocomplete_emojis, |p, v| p.look.autocomplete_emojis = v))
            .child(look_switch(st, p, acc, Some("Show custom emojis in expression autocomplete"), false, |p| p.look.autocomplete_custom_emojis, |p, v| p.look.autocomplete_custom_emojis = v))
            .child(look_switch(st, p, acc, Some("Show stickers in expression autocomplete"), false, |p| p.look.autocomplete_stickers, |p, v| p.look.autocomplete_stickers = v))
            .child(look_switch(st, p, acc, Some("Show saved media in expression autocomplete"), false, |p| p.look.autocomplete_memes, |p, v| p.look.autocomplete_memes = v))
    } else if id == "voice-video-screen-share-preview-behavior" {
        col()
            .w_full()
            .gap(12.0)
            .child(look_switch(st, p, acc, Some("Pause my screen share preview in the background"), false, |p| p.look.pause_preview_unfocused, |p, v| p.look.pause_preview_unfocused = v))
            .child(look_switch(st, p, acc, Some("Disable picture-in-picture popout for screen shares"), false, |p| p.look.disable_screen_pip, |p, v| p.look.disable_screen_pip = v))
            .child(look_switch(st, p, acc, Some("Hide my stream preview thumbnail"), false, |p| p.look.hide_stream_preview, |p, v| p.look.hide_stream_preview = v))
    } else if id == "appearance-voice-channel-join-behavior" {
        let double = st.prefs.look.join_double_click;
        col().w_full().gap(12.0)
            .child(look_switch(st, p, acc, Some("Require double click to join voice channels"), false, |p| p.look.join_double_click, |p, v| p.look.join_double_click = v))
            .child(look_switch(st, p, acc, Some("Confirm before joining voice channels"), double, |p| p.look.confirm_join_voice, |p, v| p.look.confirm_join_voice = v))
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
pub fn tab<S: 'static>(store: &Store, st: &ShellState, query: &str, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let inline = !query.is_empty();
    let own = st.form.text.get("advanced-search").map_or("", |e| e.text.as_str());
    let query = if inline { query } else { own };
    let toolbar = (!inline).then(|| col().w_full().mb(20.0).child(search_field(st, p, acc)));
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(str::to_string).collect();
    let iso = crate::social::iso;
    let (now, month_ago, created) = (iso(st.now_ms), iso(st.now_ms.saturating_sub(30 * 86_400_000)), iso(store.my_id().timestamp_ms()));
    let rows: Vec<(&str, El)> = ITEMS
        .iter()
        .filter(|i| i.6 && !MISSING.contains(&i.0) && !UNAVAILABLE.contains(&i.0))
        .filter(|&&(_, _, _, label, keywords, desc, _)| words.is_empty() || super::search::score(label, keywords, desc, &words) > 0)
        .filter_map(|&(id, tab, _, label, _, desc, _)| {
            let action = match control(id, store, st, p, acc, layer) {
                Some(c) => c,
                None if DIRECT.contains(&id) => return None,
                None if tab != "advanced_settings" && super::visible_id(tab, store, st.premium) => crate::ui::Button::new(t("Open"))
                        .variant(ButtonVariant::Secondary)
                        .icon_right(&icons::ARROW_RIGHT_FILL)
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
                .key(id)
                .w_full()
                .py(16.0)
                .child(body)
                .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.border_color));
            Some((if is_new(id, &now, &month_ago, &created) { "new" } else { category(id, tab) }, r))
        })
        .collect();
    if rows.is_empty() {
        return col().w_full().children(toolbar).child(col().key("none").w_full().py(32.0).items_center().child(text(t("No advanced settings found")).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary)));
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

/// A boolean voice setting row in the encoder modal.
fn codec_switch<S: 'static>(
    st: &ShellState,
    p: &Palette,
    acc: ShellAccess<S>,
    _id: &'static str,
    label: &str,
    get: fn(&crate::shell::voice::Settings) -> bool,
    put: fn(&mut crate::shell::voice::Settings, bool),
) -> El {
    let on = get(&st.voice.settings);
    row()
        .w_full()
        .min_h(44.0)
        .py(4.0)
        .gap(16.0)
        .center_y()
        .child(
            text(t(label).to_string())
                .font(font(14.0, 500.0).lh(20.0))
                .color(p.text_primary)
                .grow(1.0),
        )
        .child(
            crate::ui::Switch::new(on)
                .on_toggle(move |s: &mut S| {
                    let sh = acc(s);
                    put(&mut sh.voice.settings, !on);
                    sh.intents.push(Intent::Voice(crate::shell::voice::Op::Settings));
                })
                .el(p),
        )
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
            "advanced-codec",
            "Preferred screen share codec",
            "Automatic uses H.264 (hardware) unless SVC needs VP9. AV1 and H.265 stay off until opted in.",
            &[
                ("auto", "Automatic"),
                ("av1", "AV1 (requires opt-in)"),
                ("h265", "H.265 (requires opt-in)"),
                ("h264", "H.264"),
                ("vp9", "VP9"),
                ("vp8", "VP8"),
            ],
            &v.screen_codec,
            200.0,
            |s, v| s.screen_codec = v,
        ))
        .child(codec_switch(st, p, acc, "advanced-av1-opt-in", "Allow AV1 for screen sharing", |s| s.screen_av1_opt_in, |s, v| s.screen_av1_opt_in = v))
        .child(codec_switch(st, p, acc, "advanced-hevc-opt-in", "Allow H.265 (HEVC) for screen sharing", |s| s.screen_hevc_opt_in, |s, v| s.screen_hevc_opt_in = v))
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
            "Choosing a mode shares in VP9 (software encoding) so layers apply; Automatic uses H.264 with the hardware encoder.",
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

    fn access(s: &mut ShellState) -> &mut ShellState { s }

    #[test]
    fn provider_settings_items_have_controls() {
        let (store, st) = (Store::default(), ShellState::default());
        let p = crate::theme::palette(crate::theme::Kind::Dark);
        for id in [
            "chat-settings-search-engines",
            "chat-settings-translators",
            "chat-settings-reverse-image-search",
        ] {
            assert!(ITEMS.iter().find(|i| i.0 == id).is_some_and(|i| i.6), "{id} is not an advanced item");
            let mut layer = Vec::new();
            assert!(control(id, &store, &st, p, access, &mut layer).is_some(), "{id} has no control");
        }
    }

    #[test]
    fn every_direct_control_is_shown_or_documented() {
        let (store, st) = (Store::default(), ShellState::default());
        let p = crate::theme::palette(crate::theme::Kind::Dark);
        for id in DIRECT {
            let mut layer = Vec::new();
            assert!(
                control(id, &store, &st, p, access, &mut layer).is_some() || UNAVAILABLE.contains(id),
                "{id} is neither implemented nor in UNAVAILABLE"
            );
        }
        for id in UNAVAILABLE {
            assert!(DIRECT.contains(id), "{id} is not a direct control");
        }
    }

    #[test]
    fn advanced_settings_defaults_match_fluxer() {
        let l = crate::look::Prefs::default();
        assert!(!l.text_selection && !l.hide_keyboard_hints && !l.keep_attachments_on_empty_edit);
        assert!(l.show_favorites && l.show_help_button && l.show_download_button);
        assert!(l.auto_send_gifs && !l.save_gif_favorites && !l.strip_tracking);
        assert!(l.autocomplete_emojis && l.autocomplete_custom_emojis);
        assert!(l.scroll_to_bottom_on_send && !l.skip_mark_all_read);
        assert!(l.new_device_alerts);
        assert!(l.pause_preview_unfocused && !l.disable_screen_pip);
        assert!(l.media_favorite_button && l.media_download_button && l.media_delete_button);
    }

    #[test]
    fn new_settings_window() {
        let (now, ago) = ("2026-11-07T00:00:00.000Z", "2026-10-08T00:00:00.000Z");
        assert!(is_new("advanced-hide-blocked-messages", now, ago, "2020-01-01T00:00:00.000Z"));
        assert!(!is_new("advanced-hide-blocked-messages", now, ago, "2026-10-25T00:00:00.000Z"), "the account is newer");
        assert!(!is_new("advanced-expression-clone-shortcuts", now, ago, "2020-01-01T00:00:00.000Z"), "older than 30 days");
        assert!(!is_new("client-developer-mode", now, ago, ""), "no addedAt");
    }
}
