//! Media proxy URLs (`fluxer_docs/media-proxy/*`, `AvatarUtils.ts`, `MediaProxyUtils.ts`) and
//! the loader that fetches them for the views.
//!
//! URLs ask for WebP as Fluxer's do. Rune decodes it (and GIF, APNG) and plays animations while
//! drawn, so views ask for `animated=true` URLs only when Fluxer would animate (hover, the
//! user's animation settings).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use reflux_model::Snowflake;
use rune::{Image, ImageCache, Pixels, Waker};

/// A body cap: the media proxy is remote input, so nothing here buffers an unbounded response.
pub(crate) const MAX_IMAGE_BYTES: usize = 64 << 20;
const MAX_TEXT_BYTES: usize = 4 << 20;
const MAX_DOWNLOAD_BYTES: usize = 512 << 20;

/// GETs `url` and reads at most `cap` bytes, refusing a larger body from `Content-Length` or
/// while streaming, so a hostile or broken proxy can't exhaust memory or the disk.
pub(crate) async fn get_capped(http: &reqwest::Client, url: &str, cap: usize) -> Result<Vec<u8>, String> {
    let mut resp = http.get(url).send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?;
    if resp.content_length().is_some_and(|n| n > cap as u64) {
        return Err(format!("response is larger than {cap} bytes"));
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        if out.len() + chunk.len() > cap {
            return Err(format!("response is larger than {cap} bytes"));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

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

/// `parseMediaHashForRequest`: an `a_` hash asked for animated adds `animated=true`.
pub fn animate(url: String, hash: &str, animated: bool) -> String {
    if animated && hash.starts_with("a_") { format!("{url}&animated=true") } else { url }
}

/// `getUserAvatarURL`: icon class (128–1024).
pub fn avatar_url(base: &str, user: Snowflake, hash: &str, css: f32, scale: f32) -> String {
    asset(base, "avatars", user, hash, snap_within(css * scale, 128, 1024))
}

/// `getGuildMemberAvatarURL` / `getGuildMemberBannerURL`: a member's per-community image.
fn member_asset(base: &str, route: &str, guild: Snowflake, user: Snowflake, hash: &str, size: u32) -> String {
    let hash = hash.strip_prefix("a_").unwrap_or(hash);
    format!("{}/guilds/{guild}/users/{user}/{route}/{hash}.webp?size={size}", base.trim_end_matches('/'))
}

pub fn member_avatar_url(base: &str, guild: Snowflake, user: Snowflake, hash: &str, css: f32, scale: f32) -> String {
    member_asset(base, "avatars", guild, user, hash, snap_within(css * scale, 128, 1024))
}

pub fn member_banner_url(base: &str, guild: Snowflake, user: Snowflake, hash: &str, scale: f32) -> String {
    member_asset(base, "banners", guild, user, hash, snap_within(360.0 * scale, 480, 2400))
}

/// `getGuildIconURL`: icon class.
pub fn guild_icon_url(base: &str, guild: Snowflake, hash: &str, css: f32, scale: f32) -> String {
    asset(base, "icons", guild, hash, snap_within(css * scale, 128, 1024))
}

/// `getGuildBannerURL`: banner class (480–2400) at the 360px header width.
pub fn guild_banner_url(base: &str, guild: Snowflake, hash: &str, scale: f32) -> String {
    asset(base, "banners", guild, hash, snap_within(360.0 * scale, 480, 2400))
}

/// `getGuildSplashURL` / `getGuildEmbedSplashURL` (`kind` "splashes" / "embed-splashes"): wide
/// class at the 360px preview width.
pub fn guild_wide_url(base: &str, kind: &str, guild: Snowflake, hash: &str, scale: f32) -> String {
    asset(base, kind, guild, hash, snap_within(360.0 * scale, 480, 2400))
}

/// `getEmojiURL`: no `size`, so the server's 128.
pub fn emoji_url(base: &str, id: &str) -> String {
    format!("{}/emojis/{id}.webp", base.trim_end_matches('/'))
}

/// `getEmojiURL` for an animated emoji that should move.
pub fn emoji_url_animated(base: &str, id: &str) -> String {
    format!("{}?animated=true", emoji_url(base, id))
}

/// `getStickerURL` as `MessageAttachments` asks for it: always 320.
pub fn sticker_url(base: &str, id: Snowflake, animated: bool) -> String {
    let a = if animated { "&animated=true" } else { "" };
    format!("{}/stickers/{id}.webp?size=320{a}", base.trim_end_matches('/'))
}

/// `buildFitInsideMediaProxyURL` for an attachment or embed `proxy_url`: keeps the issued query
/// (signatures) and asks for `width` device pixels as WebP; the proxy never enlarges.
pub fn fit_url(proxy_url: &str, css_w: f32, scale: f32) -> String {
    let sep = if proxy_url.contains('?') { '&' } else { '?' };
    format!("{proxy_url}{sep}format=webp&width={}", (css_w * scale).ceil().max(1.0) as u32)
}

/// `buildFittedAnimatedImageProxyURL`: [`fit_url`] that keeps the animation.
pub fn fit_url_animated(proxy_url: &str, css_w: f32, scale: f32) -> String {
    format!("{}&animated=true", fit_url(proxy_url, css_w, scale))
}

/// Network images larger than this on either side are refused before decoding.
const MAX_SIDE: u32 = 4096;

/// Encoded bytes to an image, decoded (and animated) by Rune's workers. Bounded to
/// [`MAX_SIDE`] from the header since the bytes come from the network.
pub fn decode(cache: &ImageCache, bytes: Vec<u8>) -> Result<Image, String> {
    use image::{ImageReader, Limits};
    // Twemoji SVGs, rasterised for the largest (48pt jumbo) emoji at 2×.
    if is_svg(&bytes) {
        return Ok(cache.svg(bytes, 48.0, 2.0));
    }
    let mut reader = ImageReader::new(std::io::Cursor::new(&bytes[..])).with_guessed_format().map_err(|e| e.to_string())?;
    reader.format().ok_or("unknown image format")?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    reader.limits(limits);
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("{w}×{h} is over {MAX_SIDE}"));
    }
    Ok(cache.load(bytes))
}

fn is_svg(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]);
    let head = head.trim_start_matches('\u{FEFF}').trim_start();
    head.starts_with("<svg") || (head.starts_with("<?xml") && head.contains("<svg"))
}

/// A 32px cell of `assets/media/images/neko.png` (11×4 oneko frames).
fn neko_cell(cache: &ImageCache, col: u8, row: u8, mirror: bool) -> Result<Image, String> {
    static SHEET: OnceLock<Option<image::RgbaImage>> = OnceLock::new();
    const PNG: &[u8] = include_bytes!("../../../assets/media/images/neko.png");
    let sheet = SHEET
        .get_or_init(|| image::load_from_memory_with_format(PNG, image::ImageFormat::Png).ok().map(|i| i.into_rgba8()))
        .as_ref()
        .ok_or("Neko sheet failed to decode")?;
    let (x, y) = (col as u32 * 32, row as u32 * 32);
    if x + 32 > sheet.width() || y + 32 > sheet.height() {
        return Err("Neko cell is off the sheet".into());
    }
    let mut cell = image::imageops::crop_imm(sheet, x, y, 32, 32).to_image();
    if mirror {
        image::imageops::flip_horizontal_in_place(&mut cell);
    }
    Ok(cache.rgba(32, 32, cell.into_raw()))
}

/// `EMOJI_SPRITES.BasePerRow` cells of 64 device px in `spritesheet-emoji@2x.png`; the
/// skin-tone sheets (`spritesheet-1f3fb@2x.png` …) have `SkinTonePerRow` (10).
const SPRITE_COLUMNS: u32 = 42;
const SKIN_COLUMNS: u32 = 10;
const SPRITE_CELL: u32 = 64;

/// Cell `index` of Fluxer's unicode emoji spritesheet (`setSpriteSheetIndex` order); `tone`
/// 1..=5 picks a skin-tone sheet, where `index` is the emoji's `skinToneIndex`.
fn sprite_cell(cache: &ImageCache, index: usize, tone: u8) -> Result<Image, String> {
    static SHEETS: [OnceLock<Option<image::RgbaImage>>; 6] = [const { OnceLock::new() }; 6];
    const PNGS: [&[u8]; 6] = [
        include_bytes!("../../../assets/media/images/spritesheet-emoji@2x.png"),
        include_bytes!("../../../assets/media/images/spritesheet-1f3fb@2x.png"),
        include_bytes!("../../../assets/media/images/spritesheet-1f3fc@2x.png"),
        include_bytes!("../../../assets/media/images/spritesheet-1f3fd@2x.png"),
        include_bytes!("../../../assets/media/images/spritesheet-1f3fe@2x.png"),
        include_bytes!("../../../assets/media/images/spritesheet-1f3ff@2x.png"),
    ];
    let t = (tone as usize).min(5);
    let sheet = SHEETS[t]
        .get_or_init(|| image::load_from_memory_with_format(PNGS[t], image::ImageFormat::Png).ok().map(|i| i.into_rgba8()))
        .as_ref()
        .ok_or("spritesheet failed to decode")?;
    let cols = if t == 0 { SPRITE_COLUMNS } else { SKIN_COLUMNS };
    let (x, y) = (index as u32 % cols * SPRITE_CELL, index as u32 / cols * SPRITE_CELL);
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
    /// `endpoints.static_cdn` (twemoji SVGs).
    static_base: Mutex<String>,
    /// `f32` bits of the window scale; 0 = unset (1×).
    scale: AtomicU32,
    entries: Mutex<Entries>,
    /// Fetched text files (previews), by URL: `None` while loading or after a failure.
    texts: Mutex<HashMap<String, Option<Result<String, String>>>>,
    /// An image fetched for "Copy image", as PNG, for the next frame to put on the clipboard.
    clip: Mutex<Option<Vec<u8>>>,
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

    /// `endpoints.static_cdn` from discovery.
    pub fn set_static_base(&self, base: &str) {
        *self.0.static_base.lock().unwrap_or_else(|e| e.into_inner()) = base.trim_end_matches('/').to_owned();
    }

    /// `getTwemojiURL`: a unicode emoji's twemoji SVG on the static host (`codepoints` as
    /// `convertToCodePoints` writes them); `Failed` before discovery named the host.
    pub fn twemoji(&self, codepoints: &str) -> Load {
        let base = self.0.static_base.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if base.is_empty() || codepoints.is_empty() {
            return Load::Failed;
        }
        self.get(&format!("{base}/emoji/{codepoints}.svg?v=2"))
    }

    /// The media client, for one-off fetches through [`get_capped`].
    pub fn http(&self) -> reqwest::Client {
        self.0.http.clone()
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
        self.skin_sprite(index, 0)
    }

    /// [`Self::sprite`] from skin tone `tone`'s sheet (1..=5; 0 is the base sheet).
    pub fn skin_sprite(&self, index: usize, tone: u8) -> Load {
        let key = format!("sprite:{tone}:{index}");
        self.entry(&key, |cache| {
            if self.0.fixture.get().is_some() {
                // Headless snapshots draw what's ready in the first frames, so crop in place.
                return sprite_cell(cache, index, tone).map_or(Slot::Failed, |img| {
                    let _ = img.wait();
                    Slot::Ready(img)
                });
            }
            self.decode_later(key.clone(), move |cache| sprite_cell(cache, index, tone));
            Slot::Pending
        })
    }

    /// A 32px cell of Fluxer's Neko sheet (`NekoSprite`), by `(column, row)`, `mirror`ed for
    /// `scaleX(-1)`.
    pub fn neko(&self, cell: (u8, u8), mirror: bool) -> Load {
        let key = format!("neko:{}:{}:{mirror}", cell.0, cell.1);
        self.entry(&key, |cache| {
            let crop = move |cache: &ImageCache| neko_cell(cache, cell.0, cell.1, mirror);
            if self.0.fixture.get().is_some() {
                return crop(cache).map_or(Slot::Failed, |img| {
                    let _ = img.wait();
                    Slot::Ready(img)
                });
            }
            self.decode_later(key.clone(), crop);
            Slot::Pending
        })
    }

    /// A local file or pasted image (upload previews), read and decoded off the UI thread.
    pub fn local(&self, key: &str, read: impl FnOnce() -> Result<Vec<u8>, String> + Send + 'static) -> Load {        self.entry(key, |cache| {
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
            return self.state(slot);
        }
        if e.map.len() >= CAP {
            let old = e.map.iter().filter(|(_, (s, _))| !matches!(s, Slot::Pending)).min_by_key(|(_, (_, u))| *u).map(|(k, _)| k.clone());
            if let Some(k) = old {
                e.map.remove(&k);
            }
        }
        let slot = start(cache);
        let load = self.state(&slot);
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
            match get_capped(&inner.http, &url, MAX_IMAGE_BYTES).await {
                Ok(b) => Media(inner).decode_later(url, move |cache| decode(cache, b)),
                Err(err) => inner.finish(&url, Err(err)),
            }
        });
    }
}

impl Inner {
    fn finish(&self, key: &str, res: Result<Image, String>) {
        let slot = match res {
            Ok(i) => Slot::Ready(i),
            Err(err) => {
                eprintln!("reflux: image {key}: {err}");
                Slot::Failed
            }
        };
        if let Some(entry) = self.entries.lock().unwrap_or_else(|e| e.into_inner()).map.get_mut(key) {
            entry.0 = slot;
        }
        // Content the cache already decoded doesn't wake the app, so ask for a frame.
        crate::app::update(|_| {});
    }
}

fn still(img: &Image) -> Load {
    if img.error().is_some() {
        Load::Failed
    } else if !img.is_ready() {
        Load::Pending
    } else {
        Load::Ready(img.pixels().clone())
    }
}

impl Media {
    fn state(&self, slot: &Slot) -> Load {
        match slot {
            Slot::Pending => Load::Pending,
            Slot::Failed => Load::Failed,
            Slot::Ready(img) => still(img),
        }
    }

    /// A text file's contents (previews), fetched on first use; `None` while loading.
    pub fn text(&self, url: &str) -> Option<Result<String, String>> {
        let mut texts = self.0.texts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = texts.get(url) {
            return t.clone();
        }
        texts.insert(url.to_owned(), None);
        let (inner, url) = (self.0.clone(), url.to_owned());
        crate::app::spawn(async move {
            let r = get_capped(&inner.http, &url, MAX_TEXT_BYTES).await.map(|b| String::from_utf8_lossy(&b).into_owned());
            inner.texts.lock().unwrap_or_else(|e| e.into_inner()).insert(url, Some(r));
            crate::app::update(|_| {});
        });
        None
    }

    /// Puts a text file's contents in place (`--shell-demo`, which has no network).
    pub fn seed_text(&self, url: &str, text: &str) {
        self.0.texts.lock().unwrap_or_else(|e| e.into_inner()).insert(url.to_owned(), Some(Ok(text.to_owned())));
    }

    /// Saves `url` to `path` (a "Download" the user picked a place for).
    pub fn download(&self, url: String, path: std::path::PathBuf) {
        let inner = self.0.clone();
        crate::app::spawn(async move {
            let r = match get_capped(&inner.http, &url, MAX_DOWNLOAD_BYTES).await {
                Ok(b) => tokio::fs::write(&path, &b).await.map_err(|e| e.to_string()),
                Err(e) => Err(e),
            };
            if let Err(e) = r {
                eprintln!("reflux: download {url} to {}: {e}", path.display());
            }
        });
    }

    /// Fetches `url` and re-encodes it as PNG for the clipboard (`MediaClipboard`); the next
    /// frame copies it ([`Self::take_clip`]).
    pub fn copy_image(&self, url: String) {
        let inner = self.0.clone();
        crate::app::spawn(async move {
            let png = get_capped(&inner.http, &url, MAX_IMAGE_BYTES).await.and_then(|b| {
                let img = image::load_from_memory(&b).map_err(|e| e.to_string())?;
                let mut png = Vec::new();
                img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
                Ok(png)
            });
            match png {
                Ok(png) => {
                    *inner.clip.lock().unwrap_or_else(|e| e.into_inner()) = Some(png);
                    crate::app::update(|_| {});
                }
                Err(e) => eprintln!("reflux: copy image {url}: {e}"),
            }
        });
    }

    pub fn take_clip(&self) -> Option<Vec<u8>> {
        self.0.clip.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_body_refuses_an_oversized_response() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf);
            // Content-Length exceeds the cap, so it must be refused before the body arrives.
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\n\r\n");
        });
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let http = reqwest::Client::new();
        let err = rt.block_on(get_capped(&http, &format!("http://{addr}/x"), 16)).unwrap_err();
        assert!(err.contains("larger than 16 bytes"), "{err}");
    }

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
        assert_eq!(member_avatar_url(B, Snowflake(7), Snowflake(42), "a_m", 40.0, 2.0), "https://media.fluxer.app/guilds/7/users/42/avatars/m.webp?size=128");
        assert_eq!(member_banner_url(B, Snowflake(7), Snowflake(42), "mb", 2.0), "https://media.fluxer.app/guilds/7/users/42/banners/mb.webp?size=1024");
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
    fn animations_go_to_rune() {
        use image::codecs::gif::{GifEncoder, Repeat};
        let mut gif = Vec::new();
        {
            let mut enc = GifEncoder::new(&mut gif);
            enc.set_repeat(Repeat::Infinite).unwrap();
            for (c, ms) in [(255u8, 50u32), (0, 150)] {
                let px = image::RgbaImage::from_pixel(4, 4, image::Rgba([c, 0, 0, 255]));
                enc.encode_frame(image::Frame::from_parts(px, 0, 0, image::Delay::from_numer_denom_ms(ms, 1))).unwrap();
            }
        }
        assert_eq!(decode(&cache(), gif).unwrap().wait().unwrap(), [4, 4]);
        assert_eq!(animate("u?size=1".into(), "a_x", true), "u?size=1&animated=true");
        assert_eq!(animate("u?size=1".into(), "x", true), "u?size=1", "only a_ hashes animate");
    }

    #[test]
    fn sprite_cells_crop_the_sheet() {
        let c = cache();
        assert_eq!(sprite_cell(&c, 0, 0).unwrap().wait().unwrap(), [64, 64]);
        assert!(sprite_cell(&c, 42 * 47, 0).is_err());
        assert!(sprite_cell(&c, 0, 3).is_ok());
    }
}
