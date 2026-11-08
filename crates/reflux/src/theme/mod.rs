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
];
pub const FAMILIES: (&str, &str) = ("Fluxer Sans", "Fluxer Mono");

/// The script faces of Fluxer's `--font-sans` chain, in its base order, by family and file stem.
/// They're 100+ MB, so `scripts/fonts-extract.py` merges them into a `fonts` folder shipped
/// beside the binary rather than built in, and they're memory-mapped: only pages text uses load.
const SCRIPTS: [(&str, &str); 8] = [
    ("Fluxer Sans Arabic", "FluxerSansArabic"),
    ("Fluxer Sans Hebrew", "FluxerSansHebrew"),
    ("Fluxer Sans Devanagari", "FluxerSansDevanagari"),
    ("Fluxer Sans Thai Looped", "FluxerSansThaiLooped"),
    ("Fluxer Sans SC", "FluxerSansSC"),
    ("Fluxer Sans TC", "FluxerSansTC"),
    ("Fluxer Sans JP", "FluxerSansJP"),
    ("Fluxer Sans KR", "FluxerSansKR"),
];
const WEIGHTS: [&str; 4] = ["Regular", "Medium", "SemiBold", "Bold"];

/// The `fonts` folder beside the executable, else the repository's `assets/fonts` (a build
/// run from `target/`).
fn font_dir() -> Option<std::path::PathBuf> {
    let beside = std::env::current_exe().ok()?.parent()?.join("fonts");
    let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
    [beside, dev].into_iter().find(|d| d.join("FluxerSansKR-Regular.ttf").is_file())
}

/// The script faces found, mapped for the life of the process.
static SCRIPT_FONTS: LazyLock<Vec<&'static [u8]>> = LazyLock::new(|| {
    let Some(dir) = font_dir() else {
        eprintln!("reflux: no fonts folder; CJK, Arabic, Hebrew, Devanagari and Thai text use system fonts");
        return Vec::new();
    };
    let map = |path: std::path::PathBuf| -> Option<&'static [u8]> {
        let file = std::fs::File::open(&path).inspect_err(|e| eprintln!("reflux: font {}: {e}", path.display())).ok()?;
        // SAFETY: the font files are ours and not written while the app runs.
        let m = unsafe { memmap2::Mmap::map(&file) }.inspect_err(|e| eprintln!("reflux: font {}: {e}", path.display())).ok()?;
        Some(&**Box::leak(Box::new(m)))
    };
    let dir = &dir;
    SCRIPTS.iter().flat_map(|(_, stem)| WEIGHTS.iter().map(move |w| dir.join(format!("{stem}-{w}.ttf")))).filter_map(map).collect()
});

/// [`FONTS`] and the script faces.
pub fn app_fonts() -> &'static [&'static [u8]] {
    static ALL: LazyLock<&'static [&'static [u8]]> = LazyLock::new(|| Vec::leak(FONTS.iter().copied().chain(SCRIPT_FONTS.iter().copied()).collect()));
    *ALL
}

/// The sans slot's base, `--font-sans` (`font-fallback.css`) for the locale: Fluxer Sans, the
/// locale's script face first and the rest in their base order, then `--font-fallback-sans`'s
/// Noto Sans CJK for what none of them draws.
/// shortcut: the rest of `--font-fallback-sans` and `system-ui` are left to Rune's own fallbacks.
pub fn sans_family() -> String {
    let first = match crate::i18n::locale() {
        l if l.starts_with("ja") => "Fluxer Sans JP",
        l if l.starts_with("ko") => "Fluxer Sans KR",
        "zh-CN" => "Fluxer Sans SC",
        l if l.starts_with("zh") => "Fluxer Sans TC",
        l if l.starts_with("ar") => "Fluxer Sans Arabic",
        l if l.starts_with("he") => "Fluxer Sans Hebrew",
        l if ["hi", "mr", "ne", "sa"].iter().any(|p| l.starts_with(p)) => "Fluxer Sans Devanagari",
        l if l.starts_with("th") => "Fluxer Sans Thai Looped",
        _ => "",
    };
    let mut out = vec![FAMILIES.0];
    out.extend((!first.is_empty()).then_some(first));
    out.extend(SCRIPTS.iter().map(|(f, _)| *f).filter(|f| *f != first));
    out.extend(["Noto Sans CJK SC", "Noto Sans CJK TC", "Noto Sans CJK JP", "Noto Sans CJK KR"]);
    out.join(", ")
}

/// A Fluxer Sans style at a CSS pixel size and weight.
pub const fn font(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight)
}

#[cfg(test)]
mod font_tests {
    /// `█` is Fluxer Sans SC's full-width square, as in Fluxer, not DejaVu Sans' narrower one
    /// (needs the `fonts` folder).
    #[test]
    fn block_is_full_width() {
        let fam = super::sans_family();
        let mut ts = rune::text::TextSystem::new(super::app_fonts(), (&fam, super::FAMILIES.1));
        let (w, _) = ts.measure("\u{2588}", super::font(16.0, 500.0), 1.0);
        assert!((w - 16.0).abs() < 0.5, "{w}");
    }
}
