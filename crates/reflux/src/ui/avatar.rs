//! `<BaseAvatar>` (`fluxer_app/src/features/ui/components/BaseAvatar.*`, status geometry from
//! `scripts/GenerateAvatarMasks.ts`).
//!
//! Fluxer masks the avatar with an SVG notch so the status shows the page through a ring. Rune
//! has no masks, so the notch and the status's holes are painted in `surface`, the colour
//! behind the avatar: identical on a solid background.

use rune::{Color, El, LoopSpec, Pixels, el, row};

use super::rgba;
use crate::theme::Palette;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Online,
    Idle,
    Dnd,
    /// Also invisible, which renders as offline.
    Offline,
}

/// `getStatusGeometry`: (status size, cutout radius, cutout centre) for an avatar size, from
/// `STATUS_CONFIG`, interpolated between its sizes and scaled past its ends.
pub fn status_geometry(size: f32) -> (f32, f32, f32) {
    const G: f32 = 0.2; // LARGE_STATUS_CUTOUT_GUTTER_RATIO
    const T: [(f32, [f32; 3]); 11] = [
        (16.0, [10.0, 5.0, 13.0]),
        (20.0, [10.0, 6.0, 17.0]),
        (24.0, [10.0, 7.0, 20.0]),
        (32.0, [10.0, 8.0, 27.0]),
        (36.0, [10.0, 8.0, 30.0]),
        (40.0, [12.0, 9.0, 34.0]),
        (44.0, [14.0, 10.0, 38.0]),
        (48.0, [14.0, 10.0, 42.0]),
        (56.0, [16.0, 11.0, 49.0]),
        (80.0, [16.0, 8.0 + 16.0 * G, 68.0]),
        (120.0, [24.0, 12.0 + 24.0 * G, 100.0]),
    ];
    let tuple = |v: [f32; 3], k: f32| (v[0] * k, v[1] * k, v[2] * k);
    if size <= T[0].0 {
        return tuple(T[0].1, size / T[0].0);
    }
    if size >= T[10].0 {
        return tuple(T[10].1, size / T[10].0);
    }
    let i = T.iter().position(|(s, _)| *s >= size).unwrap();
    let ((s0, a), (s1, b)) = (T[i - 1], T[i]);
    let t = (size - s0) / (s1 - s0);
    let l = |j: usize| a[j] + (b[j] - a[j]) * t;
    (l(0), l(1), l(2))
}

pub struct Avatar<'a> {
    size: f32,
    pixels: Option<&'a Pixels>,
    status: Option<Status>,
    mobile: bool,
    typing: bool,
    clickable: bool,
    surface: Option<Color>,
}

impl<'a> Avatar<'a> {
    /// Sizes the app uses: 16 20 24 32 36 40 44 48 56 80 120.
    pub fn new(size: f32) -> Self {
        Self { size, pixels: None, status: None, mobile: false, typing: false, clickable: false, surface: None }
    }
    /// The decoded avatar; without one the skeleton placeholder (`background-secondary-alt`) shows.
    pub fn pixels(mut self, p: Option<&'a Pixels>) -> Self { self.pixels = p; self }
    pub fn status(mut self, s: Option<Status>) -> Self { self.status = s; self }
    /// Online from a phone: a phone glyph instead of the dot.
    pub fn mobile(mut self, m: bool) -> Self { self.mobile = m; self }
    pub fn typing(mut self, t: bool) -> Self { self.typing = t; self }
    pub fn clickable(mut self, c: bool) -> Self { self.clickable = c; self }
    /// The colour behind the avatar, painted into the status notch (default `background-primary`).
    pub fn surface(mut self, c: Color) -> Self { self.surface = Some(c); self }

    pub fn el(self, p: &Palette) -> El {
        let n = self.size;
        let mut face = el().size(n, n).radius(n / 2.0);
        face = match self.pixels {
            Some(px) => face.pixels(px),
            None => face.fill(p.background_secondary_alt),
        };
        let mut root = el().size(n, n).child(face).a11y_hidden();
        if self.clickable {
            // `.clickable:hover` dims the image (opacity .9) and fades a black overlay to .4.
            root = root.hover(rgba(0, 0, 0, 0.4)).radius(n / 2.0).cursor(rune::Cursor::Pointer);
        }
        let Some(status) = self.status.or(self.typing.then_some(Status::Online)) else { return root };
        if n <= 16.0 {
            return root;
        }
        let surface = self.surface.unwrap_or(p.background_primary);
        let colour = match status {
            Status::Online => p.status_online,
            Status::Idle => p.status_idle,
            Status::Dnd => p.status_dnd,
            Status::Offline => p.status_offline,
        };
        let (s, r, c) = status_geometry(n);
        let border = r - s / 2.0;
        // Absolute box of (w, h) centred on the cutout centre.
        let at = |w: f32, h: f32| el().absolute(c - w / 2.0, c - h / 2.0).size(w, h);

        if self.typing {
            let tw = (s * 2.5).round();
            let ext = (tw - s).max(0.0);
            let dot = (s * 0.5).round();
            let gap = (s * 0.125).round();
            let dots = row().center().gap(gap).children((0..3).map(|i| {
                el().size(dot, dot).radius(dot / 2.0).fill(Color::WHITE).looping(LoopSpec::opacity(1.0, 0.25 * i as f64, 0.0, 1.0))
            }));
            return root
                .child(at(ext + 2.0 * r, 2.0 * r).radius(r).fill(surface).key("typing-notch"))
                .child(row().absolute(c - tw / 2.0, c - s / 2.0).size(tw, s).radius(s / 2.0).fill(colour).center().child(dots).key("typing"));
        }

        if self.mobile && status == Status::Online {
            let ph = ((s / 0.75 + 2.0) / 2.0).round() * 2.0;
            let rx = (s * 0.12).round();
            let screen = el().absolute(0.14 * s, 0.06 * ph).size(0.72 * s, 0.525 * ph).radius(0.04 * s).fill(surface);
            let wheel = el().absolute(0.37 * s, 0.83 * ph - 0.13 * s).size(0.26 * s, 0.26 * s).radius(0.13 * s).fill(surface);
            return root
                .child(at(s + 2.0 * border, ph + 2.0 * border).radius(rx + border).fill(surface).key("mobile-notch"))
                .child(at(s, ph).radius(rx).fill(colour).child(screen).child(wheel).key("mobile"));
        }

        let _ = colour;
        root.child(at(2.0 * r, 2.0 * r).radius(r).fill(surface).key("notch")).child(status_indicator(status, s, surface, p).absolute(c - s / 2.0, c - s / 2.0).key("status"))
    }
}

/// `StatusIndicator`: the presence dot `s` wide, its `flx-mask-presence-*` hole cut in `surface`.
pub fn status_indicator(status: Status, s: f32, surface: Color, p: &Palette) -> El {
    let colour = match status {
        Status::Online => p.status_online,
        Status::Idle => p.status_idle,
        Status::Dnd => p.status_dnd,
        Status::Offline => p.status_offline,
    };
    let hole = match status {
        Status::Online => None,
        Status::Idle => Some(el().absolute(-0.125 * s, -0.125 * s).size(0.75 * s, 0.75 * s).radius(0.375 * s)),
        Status::Dnd => Some(el().absolute(0.125 * s, 0.375 * s).size(0.75 * s, 0.25 * s).radius(0.125 * s)),
        Status::Offline => Some(el().absolute(0.25 * s, 0.25 * s).size(0.5 * s, 0.5 * s).radius(0.25 * s)),
    };
    let mut dot = el().size(s, s).radius(s / 2.0).fill(colour).clip_rounded(s / 2.0);
    if let Some(h) = hole {
        dot = dot.child(h.fill(surface));
    }
    dot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_geometry_matches_fluxer_table() {
        assert_eq!(status_geometry(32.0), (10.0, 8.0, 27.0));
        let (s, r, c) = status_geometry(80.0);
        assert_eq!((s, c), (16.0, 68.0));
        assert!((r - 11.2).abs() < 1e-4);
        // Between 48 and 56: halfway.
        assert_eq!(status_geometry(52.0), (15.0, 10.5, 45.5));
        // Past the ends: scaled.
        assert_eq!(status_geometry(8.0), (5.0, 2.5, 6.5));
        let (s, _, c) = status_geometry(240.0);
        assert_eq!((s, c), (48.0, 200.0));
    }
}
