//! The palette actually drawn: the base theme (`dark_legacy` built like Fluxer's
//! `GenerateColorSystem.ts` does, `system` following the OS), the accessibility saturation factor,
//! and the theme variable overrides in quick CSS (`:root { --text-link: #f0f; }`, what Fluxer's
//! "Colors and fonts" editor writes). Rune draws no CSS, so other rules are ignored and counted.

use rune::Color;
use rune::color::{srgb_decode, srgb_encode};

use super::{Kind, Palette, palette};
use crate::ui::mix;

/// The palette for a `theme` setting, the OS appearance, the saturation factor and quick CSS.
pub fn effective(setting: &str, system_dark: bool, saturation: f32, css: &str) -> Palette {
    let mut p = match setting {
        "dark_legacy" => legacy(),
        "system" => *palette(if system_dark { Kind::Dark } else { Kind::Light }),
        s => *palette(Kind::from_setting(s)),
    };
    for (name, value) in overrides(css).0 {
        if let (Some(slot), Some(c)) = (p.get_mut(&name), parse_color(&value)) {
            *slot = c;
        }
    }
    if saturation < 1.0 {
        desaturate(&mut p, saturation.max(0.0));
    }
    p
}

/// `.theme-dark_legacy`: the dark palette with Fluxer's legacy (blue-grey, hue 220) surfaces,
/// text and accents.
pub fn legacy() -> Palette {
    let mut p = *palette(Kind::Dark);
    // (scale range, curve, stops), as `legacyDarkSurface` / `legacyDarkText`.
    let surfaces: &[(&str, f32)] = &[
        ("--background-primary", 0.0),
        ("--background-secondary", 0.16),
        ("--background-secondary-lighter", 0.22),
        ("--background-secondary-alt", 0.28),
        ("--background-tertiary", 0.4),
        ("--background-channel-header", 0.34),
        ("--guild-list-foreground", 0.38),
        ("--background-header-secondary", 0.5),
        ("--background-header-primary", 0.5),
        ("--background-textarea", 0.3),
        ("--background-header-primary-hover", 0.85),
    ];
    let texts: &[(&str, f32)] = &[
        ("--text-tertiary-secondary", 0.0),
        ("--text-tertiary-muted", 0.2),
        ("--text-tertiary", 0.38),
        ("--text-primary-muted", 0.55),
        ("--text-chat-muted", 0.55),
        ("--text-secondary", 0.72),
        ("--text-chat", 0.82),
        ("--text-primary", 1.0),
    ];
    let mut set = |name: &str, c: Color| {
        if let Some(slot) = p.get_mut(name) {
            *slot = c;
        }
    };
    let ease_out = |t: f32| 1.0 - (1.0 - t) * (1.0 - t);
    let ease_in_out = |t: f32| if t < 0.5 { 2.0 * t * t } else { 1.0 - 2.0 * (1.0 - t) * (1.0 - t) };
    for &(n, t) in surfaces {
        set(n, hsl(220.0, 13.0, 5.0 + 21.0 * ease_out(t), 1.0));
    }
    for &(n, t) in texts {
        set(n, hsl(220.0, 13.0, 52.0 + 44.0 * ease_in_out(t), 1.0));
    }
    let tones: &[(&str, [f32; 4])] = &[
        ("--panel-control-border", [220.0, 30.0, 65.0, 0.45]),
        ("--panel-control-divider", [220.0, 30.0, 55.0, 0.35]),
        ("--background-modifier-hover", [220.0, 13.0, 100.0, 0.05]),
        ("--background-modifier-selected", [220.0, 13.0, 100.0, 0.1]),
        ("--background-modifier-accent", [220.0, 13.0, 80.0, 0.15]),
        ("--background-modifier-accent-focus", [220.0, 13.0, 80.0, 0.22]),
        ("--control-button-hover-bg", [220.0, 13.0, 22.0, 1.0]),
        ("--control-button-active-bg", [220.0, 13.0, 24.0, 1.0]),
        ("--status-online", [142.0, 76.0, 40.0, 1.0]),
        ("--text-link", [210.0, 100.0, 70.0, 1.0]),
        ("--text-selection", [210.0, 90.0, 70.0, 0.35]),
        ("--markup-mention-border", [210.0, 100.0, 70.0, 0.3]),
        ("--border-color", [220.0, 13.0, 50.0, 0.2]),
        ("--border-color-hover", [220.0, 13.0, 50.0, 0.3]),
        ("--border-color-focus", [210.0, 90.0, 70.0, 0.45]),
        ("--accent-purple", [270.0, 80.0, 65.0, 1.0]),
        ("--alert-note-color", [210.0, 100.0, 70.0, 1.0]),
        ("--alert-tip-color", [142.0, 76.0, 45.0, 1.0]),
        ("--alert-important-color", [270.0, 80.0, 65.0, 1.0]),
    ];
    for &(n, [h, s, l, a]) in tones {
        set(n, hsl(h, s, l, a));
    }
    // The code tokens mix the surfaces and text above (`DARK_CODE_TOKENS` + the legacy overrides).
    p.code_text = mix(p.text_secondary, hsl(340.0, 50.0, 90.0, 1.0), 82.0);
    p.text_code = p.code_text;
    p.code_inline_bg = mix(p.background_secondary_alt, p.text_code, 82.0);
    p.bg_code = p.code_inline_bg;
    p.code_block_bg = mix(p.background_secondary_alt, p.background_primary, 88.0);
    p.bg_code_block = p.code_block_bg;
    p
}

/// CSS `hsl()` (degrees, percents) to a linear colour.
pub fn hsl(h: f32, s: f32, l: f32, a: f32) -> Color {
    let (s, l) = (s / 100.0, l / 100.0);
    let k = |n: f32| (n + h / 30.0).rem_euclid(12.0);
    let c = s * l.min(1.0 - l);
    let f = |n: f32| l - c * (k(n) - 3.0).min(9.0 - k(n)).clamp(-1.0, 1.0);
    Color { r: srgb_decode(f(0.0)), g: srgb_decode(f(8.0)), b: srgb_decode(f(4.0)), a }
}

/// `--saturation-factor`: Fluxer scales the HSL saturation of every token by it; on resolved
/// colours that's the same as scaling each colour's own HSL saturation.
fn desaturate(p: &mut Palette, factor: f32) {
    for name in super::palette::NAMES {
        if let Some(c) = p.get_mut(name) {
            *c = saturate(*c, factor);
        }
    }
}

pub fn saturate(c: Color, factor: f32) -> Color {
    let [r, g, b] = [srgb_encode(c.r), srgb_encode(c.g), srgb_encode(c.b)];
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    if d <= f32::EPSILON {
        return c;
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    hsl(h, s * factor * 100.0, l * 100.0, c.a)
}

/// The theme variable declarations in `:root { … }` blocks of `css` (`extractThemeVariableOverrides`),
/// and how many other rules and declarations there are (they can't apply to Rune).
pub fn overrides(css: &str) -> (Vec<(String, String)>, usize) {
    let mut out = Vec::new();
    let mut ignored = 0;
    let css = strip_comments(css);
    let mut rest = css.as_str();
    while let Some(open) = rest.find('{') {
        let selector = rest[..open].trim();
        let Some(close) = rest[open..].find('}') else { break };
        let body = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];
        if selector.rsplit(['}', ';']).next().unwrap_or("").trim() != ":root" {
            ignored += 1;
            continue;
        }
        for decl in body.split(';') {
            let Some((name, value)) = decl.split_once(':') else { continue };
            let (name, value) = (name.trim(), value.trim());
            let value = value.strip_suffix("!important").unwrap_or(value).trim();
            if FONTS.contains(&name) {
                continue;
            }
            if name.starts_with("--") && super::palette::NAMES.contains(&name) && parse_color(value).is_some() {
                out.retain(|(n, _): &(String, String)| n != name);
                out.push((name.to_string(), value.to_string()));
            } else if !name.is_empty() {
                ignored += 1;
            }
        }
    }
    (out, ignored)
}

/// The font tokens Rune can follow (`--font-sans`, `--font-mono` in `globals.css`), through
/// `Tx::set_font_families`.
pub const FONTS: [&str; 2] = ["--font-sans", "--font-mono"];

/// The `:root` values of [`FONTS`] in `css` (the last wins).
pub fn fonts(css: &str) -> [Option<String>; 2] {
    let css = strip_comments(css);
    let mut out = [None, None];
    let mut rest = css.as_str();
    while let Some(open) = rest.find('{') {
        let selector = rest[..open].trim();
        let Some(close) = rest[open..].find('}') else { break };
        let body = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];
        if selector.rsplit(['}', ';']).next().unwrap_or("").trim() != ":root" {
            continue;
        }
        for decl in body.split(';') {
            let Some((name, value)) = decl.split_once(':') else { continue };
            let value = value.trim();
            let value = value.strip_suffix("!important").unwrap_or(value).trim();
            if let Some(i) = FONTS.iter().position(|f| *f == name.trim()).filter(|_| !value.is_empty()) {
                out[i] = Some(value.to_string());
            }
        }
    }
    out
}

/// The families Rune draws with: an override first, falling back to Fluxer's bundled fonts.
pub fn families(css: &str) -> (String, String) {
    let [sans, mono] = fonts(css);
    let with = |o: Option<String>, base: &str| o.map_or(base.to_string(), |o| format!("{o}, {base}"));
    (with(sans, &super::sans_family()), with(mono, super::FAMILIES.1))
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        rest = rest[i + 2..].find("*/").map_or("", |j| &rest[i + 2 + j + 2..]);
    }
    out.push_str(rest);
    out
}

/// `updateCssForVariable`: sets (or with `None` removes) one variable, keeping the rest of the CSS.
pub fn set_var(css: &str, name: &str, value: Option<&str>) -> String {
    let mut out = String::new();
    let mut done = false;
    let mut rest = css;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        let (head, body) = (&rest[..open + 1], &rest[open + 1..open + close]);
        out.push_str(head);
        if head[..open].rsplit(['}', ';']).next().unwrap_or("").trim() == ":root" {
            let decls: Vec<String> = body
                .split(';')
                .filter(|d| !d.trim().is_empty())
                .filter_map(|d| match d.split_once(':') {
                    Some((n, _)) if n.trim() == name => match value.filter(|_| !done) {
                        Some(v) => {
                            done = true;
                            Some(format!(" {name}: {v}"))
                        }
                        None => None,
                    },
                    _ => Some(d.to_string()),
                })
                .collect();
            out.push_str(&decls.iter().map(|d| format!("{d};")).collect::<String>());
            out.push(' ');
        } else {
            out.push_str(body);
        }
        out.push('}');
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    // Drop emptied `:root {}` blocks.
    let mut out = out.replace(":root { }", "").replace(":root {  }", "").replace(":root {}", "");
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    let mut out = out.trim().to_string();
    if let (Some(v), false) = (value, done) {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!(":root {{ {name}: {v}; }}"));
    }
    out
}

/// A CSS colour: `#rgb[a]`, `#rrggbb[aa]`, `rgb[a](…)`, `hsl[a](…)`, `transparent`, `white`, `black`.
pub fn parse_color(v: &str) -> Option<Color> {
    let v = v.trim();
    if v.starts_with('#') {
        return match v.len() {
            5 => {
                let a = u8::from_str_radix(&v[4..5], 16).ok()? as f32 * 17.0 / 255.0;
                Some(Color { a, ..rune::color::hex(&v[..4])? })
            }
            _ => rune::color::hex(v),
        };
    }
    match v.to_ascii_lowercase().as_str() {
        "transparent" => return Some(Color::TRANSPARENT),
        "white" => return Some(Color::WHITE),
        "black" => return Some(Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }),
        _ => {}
    }
    let (func, args) = v.split_once('(')?;
    let args = args.strip_suffix(')')?;
    let parts: Vec<&str> = args.split([',', ' ', '/']).filter(|s| !s.is_empty()).collect();
    if parts.len() < 3 {
        return None;
    }
    let num = |s: &str| s.trim_end_matches(['%', 'g', 'd', 'e']).parse::<f32>().ok();
    let alpha = match parts.get(3) {
        Some(a) if a.ends_with('%') => num(a)? / 100.0,
        Some(a) => num(a)?,
        None => 1.0,
    };
    match func.trim().to_ascii_lowercase().as_str() {
        "rgb" | "rgba" => {
            let ch = |s: &str| -> Option<f32> { Some(if s.ends_with('%') { num(s)? / 100.0 } else { num(s)? / 255.0 }) };
            let (r, g, b) = (ch(parts[0])?, ch(parts[1])?, ch(parts[2])?);
            Some(Color { r: srgb_decode(r.clamp(0.0, 1.0)), g: srgb_decode(g.clamp(0.0, 1.0)), b: srgb_decode(b.clamp(0.0, 1.0)), a: alpha.clamp(0.0, 1.0) })
        }
        "hsl" | "hsla" => Some(hsl(num(parts[0])?, num(parts[1])?, num(parts[2])?, alpha.clamp(0.0, 1.0))),
        _ => None,
    }
}

/// `#rrggbb` (or `#rrggbbaa` when translucent), for the editor's fields.
pub fn to_hex(c: Color) -> String {
    let h = c.hex();
    if h.ends_with("ff") { h[..7].to_string() } else { h }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Color, b: Color) -> bool {
        (a.r - b.r).abs() < 0.01 && (a.g - b.g).abs() < 0.01 && (a.b - b.b).abs() < 0.01 && (a.a - b.a).abs() < 0.01
    }

    #[test]
    fn parses_css_colours() {
        assert!(close(parse_color("#ff0000").unwrap(), crate::ui::rgb(255, 0, 0)));
        assert!(close(parse_color("rgb(0, 128, 255)").unwrap(), crate::ui::rgb(0, 128, 255)));
        assert!(close(parse_color("rgba(0 128 255 / 50%)").unwrap(), crate::ui::rgba(0, 128, 255, 0.5)));
        assert!(close(parse_color("hsl(0, 100%, 50%)").unwrap(), crate::ui::rgb(255, 0, 0)));
        assert!(close(parse_color("hsl(120deg 100% 25%)").unwrap(), crate::ui::rgb(0, 128, 0)));
        assert!(parse_color("var(--x)").is_none());
    }

    #[test]
    fn reads_and_writes_root_overrides() {
        let css = "/* mine */\n:root { --text-link: #ff00ff; --nope: 1px; }\n.message { color: red; }";
        let (o, ignored) = overrides(css);
        assert_eq!(o, vec![("--text-link".to_string(), "#ff00ff".to_string())]);
        assert_eq!(ignored, 2);
        let css2 = set_var(css, "--text-link", Some("#00ff00"));
        assert_eq!(overrides(&css2).0[0].1, "#00ff00");
        assert!(css2.contains(".message { color: red; }"));
        let added = set_var("", "--background-primary", Some("#101010"));
        assert_eq!(added, ":root { --background-primary: #101010; }");
        assert_eq!(set_var(&added, "--background-primary", None), "");
    }

    #[test]
    fn overrides_and_saturation_apply() {
        assert_eq!(families(":root { --font-sans: Inter; --font-mono: 'Fira Code' }"), (format!("Inter, {}", crate::theme::sans_family()), "'Fira Code', Fluxer Mono".into()));
        assert_eq!(overrides(":root { --font-sans: Inter }").1, 0, "fonts aren't ignored");
        let p = effective("dark", true, 1.0, ":root { --text-link: #ff0000 }");
        assert!(close(p.text_link, crate::ui::rgb(255, 0, 0)));
        let grey = effective("dark", true, 0.0, ":root { --text-link: #ff0000 }");
        let l = grey.text_link;
        assert!((l.r - l.g).abs() < 0.001 && (l.g - l.b).abs() < 0.001, "{l:?}");
        // `system` follows the OS; legacy differs from dark.
        assert!(close(effective("system", false, 1.0, "").background_primary, palette(Kind::Light).background_primary));
        // Legacy surfaces lean blue (hsl 220), dark ones purple.
        let (lg, dk) = (legacy().background_primary, palette(Kind::Dark).background_primary);
        assert!(lg.g > lg.r && dk.r > dk.g, "{lg:?} {dk:?}");
        // hsl(220, 13%, 5%) = #0b0c0e.
        assert!(close(legacy().background_primary, crate::ui::rgb(11, 12, 14)));
    }
}
