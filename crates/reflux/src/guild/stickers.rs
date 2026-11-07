//! The Stickers tab (`GuildStickersTab`): upload from a file, then rename, describe, tag or
//! delete each sticker inline.

use reflux_model::{Permissions as P, Snowflake, Sticker};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, Tx, col, icon, row};
use serde_json::{Value, json};

use super::settings::{card, empty, small, title};
use super::{Then, error, field};
use crate::icons;
use crate::media;
use crate::settings::tabs::section;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::Palette;
use crate::ui::{Button, ButtonSize, ButtonVariant};
use crate::i18n::t;

/// A sticker name from a file name: its stem, 2-30 characters (`GuildStickerCreateRequest`).
pub fn sticker_name(path: &std::path::Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let mut n: String = stem.chars().take(30).collect::<String>().trim().to_string();
    while n.chars().count() < 2 {
        n.push('_');
    }
    n
}

/// `tags` as typed: comma-separated, trimmed, at most 30 characters each.
fn tags(s: &str) -> Vec<String> {
    s.split(',').map(|t| t.trim().chars().take(30).collect::<String>()).filter(|t| !t.is_empty()).collect()
}

fn editing(st: &ShellState) -> Option<Snowflake> {
    st.form.data.get("edit-sticker").and_then(Value::as_str).and_then(|s| s.parse().ok())
}

fn icon_button<S: 'static>(glyph: &'static rune::Icon, label: &str, p: &Palette, f: impl Fn(&mut S) + 'static) -> El {
    row()
        .size(28.0, 28.0)
        .center()
        .radius(6.0)
        .hover(p.background_modifier_hover)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.to_string())
        .on_press(f)
        .child(icon(glyph, 16.0).color(p.text_primary_muted))
}

/// Squares a sticker to 320 px (`optimizeStickerImage`) and posts it.
fn post(sh: &mut ShellState, g: Snowflake, path: &std::path::Path) {
    match super::expr_upload::prepared(path, 320) {
        Ok(image) => sh.api(Method::POST, format!("/guilds/{g}/stickers"), Some(json!({ "name": sticker_name(path), "tags": [], "image": image })), Then::Nothing),
        Err(e) => sh.form.error = Some(format!("Couldn't open the image: {e}")),
    }
}

pub(super) fn view<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let list: Vec<&Sticker> = store.guilds.get(&g).map(|gs| gs.stickers.iter().collect()).unwrap_or_default();
    let manage = store.guild_permissions(g).contains(P::MANAGE_EXPRESSIONS);
    let me = store.my_id();
    let upload = Button::<S>::new(t("Upload sticker")).size(ButtonSize::Small).key("upload-sticker").el(p).on_press_tx(move |_: &mut S, tx: &mut Tx| {
        let options = rune::FileDialogOptions { title: "Upload sticker".into(), ..Default::default() }.filter(t("Images"), &["png", "jpg", "jpeg", "gif", "webp", "avif"]);
        tx.open_file(options, move |s: &mut S, r, _| {
            let sh = acc(s);
            match r {
                Ok(Some(path)) => post(sh, g, &path),
                Ok(None) => {}
                Err(e) => sh.form.error = Some(e.to_string()),
            }
        });
    });
    let open = editing(st);
    let rows: Vec<El> = list
        .iter()
        .map(|x| {
            let id = x.id;
            let img = crate::shell::picture(st.media.asset(|b, _| media::sticker_url(b, id, false)), 48.0, 48.0, 4.0, p);
            let mine = manage || x.user.as_ref().is_some_and(|u| u.id == me);
            if open == Some(id) {
                let save = Button::<S>::new(t("Save")).size(ButtonSize::Small).key("sticker-save").el(p).on_press(move |s: &mut S| {
                    let sh = acc(s);
                    let (name, desc, t) = (sh.form.value("sticker-name", ""), sh.form.value("sticker-desc", ""), sh.form.value("sticker-tags", ""));
                    let body = json!({ "name": name.trim(), "description": Some(desc.trim()).filter(|d| !d.is_empty()), "tags": tags(&t) });
                    sh.form.data.remove("edit-sticker");
                    sh.api(Method::PATCH, format!("/guilds/{g}/stickers/{id}"), Some(body), Then::Nothing);
                });
                let cancel = Button::<S>::new(t("Cancel")).size(ButtonSize::Small).variant(ButtonVariant::Secondary).el(p).on_press(move |s: &mut S| {
                    acc(s).form.data.remove("edit-sticker");
                });
                return row().w_full().gap(12.0).items_start().child(img).child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(8.0)
                        .child(field(st, acc, "sticker-name", &x.name, t("Name"), "2-30 characters", p))
                        .child(field(st, acc, "sticker-desc", &x.description, t("Description"), "What it shows", p))
                        .child(field(st, acc, "sticker-tags", &x.tags.join(", "), t("Tags"), "Comma-separated", p))
                        .child(row().gap(8.0).justify_end().child(cancel).child(save)),
                );
            }
            let detail = [x.description.clone(), x.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            let mut line = row().w_full().gap(12.0).center_y().child(img).child(col().grow(1.0).min_w(0.0).child(title(x.name.clone(), p)).children((!detail.is_empty()).then(|| small(detail, p).truncate())));
            if mine {
                line = line
                    .child(icon_button(&icons::PENCIL_SIMPLE, t("Edit sticker"), p, move |s: &mut S| {
                        let f = &mut acc(s).form;
                        f.data.insert("edit-sticker".into(), Value::String(id.to_string()));
                        for k in ["sticker-name", "sticker-desc", "sticker-tags"] {
                            f.text.remove(k);
                        }
                    }))
                    .child(icon_button(&icons::X_BOLD, t("Delete sticker"), p, move |s: &mut S| acc(s).api(Method::DELETE, format!("/guilds/{g}/stickers/{id}"), None, Then::Nothing)));
            }
            line
        })
        .collect();
    let mut content = col()
        .w_full()
        .gap(16.0)
        .child(row().w_full().justify_between().center_y().gap(12.0).child(small("Names come from the file name; edit them after uploading. PNG, JPEG, GIF, WebP or AVIF up to 512 KB.", p).wrap().shrink()).child(upload));
    if store.guild_permissions(g).intersects(P::MANAGE_EXPRESSIONS | P::CREATE_EXPRESSIONS) {
        content = content.child(super::expr_upload::drop_zone(st, t("Drag and drop a sticker file here (one at a time)"), false, p, acc, move |sh, files| post(sh, g, &files[0])));
    }
    if let Some(e) = error(st, p) {
        content = content.child(e);
    }
    content = content.child(if rows.is_empty() { empty("No stickers yet.", p) } else { card(rows, p) });
    section(&format!("Stickers — {}", list.len()), content, true, p)
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_and_tags() {
        assert_eq!(super::sticker_name(std::path::Path::new("/x/a.png")), "a_");
        assert_eq!(super::sticker_name(std::path::Path::new("/x/ship it.webp")), "ship it");
        assert_eq!(super::tags(" crab, ,rust "), ["crab", "rust"]);
    }
}
