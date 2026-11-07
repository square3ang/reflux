//! Watched theme folders (`ThemeLibrary.importDesktopThemeDirectory` / `applyFolderFiles`,
//! `YourThemesSection.FolderRow`, desktop `ThemeFolderWatch.ts`): every `.css` file under a
//! folder becomes a theme that follows the file, polled every 2 s like the desktop watcher's
//! fallback.
// shortcut: "Stop watching" confirms inline (like "Reset data") instead of a ConfirmModal.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use rune::{El, Tx, col, row, text};
use serde_json::json;

use super::look_tabs::update;
use crate::i18n::{t, tf, tp};
use crate::look::{LibraryTheme, Prefs};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant};

const MAX_FILES: usize = 200;
const MAX_FOLDERS: usize = 20;
const MAX_BYTES: u64 = 50 * 1024 * 1024;
const POLL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Status {
    Live,
    Missing,
    NotAllowed,
}

/// A scan: the status, whether files past the cap were left out, and `(path, mtime, size)` for
/// each CSS file (to skip reading files that didn't change).
#[derive(Clone, PartialEq)]
struct Seen {
    status: Status,
    truncated: bool,
    sig: Vec<(PathBuf, Option<SystemTime>, u64)>,
}

static SEEN: Mutex<Option<HashMap<String, Seen>>> = Mutex::new(None);
static LAST: Mutex<Option<Instant>> = Mutex::new(None);

/// `listThemeFolderCssFiles`: sorted, recursive, skipping dot folders and `node_modules`.
fn list(folder: &Path) -> std::io::Result<(Vec<PathBuf>, bool)> {
    fn visit(dir: &Path, root: bool, files: &mut Vec<PathBuf>) -> std::io::Result<bool> {
        let mut entries: Vec<_> = match std::fs::read_dir(dir) {
            Ok(r) => r.flatten().collect(),
            Err(e) if root => return Err(e),
            Err(_) => return Ok(false),
        };
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !name.starts_with('.') && name != "node_modules" && visit(&e.path(), false, files)? {
                    return Ok(true);
                }
            } else if ft.is_file() && name.to_lowercase().ends_with(".css") {
                if files.len() == MAX_FILES {
                    return Ok(true);
                }
                files.push(e.path());
            }
        }
        Ok(false)
    }
    let mut files = Vec::new();
    let truncated = visit(folder, true, &mut files)?;
    Ok((files, truncated))
}

fn scan(folder: &str) -> Seen {
    match list(Path::new(folder)) {
        Ok((files, truncated)) => {
            let sig = files
                .into_iter()
                .map(|f| {
                    let m = std::fs::metadata(&f).ok();
                    (f, m.as_ref().and_then(|m| m.modified().ok()), m.map_or(0, |m| m.len()))
                })
                .collect();
            Seen { status: Status::Live, truncated, sig }
        }
        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory) => Seen { status: Status::Missing, truncated: false, sig: vec![] },
        Err(_) => Seen { status: Status::NotAllowed, truncated: false, sig: vec![] },
    }
}

/// `(path, fileName relative to the folder, css)` for the files that read as text.
fn read(folder: &str, seen: &Seen) -> Vec<(String, String, String)> {
    seen.sig
        .iter()
        .filter(|(_, _, len)| *len <= MAX_BYTES)
        .filter_map(|(f, _, _)| {
            let css = std::fs::read_to_string(f).ok()?;
            let name = f.strip_prefix(folder).unwrap_or(f).to_string_lossy().into_owned();
            Some((f.to_string_lossy().into_owned(), name, css))
        })
        .collect()
}

/// `applyFolderFiles` for a full snapshot: updates, adds (a removed file with the same CSS counts
/// as renamed) and drops the folder's themes. Returns the themes added or changed.
fn apply(l: &mut Prefs, folder: &str, files: &[(String, String, String)], at: i64) -> usize {
    let outside: Vec<String> = l.themes.iter().filter(|t| t.folder_path.as_deref() != Some(folder)).filter_map(|t| t.linked_path.clone()).collect();
    let mut matched: Vec<String> = Vec::new();
    let mut fresh = Vec::new();
    let mut n = 0;
    for (path, name, css) in files {
        match l.themes.iter_mut().find(|t| t.folder_path.as_deref() == Some(folder) && t.linked_path.as_deref() == Some(path)) {
            Some(th) => {
                matched.push(th.id.clone());
                if th.css != *css {
                    th.css = css.clone();
                    th.updated_at = at;
                    n += 1;
                }
            }
            None if !outside.contains(path) => fresh.push((path, name, css)),
            None => {}
        }
    }
    let mut leftovers: Vec<String> = l.themes.iter().filter(|t| t.folder_path.as_deref() == Some(folder) && !matched.contains(&t.id)).map(|t| t.id.clone()).collect();
    for (path, name, css) in fresh {
        let renamed = leftovers.iter().position(|id| l.themes.iter().any(|t| t.id == *id && t.css == *css)).map(|i| leftovers.remove(i));
        let th = match renamed.and_then(|id| l.themes.iter_mut().find(|t| t.id == id)) {
            Some(th) => th,
            None => {
                let k = l.themes.len();
                l.themes.push(super::theme_library::theme(css.clone(), name, at, k));
                l.themes.last_mut().expect("pushed")
            }
        };
        th.name = name.strip_suffix(".css").unwrap_or(name).to_string();
        th.file_name = name.clone();
        th.css = css.clone();
        th.linked_path = Some(path.clone());
        th.folder_path = Some(folder.to_string());
        th.updated_at = at;
        n += 1;
    }
    l.themes.retain(|t| !leftovers.contains(&t.id));
    l.enabled_themes.retain(|id| !leftovers.contains(id));
    l.themes.sort_by_key(|t| t.name.to_lowercase());
    n + leftovers.len()
}

fn now() -> i64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The watcher: rescans the folders every 2 s and saves the library when a CSS file changed.
pub fn poll(st: &mut ShellState, tx: &mut Tx) -> bool {
    if st.prefs.look.theme_folders.is_empty() {
        return false;
    }
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = *last
        && at.elapsed() < POLL
    {
        tx.wake_after(POLL - at.elapsed());
        return false;
    }
    *last = Some(Instant::now());
    tx.wake_after(POLL);
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let seen = seen.get_or_insert_with(HashMap::new);
    let mut prefs = st.prefs.clone();
    let (mut redraw, mut save) = (false, false);
    for folder in &st.prefs.look.theme_folders {
        let now_seen = scan(folder);
        if seen.get(folder) == Some(&now_seen) {
            continue;
        }
        redraw = true;
        // A folder that went missing keeps its themes until it's back (`applyFolderChange`).
        if now_seen.status == Status::Live {
            save |= apply(&mut prefs.look, folder, &read(folder, &now_seen), now()) > 0;
        }
        seen.insert(folder.clone(), now_seen);
    }
    if save {
        st.intents.push(crate::shell::Intent::Prefs(Box::new(prefs)));
    }
    redraw
}

/// "Import folder": watch a folder and load its CSS files.
pub fn import_button<S: 'static>(p: &Palette, acc: ShellAccess<S>) -> El {
    Button::new(t("Import folder"))
        .variant(ButtonVariant::Secondary)
        .size(ButtonSize::Small)
        .key("theme-import-folder")
        .on_press_tx(move |_: &mut S, tx: &mut Tx| {
            let options = rune::FileDialogOptions { title: t("Import folder").into(), ..Default::default() };
            tx.open_folder(options, move |s: &mut S, r, _| {
                let Ok(Some(path)) = r else { return };
                let folder = std::fs::canonicalize(&path).unwrap_or(path).to_string_lossy().into_owned();
                let sh = acc(s);
                let folders = &sh.prefs.look.theme_folders;
                if !folders.contains(&folder) && folders.len() >= MAX_FOLDERS {
                    let msg = tf("You can watch up to {limit} folders. Stop watching one to add another.", &[("limit", &MAX_FOLDERS.to_string())]);
                    return crate::desktop::toast(&mut sh.desktop, msg, false);
                }
                let seen = scan(&folder);
                if seen.status != Status::Live {
                    return crate::desktop::toast(&mut sh.desktop, t("Fluxer can't read this folder.").to_string(), false);
                }
                let files = read(&folder, &seen);
                update(s, acc, |l| {
                    if !l.theme_folders.contains(&folder) {
                        l.theme_folders.push(folder.clone());
                    }
                    apply(l, &folder, &files, now());
                });
                SEEN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(folder, seen);
                if !files.is_empty() {
                    let msg = tp("Linked {length, plural, one {# theme file} other {# theme files}}. Changes you save to them apply right away.", files.len() as i64, &[("length", &files.len().to_string())]);
                    crate::desktop::toast(&mut acc(s).desktop, msg, true);
                }
            });
        })
        .el(p)
}

/// `folderName`: the last path segment.
pub fn folder_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(path)
}

/// `FolderRow`s for the watched folders.
pub fn rows<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Vec<El> {
    let seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    st.prefs
        .look
        .theme_folders
        .iter()
        .map(|folder| {
            let s = seen.as_ref().and_then(|m| m.get(folder));
            let (status, truncated) = s.map_or((Status::Live, false), |s| (s.status, s.truncated));
            let mut info = col().grow(1.0).min_w(0.0).gap(2.0).child(text(folder_name(folder).to_string()).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary).truncate());
            info = info.child(match status {
                Status::Live => text(tf("Watching {path}", &[("path", folder)])).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary).wrap(),
                bad => row()
                    .gap(4.0)
                    .center_y()
                    .child(rune::icon(&crate::icons::WARNING_CIRCLE_BOLD, 14.0).color(p.status_warning))
                    .child(
                        text(t(if bad == Status::Missing { "Fluxer can't find this folder. It will load the themes again when the folder is back." } else { "Fluxer can't open this folder. Import it again to keep watching it." }))
                            .font(font(12.0, 400.0).lh(16.0))
                            .color(p.status_warning)
                            .wrap()
                            .grow(1.0)
                            .shrink(),
                    ),
            });
            if truncated {
                info = info.child(text(t("Only the first 200 CSS files in this folder are loaded.")).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary).wrap());
            }
            let key = format!("theme-folder-stop:{folder}");
            let control = if st.form.data.contains_key(&key) {
                let (k1, k2, f) = (key.clone(), key.clone(), folder.clone());
                col()
                    .gap(6.0)
                    .items_end()
                    .child(text(t("Stop watching this folder?")).font(font(13.0, 600.0).lh(18.0)).color(p.text_primary))
                    .child(text(t("Themes from this folder will be removed from \"Your themes\".")).font(font(12.0, 400.0).lh(16.0)).color(p.text_secondary))
                    .child(
                        row()
                            .gap(8.0)
                            .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).on_press(move |s: &mut S| drop(acc(s).form.data.remove(&k1))).el(p))
                            .child(
                                Button::new(t("Stop watching"))
                                    .variant(ButtonVariant::Danger)
                                    .size(ButtonSize::Small)
                                    .on_press(move |s: &mut S| {
                                        acc(s).form.data.remove(&k2);
                                        let f = f.clone();
                                        update(s, acc, move |l| stop_watching(l, &f));
                                    })
                                    .el(p),
                            ),
                    )
            } else {
                Button::new(t("Stop watching")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).on_press(move |s: &mut S| drop(acc(s).form.data.insert(key.clone(), json!(true)))).el(p)
            };
            row()
                .w_full()
                .px(12.0)
                .py(10.0)
                .gap(12.0)
                .center_y()
                .radius(8.0)
                .fill(p.form_surface_background)
                .border(1.0, p.background_modifier_accent)
                .child(rune::icon(&crate::icons::FOLDER_OPEN, 18.0).color(p.text_secondary))
                .child(info)
                .child(control)
        })
        .collect()
}

/// `stopWatchingFolder`: forgets the folder and drops its themes (the files stay on disk).
fn stop_watching(l: &mut Prefs, folder: &str) {
    l.theme_folders.retain(|f| f != folder);
    let gone: Vec<String> = l.themes.iter().filter(|t| t.folder_path.as_deref() == Some(folder)).map(|t| t.id.clone()).collect();
    l.themes.retain(|t| !gone.contains(&t.id));
    l.enabled_themes.retain(|id| !gone.contains(id));
}

/// A theme row's subtitle and whether it can be deleted (folder themes follow their files).
pub fn subtitle(th: &LibraryTheme) -> (String, bool) {
    match &th.folder_path {
        Some(f) => (tf("From {folder}", &[("folder", folder_name(f))]), false),
        None => (th.file_name.clone(), true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_updates_renames_and_drops() {
        let mut l = Prefs::default();
        let f = |p: &str, css: &str| (format!("/t/{p}"), p.to_string(), css.to_string());
        assert_eq!(apply(&mut l, "/t", &[f("a.css", "a"), f("b.css", "b")], 1), 2);
        let b = l.themes.iter().find(|t| t.name == "b").unwrap().id.clone();
        l.enabled_themes.push(b.clone());
        // a changed, b renamed to c (same CSS keeps the ID), nothing else.
        assert_eq!(apply(&mut l, "/t", &[f("a.css", "a2"), f("c.css", "b")], 2), 2);
        assert_eq!(l.themes.iter().map(|t| (t.name.as_str(), t.css.as_str())).collect::<Vec<_>>(), [("a", "a2"), ("c", "b")]);
        assert_eq!(l.themes[1].id, b);
        assert_eq!(apply(&mut l, "/t", &[f("a.css", "a2")], 3), 1);
        assert!(l.enabled_themes.is_empty());
        assert_eq!(folder_name("/home/x/themes/"), "themes");
        let d = std::env::temp_dir().join(format!("reflux-theme-folder-{}", std::process::id()));
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::write(d.join("x.css"), "x").unwrap();
        std::fs::write(d.join(".git/y.css"), "y").unwrap();
        std::fs::write(d.join("z.txt"), "z").unwrap();
        assert_eq!(list(&d).unwrap(), (vec![d.join("x.css")], false));
        std::fs::remove_dir_all(&d).unwrap();
        assert_eq!(scan(d.to_str().unwrap()).status, Status::Missing);
    }
}
