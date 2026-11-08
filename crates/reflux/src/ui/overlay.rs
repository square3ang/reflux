//! Popover anchoring, the shared open-popup state, and `<Tooltip>`
//! (`fluxer_app/src/features/ui/tooltip/Tooltip.module.css`).
//!
//! Overlays render in two parts, as rune-kit's do: the trigger stays where the view puts it,
//! and the surface goes into a `layer` the screen mounts at the root with `rune_kit::root`.

use std::f32::consts::SQRT_2;

use rune::{Align, Color, El, Shadow, Shape, Side, col, el, row, text};

use super::{is_light, mix, over, rgba};
use crate::theme::{Palette, font};

/// Which popup is open. One at a time, as in Fluxer: opening one closes the others.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Popups {
    /// The open menu, select or popover, by id.
    pub open: Option<String>,
    /// Where a context menu opened (window points); `None` anchors it to the element keyed `open`.
    pub at: Option<[f32; 2]>,
    /// The open submenu of the open menu.
    pub sub: Option<String>,
    /// The tooltip under the pointer.
    pub tip: Option<String>,
}

impl Popups {
    pub fn is_open(&self, id: &str) -> bool {
        self.open.as_deref() == Some(id)
    }
    pub fn toggle(&mut self, id: &str) {
        let was = self.is_open(id);
        self.close();
        if !was {
            self.open = Some(id.into());
        }
    }
    pub fn open_at(&mut self, id: &str, at: [f32; 2]) {
        self.close();
        self.open = Some(id.into());
        self.at = Some(crate::desktop::window::unzoom(at));
    }
    pub fn close(&mut self) {
        self.open = None;
        self.at = None;
        self.sub = None;
        self.tip = None;
    }
}

/// Reaches the [`Popups`] in app state: a plain `fn`, or a `Copy` closure made with
/// [`popup_access`] (one that captures another accessor).
pub trait PopupAccess<S>: Fn(&mut S) -> &mut Popups + Copy + 'static {}
impl<S, F: Fn(&mut S) -> &mut Popups + Copy + 'static> PopupAccess<S> for F {}

/// Pins a closure's signature to an accessor's (closures returning a borrow of their argument
/// need the expected type spelled out).
pub fn popup_access<S, F: Fn(&mut S) -> &mut Popups + Copy + 'static>(f: F) -> F {
    f
}

/// Floating UI placements.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
    BottomStart,
    BottomEnd,
    RightStart,
    LeftStart,
}

impl Placement {
    fn split(self) -> (Side, Align) {
        match self {
            Placement::Top => (Side::Top, Align::Center),
            Placement::Bottom => (Side::Bottom, Align::Center),
            Placement::Left => (Side::Left, Align::Center),
            Placement::Right => (Side::Right, Align::Center),
            Placement::BottomStart => (Side::Bottom, Align::Start),
            Placement::BottomEnd => (Side::Bottom, Align::End),
            Placement::RightStart => (Side::Right, Align::Start),
            Placement::LeftStart => (Side::Left, Align::Start),
        }
    }
}

thread_local! {
    static LATE: std::cell::RefCell<Vec<El>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Queues an open popup (select list, date picker, menu) for the end of the window's overlays.
/// An anchored overlay must come after its anchor, and above the modal holding it; the
/// callers' own `layer` order can't promise that (a modal builder that adds its selects' lists
/// before the caller pushes the modal itself), so [`super::root`] mounts these last.
pub fn late(e: El) {
    LATE.with(|l| l.borrow_mut().push(e));
}

/// The queued popups, in the order they were opened in the view.
pub(crate) fn take_late() -> Vec<El> {
    LATE.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

/// Places `content` on `placement` of the element keyed `anchor`, `gap` away; it flips when it
/// doesn't fit and stays 8pt inside the window (Floating UI's `flip` + `shift({padding: 8})`).
pub fn anchor(content: El, anchor: &str, placement: Placement, gap: f32) -> El {
    let (side, align) = placement.split();
    content.anchored(anchor, side, align, gap)
}

/// Places `content` with its top-left at a window point (a context menu at the pointer). Pushes
/// a zero-size anchor keyed `{key}-at` into `layer` first.
pub fn anchor_at_point(layer: &mut Vec<El>, content: El, key: &str, at: [f32; 2]) -> El {
    let k = format!("{key}-at");
    layer.push(el().key(k.clone()).absolute(at[0], at[1]).size(0.0, 0.0));
    content.anchored(k, Side::Bottom, Align::Start, 0.0)
}

/// `0 0.5rem 1rem rgb(0 0 0 / a)`, the popout/menu/tooltip elevation (lighter in the light theme).
pub(crate) fn popout_shadow(p: &Palette, dark: f32, light: f32) -> Shadow {
    let a = if is_light(p) { light } else { dark };
    Shadow { x: 0.0, y: 8.0, blur: 16.0, spread: 0.0, color: rgba(0, 0, 0, a), inset: false }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TooltipKind {
    #[default]
    Normal,
    Danger,
}

/// `<Tooltip text position>` wrapped around a trigger. The open state lives in [`Popups::tip`];
/// Fluxer's default delay is 0, so it opens as the pointer enters.
pub struct Tooltip<'a> {
    id: &'a str,
    text: &'a str,
    placement: Placement,
    kind: TooltipKind,
    large: bool,
    /// Draw it regardless of hover (gallery and tests).
    force: bool,
}

impl<'a> Tooltip<'a> {
    pub fn new(id: &'a str, text: &'a str) -> Self {
        Self { id, text, placement: Placement::Top, kind: TooltipKind::Normal, large: false, force: false }
    }
    pub fn placement(mut self, p: Placement) -> Self { self.placement = p; self }
    pub fn danger(mut self) -> Self { self.kind = TooltipKind::Danger; self }
    pub fn large(mut self) -> Self { self.large = true; self }
    pub fn force(mut self, f: bool) -> Self { self.force = f; self }

    /// Wraps `trigger` (keyed `id`) and pushes the tooltip into `layer` while it shows.
    pub fn el<S: 'static>(self, trigger: El, popups: &Popups, access: impl PopupAccess<S>, p: &Palette, layer: &mut Vec<El>) -> El {
        let id = self.id.to_string();
        if self.force || popups.tip.as_deref() == Some(self.id) {
            layer.push(anchor(tooltip_surface(self.text, self.placement, self.kind, self.large, p), self.id, self.placement, 8.0));
        }
        row().key(self.id).child(trigger).on_hover_change(move |s: &mut S, inside, _| {
            let t = &mut access(s).tip;
            if inside {
                *t = Some(id.clone());
            } else if t.as_deref() == Some(id.as_str()) {
                *t = None;
            }
        })
    }
}

/// The tooltip card and its pointer, pointing back at the anchor on `placement`'s side.
pub fn tooltip_surface(label: &str, placement: Placement, kind: TooltipKind, large: bool, p: &Palette) -> El {
    let (surface, border, fg) = match kind {
        TooltipKind::Normal => (p.form_surface_background, p.background_modifier_accent, p.text_primary),
        TooltipKind::Danger => (
            mix(p.status_danger, p.form_surface_background, 8.0),
            mix(p.status_danger, Color::TRANSPARENT, 32.0),
            mix(p.status_danger, p.text_primary, 78.0),
        ),
    };
    let (py, px, style) =
        if large { (10.0, 14.0, font(15.0, 500.0).lh(20.0)) } else { (7.0, 10.0, font(13.0, 500.0).lh(18.0)) };
    tooltip_card(text(label).font(style).color(fg).align(Align::Center).wrap().py(py).px(px), 190.0, placement, surface, border, p)
}

/// A tooltip whose content is built by the caller (`text={() => …}`), `max_w` wide at most
/// (`maxWidth`: 190 default, 350 `xl`); `content` brings `.tooltipContent`'s 7/10 padding.
pub fn tooltip_custom(content: El, max_w: f32, placement: Placement, p: &Palette) -> El {
    tooltip_card(content, max_w, placement, p.form_surface_background, p.background_modifier_accent, p)
}

fn tooltip_card(content: El, max_w: f32, placement: Placement, surface: Color, border: Color, p: &Palette) -> El {
    let edge = over(border, surface);
    // max-width is the border box; Rune's border takes no space, so pad by its 1px.
    col()
        .max_w(max_w)
        .padding(1.0)
        .radius(8.0)
        .fill(surface)
        .border(1.0, edge)
        .box_shadow(popout_shadow(p, 0.22, 0.12))
        .inert()
        .child(content)
        .child(pointer(placement, surface, edge))
}

/// `.tooltipPointerBg`: a 10px square turned 45° and clipped to the half outside the card, so its
/// base covers the card's border and its two outer sides carry it.
fn pointer(placement: Placement, surface: Color, edge: Color) -> El {
    let across = 10.0 * SQRT_2;
    let depth = across / 2.0;
    // The square's centre sits on the card's inner border edge, 1px inside its outer edge.
    let out = -(depth - 1.0);
    let (side, _) = placement.split();
    // Triangle in its own box: base on the card side, apex away from it.
    let (w, h, base, apex): (f32, f32, [[f32; 2]; 2], [f32; 2]) = match side {
        Side::Top => (across, depth, [[0.0, 0.0], [across, 0.0]], [depth, depth]),
        Side::Bottom => (across, depth, [[0.0, depth], [across, depth]], [depth, 0.0]),
        Side::Left => (depth, across, [[0.0, 0.0], [0.0, across]], [depth, depth]),
        Side::Right => (depth, across, [[depth, 0.0], [depth, across]], [0.0, depth]),
    };
    let shape = Shape::new()
        .fill(vec![base[0], base[1], apex], surface)
        .stroke(vec![base[0], apex, base[1]], 1.0, edge);
    let tri = el().size(w, h).shape(shape);
    // A strip along the facing edge, centring the triangle on it.
    match side {
        Side::Top => row().pin(Some(0.0), None, Some(0.0), Some(out)).justify_center().child(tri),
        Side::Bottom => row().pin(Some(0.0), Some(out), Some(0.0), None).justify_center().child(tri),
        Side::Left => col().pin(None, Some(0.0), Some(out), Some(0.0)).justify_center().child(tri),
        Side::Right => col().pin(Some(out), Some(0.0), None, Some(0.0)).justify_center().child(tri),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popups_toggle_and_close() {
        let mut p = Popups::default();
        p.toggle("a");
        assert!(p.is_open("a"));
        p.sub = Some("x".into());
        p.toggle("b");
        assert!(p.is_open("b") && p.sub.is_none());
        p.toggle("b");
        assert_eq!(p, Popups::default());
        p.open_at("m", [3.0, 4.0]);
        assert_eq!((p.open.as_deref(), p.at), (Some("m"), Some([3.0, 4.0])));
    }
}
