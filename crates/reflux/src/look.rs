//! Appearance, accessibility, chat input and notification preferences that Fluxer keeps per device
//! (`Accessibility`, `ChatInputSettings`, `Notification`, `Sound` stores and the quick CSS). They
//! live in session.json (`desktop::Prefs::look`) and the views read the current copy with [`get`].
// shortcut: emoticons are matched as whole space-separated words from a short list.
// shortcut: Fluxer also syncs some of these through `synced_preferences` (a protobuf in the
// account settings); Reflux keeps them on this device only.

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

impl Prefs {
    /// Whether motion is reduced, given the OS preference.
    pub fn reduced(&self, system: bool) -> bool {
        if self.sync_motion { system } else { self.reduce_motion }
    }

    /// `Sound.isSoundDisabled`: the user's choice, else the same-channel sound is off by default.
    pub fn sound_on(&self, id: &str) -> bool {
        !self.sounds_off.get(id).copied().unwrap_or(id == "same-channel-message")
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

/// `EMOTICON_MAP`-style text smileys turned into emoji when sent (`convertEmoticons`).
const EMOTICONS: &[(&str, &str)] = &[
    (":)", "🙂"),
    (":-)", "🙂"),
    (":(", "🙁"),
    (":-(", "🙁"),
    (":D", "😄"),
    (":-D", "😄"),
    (";)", "😉"),
    (";-)", "😉"),
    (":P", "😛"),
    (":-P", "😛"),
    (":p", "😛"),
    (":O", "😮"),
    (":o", "😮"),
    (":'(", "😢"),
    (":|", "😐"),
    ("<3", "❤️"),
    ("</3", "💔"),
    ("B)", "😎"),
    (":*", "😘"),
    ("XD", "😆"),
    (">:(", "😠"),
];

/// Replaces whole-word emoticons outside code spans.
pub fn convert_emoticons(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    for (i, part) in text.split('`').enumerate() {
        if i > 0 {
            out.push('`');
            in_code = !in_code;
        }
        if in_code {
            out.push_str(part);
            continue;
        }
        let words: Vec<String> = part
            .split(' ')
            .map(|w| {
                let (core, nl) = match w.strip_suffix('\n') {
                    Some(c) => (c, "\n"),
                    None => (w, ""),
                };
                match EMOTICONS.iter().find(|(e, _)| *e == core) {
                    Some((_, emoji)) => format!("{emoji}{nl}"),
                    None => w.to_string(),
                }
            })
            .collect();
        out.push_str(&words.join(" "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoticons_convert_whole_words_outside_code() {
        assert_eq!(convert_emoticons("hi :) <3"), "hi 🙂 ❤️");
        assert_eq!(convert_emoticons("a:)b `:)` :D"), "a:)b `:)` 😄");
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
