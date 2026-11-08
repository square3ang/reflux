//! Selected-text web search and translation (`SearchEngine` / `Translation` =
//! `UrlTemplateProviderStore`, `WebSearchMenuItems` / `TranslateMenuItems`,
//! `SearchProviderPickerModal` in text or translate mode): Fluxer's built-in engines plus custom
//! ones, enabled on this device; the default is
//! `synced_preferences.search_engines.text_search_engine_id` (field 1) or
//! `.translation_provider_id` (field 3). Without a default the first pick asks which provider to
//! use and remembers it. Advanced configures them (`settings::search_engines`).
//!
//! Both open the engine's URL in the browser, as Fluxer does: there is no instance translation
//! endpoint (`Translation.ts` only builds a URL template).

use reflux_store::Store;
use rune::{El, col, row, text};

use super::{ShellAccess, ShellState};
use crate::desktop::Prefs;
use crate::i18n::{t, tf};
use crate::synced;
use crate::theme::{Palette, font};
use crate::ui::{MenuEntry, MenuItem, Modal, Select, popup_access};

/// `SyncedPreferences.search_engines`; its fields 1, 2 and 3 are the text search, reverse image
/// search and translation defaults.
pub const SEARCH_ENGINES: u32 = 102;

/// `SearchEngine.builtIns` (text mode): (id, name, URL template).
const SEARCH_BUILT_INS: [(&str, &str, &str); 12] = [
    ("google", "Google", "https://www.google.com/search?q={query}"),
    ("bing", "Bing", "https://www.bing.com/search?q={query}"),
    ("duckduckgo", "DuckDuckGo", "https://duckduckgo.com/?q={query}"),
    ("yahoo", "Yahoo", "https://search.yahoo.com/search?p={query}"),
    ("ecosia", "Ecosia", "https://www.ecosia.org/search?q={query}"),
    ("brave", "Brave Search", "https://search.brave.com/search?q={query}"),
    ("startpage", "Startpage", "https://www.startpage.com/do/dsearch?query={query}"),
    ("yandex", "Yandex", "https://yandex.com/search/?text={query}"),
    ("wikipedia", "Wikipedia", "https://en.wikipedia.org/w/index.php?search={query}"),
    ("youtube", "YouTube", "https://www.youtube.com/results?search_query={query}"),
    ("github", "GitHub", "https://github.com/search?q={query}"),
    ("reddit", "Reddit", "https://www.reddit.com/search/?q={query}"),
];

/// `Translation.builtIns`: (id, name, URL template).
const TRANSLATE_BUILT_INS: [(&str, &str, &str); 7] = [
    ("google_translate", "Google Translate", "https://translate.google.com/?sl=auto&tl=auto&text={query}&op=translate"),
    ("deepl", "DeepL", "https://www.deepl.com/translator#auto/auto/{query}"),
    ("bing_translator", "Bing Translator", "https://www.bing.com/translator/?text={query}"),
    ("yandex_translate", "Yandex Translate", "https://translate.yandex.com/?text={query}"),
    ("reverso", "Reverso", "https://www.reverso.net/text-translation#sl=auto&tl=eng&text={query}"),
    ("linguee", "Linguee", "https://www.linguee.com/english-german/search?source=auto&query={query}"),
    ("papago", "Papago", "https://papago.naver.com/?st={query}"),
];

/// Which provider list a menu or settings surface works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Search,
    Translate,
}

impl Kind {
    fn built_ins(self) -> &'static [(&'static str, &'static str, &'static str)] {
        match self {
            Kind::Search => &SEARCH_BUILT_INS,
            Kind::Translate => &TRANSLATE_BUILT_INS,
        }
    }

    /// The `SearchEngineSettings` field holding its default.
    pub fn field(self) -> u32 {
        match self {
            Kind::Search => 1,
            Kind::Translate => 3,
        }
    }

    /// `WebSearchMenuItems` / `TranslateMenuItems`' `defaultLabel`.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Search => t("Search the web"),
            Kind::Translate => t("Translate"),
        }
    }
}

/// `UrlTemplateProvider`.
#[derive(Clone, Debug, PartialEq)]
pub struct Engine {
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub built_in: bool,
}

fn lists(prefs: &Prefs, kind: Kind) -> (&Vec<String>, &Vec<(String, String, String)>) {
    match kind {
        Kind::Search => (&prefs.search_enabled, &prefs.search_custom),
        Kind::Translate => (&prefs.translators_enabled, &prefs.translators_custom),
    }
}

fn lists_mut(prefs: &mut Prefs, kind: Kind) -> (&mut Vec<String>, &mut Vec<(String, String, String)>) {
    match kind {
        Kind::Search => (&mut prefs.search_enabled, &mut prefs.search_custom),
        Kind::Translate => (&mut prefs.translators_enabled, &mut prefs.translators_custom),
    }
}

/// `engines`: the built-ins, then the custom ones.
pub fn engines(prefs: &Prefs, kind: Kind) -> Vec<Engine> {
    let (enabled, custom) = lists(prefs, kind);
    let on = |id: &str| enabled.iter().any(|e| e == id);
    let built = kind.built_ins().iter().map(|(id, name, url)| Engine {
        id: id.to_string(),
        name: name.to_string(),
        url: url.to_string(),
        enabled: on(id),
        built_in: true,
    });
    let custom = custom.iter().map(|(id, name, url)| Engine {
        id: id.clone(),
        name: name.clone(),
        url: url.clone(),
        enabled: on(id),
        built_in: false,
    });
    built.chain(custom).collect()
}

/// The synced default id, if any (the queued one before it's sent).
pub fn default_id(store: &Store, st: &ShellState, kind: Kind) -> Option<String> {
    let p = synced::get(&st.synced.view(&store.settings.synced_preferences), SEARCH_ENGINES)?;
    synced::fields(&p)?
        .iter()
        .rev()
        .find(|f| f.num == kind.field())
        .map(synced::str_of)
        .filter(|s| !s.is_empty())
}

/// `defaultEngine`: the default, when it's enabled.
pub fn default_engine(store: &Store, st: &ShellState, kind: Kind) -> Option<Engine> {
    let id = default_id(store, st, kind)?;
    engines(&st.prefs, kind).into_iter().find(|e| e.id == id && e.enabled)
}

/// `b64` with `field`'s default replaced (`persistDefault`), the other search engine fields kept
/// in field order as Fluxer's protobuf encoder writes them.
pub fn with_default(b64: &str, field: u32, id: Option<&str>) -> String {
    let p = synced::get(b64, SEARCH_ENGINES).unwrap_or_default();
    let mut items: Vec<(u32, Vec<u8>)> = synced::fields(&p)
        .unwrap_or_default()
        .iter()
        .filter(|f| f.num != field)
        .map(|f| (f.num, f.raw.to_vec()))
        .collect();
    if let Some(id) = id {
        let mut b = Vec::new();
        synced::bytes(&mut b, field, id.as_bytes());
        items.push((field, b));
    }
    items.sort_by_key(|f| f.0);
    let out: Vec<u8> = items.into_iter().flat_map(|(_, b)| b).collect();
    synced::set(b64, SEARCH_ENGINES, (!out.is_empty()).then_some(&out[..]))
}

/// Queues a new default for [`super::expr::sync::poll`] to send.
pub fn set_default(st: &mut ShellState, kind: Kind, id: Option<String>) {
    match kind {
        Kind::Search => st.expr.search_default = Some(id),
        Kind::Translate => st.expr.translate_default = Some(id),
    }
}

/// Saves the device's engine list.
pub fn save_prefs(st: &mut ShellState, f: impl FnOnce(&mut Prefs)) {
    let mut prefs = st.prefs.clone();
    f(&mut prefs);
    st.prefs = prefs.clone();
    st.intents.push(super::Intent::Prefs(Box::new(prefs)));
}

/// `setEnabled`: turning off the default clears it.
pub fn set_enabled(st: &mut ShellState, kind: Kind, id: &str, on: bool, was_default: bool) {
    save_prefs(st, |p| {
        let (enabled, _) = lists_mut(p, kind);
        enabled.retain(|e| e != id);
        if on {
            enabled.push(id.to_string());
        }
    });
    if !on && was_default {
        set_default(st, kind, None);
    }
}

/// `setDefaultEngine`: enables it too.
pub fn make_default(st: &mut ShellState, kind: Kind, id: &str) {
    if !lists(&st.prefs, kind).0.iter().any(|e| e == id) {
        save_prefs(st, |p| lists_mut(p, kind).0.push(id.to_string()));
    }
    set_default(st, kind, Some(id.to_string()));
}

/// Adds or updates a custom engine (an id of `None` adds).
pub fn put_custom(st: &mut ShellState, kind: Kind, id: Option<&str>, name: String, url: String) {
    let new_id = format!("custom_{}", crate::app::now_ms());
    save_prefs(st, |p| {
        let (enabled, custom) = lists_mut(p, kind);
        match id {
            Some(id) => {
                if let Some(c) = custom.iter_mut().find(|c| c.0 == id) {
                    (c.1, c.2) = (name, url);
                }
            }
            None => {
                enabled.push(new_id.clone());
                custom.push((new_id, name, url));
            }
        }
    });
}

/// Removes a custom engine.
pub fn remove_custom(st: &mut ShellState, kind: Kind, id: &str) {
    save_prefs(st, |p| {
        let (enabled, custom) = lists_mut(p, kind);
        custom.retain(|c| c.0 != id);
        enabled.retain(|e| e != id);
    });
}

/// `AddCustomSearchEngineModal`'s checks: the error, or the trimmed fields.
pub fn validate(name: &str, url: &str) -> Result<(String, String), &'static str> {
    let (name, url) = (name.trim(), url.trim());
    if name.is_empty() {
        return Err("Name is required.");
    }
    if url.is_empty() {
        return Err("URL pattern is required.");
    }
    if !url.contains("{query}") {
        return Err("URL pattern must contain the {query} placeholder.");
    }
    if url::Url::parse(&url.replacen("{query}", "test", 1)).is_err() {
        return Err("URL pattern must be a valid URL.");
    }
    Ok((name.to_string(), url.to_string()))
}

/// `buildSearchUrl`: the template with the query percent-encoded as `encodeURIComponent` does
/// (spaces as `%20`, not the form encoder's `+`).
pub fn search_url(engines: &[Engine], id: &str, query: &str) -> Option<String> {
    engines
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.url.replace("{query}", &super::expr::enc(query).replace('+', "%20")))
}

fn search(engines: &[Engine], id: &str, query: &str) {
    if let Some(u) = search_url(engines, id, query) {
        crate::settings::privacy::open_url(&u);
    }
}

/// `SearchProviderContextMenuItems`: nothing with no engine enabled or no text; one
/// "Search the web" / "Translate" item (the default, or the picker); or, with a default and
/// others enabled, a submenu of the default (hinted "Default") and the rest.
pub fn entries<S: 'static>(store: &Store, st: &ShellState, kind: Kind, text: &str, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    let all = engines(&st.prefs, kind);
    let enabled: Vec<Engine> = all.iter().filter(|e| e.enabled).cloned().collect();
    if enabled.is_empty() {
        return Vec::new();
    }
    let default = default_engine(store, st, kind);
    let label = kind.label();
    let alternates: Vec<Engine> = match &default {
        Some(d) => enabled.iter().filter(|e| e.id != d.id).cloned().collect(),
        None => Vec::new(),
    };
    let item = |e: &Engine| {
        let (text, id, all) = (text.to_string(), e.id.clone(), all.clone());
        MenuItem::new(format!("provider-{}-{}", kind.field(), e.id), e.name.clone()).on_select(move |_: &mut S| {
            search(&all, &id, &text);
        })
    };
    match default {
        Some(d) if !alternates.is_empty() => {
            let mut sub = vec![MenuEntry::Item(item(&d).hint(t("Default")))];
            sub.extend(alternates.iter().map(|e| MenuEntry::Item(item(e))));
            vec![MenuEntry::Submenu(format!("provider-{}", kind.field()), label.into(), sub)]
        }
        _ => {
            let (first, all, pick_text) = (enabled[0].id.clone(), all.clone(), text.to_string());
            vec![MenuEntry::Item(
                MenuItem::new(format!("provider-{}", kind.field()), label)
                    .icon(match kind {
                        Kind::Search => &crate::icons::MAGNIFYING_GLASS,
                        Kind::Translate => &crate::icons::TRANSLATE_FILL,
                    })
                    .on_select(move |s: &mut S| match &default {
                        Some(d) => search(&all, &d.id, &pick_text),
                        None => {
                            let st = acc(s);
                            st.popups.close();
                            st.expr.provider_picker = Some((kind, pick_text.clone(), first.clone()));
                        }
                    }),
            )]
        }
    }
}

/// `SearchProviderPickerModal` (text and translate modes): every engine in a select; the
/// confirm makes the pick the default and searches or translates with it.
pub fn layer<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some((kind, _, sel)) = st.expr.provider_picker.clone() else { return };
    let close = move |s: &mut S| acc(s).expr.provider_picker = None;
    let all = engines(&st.prefs, kind);
    let options: Vec<(&str, &str)> = all.iter().map(|e| (e.id.as_str(), e.name.as_str())).collect();
    let path = format!("{} > {}", t("User Settings"), t("Advanced"));
    let (title, desc, select_label, confirm) = match kind {
        Kind::Search => (
            t("Pick a web search provider"),
            tf(
                "Choose where {PRODUCT_NAME} should search highlighted text. We'll remember your pick. You can change it or add your own provider later in {searchEnginesSettingsPath}.",
                &[("PRODUCT_NAME", "Fluxer"), ("searchEnginesSettingsPath", &path)],
            ),
            t("Web search provider"),
            t("Search the web"),
        ),
        Kind::Translate => (
            t("Pick a translation provider"),
            tf(
                "Choose where {PRODUCT_NAME} should send highlighted text for translation. We'll remember your pick. You can change it or add your own provider later in {searchEnginesSettingsPath}.",
                &[("PRODUCT_NAME", "Fluxer"), ("searchEnginesSettingsPath", &path)],
            ),
            t("Translation provider"),
            t("Translate"),
        ),
    };
    let mut select_layer = Vec::new();
    let select = Select::new("provider-engine", &options, Some(&sel))
        .label(select_label)
        .width(408.0)
        .on_choose(move |s: &mut S, v| {
            if let Some(x) = &mut acc(s).expr.provider_picker {
                x.2 = v.to_string();
            }
        })
        .el(&st.popups, popup_access(move |s: &mut S| &mut acc(s).popups), p, &mut select_layer);
    let body = col().w_full().gap(16.0).child(text(desc).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap()).child(select);
    let go = move |s: &mut S| {
        let st = acc(s);
        let Some((kind, query, engine)) = st.expr.provider_picker.take() else { return };
        make_default(st, kind, &engine);
        search(&engines(&st.prefs, kind), &engine, &query);
    };
    let footer = row().gap(8.0).child(
        crate::ui::Button::new(t("Cancel"))
            .variant(crate::ui::ButtonVariant::Secondary)
            .on_press(close)
            .el(p),
    ).child(
        crate::ui::Button::new(confirm)
            .key("provider-search")
            .on_press(go)
            .el(p),
    );
    layer.extend(select_layer);
    layer.push(
        Modal::new(title, close)
            .body(body)
            .footer(footer)
            .el(p),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engines_urls_and_validation() {
        let mut prefs = Prefs::default();
        prefs.search_custom.push(("custom_1".into(), "Mine".into(), "https://x.test/s?q={query}".into()));
        prefs.search_enabled.push("custom_1".into());
        let all = engines(&prefs, Kind::Search);
        assert_eq!(
            all.iter().filter(|e| e.enabled).map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["google", "custom_1"]
        );
        assert_eq!(search_url(&all, "github", "a b&c").as_deref(), Some("https://github.com/search?q=a%20b%26c"));
        assert_eq!(search_url(&all, "custom_1", "a b").as_deref(), Some("https://x.test/s?q=a%20b"));
        assert_eq!(validate(" ", "x"), Err("Name is required."));
        assert_eq!(validate("a", "https://x.test/"), Err("URL pattern must contain the {query} placeholder."));
        assert_eq!(validate("a", "not a url {query}"), Err("URL pattern must be a valid URL."));
        let t = engines(&prefs, Kind::Translate);
        assert!(t.iter().any(|e| e.id == "google_translate" && e.enabled));
        assert_eq!(
            search_url(&t, "deepl", "hi there").as_deref(),
            Some("https://www.deepl.com/translator#auto/auto/hi%20there")
        );
    }

    #[test]
    fn default_splices_into_search_engines() {
        let mut se = Vec::new();
        synced::bytes(&mut se, 3, b"deepl");
        let b64 = synced::set("", SEARCH_ENGINES, Some(&se));
        let next = with_default(&b64, 1, Some("bing"));
        let f = synced::fields(&synced::get(&next, SEARCH_ENGINES).unwrap())
            .unwrap()
            .iter()
            .map(|f| (f.num, synced::str_of(f)))
            .collect::<Vec<_>>();
        assert_eq!(f, [(1, "bing".to_string()), (3, "deepl".to_string())]);
        assert_eq!(synced::get(&with_default(&next, 1, None), SEARCH_ENGINES).as_deref(), Some(&se[..]));
    }
}
