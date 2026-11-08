//! The Media tab (`MemesPicker`): saved media from `/users/@me/memes`, filtered and searched,
//! sent with `favorite_meme_id`.

use rune::{El, col, icon, row, text};
use serde_json::Value;

use super::{For, Fetched, ShellAccess, ShellState, close, search_bar};
use crate::icons;
use crate::shell::Intent;
use crate::theme::{Palette, font};
use crate::i18n::t;

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
    let field = search_bar(st, t("Search saved media").into(), p, acc, move |s: &mut S, k, _| {
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
                col().w_full().h_full().key("empty").child(super::empty_state(t("No saved media"), None, p))
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
                        let (id, name) = (m["id"].as_str().unwrap_or_default().to_string(), m["name"].as_str().unwrap_or_default().to_string());
                        let key = format!("meme-{id}");
                        // `MemeGridItem`'s hover Edit and Delete.
                        let m2 = m.clone();
                        let tools = hover_tools(
                            vec![
                                tool(&icons::PENCIL_SIMPLE_FILL, t("Edit saved media"), 18.0, Tool::Plain, p, move |s: &mut S, _: &mut rune::Tx| {
                                    let st = acc(s);
                                    close(st);
                                    crate::social::open_edit_meme(st, &m2);
                                }),
                                tool(&icons::TRASH_FILL, t("Delete saved media"), 18.0, Tool::Danger, p, move |s: &mut S, _: &mut rune::Tx| acc(s).expr.confirm_meme = Some((id.clone(), name.clone()))),
                            ],
                            p,
                        );
                        let e = rune::el()
                            .size(w, h)
                            .radius(6.0)
                            .clip_rounded(6.0)
                            .cursor(rune::Cursor::Pointer)
                            .role("button")
                            .label(m["name"].as_str().unwrap_or("Media").to_string())
                            .hover_group()
                            .key(key)
                            .child(super::gif::image(st, shown, w, h, p))
                            .child(tools)
                            .on_press(move |s: &mut S| pick(acc(s), &m));
                        (e, h)
                    })
                    .collect();
                super::gif::masonry::<S>(cols, w, tiles, p).key("grid")
            }
        }
        Some(Fetched::Failed(_)) => col().w_full().h_full().key("failed").child(super::empty_state("Couldn't load saved media", None, p)),
        _ => col().w_full().h_full().center().key("loading").child(text(t("Loading…")).font(font(14.0, 400.0)).color(p.text_primary_muted)),
    };
    (bar, body)
}

/// `FAVORITE_MEME_CREATE` / `_UPDATE` (`Some`) and `_DELETE` (`None`) on the loaded list.
pub fn on_meme(st: &mut ShellState, id: &str, meme: Option<Value>) {
    let Some(Fetched::Done(Value::Array(list))) = st.expr.fetched.get_mut("memes") else { return };
    let at = list.iter().position(|m| m["id"].as_str() == Some(id));
    match (at, meme) {
        (Some(i), Some(m)) => list[i] = m,
        // Newest first, as `/users/@me/memes` lists them.
        (None, Some(m)) => list.insert(0, m),
        (Some(i), None) => drop(list.remove(i)),
        (None, None) => {}
    }
}

/// How an [`tool`] button looks: `.actionButton`, `.favoriteButtonActive`, `.deleteButton`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Plain,
    Active,
    Danger,
}

/// One `ActionToolbar` item (28px, radius 6) in a media hover toolbar.
pub fn tool<S: 'static>(glyph: &'static rune::Icon, label: &str, size: f32, kind: Tool, p: &Palette, f: impl Fn(&mut S, &mut rune::Tx) + 'static) -> El {
    let fg = match kind {
        Tool::Plain => p.text_tertiary,
        Tool::Active => p.text_primary,
        Tool::Danger => crate::ui::mix(p.status_danger, p.text_primary, 78.0),
    };
    let b = row().size(28.0, 28.0).center().radius(6.0).cursor(rune::Cursor::Pointer).role("button").label(label.to_string()).child(icon(glyph, size).color(fg));
    match kind {
        Tool::Plain => b.hover(p.surface_interactive_hover_bg),
        Tool::Active => b.fill(p.surface_interactive_selected_bg),
        Tool::Danger => b.hover(crate::ui::mix(p.status_danger, p.surface_interactive_hover_bg, 6.0)),
    }
    .on_press_tx(f)
}

/// `.mediaHoverAction`: the toolbar 8px in from the top-right corner, shown while the
/// enclosing `hover_group` is hovered.
pub fn hover_tools(items: Vec<El>, p: &Palette) -> El {
    let bg = p.form_surface_background;
    row()
        .pin(None, Some(8.0), Some(8.0), None)
        .gap(2.0)
        .padding(3.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, crate::ui::over(p.background_modifier_accent, bg))
        .box_shadow(rune::Shadow { x: 0.0, y: 6.0, blur: 14.0, spread: 0.0, color: rune::Color { a: if crate::ui::is_light(p) { 0.1 } else { 0.18 }, ..rune::Color::BLACK }, inset: false })
        .children(items)
        .group_opacity(0.0, 1.0)
}

/// `buildDeleteMemeConfirmation`: deletes on confirm and drops the tile at once.
pub fn confirm_layer<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    use crate::ui::{Button, ButtonVariant, Modal};
    let Some((id, name)) = st.expr.confirm_meme.clone() else { return };
    let close = move |s: &mut S| acc(s).expr.confirm_meme = None;
    let body = text(crate::i18n::tf("Delete \"{memeName}\"? Can't be undone.", &[("memeName", &name)])).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap();
    let delete = Button::new(t("Delete")).variant(ButtonVariant::Danger).on_press(move |s: &mut S| {
        let st = acc(s);
        st.expr.confirm_meme = None;
        if let Some(Fetched::Done(Value::Array(list))) = st.expr.fetched.get_mut("memes") {
            list.retain(|m| m["id"].as_str() != Some(id.as_str()));
        }
        super::send(st, reqwest::Method::DELETE, format!("/users/@me/memes/{id}"), None);
        crate::desktop::toast(&mut st.desktop, "Removed from saved media", true);
    });
    let footer = row().gap(8.0).child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p)).child(delete.el(p));
    layer.push(Modal::new(t("Delete saved media"), close).body(body).footer(footer).el(p));
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
