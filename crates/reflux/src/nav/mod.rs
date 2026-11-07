//! Navigation extras: guild rail folders and drag and drop, folder settings, favorites.

pub mod favorites;
mod favorites_sync;
pub mod folder;
mod keybind_sync;
pub mod keys;
pub mod oauth;
pub mod rail;
pub mod skeleton;
pub mod terms;

use std::collections::HashSet;

use reflux_model::Snowflake;
use reflux_store::Store;
use rune::{El, TextEdit};
use serde::{Deserialize, Serialize};

use crate::shell::{ShellAccess, ShellState};
use crate::theme::Palette;

/// A rail drag in progress: what moves, where it would land, and the pointer's list-y.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RailDrag {
    pub src: rail::Slot,
    pub drop: Option<rail::Drop>,
}

/// Navigation UI state (`ShellState::nav`).
#[derive(Default)]
pub struct Nav {
    /// Open guild folders (`GuildFolderExpanded`).
    pub open: HashSet<Option<i64>>,
    pub drag: Option<RailDrag>,
    /// A released drag, applied where the store is at hand ([`poll`]).
    pub pending: Option<(rail::Slot, rail::Drop)>,
    pub hover_folder: Option<Option<i64>>,
    /// The folder the open context menu is for.
    pub menu_folder: Option<Option<i64>>,
    /// The folder whose settings modal is open, its form, and a saved form to write.
    pub folder: Option<Option<i64>>,
    pub folder_form: folder::Form,
    pub folder_edit: Option<serde_json::Value>,
    /// The community waiting on the "Delete my messages" confirmation.
    pub confirm_purge: Option<Snowflake>,
    /// The Favorites rail item is selected (the sidebar lists favorite channels).
    pub favorites: bool,
    pub hover_favorites: bool,
    pub synced_favorites: Option<favorites_sync::Seen>,
    pub synced_keys: Option<keybind_sync::Seen>,
    /// "Sync keybinds" was just switched on: send this device's keybinds.
    pub push_keys: bool,
    /// The image crop modal over the settings.
    pub crop: Option<crate::settings::crop::Crop>,
    /// The profile local time modal and its time zone search.
    pub timezone: bool,
    pub timezone_query: TextEdit,
    /// "Add favorite channels" modal: the community picked in it, its search and filter.
    pub add_favorite: Option<Option<Snowflake>>,
    pub favorite_query: TextEdit,
    pub favorite_hide_muted: bool,
    /// A `/msg` waiting for its DM channel (`crate::compose::poll`).
    // shortcut: kept until the DM shows up; a DM that can't open leaves it pending.
    pub pending_dm: Option<(Snowflake, String)>,
    /// The custom keybind (by index) whose recorder is listening.
    pub recording: Option<usize>,
    /// The Keybinds tab's "Search keybinds…" box.
    pub key_query: TextEdit,
    pub key_menu: Option<keys::RowMenu>,
    pub confirm_reset_keys: bool,
    /// The Ctrl+/ keybind list and the first-Tab keyboard mode intro.
    pub cheatsheet: bool,
    pub keyboard_intro: bool,
    /// The open "Authorize application" modal.
    pub oauth: Option<oauth::OAuth>,
    /// The updated policies modal, and the demo's stand-in for an outdated agreement.
    pub terms: bool,
    pub terms_demo: bool,
    /// The Design system tab's component gallery (`crate::ui::gallery`).
    pub gallery: crate::ui::gallery::Gallery,
    /// The Applications tab (`crate::settings::dev`).
    pub dev: crate::settings::dev::Dev,
}

/// Preferences kept per account on this device (`desktop::Prefs::accounts`, by user ID).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountPrefs {
    pub favorites: Vec<favorites::Favorite>,
    pub hide_muted_favorites: bool,
    pub keybinds: Vec<keys::Custom>,
    /// "Disable built-ins", and the built-ins turned off one by one.
    pub builtins_off: bool,
    pub disabled_builtins: Vec<crate::keybinds::Action>,
    pub keyboard_intro_seen: bool,
    /// "Sync keybinds".
    pub sync_keybinds: bool,
}

/// This account's preferences.
pub fn mine(st: &ShellState) -> AccountPrefs {
    st.prefs.accounts.get(&st.account.me.to_string()).cloned().unwrap_or_default()
}

/// Changes this account's preferences and saves them.
pub fn update_mine(st: &mut ShellState, f: impl FnOnce(&mut AccountPrefs)) {
    let mut prefs = st.prefs.clone();
    f(prefs.accounts.entry(st.account.me.to_string()).or_default());
    st.prefs = prefs.clone();
    st.intents.push(crate::shell::Intent::Prefs(Box::new(prefs)));
}

/// Once a frame with the store: writes a released rail drop as the new `guild_folders`.
pub fn poll(store: &Store, st: &mut ShellState) -> bool {
    let changed = favorites::reconcile(store, st) | favorites_sync::poll(store, st) | keybind_sync::poll(store, st) | crate::compose::poll(store, st) | oauth::poll(store, st);
    let folders = match (st.nav.pending.take(), st.nav.folder_edit.take()) {
        (Some((src, drop)), _) => rail::apply(store, src, drop),
        (None, Some(edit)) => Some(folder::edited(&store.settings.guild_folders, &edit)),
        (None, None) => return changed,
    };
    if let Some(folders) = folders {
        st.intents.push(crate::shell::Intent::PatchSettings(serde_json::json!({ "guild_folders": folders })));
    }
    true
}

/// Every overlay this module draws for the shell.
pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    folder::layer(store, st, p, acc, layer);
    favorites::layer(store, st, p, acc, layer);
    keys::layer(st, p, acc, layer);
    oauth::layer(store, st, p, acc, layer);
    terms::layer(store, st, p, acc, layer);
}

/// `--nav=<scene>` in `--shell-demo`: `folder-open`, `folder-settings`, `drag-combine`,
/// `drag-before`, `drag-inside`, `favorites`, `add-favorite`, `composer`, `composer-menu`, `keybinds`,
/// `keybinds-menu`, `cheatsheet`, `keyboard-intro`.
pub fn demo(sh: &mut ShellState, store: &Store, scene: &str) {
    use rail::{Drop, Slot};
    let first = store.guild_list().into_iter().find_map(|i| match i {
        reflux_store::GuildListItem::Guild(g) => Some(g),
        _ => None,
    });
    let folder = store.guild_list().into_iter().find_map(|i| match i {
        reflux_store::GuildListItem::Folder { folder, .. } => Some(folder.id),
        _ => None,
    });
    let second = store.guild_list().into_iter().filter_map(|i| match i {
        reflux_store::GuildListItem::Guild(g) => Some(g),
        _ => None,
    }).nth(1);
    match scene {
        "folder-open" => {
            sh.nav.open.extend(folder);
        }
        "folder-settings" => sh.nav.folder = folder,
        "drag-combine" | "drag-before" | "drag-inside" => {
            let (Some(a), Some(b), Some(f)) = (first, second, folder) else { return };
            let drop = match scene {
                "drag-combine" => Drop::Combine(a),
                "drag-before" => Drop::Before(Slot::Guild(a)),
                _ => Drop::Inside(f),
            };
            sh.nav.drag = Some(RailDrag { src: Slot::Guild(b), drop: Some(drop) });
        }
        "favorites" | "add-favorite" => {
            let ch: Vec<_> = sh.guild.map(|g| store.channel_groups(g).iter().flat_map(|grp| grp.text.iter().map(|c| (g, c.id))).take(2).collect()).unwrap_or_default();
            for (g, c) in ch {
                favorites::add(sh, g, c);
            }
            if scene == "favorites" {
                favorites::open(sh, store);
                favorites::reconcile(store, sh);
            } else {
                sh.nav.add_favorite = Some(sh.guild);
            }
        }
        // The composer with a mention and a channel pill, "hi" selected under the toolbar.
        "composer" | "composer-menu" => {
            let me = store.my_id();
            let ch = sh.guild.and_then(|g| store.channel_groups(g).first().and_then(|grp| grp.text.first().map(|c| (c.id, c.name.clone().unwrap_or_default()))));
            let mut e = TextEdit::new("hi ");
            e.set_caret(3);
            e.insert_pill(format!("<@{me}>"), "@Square");
            e.insert(" see ");
            if let Some((c, name)) = ch {
                e.insert_pill(format!("<#{c}>"), &name);
            }
            e.set_selection(0, 2);
            e.focused = scene == "composer";
            sh.composer = e;
            if scene == "composer-menu" {
                sh.popups.open_at("spell-menu", [560.0, 560.0]);
            }
        }
        "keybinds" | "keybinds-menu" => {
            use crate::keybinds::{Action, Combo, K};
            let j = Some(Combo { key: K::Char('j'), ctrl: true, alt: false, shift: false });
            update_mine(sh, |m| {
                m.keybinds.push(keys::Custom { action: Some(Action::QuickSwitcher), combo: j, enabled: true });
                m.keybinds.push(keys::Custom { action: Some(Action::ToggleMembers), combo: j, enabled: true });
                m.keybinds.push(keys::Custom { action: None, combo: None, enabled: false });
            });
            sh.nav.recording = Some(2);
            sh.open_settings(Some("keybinds"));
            if scene == "keybinds-menu" {
                sh.nav.recording = None;
                sh.nav.key_menu = Some(keys::RowMenu::Default(vec![Action::ToggleMembers]));
                sh.popups.toggle(&format!("kb-default-{:?}", Action::ToggleMembers));
            }
        }
        "cheatsheet" => sh.nav.cheatsheet = true,
        "keyboard-intro" => sh.nav.keyboard_intro = true,
        "oauth" | "oauth-community" | "oauth-perms" => oauth::demo(sh, scene),
        "terms" => (sh.nav.terms_demo, sh.nav.terms) = (true, true),
        "terms-bar" => sh.nav.terms_demo = true,
        "apps" | "apps-create" | "app-detail" | "embed-debugger" => crate::settings::dev::demo(sh, scene),
        "design-system" => sh.open_settings(Some("component_gallery")),
        _ => {}
    }
}
