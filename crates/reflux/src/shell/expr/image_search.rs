//! Reverse image search (`ReverseImageSearchMenuItems`, `SearchProviderPickerModal` in image
//! mode): Fluxer's built-in engines, Google Lens enabled; the first search asks which provider
//! to use and remembers it.
// shortcut: the default lives in local prefs, not the synced `searchEngines` setting, and the
// Search engines settings (enable more, add custom ones) aren't ported; upgrade with that tab.

use rune::{El, col, row, text};

use super::{ShellAccess, ShellState};
use crate::i18n::t;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, MenuEntry, MenuItem, Modal, Select, popup_access};

/// `ReverseImageSearch.builtIns`: (id, name, URL template).
const ENGINES: [(&str, &str, &str); 6] = [
    ("google_lens", "Google Lens", "https://lens.google.com/uploadbyurl?url={url}"),
    ("yandex", "Yandex", "https://yandex.com/images/search?rpt=imageview&url={url}"),
    ("bing", "Bing", "https://www.bing.com/images/searchbyimage?cbir=sbi&imgurl={url}"),
    ("tineye", "TinEye", "https://tineye.com/search?url={url}"),
    ("saucenao", "SauceNAO", "https://saucenao.com/search.php?url={url}"),
    ("iqdb", "IQDB", "https://iqdb.org/?url={url}"),
];

/// `buildSearchUrl`: the template with the image URL percent-encoded.
fn search_url(engine: &str, image: &str) -> Option<String> {
    ENGINES.iter().find(|e| e.0 == engine).map(|e| e.2.replace("{url}", &super::enc(image)))
}

fn search(engine: &str, image: &str) {
    if let Some(u) = search_url(engine, image) {
        crate::settings::privacy::open_url(&u);
    }
}

/// The enabled engines: the suggested default plus the one picked (`setDefaultEngine` enables it).
fn enabled(default: Option<&str>) -> Vec<(&'static str, &'static str)> {
    ENGINES.iter().filter(|e| e.0 == "google_lens" || Some(e.0) == default).map(|e| (e.0, e.1)).collect()
}

/// `SearchProviderContextMenuItems`: one "Reverse image search" item, or a submenu of the default
/// (hinted "Default") and the other enabled engines once a default is picked.
pub fn entries<S: 'static>(st: &ShellState, image: &str, acc: ShellAccess<S>, close: impl Fn(&mut S) + Clone + 'static) -> Vec<MenuEntry<S>> {
    let default = st.prefs.reverse_image_engine.clone().filter(|d| ENGINES.iter().any(|e| e.0 == d));
    let label = t("Reverse image search");
    let alternates: Vec<_> = enabled(default.as_deref()).into_iter().filter(|e| Some(e.0) != default.as_deref()).collect();
    let item = |id: &'static str, name: &str, image: String| {
        let close = close.clone();
        MenuItem::new(format!("ris-{id}"), name.to_string()).on_select(move |s: &mut S| {
            search(id, &image);
            close(s);
        })
    };
    match default {
        Some(d) if !alternates.is_empty() => {
            let name = ENGINES.iter().find(|e| e.0 == d).map_or("", |e| e.1);
            let id = ENGINES.iter().find(|e| e.0 == d).map_or("", |e| e.0);
            let mut sub = vec![MenuEntry::Item(item(id, name, image.to_string()).hint(t("Default")))];
            sub.extend(alternates.into_iter().map(|(id, name)| MenuEntry::Item(item(id, name, image.to_string()))));
            vec![MenuEntry::Submenu("ris".into(), label.into(), sub)]
        }
        _ => {
            let image = image.to_string();
            vec![MenuEntry::Item(MenuItem::new("ris", label).icon(&crate::icons::MAGNIFYING_GLASS).on_select(move |s: &mut S| {
                close(s);
                let st = acc(s);
                match st.prefs.reverse_image_engine.clone().filter(|d| ENGINES.iter().any(|e| e.0 == d)) {
                    Some(d) => search(&d, &image),
                    None => st.expr.image_search = Some((image.clone(), "google_lens".into())),
                }
            }))]
        }
    }
}

/// `SearchProviderPickerModal` (image mode): every engine in a select; Search remembers the pick
/// as the default and searches with it.
pub fn layer<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some((_, sel)) = st.expr.image_search.clone() else { return };
    let close = move |s: &mut S| acc(s).expr.image_search = None;
    let options: Vec<(&str, &str)> = ENGINES.iter().map(|e| (e.0, e.1)).collect();
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
        let Some((image, engine)) = st.expr.image_search.take() else { return };
        st.prefs.reverse_image_engine = Some(engine.clone());
        let prefs = Box::new(st.prefs.clone());
        st.intents.push(super::Intent::Prefs(prefs));
        search(&engine, &image);
    };
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
        .child(Button::new(t("Search")).key("ris-go").on_press(go).el(p));
    layer.push(Modal::new(t("Pick a reverse image search provider"), close).body(body).footer(footer).el(p));
    layer.extend(select_layer);
}

#[cfg(test)]
mod tests {
    #[test]
    fn urls_and_enabled() {
        assert_eq!(super::search_url("tineye", "https://a.b/c.png?x=1").as_deref(), Some("https://tineye.com/search?url=https%3A%2F%2Fa.b%2Fc.png%3Fx%3D1"));
        assert_eq!(super::search_url("nope", "x"), None);
        assert_eq!(super::enabled(None).len(), 1);
        assert_eq!(super::enabled(Some("bing")).iter().map(|e| e.0).collect::<Vec<_>>(), ["google_lens", "bing"]);
    }
}
