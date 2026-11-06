//! The media viewer (`messaging/components/modals/MediaModal.tsx`, `media_modal/*`): the
//! picture fitted to the window over a dimmed, blurred backdrop; click or the buttons to zoom
//! (fit ↔ 250%, steps of 125% up to 500%), drag or wheel to pan, previous/next and a thumbnail
//! strip across the message's media, and the HUD (close, open in browser, copy link, copy
//! image, download, reset, zoom, reply) with the name, size and zoom on the right.

use reflux_model::Snowflake;
use rune::{Color, El, Icon, KeyEvent, PlayerState, Tx, col, el, icon, row, text};

use super::{ReplyTo, ShellAccess, ShellState};
use crate::icons;
use crate::media::Load;
use crate::theme::{Palette, font};
use crate::ui::rgba;

/// One picture or video the viewer can show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    /// The original URL (open in browser, copy link).
    pub url: String,
    /// The media proxy URL the pictures are fetched from.
    pub proxy: String,
    pub w: u32,
    pub h: u32,
    pub name: String,
    pub size: u64,
    pub alt: Option<String>,
    pub video: bool,
    pub animated: bool,
}

/// `PanZoomMath`: fitted is 1×; a click zooms to 2.5×; buttons step by 1.25× up to 5×.
const DEFAULT_ZOOM: f32 = 2.5;
const MAX_ZOOM: f32 = 5.0;
const STEP: f32 = 1.25;

pub struct Viewer {
    pub items: Vec<Item>,
    pub index: usize,
    pub zoom: f32,
    pub pan: (f32, f32),
    /// The pan when the current drag started.
    drag_from: (f32, f32),
    /// The message the media belongs to (reply), as (channel, message).
    pub message: Option<(Snowflake, Snowflake)>,
}

/// Opens the viewer on `items[index]`.
pub fn open(st: &mut ShellState, items: Vec<Item>, index: usize, message: Option<(Snowflake, Snowflake)>) {
    if items.is_empty() {
        return;
    }
    // A clip playing inline gives way to the viewer.
    if let Some((_, c)) = st.media_ui.playing.take() {
        c.pause();
    }
    let index = index.min(items.len() - 1);
    st.popups.close();
    st.media_ui.viewer = Some(Viewer { items, index, zoom: 1.0, pan: (0.0, 0.0), drag_from: (0.0, 0.0), message });
}

fn close(st: &mut ShellState) {
    st.media_ui.viewer = None;
    if st.media_ui.gst.as_ref().is_some_and(|c| c.key.starts_with("viewer-")) {
        st.media_ui.gst = None;
    }
    if st.media_ui.playing.as_ref().is_some_and(|(k, _)| k.starts_with("viewer-")) {
        st.media_ui.playing = None;
    }
}

fn go(st: &mut ShellState, delta: isize) {
    if st.media_ui.gst.as_ref().is_some_and(|c| c.key.starts_with("viewer-")) {
        st.media_ui.gst = None;
    }
    if st.media_ui.playing.as_ref().is_some_and(|(k, _)| k.starts_with("viewer-")) {
        st.media_ui.playing = None;
    }
    if let Some(v) = &mut st.media_ui.viewer {
        let n = v.items.len() as isize;
        v.index = (v.index as isize + delta).rem_euclid(n) as usize;
        (v.zoom, v.pan) = (1.0, (0.0, 0.0));
    }
}

fn zoom_to(v: &mut Viewer, z: f32) {
    v.zoom = z.clamp(1.0, MAX_ZOOM);
    if v.zoom <= 1.0 + 0.015 {
        (v.zoom, v.pan) = (1.0, (0.0, 0.0));
    }
}

/// Window keys while the viewer is open: Escape closes, the arrows step through the media.
pub fn on_key(st: &mut ShellState, k: &KeyEvent) -> bool {
    use rune::Key;
    if st.media_ui.viewer.is_none() {
        return false;
    }
    match k.key {
        Key::Escape => close(st),
        Key::Left => go(st, -1),
        Key::Right => go(st, 1),
        _ => return false,
    }
    true
}

/// A HUD button (`.controlButton`): 38×38, radius 12, white 20px icon.
fn control<S: 'static>(glyph: &'static Icon, label: &str, enabled: bool, f: impl Fn(&mut S, &mut Tx) + 'static) -> El {
    let b = row().size(38.0, 38.0).center().radius(12.0).role("button").label(label.to_owned()).child(icon(glyph, 20.0).color(Color::WHITE));
    if !enabled {
        return b.opacity(0.38).disabled(true);
    }
    b.hover(rgba(255, 255, 255, 0.14)).cursor(rune::Cursor::Pointer).on_press_tx(f)
}

/// `.floatingNavButton`: 48×48 frosted pill at the window's side.
fn nav<S: 'static>(left: bool, acc: ShellAccess<S>) -> El {
    let glyph = if left { &icons::CARET_LEFT_BOLD } else { &icons::CARET_RIGHT_BOLD };
    let b = row()
        .size(48.0, 48.0)
        .center()
        .radius(18.0)
        .fill(rgba(0, 0, 0, 0.58))
        .border(1.0, rgba(255, 255, 255, 0.11))
        .backdrop_blur(36.0)
        .hover(rgba(255, 255, 255, 0.13))
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(if left { "Previous attachment" } else { "Next attachment" })
        .child(icon(glyph, 24.0).color(rgba(255, 255, 255, 0.72)))
        .on_press(move |s: &mut S| go(acc(s), if left { -1 } else { 1 }));
    let side = if left { (Some(6.0), None) } else { (None, Some(6.0)) };
    col().pin(side.0, Some(0.0), side.1, Some(0.0)).center().child(b)
}

/// The proxy URL to show an item at (its natural size, at most 4096 device px).
fn full_url(it: &Item, scale: f32) -> String {
    let w = (it.w.max(1) as f32).min(4096.0) / scale.max(1.0);
    if it.animated { crate::media::fit_url_animated(&it.proxy, w, scale) } else { crate::media::fit_url(&it.proxy, w, scale) }
}

/// The whole viewer layer, when open.
pub fn view<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let v = st.media_ui.viewer.as_ref()?;
    let it = v.items.get(v.index)?;
    let n = v.items.len();
    let ratio = it.w.max(1) as f32 / it.h.max(1) as f32;
    let fill = || el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0));
    let key = format!("viewer-{}", v.index);
    let clip = st.media_ui.playing.as_ref().filter(|(k, _)| *k == key).map(|(_, c)| c.clone());

    // The picture: fitted inside the HUD insets, never past its natural size.
    let gst = super::clip::get(st, &key);
    let media = match (&clip, it.video) {
        (None, true) if gst.is_some_and(|c| !c.failed) => {
            let k = key.clone();
            el().aspect_fit(ratio).max_w(it.w as f32).max_h(it.h as f32).pixels_fit(&gst.unwrap().pixels, rune::SurfaceFit::Fit).cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| super::clip::toggle(acc, &k, s))
        }
        (Some(c), true) if !matches!(c.state(), PlayerState::Failed(_)) => el().aspect_fit(ratio).max_w(it.w as f32).max_h(it.h as f32).video(c, rune::SurfaceFit::Fit),
        _ => {
            let pic = el().aspect_fit(ratio).max_w(it.w.max(1) as f32).max_h(it.h.max(1) as f32).label(it.alt.clone().unwrap_or_else(|| it.name.clone()));
            let pic = match st.media.get(&full_url(it, st.media.scale())) {
                Load::Ready(px) => pic.pixels_fit(&px, rune::SurfaceFit::Fit),
                Load::Pending => pic.child(fill().center().child(crate::ui::spinner(Color::WHITE))),
                Load::Failed => pic.fill(p.background_secondary_alt).child(fill().center().child(icon(&icons::IMAGE, 48.0).color(p.text_tertiary_muted))),
            };
            if it.video {
                let (k, url, failed) = (key.clone(), it.url.clone(), clip.as_ref().map(|c| c.state()));
                let msg = if let Some(PlayerState::Failed(e)) = failed { Some(e) } else { None };
                let mut over = col().gap(12.0).items_center();
                over = over.child(
                    row()
                        .size(56.0, 56.0)
                        .center()
                        .radius(28.0)
                        .fill(rgba(0, 0, 0, 0.75))
                        .cursor(rune::Cursor::Pointer)
                        .child(icon(&icons::PLAY_FILL, 28.0).color(Color::WHITE))
                        .on_press(move |s: &mut S| super::clip::start(acc(s), k.clone(), url.clone(), false)),
                );
                if let Some(e) = msg {
                    over = over.child(text(e).font(font(13.0, 500.0).lh(18.0)).color(Color::WHITE));
                }
                pic.child(fill().center().child(over))
            } else {
                pic.cursor(rune::Cursor::Pointer)
            }
        }
    };
    let zoomable = !it.video;
    let media = media
        .scale(v.zoom)
        .offset(v.pan.0, v.pan.1)
        .on_press(move |s: &mut S| {
            if let Some(v) = acc(s).media_ui.viewer.as_mut().filter(|_| zoomable) {
                let z = if v.zoom > 1.0 { 1.0 } else { DEFAULT_ZOOM };
                zoom_to(v, z);
            }
        })
        .on_drag(move |s: &mut S, d: &mut rune::Drag, _| {
            let Some(v) = acc(s).media_ui.viewer.as_mut().filter(|v| v.zoom > 1.0) else { return };
            if d.phase == rune::Phase::Down {
                v.drag_from = v.pan;
            }
            v.pan = (v.drag_from.0 + d.dx, v.drag_from.1 + d.dy);
        });
    // `--media-fit-safe-inset-block` (1rem + 3rem bars + 0.75rem gap) and 1rem at the sides.
    let area = col()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .py(78.0)
        .px(16.0)
        .center()
        .on_press(move |s: &mut S| close(acc(s)))
        .on_wheel(move |s: &mut S, e: rune::WheelEvent, _| {
            let Some(v) = acc(s).media_ui.viewer.as_mut().filter(|v| v.zoom > 1.0) else { return false };
            v.pan = (v.pan.0 + e.dx, v.pan.1 + e.dy);
            true
        })
        .on_magnify(move |s: &mut S, e: rune::MagnifyEvent, _| {
            let Some(v) = acc(s).media_ui.viewer.as_mut().filter(|_| zoomable) else { return false };
            let z = v.zoom * (1.0 + e.delta);
            zoom_to(v, z);
            true
        })
        .child(media);

    // HUD: actions bottom-left, details bottom-right.
    let (url, proxy, name) = (it.url.clone(), it.proxy.clone(), it.name.clone());
    let z = v.zoom;
    let gap = |w: f32| el().w(w);
    let mut actions = row()
        .pin(Some(16.0), None, None, Some(18.0))
        .gap(2.0)
        .center_y()
        .child(control(&icons::X_BOLD, "Close", true, move |s: &mut S, _| close(acc(s))))
        .child(gap(6.0));
    let u1 = url.clone();
    actions = actions.child(control(&icons::ARROW_SQUARE_OUT_BOLD, "Open in browser", !url.is_empty(), move |s: &mut S, tx| super::links::open(acc(s), &u1, tx)));
    let u2 = url.clone();
    actions = actions.child(control(&icons::LINK_BOLD, "Copy link", !url.is_empty(), move |_: &mut S, tx| tx.copy(u2.clone())));
    if !it.video {
        let pr = proxy.clone();
        actions = actions.child(control(&icons::COPY_SIMPLE_BOLD, "Copy image", true, move |s: &mut S, _| acc(s).media.copy_image(format!("{pr}{}format=png", if pr.contains('?') { '&' } else { '?' }))));
    }
    let (pr, nm) = (if proxy.is_empty() { url.clone() } else { proxy.clone() }, name.clone());
    actions = actions.child(control(&icons::DOWNLOAD_SIMPLE_BOLD, "Download", !pr.is_empty(), move |_: &mut S, tx| super::attachments::save_as(acc, pr.clone(), nm.clone(), tx)));
    if zoomable {
        actions = actions
            .child(gap(6.0))
            .child(control(&icons::ARROWS_IN_SIMPLE_BOLD, "Reset", z > 1.0, move |s: &mut S, _| {
                if let Some(v) = &mut acc(s).media_ui.viewer {
                    zoom_to(v, 1.0)
                }
            }))
            .child(control(&icons::MAGNIFYING_GLASS_PLUS_BOLD, "Zoom in", z * STEP <= MAX_ZOOM + 0.01, move |s: &mut S, _| {
                if let Some(v) = &mut acc(s).media_ui.viewer {
                    let z = v.zoom * STEP;
                    zoom_to(v, z)
                }
            }))
            .child(control(&icons::MAGNIFYING_GLASS_MINUS_BOLD, "Zoom out", z > 1.0, move |s: &mut S, _| {
                if let Some(v) = &mut acc(s).media_ui.viewer {
                    let z = v.zoom / STEP;
                    zoom_to(v, z)
                }
            }));
    }
    if let Some((channel, message)) = v.message {
        actions = actions.child(gap(16.0)).child(control(&icons::ARROW_BEND_UP_LEFT_BOLD, "Reply", true, move |s: &mut S, _| {
            let st = acc(s);
            close(st);
            st.reply = Some(ReplyTo { channel, message, mention: true });
            st.composer.focused = true;
        }));
    }
    let mut info: Vec<String> = vec![name];
    if it.w > 0 && it.h > 0 {
        info.push(format!("{}×{}", it.w, it.h));
    }
    if it.size > 0 {
        info.push(super::upload::format_size(it.size));
    }
    if zoomable {
        info.push(format!("{}%", (z * 100.0).round()));
    }
    let details = row()
        .pin(None, None, Some(16.0), Some(18.0))
        .max_w(760.0)
        .gap(18.0)
        .center_y()
        .children(info.into_iter().enumerate().map(|(i, s)| {
            let t = text(s).font(font(13.0, 400.0).lh(20.0)).color(Color::WHITE).truncate();
            if i == 0 { t.max_w(360.0).shrink() } else { t }
        }));

    let mut root = fill()
        .child(fill().fill(rgba(0, 0, 0, 0.78)).backdrop_blur(5.0))
        .child(area)
        .child(actions)
        .child(details);
    if n > 1 {
        root = root.child(nav(true, acc)).child(nav(false, acc)).child(thumbnails(st, v, p, acc));
    }
    Some(root.modal(true, move |s: &mut S| close(acc(s))).role("dialog").label("Media viewer"))
}

/// `.thumbnailCarousel`: 44px tiles across the top, the current one lit.
fn thumbnails<S: 'static>(st: &ShellState, v: &Viewer, p: &Palette, acc: ShellAccess<S>) -> El {
    let n = v.items.len();
    let tiles = v.items.iter().enumerate().map(|(i, it)| {
        let r = 6.0;
        let corners = [if i == 0 { r } else { 0.0 }, if i == n - 1 { r } else { 0.0 }, if i == n - 1 { r } else { 0.0 }, if i == 0 { r } else { 0.0 }];
        let load = st.media.get(&crate::media::fit_url(&it.proxy, 44.0, st.media.scale()));
        let mut t = el().size(44.0, 44.0).corners(corners).clip_rounded(0.0).fill(p.background_secondary);
        if let Load::Ready(px) = load {
            t = t.pixels_fit(&px, rune::SurfaceFit::Fill);
        }
        if i != v.index {
            t = t.opacity(0.55).child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(rgba(0, 0, 0, 0.3)));
        }
        t.cursor(rune::Cursor::Pointer).role("button").on_press(move |s: &mut S| {
            let st = acc(s);
            let d = i as isize - st.media_ui.viewer.as_ref().map_or(0, |v| v.index as isize);
            go(st, d);
        })
    });
    row().pin(Some(0.0), Some(16.0), Some(0.0), None).justify_center().child(row().py(5.6).px(6.4).child(row().gap(1.0).padding(2.0).children(tiles)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_steps_wrap_and_zoom_clamps() {
        let mut st = ShellState::default();
        let it = |n: &str| Item { name: n.into(), w: 10, h: 10, ..Default::default() };
        open(&mut st, vec![it("a"), it("b"), it("c")], 5, None);
        assert_eq!(st.media_ui.viewer.as_ref().map(|v| v.index), Some(2), "index clamps");
        go(&mut st, 1);
        assert_eq!(st.media_ui.viewer.as_ref().map(|v| v.index), Some(0), "wraps");
        let v = st.media_ui.viewer.as_mut().unwrap();
        zoom_to(v, 9.0);
        assert_eq!(v.zoom, MAX_ZOOM);
        v.pan = (5.0, 5.0);
        zoom_to(v, 1.01);
        assert_eq!((v.zoom, v.pan), (1.0, (0.0, 0.0)), "back to fit resets the pan");
        assert!(on_key(&mut st, &KeyEvent { key: rune::Key::Escape, mods: Default::default() }));
        assert!(st.media_ui.viewer.is_none());
        assert!(!on_key(&mut st, &KeyEvent { key: rune::Key::Escape, mods: Default::default() }));
    }
}
