//! Settings tabs for the desktop integration: Notifications (`notifications_tab/`), Keybinds
//! (`keybinds_tab/DefaultKeybindsList.tsx`), Language (`LanguageTab.tsx`), the Linux app tab
//! (`DesktopSettingsTab.tsx`), and Appearance's Scaling and Streaming privacy sections
//! (`appearance_tab/{ScalingTab,StreamerModeTab}.tsx`).

use rune::{El, col, el, row, text};

use super::tabs::section;
use crate::desktop::Prefs;
use crate::i18n::{self, t, tf};
use crate::keybinds::{BINDS, SECTIONS};
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::Switch;

fn switch<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, label: &str, desc: Option<String>, get: fn(&Prefs) -> bool, put: fn(&mut Prefs, bool)) -> El {
    switch_if(st, p, acc, label, desc, false, get, put)
}

#[allow(clippy::too_many_arguments)]
fn switch_if<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, label: &str, desc: Option<String>, disabled: bool, get: fn(&Prefs) -> bool, put: fn(&mut Prefs, bool)) -> El {
    let on = get(&st.prefs);
    let mut sw = Switch::new(on).label(label.to_string()).disabled(disabled);
    if let Some(d) = desc {
        sw = sw.description(d);
    }
    sw.on_toggle(move |s: &mut S| {
        let sh = acc(s);
        let mut prefs = sh.prefs.clone();
        put(&mut prefs, !on);
        sh.intents.push(Intent::Prefs(prefs));
    })
    .el(p)
}

/// "Enable desktop notifications" (`Notifications.tsx`).
pub fn notifications_switch<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let product = [("productName", "Fluxer")];
    switch(
        st,
        p,
        acc,
        t("Enable desktop notifications"),
        Some(tf("Get notified when you receive messages. You may need to allow notifications for {productName} in your device settings. For per-channel/per-community controls, open notification settings from a community's menu.", &product)),
        |p| p.notifications,
        |p, v| p.notifications = v,
    )
}

pub fn desktop<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let mac = cfg!(target_os = "macos");
    let pr = &st.prefs;
    let login = switch(st, p, acc, &tf("Launch {productName} at login", &[("productName", "Fluxer")]), None, |p| p.autostart, |p, v| p.autostart = v);
    let minimized = switch_if(
        st,
        p,
        acc,
        t("Start minimized"),
        Some(t(if mac { "When launched at login, start in the menu bar instead of opening the window" } else { "When launched at login, start in the system tray instead of opening the window" }).into()),
        !pr.autostart || !pr.tray,
        |p| p.start_minimized,
        |p, v| p.start_minimized = v,
    );
    let remember = switch(st, p, acc, t("Remember window size and position"), None, |p| p.remember_window, |p, v| p.remember_window = v);
    let tray = switch(st, p, acc, t(if mac { "Show menu bar icon" } else { "Show system tray icon" }), None, |p| p.tray, |p, v| p.tray = v);
    let close = switch_if(st, p, acc, t(if mac { "Close to menu bar" } else { "Close to tray" }), None, !pr.tray, |p| p.close_to_tray, |p, v| p.close_to_tray = v);
    let window = col().w_full().gap(16.0).child(login).child(minimized).child(remember).child(tray).child(close);
    let global = switch(
        st,
        p,
        acc,
        t("Global keybind"),
        Some(t(if cfg!(windows) { "Ctrl+Shift+M mutes and Ctrl+Shift+D deafens from any app." } else { "Mute, deafen and push-to-talk from any app. Assign the keys in your system settings." }).into()),
        |p| p.global_shortcuts,
        |p, v| p.global_shortcuts = v,
    );
    col().w_full().child(section(t("Desktop window"), window, true, p)).child(section(t("Keybinds"), global, false, p))
}

/// Appearance → Scaling (`AppZoomLevelTabContent`).
pub fn scaling<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    use crate::desktop::window::{ZOOM_MAX, ZOOM_MIN, clamp_zoom};
    let pct = (st.prefs.zoom * 100.0).round();
    let slider = crate::ui::Slider::new("app-zoom", pct, ZOOM_MIN * 100.0, ZOOM_MAX * 100.0)
        .step(10.0)
        .on_change(move |s: &mut S, v| {
            let sh = acc(s);
            let mut prefs = sh.prefs.clone();
            prefs.zoom = clamp_zoom(v / 100.0);
            crate::desktop::window::set_zoom(prefs.zoom);
            sh.intents.push(Intent::Prefs(prefs));
        })
        .el(p);
    let zoom = col()
        .w_full()
        .gap(8.0)
        .child(
            row()
                .w_full()
                .child(text(t("App zoom level")).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0))
                .child(text(format!("{pct}%")).font(font(14.0, 500.0).lh(20.0)).color(p.text_secondary)),
        )
        .child(slider)
        .child(
            row()
                .gap(8.0)
                .child(text("Ctrl + = / Ctrl + - / Ctrl + 0").font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary))
                .child(text(t("Reset zoom")).font(font(12.0, 400.0).lh(16.0)).color(p.text_link).cursor(rune::Cursor::Pointer).role("button").on_press(move |s: &mut S| {
                    let sh = acc(s);
                    let mut prefs = sh.prefs.clone();
                    prefs.zoom = 1.0;
                    crate::desktop::window::set_zoom(1.0);
                    sh.intents.push(Intent::Prefs(prefs));
                })),
        );
    section(t("App zoom level"), zoom, false, p)
}

/// Appearance → Streaming privacy (`StreamerModeTab`).
pub fn streaming<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let s = &st.prefs.streamer;
    let off = !s.manual && !s.auto;
    let privacy = col()
        .w_full()
        .gap(16.0)
        .child(switch(st, p, acc, t("Turn on streaming privacy"), Some(t("Mask private details before sharing your screen or stream.").into()), |p| p.streamer.manual, |p, v| {
            p.streamer.manual = v;
            p.streamer.nagbar_dismissed = false;
        }))
        .child(switch(st, p, acc, t("Turn on automatically when OBS or XSplit is detected"), None, |p| p.streamer.auto, |p, v| {
            p.streamer.auto = v;
            p.streamer.nagbar_dismissed = false;
        }))
        .child(text(t("When streaming privacy is on")).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary))
        .child(switch_if(st, p, acc, t("Mask private account details like email, linked accounts, and notes"), None, off, |p| p.streamer.hide_personal, |p, v| p.streamer.hide_personal = v))
        .child(switch_if(st, p, acc, t("Mask community invite links"), None, off, |p| p.streamer.hide_invites, |p, v| p.streamer.hide_invites = v))
        .child(switch_if(st, p, acc, t("Mute app sound effects"), None, off, |p| p.streamer.disable_sounds, |p, v| p.streamer.disable_sounds = v))
        .child(switch_if(st, p, acc, t("Pause notifications"), None, off, |p| p.streamer.disable_notifications, |p, v| p.streamer.disable_notifications = v));
    section(t("Streaming privacy"), privacy, false, p)
}

/// `StreamerModeGate`: account details stay hidden while streaming privacy is on.
pub fn streamer_gate<S: 'static>(p: &Palette, acc: ShellAccess<S>) -> El {
    col()
        .w_full()
        .py(48.0)
        .gap(8.0)
        .items_center()
        .child(text(t("Account and security details are hidden")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary))
        .child(text(t("Turn off streaming privacy before viewing login or account security details.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap())
        .child(
            crate::ui::Button::new(t("Open streaming privacy settings"))
                .key("streamer-gate-open")
                .on_press(super::select(acc, "appearance"))
                .el(p),
        )
}

/// One key cap (`KeyboardKey`): a small raised chip.
fn cap(label: String, p: &Palette) -> El {
    row()
        .min_w(22.0)
        .h(22.0)
        .px(6.0)
        .center()
        .radius(4.0)
        .fill(p.background_tertiary)
        .border(1.0, p.background_modifier_accent)
        .child(text(label).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary))
}

pub fn keybinds(p: &Palette) -> El {
    let mut out = col().w_full();
    for (i, &(id, heading)) in SECTIONS.iter().enumerate() {
        let mut rows = col().w_full();
        let binds: Vec<_> = BINDS.iter().filter(|b| b.section == id).collect();
        // Rows with the same label (prev / next) merge into one, combos separated by "/".
        let mut k = 0;
        while k < binds.len() {
            let label = binds[k].label;
            let mut combos = row().gap(6.0).center_y();
            let mut first = true;
            while k < binds.len() && binds[k].label == label {
                if !first {
                    combos = combos.child(text("/").font(font(13.0, 400.0)).color(p.text_tertiary));
                }
                first = false;
                combos = combos.children(binds[k].combo.caps().into_iter().map(|c| cap(c, p)));
                k += 1;
            }
            rows = rows.child(
                row()
                    .w_full()
                    .py(12.0)
                    .gap(12.0)
                    .center_y()
                    .child(text(t(label)).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0).min_w(0.0).wrap())
                    .child(combos)
                    .child(el().pin(None, Some(0.0), Some(0.0), Some(0.0)).h(1.0).fill(p.background_modifier_accent)),
            );
        }
        out = out.child(section(t(heading), rows, i == 0, p));
    }
    out
}

pub fn language<S: 'static>(store: &reflux_store::Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let current = i18n::locale();
    let list = col().w_full().gap(4.0).role("radiogroup").label(t("Select interface language")).children(i18n::LOCALES.iter().map(|&(code, name)| {
        let on = code == current;
        let fill = if on { p.background_modifier_selected } else { rune::Color::TRANSPARENT };
        let r = row()
            .w_full()
            .h(40.0)
            .px(12.0)
            .gap(12.0)
            .center_y()
            .radius(6.0)
            .fill(fill)
            .role("radio")
            .checked(on)
            .label(name)
            .child(crate::ui::radio_dot(on, p))
            .child(text(name).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0))
            .child(text(code).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary));
        if on { r } else { r.cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| acc(s).intents.push(Intent::SetLocale(code))) }
    }));
    col()
        .w_full()
        .child(section(t("Interface language"), list, true, p))
        .child(section(t("Time format"), time_format(store, p, acc), false, p))
        .child(section(t("Spellcheck"), spellcheck(st, p, acc), false, p))
}

/// `timeFormatOptions`: Auto / 12-hour / 24-hour with an example each.
fn time_format<S: 'static>(store: &reflux_store::Store, p: &Palette, acc: ShellAccess<S>) -> El {
    use crate::desktop::time;
    let now = chrono::Local::now();
    let locale = i18n::locale();
    let current = store.settings.time_format;
    // The examples print with the format each option picks, then the setting goes back.
    let example = |f: u8| {
        time::set_format(f);
        let e = time::time(&now, locale);
        time::set_format(current);
        e
    };
    let (auto, h12, h24) = (example(0), example(1), example(2));
    let options = [("0", t("Auto"), Some(auto.as_str())), ("1", t("12-hour"), Some(h12.as_str())), ("2", t("24-hour"), Some(h24.as_str()))];
    let value = current.to_string();
    crate::ui::RadioGroup::new(&options, Some(&value))
        .on_change(move |s: &mut S, v: &str| {
            let n: u8 = v.parse().unwrap_or(0);
            acc(s).intents.push(Intent::PatchSettings(serde_json::json!({ "time_format": n })));
        })
        .el(p)
}

/// Spellcheck on/off, its dictionaries (none picked = the interface language's) and the words
/// added from the composer's menu.
fn spellcheck<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    use crate::desktop::spell::{DICTS, wanted};
    let mut c = col().w_full().gap(16.0).child(switch(st, p, acc, t("Enable spellcheck"), None, |p| p.spellcheck, |p, v| p.spellcheck = v));
    if !st.prefs.spellcheck {
        return c;
    }
    let active = wanted(&st.prefs);
    let langs = col().w_full().gap(8.0).children(DICTS.iter().map(|d| {
        let on = active.contains(&d.package);
        let package = d.package;
        crate::ui::Checkbox::new(on)
            .size(18.0)
            .label(d.name)
            .on_toggle(move |s: &mut S| {
                let sh = acc(s);
                let mut prefs = sh.prefs.clone();
                // Start from what's in effect, so the first pick keeps the language's own.
                let mut list: Vec<String> = wanted(&prefs).into_iter().map(String::from).collect();
                if on {
                    list.retain(|p| p != package);
                } else {
                    list.push(package.into());
                }
                prefs.spell_langs = list;
                sh.intents.push(Intent::Prefs(prefs));
            })
            .el(p)
    }));
    c = c
        .child(text(t("Languages")).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary))
        .child(text(t("Pick one or more dictionaries. Words from any selected dictionary are accepted, which is useful for multilingual chats.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap())
        .child(langs);
    if !st.prefs.words.is_empty() {
        let words = col().w_full().gap(4.0).children(st.prefs.words.iter().map(|w| {
            let word = w.clone();
            let label = tf("Remove \"{word}\" from dictionary", &[("word", w)]);
            row()
                .w_full()
                .h(32.0)
                .gap(8.0)
                .center_y()
                .child(text(w.clone()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0))
                .child(
                    rune::icon(&crate::icons::X_BOLD, 14.0)
                        .color(p.text_tertiary)
                        .cursor(rune::Cursor::Pointer)
                        .role("button")
                        .label(label)
                        .on_press(move |s: &mut S| {
                            let sh = acc(s);
                            let mut prefs = sh.prefs.clone();
                            prefs.words.retain(|x| *x != word);
                            sh.intents.push(Intent::Prefs(prefs));
                        }),
                )
        }));
        c = c.child(text(t("Dictionary")).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary)).child(words);
    }
    c
}
