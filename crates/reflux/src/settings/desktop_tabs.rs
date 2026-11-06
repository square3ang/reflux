//! Settings tabs for the desktop integration: Notifications (`notifications_tab/`), Keybinds
//! (`keybinds_tab/DefaultKeybindsList.tsx`), Language (`LanguageTab.tsx`) and the Linux app tab
//! (`DesktopSettingsTab.tsx`).

use rune::{El, col, el, row, text};

use super::tabs::section;
use crate::desktop::Prefs;
use crate::i18n::{self, t, tf};
use crate::keybinds::{BINDS, SECTIONS};
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::Switch;

fn switch<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, label: &str, desc: Option<String>, get: fn(&Prefs) -> bool, put: fn(&mut Prefs, bool)) -> El {
    let on = get(&st.prefs);
    let mut sw = Switch::new(on).label(label.to_string());
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

pub fn notifications<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let product = [("productName", "Fluxer")];
    let notes = switch(
        st,
        p,
        acc,
        t("Enable desktop notifications"),
        Some(tf("Get notified when you receive messages. You may need to allow notifications for {productName} in your device settings. For per-channel/per-community controls, open notification settings from a community's menu.", &product)),
        |p| p.notifications,
        |p, v| p.notifications = v,
    );
    let sounds = switch(st, p, acc, t("Disable all notification sounds"), Some(t("Your existing notification sound settings will be preserved.").into()), |p| !p.sounds, |p, v| p.sounds = !v);
    col().w_full().child(section(t("Notifications"), notes, true, p)).child(section(t("Sounds"), sounds, false, p))
}

pub fn desktop<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let mac = cfg!(target_os = "macos");
    let tray = switch(st, p, acc, t(if mac { "Show menu bar icon" } else { "Show system tray icon" }), None, |p| p.tray, |p, v| p.tray = v);
    let mut c = col().w_full().gap(16.0).child(tray);
    if st.prefs.tray {
        c = c.child(switch(st, p, acc, t(if mac { "Close to menu bar" } else { "Close to tray" }), None, |p| p.close_to_tray, |p, v| p.close_to_tray = v));
    }
    section(t("Desktop window"), c, true, p)
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

pub fn language<S: 'static>(p: &Palette, acc: ShellAccess<S>) -> El {
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
    section(t("Interface language"), list, true, p)
}
