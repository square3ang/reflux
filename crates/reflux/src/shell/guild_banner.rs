//! The community banner behind the sidebar header, collapsing as the channel list scrolls
//! (`GuildBannerCollapse.ts`, `useGuildBannerPresentation`, `GuildHeader`): the list starts
//! with a spacer as tall as the banner overhang; scrolling it slides the banner's clip up (the
//! image stays put, so it's cut from the bottom) and fades it out, while a frosted header layer
//! fades in over the first 16 points and the name turns from white to the text colour.

use rune::{El, Live, ScrollPos, el};

/// `GUILD_BANNER_MAX_VIEWPORT_HEIGHT_FRACTION`, `GUILD_BANNER_HEADER_GLASS_MAX_OPACITY`,
/// `GUILD_BANNER_HEADER_GLASS_DISTANCE`.
const MAX_VIEWPORT: f32 = 0.3;
pub const GLASS_MAX: f32 = 0.72;
const GLASS_DISTANCE: f32 = 16.0;

/// `resolveGuildBannerGeometry`: the banner's height and how far it collapses.
pub fn geometry(width: f32, viewport_h: f32, header_h: f32, ratio: f32) -> (f32, f32) {
    if width <= 0.0 || ratio <= 0.0 {
        return (header_h, 0.0);
    }
    let cap = if viewport_h > 0.0 { viewport_h * MAX_VIEWPORT } else { f32::INFINITY };
    let h = header_h.max((width / ratio).min(cap));
    (h, (h - header_h).max(0.0))
}

/// `resolveGuildBannerCollapseProgress`.
pub fn progress(scroll: f32, dist: f32) -> f32 {
    if dist <= 0.0 { 0.0 } else { (scroll / dist).clamp(0.0, 1.0) }
}

/// `resolveGuildBannerHeaderGlassOpacity`.
pub fn glass(scroll: f32) -> f32 {
    if scroll <= 0.0 { 0.0 } else { (scroll / GLASS_DISTANCE * GLASS_MAX).min(GLASS_MAX) }
}

/// The values the scroll drives, bound to the banner and header layers.
#[derive(Clone)]
pub struct Lives {
    pub clip: Live<[f32; 2]>,
    pub image: Live<[f32; 2]>,
    pub image_opacity: Live<f32>,
    pub glass: Live<f32>,
    pub scrim: Live<f32>,
    /// 1 while the name is on the banner (white), 0 once the header is frosted.
    pub on_banner: Live<f32>,
    pub frosted: Live<f32>,
    dist: f32,
}

impl Lives {
    pub fn new(dist: f32) -> Self {
        let l = Self {
            clip: Live::new([0.0, 0.0]),
            image: Live::new([0.0, 0.0]),
            image_opacity: Live::new(1.0),
            glass: Live::new(0.0),
            scrim: Live::new(1.0),
            on_banner: Live::new(1.0),
            frosted: Live::new(0.0),
            dist,
        };
        l.at(0.0);
        l
    }

    /// `syncToScroll`.
    pub fn at(&self, scroll: f32) {
        let k = progress(scroll, self.dist);
        let shift = -k * self.dist;
        self.clip.set([0.0, shift]);
        self.image.set([0.0, -shift]);
        self.image_opacity.set(1.0 - k);
        let g = if self.dist > 0.0 { glass(scroll) } else { 0.0 };
        self.glass.set(g);
        self.scrim.set(1.0 - g / GLASS_MAX);
        let frosted = self.dist > 0.0 && g >= GLASS_MAX;
        self.on_banner.set(if frosted { 0.0 } else { 1.0 });
        self.frosted.set(if frosted { 1.0 } else { 0.0 });
    }

    /// For the channel list's viewport.
    pub fn follow(&self) -> impl Fn(ScrollPos) + 'static {
        let l = self.clone();
        move |at: ScrollPos| l.at(at.offset[1])
    }

    /// `.bannerClip` around the image: pinned at the top, `h` tall, sliding up and fading.
    pub fn banner(&self, image: El, w: f32, h: f32) -> El {
        el().pin(Some(0.0), Some(0.0), Some(0.0), None)
            .size(w, h)
            .clip_rounded(0.0)
            .live_offset(&self.clip)
            .live_opacity(&self.image_opacity)
            .child(image.live_offset(&self.image))
    }

    /// The list's leading spacer (`.bannerSpacer`).
    pub fn spacer(&self) -> El {
        el().w_full().h(self.dist)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_guild_banner_collapse() {
        // 240 wide at 16:9 is 135 tall, under 30% of an 800 window; 79 to collapse.
        assert_eq!(geometry(240.0, 800.0, 56.0, 16.0 / 9.0), (135.0, 79.0));
        assert_eq!(geometry(240.0, 300.0, 56.0, 16.0 / 9.0).0, 90.0, "capped at 30% of the window");
        assert_eq!(geometry(240.0, 100.0, 56.0, 16.0 / 9.0), (56.0, 0.0), "never below the header");
        assert_eq!(progress(40.0, 80.0), 0.5);
        assert_eq!(progress(400.0, 80.0), 1.0);
        assert_eq!(glass(8.0), 0.36);
        assert_eq!(glass(100.0), GLASS_MAX);
        let l = Lives::new(80.0);
        l.at(40.0);
        assert_eq!((l.clip.get(), l.image.get(), l.image_opacity.get()), ([0.0, -40.0], [0.0, 40.0], 0.5));
        assert_eq!((l.on_banner.get(), l.frosted.get()), (0.0, 1.0));
        l.at(4.0);
        assert_eq!(l.on_banner.get(), 1.0);
    }
}
