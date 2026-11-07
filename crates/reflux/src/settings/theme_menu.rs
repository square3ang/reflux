//! A saved theme's options menu (`YourThemesSection.ThemeRow.openMenu`): Edit details
//! (`ThemeDetailsModal`), Edit/View CSS, Move up/down among the turned-on themes, Link file /
//! Unlink, Export, Duplicate and Delete theme (a `ConfirmModal`), plus the folder row's
//! "Stop watching" confirmation.

use rune::{El, Tx, col, row, text};

use super::AccountModal;
use super::look_tabs::update;
use super::theme_meta::{self, Meta};
use crate::i18n::{t, tf};
use crate::icons;
use crate::look::{LibraryTheme, Prefs};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Menu, MenuItem, Modal, Placement, popup_access};

fn now() -> i64 {
    crate::app::now_ms() as i64
}

fn menu_id(id: &str) -> String {
    format!("theme-options-{id}")
}

/// `isLinkedThemeReadOnly`: a theme that follows a file on disk.
pub fn read_only(th: &LibraryTheme) -> bool {
    th.linked_path.is_some()
}

/// `createThemeFromCss` with `id` and `createdAt` kept (linking a file, a linked file changing).
pub fn from_css(old: &LibraryTheme, css: String, file_name: &str, at: i64) -> LibraryTheme {
    let m = theme_meta::parse(&css, &theme_meta::fallback_name(file_name));
    LibraryTheme {
        id: old.id.clone(),
        name: m.name,
        description: m.description,
        author: m.author,
        version: m.version,
        tags: m.tags,
        css,
        file_name: file_name.to_string(),
        created_at: old.created_at,
        updated_at: at,
        linked_path: old.linked_path.clone(),
        folder_path: old.folder_path.clone(),
    }
}

/// `duplicateTheme`.
pub fn duplicate(l: &mut Prefs, id: &str, at: i64) {
    let Some(th) = l.themes.iter().find(|x| x.id == id) else { return };
    let file = th.file_name.strip_suffix(".css").map_or_else(|| th.file_name.clone(), |b| format!("{b}-copy.css"));
    let copy = LibraryTheme {
        id: format!("theme-{at}-{}", l.themes.len()),
        name: tf("{themeName} Copy", &[("themeName", &th.name)]),
        file_name: file,
        linked_path: None,
        folder_path: None,
        created_at: at,
        updated_at: at,
        ..th.clone()
    };
    l.themes.push(copy);
    l.themes.sort_by_key(|t| t.name.to_lowercase());
}

/// `moveEnabledTheme`: swaps the theme with its neighbour in the turned-on order.
pub fn move_enabled(l: &mut Prefs, id: &str, by: isize) {
    let Some(i) = l.enabled_themes.iter().position(|x| x == id) else { return };
    let j = i as isize + by;
    if j >= 0 && (j as usize) < l.enabled_themes.len() {
        l.enabled_themes.swap(i, j as usize);
    }
}

/// The ⋯ button that opens the row's menu.
pub fn button<S: 'static>(th: &LibraryTheme, p: &Palette, acc: ShellAccess<S>) -> El {
    let id = menu_id(&th.id);
    rune::icon(&icons::DOTS_THREE_BOLD, 18.0)
        .color(p.text_tertiary)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(tf("Options for {themeName}", &[("themeName", &th.name)]))
        .key(id.clone())
        .on_press(move |s: &mut S| acc(s).popups.toggle(&id))
}

/// The open row menu, if any.
pub fn menu<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let l = &st.prefs.look;
    let Some(th) = l.themes.iter().find(|th| st.popups.is_open(&menu_id(&th.id))) else { return };
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let at = l.enabled_themes.iter().position(|x| *x == th.id);
    let n = l.enabled_themes.len();
    let id = th.id.clone();
    let with = |f: fn(&mut S, ShellAccess<S>, String)| {
        let id = id.clone();
        move |s: &mut S| f(s, acc, id.clone())
    };
    let mut m = Menu::new(menu_id(&th.id))
        .placement(Placement::BottomEnd)
        .item(MenuItem::new("theme-details", t("Edit details")).icon(&icons::PENCIL_SIMPLE).on_select(with(|s, acc, id| acc(s).open_account_modal(AccountModal::ThemeDetails(id)))))
        .item(MenuItem::new("theme-css", t(if read_only(th) { "View CSS" } else { "Edit CSS" })).icon(&icons::FILE_CSS).on_select({
            let (id, pal) = (id.clone(), *p);
            move |s: &mut S| {
                if let Some(th) = acc(s).prefs.look.themes.iter().find(|x| x.id == id) {
                    crate::quick_css::open_theme(th, read_only(th), &pal);
                }
            }
        }));
    if at.is_some_and(|i| i > 0) {
        m = m.item(MenuItem::new("theme-up", t("Move up")).icon(&icons::ARROW_UP).on_select(with(|s, acc, id| update(s, acc, move |l| move_enabled(l, &id, -1)))));
    }
    if at.is_some_and(|i| i + 1 < n) {
        m = m.item(MenuItem::new("theme-down", t("Move down")).icon(&icons::ARROW_DOWN).on_select(with(|s, acc, id| update(s, acc, move |l| move_enabled(l, &id, 1)))));
    }
    if th.linked_path.is_none() {
        let id = th.id.clone();
        m = m.item(MenuItem::new("theme-link", t("Link file")).icon(&icons::LINK_SIMPLE).on_select_tx(move |_: &mut S, tx: &mut Tx| link(acc, id.clone(), tx)));
    } else if th.folder_path.is_none() {
        m = m.item(MenuItem::new("theme-unlink", t("Unlink")).icon(&icons::LINK_BREAK).on_select(with(|s, acc, id| update(s, acc, move |l| unlink(l, &id, now())))));
    }
    let (css, file) = (th.css.clone(), if th.file_name.to_lowercase().ends_with(".css") { th.file_name.clone() } else { format!("{}.css", th.file_name) });
    m = m
        .item(MenuItem::new("theme-export", t("Export")).icon(&icons::DOWNLOAD_SIMPLE).on_select_tx(move |_: &mut S, tx: &mut Tx| {
            let css = css.clone();
            let options = rune::FileDialogOptions { title: t("Export").into(), file_name: Some(file.clone()), ..Default::default() }.filter("CSS", &["css"]);
            tx.save_file(options, move |s: &mut S, r, _| {
                if let Ok(Some(path)) = r
                    && let Err(e) = std::fs::write(&path, &css)
                {
                    crate::desktop::toast(&mut acc(s).desktop, e.to_string(), false);
                }
            });
        }))
        .item(MenuItem::new("theme-duplicate", t("Duplicate")).icon(&icons::COPY).on_select(with(|s, acc, id| update(s, acc, move |l| duplicate(l, &id, now())))));
    if th.folder_path.is_none() {
        m = m.separator().item(MenuItem::new("theme-delete", t("Delete theme")).icon(&icons::TRASH).danger().on_select(with(|s, acc, id| acc(s).open_account_modal(AccountModal::ThemeDelete(id)))));
    }
    m.el(&st.popups, pops, p, layer);
}

/// `linkThemeToDesktopFile`: the theme becomes the picked file and follows it.
fn link<S: 'static>(acc: ShellAccess<S>, id: String, tx: &mut Tx) {
    let options = rune::FileDialogOptions { title: t("Link file").into(), ..Default::default() }.filter("CSS", &["css"]);
    tx.open_file(options, move |s: &mut S, r, _| {
        let Ok(Some(path)) = r else { return };
        let css = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => return crate::desktop::toast(&mut acc(s).desktop, e.to_string(), false),
        };
        let (path, file) = (path.to_string_lossy().into_owned(), path.file_name().map_or("theme.css".into(), |f| f.to_string_lossy().into_owned()));
        let id = id.clone();
        update(s, acc, move |l| {
            // Another theme already follows this file: that one stays (`linkedTheme`).
            if l.themes.iter().any(|x| x.id != id && x.linked_path.as_deref() == Some(&path)) {
                return;
            }
            if let Some(th) = l.themes.iter_mut().find(|x| x.id == id) {
                let mut next = from_css(th, css, &file, now());
                next.linked_path = Some(path);
                *th = next;
            }
            l.themes.sort_by_key(|t| t.name.to_lowercase());
        });
    });
}

/// `unlinkTheme`: an editable copy again.
pub fn unlink(l: &mut Prefs, id: &str, at: i64) {
    if let Some(th) = l.themes.iter_mut().find(|x| x.id == id && x.linked_path.is_some()) {
        (th.linked_path, th.folder_path, th.updated_at) = (None, None, at);
    }
}

/// `getLinkedThemeFileLine`.
fn linked_line(th: &LibraryTheme) -> Option<String> {
    let path = th.linked_path.as_deref()?;
    Some(match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => tf("Can't find {path}. Using the last loaded version.", &[("path", path)]),
        Err(_) => tf("Can't read {path}. Using the last loaded version.", &[("path", path)]),
        Ok(_) => tf("Linked to {path}. Edit the file in any editor and saved changes apply here right away.", &[("path", path)]),
    })
}

/// These settings modals, if `m` is one of them.
pub fn modal<S: 'static>(st: &ShellState, m: &AccountModal, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> bool {
    let close = move |s: &mut S| acc(s).close_account_modals();
    let desc = |s: String| text(s).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap();
    let confirm = |title: String, body: String, primary: String, go: Box<dyn Fn(&mut S)>| {
        let footer = row()
            .gap(8.0)
            .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
            .child(Button::new(primary).variant(ButtonVariant::Danger).key("theme-confirm").on_press(move |s: &mut S| go(s)).el(p));
        Modal::new(title, close).body(desc(body)).footer(footer).el(p)
    };
    match m {
        AccountModal::ThemeDelete(id) => {
            let name = st.prefs.look.themes.iter().find(|x| x.id == *id).map_or(String::new(), |x| x.name.clone());
            let id = id.clone();
            let go = Box::new(move |s: &mut S| {
                acc(s).close_account_modals();
                let id = id.clone();
                update(s, acc, move |l| {
                    l.themes.retain(|x| x.id != id);
                    l.enabled_themes.retain(|x| *x != id);
                });
            });
            layer.push(confirm(t("Delete theme?").into(), tf("This removes \"{themeName}\" from this device.", &[("themeName", &name)]), t("Delete theme").into(), go));
        }
        AccountModal::ThemeStopWatching(folder) => {
            let f = folder.clone();
            let go = Box::new(move |s: &mut S| {
                acc(s).close_account_modals();
                let f = f.clone();
                update(s, acc, move |l| super::theme_folder::stop_watching(l, &f));
            });
            layer.push(confirm(t("Stop watching this folder?").into(), t("Themes from this folder will be removed from \"Your themes\".").into(), t("Stop watching").into(), go));
        }
        AccountModal::ThemeDetails(id) => {
            let Some(th) = st.prefs.look.themes.iter().find(|x| x.id == *id) else { return true };
            layer.push(details(st, th, p, acc));
        }
        _ => return false,
    }
    true
}

/// `ThemeDetailsModal`.
fn details<S: 'static>(st: &ShellState, th: &LibraryTheme, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).close_account_modals();
    let title = tf("Edit {themeName}", &[("themeName", &th.name)]);
    let tags = th.tags.join(", ");
    let ro = read_only(th);
    let mut body = col().w_full().gap(16.0);
    if let Some(line) = linked_line(th) {
        let id = th.id.clone();
        body = body.child(
            col()
                .w_full()
                .gap(8.0)
                .child(text(line).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap())
                .children((th.folder_path.is_none()).then(|| {
                    Button::new(t("Unlink"))
                        .variant(ButtonVariant::Secondary)
                        .size(crate::ui::ButtonSize::Small)
                        .on_press(move |s: &mut S| {
                            let id = id.clone();
                            update(s, acc, move |l| unlink(l, &id, now()));
                        })
                        .el(p)
                })),
        );
    }
    let fields: [(&'static str, &str, &str, &str); 5] = [
        ("m.theme_name", "Name", &th.name, ""),
        ("m.theme_author", "Author", &th.author, ""),
        ("m.theme_version", "Version", &th.version, ""),
        ("m.theme_tags", "Tags", &tags, "comma, separated"),
        ("m.theme_description", "Description", &th.description, ""),
    ];
    for (key, label, value, ph) in fields {
        body = body.child(if ro {
            col()
                .w_full()
                .gap(8.0)
                .child(text(t(label).to_string()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary))
                .child(text(value.to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap())
        } else {
            super::input(st, acc, key, value, t(label), t(ph), false, p)
        });
    }
    let footer = if ro {
        row().child(Button::new(t("Close")).variant(ButtonVariant::Secondary).on_press(close).el(p))
    } else {
        let id = th.id.clone();
        let init: Vec<(&'static str, String)> = fields.iter().map(|f| (f.0, f.2.to_string())).collect();
        let save = move |s: &mut S| {
            let sh = acc(s);
            let get = |k: &str| sh.form.text.get(k).map_or_else(|| init.iter().find(|x| x.0 == k).map(|x| x.1.clone()).unwrap_or_default(), |e| e.text.to_string());
            let (name, author, version, tags, description) = (get("m.theme_name"), get("m.theme_author"), get("m.theme_version"), get("m.theme_tags"), get("m.theme_description"));
            sh.form.text.retain(|k, _| !k.starts_with("m.theme_"));
            sh.close_account_modals();
            crate::desktop::toast(&mut sh.desktop, t("Theme saved."), true);
            let id = id.clone();
            update(s, acc, move |l| {
                let Some(th) = l.themes.iter_mut().find(|x| x.id == id) else { return };
                let meta = Meta { name: if name.trim().is_empty() { th.name.clone() } else { name.trim().to_string() }, description, author, version, tags: theme_meta::tags(&tags) };
                th.css = theme_meta::upsert(&th.css, &meta);
                (th.name, th.description, th.author, th.version, th.tags) = (meta.name, meta.description, meta.author, meta.version, meta.tags);
                th.updated_at = now();
                l.themes.sort_by_key(|t| t.name.to_lowercase());
            });
        };
        row()
            .gap(8.0)
            .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
            .child(Button::new(t("Save")).key("theme-details-save").on_press(save).el(p))
    };
    Modal::new(title, close).body(body).footer(footer).el(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_duplicate_unlink() {
        let mut l = Prefs::default();
        let mut a = super::super::theme_library::theme("/* @name A */".into(), "a.css", 1, 0);
        a.linked_path = Some("/x/a.css".into());
        l.themes.push(a);
        l.themes.push(super::super::theme_library::theme(":root{}".into(), "b.css", 1, 1));
        let (ida, idb) = (l.themes[0].id.clone(), l.themes[1].id.clone());
        l.enabled_themes = vec![ida.clone(), idb.clone()];
        move_enabled(&mut l, &idb, -1);
        assert_eq!(l.enabled_themes, [idb.clone(), ida.clone()]);
        move_enabled(&mut l, &idb, -1);
        assert_eq!(l.enabled_themes[0], idb, "the first stays first");
        duplicate(&mut l, &ida, 5);
        let copy = l.themes.iter().find(|x| x.name == "A Copy").expect("copy");
        assert_eq!((copy.file_name.as_str(), copy.linked_path.is_none()), ("a-copy.css", true));
        unlink(&mut l, &ida, 6);
        assert!(l.themes.iter().all(|x| x.linked_path.is_none()));
    }
}
