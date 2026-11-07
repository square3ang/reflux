//! `AssetCropModal` / `ImageCropModal`: the picked image fitted in a 350-point stage under a
//! crop frame (round for avatars, 17:6 for profile banners); drag to move, wheel or the slider
//! to zoom (1–3×), rotate a quarter turn, then save the framed part, scaled down to the asset's
//! maximum, into the form as a data URL. An animated GIF is cropped frame by frame and stays an
//! animated GIF (Fluxer's `AnimatedImageCropWorker`).
// shortcut: animated WebP crops to its first frame (no WebP animation encoder in `image`).

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
    GuildIcon,
    GuildBanner,
    Splash,
    EmbedSplash,
    RoleIcon,
}

impl Asset {
    /// The community asset a settings form key holds.
    pub fn for_key(key: &str) -> Asset {
        match key {
            "banner" => Asset::GuildBanner,
            "splash" => Asset::Splash,
            "embed_splash" => Asset::EmbedSplash,
            "role-icon" => Asset::RoleIcon,
            _ => Asset::GuildIcon,
        }
    }
    fn round(self) -> bool {
        matches!(self, Asset::Avatar | Asset::GuildIcon | Asset::RoleIcon)
    }
    fn aspect(self) -> f32 {
        match self {
            Asset::ProfileBanner => 17.0 / 6.0,
            Asset::GuildBanner | Asset::Splash | Asset::EmbedSplash => 16.0 / 9.0,
            _ => 1.0,
        }
    }
    /// `minHeightRatio`/`maxHeightRatio`: community banners and backgrounds may be cut down to
    /// half their height.
    fn height_range(self) -> (f32, f32) {
        match self {
            Asset::GuildBanner | Asset::Splash | Asset::EmbedSplash => (0.5, 1.0),
            _ => (1.0, 1.0),
        }
    }
    /// `canSkipOriginalAssetImage`: the original fits the maximum size and the aspect range
    /// (1% tolerance); a role icon also needs the minimum size.
    fn can_skip(self, (w, h): (f32, f32)) -> bool {
        let (mw, mh) = self.max();
        let (nw, nh) = self.min();
        if w <= 0.0 || h <= 0.0 || w > mw as f32 || h > mh as f32 || (self == Asset::RoleIcon && (w < nw as f32 || h < nh as f32)) {
            return false;
        }
        let (lo, hi) = self.height_range();
        let a = w / h;
        a >= self.aspect() / hi * 0.99 && a <= self.aspect() / lo * 1.01
    }
    fn max(self) -> (u32, u32) {
        match self {
            Asset::Avatar | Asset::GuildIcon => (1024, 1024),
            Asset::ProfileBanner => (2048, 723),
            Asset::GuildBanner | Asset::Splash | Asset::EmbedSplash => (2048, 1152),
            Asset::RoleIcon => (256, 256),
        }
    }
    fn min(self) -> (u32, u32) {
        match self {
            Asset::Avatar | Asset::GuildIcon => (256, 256),
            Asset::ProfileBanner => (680, 240),
            Asset::GuildBanner | Asset::Splash | Asset::EmbedSplash => (960, 540),
            Asset::RoleIcon => (64, 64),
        }
    }
    /// `getTitle`, `getDescription`, `getSaveButtonLabel`.
    fn texts(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Asset::Avatar => ("Crop avatar", "Drag to reposition your avatar and use the scroll wheel or pinch to zoom. The recommended minimum size is {minimumWidth}×{minimumHeight} pixels.", "Save avatar"),
            Asset::ProfileBanner => ("Crop profile banner", "Drag to reposition your banner and use the scroll wheel or pinch to zoom. The recommended minimum size is {minimumWidth}×{minimumHeight} pixels (17:6).", "Save banner"),
            Asset::GuildIcon => ("Crop community icon", "Drag to reposition your community icon and use the scroll wheel or pinch to zoom. The recommended minimum size is {minimumWidth}×{minimumHeight} pixels.", "Save icon"),
            Asset::GuildBanner => ("Crop banner", "Drag to reposition your banner and use the scroll wheel or pinch to zoom. The recommended minimum size is {minimumWidth}×{minimumHeight} pixels (16:9).", "Save banner"),
            Asset::Splash => ("Crop invite background", "Drag to reposition your invite background and use the scroll wheel or pinch to zoom. The recommended minimum size is {minimumWidth}×{minimumHeight} pixels (16:9).", "Save background"),
            Asset::EmbedSplash => ("Crop chat embed background", "Drag to reposition your chat embed background and use the scroll wheel or pinch to zoom. The recommended minimum size is {minimumWidth}×{minimumHeight} pixels (16:9).", "Save background"),
            Asset::RoleIcon => ("Crop role icon", "Drag to reposition your role icon and use the scroll wheel or pinch to zoom. The recommended minimum size is {minimumWidth}×{minimumHeight} pixels.", "Save icon"),
        }
    }
}

/// A press handler that asks for an image for `form.data[key]` and opens the crop modal.
pub(crate) fn pick<S: 'static>(acc: ShellAccess<S>, key: &'static str, asset: Asset) -> impl Fn(&mut S, &mut rune::Tx) + 'static {
    move |_: &mut S, tx: &mut rune::Tx| {
        let exts: &[&str] = if asset == Asset::RoleIcon { &["png", "jpg", "jpeg", "webp", "avif"] } else { &["png", "jpg", "jpeg", "gif", "webp", "avif"] };
        let options = rune::FileDialogOptions { title: t("Choose an image").into(), ..Default::default() }.filter(t("Images"), exts);
        tx.open_file(options, move |s: &mut S, r, _| {
            let sh = acc(s);
            let read = r.map_err(|e| e.to_string()).and_then(|p| p.map(|p| std::fs::read(&p).map_err(|e| e.to_string())).transpose());
            match read {
                Ok(Some(b)) if asset == Asset::RoleIcon && b.starts_with(b"GIF8") => sh.form.error = Some(t("Animated icons are not supported. Use a static image.").into()),
                Ok(Some(bytes)) => match Crop::new(key, asset, bytes) {
                    Some(c) => sh.nav.crop = Some(c),
                    None => sh.form.error = Some(t("This image couldn't be read. It may be damaged.").into()),
                },
                Ok(None) => {}
                Err(e) => sh.form.error = Some(format!("Couldn't open the image: {e}")),
            }
        });
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
    /// `heightRatio`: the frame's height against the asset's aspect (1 = full).
    height: f32,
}

impl Crop {
    /// `None` if the bytes aren't an image.
    pub fn new(key: &'static str, asset: Asset, bytes: Vec<u8>) -> Option<Crop> {
        let (w, h) = image::load_from_memory(&bytes).ok().map(|i| (i.width(), i.height()))?;
        let mut hs = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hs);
        Some(Crop { key, asset, src: Arc::new(bytes), id: hs.finish(), nat: (w as f32, h as f32), zoom: 1.0, off: (0.0, 0.0), grab: (0.0, 0.0), height: 1.0 })
    }

    /// The image contained in the stage, and the crop frame (`computeCropRect`) inside it.
    fn layout(&self) -> ((f32, f32), (f32, f32)) {
        let s = (STAGE_W / self.nat.0).min(STAGE_H / self.nat.1);
        let fit = (self.nat.0 * s, self.nat.1 * s);
        let a = self.asset.aspect() / self.height;
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

    fn set_height(&mut self, h: f32) {
        let (lo, hi) = self.asset.height_range();
        self.height = h.clamp(lo, hi);
        self.off = self.clamp(self.off.0, self.off.1);
    }

    fn edited(&self) -> bool {
        self.zoom != 1.0 || self.off != (0.0, 0.0) || self.height != 1.0
    }

    /// `cropCoversDisplayedImage`: the frame shows the whole image (within a point).
    fn covers_image(&self) -> bool {
        let (fit, frame) = self.layout();
        (frame.0 - fit.0 * self.zoom).abs() <= 1.0 && (frame.1 - fit.1 * self.zoom).abs() <= 1.0
    }

    /// The original image as it was picked (`onSkip`).
    fn original(&self) -> &[u8] {
        &self.src
    }

    /// A quarter turn clockwise (the image itself turns; the frame stays).
    fn rotate(&mut self) {
        if let Ok(out) = map_frames(&self.src, |i| i.rotate90())
            && let Some(c) = Crop::new(self.key, self.asset, out)
        {
            *self = Crop { zoom: self.zoom, height: self.height, ..c };
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

    /// The cropped PNG (GIF for a GIF), scaled to fit the asset's maximum.
    pub fn export(&self) -> Result<Vec<u8>, String> {
        let (x, y, w, h) = self.source_rect();
        let (mw, mh) = self.asset.max();
        let role = self.asset == Asset::RoleIcon;
        map_frames(&self.src, |img| {
            let out = img.crop_imm(x, y, w.min(img.width() - x), h.min(img.height() - y));
            if out.width() > mw || out.height() > mh {
                out.resize(mw, mh, image::imageops::FilterType::Lanczos3)
            } else if role && out.width() < 32 {
                // `role_icon` policy: at least 32px.
                out.resize_exact(64, 64, image::imageops::FilterType::Lanczos3)
            } else {
                out
            }
        })
    }
}

/// Applies `f` to every frame of a GIF (delays and looping kept), else to the still image (PNG out).
fn map_frames(bytes: &[u8], f: impl Fn(image::DynamicImage) -> image::DynamicImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    if bytes.starts_with(b"GIF8") {
        use image::AnimationDecoder;
        use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
        let frames = GifDecoder::new(std::io::Cursor::new(bytes)).and_then(|d| d.into_frames().collect_frames()).map_err(|e| e.to_string())?;
        let mut enc = GifEncoder::new_with_speed(&mut out, 10);
        enc.set_repeat(Repeat::Infinite).map_err(|e| e.to_string())?;
        for fr in frames {
            let delay = fr.delay();
            let img = f(image::DynamicImage::ImageRgba8(fr.into_buffer())).to_rgba8();
            enc.encode_frame(image::Frame::from_parts(img, 0, 0, delay)).map_err(|e| e.to_string())?;
        }
        drop(enc);
        return Ok(out);
    }
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    f(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out)
}

fn crop_of<S: 'static>(acc: ShellAccess<S>, s: &mut S) -> Option<&mut Crop> {
    acc(s).nav.crop.as_mut()
}

/// The modal over the settings.
pub fn modal<S: 'static>(st: &ShellState, c: &Crop, p: &Palette, acc: ShellAccess<S>) -> El {
    let round = c.asset.round();
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
    let (title, desc, save) = c.asset.texts();
    let (title, desc, save) = (t(title), tf(desc, &dims), t(save));
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
    let (lo, hi) = c.asset.height_range();
    let slider_label = |s: &str| text(t(s).to_string()).font(font(12.0, 500.0).lh(16.0)).color(p.text_secondary);
    let mut sliders = row().grow(1.0).min_w(0.0).gap(16.0).items_start().child(col().grow(1.0).w(0.0).gap(4.0).child(slider_label("Zoom")).child(zoom));
    if !round && lo < hi {
        // `.heightIconShort` / `.heightIconTall`: 12-wide bars, 6 and 16 tall.
        let bar = |h: f32, o: f32| el().size(12.0, h).radius(2.0).fill(p.text_tertiary).opacity(o);
        let height = row()
            .grow(1.0)
            .gap(10.0)
            .center_y()
            .child(bar(6.0, 0.6))
            .child(el().grow(1.0).min_w(120.0).child(Slider::new("crop-height", c.height, lo, hi).step(0.01).on_change(move |s: &mut S, v| {
                if let Some(c) = crop_of(acc, s) {
                    c.set_height(v);
                }
            }).el(p)))
            .child(bar(16.0, 0.9));
        sliders = sliders.child(col().grow(1.0).w(0.0).gap(4.0).child(slider_label("Height")).child(height));
    }
    let controls = row().w_full().px(24.0).py(16.0).gap(24.0).center_y().child(sliders).child(rotate);
    let body = col()
        .w_full()
        .gap(16.0)
        .child(text(desc).font(font(14.0, 400.0).lh(20.0)).color(p.text_secondary).wrap())
        .child(stage)
        .child(controls);
    let close = move |s: &mut S| acc(s).nav.crop = None;
    let edited = c.edited();
    let skip_ok = c.asset.can_skip(c.nat);
    // `onSkip`: keep the picked file as it is.
    let keep = move |sh: &mut ShellState, c: &Crop| {
        let ext = image::guess_format(c.original()).ok().and_then(|f| f.extensions_str().first().copied()).unwrap_or("png");
        let name = format!("a.{ext}");
        if c.asset == Asset::RoleIcon {
            sh.form.data.remove("role-emoji");
        }
        sh.form.data.insert(c.key.into(), Value::String(crate::guild::data_url(std::path::Path::new(&name), c.original())));
    };
    let skip = move |s: &mut S| {
        let sh = acc(s);
        if let Some(c) = sh.nav.crop.take() {
            keep(sh, &c);
        }
    };
    let apply = move |s: &mut S| {
        let sh = acc(s);
        let Some(c) = sh.nav.crop.take() else { return };
        // Saving an untouched frame that shows the whole picture keeps the original.
        if !c.edited() && skip_ok && c.covers_image() {
            keep(sh, &c);
            return;
        }
        match c.export() {
            Ok(out) => {
                let name = if out.starts_with(b"GIF8") { "a.gif" } else { "a.png" };
                if c.asset == Asset::RoleIcon {
                    sh.form.data.remove("role-emoji");
                }
                sh.form.data.insert(c.key.into(), Value::String(crate::guild::data_url(std::path::Path::new(name), &out)));
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
                        (c.zoom, c.off, c.height) = (1.0, (0.0, 0.0), 1.0);
                    }
                })
                .el(p),
        )
        .child(el().grow(1.0))
        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
        .children(skip_ok.then(|| Button::new(t("Skip cropping")).variant(ButtonVariant::Secondary).key("crop-skip").on_press(skip).el(p)))
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
    fn height_ratio_and_skip_follow_the_asset_config() {
        let mut c = Crop::new("k", Asset::GuildBanner, png(1600, 900)).unwrap();
        assert!(c.covers_image() && !c.edited());
        c.set_height(0.2);
        assert_eq!(c.height, 0.5, "community banners go down to half height");
        let (fit, frame) = c.layout();
        assert_eq!((frame.0, (frame.1 * 10.0).round() / 10.0), (fit.0, (fit.1 / 2.0 * 10.0).round() / 10.0));
        assert!(c.edited() && !c.covers_image());
        let mut a = Crop::new("k", Asset::Avatar, png(400, 400)).unwrap();
        a.set_height(0.5);
        assert_eq!(a.height, 1.0, "no height slider for round assets");
        assert!(Asset::GuildBanner.can_skip((1600.0, 900.0)));
        assert!(Asset::GuildBanner.can_skip((1600.0, 450.0)), "a half-height (32:9) banner is in range");
        assert!(!Asset::GuildBanner.can_skip((1600.0, 400.0)), "flatter than half height");
        assert!(!Asset::GuildBanner.can_skip((4000.0, 2250.0)), "larger than the maximum");
        assert!(!Asset::Avatar.can_skip((400.0, 300.0)), "not square");
        assert!(Asset::Avatar.can_skip((400.0, 400.0)));
        assert!(!Asset::RoleIcon.can_skip((32.0, 32.0)), "role icons need the minimum size");
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
        // A GIF stays animated through rotate and crop.
        let mut gif = Vec::new();
        {
            use image::codecs::gif::GifEncoder;
            let mut e = GifEncoder::new(&mut gif);
            for _ in 0..2 {
                e.encode_frame(image::Frame::new(image::RgbaImage::new(40, 20))).unwrap();
            }
        }
        let mut g = Crop::new("k", Asset::Avatar, gif).unwrap();
        g.rotate();
        assert_eq!(g.nat, (20.0, 40.0));
        let out = g.export().unwrap();
        use image::AnimationDecoder;
        let frames = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&out)).unwrap().into_frames().collect_frames().unwrap();
        assert_eq!((frames.len(), frames[0].buffer().width(), frames[0].buffer().height()), (2, 20, 20));
        let b = Crop::new("k", Asset::ProfileBanner, png(4000, 1000)).unwrap();
        let out = image::load_from_memory(&b.export().unwrap()).unwrap();
        assert!(out.width() <= 2048 && out.height() <= 723);
    }
}
