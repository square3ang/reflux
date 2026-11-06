//! Media proxy URLs (`fluxer_docs/media-proxy/*`, `AvatarUtils.ts`, `MediaProxyUtils.ts`) and
//! the loader that fetches them for the views.
//!
//! Rune decodes PNG and JPEG on native (no WebP), so every URL asks for PNG where Fluxer asks
//! for WebP.
// shortcut: animated avatars, icons and emoji show their first frame; request `animated=true`
// once Rune plays GIF/APNG.

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
    format!("{}/{route}/{id}/{hash}.png?size={size}", base.trim_end_matches('/'))
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
    format!("{}/emojis/{id}.png", base.trim_end_matches('/'))
}

/// `buildFitInsideMediaProxyURL` for an attachment or embed `proxy_url`: keeps the issued query
/// (signatures) and asks for `width` device pixels as PNG; the proxy never enlarges.
pub fn fit_url(proxy_url: &str, css_w: f32, scale: f32) -> String {
    let sep = if proxy_url.contains('?') { '&' } else { '?' };
    format!("{proxy_url}{sep}format=png&width={}", (css_w * scale).ceil().max(1.0) as u32)
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
        let Some(cache) = self.0.cache.get() else { return Load::Pending };
        let mut e = self.0.entries.lock().unwrap_or_else(|e| e.into_inner());
        e.tick += 1;
        let tick = e.tick;
        if let Some((slot, used)) = e.map.get_mut(url) {
            *used = tick;
            return match slot {
                Slot::Pending => Load::Pending,
                Slot::Failed => Load::Failed,
                Slot::Ready(img) if img.error().is_some() => Load::Failed,
                Slot::Ready(img) if !img.is_ready() => Load::Pending,
                Slot::Ready(img) => Load::Ready(img.pixels().clone()),
            };
        }
        if e.map.len() >= CAP {
            let old = e.map.iter().filter(|(_, (s, _))| !matches!(s, Slot::Pending)).min_by_key(|(_, (_, u))| *u).map(|(k, _)| k.clone());
            if let Some(k) = old {
                e.map.remove(&k);
            }
        }
        let slot = match self.0.fixture.get() {
            Some(f) => f(cache, url).map_or(Slot::Failed, Slot::Ready),
            None => {
                self.fetch(url.to_owned());
                Slot::Pending
            }
        };
        e.map.insert(url.to_owned(), (slot, tick));
        drop(e);
        if self.0.fixture.get().is_some() { self.get(url) } else { Load::Pending }
    }

    fn fetch(&self, url: String) {
        let inner = self.0.clone();
        crate::app::spawn(async move {
            let res = async { inner.http.get(&url).send().await?.error_for_status()?.bytes().await }.await;
            let slot = match (res, inner.cache.get()) {
                (Ok(b), Some(cache)) => Slot::Ready(cache.load(b.to_vec())),
                (Err(err), _) => {
                    eprintln!("reflux: image {url}: {err}");
                    Slot::Failed
                }
                _ => Slot::Failed,
            };
            if let Some(entry) = inner.entries.lock().unwrap_or_else(|e| e.into_inner()).map.get_mut(&url) {
                entry.0 = slot;
            }
            // Content the cache already decoded doesn't wake the app, so ask for a frame.
            crate::app::update(|_| {});
        });
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
        assert_eq!(avatar_url(B, Snowflake(42), "abc", 40.0, 2.0), "https://media.fluxer.app/avatars/42/abc.png?size=128");
        // Animated hashes load the static frame.
        assert_eq!(avatar_url(B, Snowflake(42), "a_abc", 80.0, 2.0), "https://media.fluxer.app/avatars/42/abc.png?size=160");
        assert_eq!(guild_icon_url(B, Snowflake(7), "ff", 48.0, 1.0), "https://media.fluxer.app/icons/7/ff.png?size=128");
        assert_eq!(guild_banner_url(B, Snowflake(7), "bb", 2.0), "https://media.fluxer.app/banners/7/bb.png?size=1024");
        assert_eq!(emoji_url(B, "99"), "https://media.fluxer.app/emojis/99.png");
    }

    #[test]
    fn fit_urls_keep_the_signature() {
        assert_eq!(fit_url("https://m.x/attachments/1/2/a.webp", 550.0, 1.0), "https://m.x/attachments/1/2/a.webp?format=png&width=550");
        assert_eq!(
            fit_url("https://m.x/attachments/1/2/a.png?ex=1&is=2&hm=3", 275.5, 2.0),
            "https://m.x/attachments/1/2/a.png?ex=1&is=2&hm=3&format=png&width=551"
        );
    }
}
