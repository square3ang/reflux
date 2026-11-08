pub mod custom;
mod palette;

use std::sync::LazyLock;

pub use palette::{NAMES, Palette, dim};
use rune::tok::TextStyle;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    #[default]
    Dark,
    Light,
    Coal,
}

impl Kind {
    /// The base palette for a user settings `theme` value (`dark_legacy` and `system` are
    /// resolved by [`custom::effective`]).
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
    // Fluxer Sans SC's symbols (blocks, box drawing, arrows, shapes, full-width punctuation),
    // without its ideographs: in Fluxer's chain they come before any system font, so `█` is
    // SC's full-width square. `scripts/fonts-extract.py`.
    include_bytes!("../../../../assets/fonts/FluxerSansSC-Symbols-Regular.ttf"),
    include_bytes!("../../../../assets/fonts/FluxerSansSC-Symbols-Medium.ttf"),
    include_bytes!("../../../../assets/fonts/FluxerSansSC-Symbols-SemiBold.ttf"),
    include_bytes!("../../../../assets/fonts/FluxerSansSC-Symbols-Bold.ttf"),
];
pub const FAMILIES: (&str, &str) = ("Fluxer Sans", "Fluxer Mono");

/// Fluxer Sans KR's weights, read from a `fonts` folder at startup rather than built in
/// (the four come to ~10 MB): the merged `FluxerSansKR-*.woff2` subsets of `@fluxer/fonts`.
const KR_FILES: [&str; 4] = ["FluxerSansKR-Regular.ttf", "FluxerSansKR-Medium.ttf", "FluxerSansKR-SemiBold.ttf", "FluxerSansKR-Bold.ttf"];

/// The `fonts` folder beside the executable, else the repository's `assets/fonts` (a build
/// run from `target/`).
fn font_dir() -> Option<std::path::PathBuf> {
    let beside = std::env::current_exe().ok()?.parent()?.join("fonts");
    let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
    [beside, dev].into_iter().find(|d| d.join(KR_FILES[0]).is_file())
}

/// Fluxer Sans KR's faces, empty when the folder is missing.
static KR: LazyLock<Vec<&'static [u8]>> = LazyLock::new(|| {
    let Some(dir) = font_dir() else {
        eprintln!("reflux: no fonts folder with Fluxer Sans KR; Korean text uses system fonts");
        return Vec::new();
    };
    KR_FILES.iter().filter_map(|f| std::fs::read(dir.join(f)).inspect_err(|e| eprintln!("reflux: font {f}: {e}")).ok()).map(|b| &*Vec::leak(b)).collect()
});

/// [`FONTS`] and the fonts read from the `fonts` folder.
pub fn app_fonts() -> &'static [&'static [u8]] {
    static ALL: LazyLock<&'static [&'static [u8]]> = LazyLock::new(|| Vec::leak(FONTS.iter().copied().chain(KR.iter().copied()).collect()));
    *ALL
}

/// The sans slot's base, after `--font-sans` (`font-fallback.css`): Fluxer Sans, the locale's
/// Fluxer CJK face first, the others, then `--font-fallback-sans`'s Noto Sans CJK. Only KR and
/// SC's symbols ship, so the system Noto Sans CJK stands in for Fluxer Sans SC/TC/JP, in the
/// locale's order (so `ja` Han stays Japanese).
/// shortcut: Fluxer Sans SC/TC/JP/Arabic/Hebrew/Devanagari/Thai don't ship; system fonts stand in.
pub fn sans_family() -> String {
    let loc = crate::i18n::locale();
    let first = match loc {
        l if l.starts_with("ko") => "KR",
        l if l.starts_with("ja") => "JP",
        "zh-TW" | "zh-HK" => "TC",
        _ => "SC",
    };
    let mut out = String::from(FAMILIES.0);
    if first == "KR" && !KR.is_empty() {
        out.push_str(", Fluxer Sans KR");
    }
    out.push_str(", Fluxer Sans SC");
    for r in std::iter::once(first).chain(["SC", "TC", "JP", "KR"].into_iter().filter(|r| *r != first)) {
        out.push_str(&format!(", Noto Sans CJK {r}"));
    }
    // Elsewhere after Noto Sans CJK, so its Hanja don't take Chinese or Japanese Han.
    if first != "KR" && !KR.is_empty() {
        out.push_str(", Fluxer Sans KR");
    }
    out
}

/// A Fluxer Sans style at a CSS pixel size and weight.
pub const fn font(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight)
}

#[cfg(test)]
mod font_tests {
    /// `█` is Fluxer Sans SC's full-width square, as in Fluxer, not DejaVu Sans' narrower one.
    #[test]
    fn block_is_full_width() {
        let fam = super::sans_family();
        let mut ts = rune::text::TextSystem::new(super::app_fonts(), (&fam, super::FAMILIES.1));
        let (w, _) = ts.measure("\u{2588}", super::font(16.0, 500.0), 1.0);
        assert!((w - 16.0).abs() < 0.5, "{w}");
    }
}
