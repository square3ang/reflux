//! Media proxy URLs (`fluxer_docs/media-proxy/*`, `AvatarUtils.ts`, `MediaProxyUtils.ts`) and
//! the loader that fetches them for the views.
//!
//! URLs ask for WebP as Fluxer's do. Rune decodes only PNG and JPEG on native, so WebP and GIF
//! are decoded here (first frame) on the blocking pool and handed to Rune as RGBA.
// shortcut: animated avatars, icons, emoji and stickers show their first frame; request
// `animated=true` once Rune plays GIF/APNG.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use reflux_model::Snowflake;
use rune::{Image, ImageCache, Pixels, Waker};

/// `MEDIA_PROXY_IMAGE_SIZES`: the `size` ladder.
const LADDER: [u32; 33] = [
    16, 20, 22, 24, 28, 32, 40, 44, 48, 56, 60, 64, 80, 96, 100, 128, 160, 240, 256, 300, 320, 480, 512, 600, 640, 1024,
    1280, 1536, 2048, 3072, 4096, 8192, 16384,
];

/// `snapMediaProxyImageSize`: the first rung at or above `px` device pixels.
pub fn snap(px: f32) -> u32 {
    LADDER.iter().copied().find(|&r| px <= r as f32).unwrap_or(16384)
}

/// `snapImageSizeWithinServerRange`: snapped, then clamped to the rungs inside an asset class.
fn snap_within(px: f32, min: u32, max: u32) -> u32 {
    let floor = LADDER.iter().copied().find(|&r| r >= min).unwrap_or(16384);
    let ceiling = LADDER.iter().copied().rfind(|&r| r <= max).unwrap_or(16);
    snap(px).clamp(floor, ceiling)
}

fn asset(base: &str, route: &str, id: Snowflake, hash: &str, size: u32) -> String {
    let hash = hash.strip_prefix("a_").unwrap_or(hash);
    format!("{}/{route}/{id}/{hash}.webp?size={size}", base.trim_end_matches('/'))
}

/// `getUserAvatarURL`: icon class (128–1024).
pub fn avatar_url(base: &str, user: Snowflake, hash: &str, css: f32, scale: f32) -> String {
    asset(base, "avatars", user, hash, snap_within(css * scale, 128, 1024))
}

/// `getGuildIconURL`: icon class.
pub fn guild_icon_url(base: &str, guild: Snowflake, hash: &str, css: f32, scale: f32) -> String {
    asset(base, "icons", guild, hash, snap_within(css * scale, 128, 1024))
}

/// `getGuildBannerURL`: banner class (480–2400) at the 360px header width.
pub fn guild_banner_url(base: &str, guild: Snowflake, hash: &str, scale: f32) -> String {
    asset(base, "banners", guild, hash, snap_within(360.0 * scale, 480, 2400))
}

/// `getEmojiURL`: no `size`, so the server's 128.
pub fn emoji_url(base: &str, id: &str) -> String {
    format!("{}/emojis/{id}.webp", base.trim_end_matches('/'))
}

/// `getStickerURL` as `MessageAttachments` asks for it: always 320.
pub fn sticker_url(base: &str, id: Snowflake) -> String {
    format!("{}/stickers/{id}.webp?size=320", base.trim_end_matches('/'))
}

/// `buildFitInsideMediaProxyURL` for an attachment or embed `proxy_url`: keeps the issued query
/// (signatures) and asks for `width` device pixels as WebP; the proxy never enlarges.
pub fn fit_url(proxy_url: &str, css_w: f32, scale: f32) -> String {
    let sep = if proxy_url.contains('?') { '&' } else { '?' };
    format!("{proxy_url}{sep}format=webp&width={}", (css_w * scale).ceil().max(1.0) as u32)
}

/// Network images larger than this on either side are refused before decoding.
const MAX_SIDE: u32 = 4096;

/// Encoded bytes to an image: WebP and GIF (first frame) decoded here, PNG and JPEG by Rune's
/// workers. Bounded to [`MAX_SIDE`] since the bytes come from the network.
pub fn decode(cache: &ImageCache, bytes: Vec<u8>) -> Result<Image, String> {
    use image::{ImageFormat, ImageReader, Limits};
    let mut reader = ImageReader::new(std::io::Cursor::new(&bytes[..])).with_guessed_format().map_err(|e| e.to_string())?;
    let format = reader.format().ok_or("unknown image format")?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    reader.limits(limits);
    if !matches!(format, ImageFormat::WebP | ImageFormat::Gif) {
        let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
        if w > MAX_SIDE || h > MAX_SIDE {
            return Err(format!("{w}×{h} is over {MAX_SIDE}"));
        }
        return Ok(cache.load(bytes));
    }
    let img = reader.decode().map_err(|e| e.to_string())?.into_rgba8();
    Ok(cache.rgba(img.width(), img.height(), img.into_raw()))
}

/// `EMOJI_SPRITES.BasePerRow` cells of 64 device px in `spritesheet-emoji@2x.png`.
const SPRITE_COLUMNS: u32 = 42;
const SPRITE_CELL: u32 = 64;

/// Cell `index` of Fluxer's unicode emoji spritesheet (`setSpriteSheetIndex` order).
fn sprite_cell(cache: &ImageCache, index: usize) -> Result<Image, String> {
    static SHEET: OnceLock<Option<image::RgbaImage>> = OnceLock::new();
    let sheet = SHEET
        .get_or_init(|| {
            let png = include_bytes!("../../../assets/media/images/spritesheet-emoji@2x.png");
            image::load_from_memory_with_format(png, image::ImageFormat::Png).ok().map(|i| i.into_rgba8())
        })
        .as_ref()
        .ok_or("spritesheet failed to decode")?;
    let (x, y) = (index as u32 % SPRITE_COLUMNS * SPRITE_CELL, index as u32 / SPRITE_COLUMNS * SPRITE_CELL);
    if y + SPRITE_CELL > sheet.height() {
        return Err(format!("sprite {index} is off the sheet"));
    }
    let cell = image::imageops::crop_imm(sheet, x, y, SPRITE_CELL, SPRITE_CELL).to_image();
    Ok(cache.rgba(SPRITE_CELL, SPRITE_CELL, cell.into_raw()))
}

/// What a view gets for a URL.
pub enum Load {
    Pending,
    Ready(Pixels),
    Failed,
}

enum Slot {
    Pending,
    Ready(Image),
    Failed,
}

#[derive(Default)]
struct Entries {
    map: HashMap<String, (Slot, u64)>,
    tick: u64,
}

/// Decoded images are kept for this many URLs; the least recently drawn go first.
const CAP: usize = 768;

/// Offline images for `--shell-demo`: called instead of the network for a URL.
pub type Fixture = fn(&ImageCache, &str) -> Option<Image>;

#[derive(Default)]
struct Inner {
    cache: OnceLock<ImageCache>,
    fixture: OnceLock<Fixture>,
    http: reqwest::Client,
    base: Mutex<String>,
    /// `f32` bits of the window scale; 0 = unset (1×).
    scale: AtomicU32,
    entries: Mutex<Entries>,
}

/// The shared image loader. Views call it through `&`; fetches run on the app runtime and
/// decode on Rune's image workers, which wake the app.
#[derive(Clone, Default)]
pub struct Media(Arc<Inner>);

impl Media {
    pub fn setup(&self, waker: Waker, fixture: Option<Fixture>) {
        let _ = self.0.cache.set(ImageCache::new(waker));
        if let Some(f) = fixture {
            let _ = self.0.fixture.set(f);
        }
    }

    /// `endpoints.media` from discovery.
    pub fn set_base(&self, base: &str) {
        *self.0.base.lock().unwrap_or_else(|e| e.into_inner()) = base.to_owned();
    }

    pub fn base(&self) -> String {
        self.0.base.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set_scale(&self, scale: f32) {
        self.0.scale.store(scale.to_bits(), Ordering::Relaxed);
    }

    pub fn scale(&self) -> f32 {
        match self.0.scale.load(Ordering::Relaxed) {
            0 => 1.0,
            b => f32::from_bits(b),
        }
    }

    /// A URL built from the base and scale; pending while the instance isn't known yet.
    pub fn asset(&self, url: impl FnOnce(&str, f32) -> String) -> Load {
        let base = self.base();
        if base.is_empty() { Load::Pending } else { self.get(&url(&base, self.scale())) }
    }

    /// Starts loading `url` on first use; later calls report its state.
    pub fn get(&self, url: &str) -> Load {
        self.entry(url, |cache| match self.0.fixture.get() {
            Some(f) => f(cache, url).map_or(Slot::Failed, Slot::Ready),
            None => {
                self.fetch(url.to_owned());
                Slot::Pending
            }
        })
    }

    /// A unicode emoji from the bundled spritesheet, by its [`crate::shell`] sprite index.
    pub fn sprite(&self, index: usize) -> Load {
        let key = format!("sprite:{index}");
        self.entry(&key, |cache| {
            if self.0.fixture.get().is_some() {
                // Headless snapshots draw what's ready in the first frames, so crop in place.
                return sprite_cell(cache, index).map_or(Slot::Failed, |img| {
                    let _ = img.wait();
                    Slot::Ready(img)
                });
            }
            self.decode_later(key.clone(), move |cache| sprite_cell(cache, index));
            Slot::Pending
        })
    }

    /// A local file or pasted image (upload previews), read and decoded off the UI thread.
    pub fn local(&self, key: &str, read: impl FnOnce() -> Result<Vec<u8>, String> + Send + 'static) -> Load {
        self.entry(key, |cache| {
            if self.0.fixture.get().is_some() {
                return read().and_then(|b| decode(cache, b)).map_or(Slot::Failed, |img| {
                    let _ = img.wait();
                    Slot::Ready(img)
                });
            }
            self.decode_later(key.to_owned(), move |cache| read().and_then(|b| decode(cache, b)));
            Slot::Pending
        })
    }

    fn entry(&self, key: &str, start: impl FnOnce(&ImageCache) -> Slot) -> Load {
        let Some(cache) = self.0.cache.get() else { return Load::Pending };
        let mut e = self.0.entries.lock().unwrap_or_else(|e| e.into_inner());
        e.tick += 1;
        let tick = e.tick;
        if let Some((slot, used)) = e.map.get_mut(key) {
            *used = tick;
            return state(slot);
        }
        if e.map.len() >= CAP {
            let old = e.map.iter().filter(|(_, (s, _))| !matches!(s, Slot::Pending)).min_by_key(|(_, (_, u))| *u).map(|(k, _)| k.clone());
            if let Some(k) = old {
                e.map.remove(&k);
            }
        }
        let slot = start(cache);
        let load = state(&slot);
        e.map.insert(key.to_owned(), (slot, tick));
        load
    }

    /// Runs `make` on the blocking pool and stores its image under `key`.
    fn decode_later(&self, key: String, make: impl FnOnce(&ImageCache) -> Result<Image, String> + Send + 'static) {
        let inner = self.0.clone();
        crate::app::spawn(async move {
            let job = inner.clone();
            let res = tokio::task::spawn_blocking(move || job.cache.get().ok_or_else(|| "no image cache".to_owned()).and_then(make)).await;
            inner.finish(&key, res.map_err(|e| e.to_string()).and_then(|r| r));
        });
    }

    fn fetch(&self, url: String) {
        let inner = self.0.clone();
        crate::app::spawn(async move {
            match async { inner.http.get(&url).send().await?.error_for_status()?.bytes().await }.await {
                Ok(b) => Media(inner).decode_later(url, move |cache| decode(cache, b.to_vec())),
                Err(err) => inner.finish(&url, Err(err.to_string())),
            }
        });
    }
}

impl Inner {
    fn finish(&self, key: &str, res: Result<Image, String>) {
        let slot = res.map_or_else(
            |err| {
                eprintln!("reflux: image {key}: {err}");
                Slot::Failed
            },
            Slot::Ready,
        );
        if let Some(entry) = self.entries.lock().unwrap_or_else(|e| e.into_inner()).map.get_mut(key) {
            entry.0 = slot;
        }
        // Content the cache already decoded doesn't wake the app, so ask for a frame.
        crate::app::update(|_| {});
    }
}

fn state(slot: &Slot) -> Load {
    match slot {
        Slot::Pending => Load::Pending,
        Slot::Failed => Load::Failed,
        Slot::Ready(img) if img.error().is_some() => Load::Failed,
        Slot::Ready(img) if !img.is_ready() => Load::Pending,
        Slot::Ready(img) => Load::Ready(img.pixels().clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: &str = "https://media.fluxer.app/";

    #[test]
    fn sizes_snap_to_the_ladder_and_asset_class() {
        assert_eq!(snap(641.0), 1024);
        assert_eq!(snap(64.0), 64);
        assert_eq!(snap(1e9), 16384);
        // Icon class floor 128, ceiling 1024.
        assert_eq!(snap_within(40.0, 128, 1024), 128);
        assert_eq!(snap_within(80.0 * 2.0, 128, 1024), 160);
        assert_eq!(snap_within(4000.0, 128, 1024), 1024);
        // Banner class: 360 → 480 at 1×, 720 → 1024 at 2×, ceiling 2048 (the last rung ≤ 2400).
        assert_eq!(snap_within(360.0, 480, 2400), 480);
        assert_eq!(snap_within(720.0, 480, 2400), 1024);
        assert_eq!(snap_within(9000.0, 480, 2400), 2048);
    }

    #[test]
    fn asset_urls_follow_fluxer() {
        assert_eq!(avatar_url(B, Snowflake(42), "abc", 40.0, 2.0), "https://media.fluxer.app/avatars/42/abc.webp?size=128");
        // Animated hashes load the static frame.
        assert_eq!(avatar_url(B, Snowflake(42), "a_abc", 80.0, 2.0), "https://media.fluxer.app/avatars/42/abc.webp?size=160");
        assert_eq!(guild_icon_url(B, Snowflake(7), "ff", 48.0, 1.0), "https://media.fluxer.app/icons/7/ff.webp?size=128");
        assert_eq!(guild_banner_url(B, Snowflake(7), "bb", 2.0), "https://media.fluxer.app/banners/7/bb.webp?size=1024");
        assert_eq!(emoji_url(B, "99"), "https://media.fluxer.app/emojis/99.webp");
    }

    #[test]
    fn fit_urls_keep_the_signature() {
        assert_eq!(fit_url("https://m.x/attachments/1/2/a.webp", 550.0, 1.0), "https://m.x/attachments/1/2/a.webp?format=webp&width=550");
        assert_eq!(
            fit_url("https://m.x/attachments/1/2/a.png?ex=1&is=2&hm=3", 275.5, 2.0),
            "https://m.x/attachments/1/2/a.png?ex=1&is=2&hm=3&format=webp&width=551"
        );
    }

    fn cache() -> ImageCache {
        ImageCache::new(Arc::new(|| {}))
    }

    /// A lossless WebP of `w`×`h` red pixels.
    pub(crate) fn webp(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let px = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 255]));
        image::DynamicImage::ImageRgba8(px).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::WebP).unwrap();
        out
    }

    #[test]
    fn webp_decodes_and_oversized_images_are_refused() {
        let c = cache();
        assert_eq!(decode(&c, webp(3, 2)).unwrap().wait().unwrap(), [3, 2]);
        assert!(decode(&c, webp(MAX_SIDE + 1, 1)).is_err());
        assert!(decode(&c, b"not an image".to_vec()).is_err());
        // PNG goes to Rune's decoder, after the same size check.
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(5, 4).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        assert_eq!(decode(&c, png).unwrap().wait().unwrap(), [5, 4]);
    }

    #[test]
    fn sprite_cells_crop_the_sheet() {
        let c = cache();
        assert_eq!(sprite_cell(&c, 0).unwrap().wait().unwrap(), [64, 64]);
        assert!(sprite_cell(&c, 42 * 47).is_err());
    }
}
