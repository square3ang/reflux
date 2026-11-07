//! Appearance, accessibility, chat input and notification preferences that Fluxer keeps per device
//! (`Accessibility`, `ChatInputSettings`, `Notification`, `Sound` stores and the quick CSS). They
//! live in session.json (`desktop::Prefs::look`) and the views read the current copy with [`get`].
//! The ones Fluxer syncs through `synced_preferences` go through [`crate::nav`]'s `look_sync`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, RwLock, RwLockReadGuard};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// `fontSize`: the chat's text size in px (12, 14, 15, 16, 18, 20, 24).
    pub font_size: f32,
    /// `messageGroupSpacing` / `compactMessageGroupSpacing` (0, 4, 8, 16, 24).
    pub group_spacing: f32,
    pub compact_group_spacing: f32,
    /// `showUserAvatarsInCompactMode` (read by the compact layout).
    pub compact_avatars: bool,
    /// `channelTypingIndicatorMode`: 0 typing + avatars, 1 typing only, 2 hidden.
    pub typing_indicator: u8,
    /// `showFadedUnreadOnMutedChannels`.
    pub faded_unread_muted: bool,
    /// `PrivacyPreferences.showActiveNow`.
    pub active_now: bool,
    /// `saturationFactor` (0..=1).
    pub saturation: f32,
    pub underline_links: bool,
    pub dim_strikethrough: bool,
    /// `dmMessagePreviewMode`: 0 all, 1 unread only, 2 none (the desktop default).
    pub dm_previews: u8,
    /// `screenReaderAnnounceNewMessages`.
    pub announce_messages: bool,
    /// `ttsRate` (0.1, 0.5, 1, 1.5, 2), `enableTTSCommand`, and `ttsNotificationMode`: 0 every
    /// channel, 1 the open channel, 2 never.
    pub tts_rate: f32,
    pub tts_command: bool,
    pub tts_mode: u8,
    /// Accessibility: joining a voice channel needs a double click (or asks first).
    pub join_double_click: bool,
    pub confirm_join_voice: bool,
    /// Keyboard: `showTextareaFocusRing`, `escapeExitsKeyboardMode`, `showContextMenuShortcuts`,
    /// `confirmBeforeStartingCalls`.
    pub focus_ring: bool,
    pub escape_exits_keyboard: bool,
    pub menu_shortcuts: bool,
    pub confirm_calls: bool,
    /// `VoicePrompts.skipP2pJoinConfirm`: join peer-to-peer calls without the consent dialog.
    pub skip_p2p_join: bool,
    /// `syncReducedMotionWithSystem`, else `reducedMotionOverride`.
    pub sync_motion: bool,
    pub reduce_motion: bool,
    /// The message action bar (`showMessageActionBar`, `…OnlyMoreButton`, `…QuickReactions`,
    /// `…ShiftExpand`), `messageDoubleClickActions`, and `AdvancedSettings.hideBlockedMessages`.
    pub action_bar: bool,
    pub action_bar_more_only: bool,
    pub action_bar_quick_reactions: bool,
    pub action_bar_shift_expand: bool,
    pub double_click_actions: bool,
    pub hide_blocked: bool,
    /// `AdvancedSettings.expressionCloneShortcutsEnabled`: "Clone to…" on emoji and stickers.
    pub clone_shortcuts: bool,
    /// `Accessibility.enableTextSelection`: message text can be selected and copied.
    pub text_selection: bool,
    /// `Accessibility.showNeko` / `keepNekoStill`.
    pub show_neko: bool,
    pub keep_neko_still: bool,
    /// `Accessibility.hideKeyboardHints`: no Ctrl+K badge next to the quick switcher.
    pub hide_keyboard_hints: bool,
    /// `Accessibility.showFavorites`, and the rail's help / download buttons
    /// (`HiddenGuildListButtons`).
    pub show_favorites: bool,
    pub show_help_button: bool,
    pub show_download_button: bool,
    /// `Accessibility.autoSendKlipyGifs`: picking a GIF sends it, else it's inserted.
    pub auto_send_gifs: bool,
    /// `FavoriteGif.saveGifFavoritesAsSavedMedia`: star GIFs through `/users/@me/memes`.
    pub save_gif_favorites: bool,
    /// `UserSettings.sanitizeUrls`: remove tracking parameters from links sent.
    pub strip_tracking: bool,
    /// `Accessibility.show*InExpressionAutocomplete`.
    pub autocomplete_emojis: bool,
    pub autocomplete_custom_emojis: bool,
    pub autocomplete_stickers: bool,
    pub autocomplete_memes: bool,
    /// `AdvancedSettings.keepAttachmentsOnEmptyMessageEdit`.
    pub keep_attachments_on_empty_edit: bool,
    /// `Accessibility.scrollToBottomOnMessageSend`.
    pub scroll_to_bottom_on_send: bool,
    /// `Inbox.skipMarkAllAsReadConfirmation`.
    pub skip_mark_all_read: bool,
    /// `NewDeviceMonitoring.suppressAlerts` (stored inverted: alerts on).
    pub new_device_alerts: bool,
    /// `VoiceSettings.pauseOwnScreenSharePreviewOnUnfocus`,
    /// `disablePictureInPicturePopoutScreenShare`, `PrivacyPreferences.disableStreamPreviews`.
    pub pause_preview_unfocused: bool,
    pub disable_screen_pip: bool,
    pub hide_stream_preview: bool,
    /// `Accessibility.showMedia*Button`, `showGifIndicator`, `showAttachmentExpiryIndicator`.
    pub media_favorite_button: bool,
    pub media_download_button: bool,
    pub media_delete_button: bool,
    pub media_suppress_embeds: bool,
    pub media_gif_indicator: bool,
    pub media_expiry_indicator: bool,
    /// `AccountTheme.syncAcrossDevices` and `localTheme`: off, this device keeps its own base
    /// theme instead of the account's.
    pub theme_sync: bool,
    /// Composer buttons: `showGifButton`, `showMemesButton`, `showStickersButton`,
    /// `showEmojiButton` and `showMessageSendButton`.
    pub show_gif_button: bool,
    pub show_memes_button: bool,
    pub show_stickers_button: bool,
    pub show_emoji_button: bool,
    pub show_send_button: bool,
    pub local_theme: String,
    /// `AdvancedSettings.unreadBadgeCustomizationEnabled`: per-community unread badge levels.
    pub unread_badge_customization: bool,
    /// `ChatInputSettings.convertEmoticons`.
    pub convert_emoticons: bool,
    /// `Notification.unreadMessageBadgeEnabled` (dock / taskbar / title badge).
    pub unread_badge: bool,
    /// `Sound.settings.disabledSounds` by Fluxer sound ID (absent = the sound's default).
    pub sounds_off: BTreeMap<String, bool>,
    /// `masterVolume` and `soundOverrides` by sound ID (0..=200 %).
    pub volume: f32,
    pub sound_volumes: BTreeMap<String, f32>,
    /// Custom sound files by sound ID (copied under the config dir).
    pub custom_sounds: BTreeMap<String, String>,
    /// Quick CSS. Only `:root` theme variable overrides apply ([`crate::theme::custom`]).
    pub css: String,
    /// `ThemeLibrary`: saved themes, the IDs turned on (applied before the quick CSS, in this
    /// order), and the imported assets, kept as they came so an export gives them back.
    pub themes: Vec<LibraryTheme>,
    pub enabled_themes: Vec<String>,
    pub theme_assets: Vec<serde_json::Value>,
    /// `ThemeLibrary.folderPaths`: watched theme folders ([`crate::settings::theme_folder`]).
    pub theme_folders: Vec<String>,
}

/// `ThemeLibraryTheme` (the fields an export carries).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LibraryTheme {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub tags: Vec<String>,
    pub css: String,
    pub file_name: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// The CSS file on disk and the watched folder it came from (folder themes only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_path: Option<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            font_size: 16.0,
            group_spacing: 16.0,
            compact_group_spacing: 0.0,
            compact_avatars: false,
            typing_indicator: 0,
            faded_unread_muted: false,
            active_now: true,
            saturation: 1.0,
            underline_links: false,
            dim_strikethrough: true,
            dm_previews: 2,
            announce_messages: false,
            tts_rate: 1.0,
            tts_command: true,
            tts_mode: 2,
            join_double_click: false,
            confirm_join_voice: false,
            focus_ring: true,
            escape_exits_keyboard: false,
            menu_shortcuts: false,
            confirm_calls: true,
            skip_p2p_join: false,
            sync_motion: true,
            reduce_motion: false,
            action_bar: true,
            action_bar_more_only: false,
            action_bar_quick_reactions: true,
            action_bar_shift_expand: true,
            double_click_actions: false,
            hide_blocked: false,
            clone_shortcuts: false,
            text_selection: false,
            show_neko: false,
            keep_neko_still: false,
            hide_keyboard_hints: false,
            show_favorites: true,
            show_help_button: true,
            show_download_button: true,
            auto_send_gifs: true,
            save_gif_favorites: false,
            strip_tracking: false,
            autocomplete_emojis: true,
            autocomplete_custom_emojis: true,
            autocomplete_stickers: true,
            autocomplete_memes: true,
            keep_attachments_on_empty_edit: false,
            scroll_to_bottom_on_send: true,
            skip_mark_all_read: false,
            new_device_alerts: true,
            pause_preview_unfocused: true,
            disable_screen_pip: false,
            hide_stream_preview: false,
            media_favorite_button: true,
            media_download_button: true,
            media_delete_button: true,
            media_suppress_embeds: true,
            media_gif_indicator: true,
            media_expiry_indicator: true,
            theme_sync: true,
            show_gif_button: true,
            show_memes_button: true,
            show_stickers_button: true,
            show_emoji_button: true,
            show_send_button: false,
            local_theme: "dark".into(),
            unread_badge_customization: false,
            convert_emoticons: false,
            unread_badge: true,
            sounds_off: BTreeMap::new(),
            volume: 100.0,
            sound_volumes: BTreeMap::new(),
            custom_sounds: BTreeMap::new(),
            css: String::new(),
            themes: Vec::new(),
            enabled_themes: Vec::new(),
            theme_assets: Vec::new(),
            theme_folders: Vec::new(),
        }
    }
}

/// `TrackingUrlRules`' global parameter names (the prefixes handled in [`tracking`] aside).
const TRACKING_PARAMS: &[&str] = &[
    "gclid", "gclsrc", "dclid", "fbclid", "msclkid", "yclid", "twclid", "ttclid", "igshid",
    "mc_cid", "mc_eid", "oly_enc_id", "oly_anon_id", "vero_id", "wickedid", "rb_clickid",
    "s_cid", "mkt_tok", "trk", "trkCampaign", "_hsenc", "_hsmi", "hsCtaTracking", "ref_src",
    "ref_url", "li_fat_id", "epik", "s_kwcid", "spm", "scm", "si", "pp", "feature",
];

/// `TrackingUrlRules`' global rules: known analytics/referral parameters.
fn tracking(name: &str) -> bool {
    TRACKING_PARAMS.contains(&name)
        || ["utm_", "itm_", "int_", "pk_", "piwik_", "mtm_", "matomo_", "hsa_", "share_", "ss_"]
            .iter()
            .any(|p| name.starts_with(p))
}

/// `sanitizeTrackingUrls`: removes tracking parameters from every http(s) link in `text`.
pub fn strip_tracking(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut start = 0;
    for (i, c) in text.char_indices().chain([(text.len(), ' ')]) {
        if !c.is_whitespace() {
            continue;
        }
        if i > start {
            let token = &text[start..i];
            let (open, close) = (token.starts_with('<'), token.ends_with('>'));
            let url = token.trim_start_matches('<').trim_end_matches('>');
            let cleaned = url::Url::parse(url).ok().filter(|u| matches!(u.scheme(), "http" | "https")).and_then(|mut u| {
                let pairs: Vec<(String, String)> = u.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
                let kept: Vec<(String, String)> = pairs.iter().filter(|(k, _)| !tracking(k)).cloned().collect();
                if kept.len() == pairs.len() {
                    return None;
                }
                let query = (!kept.is_empty()).then(|| url::form_urlencoded::Serializer::new(String::new()).extend_pairs(&kept).finish());
                u.set_query(query.as_deref());
                Some(u.to_string())
            });
            match cleaned {
                Some(u) => {
                    if open { out.push('<'); }
                    out.push_str(&u);
                    if close { out.push('>'); }
                }
                None => out.push_str(token),
            }
        }
        if i < text.len() {
            out.push(c);
        }
        start = i + c.len_utf8();
    }
    out
}

impl Prefs {
    /// Whether motion is reduced, given the OS preference.
    pub fn reduced(&self, system: bool) -> bool {
        if self.sync_motion { system } else { self.reduce_motion }
    }

    /// `Sound.isSoundDisabled`: the user's choice, else the same-channel sound is off by default.
    pub fn sound_on(&self, id: &str) -> bool {
        !self.sounds_off.get(id).copied().unwrap_or(id == "same-channel-message")
    }

    /// `themePreference`: the account's theme, or this device's while sync is off.
    pub fn theme<'a>(&'a self, account: &'a str) -> &'a str {
        if self.theme_sync { account } else { &self.local_theme }
    }

    /// The CSS that applies: the turned-on saved themes, then the quick CSS.
    pub fn theme_css(&self) -> String {
        let on = self.enabled_themes.iter().filter_map(|id| self.themes.iter().find(|t| &t.id == id));
        on.map(|t| t.css.as_str()).chain([self.css.as_str()]).filter(|c| !c.trim().is_empty()).collect::<Vec<_>>().join("\n\n")
    }

    /// `getEffectiveMultiplier`: the sound's override, else the master volume, as a gain (0..=2).
    pub fn sound_gain(&self, id: &str) -> f32 {
        self.sound_volumes.get(id).copied().unwrap_or(self.volume).clamp(0.0, 200.0) / 100.0
    }

    /// The chat text scale (`--font-size` against the 16px default).
    pub fn text_scale(&self) -> f32 {
        (self.font_size / 16.0).clamp(0.5, 3.0)
    }
}

static CURRENT: LazyLock<RwLock<Prefs>> = LazyLock::new(Default::default);
static REDUCED: AtomicBool = AtomicBool::new(false);

/// The current preferences (set by [`set`] whenever they change).
pub fn get() -> RwLockReadGuard<'static, Prefs> {
    CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

/// Reads one value of the current preferences.
pub fn with<T>(f: impl FnOnce(&Prefs) -> T) -> T {
    f(&get())
}

pub fn set(p: &Prefs) {
    let mut g = CURRENT.write().unwrap_or_else(|e| e.into_inner());
    if *g != *p {
        *g = p.clone();
    }
}

/// Whether this frame reduces motion (the view sets it from the OS preference and [`Prefs`]).
pub fn reduced_motion() -> bool {
    REDUCED.load(Ordering::Relaxed)
}

pub fn set_reduced_motion(on: bool) {
    REDUCED.store(on, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn tracking_parameters_are_stripped_from_links() {
        assert_eq!(
            strip_tracking("see https://example.com/a?utm_source=x&id=7&fbclid=y now"),
            "see https://example.com/a?id=7 now"
        );
        assert_eq!(strip_tracking("<https://example.com/a?utm_medium=z>"), "<https://example.com/a>");
        assert_eq!(strip_tracking("https://example.com/a?q=1"), "https://example.com/a?q=1", "nothing tracking");
        assert_eq!(strip_tracking("not a url ?utm_source=x"), "not a url ?utm_source=x");
    }

    #[test]
    fn theme_follows_sync() {
        let mut p = Prefs::default();
        assert_eq!(p.theme("coal"), "coal");
        p.theme_sync = false;
        p.local_theme = "light".into();
        assert_eq!(p.theme("coal"), "light");
    }

    #[test]
    fn sound_defaults_and_motion() {
        let mut p = Prefs::default();
        assert!(p.sound_on("message") && !p.sound_on("same-channel-message"));
        p.volume = 50.0;
        p.sound_volumes.insert("mute".into(), 150.0);
        assert_eq!((p.sound_gain("message"), p.sound_gain("mute")), (0.5, 1.5));
        p.sounds_off.insert("message".into(), true);
        assert!(!p.sound_on("message"));
        assert!(p.reduced(true) && !p.reduced(false));
        p.sync_motion = false;
        p.reduce_motion = true;
        assert!(p.reduced(false));
    }
}
