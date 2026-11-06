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
    let url = f["proxy_src"].as_str().or(f["src"].as_str()).filter(|s| !s.is_empty())?;
    Some((url.to_string(), f["width"].as_f64().unwrap_or(200.0) as f32, f["height"].as_f64().unwrap_or(200.0) as f32))
}

/// Sends the GIF (and any reply in progress), then tells the provider.
fn pick(st: &mut ShellState, g: &Value) {
    let Some(For::Composer(channel)) = st.expr.target else { return };
    let Some(url) = g["url"].as_str().filter(|u| !u.is_empty()) else { return };
    let reply = st.reply.take().filter(|r| r.channel == channel).map(|r| (r.message, r.mention));
    st.intents.push(Intent::SendMessage { channel, content: url.to_string(), reply, attachments: vec![] });
    if let Some(id) = g["id"].as_str() {
        let q = if let GifView::Search(q) = &st.expr.gif { q.clone() } else { String::new() };
        super::send(st, reqwest::Method::POST, "/gifs/register-share".into(), Some(serde_json::json!({ "id": id, "q": q, "locale": "en-US" })));
    }
    close(st);
}

fn toggle_favorite(st: &mut ShellState, g: &Value) {
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
                let mut v = vec![Tile::Page("Favorites".into(), fav, GifView::Favorites), Tile::Page("Trending GIFs".into(), trend, GifView::Trending)];
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
                        .children(matches!(view, GifView::Trending).then(|| icon(&icons::TREND_UP, 18.0).color(Color::WHITE)))
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
            let fav = st.expr.saved.favorite_gifs.iter().any(|f| f["url"] == g["url"]);
            let (g1, g2) = (g.clone(), g.clone());
            let star = row()
                .pin(None, Some(6.0), Some(6.0), None)
                .size(28.0, 28.0)
                .center()
                .radius(6.0)
                .fill(Color { a: 0.6, ..Color::BLACK })
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(if fav { "Remove from favorites" } else { "Add to favorites" })
                .child(icon(if fav { &icons::STAR_FILL } else { &icons::STAR }, 18.0).color(if fav { p.status_idle } else { Color::WHITE }))
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
    col().w_full().h_full().pt(12.0).fill(p.background_tertiary).child(scroller(col().w_full().h_full().scroll(rune::ScrollSpec { stick_bottom: false }).child(grid), false, p))
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
        GifView::Trending => "Trending GIFs",
        GifView::Favorites => "Favorite GIFs",
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
                    .label("Back to GIF categories")
                    .child(icon(&icons::ARROW_LEFT, 20.0).color(p.text_primary))
                    .on_press(move |s: &mut S| acc(s).expr.gif = GifView::Categories),
            )
            .child(text(title).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).truncate())
    };
    let (cols, w) = columns();
    let body = match tiles(st) {
        Ok(v) if v.is_empty() => {
            let line = if st.expr.gif == GifView::Favorites { "No favorite GIFs yet" } else { "No GIFs match that search" };
            col().w_full().h_full().child(super::empty_state(line, None, p))
        }
        Ok(v) => masonry(cols, w, v.iter().map(|t| tile(st, t, w, p, acc)).collect(), p),
        Err("") => col().w_full().h_full().center().child(text("Loading…").font(font(14.0, 400.0)).color(p.text_primary_muted)),
        Err(msg) => col().w_full().h_full().child(super::empty_state(msg, None, p)),
    };
    (bar, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_prefers_decodable_formats() {
        let g = serde_json::json!({"url":"u","proxy_src":"big.webm","width":1,"height":1,"media":{
            "webm":{"proxy_src":"x.webm","width":1,"height":1},"tinygif":{"proxy_src":"t.gif","width":220,"height":110}}});
        assert_eq!(preview(&g), Some(("t.gif".into(), 220.0, 110.0)));
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
