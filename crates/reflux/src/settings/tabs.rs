//! The content of each settings tab.

use reflux_store::Store;
use rune::{Color, El, col, el, icon, row, text};
use serde_json::json;

use super::{SettingsUi, select};
use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Switch, over, rgb};

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
        "appearance" => appearance(st, p, acc),
        "themes" => themes(store, p, acc),
        "advanced_settings" => advanced(store, p, acc),
        "voice_video" => voice(st, p, acc, layer),
        "notifications" => super::desktop_tabs::notifications(st, p, acc),
        "keybinds" => super::desktop_tabs::keybinds(p),
        "language" => super::desktop_tabs::language(store, st, p, acc),
        "desktop_settings" => super::desktop_tabs::desktop(st, p, acc),
        _ => not_available(p),
    }
}

/// `SettingsSection`: title 18/600, content gap 20; later sections get a top border and 24 padding.
pub(crate) fn section(title: &str, content: El, first: bool, p: &Palette) -> El {
    let mut s = col()
        .w_full()
        .gap(16.0)
        .pb(24.0)
        .child(text(title.to_string()).font(font(18.0, 600.0).lh(23.4)).color(p.text_primary))
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

/// `UserVoiceTab`'s "Input & output" section: the devices WebRTC's audio module sees.
// shortcut: volume, voice processing, mic test, entrance sound and video settings aren't here yet.
fn voice<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let Some((inputs, outputs)) = &st.voice.devices else {
        return section("Input & output", text("Looking for devices…").font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted), true, p);
    };
    let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
    let options = |names: &[String]| -> Vec<(String, String)> {
        std::iter::once((String::new(), "Default".to_string())).chain(names.iter().map(|n| (n.clone(), n.clone()))).collect()
    };
    let (ins, outs) = (options(inputs), options(outputs));
    let pick = |id: &str, label: &str, opts: &[(String, String)], value: &Option<String>, output: bool, layer: &mut Vec<El>| {
        let refs: Vec<(&str, &str)> = opts.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
        crate::ui::Select::new(id, &refs, Some(value.as_deref().unwrap_or("")))
            .label(label)
            .width(340.0)
            .on_choose(move |s: &mut S, v| {
                let sh = acc(s);
                let v = (!v.is_empty()).then(|| v.to_string());
                if output { sh.voice.output = v } else { sh.voice.input = v }
                sh.intents.push(Intent::VoiceDevices);
            })
            .el(&st.popups, pops, p, layer)
    };
    let content = col()
        .w_full()
        .gap(16.0)
        .child(pick("voice-input", "Input device", &ins, &st.voice.input, false, layer))
        .child(pick("voice-output", "Output device", &outs, &st.voice.output, true, layer));
    section("Input & output", content, true, p)
}

fn appearance<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let link = text("\"Themes\"")
        .font(font(14.0, 400.0).lh(20.0))
        .color(p.text_link)
        .cursor(rune::Cursor::Pointer)
        .role("link")
        .on_press(select(acc, "themes"));
    let line = row()
        .w_full()
        .child(text("You can pick a dark or light theme and change colors and quick CSS in ").font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted))
        .child(link)
        .child(text(".").font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted));
    // shortcut: messages display (cozy/compact), interface, channel list and active now sections
    // need shell support first.
    col()
        .w_full()
        .child(section("Theme", line, true, p))
        .child(super::desktop_tabs::appearance(st, p, acc))
        .child(section("More appearance settings", not_available(p), false, p))
}

/// `ThemeSelector` options: (setting value, label, swatch).
const THEMES: [(&str, &str, [u8; 3]); 5] = [
    ("dark", "Dark theme", [29, 28, 34]),
    ("dark_legacy", "Dark (legacy) theme", [25, 27, 32]),
    ("coal", "Coal theme", [5, 5, 6]),
    ("light", "Light theme", [251, 251, 252]),
    ("system", "System", [12, 11, 14]),
];

fn themes<S: 'static>(store: &Store, p: &Palette, acc: ShellAccess<S>) -> El {
    let current = match crate::theme::Kind::from_setting(&store.settings.theme) {
        crate::theme::Kind::Dark => "dark",
        crate::theme::Kind::Light => "light",
        crate::theme::Kind::Coal => "coal",
    };
    let buttons = row().gap(12.0).role("radiogroup").label("Theme").children(THEMES.iter().map(|&(value, label, [r, g, b])| {
        // shortcut: no legacy palette and no system light/dark detection yet, so those can't be picked.
        let (light, ok) = (value == "light", !matches!(value, "dark_legacy" | "system"));
        let selected = ok && current == value;
        let border = if selected { p.brand_primary_foreground } else if light { p.border_color } else { Color::WHITE };
        let swatch = rgb(r, g, b);
        let mut b = el()
            .size(56.0, 56.0)
            .radius(28.0)
            .fill(swatch)
            .border(2.0, over(border, swatch))
            .role("radio")
            .checked(selected)
            .label(label);
        if value == "system" {
            b = b.child(row().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().child(icon(&icons::ARROWS_COUNTER_CLOCKWISE_BOLD, 24.0).color(Color::WHITE)));
        }
        if selected {
            b = b.child(
                row()
                    .pin(None, Some(-4.0), Some(-4.0), None)
                    .size(20.0, 20.0)
                    .radius(10.0)
                    .center()
                    .fill(p.brand_primary)
                    .child(icon(&icons::CHECK_BOLD, 12.0).color(p.brand_primary_fill)),
            );
        }
        if !ok {
            b.opacity(0.48).disabled(true)
        } else if selected {
            b
        } else {
            b.cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| acc(s).intents.push(Intent::PatchSettings(json!({ "theme": value }))))
        }
    }));
    // shortcut: "Sync theme across devices" is always on (the theme is saved to the account).
    section("Base theme", buttons, true, p)
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
                .child(text("Developer mode").font(font(14.0, 600.0).lh(18.9)).color(p.text_primary))
                .child(text("Enable developer mode").font(font(13.0, 400.0).lh(18.2)).color(p.text_secondary)),
        )
        .child(
            Switch::new(on)
                .on_toggle(move |s: &mut S| acc(s).intents.push(Intent::PatchSettings(json!({ "developer_mode": !on }))))
                .el(p),
        )
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.border_color));
    // shortcut: only developer mode; Fluxer's other advanced settings aren't implemented.
    section("Developer", row, true, p)
}
