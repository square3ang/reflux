//! The theme library (`theme/state/ThemeLibrary.ts`, `YourThemesSection.tsx`,
//! `ShareAndBackUpSection.tsx`): CSS files saved on this device that can be turned on over the
//! base theme, and the library backup file (`fluxer-theme-library.json`, export version 1).
//! Each row's options are in [`super::theme_menu`]; names and details come from the CSS header
//! ([`super::theme_meta`]).

use rune::{El, Tx, col, row, text};
use serde_json::{Value, json};

use super::look_tabs::update;
use crate::i18n::{t, tf};
use crate::look::{LibraryTheme, Prefs};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Switch};

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// `createThemeFromCss`: a new theme, its details from the CSS header, else named after its file.
pub(super) fn theme(css: String, file_name: &str, at: i64, n: usize) -> LibraryTheme {
    let blank = LibraryTheme { id: format!("theme-{at}-{n}"), created_at: at, ..Default::default() };
    super::theme_menu::from_css(&blank, css, file_name, at)
}

/// `buildExportPayload`.
pub fn export(l: &Prefs) -> Value {
    let themes: Vec<Value> = l.themes.iter().map(|t| serde_json::to_value(t).unwrap_or_default()).collect();
    json!({ "version": 1, "exportedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true), "themes": themes, "assets": l.theme_assets })
}

/// `importExportPayload`: adds the themes (new IDs) and assets; `None` for a file that isn't a
/// version 1 library.
pub fn import(l: &mut Prefs, v: &Value) -> Option<(usize, usize)> {
    if v["version"] != 1 {
        return None;
    }
    let at = now();
    let mut n = 0;
    for item in v["themes"].as_array()? {
        let Some(css) = item["css"].as_str() else { continue };
        let mut th = theme(css.to_string(), item["fileName"].as_str().unwrap_or("imported-theme.css"), at, l.themes.len());
        let s = |k: &str| item[k].as_str().map(str::to_string);
        if let Some(name) = s("name").filter(|n| !n.trim().is_empty()) {
            th.name = name.trim().to_string();
        }
        th.description = s("description").unwrap_or_default();
        th.author = s("author").unwrap_or_default();
        th.version = s("version").unwrap_or_default();
        th.tags = item["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default();
        th.created_at = item["createdAt"].as_i64().unwrap_or(at);
        th.updated_at = item["updatedAt"].as_i64().unwrap_or(at);
        l.themes.push(th);
        n += 1;
    }
    let assets: Vec<Value> = v["assets"].as_array().map(|a| a.iter().filter(|a| a["name"].is_string() && a["mimeType"].is_string()).cloned().collect()).unwrap_or_default();
    let m = assets.len();
    l.theme_assets.extend(assets);
    l.themes.sort_by_key(|t| t.name.to_lowercase());
    Some((n, m))
}

/// "Your themes": the saved themes, turned-on ones first in their order, each with a switch.
pub fn your_themes<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let l = &st.prefs.look;
    super::theme_menu::menu(st, p, acc, layer);
    let import = Button::new(t("Import CSS"))
        .variant(ButtonVariant::Secondary)
        .size(ButtonSize::Small)
        .key("theme-import-css")
        .on_press_tx(move |_: &mut S, tx: &mut Tx| {
            let options = rune::FileDialogOptions { title: t("Import CSS").into(), ..Default::default() }.filter("CSS", &["css", "txt"]);
            tx.open_file(options, move |s: &mut S, r, _| {
                let Ok(Some(path)) = r else { return };
                match std::fs::read_to_string(&path) {
                    Ok(css) => {
                        let file = path.file_name().map_or("theme.css".into(), |f| f.to_string_lossy().into_owned());
                        update(s, acc, |l| {
                            let th = theme(css, &file, now(), l.themes.len());
                            l.themes.push(th);
                            l.themes.sort_by_key(|t| t.name.to_lowercase());
                        });
                        crate::desktop::toast(&mut acc(s).desktop, crate::i18n::tp("Imported {length, plural, one {# theme file} other {# theme files}}.", 1, &[("length", "1")]), true);
                    }
                    Err(e) => crate::desktop::toast(&mut acc(s).desktop, e.to_string(), false),
                }
            });
        })
        .el(p);
    let mut c = col()
        .w_full()
        .gap(12.0)
        .child(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(text(t("Themes you import from CSS files. The ones you turn on are added on top of the base theme in the order shown.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap().grow(1.0).shrink())
                .child(row().gap(8.0).child(import).child(super::theme_folder::import_button(p, acc))),
        )
        .children(super::theme_folder::rows(st, p, acc));
    if l.themes.is_empty() {
        return c.child(
            col()
                .w_full()
                .gap(4.0)
                .py(16.0)
                .items_center()
                .child(text(t("No themes saved yet.")).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary))
                .child(text(t("Import a .css file to get started, or write your own CSS in quick CSS.")).font(font(13.0, 400.0).lh(18.0)).color(p.text_secondary).wrap()),
        );
    }
    let order = |th: &LibraryTheme| l.enabled_themes.iter().position(|id| *id == th.id).unwrap_or(usize::MAX);
    let mut themes: Vec<&LibraryTheme> = l.themes.iter().collect();
    themes.sort_by_key(|th| order(th));
    for th in themes {
        let on = l.enabled_themes.contains(&th.id);
        let id = th.id.clone();
        let label = tf("Enable {themeName}", &[("themeName", &th.name)]);
        let toggle = Switch::new(on).on_toggle(move |s: &mut S| {
            let id = id.clone();
            update(s, acc, move |l| {
                if on {
                    l.enabled_themes.retain(|x| *x != id);
                } else {
                    l.enabled_themes.push(id);
                }
            })
        });
        let (sub, _) = super::theme_folder::subtitle(th);
        c = c.child(
            row()
                .w_full()
                .px(12.0)
                .py(10.0)
                .gap(12.0)
                .center_y()
                .radius(8.0)
                .fill(p.form_surface_background)
                .border(1.0, p.background_modifier_accent)
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(text(th.name.clone()).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary).truncate())
                        .child(text(sub).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary).truncate()),
                )
                .child(toggle.el(p).label(label))
                .child(super::theme_menu::button(th, p, acc)),
        );
    }
    c
}

/// `ActionRow`: a title and description, then the controls.
fn action_row(title: &str, desc: &str, button: El, p: &Palette) -> El {
    row()
        .w_full()
        .gap(16.0)
        .py(12.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(text(t(title).to_string()).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary))
                .child(text(t(desc).to_string()).font(font(13.0, 400.0).lh(18.0)).color(p.text_secondary).wrap()),
        )
        .child(button)
}

/// Export library, Import library and Reset theme data (`ShareAndBackUpSection`).
pub fn backup_rows<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Vec<El> {
    let payload = serde_json::to_string_pretty(&export(&st.prefs.look)).unwrap_or_default();
    let export = Button::new(t("Export library"))
        .variant(ButtonVariant::Secondary)
        .size(ButtonSize::Small)
        .key("theme-export-library")
        .on_press_tx(move |_: &mut S, tx: &mut Tx| {
            let payload = payload.clone();
            let options = rune::FileDialogOptions { title: t("Export library").into(), file_name: Some("fluxer-theme-library.json".into()), ..Default::default() }.filter("JSON", &["json"]);
            tx.save_file(options, move |s: &mut S, r, _| {
                if let Ok(Some(path)) = r
                    && let Err(e) = std::fs::write(&path, &payload)
                {
                    crate::desktop::toast(&mut acc(s).desktop, e.to_string(), false);
                }
            });
        })
        .el(p);
    let import = Button::new(t("Import library"))
        .variant(ButtonVariant::Secondary)
        .size(ButtonSize::Small)
        .key("theme-import-library")
        .on_press_tx(move |_: &mut S, tx: &mut Tx| {
            let options = rune::FileDialogOptions { title: t("Import library").into(), ..Default::default() }.filter("JSON", &["json", "txt"]);
            tx.open_file(options, move |s: &mut S, r, _| {
                let Ok(Some(path)) = r else { return };
                let v: Option<Value> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok());
                let mut prefs = acc(s).prefs.clone();
                let msg = match v.as_ref().and_then(|v| import(&mut prefs.look, v)) {
                    Some((n, m)) => {
                        acc(s).intents.push(crate::shell::Intent::Prefs(Box::new(prefs)));
                        Ok(tf(
                            "Imported {themes, plural, one {# theme} other {# themes}} and {assets, plural, one {# asset} other {# assets}}.",
                            &[("themes", &n.to_string()), ("assets", &m.to_string())],
                        ))
                    }
                    None => Err(t("We couldn't import that theme library file.").to_string()),
                };
                let ok = msg.is_ok();
                crate::desktop::toast(&mut acc(s).desktop, msg.unwrap_or_else(|e| e), ok);
            });
        })
        .el(p);
    let confirming = st.form.data.contains_key("theme-reset");
    let reset = if confirming {
        row()
            .gap(8.0)
            .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).on_press(move |s: &mut S| drop(acc(s).form.data.remove("theme-reset"))).el(p))
            .child(
                Button::new(t("Reset data"))
                    .variant(ButtonVariant::Danger)
                    .size(ButtonSize::Small)
                    .key("theme-reset-confirm")
                    .on_press(move |s: &mut S| {
                        acc(s).form.data.remove("theme-reset");
                        acc(s).form.text.retain(|k, _| !k.starts_with("tok-") && k != "quick-css");
                        update(s, acc, |l| {
                            l.css.clear();
                            l.themes.clear();
                            l.enabled_themes.clear();
                            l.theme_assets.clear();
                        })
                    })
                    .el(p),
            )
    } else {
        Button::new(t("Reset data")).variant(ButtonVariant::Danger).size(ButtonSize::Small).key("theme-reset").on_press(move |s: &mut S| drop(acc(s).form.data.insert("theme-reset".into(), json!(true)))).el(p)
    };
    vec![
        action_row("Export library", "Save all your themes and assets to one file.", export, p),
        action_row("Import library", "Add themes and assets from a library file.", import, p),
        action_row("Reset theme data", "Deletes the quick CSS of the desktop app theme, and every saved theme, asset and local file reference on this device.", reset, p),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_round_trips_through_an_export() {
        let mut a = Prefs::default();
        a.themes.push(theme(":root { --text-link: #f00 }".into(), "red.css", 1, 0));
        a.theme_assets.push(json!({ "name": "bg.png", "mimeType": "image/png", "dataUrl": "data:," }));
        let file = export(&a);
        let mut b = Prefs::default();
        assert_eq!(import(&mut b, &file), Some((1, 1)));
        assert_eq!((b.themes[0].name.as_str(), b.themes[0].css.as_str()), ("Red", ":root { --text-link: #f00 }"));
        assert_eq!(import(&mut b, &json!({ "version": 2, "themes": [] })), None);
        b.enabled_themes.push(b.themes[0].id.clone());
        b.css = ":root { --text-link: #0f0 }".into();
        assert_eq!(b.theme_css(), ":root { --text-link: #f00 }\n\n:root { --text-link: #0f0 }", "quick CSS goes last");
    }
}
