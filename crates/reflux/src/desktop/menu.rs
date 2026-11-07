//! The macOS application menu (`fluxer_desktop/src/main/Menu.ts`): Rune's standard Edit and
//! Window menus, plus Fluxer's Preferences, Check for updates, View zoom and devtools items and
//! Help links.
//! AppKit keeps its own About, Hide and Quit items in the app menu.

use rune::{App, MenuItem, Tx};

use crate::State;
use crate::keybinds::Action;

fn key(s: &str) -> rune::Shortcut {
    s.parse().expect("menu accelerator")
}

fn zoom(s: &mut State, a: Action) {
    crate::keybinds::run(&s.app.store, &mut s.app.shell, a);
}

/// Every custom item, in menu order.
pub fn items() -> Vec<MenuItem<State>> {
    vec![
        MenuItem::new(crate::i18n::t("Check for updates…"), |s: &mut State, _: &mut Tx| s.desktop.update_checked = None).id("check-updates"),
        MenuItem::separator(),
        MenuItem::new(crate::i18n::t("Preferences…"), |s: &mut State, _: &mut Tx| s.app.shell.open_settings(None)).id("preferences").key_equivalent(key("cmd+,")),
        MenuItem::new("Toggle Developer Tools", |s: &mut State, _: &mut Tx| {
            let sh = &mut s.app.shell;
            if sh.debug.is_some() { sh.debug = None } else { sh.open_debug(crate::shell::debug::Subject::Client) }
        })
        .id("devtools")
        .menu("View")
        .key_equivalent(key("cmd+alt+i")),
        MenuItem::separator().menu("View"),
        MenuItem::new(crate::i18n::t("Actual Size"), |s: &mut State, _: &mut Tx| zoom(s, Action::ZoomReset)).id("zoom-reset").menu("View").key_equivalent(key("cmd+0")).keys_first(),
        MenuItem::new(crate::i18n::t("Zoom In"), |s: &mut State, _: &mut Tx| zoom(s, Action::ZoomIn)).id("zoom-in").menu("View").key_equivalent(key("cmd+=")).keys_first(),
        MenuItem::new(crate::i18n::t("Zoom Out"), |s: &mut State, _: &mut Tx| zoom(s, Action::ZoomOut)).id("zoom-out").menu("View").key_equivalent(key("cmd+-")).keys_first(),
        MenuItem::new(crate::i18n::t("Fluxer Website"), |_: &mut State, tx: &mut Tx| drop(tx.open_url("https://fluxer.com"))).id("website").menu("Help"),
        MenuItem::new("GitHub", |_: &mut State, tx: &mut Tx| drop(tx.open_url("https://github.com/fluxerapp/fluxer"))).id("github").menu("Help"),
        MenuItem::separator().menu("Help"),
        MenuItem::new(crate::i18n::t("Report an Issue"), |_: &mut State, tx: &mut Tx| drop(tx.open_url("https://feedback.fluxer.com"))).id("report-issue").menu("Help"),
    ]
}

/// Adds the menu on macOS (elsewhere Rune has no application menu).
pub fn install(app: App<State>) -> App<State> {
    if !cfg!(target_os = "macos") {
        return app;
    }
    items().into_iter().fold(app.standard_menus(), App::menu_item)
}

#[cfg(test)]
mod tests {
    #[test]
    fn accelerators_parse() {
        let items = super::items();
        assert_eq!(items.len(), 12);
    }
}
