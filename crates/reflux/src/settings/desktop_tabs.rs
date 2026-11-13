//! Settings tabs for the desktop integration: Notifications (`notifications_tab/`), Keybinds
//! (`keybinds_tab/DefaultKeybindsList.tsx`), Language (`LanguageTab.tsx`), the Linux app tab
//! (`DesktopSettingsTab.tsx`), and Appearance's Scaling and Streaming privacy sections
//! (`appearance_tab/{ScalingTab,StreamerModeTab}.tsx`).

use rune::{El, col, row};
use crate::ui::mark::marked as text;

use super::tabs::section;
use crate::desktop::Prefs;
use crate::i18n::{self, t, tf};
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
        sh.intents.push(Intent::Prefs(Box::new(prefs)));
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
    let switch_row = switch(
        st,
        p,
        acc,
        t("Global keybind"),
        Some(t(if cfg!(windows) { "Ctrl+Shift+M mutes and Ctrl+Shift+D deafens from any app." } else { "Mute, deafen and push-to-talk from any app. Assign the keys in your system settings." }).into()),
        |p| p.global_shortcuts,
        |p, v| p.global_shortcuts = v,
    );
    // The switch is a label | toggle row; the button goes under it, not into it.
    let mut global = col().w_full().child(switch_row);
    // Wayland's portal: the desktop owns the keys, so it also owns the dialog that sets them
    // (`GlobalShortcutsPortal.configure`). Without assigned keys the shortcuts never fire.
    if crate::desktop::shortcuts::portal() {
        global = global.child(
            row()
                .w_full()
                .mt(8.0)
                .child(
                    crate::ui::Button::new(t("Configure global shortcuts…"))
                        .variant(crate::ui::ButtonVariant::Secondary)
                        .key("configure-global-shortcuts")
                        .on_press(|_: &mut S| crate::desktop::shortcuts::configure())
                        .el(p),
                ),
        );
    }
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
            sh.intents.push(Intent::Prefs(Box::new(prefs)));
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
                    sh.intents.push(Intent::Prefs(Box::new(prefs)));
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
    let sub = |title: &str, content: El| col().w_full().gap(12.0).child(text(title.to_string()).font(font(15.0, 600.0).lh(19.5)).color(p.text_primary)).child(content);
    let hint = |s: &str| text(s.to_string()).font(font(13.0, 400.0).lh(17.6)).color(p.text_primary_muted).wrap();
    let mut c = col().w_full().gap(16.0).child(switch(st, p, acc, t("Enable spellcheck"), None, |p| p.spellcheck, |p, v| p.spellcheck = v));
    if !st.prefs.spellcheck {
        return c;
    }
    // Languages: detect while typing, else the dictionaries picked (`.dictionaryGrid`).
    let mut langs = col().w_full().gap(12.0).child(switch(st, p, acc, t("Detect while typing"), None, |p| p.spell_auto, |p, v| p.spell_auto = v));
    if !st.prefs.spell_auto {
        let active = wanted(&st.prefs);
        let cell = |d: &'static crate::desktop::spell::Dict| {
            let on = active.contains(&d.package);
            let package = d.package;
            let r = row()
                .w(0.0)
                .grow(1.0)
                .min_w(0.0)
                .px(10.0)
                .py(8.0)
                .gap(8.0)
                .center_y()
                .radius(6.0)
                .cursor(rune::Cursor::Pointer)
                .role("checkbox")
                .checked(on)
                .label(d.name)
                .on_press(move |s: &mut S| {
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
                    sh.intents.push(Intent::Prefs(Box::new(prefs)));
                })
                .child(crate::ui::Checkbox::<S>::new(on).size(18.0).el(p))
                .child(text(d.name).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).grow(1.0).min_w(0.0).truncate())
                .child(text(d.tag).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted));
            if on { r.fill(p.background_modifier_selected) } else { r.hover(p.background_modifier_hover) }
        };
        let mut grid = col().w_full().gap(8.0);
        for pair in DICTS.chunks(2) {
            grid = grid.child(row().w_full().gap(8.0).children(pair.iter().map(cell)).children((pair.len() == 1).then(|| rune::el().w(0.0).grow(1.0))));
        }
        langs = langs.child(hint(t("Pick one or more dictionaries. Words from any selected dictionary are accepted, which is useful for multilingual chats."))).child(grid);
    }
    c = c.child(sub(t("Languages"), langs));
    // Personal dictionary: add a word, then the words as removable chips.
    let typed = st.form.text.get("spell.word").map(|e| e.text.trim().to_string()).unwrap_or_default();
    let add = move |s: &mut S| {
        let sh = acc(s);
        let word = sh.form.text.get("spell.word").map(|e| e.text.trim().to_string()).unwrap_or_default();
        if word.is_empty() {
            return;
        }
        let mut prefs = sh.prefs.clone();
        if !prefs.words.contains(&word) {
            prefs.words.push(word);
        }
        sh.form.text.insert("spell.word".into(), rune::TextEdit::new(""));
        sh.intents.push(Intent::Prefs(Box::new(prefs)));
    };
    let edit = st.form.text.get("spell.word").cloned().unwrap_or_else(|| rune::TextEdit::new(""));
    let field = rune::text_input(&edit, font(14.0, 400.0).lh(20.0))
        .single_line()
        .key("spell-word")
        .grow(1.0)
        .min_w(0.0)
        .px(10.0)
        .py(8.0)
        .radius(6.0)
        .fill(p.form_surface_background)
        .border(1.0, p.background_modifier_accent)
        .color(p.text_primary)
        .placeholder(t("Add a word…"), p.text_tertiary)
        .caret(p.text_primary, p.background_modifier_selected)
        .on_edit(move |s: &mut S| -> &mut rune::TextEdit { acc(s).form.text.entry("spell.word".into()).or_insert_with(|| rune::TextEdit::new("")) })
        .on_submit(move |s: &mut S, _| add(s));
    let add_row = row().w_full().gap(8.0).center_y().child(field).child(
        crate::ui::Button::new(t("Add")).variant(crate::ui::ButtonVariant::Secondary).size(crate::ui::ButtonSize::Small).disabled(typed.is_empty()).key("spell-add").on_press(add).el(p),
    );
    let words = if st.prefs.words.is_empty() {
        text(t("No personal words yet.")).key("none").font(font(13.0, 400.0).lh(17.6).italic()).color(p.text_primary_muted)
    } else {
        row().w_full().flex_wrap().gaps(6.0, 6.0).children(st.prefs.words.iter().map(|w| {
            let word = w.clone();
            let label = tf("Remove \"{word}\" from dictionary", &[("word", w)]);
            row()
                .key(format!("w-{w}"))
                .py(4.0)
                .pl(10.0)
                .pr(8.0)
                .gap(4.0)
                .center_y()
                .radius(999.0)
                .fill(p.background_modifier_accent)
                .child(text(w.clone()).font(font(13.0, 400.0).lh(17.0)).color(p.text_primary))
                .child(
                    text("×")
                        .font(font(14.0, 400.0).lh(14.0))
                        .color(p.text_primary_muted)
                        .px(2.0)
                        .cursor(rune::Cursor::Pointer)
                        .role("button")
                        .label(label)
                        .on_press(move |s: &mut S| {
                            let sh = acc(s);
                            let mut prefs = sh.prefs.clone();
                            prefs.words.retain(|x| *x != word);
                            sh.intents.push(Intent::Prefs(Box::new(prefs)));
                        }),
                )
        }))
    };
    c.child(sub(t("Personal dictionary"), col().w_full().gap(12.0).child(add_row).child(words)))
}
