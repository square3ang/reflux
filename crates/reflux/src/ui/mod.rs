//! Fluxer's design-system components, drawn with Rune to match `fluxer_app/src/features/ui`.
// Builder handler fields (`Option<Rc<dyn Fn(&mut S, &str)>>`) read better spelled out than aliased.
#![allow(clippy::type_complexity)]

pub mod avatar;
pub mod button;
pub mod controls;
pub mod datepicker;
pub mod gallery;
pub mod input;
pub mod mark;
pub mod menu;
pub mod misc;
pub mod modal;
pub mod overlay;
pub mod select;

use rune::Color;
use rune::color::{srgb_decode, srgb_encode};

pub use avatar::{Avatar, Status};
pub use button::{Button, ButtonSize, ButtonVariant, spinner};
pub use datepicker::DatePicker;
pub use controls::{Checkbox, RadioGroup, Slider, Switch, radio_dot};
pub use input::Input;
pub use menu::{Menu, MenuEntry, MenuItem, menu_slider};
pub use misc::{SpinnerSize, mention_badge, notification_dot, scroller};
pub use modal::{Modal, ModalSize};
pub use overlay::{Placement, PopupAccess, Popups, Tooltip, popup_access};
pub use select::Select;

/// Whether a palette is the light theme (its primary text is dark).
pub fn is_light(p: &crate::theme::Palette) -> bool {
    p.text_primary.r + p.text_primary.g + p.text_primary.b < 1.5
}

/// `top` composited over an opaque `bottom` in gamma-encoded sRGB, as browsers blend. Rune's border
/// replaces the fill under it, while CSS paints the background under the border; pre-blending keeps
/// a translucent CSS border looking the same.
pub fn over(top: Color, bottom: Color) -> Color {
    let ch = |t: f32, b: f32| srgb_decode(srgb_encode(t) * top.a + srgb_encode(b) * (1.0 - top.a));
    Color { r: ch(top.r, bottom.r), g: ch(top.g, bottom.g), b: ch(top.b, bottom.b), a: top.a + bottom.a * (1.0 - top.a) }
}

/// CSS `color-mix(in srgb, a p%, b)`: mixes gamma-encoded channels, as browsers do.
pub fn mix(a: Color, b: Color, p: f32) -> Color {
    let (ta, tb) = (p / 100.0, 1.0 - p / 100.0);
    let alpha = a.a * ta + b.a * tb;
    if alpha <= 0.0 {
        return Color::TRANSPARENT;
    }
    let ch = |x: f32, y: f32| srgb_decode((srgb_encode(x) * a.a * ta + srgb_encode(y) * b.a * tb) / alpha);
    Color { r: ch(a.r, b.r), g: ch(a.g, b.g), b: ch(a.b, b.b), a: alpha }
}

/// A colour from 8-bit sRGB channels.
pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    rgba(r, g, b, 1.0)
}

pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    let d = |v: u8| srgb_decode(v as f32 / 255.0);
    Color { r: d(r), g: d(g), b: d(b), a }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_matches_browser_srgb_mixing() {
        // color-mix(in srgb, rgb(65,60,221) 90%, #000) resolves to color(srgb 0.2304 0.2115 0.7785).
        let m = mix(rgb(65, 60, 221), Color::BLACK, 90.0);
        let e = [srgb_encode(m.r), srgb_encode(m.g), srgb_encode(m.b)];
        assert!((e[0] - 0.2294).abs() < 0.002 && (e[1] - 0.2118).abs() < 0.002 && (e[2] - 0.7800).abs() < 0.002, "{e:?}");
        let half = mix(Color::WHITE, Color::TRANSPARENT, 50.0);
        assert!((half.a - 0.5).abs() < 1e-6 && (half.r - 1.0).abs() < 1e-6);
    }

    #[test]
    fn over_blends_like_css_background_under_border() {
        // rgba(201,197,211,.15) over rgb(42,39,48) = rgb(65.85, 62.7, 72.45) in encoded sRGB.
        let c = over(rgba(201, 197, 211, 0.15), rgb(42, 39, 48));
        let e = [srgb_encode(c.r) * 255.0, srgb_encode(c.g) * 255.0, srgb_encode(c.b) * 255.0];
        assert!((e[0] - 65.85).abs() < 0.1 && (e[1] - 62.7).abs() < 0.1 && (e[2] - 72.45).abs() < 0.1, "{e:?}");
        assert!((c.a - 1.0).abs() < 1e-6);
    }
}
