mod palette;

use std::sync::LazyLock;

pub use palette::{Palette, dim};
use rune::tok::TextStyle;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    #[default]
    Dark,
    Light,
    Coal,
}

impl Kind {
    /// The palette for a user settings `theme` value. `dark_legacy` and `system` have no palette
    /// here yet and use dark.
    pub fn from_setting(value: &str) -> Kind {
        match value {
            "light" => Kind::Light,
            "coal" => Kind::Coal,
            _ => Kind::Dark,
        }
    }
}

static DARK: LazyLock<Palette> = LazyLock::new(palette::dark);
static LIGHT: LazyLock<Palette> = LazyLock::new(palette::light);
static COAL: LazyLock<Palette> = LazyLock::new(palette::coal);

pub fn palette(kind: Kind) -> &'static Palette {
    match kind {
        Kind::Dark => &DARK,
        Kind::Light => &LIGHT,
        Kind::Coal => &COAL,
    }
}

pub const FONTS: &[&[u8]] = &[
    include_bytes!("../../../../assets/fonts/FluxerSans-Regular.ttf"),
    include_bytes!("../../../../assets/fonts/FluxerSans-Medium.ttf"),
    include_bytes!("../../../../assets/fonts/FluxerSans-SemiBold.ttf"),
    include_bytes!("../../../../assets/fonts/FluxerSans-Bold.ttf"),
    include_bytes!("../../../../assets/fonts/FluxerSans-Italic.ttf"),
    include_bytes!("../../../../assets/fonts/FluxerMono-Regular.ttf"),
    include_bytes!("../../../../assets/fonts/FluxerMono-Bold.ttf"),
];
pub const FAMILIES: (&str, &str) = ("Fluxer Sans", "Fluxer Mono");

/// A Fluxer Sans style at a CSS pixel size and weight.
pub const fn font(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight)
}
