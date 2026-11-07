//! Guild folder context menu (`GuildFolderContextMenu`), folder settings
//! (`GuildFolderSettingsModal`) and "Delete my messages" in a community
//! (`useDeleteMyMessagesInGuild`).

use reflux_model::{GuildFolder, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, TextEdit, col, el, icon, row, text};
use serde_json::json;

use crate::guild::Then;
use crate::i18n::t;
use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Input, Menu, MenuItem, Modal, Select, Switch, popup_access};

const MENU: &str = "folder-menu";
/// `GuildFolderIcons` and their labels.
const ICONS: [(&str, &str); 7] = [
    ("folder", "Folder"),
    ("star", "Star"),
    ("heart", "Heart"),
    ("bookmark", "Bookmark"),
    ("game_controller", "Game controller"),
    ("shield", "Shield"),
    ("music_note", "Music note"),
];
/// The colour swatches the role editor offers too.
const SWATCHES: [u32; 10] = [0x1abc9c, 0x2ecc71, 0x3498db, 0x9b59b6, 0xe91e63, 0xf1c40f, 0xe67e22, 0xe74c3c, 0x95a5a6, 0x607d8b];

/// The folder settings form.
#[derive(Default)]
pub struct Form {
    pub name: TextEdit,
    pub color: u32,
    pub flags: u32,
    pub icon: String,
}

/// The folder's name, else its first three guilds' names (`deriveGuildFolderName`), else "Folder".
pub fn display_name(store: &Store, f: &GuildFolder) -> String {
    if let Some(n) = f.name.as_deref().filter(|n| !n.is_empty()) {
        return n.to_string();
    }
    let names: Vec<&str> = f.guild_ids.iter().filter_map(|g| store.guilds.get(g)?.info.as_ref().map(|i| i.name.as_str())).take(3).collect();
    if names.is_empty() { "Folder".into() } else { names.join(", ") }
}

pub fn open_menu(st: &mut ShellState, folder: Option<i64>, at: [f32; 2]) {
    st.popups.open_at(MENU, at);
    st.nav.menu_folder = Some(folder);
}

fn open_settings(st: &mut ShellState, store_folder: Option<&GuildFolder>, id: Option<i64>) {
    let f = store_folder.cloned().unwrap_or_default();
    st.nav.folder_form = Form { name: TextEdit::new(f.name.unwrap_or_default()), color: f.color.unwrap_or(0), flags: f.flags, icon: if f.icon.is_empty() { "folder".into() } else { f.icon } };
    st.nav.folder_form.name.focused = true;
    st.nav.folder = Some(id);
}

pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if st.popups.is_open(MENU)
        && let Some(fid) = st.nav.menu_folder
    {
        menu(store, fid, acc).el(&st.popups, popup_access(move |s: &mut S| &mut acc(s).popups), p, layer);
    }
    if let Some(fid) = st.nav.folder {
        let start = layer.len();
        let modal = settings(store, st, fid, p, acc, layer);
        // Under the icon list it opened.
        layer.insert(start, modal);
    }
    if let Some(g) = st.nav.confirm_purge {
        layer.push(purge(store, g, p, acc));
    }
}

fn menu<S: 'static>(store: &Store, fid: Option<i64>, acc: ShellAccess<S>) -> Menu<S> {
    let guilds: Vec<Snowflake> = store.settings.guild_folders.iter().find(|f| f.id == fid).map(|f| f.guild_ids.clone()).unwrap_or_default();
    let acks: Vec<(Snowflake, Snowflake)> = store
        .channels
        .values()
        .filter(|c| c.guild_id.is_some_and(|g| guilds.contains(&g)) && store.has_unread(c.id))
        .filter_map(|c| Some((c.id, c.last_message_id?)))
        .collect();
    let none = acks.is_empty();
    let folder = store.settings.guild_folders.iter().find(|f| f.id == fid).cloned();
    Menu::new(MENU)
        .item(MenuItem::new("mark-read", "Mark folder as read").icon(&icons::EYE_FILL).disabled(none).on_select(move |s: &mut S| acc(s).intents.push(Intent::MarkRead(acks.clone()))))
        .separator()
        .item(MenuItem::new("settings", "Folder settings").icon(&icons::GEAR_FILL).on_select(move |s: &mut S| open_settings(acc(s), folder.as_ref(), fid)))
}

fn settings<S: 'static>(store: &Store, st: &ShellState, fid: Option<i64>, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let close = move |s: &mut S| acc(s).nav.folder = None;
    let Some(folder) = store.settings.guild_folders.iter().find(|f| f.id == fid) else { return Modal::new("Folder settings", close).el(p) };
    let form = &st.nav.folder_form;
    let auto = {
        let f = GuildFolder { name: None, ..folder.clone() };
        display_name(store, &f)
    };
    let name = Input::new("folder-name", &form.name, move |s: &mut S| &mut acc(s).nav.folder_form.name).label("Folder name").placeholder(&auto).el(p);
    let preview = el().size(40.0, 40.0).radius(8.0).fill(if form.color == 0 { p.brand_primary_light } else { crate::shell::role_color(form.color) });
    let swatches = row().gap(8.0).children(SWATCHES.iter().map(|&c| {
        let on = form.color == c;
        el().size(28.0, 28.0)
            .radius(6.0)
            .fill(crate::shell::role_color(c))
            .border(if on { 2.0 } else { 0.0 }, p.text_primary)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(crate::guild::hex(c))
            .on_press(move |s: &mut S| acc(s).nav.folder_form.color = c)
    }));
    let color = col()
        .gap(8.0)
        .child(text("Folder color").font(font(14.0, 500.0).lh(20.0)).color(p.text_primary))
        .child(row().gap(12.0).center_y().child(preview).child(swatches));
    let show = Switch::new(form.flags & 1 != 0).label("Show icon when collapsed").on_toggle(move |s: &mut S| acc(s).nav.folder_form.flags ^= 1).el(p);
    let icon_pick = Select::new("folder-icon", &ICONS, Some(form.icon.as_str()))
        .label("Folder icon")
        .width(392.0)
        .on_choose(move |s: &mut S, v: &str| acc(s).nav.folder_form.icon = v.to_string())
        .el(&st.popups, popup_access(move |s: &mut S| &mut acc(s).popups), p, layer);
    let glyph = row().gap(8.0).center_y().child(icon(crate::shell::folder_glyph(&form.icon), 18.0).color(p.text_primary_muted)).child(icon_pick);
    let save = move |s: &mut S| {
        let sh = acc(s);
        let f = &sh.nav.folder_form;
        let patch = json!({ "id": fid, "name": Some(f.name.text.trim()).filter(|n| !n.is_empty()), "color": Some(f.color).filter(|&c| c != 0), "flags": f.flags, "icon": f.icon });
        sh.nav.folder = None;
        sh.nav.folder_edit = Some(patch);
    };
    let delete = move |s: &mut S| {
        let sh = acc(s);
        sh.nav.folder = None;
        sh.nav.folder_edit = Some(json!({ "id": fid, "delete": true }));
    };
    Modal::new("Folder settings", close)
        .body(col().w_full().gap(16.0).child(name).child(color).child(show).child(glyph))
        .footer(
            row()
                .w_full()
                .gap(8.0)
                .child(Button::new("Delete folder").variant(ButtonVariant::Danger).on_press(delete).el(p))
                .child(el().grow(1.0))
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
                .child(Button::new("Save").on_press(save).el(p)),
        )
        .el(p)
}

/// Applies a saved folder form (`folder_edit`) to the store's folders.
pub fn edited(folders: &[GuildFolder], edit: &serde_json::Value) -> Vec<GuildFolder> {
    let id = edit["id"].as_i64();
    if edit["delete"].as_bool() == Some(true) {
        return super::rail::ungroup(folders, id);
    }
    folders
        .iter()
        .map(|f| {
            if f.id != id {
                return f.clone();
            }
            GuildFolder {
                name: edit["name"].as_str().map(str::to_string),
                color: edit["color"].as_u64().map(|c| c as u32),
                flags: edit["flags"].as_u64().unwrap_or(0) as u32,
                icon: edit["icon"].as_str().unwrap_or("folder").to_string(),
                ..f.clone()
            }
        })
        .collect()
}

/// "Delete my messages" in a community: `ConfirmModal`, then
/// `POST /users/@me/guilds/{id}/messages/bulk-delete-mine`.
fn purge<S: 'static>(store: &Store, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let _ = store;
    let close = move |s: &mut S| acc(s).nav.confirm_purge = None;
    let go = move |s: &mut S| {
        let sh = acc(s);
        sh.nav.confirm_purge = None;
        sh.api(Method::POST, format!("/users/@me/guilds/{g}/messages/bulk-delete-mine"), Some(json!({})), Then::Toast("Deleted your messages.", "Couldn't delete your messages"));
    };
    Modal::new(t("Delete your messages in this community?"), close)
        .body(text(t("Permanently delete every message you've sent here, across every channel. Can't be undone.")).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap())
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
                .child(Button::new(t("Delete my messages")).variant(ButtonVariant::Danger).on_press(go).el(p)),
        )
        .el(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_and_deleting_a_folder() {
        let folders = vec![GuildFolder { id: Some(7), name: Some("Old".into()), guild_ids: vec![Snowflake(5)], icon: "folder".into(), ..Default::default() }];
        let out = edited(&folders, &json!({"id": 7, "name": "Games", "color": 0x2ecc71, "flags": 1, "icon": "star"}));
        assert_eq!((out[0].name.as_deref(), out[0].color, out[0].flags, out[0].icon.as_str()), (Some("Games"), Some(0x2ecc71), 1, "star"));
        let out = edited(&folders, &json!({"id": 7, "name": null, "color": null, "flags": 0, "icon": "folder"}));
        assert_eq!((out[0].name.clone(), out[0].color), (None, None));
        let out = edited(&folders, &json!({"id": 7, "delete": true}));
        assert_eq!((out[0].id, out[0].guild_ids.clone()), (Some(-1), vec![Snowflake(5)]));
    }
}
