//! Custom keybinds (`user/components/modals/tabs/keybinds_tab`): the Keybinds settings tab with
//! its custom list, recorder (`KeybindRecorder`) and per-row menus, the Ctrl+/ list
//! (`KeyboardShortcutsCheatsheetModal`) and the first-Tab `KeyboardModeIntroModal`. Kept per
//! account on this device ([`super::AccountPrefs`]).
// shortcut: no search box, "Sync across devices" or global (system-wide) custom keybinds.

use rune::{Color, El, Key, KeyEvent, Shadow, col, el, icon, row, text};
use serde::{Deserialize, Serialize};

use super::{AccountPrefs, mine, update_mine};
use crate::i18n::t;
use crate::icons;
use crate::keybinds::{Action, BINDS, Combo, SECTIONS};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::menu::{Menu, MenuItem};
use crate::ui::{Button, ButtonSize, ButtonVariant, Select, Switch, popup_access, rgba};

/// One custom keybind: an action (`None` = "(unassigned)"), its combo, and whether it's on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Custom {
    pub action: Option<Action>,
    pub combo: Option<Combo>,
    pub enabled: bool,
}

/// The open keybind row menu.
#[derive(Clone, Debug, PartialEq)]
pub enum RowMenu {
    Default(Vec<Action>),
    Custom(usize),
}

/// Whether a built-in still runs: built-ins aren't switched off, this one isn't disabled, and
/// no custom keybind took its action (`getSuppressedBuiltinActions`).
pub fn builtin_on(m: &AccountPrefs, a: Action) -> bool {
    !m.builtins_off && !m.disabled_builtins.contains(&a) && !m.keybinds.iter().any(|c| c.action == Some(a))
}

/// The action a key runs: an enabled custom keybind first, then the built-ins left on.
pub fn action(m: &AccountPrefs, k: &KeyEvent) -> Option<Action> {
    m.keybinds
        .iter()
        .filter(|c| c.enabled && c.combo.is_some_and(|x| x.matches(k)))
        .find_map(|c| c.action)
        .or_else(|| BINDS.iter().find(|b| builtin_on(m, b.action) && b.combo.matches(k)).map(|b| b.action))
}

/// Every combo that runs `a` now (`getActiveCombosForAction`).
fn active(m: &AccountPrefs, a: Action) -> Vec<Combo> {
    let custom = m.keybinds.iter().filter(|c| c.enabled && c.action == Some(a)).filter_map(|c| c.combo);
    let builtin = BINDS.iter().filter(|b| b.action == a && builtin_on(m, a)).map(|b| b.combo);
    custom.chain(builtin).collect()
}

/// For each custom keybind, the label of another enabled one with the same combo.
pub fn conflicts(m: &AccountPrefs) -> Vec<Option<String>> {
    let on = |c: &Custom| c.enabled.then_some(c.combo).flatten();
    m.keybinds
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mine = on(c)?;
            m.keybinds.iter().enumerate().find(|(j, o)| *j != i && on(o) == Some(mine)).map(|(_, o)| o.action.map_or_else(|| t("(unassigned)").to_string(), label))
        })
        .collect()
}

/// An action's name; the prev / next pairs that share one get "(previous)" / "(next)".
pub fn label(a: Action) -> String {
    use Action::*;
    let base = BINDS.iter().find(|b| b.action == a).map_or("", |b| b.label);
    match a {
        GuildPrev | ChannelPrev | UnreadPrev | MentionPrev => format!("{} ({})", t(base), t("previous")),
        GuildNext | ChannelNext | UnreadNext | MentionNext => format!("{} ({})", t(base), t("next")),
        _ => t(base).to_string(),
    }
}

/// Keys the shell takes before any keybind: the recorder's capture ("Keybinds are paused
/// while recording"), and the first Tab with nothing focused turning on keyboard mode.
// shortcut: a Tab inside a field moves focus without showing the intro.
pub fn on_key(sh: &mut ShellState, k: &KeyEvent) -> bool {
    if let Some(i) = sh.nav.recording {
        if k.key == Key::Escape && k.mods == rune::Mods::default() {
            sh.nav.recording = None;
        } else if let Some(combo) = Combo::from_event(k) {
            sh.nav.recording = None;
            update_mine(sh, |m| {
                if let Some(c) = m.keybinds.get_mut(i) {
                    c.combo = Some(combo);
                }
            });
        }
        return true;
    }
    if k.key == Key::Tab && !mine(sh).keyboard_intro_seen && sh.settings.is_none() && sh.dialog.is_none() {
        sh.nav.keyboard_intro = true;
        update_mine(sh, |m| m.keyboard_intro_seen = true);
    }
    false
}

/// `KeyboardKey`: a small raised chip.
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

fn caps(combo: &Combo, p: &Palette) -> El {
    row().gap(4.0).center_y().children(combo.caps().into_iter().map(|c| cap(c, p)))
}

/// `.dotsButton`: 28 square, muted, opens the row menu.
fn dots<S: 'static>(id: String, menu: RowMenu, p: &Palette, acc: ShellAccess<S>) -> El {
    let key = id.clone();
    row()
        .key(id)
        .size(28.0, 28.0)
        .center()
        .radius(8.0)
        .hover(p.background_modifier_hover)
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(t("More keybind options"))
        .child(icon(&icons::DOTS_THREE_VERTICAL, 18.0).color(p.text_primary_muted).group_color(p.text_primary))
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.nav.key_menu = Some(menu.clone());
            sh.popups.toggle(&key);
        })
}

fn add_custom(m: &mut AccountPrefs, action: Option<Action>) {
    m.keybinds.push(Custom { action, combo: None, enabled: true });
}

/// The Keybinds tab: the custom list, then the built-ins by section.
pub fn tab<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let m = mine(st);
    let mut out = col().w_full().gap(24.0).child(custom_list(st, &m, p, acc, layer));
    for (i, &(id, heading)) in SECTIONS.iter().enumerate() {
        out = out.child(crate::settings::tabs::section(t(heading), defaults(st, &m, id, p, acc), i == 0, p));
    }
    out
}

/// `CustomKeybindsList`.
fn custom_list<S: 'static>(st: &ShellState, m: &AccountPrefs, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let any = !m.keybinds.is_empty() || m.builtins_off || !m.disabled_builtins.is_empty();
    let head = row()
        .w_full()
        .gap(12.0)
        .center_y()
        .justify_between()
        .child(
            col()
                .gap(2.0)
                .min_w(0.0)
                .child(text(t("Custom keybinds")).font(font(15.0, 600.0).lh(20.0)).color(p.text_primary))
                .child(text(t("Keybinds are paused while recording.")).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted)),
        )
        .child(
            row()
                .gap(8.0)
                .center_y()
                .children(any.then(|| Button::new(t("Reset to defaults")).key("kb-reset").variant(ButtonVariant::Secondary).size(ButtonSize::Small).on_press(move |s: &mut S| acc(s).nav.confirm_reset_keys = true).el(p)))
                .child(Button::new(t("Add keybind")).key("kb-add").size(ButtonSize::Small).icon(&icons::PLUS_BOLD).on_press(move |s: &mut S| update_mine(acc(s), |m| add_custom(m, None))).el(p)),
        );
    let off = m.builtins_off;
    let options = col().w_full().gap(8.0).px(12.0).py(10.0).radius(8.0).border(1.0, p.background_modifier_accent).fill(p.background_secondary).child(
        Switch::new(off).label(t("Disable built-ins")).on_toggle(move |s: &mut S| update_mine(acc(s), |m| m.builtins_off = !off)).el(p),
    );
    let mut list = col().w_full().gap(8.0);
    if m.keybinds.is_empty() {
        list = list.child(text(t("No custom keybinds yet.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted));
    }
    let clash = conflicts(m);
    let options_list: Vec<(String, String)> = BINDS.iter().map(|b| (format!("{:?}", b.action), label(b.action))).collect();
    let opts: Vec<(&str, &str)> = options_list.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let ids: Vec<String> = (0..m.keybinds.len()).map(|i| format!("kb-action-{i}")).collect();
    for (i, c) in m.keybinds.iter().enumerate() {
        list = list.child(custom_row(st, i, c, clash[i].as_deref(), &ids[i], &opts, p, acc, layer));
    }
    col().w_full().gap(12.0).child(head).child(options).child(list)
}

/// `CustomKeybindRow`: action, recorder, enabled switch and menu; a conflict warning under it.
#[allow(clippy::too_many_arguments)]
fn custom_row<S: 'static>(st: &ShellState, i: usize, c: &Custom, clash: Option<&str>, id: &str, opts: &[(&str, &str)], p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let field_label = |s: &str| text(t(s)).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted);
    let value = c.action.map(|a| format!("{a:?}"));
    let pick = Select::new(id, opts, value.as_deref())
        .placeholder(t("(unassigned)"))
        .width(260.0)
        .on_choose(move |s: &mut S, v: &str| {
            let a = BINDS.iter().find(|b| format!("{:?}", b.action) == v).map(|b| b.action);
            update_mine(acc(s), |m| {
                if let Some(c) = m.keybinds.get_mut(i) {
                    c.action = a;
                }
            });
        })
        .el(&st.popups, popup_access(move |s: &mut S| &mut acc(s).popups), p, layer);
    let on = c.enabled;
    let main = row()
        .w_full()
        .gap(12.0)
        .items_end()
        .child(col().gap(6.0).child(field_label("Action")).child(pick))
        .child(col().gap(6.0).grow(1.0).min_w(0.0).child(field_label("Keybind")).child(recorder(st, i, c.combo, p, acc)))
        .child(
            row()
                .gap(4.0)
                .h(34.0)
                .center_y()
                .child(Switch::new(on).on_toggle(move |s: &mut S| update_mine(acc(s), |m| {
                    if let Some(c) = m.keybinds.get_mut(i) {
                        c.enabled = !on;
                    }
                })).el(p).label(if on { t("Disable keybind") } else { t("Enable keybind") }))
                .child(dots(format!("kb-custom-{i}"), RowMenu::Custom(i), p, acc)),
        );
    let mut r = col().w_full().gap(8.0).px(12.0).py(10.0).radius(8.0).border(1.0, p.background_modifier_accent).fill(p.background_secondary).opacity(if on { 1.0 } else { 0.62 }).child(main);
    if c.action.is_none() {
        r = r.child(text(t("Choose an action.")).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted));
    }
    if let Some(other) = clash {
        r = r.child(
            row()
                .gap(4.0)
                .mt(4.0)
                .center_y()
                .role("alert")
                .child(icon(&icons::WARNING_FILL, 14.0).color(p.accent_warning))
                .child(text(crate::i18n::tf("Conflicts with {conflictLabel}", &[("conflictLabel", other)])).font(font(12.0, 400.0).lh(16.0)).color(p.accent_warning)),
        );
    }
    r.key(format!("kb-custom-row-{i}"))
}

/// `KeybindRecorder`'s trigger: the combo's caps (or "No keybind set"); a press starts
/// recording, shown in the danger colour until a key lands.
// shortcut: records in place instead of in a popout.
fn recorder<S: 'static>(st: &ShellState, i: usize, combo: Option<Combo>, p: &Palette, acc: ShellAccess<S>) -> El {
    let rec = st.nav.recording == Some(i);
    let inner = match (rec, combo) {
        (true, _) => text(t("Press a key combination…")).font(font(14.0, 600.0).lh(18.0)).color(p.status_danger),
        (false, Some(c)) => caps(&c, p),
        (false, None) => text(t("No keybind set")).font(font(14.0, 400.0).lh(18.0)).color(p.text_primary_muted),
    };
    row()
        .key(format!("kb-recorder-{i}"))
        .min_h(34.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .border(1.0, if rec { p.status_danger } else { p.background_modifier_accent })
        .fill(if rec { p.control_button_danger_hover_bg } else { p.form_surface_background })
        .hover(if rec { p.control_button_danger_hover_bg } else { p.form_surface_background })
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(if combo.is_some() { t("Edit keybind") } else { t("Record keybind") })
        .child(icon(&icons::KEYBOARD, 16.0).color(if rec { p.status_danger } else { p.text_primary_muted }).opacity(0.7))
        .child(inner)
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.nav.recording = if rec { None } else { Some(i) };
        })
}

/// `DefaultKeybindsList` for one section: rows merged by label, their built-in chips (dimmed
/// once overridden or disabled) and a menu.
fn defaults<S: 'static>(_st: &ShellState, m: &AccountPrefs, id: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let mut rows = col().w_full();
    let binds: Vec<_> = BINDS.iter().filter(|b| b.section == id).collect();
    let mut k = 0;
    while k < binds.len() {
        let label = binds[k].label;
        let mut combos = row().gap(6.0).center_y();
        let mut actions = Vec::new();
        while k < binds.len() && binds[k].label == label {
            if !actions.is_empty() {
                combos = combos.child(text("/").font(font(13.0, 400.0)).color(p.text_tertiary));
            }
            let b = binds[k];
            combos = combos.child(caps(&b.combo, p).opacity(if builtin_on(m, b.action) { 1.0 } else { 0.4 }));
            actions.push(b.action);
            k += 1;
        }
        let key = format!("kb-default-{:?}", actions[0]);
        rows = rows.child(
            row()
                .w_full()
                .py(12.0)
                .gap(12.0)
                .center_y()
                .child(text(t(label)).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).grow(1.0).min_w(0.0).wrap())
                .child(combos)
                .child(dots(key, RowMenu::Default(actions), p, acc))
                .child(el().pin(None, Some(0.0), Some(0.0), Some(0.0)).h(1.0).fill(p.background_modifier_accent)),
        );
    }
    rows
}

/// The open row menu (`DefaultShortcutRow` / `CustomKeybindRow`).
fn row_menu<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some(menu) = st.nav.key_menu.clone() else { return };
    let m = mine(st);
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let change = move |f: Box<dyn Fn(&mut AccountPrefs)>| {
        move |s: &mut S| {
            let sh = acc(s);
            sh.popups.close();
            update_mine(sh, |m| f(m));
        }
    };
    match menu {
        RowMenu::Custom(i) => {
            if !st.popups.is_open(&format!("kb-custom-{i}")) {
                return;
            }
            Menu::new(format!("kb-custom-{i}"))
                .item(MenuItem::new("kb-delete", t("Delete keybind")).danger().on_select(change(Box::new(move |m| {
                    if i < m.keybinds.len() {
                        m.keybinds.remove(i);
                    }
                }))))
                .el(&st.popups, pops, p, layer);
        }
        RowMenu::Default(actions) => {
            let id = format!("kb-default-{:?}", actions[0]);
            if !st.popups.is_open(&id) {
                return;
            }
            let has = |a: Action| m.keybinds.iter().any(|c| c.action == Some(a));
            let mut menu = Menu::new(id);
            if actions.iter().any(|a| m.disabled_builtins.contains(a)) {
                let acts = actions.clone();
                menu = menu
                    .item(MenuItem::new("kb-reenable", t("Re-enable built-in keybind")).on_select(change(Box::new(move |m| m.disabled_builtins.retain(|a| !acts.contains(a))))))
                    .separator();
            }
            if actions.iter().all(|&a| !has(a)) {
                let acts = actions.clone();
                menu = menu.item(MenuItem::new("kb-set", t("Set custom keybind")).on_select(change(Box::new(move |m| acts.iter().for_each(|&a| add_custom(m, Some(a)))))));
                if !actions.iter().all(|a| m.disabled_builtins.contains(a)) {
                    let acts = actions.clone();
                    menu = menu.item(MenuItem::new("kb-disable", t("Disable built-in keybind")).on_select(change(Box::new(move |m| m.disabled_builtins.extend(acts.iter().copied())))));
                }
            } else if let [a] = actions[..] {
                menu = menu
                    .item(MenuItem::new("kb-reset-one", t("Reset to built-in")).danger().on_select(change(Box::new(move |m| m.keybinds.retain(|c| c.action != Some(a))))))
                    .item(MenuItem::new("kb-another", t("Add another custom keybind")).on_select(change(Box::new(move |m| add_custom(m, Some(a))))));
            } else {
                for (n, &a) in actions.iter().enumerate() {
                    let l = label(a);
                    menu = if has(a) {
                        menu.item(MenuItem::new(format!("kb-reset-{n}"), crate::i18n::tf("Reset \"{label}\" to built-in", &[("label", &l)])).danger().on_select(change(Box::new(move |m| m.keybinds.retain(|c| c.action != Some(a))))))
                    } else {
                        menu.item(MenuItem::new(format!("kb-set-{n}"), crate::i18n::tf("Set custom keybind for \"{label}\"", &[("label", &l)])).on_select(change(Box::new(move |m| add_custom(m, Some(a))))))
                    };
                }
            }
            menu.el(&st.popups, pops, p, layer);
        }
    }
}

/// The overlays: row menus, the reset confirmation, the keybind list and the keyboard intro.
pub fn layer<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    row_menu(st, p, acc, layer);
    if st.nav.confirm_reset_keys {
        let close = move |s: &mut S| acc(s).nav.confirm_reset_keys = false;
        let reset = move |s: &mut S| {
            let sh = acc(s);
            sh.nav.confirm_reset_keys = false;
            update_mine(sh, |m| {
                m.keybinds.clear();
                m.disabled_builtins.clear();
                m.builtins_off = false;
            });
        };
        let footer = row()
            .gap(8.0)
            .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
            .child(Button::new(t("Reset to defaults")).key("kb-reset-confirm").variant(ButtonVariant::Danger).on_press(reset).el(p));
        let body = text(t("This removes every custom keybind and re-enables all built-in keybinds. This cannot be undone.")).font(font(16.0, 400.0).lh(24.0)).color(p.text_primary).wrap();
        layer.push(crate::ui::Modal::new(t("Reset all keybinds?"), close).body(body).footer(footer).el(p));
    }
    if st.nav.cheatsheet {
        layer.push(cheatsheet(st, p, acc));
    }
    if st.nav.keyboard_intro {
        layer.push(intro(p, acc));
    }
}

/// `KeyboardShortcutsCheatsheetModal`: the xlarge modal, sections flowing in 288px columns.
fn cheatsheet<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let m = mine(st);
    let mut sections = Vec::new();
    for &(id, heading) in SECTIONS {
        let binds: Vec<_> = BINDS.iter().filter(|b| b.section == id).collect();
        let mut rows = Vec::new();
        let mut k = 0;
        while k < binds.len() {
            let label = binds[k].label;
            let mut groups = Vec::new();
            while k < binds.len() && binds[k].label == label {
                groups.extend(active(&m, binds[k].action));
                k += 1;
            }
            if !groups.is_empty() {
                rows.push((label, groups));
            }
        }
        if !rows.is_empty() {
            sections.push((heading, rows));
        }
    }
    let chip = |s: String| {
        row()
            .min_w(22.0)
            .h(20.0)
            .px(5.0)
            .center()
            .radius(4.0)
            .border(1.0, p.background_modifier_accent)
            .fill(p.background_primary)
            .box_shadow(Shadow { x: 0.0, y: -1.0, blur: 0.0, spread: 0.0, color: p.background_modifier_accent, inset: true })
            .child(text(s).font(font(11.0, 600.0).lh(11.0)).color(p.text_primary))
    };
    let section = |heading: &str, rows: &[(&str, Vec<Combo>)]| {
        let n = rows.len();
        col()
            .w_full()
            .radius(8.0)
            .border(1.0, p.background_modifier_accent)
            .fill(p.background_secondary_alt)
            .clip_rounded(8.0)
            .child(
                row()
                    .w_full()
                    .px(10.0)
                    .py(8.0)
                    .child(text(t(heading)).font(font(12.0, 700.0).lh(16.0)).color(p.text_primary))
                    .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.background_modifier_accent)),
            )
            .children(rows.iter().enumerate().map(|(i, (label, groups))| {
                let mut r = row()
                    .w_full()
                    .min_h(32.0)
                    .px(10.0)
                    .py(5.0)
                    .gap(10.0)
                    .center_y()
                    .child(text(t(label)).font(font(13.0, 400.0).lh(18.4)).color(p.text_primary).grow(1.0).min_w(0.0).truncate())
                    .child(row().gap(4.0).center_y().children(groups.iter().map(|g| row().gap(3.0).children(g.caps().into_iter().map(chip)))));
                if i + 1 < n {
                    r = r.child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.background_modifier_accent));
                }
                r
            }))
    };
    // CSS columns: as many 288px columns as fit, sections filled in order to a balanced height.
    let width = rune::memo::layout("kb-cheatsheet").map_or(1240.0_f32, |r| r[2]);
    let ncols = (((width - 28.0 + 12.0) / 300.0).floor() as usize).clamp(1, 6);
    let heights: Vec<f32> = sections.iter().map(|(_, r)| 33.0 + 32.0 * r.len() as f32 + 12.0).collect();
    let target = heights.iter().sum::<f32>() / ncols as f32;
    let mut cols: Vec<rune::El> = (0..ncols).map(|_| col().grow(1.0).w(0.0).min_w(0.0).gap(12.0)).collect();
    let (mut c, mut h) = (0, 0.0);
    for (i, (heading, rows)) in sections.iter().enumerate() {
        if h > 0.0 && h + heights[i] / 2.0 > target && c + 1 < ncols {
            c += 1;
            h = 0.0;
        }
        h += heights[i];
        let column = std::mem::replace(&mut cols[c], el());
        cols[c] = column.child(section(heading, rows));
    }
    let close = move |s: &mut S| acc(s).nav.cheatsheet = false;
    let card = col()
        .key("kb-cheatsheet")
        .w_full()
        .max_w(1472.0)
        .h_full()
        .max_h(960.0)
        .radius(8.0)
        .fill(p.background_secondary)
        .border(1.0, crate::ui::over(p.background_header_secondary, p.background_secondary))
        .box_shadow(Shadow { x: 0.0, y: 8.0, blur: 24.0, spread: -4.0, color: rgba(0, 0, 0, 0.25), inset: false })
        .role("dialog")
        .label(t("Keybinds"))
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .child(row().w_full().gap(12.0).px(14.0).py(14.0).items_start().children(cols))
        .modal(true, close);
    el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(Color::BLACK).opacity(0.85))
        .child(col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).padding(20.0).center().child(card))
}

/// `KeyboardModeIntroModal`: small, centred, three tips.
fn intro<S: 'static>(p: &Palette, acc: ShellAccess<S>) -> El {
    let kbd = |s: &str| {
        row()
            .min_w(34.0)
            .px(10.0)
            .py(6.0)
            .center()
            .radius(8.0)
            .border(1.0, p.background_modifier_accent)
            .fill(p.background_secondary)
            .box_shadow(Shadow { x: 0.0, y: 2.0, blur: 4.0, spread: 0.0, color: rgba(0, 0, 0, 0.2), inset: false })
            .child(text(s.to_string()).font(font(13.6, 700.0).lh(13.6)).color(p.text_primary))
    };
    let sep = |s: &str| text(s.to_string()).font(font(14.0, 600.0).lh(18.0)).color(p.text_primary_muted);
    let tip = |keys: El, body: &str| {
        col()
            .w_full()
            .gap(12.0)
            .padding(16.0)
            .items_center()
            .radius(12.0)
            .border(1.0, p.background_modifier_accent)
            .fill(p.background_secondary_alt)
            .child(keys)
            .child(text(t(body)).font(font(16.0, 400.0).lh(22.4)).color(p.text_primary).align(rune::Align::Center).wrap())
    };
    let ctrl = if cfg!(target_os = "macos") { "⌘".to_string() } else { t("Ctrl").to_string() };
    let body = col()
        .w_full()
        .gap(12.0)
        .child(text(crate::i18n::tf("You just pressed Tab. Keyboard mode is now on so you can navigate {productName} without a mouse.", &[("productName", "Reflux")])).font(font(16.0, 400.0).lh(24.0)).color(p.text_primary).wrap())
        .child(
            col()
                .w_full()
                .mt(8.0)
                .gap(10.0)
                .child(tip(row().gap(6.0).center_y().child(kbd(t("Tab"))).child(sep(t("or"))).child(kbd(t("Shift"))).child(sep("+")).child(kbd(t("Tab"))), "Navigate buttons, inputs, and links."))
                .child(tip(row().gap(6.0).center_y().child(kbd("↑")).child(kbd("↓")), "Navigate messages and actions in chat."))
                .child(tip(row().gap(6.0).center_y().child(kbd(&ctrl)).child(kbd("/")), "Open the keybind list for quick actions.")),
        );
    crate::ui::Modal::new(t("Keyboard mode"), move |s: &mut S| acc(s).nav.keyboard_intro = false).body(body).el(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(key: Key, cmd: bool) -> KeyEvent {
        KeyEvent { key, mods: rune::Mods { cmd, ..Default::default() } }
    }

    #[test]
    fn customs_override_and_disable_builtins() {
        let mut m = AccountPrefs::default();
        let ctrl_k = ev(Key::Command('k'), true);
        assert_eq!(action(&m, &ctrl_k), Some(Action::QuickSwitcher));
        // A custom keybind for the switcher on Ctrl+J suppresses Ctrl+K.
        let j = Combo::from_event(&ev(Key::Command('j'), true));
        m.keybinds.push(Custom { action: Some(Action::QuickSwitcher), combo: j, enabled: true });
        assert_eq!(action(&m, &ctrl_k), None);
        assert_eq!(action(&m, &ev(Key::Command('j'), true)), Some(Action::QuickSwitcher));
        m.keybinds[0].enabled = false;
        assert_eq!(action(&m, &ev(Key::Command('j'), true)), None);
        m.keybinds.clear();
        m.disabled_builtins.push(Action::QuickSwitcher);
        assert_eq!(action(&m, &ctrl_k), None);
        m.disabled_builtins.clear();
        m.builtins_off = true;
        assert_eq!(action(&m, &ctrl_k), None);
    }

    #[test]
    fn conflicts_name_the_other_action() {
        let mut m = AccountPrefs::default();
        let j = Combo::from_event(&ev(Key::Command('j'), true));
        m.keybinds.push(Custom { action: Some(Action::QuickSwitcher), combo: j, enabled: true });
        m.keybinds.push(Custom { action: Some(Action::ToggleMembers), combo: j, enabled: true });
        m.keybinds.push(Custom { action: None, combo: None, enabled: true });
        let c = conflicts(&m);
        assert_eq!(c[0].as_deref(), Some(label(Action::ToggleMembers).as_str()));
        assert!(c[1].is_some() && c[2].is_none());
    }

    #[test]
    fn recording_captures_the_next_combo() {
        let mut st = ShellState::default();
        update_mine(&mut st, |m| m.keybinds.push(Custom { action: Some(Action::ToggleMute), combo: None, enabled: true }));
        st.nav.recording = Some(0);
        assert!(on_key(&mut st, &ev(Key::Text("x".into()), false)), "typing is swallowed while recording");
        assert!(on_key(&mut st, &ev(Key::Command('l'), true)));
        assert_eq!(st.nav.recording, None);
        assert_eq!(mine(&st).keybinds[0].combo.map(|c| c.caps().len()), Some(2));
    }
}
