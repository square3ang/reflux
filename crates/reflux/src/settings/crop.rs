//! `AssetCropModal` / `ImageCropModal`: the picked image fitted in a 350-point stage under a
//! crop frame (round for avatars, 17:6 for profile banners); drag to move, wheel or the slider
//! to zoom (1–3×), rotate a quarter turn, then save the framed part, scaled down to the asset's
//! maximum, into the form as a data URL.
// shortcut: animated GIFs skip the modal and upload as picked (Fluxer crops them frame by frame);
// no community banner height slider (no community asset uses this modal yet).

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use rune::{El, Tx, col, el, icon, row, text};
use serde_json::Value;

use crate::i18n::{t, tf};
use crate::icons;
use crate::media::Load;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Modal, ModalSize, Slider};

/// `ASSET_CONFIGS` for the assets Reflux crops.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Asset {
    Avatar,
    ProfileBanner,
}

impl Asset {
    fn aspect(self) -> f32 {
        match self {
            Asset::Avatar => 1.0,
            Asset::ProfileBanner => 17.0 / 6.0,
        }
    }
    fn max(self) -> (u32, u32) {
        match self {
            Asset::Avatar => (1024, 1024),
            Asset::ProfileBanner => (2048, 723),
        }
    }
    fn min(self) -> (u32, u32) {
        match self {
            Asset::Avatar => (256, 256),
            Asset::ProfileBanner => (680, 240),
        }
    }
}

const STAGE_W: f32 = 568.0;
const STAGE_H: f32 = 350.0;
const MAX_ZOOM: f32 = 3.0;

/// The open crop modal.
#[derive(Clone, Debug)]
pub struct Crop {
    /// Where the result goes in `form.data`.
    pub key: &'static str,
    pub asset: Asset,
    src: Arc<Vec<u8>>,
    id: u64,
    nat: (f32, f32),
    zoom: f32,
    off: (f32, f32),
    grab: (f32, f32),
}

impl Crop {
    /// `None` if the bytes aren't an image.
    pub fn new(key: &'static str, asset: Asset, bytes: Vec<u8>) -> Option<Crop> {
        let (w, h) = image::load_from_memory(&bytes).ok().map(|i| (i.width(), i.height()))?;
        let mut hs = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hs);
        Some(Crop { key, asset, src: Arc::new(bytes), id: hs.finish(), nat: (w as f32, h as f32), zoom: 1.0, off: (0.0, 0.0), grab: (0.0, 0.0) })
    }

    /// The image contained in the stage, and the crop frame (`computeCropRect`) inside it.
    fn layout(&self) -> ((f32, f32), (f32, f32)) {
        let s = (STAGE_W / self.nat.0).min(STAGE_H / self.nat.1);
        let fit = (self.nat.0 * s, self.nat.1 * s);
        let a = self.asset.aspect();
        let frame = if fit.0 / fit.1 > a { (fit.1 * a, fit.1) } else { (fit.0, fit.0 / a) };
        (fit, frame)
    }

    /// `clampTransformToBounds`: the frame stays on the zoomed image.
    fn clamp(&self, x: f32, y: f32) -> (f32, f32) {
        let (fit, frame) = self.layout();
        let bx = ((fit.0 * self.zoom - frame.0) / 2.0).max(0.0);
        let by = ((fit.1 * self.zoom - frame.1) / 2.0).max(0.0);
        (x.clamp(-bx, bx), y.clamp(-by, by))
    }

    fn set_zoom(&mut self, z: f32) {
        self.zoom = z.clamp(1.0, MAX_ZOOM);
        self.off = self.clamp(self.off.0, self.off.1);
    }

    /// A quarter turn clockwise (the image itself turns; the frame stays).
    fn rotate(&mut self) {
        let Ok(img) = image::load_from_memory(&self.src) else { return };
        let mut png = Vec::new();
        if img.rotate90().write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_ok()
            && let Some(c) = Crop::new(self.key, self.asset, png)
        {
            *self = Crop { zoom: self.zoom, ..c };
            self.off = self.clamp(0.0, 0.0);
        }
    }

    /// The framed part in source pixels: (x, y, w, h).
    fn source_rect(&self) -> (u32, u32, u32, u32) {
        let (fit, frame) = self.layout();
        let shown = (fit.0 * self.zoom, fit.1 * self.zoom);
        let k = self.nat.0 / shown.0;
        let x = ((shown.0 - frame.0) / 2.0 - self.off.0) * k;
        let y = ((shown.1 - frame.1) / 2.0 - self.off.1) * k;
        let (w, h) = (frame.0 * k, frame.1 * k);
        let cl = |v: f32, max: f32| v.round().clamp(0.0, max) as u32;
        (cl(x, self.nat.0 - 1.0), cl(y, self.nat.1 - 1.0), cl(w, self.nat.0).max(1), cl(h, self.nat.1).max(1))
    }

    /// The cropped PNG, scaled to fit the asset's maximum.
    pub fn export(&self) -> Result<Vec<u8>, String> {
        let img = image::load_from_memory(&self.src).map_err(|e| e.to_string())?;
        let (x, y, w, h) = self.source_rect();
        let mut out = img.crop_imm(x, y, w.min(img.width() - x), h.min(img.height() - y));
        let (mw, mh) = self.asset.max();
        if out.width() > mw || out.height() > mh {
            out = out.resize(mw, mh, image::imageops::FilterType::Lanczos3);
        }
        let mut png = Vec::new();
        out.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
        Ok(png)
    }
}

fn crop_of<S: 'static>(acc: ShellAccess<S>, s: &mut S) -> Option<&mut Crop> {
    acc(s).nav.crop.as_mut()
}

/// The modal over the settings.
pub fn modal<S: 'static>(st: &ShellState, c: &Crop, p: &Palette, acc: ShellAccess<S>) -> El {
    let round = c.asset == Asset::Avatar;
    let (fit, frame) = c.layout();
    let src = c.src.clone();
    let shown = (fit.0 * c.zoom, fit.1 * c.zoom);
    let mut picture = el().absolute((STAGE_W - shown.0) / 2.0 + c.off.0, (STAGE_H - shown.1) / 2.0 + c.off.1).size(shown.0, shown.1);
    if let Load::Ready(px) = st.media.local(&format!("crop:{:x}", c.id), move || Ok((*src).clone())) {
        picture = picture.pixels_fit(&px, rune::SurfaceFit::Fill);
    }
    // The dimmed surround with the frame's hole (`box-shadow: 0 0 0 9999px`), then its 5-point
    // white edge.
    const SPREAD: f32 = 2000.0;
    let r = if round { frame.0 / 2.0 } else { 4.0 };
    let dim = el()
        .absolute((STAGE_W - frame.0) / 2.0 - SPREAD, (STAGE_H - frame.1) / 2.0 - SPREAD)
        .size(frame.0 + 2.0 * SPREAD, frame.1 + 2.0 * SPREAD)
        .radius(r + SPREAD)
        .border(SPREAD, rune::color::hex("#2f3136").unwrap_or(rune::Color::BLACK).alpha(0.6));
    let edge = el().absolute((STAGE_W - frame.0) / 2.0, (STAGE_H - frame.1) / 2.0).size(frame.0, frame.1).radius(r).border(5.0, rune::Color::WHITE);
    let stage = el()
        .w(STAGE_W)
        .h(STAGE_H)
        .radius(4.0)
        .clip_rounded(4.0)
        .fill(p.background_primary)
        .cursor(rune::Cursor::Grab)
        .key("crop-stage")
        .label(t("Crop preview"))
        .on_drag(move |s: &mut S, d: &mut rune::Drag, _: &mut Tx| {
            let Some(c) = crop_of(acc, s) else { return };
            if d.phase == rune::Phase::Down {
                c.grab = c.off;
            }
            c.off = c.clamp(c.grab.0 + d.dx, c.grab.1 + d.dy);
        })
        .on_wheel(move |s: &mut S, e: rune::WheelEvent, _: &mut Tx| {
            if let Some(c) = crop_of(acc, s) {
                c.set_zoom(c.zoom - e.dy / 200.0);
            }
            true
        })
        .child(picture)
        .child(dim)
        .child(edge);
    let (mw, mh) = c.asset.min();
    let dims = [("minimumWidth", mw.to_string()), ("minimumHeight", mh.to_string())];
    let dims: Vec<(&str, &str)> = dims.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let (title, desc, save) = match c.asset {
        Asset::Avatar => (
            t("Crop avatar"),
            tf("Drag to reposition your avatar and use the scroll wheel or pinch to zoom. The recommended minimum size is {minimumWidth}×{minimumHeight} pixels.", &dims),
            t("Save avatar"),
        ),
        Asset::ProfileBanner => (
            t("Crop profile banner"),
            tf("Drag to reposition your banner and use the scroll wheel or pinch to zoom. The recommended minimum size is {minimumWidth}×{minimumHeight} pixels (17:6).", &dims),
            t("Save banner"),
        ),
    };
    let zoom = row()
        .grow(1.0)
        .gap(12.0)
        .center_y()
        .child(icon(&icons::IMAGE_SQUARE_FILL, 12.0).color(p.text_tertiary))
        .child(el().grow(1.0).child(Slider::new("crop-zoom", c.zoom, 1.0, MAX_ZOOM).step(0.01).on_change(move |s: &mut S, z| {
            if let Some(c) = crop_of(acc, s) {
                c.set_zoom(z);
            }
        }).el(p)))
        .child(icon(&icons::IMAGE_SQUARE_FILL, 24.0).color(p.text_tertiary));
    let rotate = row()
        .size(40.0, 40.0)
        .center()
        .radius(8.0)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(t("Rotate clockwise"))
        .focusable(true)
        .focus_ring(4.0, 0.0, p.focus_primary)
        .key("crop-rotate")
        .on_press(move |s: &mut S| {
            if let Some(c) = crop_of(acc, s) {
                c.rotate();
            }
        })
        .child(icon(&icons::ARROW_CLOCKWISE, 24.0).color(p.text_primary));
    let controls = row()
        .w_full()
        .px(24.0)
        .py(16.0)
        .gap(24.0)
        .center_y()
        .child(col().grow(1.0).gap(8.0).child(text(t("Zoom")).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary)).child(zoom))
        .child(rotate);
    let body = col()
        .w_full()
        .gap(16.0)
        .child(text(desc).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap())
        .child(stage)
        .child(controls);
    let close = move |s: &mut S| acc(s).nav.crop = None;
    let edited = c.zoom != 1.0 || c.off != (0.0, 0.0);
    let apply = move |s: &mut S| {
        let sh = acc(s);
        let Some(c) = sh.nav.crop.take() else { return };
        match c.export() {
            Ok(png) => {
                sh.form.data.insert(c.key.into(), Value::String(crate::guild::data_url(std::path::Path::new("a.png"), &png)));
            }
            Err(e) => sh.form.error = Some(format!("{}: {e}", t("Couldn't crop image"))),
        }
    };
    let footer = row()
        .gap(8.0)
        .child(
            Button::new(t("Reset"))
                .variant(ButtonVariant::Secondary)
                .disabled(!edited)
                .on_press(move |s: &mut S| {
                    if let Some(c) = crop_of(acc, s) {
                        (c.zoom, c.off) = (1.0, (0.0, 0.0));
                    }
                })
                .el(p),
        )
        .child(el().grow(1.0))
        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
        .child(Button::new(save).key("crop-save").on_press(apply).el(p));
    Modal::new(title, close).size(ModalSize::Medium).body(body).footer(footer).el(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbaImage::new(w, h).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn frames_clamps_and_exports_the_framed_part() {
        let mut c = Crop::new("k", Asset::Avatar, png(400, 200)).unwrap();
        let (fit, frame) = c.layout();
        assert_eq!(fit, (568.0, 284.0));
        assert_eq!(frame, (284.0, 284.0), "the largest square inside the fitted image");
        assert_eq!(c.source_rect(), (100, 0, 200, 200), "centred");
        assert_eq!(c.clamp(1000.0, 50.0), (142.0, 0.0), "the frame can't leave the image");
        c.off = c.clamp(1000.0, 0.0);
        assert_eq!(c.source_rect(), (0, 0, 200, 200), "dragged right shows the left edge");
        c.set_zoom(2.0);
        assert_eq!(c.source_rect().2, 100, "2× zoom frames half as much");
        let out = image::load_from_memory(&c.export().unwrap()).unwrap();
        assert_eq!((out.width(), out.height()), (100, 100));
        c.rotate();
        assert_eq!(c.nat, (200.0, 400.0));
        let b = Crop::new("k", Asset::ProfileBanner, png(4000, 1000)).unwrap();
        let out = image::load_from_memory(&b.export().unwrap()).unwrap();
        assert!(out.width() <= 2048 && out.height() <= 723);
    }
}
