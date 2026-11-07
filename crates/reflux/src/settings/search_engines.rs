//! Advanced's "Reverse image search" settings (`SearchProviderSettingsModal` in image mode =
//! `ReverseImageSearchContent` / `EngineSection`): the default, built-in switches, custom engines
//! (`AddCustomSearchEngineModal`, remove confirmation). Engines live in
//! [`crate::shell::expr::image_search`].

use reflux_store::Store;
use rune::{El, col, icon, row, text};

use super::AccountModal;
use crate::i18n::{t, tf};
use crate::icons;
use crate::shell::expr::image_search::{self, Engine};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Modal, ModalSize, Select, Switch};

/// `SettingsTabSection`: title and description over the content.
fn section(title: &str, desc: &str, content: El, p: &Palette) -> El {
    col()
        .w_full()
        .gap(12.0)
        .child(
            col()
                .w_full()
                .gap(4.0)
                .child(
                    text(t(title).to_string())
                        .font(font(15.0, 600.0).lh(19.5))
                        .color(p.text_primary),
                )
                .child(
                    text(desc.to_string())
                        .font(font(14.0, 400.0).lh(20.0))
                        .color(p.text_primary_muted)
                        .wrap(),
                ),
        )
        .child(content)
}

fn back<S: 'static>(acc: ShellAccess<S>) -> impl Fn(&mut S) + Clone + 'static {
    move |s: &mut S| acc(s).open_account_modal(AccountModal::ReverseImageSearch)
}

/// The modal for `m`, if it's one of these.
pub fn modal<S: 'static>(
    store: &Store,
    st: &ShellState,
    m: &AccountModal,
    p: &Palette,
    acc: ShellAccess<S>,
    layer: &mut Vec<El>,
) -> bool {
    match m {
        AccountModal::ReverseImageSearch => {
            let mut pops = Vec::new();
            layer.push(configure(store, st, p, acc, &mut pops));
            layer.append(&mut pops);
        }
        AccountModal::RisEngine(id) => layer.push(edit(st, id.as_deref(), p, acc)),
        AccountModal::RisRemove(id) => layer.push(remove(store, st, id, p, acc)),
        _ => return false,
    }
    true
}

fn configure<S: 'static>(
    store: &Store,
    st: &ShellState,
    p: &Palette,
    acc: ShellAccess<S>,
    layer: &mut Vec<El>,
) -> El {
    let all = image_search::engines(&st.prefs);
    let enabled: Vec<&Engine> = all.iter().filter(|e| e.enabled).collect();
    let default = image_search::default_engine(store, st).map(|e| e.id);
    let default_ui = if enabled.is_empty() {
        text(t(
            "Enable at least one reverse image search provider below.",
        ))
        .font(font(14.0, 400.0).lh(20.0))
        .color(p.text_tertiary)
        .wrap()
    } else {
        let options: Vec<(&str, &str)> = enabled
            .iter()
            .map(|e| (e.id.as_str(), e.name.as_str()))
            .collect();
        let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
        Select::new("ris-default", &options, default.as_deref())
            .width(552.0)
            .on_choose(move |s: &mut S, v| image_search::make_default(acc(s), v))
            .el(&st.popups, pops, p, layer)
    };
    let toggle = |e: &Engine| {
        let (id, on, was_default) = (
            e.id.clone(),
            e.enabled,
            default.as_deref() == Some(e.id.as_str()),
        );
        Switch::new(on)
            .label(e.name.clone())
            .on_toggle(move |s: &mut S| image_search::set_enabled(acc(s), &id, !on, was_default))
            .el(p)
    };
    let built_in = col()
        .w_full()
        .gap(12.0)
        .children(all.iter().filter(|e| e.built_in).map(toggle));
    let mut custom = col().w_full().gap(12.0);
    for e in all.iter().filter(|e| !e.built_in) {
        let (edit_id, remove_id) = (e.id.clone(), e.id.clone());
        let edit = row()
            .py(4.0)
            .px(8.0)
            .radius(4.0)
            .fill(p.background_modifier_accent)
            .hover(p.background_modifier_hover)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(tf("Edit {engineName}", &[("engineName", &e.name)]))
            .on_press(move |s: &mut S| {
                acc(s).open_account_modal(AccountModal::RisEngine(Some(edit_id.clone())))
            })
            .child(
                text(t("Edit"))
                    .font(font(12.0, 400.0).lh(16.0))
                    .color(p.text_primary_muted),
            );
        let remove = row()
            .padding(4.0)
            .radius(4.0)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(tf("Remove {engineName}", &[("engineName", &e.name)]))
            .on_press(move |s: &mut S| {
                acc(s).open_account_modal(AccountModal::RisRemove(remove_id.clone()))
            })
            .child(
                icon(&icons::TRASH_FILL, 16.0)
                    .color(p.text_primary_muted)
                    .group_color(p.status_danger),
            );
        custom = custom.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(el_grow(toggle(e)))
                .child(row().gap(4.0).center_y().child(edit).child(remove)),
        );
    }
    custom = custom.child(
        row()
            .pt(if all.iter().any(|e| !e.built_in) {
                12.0
            } else {
                0.0
            })
            .child(
                Button::new(t("Add reverse image search"))
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Small)
                    .key("ris-add")
                    .on_press(move |s: &mut S| {
                        acc(s).open_account_modal(AccountModal::RisEngine(None))
                    })
                    .el(p),
            ),
    );
    let custom_desc = tf(
        "Add your own reverse image search providers with a custom URL pattern. Use {url} as a placeholder for the image URL.",
        &[("url", "{url}")],
    );
    let body = col()
        .w_full()
        .gap(32.0)
        .child(section("Default reverse image search", t("Choose which reverse image search service is used by default when searching an image."), default_ui, p))
        .child(section(
            "Built-in reverse image search",
            t("Enable or disable built-in reverse image search providers. Enabled providers appear in the context menu of images, avatars, banners, stickers, and emoji."),
            built_in,
            p,
        ))
        .child(section("Custom reverse image search", &custom_desc, custom, p));
    // `Modal.Content` scrolls inside the window (24 margin, header, padding).
    let [_, h] = rune::memo::layout_size(super::LAYER_KEY).unwrap_or([1280.0, 800.0]);
    let body = crate::ui::scroller(
        col()
            .key("ris-scroll")
            .w_full()
            .max_h(h - 136.0)
            .scroll(rune::ScrollSpec {
                stick_bottom: false,
            })
            .child(body.pr(8.0)),
        false,
        p,
    );
    Modal::new(t("Reverse image search"), move |s: &mut S| {
        acc(s).close_account_modals()
    })
    .size(ModalSize::Medium)
    .body(body)
    .el(p)
}

fn el_grow(e: El) -> El {
    row().grow(1.0).min_w(0.0).child(e)
}

/// `AddCustomSearchEngineModal` (image mode): add, or edit custom engine `id`.
fn edit<S: 'static>(st: &ShellState, id: Option<&str>, p: &Palette, acc: ShellAccess<S>) -> El {
    let current = id
        .and_then(|id| st.prefs.ris_custom.iter().find(|c| c.0 == id))
        .cloned();
    let (name0, url0) = current
        .as_ref()
        .map_or((String::new(), String::new()), |c| {
            (c.1.clone(), c.2.clone())
        });
    let title = if id.is_some() {
        t("Edit reverse image search engine")
    } else {
        t("Add reverse image search engine")
    };
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(super::input(
            st,
            acc,
            "m.ris_name",
            &name0,
            t("Name"),
            t("My reverse image search"),
            false,
            p,
        ))
        .child(super::input(
            st,
            acc,
            "m.ris_url",
            &url0,
            t("URL pattern"),
            "https://example.com/searchbyimage?url={url}",
            false,
            p,
        ));
    if let Some(e) = &st.form.error {
        body = body.child(
            text(e.clone())
                .font(font(14.0, 400.0).lh(20.0))
                .color(p.status_danger)
                .wrap(),
        );
    }
    body = body.child(
        text(tf(
            "Use {url} where the image URL should be inserted.",
            &[("url", "{url}")],
        ))
        .font(font(14.0, 400.0).lh(20.0))
        .color(p.text_primary_muted)
        .wrap(),
    );
    let id = id.map(str::to_string);
    let (n0, u0) = (name0.clone(), url0.clone());
    let submit = move |s: &mut S| {
        let st = acc(s);
        let get = |k: &str, init: &str| {
            st.form
                .text
                .get(k)
                .map_or(init.to_string(), |t| t.text.clone())
        };
        match image_search::validate(&get("m.ris_name", &n0), &get("m.ris_url", &u0)) {
            Err(e) => st.form.error = Some(t(e).to_string()),
            Ok((name, url)) => {
                let id = id.clone();
                image_search::save_prefs(st, |p| match id {
                    Some(id) => {
                        if let Some(c) = p.ris_custom.iter_mut().find(|c| c.0 == id) {
                            (c.1, c.2) = (name, url);
                        }
                    }
                    None => {
                        let id = format!("custom_{}", crate::app::now_ms());
                        p.ris_enabled.push(id.clone());
                        p.ris_custom.push((id, name, url));
                    }
                });
                st.open_account_modal(AccountModal::ReverseImageSearch);
            }
        }
    };
    let footer = row()
        .gap(8.0)
        .child(
            Button::new(t("Cancel"))
                .variant(ButtonVariant::Secondary)
                .on_press(back(acc))
                .el(p),
        )
        .child(
            Button::new(if current.is_some() {
                t("Save")
            } else {
                t("Add")
            })
            .key("ris-save")
            .on_press(submit)
            .el(p),
        );
    Modal::new(title, back(acc)).body(body).footer(footer).el(p)
}

/// The "Remove reverse image search" `ConfirmModal`.
fn remove<S: 'static>(
    store: &Store,
    st: &ShellState,
    id: &str,
    p: &Palette,
    acc: ShellAccess<S>,
) -> El {
    let name = st
        .prefs
        .ris_custom
        .iter()
        .find(|c| c.0 == id)
        .map_or("", |c| c.1.as_str());
    let body = text(tf(
        "Are you sure you want to remove {engineName}?",
        &[("engineName", name)],
    ))
    .font(font(14.0, 400.0).lh(20.0))
    .color(p.text_primary_muted)
    .wrap();
    let was_default = image_search::default_id(store, st).as_deref() == Some(id);
    let id = id.to_string();
    let go = move |s: &mut S| {
        let st = acc(s);
        image_search::save_prefs(st, |p| {
            p.ris_custom.retain(|c| c.0 != id);
            p.ris_enabled.retain(|e| *e != id);
        });
        if was_default {
            image_search::set_default(st, None);
        }
        st.open_account_modal(AccountModal::ReverseImageSearch);
    };
    let footer = row()
        .gap(8.0)
        .child(
            Button::new(t("Cancel"))
                .variant(ButtonVariant::Secondary)
                .on_press(back(acc))
                .el(p),
        )
        .child(
            Button::new(t("Remove"))
                .variant(ButtonVariant::Danger)
                .key("ris-remove")
                .on_press(go)
                .el(p),
        );
    Modal::new(t("Remove reverse image search"), back(acc))
        .body(body)
        .footer(footer)
        .el(p)
}
