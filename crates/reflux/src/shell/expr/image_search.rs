//! Reverse image search (`ReverseImageSearch` = `UrlTemplateProviderStore`,
//! `ReverseImageSearchMenuItems`, `SearchProviderPickerModal` in image mode): Fluxer's built-in
//! engines (Google Lens enabled) plus custom ones, enabled on this device; the default is
//! `synced_preferences.search_engines.reverse_image_search_engine_id`. The first search without
//! a default asks which provider to use and remembers it. Advanced's "Reverse image search"
//! configures them (`settings::search_engines`).

use reflux_store::Store;
use rune::{El, col, row, text};

use super::{ShellAccess, ShellState};
use crate::desktop::Prefs;
use crate::i18n::t;
use crate::synced;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, MenuEntry, MenuItem, Modal, Select, popup_access};

/// `ReverseImageSearch.builtIns`: (id, name, URL template).
const BUILT_INS: [(&str, &str, &str); 6] = [
    (
        "google_lens",
        "Google Lens",
        "https://lens.google.com/uploadbyurl?url={url}",
    ),
    (
        "yandex",
        "Yandex",
        "https://yandex.com/images/search?rpt=imageview&url={url}",
    ),
    (
        "bing",
        "Bing",
        "https://www.bing.com/images/searchbyimage?cbir=sbi&imgurl={url}",
    ),
    ("tineye", "TinEye", "https://tineye.com/search?url={url}"),
    ("saucenao", "SauceNAO", "https://saucenao.com/search.php?url={url}"),
    ("iqdb", "IQDB", "https://iqdb.org/?url={url}"),
];

/// `SyncedPreferences.search_engines` and its `reverse_image_search_engine_id`.
pub(super) const SEARCH_ENGINES: u32 = 102;
const DEFAULT_FIELD: u32 = 2;

/// `UrlTemplateProvider`.
#[derive(Clone, Debug, PartialEq)]
pub struct Engine {
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub built_in: bool,
}

/// `engines`: the built-ins, then the custom ones.
pub fn engines(prefs: &Prefs) -> Vec<Engine> {
    let on = |id: &str| prefs.ris_enabled.iter().any(|e| e == id);
    let built = BUILT_INS.iter().map(|(id, name, url)| Engine {
        id: id.to_string(),
        name: name.to_string(),
        url: url.to_string(),
        enabled: on(id),
        built_in: true,
    });
    let custom = prefs.ris_custom.iter().map(|(id, name, url)| Engine {
        id: id.clone(),
        name: name.clone(),
        url: url.clone(),
        enabled: on(id),
        built_in: false,
    });
    built.chain(custom).collect()
}

/// The synced default id, if any (the queued one before it's sent).
pub fn default_id(store: &Store, st: &ShellState) -> Option<String> {
    let p = synced::get(&st.synced.view(&store.settings.synced_preferences), SEARCH_ENGINES)?;
    synced::fields(&p)?
        .iter()
        .rev()
        .find(|f| f.num == DEFAULT_FIELD)
        .map(synced::str_of)
        .filter(|s| !s.is_empty())
}

/// `defaultEngine`: the default, when it's enabled.
pub fn default_engine(store: &Store, st: &ShellState) -> Option<Engine> {
    let id = default_id(store, st)?;
    engines(&st.prefs).into_iter().find(|e| e.id == id && e.enabled)
}

/// Queues a new default for [`super::sync::poll`] to send.
pub fn set_default(st: &mut ShellState, id: Option<String>) {
    st.expr.ris_default = Some(id);
}

/// Saves the device's engine list.
pub fn save_prefs(st: &mut ShellState, f: impl FnOnce(&mut Prefs)) {
    let mut prefs = st.prefs.clone();
    f(&mut prefs);
    st.prefs = prefs.clone();
    st.intents.push(super::Intent::Prefs(Box::new(prefs)));
}

/// `setEnabled`: turning off the default clears it.
pub fn set_enabled(st: &mut ShellState, id: &str, on: bool, was_default: bool) {
    save_prefs(st, |p| {
        p.ris_enabled.retain(|e| e != id);
        if on {
            p.ris_enabled.push(id.to_string());
        }
    });
    if !on && was_default {
        set_default(st, None);
    }
}

/// `setDefaultEngine`: enables it too.
pub fn make_default(st: &mut ShellState, id: &str) {
    if !st.prefs.ris_enabled.iter().any(|e| e == id) {
        save_prefs(st, |p| p.ris_enabled.push(id.to_string()));
    }
    set_default(st, Some(id.to_string()));
}

/// `buildSearchUrl`: the template with the image URL percent-encoded.
fn search_url(engines: &[Engine], id: &str, image: &str) -> Option<String> {
    engines
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.url.replace("{url}", &super::enc(image)))
}

fn search(engines: &[Engine], id: &str, image: &str) {
    if let Some(u) = search_url(engines, id, image) {
        crate::settings::privacy::open_url(&u);
    }
}

/// `SearchProviderContextMenuItems`: nothing with no engine enabled; one "Reverse image search"
/// item (the default, or the picker); or, with a default and others enabled, a submenu of the
/// default (hinted "Default") and the rest.
pub fn entries<S: 'static>(
    store: &Store,
    st: &ShellState,
    image: &str,
    acc: ShellAccess<S>,
    close: impl Fn(&mut S) + Clone + 'static,
) -> Vec<MenuEntry<S>> {
    let all = engines(&st.prefs);
    let enabled: Vec<Engine> = all.iter().filter(|e| e.enabled).cloned().collect();
    if enabled.is_empty() {
        return Vec::new();
    }
    let default = default_engine(store, st);
    let label = t("Reverse image search");
    let alternates: Vec<Engine> = match &default {
        Some(d) => enabled.iter().filter(|e| e.id != d.id).cloned().collect(),
        None => Vec::new(),
    };
    let item = |e: &Engine| {
        let (close, image, id, all) = (close.clone(), image.to_string(), e.id.clone(), all.clone());
        MenuItem::new(format!("ris-{}", e.id), e.name.clone()).on_select(move |s: &mut S| {
            search(&all, &id, &image);
            close(s);
        })
    };
    match default {
        Some(d) if !alternates.is_empty() => {
            let mut sub = vec![MenuEntry::Item(item(&d).hint(t("Default")))];
            sub.extend(alternates.iter().map(|e| MenuEntry::Item(item(e))));
            vec![MenuEntry::Submenu("ris".into(), label.into(), sub)]
        }
        _ => {
            let image = image.to_string();
            vec![MenuEntry::Item(
                MenuItem::new("ris", label)
                    .icon(&crate::icons::MAGNIFYING_GLASS)
                    .on_select(move |s: &mut S| {
                        close(s);
                        let st = acc(s);
                        match &default {
                            Some(d) => search(&all, &d.id, &image),
                            None => {
                                st.expr.image_search = Some((image.clone(), enabled[0].id.clone()))
                            }
                        }
                    }),
            )]
        }
    }
}

/// `SearchProviderPickerModal` (image mode): every engine in a select; Search makes the pick the
/// default and searches with it.
pub fn layer<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some((_, sel)) = st.expr.image_search.clone() else { return };
    let close = move |s: &mut S| acc(s).expr.image_search = None;
    let all = engines(&st.prefs);
    let options: Vec<(&str, &str)> = all
        .iter()
        .map(|e| (e.id.as_str(), e.name.as_str()))
        .collect();
    let path = format!("{} > {}", t("User Settings"), t("Advanced"));
    let desc = crate::i18n::tf(
        "Choose where {PRODUCT_NAME} should send images for reverse image search. We'll remember your pick. You can change it or add your own provider later in {searchEnginesSettingsPath}.",
        &[("PRODUCT_NAME", "Fluxer"), ("searchEnginesSettingsPath", &path)],
    );
    let mut select_layer = Vec::new();
    let select = Select::new("ris-engine", &options, Some(&sel))
        .label(t("Reverse image search provider"))
        .width(408.0)
        .on_choose(move |s: &mut S, v| {
            if let Some(x) = &mut acc(s).expr.image_search {
                x.1 = v.to_string();
            }
        })
        .el(&st.popups, popup_access(move |s: &mut S| &mut acc(s).popups), p, &mut select_layer);
    let body = col().w_full().gap(16.0).child(text(desc).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap()).child(select);
    let go = move |s: &mut S| {
        let st = acc(s);
        let Some((image, engine)) = st.expr.image_search.take() else {
            return;
        };
        make_default(st, &engine);
        search(&engines(&st.prefs), &engine, &image);
    };
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
        .child(Button::new(t("Search")).key("ris-go").on_press(go).el(p));
    layer.push(Modal::new(t("Pick a reverse image search provider"), close).body(body).footer(footer).el(p));
    layer.extend(select_layer);
}

/// `AddCustomSearchEngineModal`'s checks (image mode): the error, or the trimmed fields.
pub fn validate(name: &str, url: &str) -> Result<(String, String), &'static str> {
    let (name, url) = (name.trim(), url.trim());
    if name.is_empty() {
        return Err("Name is required.");
    }
    if url.is_empty() {
        return Err("URL pattern is required.");
    }
    if !url.contains("{url}") {
        return Err("URL pattern must contain the {url} placeholder.");
    }
    if url::Url::parse(&url.replacen("{url}", "test", 1)).is_err() {
        return Err("URL pattern must be a valid URL.");
    }
    Ok((name.to_string(), url.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engines_urls_and_validation() {
        let mut prefs = Prefs::default();
        prefs.ris_custom.push((
            "custom_1".into(),
            "Mine".into(),
            "https://x.test/s?u={url}".into(),
        ));
        prefs.ris_enabled.push("custom_1".into());
        let all = engines(&prefs);
        assert_eq!(
            all.iter()
                .filter(|e| e.enabled)
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["google_lens", "custom_1"]
        );
        assert_eq!(
            search_url(&all, "tineye", "https://a.b/c.png?x=1").as_deref(),
            Some("https://tineye.com/search?url=https%3A%2F%2Fa.b%2Fc.png%3Fx%3D1")
        );
        assert_eq!(
            search_url(&all, "custom_1", "a").as_deref(),
            Some("https://x.test/s?u=a")
        );
        assert_eq!(validate(" ", "x"), Err("Name is required."));
        assert_eq!(
            validate("a", "https://x.test/"),
            Err("URL pattern must contain the {url} placeholder.")
        );
        assert_eq!(
            validate("a", "not a url {url}"),
            Err("URL pattern must be a valid URL.")
        );
        assert!(validate("a", "https://x.test/?u={url}").is_ok());
    }

    #[test]
    fn default_splices_into_search_engines() {
        let mut se = Vec::new();
        synced::bytes(&mut se, 1, b"google");
        let b64 = synced::set("", SEARCH_ENGINES, Some(&se));
        let next = crate::shell::search_providers::with_default(&b64, 2, Some("bing"));
        let f = synced::fields(&synced::get(&next, SEARCH_ENGINES).unwrap())
            .unwrap()
            .iter()
            .map(|f| (f.num, synced::str_of(f)))
            .collect::<Vec<_>>();
        assert_eq!(f, [(1, "google".to_string()), (2, "bing".to_string())]);
        let cleared = crate::shell::search_providers::with_default(&next, 2, None);
        assert_eq!(
            synced::get(&cleared, SEARCH_ENGINES).as_deref(),
            Some(&se[..])
        );
    }
}
