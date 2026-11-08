//! Role list drag-to-reorder (`RoleSidebar` / `RoleReorderStateMachine`) and the role icon field
//! (`RoleIconField`).

use reflux_model::{Role, Snowflake};
use reflux_store::Store;
use rune::{El, col, icon, row};
use serde_json::{Map, Value, json};

use super::settings::{small, title};
use crate::i18n::t;
use crate::icons;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::Palette;
use crate::ui::{Button, ButtonSize, ButtonVariant};

// ---- Reorder ---------------------------------------------------------------------------------

/// The `PATCH /guilds/{id}/roles` body after moving `order[from]` to `to` (top ranks highest).
pub fn moved(order: &[Snowflake], from: usize, to: usize) -> Value {
    let mut o = order.to_vec();
    let r = o.remove(from);
    o.insert(to.min(o.len()), r);
    let n = o.len();
    Value::Array(o.iter().enumerate().map(|(k, r)| json!({ "id": r, "position": n - k })).collect())
}

/// Where a drag of row `from` lands: rows are `pitch` apart.
pub fn landing(from: usize, dy: f32, pitch: f32, len: usize) -> usize {
    (from as f32 + (dy / pitch).round()).clamp(0.0, len.saturating_sub(1) as f32) as usize
}

/// The live drag: (from, to) in `form.data["role-drag"]`.
pub fn drag_state(st: &ShellState) -> Option<(usize, usize)> {
    let v = st.form.data.get("role-drag")?;
    Some((v[0].as_u64()? as usize, v[1].as_u64()? as usize))
}

/// Makes a role row draggable; `pad` is the card's vertical padding around it.
pub fn draggable<S: 'static>(e: El, g: Snowflake, i: usize, order: Vec<Snowflake>, pad: f32, acc: ShellAccess<S>) -> El {
    e.on_drag(move |s: &mut S, d: &mut rune::Drag, _| {
        let sh = acc(s);
        match d.phase {
            rune::Phase::Down => {
                sh.form.data.remove("role-drag");
            }
            rune::Phase::Move => {
                // A short wobble stays a click.
                if !sh.form.data.contains_key("role-drag") && d.dy.abs() < 4.0 {
                    return;
                }
                let to = landing(i, d.dy, d.h + pad, order.len());
                sh.form.data.insert("role-drag".into(), json!([i, to]));
            }
            rune::Phase::Up => {
                if let Some((from, to)) = drag_state(sh).filter(|d2| !d.cancelled() && d2.0 != d2.1) {
                    sh.api(reqwest::Method::PATCH, format!("/guilds/{g}/roles"), Some(moved(&order, from, to)), super::Then::Nothing);
                }
                sh.form.data.remove("role-drag");
            }
        }
    })
}

// ---- Icon ------------------------------------------------------------------------------------

/// `handleEmojiSelect`: a custom emoji has no glyph in the sprite sheet, so Fluxer fetches its
/// image and stages that as the icon. Called once when the picker picks one; a later pick wins
/// (`stageTokenRef`), and the emoji shows from its image meanwhile.
pub fn emoji_picked(st: &mut ShellState, custom: Option<String>) {
    let token = st.form.data.get("role-emoji-token").and_then(Value::as_u64).unwrap_or(0) + 1;
    st.form.data.insert("role-emoji-token".into(), json!(token));
    st.form.error = None;
    let Some(id) = custom else { return };
    let (http, url) = (st.media.http(), crate::media::emoji_url(&st.media.base(), &id));
    crate::app::spawn(async move {
        let result = match crate::media::get_capped(&http, &url, crate::media::MAX_IMAGE_BYTES).await {
            Ok(b) => prepare(&b),
            Err(e) => {
                eprintln!("reflux: role icon from emoji {url}: {e}");
                Err("The emoji couldn't be loaded. Pick another.")
            }
        };
        crate::app::update(move |s| {
            let f = &mut s.app.shell.form;
            if f.data.get("role-emoji-token").and_then(Value::as_u64) != Some(token) {
                return;
            }
            f.data.remove("role-emoji");
            match result {
                Ok(png) => {
                    f.data.insert("role-icon".into(), json!(super::data_url(std::path::Path::new("x.png"), &png)));
                }
                Err(e) => f.error = Some(t(e).into()),
            }
        });
    });
}

/// `role_icon` policy: at least 32px.
const MIN: u32 = 32;

/// A role icon from an image file: static only, cropped square, raised to the minimum size.
pub fn prepare(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    if bytes.starts_with(b"GIF8") {
        return Err("Animated icons are not supported. Use a static image.");
    }
    let img = image::load_from_memory(bytes).map_err(|_| "The image couldn't be loaded.")?;
    let side = img.width().min(img.height());
    let mut sq = img.crop_imm((img.width() - side) / 2, (img.height() - side) / 2, side, side);
    if side < MIN {
        sq = sq.resize_exact(MIN * 2, MIN * 2, image::imageops::FilterType::Lanczos3);
    }
    let mut png = Vec::new();
    sq.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|_| "The image couldn't be loaded.")?;
    Ok(png)
}

/// The staged icon change for the role PATCH.
pub fn patch(st: &ShellState, out: &mut Map<String, Value>) {
    if let Some(e) = st.form.data.get("role-emoji").filter(|e| e["id"].is_null()) {
        out.insert("unicode_emoji".into(), e["name"].clone());
        out.insert("icon".into(), Value::Null);
    } else if let Some(i) = st.form.data.get("role-icon") {
        out.insert("icon".into(), i.clone());
        out.insert("unicode_emoji".into(), Value::Null);
    }
}

/// The role as it'll look once saved (staged emoji / removal applied).
fn staged(st: &ShellState, r: &Role) -> (Role, bool) {
    let mut r = r.clone();
    let mut picked_image = false;
    if let Some(e) = st.form.data.get("role-emoji").filter(|e| e["id"].is_null()) {
        r.icon = None;
        r.unicode_emoji = e["name"].as_str().map(str::to_string);
    } else if let Some(i) = st.form.data.get("role-icon") {
        r.unicode_emoji = None;
        r.icon = None;
        picked_image = i.is_string();
    }
    (r, picked_image)
}

pub fn field<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, r: &Role, p: &Palette, acc: ShellAccess<S>) -> El {
    let (shown, picked_image) = staged(st, r);
    let has = picked_image || shown.icon.is_some() || shown.unicode_emoji.is_some();
    // A custom emoji is shown from its image while the upload copy is fetched.
    let custom = st.form.data.get("role-emoji").and_then(|e| e["id"].as_str()).map(str::to_owned);
    let glyph = if picked_image {
        Some(icon(&icons::IMAGE_FILL, 28.0).color(p.brand_primary_fill))
    } else if let Some(id) = custom {
        match st.media.asset(move |b, _| crate::media::emoji_url(b, &id)) {
            crate::media::Load::Ready(px) => Some(rune::el().size(40.0, 40.0).pixels_fit(&px, rune::SurfaceFit::Fit)),
            _ => None,
        }
    } else {
        crate::shell::role_icon::view(st, &shown, 40.0)
    };
    let tile = row()
        .size(64.0, 64.0)
        .center()
        .radius(8.0)
        .fill(if picked_image { p.brand_primary } else { p.background_tertiary })
        .border(1.0, p.background_modifier_accent)
        .children(glyph.or_else(|| (!has).then(|| icon(&icons::IMAGE_FILL, 24.0).color(p.text_tertiary))));
    let upload = Button::<S>::new(t("Upload image")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).el(p).on_press_tx(crate::settings::crop::pick(acc, "role-icon", crate::settings::crop::Asset::RoleIcon));
    let channel = store.channels.values().find(|c| c.guild_id == Some(g)).map_or(g, |c| c.id);
    let choose = Button::new(t("Choose emoji")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).key("pick-role-emoji").on_press(move |s: &mut S| crate::shell::expr::open_for_form(acc(s), channel, "role-emoji")).el(p);
    let mut buttons = row().gap(8.0).child(upload).child(choose);
    if has {
        buttons = buttons.child(Button::new(t("Remove")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).on_press(move |s: &mut S| {
            let f = &mut acc(s).form;
            f.data.remove("role-emoji");
            f.data.insert("role-icon".into(), Value::Null);
        }).el(p));
    }
    col()
        .w_full()
        .gap(8.0)
        .child(title(t("Role icon"), p))
        .child(small(t("Upload an image or pick an emoji. Members get the icon of their highest role that has one."), p).wrap())
        .child(row().gap(16.0).center_y().child(tile).child(col().gap(8.0).child(buttons).child(small(t("PNG, JPEG, WebP or AVIF. At least 32×32px, up to 256 KB."), p))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_bodies_and_landing() {
        let o = [Snowflake(1), Snowflake(2), Snowflake(3)];
        assert_eq!(moved(&o, 0, 2), json!([{ "id": Snowflake(2), "position": 3 }, { "id": Snowflake(3), "position": 2 }, { "id": Snowflake(1), "position": 1 }]));
        assert_eq!(landing(0, 130.0, 62.0, 3), 2);
        assert_eq!(landing(1, -20.0, 62.0, 3), 1);
        assert_eq!(landing(2, 500.0, 62.0, 3), 2);
        assert_eq!(landing(0, -500.0, 62.0, 3), 0);
    }

    #[test]
    fn icons_crop_square_and_refuse_gifs() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(20, 10).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let out = image::load_from_memory(&prepare(&png).unwrap()).unwrap();
        assert_eq!((out.width(), out.height()), (64, 64));
        assert!(prepare(b"GIF89a").is_err());
    }
}
