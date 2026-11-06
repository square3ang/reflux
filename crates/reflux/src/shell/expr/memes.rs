//! The Media tab (`MemesPicker`): saved media from `/users/@me/memes`, filtered and searched,
//! sent with `favorite_meme_id`.

use rune::{El, col, row, text};
use serde_json::Value;

use super::{For, Fetched, ShellAccess, ShellState, close, search_bar};
use crate::shell::Intent;
use crate::theme::{Palette, font};

const FILTERS: [&str; 3] = ["All", "Images", "Videos"];

fn video(m: &Value) -> bool {
    m["content_type"].as_str().is_some_and(|c| c.starts_with("video/")) || m["is_gifv"].as_bool() == Some(true)
}

/// `useFilteredMemes`: the filter, then the query in the name, alt text or tags.
fn filtered(list: &[Value], filter: u8, q: &str) -> Vec<Value> {
    let q = q.trim().to_lowercase();
    let has = |v: &Value| v.as_str().is_some_and(|s| s.to_lowercase().contains(&q));
    list.iter()
        .filter(|m| match filter {
            1 => !video(m),
            2 => video(m),
            _ => true,
        })
        .filter(|m| q.is_empty() || has(&m["name"]) || has(&m["alt_text"]) || m["tags"].as_array().is_some_and(|t| t.iter().any(has)))
        .cloned()
        .collect()
}

fn pick(st: &mut ShellState, m: &Value) {
    let (Some(For::Composer(channel)), Some(id)) = (st.expr.target, m["id"].as_str().and_then(|i| i.parse().ok())) else { return };
    st.intents.push(Intent::SendExtra { channel, sticker: None, meme: Some(id) });
    close(st);
}

pub fn view<S: 'static>(st: &ShellState, _: For, p: &Palette, acc: ShellAccess<S>) -> (El, El) {
    let field = search_bar(st, "Search saved media".into(), p, acc, move |s: &mut S, k, _| {
        if k.key == rune::Key::Escape {
            close(acc(s));
            return true;
        }
        false
    });
    let filters = row().gap(4.0).children(FILTERS.iter().enumerate().map(|(i, label)| {
        let on = st.expr.meme_filter == i as u8;
        let b = row()
            .px(8.0)
            .py(4.0)
            .radius(6.0)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(*label)
            .child(text(*label).font(font(14.0, 500.0).lh(20.0)).color(if on { p.text_primary } else { p.text_primary_muted }))
            .on_press(move |s: &mut S| acc(s).expr.meme_filter = i as u8);
        if on { b.fill(p.background_modifier_selected) } else { b.hover(p.background_modifier_hover) }
    }));
    let bar = row().w_full().gap(8.0).center_y().child(field).child(filters);
    let body = match st.expr.fetched.get("memes") {
        Some(Fetched::Done(v)) => {
            let list = filtered(v.as_array().map_or(&[][..], Vec::as_slice), st.expr.meme_filter, &st.expr.query.text);
            if list.is_empty() {
                col().w_full().h_full().child(super::empty_state("No saved media", None, p))
            } else {
                let (cols, w) = super::gif::columns();
                let tiles = list
                    .into_iter()
                    .map(|m| {
                        let (url, mw, mh) = super::gif::preview(&m).unwrap_or_else(|| {
                            (m["url"].as_str().unwrap_or_default().to_string(), m["width"].as_f64().unwrap_or(200.0) as f32, m["height"].as_f64().unwrap_or(200.0) as f32)
                        });
                        let h = (w * mh / mw.max(1.0)).round().max(40.0);
                        let shown = (!video(&m) || url.ends_with(".gif") || url.contains(".gif?")).then_some(url.as_str());
                        let e = rune::el()
                            .size(w, h)
                            .radius(6.0)
                            .clip_rounded(6.0)
                            .cursor(rune::Cursor::Pointer)
                            .role("button")
                            .label(m["name"].as_str().unwrap_or("Media").to_string())
                            .child(super::gif::image(st, shown, w, h, p))
                            .on_press(move |s: &mut S| pick(acc(s), &m));
                        (e, h)
                    })
                    .collect();
                super::gif::masonry(cols, w, tiles, p)
            }
        }
        Some(Fetched::Failed(_)) => col().w_full().h_full().child(super::empty_state("Couldn't load saved media", None, p)),
        _ => col().w_full().h_full().center().child(text("Loading…").font(font(14.0, 400.0)).color(p.text_primary_muted)),
    };
    (bar, body)
}

#[cfg(test)]
mod tests {
    #[test]
    fn filters_by_kind_and_query() {
        let list = serde_json::json!([
            {"id":"1","name":"Cat","content_type":"image/png","tags":["meow"]},
            {"id":"2","name":"Dog","content_type":"video/mp4","tags":[]}
        ]);
        let l = list.as_array().unwrap();
        assert_eq!(super::filtered(l, 2, "").len(), 1);
        assert_eq!(super::filtered(l, 0, "MEOW")[0]["id"], "1");
        assert!(super::filtered(l, 1, "dog").is_empty());
    }
}
