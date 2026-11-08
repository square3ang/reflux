//! Advanced's provider settings (`SearchProviderSettingsModal` = `SearchProviderSettingsModal`
//! with `SearchEnginesControl` / `TranslatorsControl` / `ReverseImageSearchControl`, the
//! `EngineSection` of `SearchEnginesTab.tsx`): the default, built-in switches and custom engines
//! (`AddCustomSearchEngineModal`, remove confirmation). Text search and translation engines live
//! in [`crate::shell::search_providers`], reverse image search in
//! [`crate::shell::expr::image_search`].

use crate::ui::SmoothWheel;
use reflux_store::Store;
use rune::{El, col, icon, row};
use crate::ui::mark::marked as text;

use super::AccountModal;
use crate::i18n::{t, tf};
use crate::icons;
use crate::shell::expr::image_search;
use crate::shell::search_providers::{self, Kind};
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Modal, ModalSize, Select, Switch};

/// `SearchEngineMode`: which provider list a settings modal edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Search,
    Translate,
    Image,
}

struct Config {
    title: &'static str,
    default_title: &'static str,
    default_desc: &'static str,
    built_in_title: &'static str,
    built_in_desc: &'static str,
    custom_title: &'static str,
    custom_desc: String,
    add: &'static str,
    no_engines: &'static str,
    confirm: &'static str,
    edit_title: &'static str,
    add_title: &'static str,
    name_placeholder: &'static str,
    url_example: &'static str,
    hint: &'static str,
}

impl Mode {
    fn config(self) -> Config {
        match self {
            Mode::Search => Config {
                title: "Search engines",
                default_title: "Default search engine",
                default_desc: "Choose which search engine is used by default when searching selected text.",
                built_in_title: "Built-in search engines",
                built_in_desc: "Enable or disable built-in search engines. Enabled engines appear in the message context menu when text is selected.",
                custom_title: "Custom search engines",
                custom_desc: tf(
                    "Add your own search engines with a custom URL pattern. Use {query} as a placeholder for the search text.",
                    &[("query", "{query}")],
                ),
                add: "Add search engine",
                no_engines: "Enable at least one search engine below.",
                confirm: "Remove search engine",
                edit_title: "Edit search engine",
                add_title: "Add search engine",
                name_placeholder: "My search engine",
                url_example: "https://example.com/search?q={query}",
                hint: "Use {query} where the search text should be inserted.",
            },
            Mode::Translate => Config {
                title: "Translators",
                default_title: "Default translator",
                default_desc: "Choose which translator is used by default when translating selected text.",
                built_in_title: "Built-in translators",
                built_in_desc: "Enable or disable built-in translators. Enabled translators appear in the message context menu when text is selected.",
                custom_title: "Custom translators",
                custom_desc: tf(
                    "Add your own translators with a custom URL pattern. Use {query} as a placeholder for the text to translate.",
                    &[("query", "{query}")],
                ),
                add: "Add translator",
                no_engines: "Enable at least one translator below.",
                confirm: "Remove translator",
                edit_title: "Edit translation provider",
                add_title: "Add translation provider",
                name_placeholder: "My translator",
                url_example: "https://example.com/translate?text={query}",
                hint: "Use {query} where the text to translate should be inserted.",
            },
            Mode::Image => Config {
                title: "Reverse image search",
                default_title: "Default reverse image search",
                default_desc: "Choose which reverse image search service is used by default when searching an image.",
                built_in_title: "Built-in reverse image search",
                built_in_desc: "Enable or disable built-in reverse image search providers. Enabled providers appear in the context menu of images, avatars, banners, stickers, and emoji.",
                custom_title: "Custom reverse image search",
                custom_desc: tf(
                    "Add your own reverse image search providers with a custom URL pattern. Use {url} as a placeholder for the image URL.",
                    &[("url", "{url}")],
                ),
                add: "Add reverse image search",
                no_engines: "Enable at least one reverse image search provider below.",
                confirm: "Remove reverse image search",
                edit_title: "Edit reverse image search engine",
                add_title: "Add reverse image search engine",
                name_placeholder: "My reverse image search",
                url_example: "https://example.com/searchbyimage?url={url}",
                hint: "Use {url} where the image URL should be inserted.",
            },
        }
    }

    fn kind(self) -> Option<Kind> {
        match self {
            Mode::Search => Some(Kind::Search),
            Mode::Translate => Some(Kind::Translate),
            Mode::Image => None,
        }
    }
}

/// `UrlTemplateProvider`, from whichever store this mode uses.
#[derive(Clone)]
struct Row {
    id: String,
    name: String,
    url: String,
    enabled: bool,
    built_in: bool,
}

fn rows(st: &ShellState, mode: Mode) -> Vec<Row> {
    match mode.kind() {
        Some(kind) => search_providers::engines(&st.prefs, kind)
            .into_iter()
            .map(|e| Row { id: e.id, name: e.name, url: e.url, enabled: e.enabled, built_in: e.built_in })
            .collect(),
        None => image_search::engines(&st.prefs)
            .into_iter()
            .map(|e| Row { id: e.id, name: e.name, url: e.url, enabled: e.enabled, built_in: e.built_in })
            .collect(),
    }
}

fn default_id(store: &Store, st: &ShellState, mode: Mode) -> Option<String> {
    match mode.kind() {
        Some(kind) => search_providers::default_id(store, st, kind),
        None => image_search::default_id(store, st),
    }
}

fn default_engine(store: &Store, st: &ShellState, mode: Mode) -> Option<String> {
    match mode.kind() {
        Some(kind) => search_providers::default_engine(store, st, kind).map(|e| e.id),
        None => image_search::default_engine(store, st).map(|e| e.id),
    }
}

fn set_enabled(st: &mut ShellState, mode: Mode, id: &str, on: bool, was_default: bool) {
    match mode.kind() {
        Some(kind) => search_providers::set_enabled(st, kind, id, on, was_default),
        None => image_search::set_enabled(st, id, on, was_default),
    }
}

fn make_default(st: &mut ShellState, mode: Mode, id: &str) {
    match mode.kind() {
        Some(kind) => search_providers::make_default(st, kind, id),
        None => image_search::make_default(st, id),
    }
}

fn put_custom(st: &mut ShellState, mode: Mode, id: Option<&str>, name: String, url: String) {
    match mode.kind() {
        Some(kind) => search_providers::put_custom(st, kind, id, name, url),
        None => {
            let new_id = format!("custom_{}", crate::app::now_ms());
            image_search::save_prefs(st, |p| match id {
                Some(id) => {
                    if let Some(c) = p.ris_custom.iter_mut().find(|c| c.0 == id) {
                        (c.1, c.2) = (name, url);
                    }
                }
                None => {
                    p.ris_enabled.push(new_id.clone());
                    p.ris_custom.push((new_id, name, url));
                }
            });
        }
    }
}

fn remove_custom(st: &mut ShellState, mode: Mode, id: &str) {
    match mode.kind() {
        Some(kind) => search_providers::remove_custom(st, kind, id),
        None => image_search::save_prefs(st, |p| {
            p.ris_custom.retain(|c| c.0 != id);
            p.ris_enabled.retain(|e| *e != id);
        }),
    }
}

fn validate(mode: Mode, name: &str, url: &str) -> Result<(String, String), &'static str> {
    match mode.kind() {
        Some(_) => search_providers::validate(name, url),
        None => image_search::validate(name, url),
    }
}

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

fn back<S: 'static>(mode: Mode, acc: ShellAccess<S>) -> impl Fn(&mut S) + Clone + 'static {
    move |s: &mut S| acc(s).open_account_modal(AccountModal::Providers(mode))
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
        AccountModal::Providers(mode) => {
            let mut pops = Vec::new();
            layer.push(configure(store, st, *mode, p, acc, &mut pops));
            layer.append(&mut pops);
        }
        AccountModal::ProviderEdit(mode, id) => layer.push(edit(st, *mode, id.as_deref(), p, acc)),
        AccountModal::ProviderRemove(mode, id) => layer.push(remove(store, st, *mode, id, p, acc)),
        _ => return false,
    }
    true
}

fn configure<S: 'static>(
    store: &Store,
    st: &ShellState,
    mode: Mode,
    p: &Palette,
    acc: ShellAccess<S>,
    layer: &mut Vec<El>,
) -> El {
    let config = mode.config();
    let all = rows(st, mode);
    let enabled: Vec<&Row> = all.iter().filter(|e| e.enabled).collect();
    let default = default_engine(store, st, mode);
    let default_ui = if enabled.is_empty() {
        text(t(config.no_engines))
            .font(font(14.0, 400.0).lh(20.0))
            .color(p.text_tertiary)
            .wrap()
    } else {
        let options: Vec<(&str, &str)> = enabled.iter().map(|e| (e.id.as_str(), e.name.as_str())).collect();
        let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
        Select::new("provider-default", &options, default.as_deref())
            .width(552.0)
            .on_choose(move |s: &mut S, v| make_default(acc(s), mode, v))
            .el(&st.popups, pops, p, layer)
    };
    let toggle = |e: &Row| {
        let (id, on, was_default) = (e.id.clone(), e.enabled, default.as_deref() == Some(e.id.as_str()));
        Switch::new(on)
            .label(e.name.clone())
            .on_toggle(move |s: &mut S| set_enabled(acc(s), mode, &id, !on, was_default))
            .el(p)
            .key(e.id.clone())
    };
    let built_in = col().w_full().gap(12.0).children(all.iter().filter(|e| e.built_in).map(toggle));
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
            .on_press(move |s: &mut S| acc(s).open_account_modal(AccountModal::ProviderEdit(mode, Some(edit_id.clone()))))
            .child(text(t("Edit")).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted));
        let remove = row()
            .padding(4.0)
            .radius(4.0)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(tf("Remove {engineName}", &[("engineName", &e.name)]))
            .on_press(move |s: &mut S| acc(s).open_account_modal(AccountModal::ProviderRemove(mode, remove_id.clone())))
            .child(icon(&icons::TRASH_FILL, 16.0).color(p.text_primary_muted).group_color(p.status_danger));
        custom = custom.child(row().key(e.id.clone()).w_full().gap(8.0).center_y().child(el_grow(toggle(e))).child(row().gap(4.0).center_y().child(edit).child(remove)));
    }
    custom = custom.child(
        row()
            .key("add")
            .pt(if all.iter().any(|e| !e.built_in) { 12.0 } else { 0.0 })
            .child(
                Button::new(t(config.add))
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Small)
                    .key("provider-add")
                    .on_press(move |s: &mut S| acc(s).open_account_modal(AccountModal::ProviderEdit(mode, None)))
                    .el(p),
            ),
    );
    let body = col()
        .w_full()
        .gap(32.0)
        .child(section(config.default_title, t(config.default_desc), default_ui, p))
        .child(section(config.built_in_title, t(config.built_in_desc), built_in, p))
        .child(section(config.custom_title, &config.custom_desc, custom, p));
    // `Modal.Content` scrolls inside the window (24 margin, header, padding).
    let [_, h] = rune::memo::layout_size(super::LAYER_KEY).unwrap_or([1280.0, 800.0]);
    let body = crate::ui::scroller(
        col()
            .key("provider-scroll")
            .w_full()
            .max_h(h - 136.0)
            .scroll(rune::ScrollSpec { stick_bottom: false }).smooth_wheel::<S>("provider-scroll")
            .child(body.pr(8.0)),
        false,
        p,
    );
    Modal::new(t(config.title), move |s: &mut S| acc(s).close_account_modals())
        .size(ModalSize::Medium)
        .body(body)
        .el(p)
}

fn el_grow(e: El) -> El {
    row().grow(1.0).min_w(0.0).child(e)
}

/// `AddCustomSearchEngineModal`: add, or edit custom engine `id`.
fn edit<S: 'static>(st: &ShellState, mode: Mode, id: Option<&str>, p: &Palette, acc: ShellAccess<S>) -> El {
    let config = mode.config();
    let current = id.and_then(|id| {
        rows(st, mode)
            .into_iter()
            .find(|c| c.id == id)
            .filter(|c| !c.built_in)
    });
    let (name0, url0) = current.as_ref().map_or((String::new(), String::new()), |c| (c.name.clone(), c.url.clone()));
    let title = if id.is_some() { t(config.edit_title) } else { t(config.add_title) };
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(super::input(st, acc, "m.provider_name", &name0, t("Name"), t(config.name_placeholder), false, p))
        .child(super::input(st, acc, "m.provider_url", &url0, t("URL pattern"), config.url_example, false, p));
    if let Some(e) = &st.form.error {
        body = body.child(text(e.clone()).key("error").font(font(14.0, 400.0).lh(20.0)).color(p.status_danger).wrap());
    }
    body = body.child(
        text(t(config.hint))
            .key("hint")
            .font(font(14.0, 400.0).lh(20.0))
            .color(p.text_primary_muted)
            .wrap(),
    );
    let id = id.map(str::to_string);
    let (n0, u0) = (name0.clone(), url0.clone());
    let submit = move |s: &mut S| {
        let st = acc(s);
        let get = |k: &str, init: &str| st.form.text.get(k).map_or(init.to_string(), |t| t.text.clone());
        match validate(mode, &get("m.provider_name", &n0), &get("m.provider_url", &u0)) {
            Err(e) => st.form.error = Some(t(e).to_string()),
            Ok((name, url)) => {
                put_custom(st, mode, id.as_deref(), name, url);
                st.open_account_modal(AccountModal::Providers(mode));
            }
        }
    };
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(back(mode, acc)).el(p))
        .child(
            Button::new(if current.is_some() { t("Save") } else { t("Add") })
                .key("provider-save")
                .on_press(submit)
                .el(p),
        );
    Modal::new(title, back(mode, acc)).body(body).footer(footer).el(p)
}

/// The "Remove …" `ConfirmModal`.
fn remove<S: 'static>(store: &Store, st: &ShellState, mode: Mode, id: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let config = mode.config();
    let name = rows(st, mode).into_iter().find(|c| c.id == id).map_or(String::new(), |c| c.name);
    let body = text(tf("Are you sure you want to remove {engineName}?", &[("engineName", &name)]))
        .font(font(14.0, 400.0).lh(20.0))
        .color(p.text_primary_muted)
        .wrap();
    let was_default = default_id(store, st, mode).as_deref() == Some(id);
    let id = id.to_string();
    let go = move |s: &mut S| {
        let st = acc(s);
        remove_custom(st, mode, &id);
        if was_default {
            match mode.kind() {
                Some(kind) => search_providers::set_default(st, kind, None),
                None => image_search::set_default(st, None),
            }
        }
        st.open_account_modal(AccountModal::Providers(mode));
    };
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(back(mode, acc)).el(p))
        .child(Button::new(t("Remove")).variant(ButtonVariant::Danger).key("provider-remove").on_press(go).el(p));
    Modal::new(t(config.confirm), back(mode, acc)).body(body).footer(footer).el(p)
}
