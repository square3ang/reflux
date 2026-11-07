//! Emoji and sticker uploads: the drop zone (`UploadDropZone`) and the image prep
//! (`ImageCropUtils.optimizeEmojiImage`: contain in a transparent square, animated GIFs as-is).

use std::path::{Path, PathBuf};

use reflux_model::Snowflake;
use reqwest::Method;
use rune::{El, col, icon, text};
use serde_json::{Value, json};

use super::{Then, data_url};
use crate::i18n::t;
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};

/// `MAX_EMOJIS_PER_UPLOAD`: one bulk request at most.
const BULK: usize = 50;

/// Fits an image inside a `target`² transparent square as PNG; GIFs pass through (animated).
pub fn contain_square(bytes: &[u8], target: u32) -> Result<Vec<u8>, String> {
    if bytes.starts_with(b"GIF8") {
        return Ok(bytes.to_vec());
    }
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    // The long side scales to `target` (up or down), centred.
    let fit = img.resize(target, target, image::imageops::FilterType::Lanczos3).to_rgba8();
    let mut out = image::RgbaImage::new(target, target);
    image::imageops::overlay(&mut out, &fit, i64::from((target - fit.width()) / 2), i64::from((target - fit.height()) / 2));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(out).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(png)
}

/// A file as an upload data URL, squared to `target`.
pub fn prepared(path: &Path, target: u32) -> Result<String, String> {
    let b = std::fs::read(path).map_err(|e| e.to_string())?;
    let out = contain_square(&b, target)?;
    // A re-encoded image is PNG whatever it was.
    Ok(if out.starts_with(b"GIF8") { data_url(path, &out) } else { data_url(Path::new("x.png"), &out) })
}

/// Uploads emoji files through `/emojis/bulk` (50 a request); what failed lands in
/// `form.data["emoji-failed"]` (`{ name, error }` rows).
pub fn upload_emojis(sh: &mut ShellState, g: Snowflake, paths: &[PathBuf]) {
    let mut failed = Vec::new();
    let mut ready = Vec::new();
    for path in paths {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        match prepared(path, 128) {
            Ok(image) => ready.push(json!({ "name": super::settings::emoji_name(path), "image": image })),
            Err(e) => failed.push(json!({ "name": name, "error": e })),
        }
    }
    sh.form.data.insert("emoji-failed".into(), Value::Array(failed));
    for chunk in ready.chunks(BULK) {
        sh.api(Method::POST, format!("/guilds/{g}/emojis/bulk"), Some(json!({ "emojis": chunk })), Then::Keep("emoji-bulk".into()));
    }
}

/// The rows that didn't upload: prep failures plus the last bulk answer's `failed`.
pub fn failures(st: &ShellState) -> Vec<(String, String)> {
    let rows = |v: Option<&Value>| -> Vec<(String, String)> {
        v.and_then(Value::as_array).into_iter().flatten().map(|f| (f["name"].as_str().unwrap_or("?").to_string(), f["error"].as_str().unwrap_or("").to_string())).collect()
    };
    let mut out = rows(st.form.data.get("emoji-failed"));
    out.extend(rows(st.form.data.get("emoji-bulk").and_then(|b| b.get("failed"))));
    out
}

/// `UploadDropZone`: a dashed box taking dropped files (`multiple`, or just the first).
pub fn drop_zone<S: 'static>(st: &ShellState, label: &str, multiple: bool, p: &Palette, acc: ShellAccess<S>, on: impl Fn(&mut ShellState, &[PathBuf]) + 'static) -> El {
    let over = st.form.data.get("drop-hover").and_then(Value::as_bool).unwrap_or(false);
    col()
        .w_full()
        .px(32.0)
        .py(32.0)
        .gap(8.0)
        .center()
        .radius(6.0)
        .border(2.0, if over { p.text_primary } else { p.background_header_secondary })
        .border_dash(6.0, 4.0)
        .fill(if over { p.background_header_secondary } else { p.background_tertiary })
        .role("group")
        .label(t("Drag and drop area for file upload"))
        .child(icon(&icons::UPLOAD_SIMPLE, 48.0).color(p.text_primary_muted))
        .child(text(label.to_string()).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted))
        .on_drop_files(move |s: &mut S, files, _| {
            let sh = acc(s);
            sh.form.data.remove("drop-hover");
            let files = if multiple { files } else { &files[..files.len().min(1)] };
            if !files.is_empty() {
                on(sh, files);
            }
        })
        .on_drop_hover(move |s: &mut S, over, _| {
            acc(s).form.data.insert("drop-hover".into(), json!(over));
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_in_a_square() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(400, 100).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let out = image::load_from_memory(&contain_square(&png, 128).unwrap()).unwrap();
        assert_eq!((out.width(), out.height()), (128, 128));
        let mut small = Vec::new();
        image::DynamicImage::new_rgba8(20, 10).write_to(&mut std::io::Cursor::new(&mut small), image::ImageFormat::Png).unwrap();
        let out = image::load_from_memory(&contain_square(&small, 128).unwrap()).unwrap();
        assert_eq!((out.width(), out.height()), (128, 128), "small images scale up");
        assert_eq!(contain_square(b"GIF89a..", 128).unwrap(), b"GIF89a..");
    }
}
