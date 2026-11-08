//! The GIFs tab (`GifPicker`): featured categories with Favorites and Trending tiles, then a
//! masonry grid of results. Picking sends the GIF's page URL.

use rune::{Color, El, Tx, col, el, icon, row, text};
use serde_json::Value;

use super::{For, Fetched, GifView, ShellAccess, ShellState, close, fetch, search_bar};
use crate::icons;
use crate::media::Load;
use crate::shell::Intent;
use crate::theme::{Palette, font};
use crate::ui::scroller;
use crate::i18n::t;

/// Search waits this long after the last keystroke.
const DEBOUNCE_MS: u64 = 300;
/// `.grid` gap and the column's minimum width; the grid's inner width.
const GAP: f32 = 12.0;
const MIN_COL: f32 = 227.0;
const INNER_W: f32 = super::WIDTH - 2.0 - 20.0;
const TILE_H: f32 = 96.0;

pub fn enabled(st: &ShellState) -> bool {
    !st.gif_provider.is_empty()
}

/// The typed search, once it settles; an empty field goes back to the categories.
pub fn poll(st: &mut ShellState, tx: &mut Tx) -> bool {
    let q = st.expr.query.text.trim().to_string();
    if q.is_empty() {
        let back = matches!(st.expr.gif, GifView::Search(_));
        if back {
            st.expr.gif = GifView::Categories;
        }
        return back;
    }
    if st.expr.gif == GifView::Search(q.clone()) {
        return false;
    }
    let left = (st.expr.edited_at + DEBOUNCE_MS).saturating_sub(crate::app::now_ms());
    if left > 0 {
        tx.wake_after(std::time::Duration::from_millis(left));
        return false;
    }
    search(st, q);
    true
}

fn search(st: &mut ShellState, q: String) {
    fetch(st, &format!("gif:search:{q}"), &format!("/gifs/search?q={}&locale=en-US", super::enc(&q)));
    st.expr.gif = GifView::Search(q);
}

/// `pickBestPreviewFormat`, decodable formats only (webm/mp4 aren't): the smallest still-animated
/// format first.
pub(super) fn preview(g: &Value) -> Option<(String, f32, f32)> {
    let media = g["media"].as_object();
    let best = ["tinygif", "nanogif", "gif", "webp", "tinywebp"].iter().find_map(|k| media.and_then(|m| m.get(*k)).filter(|f| f["proxy_src"].is_string()));
    let f = best.unwrap_or(g);
    let url = f["proxy_src"].as_str().or(f["proxy_url"].as_str()).or(f["src"].as_str()).filter(|s| !s.is_empty())?;
    Some((url.to_string(), f["width"].as_f64().unwrap_or(200.0) as f32, f["height"].as_f64().unwrap_or(200.0) as f32))
}

/// Sends the GIF (and any reply in progress), then tells the provider. With auto-send off the
/// page URL goes into the composer instead (`useTextareaExpressionHandlers`).
fn pick(st: &mut ShellState, g: &Value) {
    let Some(For::Composer(channel)) = st.expr.target else { return };
    let Some(url) = g["url"].as_str().filter(|u| !u.is_empty()) else { return };
    if !crate::look::with(|l| l.auto_send_gifs) {
        st.composer.insert(&format!("{url} "));
        close(st);
        return;
    }
    let reply = st.reply.take().filter(|r| r.channel == channel).map(|r| (r.message, r.mention));
    st.intents.push(Intent::SendMessage { channel, content: url.to_string(), reply, attachments: vec![] });
    if let Some(id) = g["id"].as_str() {
        let q = if let GifView::Search(q) = &st.expr.gif { q.clone() } else { String::new() };
        super::send(st, reqwest::Method::POST, "/gifs/register-share".into(), Some(serde_json::json!({ "id": id, "q": q, "locale": "en-US" })));
    }
    close(st);
}

/// `FavoriteGifUtils.findFavoriteMemeForGif`: the saved-media entry a GIF was starred as.
fn saved_meme(st: &ShellState, g: &Value) -> Option<Value> {
    let list = match st.expr.fetched.get("memes") {
        Some(Fetched::Done(Value::Array(l))) => l,
        _ => return None,
    };
    let (slug, url) = (g["id"].as_str(), g["url"].as_str());
    list.iter()
        .find(|m| {
            (m["url"].as_str().is_some() && m["url"].as_str() == url)
                || (slug.is_some() && m["gif_slug"].as_str() == slug && m["gif_provider"].as_str() == Some(st.gif_provider.as_str()))
        })
        .cloned()
}

fn toggle_favorite(st: &mut ShellState, g: &Value) {
    if crate::look::with(|l| l.save_gif_favorites) {
        if let Some(id) = saved_meme(st, g).and_then(|m| m["id"].as_str().map(str::to_owned)) {
            if let Some(Fetched::Done(Value::Array(l))) = st.expr.fetched.get_mut("memes") {
                l.retain(|m| m["id"].as_str() != Some(id.as_str()));
            }
            super::send(st, reqwest::Method::DELETE, format!("/users/@me/memes/{id}"), None);
        } else {
            let body = serde_json::json!({
                "url": g["url"],
                "gif_slug": g["id"],
                "gif_provider": st.gif_provider,
                "media": g["media"],
            });
            super::send(st, reqwest::Method::POST, "/users/@me/memes".into(), Some(body));
        }
        return;
    }
    let favs = &mut st.expr.saved.favorite_gifs;
    match favs.iter().position(|f| f["url"] == g["url"]) {
        Some(i) => drop(favs.remove(i)),
        None => favs.insert(0, g.clone()),
    }
    st.expr.save();
}

enum Tile {
    /// A featured page: title, preview, what it opens.
    Page(String, Option<String>, GifView),
    Gif(Value),
}

fn tiles(st: &ShellState) -> Result<Vec<Tile>, &'static str> {
    let got = |k: &str| st.expr.fetched.get(k);
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default().into_iter().map(Tile::Gif).collect();
    match &st.expr.gif {
        GifView::Favorites => Ok(st.expr.saved.favorite_gifs.iter().cloned().map(Tile::Gif).collect()),
        GifView::Categories => match got("gif:featured") {
            Some(Fetched::Done(f)) => {
                let fav = st.expr.saved.favorite_gifs.first().and_then(preview).map(|p| p.0);
                let trend = f["gifs"].get(0).and_then(preview).map(|p| p.0);
                let mut v = vec![Tile::Page(t("Favorites").into(), fav, GifView::Favorites), Tile::Page(t("Trending GIFs").into(), trend, GifView::Trending)];
                for c in f["categories"].as_array().into_iter().flatten() {
                    let name = c["name"].as_str().unwrap_or_default().to_string();
                    let src = c["proxy_src"].as_str().or(c["src"].as_str()).map(str::to_string);
                    v.push(Tile::Page(name.clone(), src, GifView::Search(name)));
                }
                Ok(v)
            }
            Some(Fetched::Failed(_)) => Err("Couldn't load GIFs"),
            _ => Err(""),
        },
        GifView::Trending => match got("gif:trending") {
            Some(Fetched::Done(v)) => Ok(list(v)),
            Some(Fetched::Failed(_)) => Err("Couldn't load GIFs"),
            _ => Err(""),
        },
        GifView::Search(q) => match got(&format!("gif:search:{q}")) {
            Some(Fetched::Done(v)) => Ok(list(v)),
            Some(Fetched::Failed(_)) => Err("Couldn't load GIFs"),
            _ => Err(""),
        },
    }
}

pub(super) fn image(st: &ShellState, url: Option<&str>, w: f32, h: f32, p: &Palette) -> El {
    match url.map(|u| st.media.get(u)) {
        Some(Load::Ready(px)) => el().size(w, h).pixels_fit(&px, rune::SurfaceFit::Fill),
        _ => el().size(w, h).fill(p.background_secondary),
    }
}

fn tile<S: 'static>(st: &ShellState, t: &Tile, w: f32, p: &Palette, acc: ShellAccess<S>) -> (El, f32) {
    match t {
        Tile::Page(title, src, view) => {
            let view = view.clone();
            let e = el()
                .size(w, TILE_H)
                .radius(6.0)
                .clip_rounded(6.0)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(title.clone())
                .hover_group()
                .child(image(st, src.as_deref(), w, TILE_H, p))
                .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(Color { a: 0.68, ..Color::BLACK }))
                .child(
                    row()
                        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                        .center()
                        .gap(6.0)
                        .children(matches!(view, GifView::Favorites).then(|| icon(&icons::STAR_FILL, 18.0).color(Color::WHITE)))
                        .children(matches!(view, GifView::Trending).then(|| icon(&icons::TREND_UP_FILL, 18.0).color(Color::WHITE)))
                        .child(text(title.clone()).font(font(16.0, 700.0).lh(20.0)).color(Color::WHITE).truncate()),
                )
                .on_press(move |s: &mut S| {
                    let st = acc(s);
                    match &view {
                        GifView::Search(q) => {
                            st.expr.query = rune::TextEdit::new(q);
                            search(st, q.clone());
                        }
                        GifView::Trending => {
                            fetch(st, "gif:trending", "/gifs/trending?locale=en-US");
                            st.expr.gif = GifView::Trending;
                        }
                        v => st.expr.gif = v.clone(),
                    }
                });
            (e, TILE_H)
        }
        Tile::Gif(g) => {
            let (url, gw, gh) = preview(g).unwrap_or_default();
            let h = (w * gh / gw.max(1.0)).round().max(40.0);
            let fav = if crate::look::with(|l| l.save_gif_favorites) {
                saved_meme(st, g).is_some()
            } else {
                st.expr.saved.favorite_gifs.iter().any(|f| f["url"] == g["url"])
            };
            let (g1, g2) = (g.clone(), g.clone());
            let star = row()
                .pin(None, Some(6.0), Some(6.0), None)
                .size(28.0, 28.0)
                .center()
                .radius(6.0)
                .fill(Color { a: 0.6, ..Color::BLACK })
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(if fav { crate::i18n::t("Remove from favorites") } else { crate::i18n::t("Add to favorites") })
                .child(icon(if fav { &icons::STAR_FILL } else { &icons::STAR_BOLD }, 18.0).color(if fav { p.status_idle } else { Color::WHITE }))
                .on_press(move |s: &mut S| toggle_favorite(acc(s), &g2));
            let mut e = el()
                .size(w, h)
                .radius(6.0)
                .clip_rounded(6.0)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(g["title"].as_str().filter(|t| !t.is_empty()).unwrap_or("GIF").to_string())
                .hover_group()
                .child(image(st, Some(&url).filter(|u| !u.is_empty()).map(String::as_str), w, h, p))
                .on_press(move |s: &mut S| pick(acc(s), &g1));
            e = e.child(if fav { star } else { star.group_opacity(0.0, 1.0) });
            (e, h)
        }
    }
}

/// How many masonry columns fit, and their width.
pub(super) fn columns() -> (usize, f32) {
    let cols = (((INNER_W + GAP) / (MIN_COL + GAP)).floor() as usize).max(1);
    (cols, ((INNER_W - GAP * (cols - 1) as f32) / cols as f32).floor())
}

/// Each (tile, height) into the shortest column, scrolling.
pub(super) fn masonry(cols: usize, w: f32, tiles: Vec<(El, f32)>, p: &Palette) -> El {
    let mut columns: Vec<(El, f32)> = (0..cols).map(|_| (col().w(w).gap(GAP), 0.0)).collect();
    for (e, h) in tiles {
        let c = columns.iter_mut().min_by(|a, b| a.1.total_cmp(&b.1)).expect("at least one column");
        c.0 = std::mem::replace(&mut c.0, el()).child(e);
        c.1 += h + GAP;
    }
    let grid = row().w_full().items_start().gap(GAP).px(10.0).pb(16.0).children(columns.into_iter().map(|c| c.0));
    col().w_full().h_full().pt(12.0).fill(p.background_tertiary).child(scroller(col().w_full().h_full().scroll(rune::ScrollSpec { stick_bottom: false }).key("gif-grid-scroll").child(grid), false, p))
}

/// (search bar with the back button and title, masonry grid).
pub fn view<S: 'static>(st: &ShellState, _: For, p: &Palette, acc: ShellAccess<S>) -> (El, El) {
    let placeholder = format!("Search {}", st.gif_provider);
    let field = search_bar(st, placeholder, p, acc, move |s: &mut S, k, _| {
        if k.key == rune::Key::Escape {
            close(acc(s));
            return true;
        }
        false
    });
    let title = match &st.expr.gif {
        GifView::Trending => t("Trending GIFs"),
        GifView::Favorites => t("Favorite GIFs"),
        _ => "",
    };
    let bar = if title.is_empty() {
        field
    } else {
        row()
            .w_full()
            .h(36.0)
            .gap(8.0)
            .center_y()
            .child(
                row()
                    .size(32.0, 32.0)
                    .center()
                    .radius(6.0)
                    .hover(p.background_modifier_hover)
                    .cursor(rune::Cursor::Pointer)
                    .role("button")
                    .label(t("Back to GIF categories"))
                    .child(icon(&icons::ARROW_LEFT, 20.0).color(p.text_primary))
                    .on_press(move |s: &mut S| acc(s).expr.gif = GifView::Categories),
            )
            .child(text(title).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).truncate().grow(1.0))
            .children((st.expr.gif == GifView::Favorites).then(|| favorites_actions(st, p, acc)))
    };
    let (cols, w) = columns();
    let body = match tiles(st) {
        Ok(v) if v.is_empty() => {
            let line = if st.expr.gif == GifView::Favorites { t("No favorite GIFs yet") } else { "No GIFs match that search" };
            col().w_full().h_full().child(super::empty_state(line, None, p))
        }
        Ok(v) => masonry(cols, w, v.iter().map(|t| tile(st, t, w, p, acc)).collect(), p),
        Err("") => col().w_full().h_full().center().child(text(t("Loading…")).font(font(14.0, 400.0)).color(p.text_primary_muted)),
        Err(msg) => col().w_full().h_full().child(super::empty_state(msg, None, p)),
    };
    (bar, body)
}

/// `MAX_FAVORITE_GIFS`; `RESOLVE_BATCH_SIZE`.
const MAX_FAVORITES: usize = 10_000;
const BATCH: usize = 100;

/// `ImportFavoriteGifsModal` / `FavoriteGifImport`: the pasted URLs and the import's progress.
#[derive(Default)]
pub struct GifImport {
    pub text: rune::TextEdit,
    /// Resolve requests in flight: (fetch key, URLs asked for).
    pending: Vec<(String, usize)>,
    total: usize,
    processed: usize,
    added: usize,
    skipped: usize,
    done: bool,
}

/// The Favorites header's "Import GIFs" and "Export GIF URLs" buttons.
fn favorites_actions<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let button = |key: &'static str, ic: &'static rune::Icon, label: &'static str, on: bool| {
        let b = row().size(32.0, 32.0).center().radius(6.0).role("button").label(t(label)).key(key).child(icon(ic, 20.0).color(p.text_primary));
        if on { b.hover(p.background_modifier_hover).cursor(rune::Cursor::Pointer) } else { b.opacity(0.5) }
    };
    let urls: Vec<String> = st.expr.saved.favorite_gifs.iter().filter_map(|g| g["url"].as_str()).map(|u| u.trim().to_string()).filter(|u| !u.is_empty()).collect();
    let has = !urls.is_empty();
    let mut export = button("gif-export", &icons::DOWNLOAD_SIMPLE, "Export GIF URLs", has);
    if has {
        export = export.on_press_tx(move |_: &mut S, tx: &mut Tx| {
            let body = format!("{}\n", urls.join("\n"));
            let options = rune::FileDialogOptions { title: "Export GIF URLs".into(), file_name: Some("fluxer-favorite-gifs.txt".into()), ..Default::default() };
            tx.save_file(options, move |s: &mut S, r, _| match r.map(|p| p.map(|p| std::fs::write(p, &body))) {
                Ok(Some(Err(e))) => acc(s).notice = Some(("Export failed".into(), e.to_string())),
                Err(e) => acc(s).notice = Some(("Export failed".into(), e.to_string())),
                _ => {}
            });
        });
    }
    let import = button("gif-import", &icons::UPLOAD_SIMPLE, "Import GIFs", true).on_press(move |s: &mut S| {
        let st = acc(s);
        if st.expr.gif_import.is_none() {
            st.expr.gif_import = Some(GifImport::default());
        }
        close(st);
    });
    row().gap(4.0).child(import).child(export)
}

/// `FavoriteGifImport.startImport`: unique http(s) lines not yet favourited, up to the limit,
/// resolved a hundred at a time.
fn start_import(st: &mut ShellState) {
    let Some(imp) = &mut st.expr.gif_import else { return };
    let mut seen = std::collections::HashSet::new();
    let urls: Vec<String> = imp.text.text.lines().map(str::trim).filter(|l| url::Url::parse(l).is_ok_and(|u| u.scheme().starts_with("http"))).filter(|l| seen.insert(l.to_string())).map(String::from).collect();
    if urls.is_empty() {
        return;
    }
    let have: std::collections::HashSet<&str> = st.expr.saved.favorite_gifs.iter().filter_map(|g| g["url"].as_str()).collect();
    let room = MAX_FAVORITES.saturating_sub(have.len());
    let (fresh, dupes): (Vec<String>, Vec<String>) = urls.iter().cloned().partition(|u| !have.contains(u.as_str()));
    let ask: Vec<String> = fresh.iter().take(room).cloned().collect();
    *imp = GifImport { text: imp.text.clone(), total: urls.len(), processed: urls.len() - ask.len(), skipped: dupes.len() + fresh.len() - ask.len(), ..Default::default() };
    for (i, chunk) in ask.chunks(BATCH).enumerate() {
        let key = format!("gif-import:{}:{i}", crate::app::now_ms());
        imp.pending.push((key.clone(), chunk.len()));
        st.expr.fetched.insert(key.clone(), Fetched::Loading);
        st.intents.push(Intent::Fetch { key, method: reqwest::Method::POST, path: "/users/@me/favorite-gifs/resolve".into(), body: Some(serde_json::json!({ "urls": chunk })) });
    }
    imp.done = imp.pending.is_empty();
}

/// Adds each answered batch's entries (skipping URLs already there); a failed batch is skipped.
pub fn poll_import(st: &mut ShellState) -> bool {
    let Some(imp) = &mut st.expr.gif_import else { return false };
    let mut changed = false;
    let favs = &mut st.expr.saved.favorite_gifs;
    imp.pending.retain(|(key, n)| {
        let entries = match st.expr.fetched.get(key) {
            Some(Fetched::Done(v)) => v["entries"].as_array().cloned().unwrap_or_default(),
            Some(Fetched::Failed(_)) => Vec::new(),
            _ => return true,
        };
        st.expr.fetched.remove(key);
        let mut added = 0;
        for e in entries {
            if !favs.iter().any(|f| f["url"] == e["url"]) {
                favs.push(e);
                added += 1;
            }
        }
        (imp.added, imp.skipped, imp.processed) = (imp.added + added, imp.skipped + n.saturating_sub(added), imp.processed + n);
        changed = true;
        false
    });
    if changed {
        imp.done = imp.pending.is_empty();
        st.expr.save();
    }
    changed
}

/// `ImportFavoriteGifsModal`: paste, then progress, then the summary.
pub fn import_layer<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    use crate::ui::{Button, ButtonVariant, Modal};
    let Some(imp) = &st.expr.gif_import else { return };
    let close = move |s: &mut S| acc(s).expr.gif_import = None;
    let line = |s: String| text(s).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap();
    let n = |v: usize| v.to_string();
    let (body, footer) = if imp.done {
        let msg = if imp.skipped > 0 {
            crate::i18n::tf("Import complete. Added {importedCount, plural, one {# GIF} other {# GIFs}}, skipped {skippedCount, number} (duplicates or over the limit).", &[("importedCount", &n(imp.added)), ("skippedCount", &n(imp.skipped))])
        } else {
            crate::i18n::tf("Import complete. Added {importedCount, plural, one {# GIF} other {# GIFs}}.", &[("importedCount", &n(imp.added))])
        };
        (
            col().w_full().child(line(msg)),
            row()
                .gap(8.0)
                .child(Button::new(t("Import more")).variant(ButtonVariant::Secondary).on_press(move |s: &mut S| acc(s).expr.gif_import = Some(GifImport::default())).el(p))
                .child(Button::new(t("Done")).on_press(close).el(p)),
        )
    } else if !imp.pending.is_empty() {
        let frac = imp.processed as f32 / imp.total.max(1) as f32;
        let msg = crate::i18n::tf("Importing… {processedCount, number} / {totalCount, number} processed, {importedCount, number} added.", &[("processedCount", &n(imp.processed)), ("totalCount", &n(imp.total)), ("importedCount", &n(imp.added))]);
        (
            col()
                .w_full()
                .gap(12.0)
                .child(line(msg))
                .child(el().w_full().h(8.0).radius(4.0).fill(p.background_modifier_accent).child(el().w_frac(frac).h_full().radius(4.0).fill(p.brand_primary)))
                .child(line(t("You can close this modal. The import will continue in the background.").into())),
            row()
                .gap(8.0)
                .child(Button::new(t("Close")).variant(ButtonVariant::Secondary).on_press(close).el(p))
                .child(Button::new(t("Cancel import")).variant(ButtonVariant::Danger).on_press(move |s: &mut S| {
                    if let Some(i) = &mut acc(s).expr.gif_import {
                        (i.pending, i.done) = (Vec::new(), true);
                    }
                }).el(p)),
        )
    } else {
        let full = st.expr.saved.favorite_gifs.len() >= MAX_FAVORITES;
        let mut b = col().w_full().gap(12.0).child(line(t("Paste GIF URLs below, one per line.").into()));
        b = if full {
            b.child(line(t("You're at the favorite GIF limit.").into()))
        } else {
            b.child(crate::ui::Input::new("gif-import-urls", &imp.text, move |s: &mut S| &mut acc(s).expr.gif_import.get_or_insert_with(GifImport::default).text).label(t("GIF URLs")).placeholder("https://example.com/gif1.gif\nhttps://example.com/gif2.gif").multiline(8.0 * 20.0 + 22.0).el(p))
        };
        (
            b,
            row()
                .gap(8.0)
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
                .child(Button::new(t("Import")).disabled(full || imp.text.text.trim().is_empty()).on_press(move |s: &mut S| start_import(acc(s))).el(p)),
        )
    };
    layer.push(Modal::new(t("Import favorite GIFs"), close).body(body).footer(footer).el(p));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_media_matches_a_gif_by_url_or_provider_slug() {
        let mut st = ShellState { gif_provider: "Klipy".into(), ..Default::default() };
        st.expr.fetched.insert("memes".into(), Fetched::Done(serde_json::json!([
            {"id": "1", "url": "https://klipy/one.gif", "gif_provider": "Klipy", "gif_slug": "one"},
            {"id": "2", "url": "https://klipy/two.gif", "gif_provider": "Klipy", "gif_slug": "two"}
        ])));
        assert_eq!(saved_meme(&st, &serde_json::json!({"url": "https://klipy/one.gif"})).unwrap()["id"], "1");
        assert_eq!(saved_meme(&st, &serde_json::json!({"url": "https://klipy/two.gif", "id": "two"})).unwrap()["id"], "2");
        assert!(saved_meme(&st, &serde_json::json!({"url": "https://klipy/three.gif", "id": "three"})).is_none());
    }

    #[test]
    fn preview_prefers_decodable_formats() {
        let g = serde_json::json!({"url":"u","proxy_src":"big.webm","width":1,"height":1,"media":{
            "webm":{"proxy_src":"x.webm","width":1,"height":1},"tinygif":{"proxy_src":"t.gif","width":220,"height":110}}});
        assert_eq!(preview(&g), Some(("t.gif".into(), 220.0, 110.0)));
    }

    #[test]
    fn import_resolves_new_urls_and_counts() {
        let mut st = ShellState::default();
        st.expr.saved.favorite_gifs = vec![serde_json::json!({"url": "https://a.gif"})];
        st.expr.gif_import = Some(GifImport { text: rune::TextEdit::new("https://a.gif\nnot a url\nhttps://b.gif\nhttps://b.gif\nhttps://c.gif"), ..Default::default() });
        start_import(&mut st);
        let key = st.expr.gif_import.as_ref().unwrap().pending[0].0.clone();
        assert!(matches!(&st.intents[0], Intent::Fetch { body: Some(b), .. } if b["urls"] == serde_json::json!(["https://b.gif", "https://c.gif"])));
        st.expr.fetched.insert(key, Fetched::Done(serde_json::json!({"entries": [{"url": "https://b.gif"}]})));
        assert!(poll_import(&mut st));
        let imp = st.expr.gif_import.as_ref().unwrap();
        assert_eq!((imp.added, imp.skipped, imp.processed, imp.total, imp.done), (1, 2, 3, 3, true));
        assert_eq!(st.expr.saved.favorite_gifs.len(), 2);
    }

    #[test]
    fn typed_search_waits_then_fetches() {
        let mut st = ShellState::default();
        st.expr.query = rune::TextEdit::new("cat");
        st.expr.edited_at = crate::app::now_ms();
        let mut tx = Tx::default();
        assert!(!poll(&mut st, &mut tx), "debounced");
        st.expr.edited_at = 0;
        assert!(poll(&mut st, &mut tx));
        assert_eq!(st.expr.gif, GifView::Search("cat".into()));
        assert!(matches!(&st.intents[..], [Intent::Fetch { path, .. }] if path == "/gifs/search?q=cat&locale=en-US"));
    }
}
