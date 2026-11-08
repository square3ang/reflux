//! `AddFavoriteMemeModal` (a message's attachment → saved media) and `EditFavoriteMemeModal`,
//! both with `MemeFormFields`: name, alt text and `TagListField`.

use reflux_model::{Attachment, Snowflake};
use reqwest::Method;
use rune::{El, col, row, text};
use serde_json::{Value, json};

use super::{Done, Modal};
use crate::i18n::t;
use crate::shell::expr::Fetched;
use crate::shell::{ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, Input, Modal as Dialog};

/// `MAX_FAVORITE_MEME_TAGS`.
const MAX_TAGS: usize = 10;

/// `deriveDefaultNameFromAttachment`: the title, else the file name without its extension and
/// with `-`/`_` as spaces, else the kind.
pub fn default_name(a: &Attachment) -> String {
    if let Some(title) = a.title.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return title.to_string();
    }
    let stem = a.filename.rsplit_once('.').map_or(a.filename.as_str(), |(s, _)| s);
    let cleaned = stem.replace(['-', '_'], " ").trim().to_string();
    if !cleaned.is_empty() {
        return cleaned;
    }
    t(match a.content_type.as_str() {
        c if c.starts_with("image/gif") => "GIF",
        c if c.starts_with("video/") => "Video",
        c if c.starts_with("audio/") => "Audio",
        _ => "Image",
    })
    .to_string()
}

/// Opens the add form for attachment `a` of message `(channel, message)`.
pub fn open_add(st: &mut ShellState, channel: Snowflake, message: Snowflake, a: &Attachment) {
    st.open_social(Modal::AddMeme { channel, message, attachment: a.id });
    st.form.text.insert("meme-name".into(), rune::TextEdit::new(default_name(a)));
    st.form.text.insert("meme-alt".into(), rune::TextEdit::new(a.filename.clone()));
}

/// Opens the edit form for a saved media entry (`/users/@me/memes` item).
pub fn open_edit(st: &mut ShellState, meme: &Value) {
    let Some(id) = meme["id"].as_str() else { return };
    st.open_social(Modal::EditMeme(id.to_string()));
    st.form.text.insert("meme-name".into(), rune::TextEdit::new(meme["name"].as_str().unwrap_or_default()));
    st.form.text.insert("meme-alt".into(), rune::TextEdit::new(meme["alt_text"].as_str().unwrap_or_default()));
    st.form.data.insert("meme-tags".into(), meme["tags"].clone());
}

fn tags(st: &ShellState) -> Vec<String> {
    st.form.data.get("meme-tags").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

/// `handleAddTag`: 1–30 characters, not a duplicate, under the limit.
fn add_tag(list: &mut Vec<String>, tag: &str) -> bool {
    let tag = tag.trim();
    let ok = !tag.is_empty() && tag.chars().count() <= 30 && list.len() < MAX_TAGS && !list.iter().any(|x| x == tag);
    if ok {
        list.push(tag.to_string());
    }
    ok
}

/// `TagListField`.
fn tag_list<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let list = tags(st);
    let full = list.len() >= MAX_TAGS;
    let typed = st.form.value("meme-tag", "");
    let add = move |s: &mut S| {
        let sh = acc(s);
        let mut list = tags(sh);
        if add_tag(&mut list, &sh.form.value("meme-tag", "")) {
            sh.form.data.insert("meme-tags".into(), json!(list));
            sh.form.text.insert("meme-tag".into(), rune::TextEdit::new(""));
        }
    };
    let count = crate::i18n::tf("Tags ({tagCount}/{tagLimit})", &[("tagCount", &list.len().to_string()), ("tagLimit", &MAX_TAGS.to_string())]);
    let input = super::field(st, acc, "meme-tag", "", "", t("Add a tag"), p).grow(1.0).min_w(0.0);
    let mut c = col()
        .w_full()
        .gap(8.0)
        .child(text(count).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(input)
                .child(Button::new(t("Add")).disabled(typed.trim().is_empty() || full).key("meme-tag-add").on_press(add).el(p)),
        );
    if !list.is_empty() {
        c = c.child(row().w_full().wrap().gap(8.0).children(list.into_iter().map(|tag| {
            let x = tag.clone();
            row()
                .key(format!("tag-{tag}"))
                .gap(6.0)
                .py(6.0)
                .px(12.0)
                .center_y()
                .radius(9999.0)
                .fill(p.brand_primary)
                .child(text(tag.clone()).font(font(14.0, 400.0).lh(20.0)).color(p.brand_primary_fill))
                .child(
                    row()
                        .size(16.0, 16.0)
                        .center()
                        .cursor(rune::Cursor::Pointer)
                        .role("button")
                        .label(crate::i18n::tf("Remove {tag}", &[("tag", &tag)]))
                        .child(text("×").font(font(14.0, 400.0).lh(16.0)).color(p.brand_primary_fill))
                        .on_press(move |s: &mut S| {
                            let sh = acc(s);
                            let mut list = tags(sh);
                            list.retain(|t| *t != x);
                            sh.form.data.insert("meme-tags".into(), json!(list));
                        }),
                )
        })));
    }
    c
}

/// `MemeFormFields`: name (required, ≤ 100), alt text (≤ 500), tags.
fn fields<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Vec<El> {
    let mut items = vec![
        super::field(st, acc, "meme-name", "", t("Name"), t("My awesome media"), p),
        alt_field(st, acc, p),
        tag_list(st, p, acc),
    ];
    items.extend(super::error(st, p));
    items
}

/// The 3-row alt text `<Textarea>`.
fn alt_field<S: 'static>(st: &ShellState, acc: ShellAccess<S>, p: &Palette) -> El {
    let edit = st.form.text.get("meme-alt").cloned().unwrap_or_default();
    Input::new("form-meme-alt", &edit, move |s: &mut S| -> &mut rune::TextEdit { acc(s).form.text.entry("meme-alt".into()).or_default() })
        .label(t("Alt text"))
        .placeholder(t("Describe the media"))
        .multiline(84.0)
        .el(p)
}

/// The trimmed name, or the form error it fails with.
fn checked_name(st: &mut ShellState) -> Option<String> {
    let name = st.form.value("meme-name", "").trim().to_string();
    let alt = st.form.value("meme-alt", "");
    st.form.error = if name.chars().count() > 100 {
        Some(t("Name must be 100 characters or fewer").into())
    } else if alt.chars().count() > 500 {
        Some(t("Alt text must be 500 characters or fewer").into())
    } else {
        None
    };
    (st.form.error.is_none() && !name.is_empty()).then_some(name)
}

pub fn add<S: 'static>(st: &ShellState, channel: Snowflake, message: Snowflake, attachment: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let go = move |s: &mut S| {
        let sh = acc(s);
        let Some(name) = checked_name(sh) else { return };
        let alt = sh.form.value("meme-alt", "").trim().to_string();
        let tags = tags(sh);
        let body = json!({
            "attachment_id": attachment.to_string(),
            "name": name,
            "alt_text": Some(alt).filter(|a| !a.is_empty()),
            "tags": Some(tags).filter(|t| !t.is_empty()),
        });
        sh.social(Method::POST, format!("/channels/{channel}/messages/{message}/memes"), Some(body), Done::Toast(t("Added to saved media").into()));
    };
    form(st, t("Add to saved media"), "meme-add", go, p, acc)
}

pub fn edit<S: 'static>(st: &ShellState, id: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let id = id.to_string();
    let go = move |s: &mut S| {
        let sh = acc(s);
        let Some(name) = checked_name(sh) else { return };
        let alt = sh.form.value("meme-alt", "").trim().to_string();
        let tags = tags(sh);
        // `updateFavoriteMeme` sends only what changed.
        let mut body = serde_json::Map::new();
        if let Some(Fetched::Done(Value::Array(list))) = sh.expr.fetched.get_mut("memes")
            && let Some(m) = list.iter_mut().find(|m| m["id"].as_str() == Some(id.as_str()))
        {
            if m["name"].as_str() != Some(name.as_str()) {
                body.insert("name".into(), json!(name));
            }
            if m["alt_text"].as_str().unwrap_or_default() != alt {
                body.insert("alt_text".into(), if alt.is_empty() { Value::Null } else { json!(alt) });
            }
            if m["tags"] != json!(tags) {
                body.insert("tags".into(), json!(tags));
            }
            for (k, v) in &body {
                m[k.as_str()] = v.clone();
            }
        }
        sh.social(Method::PATCH, format!("/users/@me/memes/{id}"), Some(Value::Object(body)), Done::Toast(t("Updated saved media").into()));
    };
    form(st, t("Edit saved media"), "meme-edit", go, p, acc)
}

fn form<S: 'static>(st: &ShellState, title: &str, key: &str, go: impl Fn(&mut S) + 'static, p: &Palette, acc: ShellAccess<S>) -> El {
    let empty = st.form.value("meme-name", "").trim().is_empty();
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Cancel")).variant(crate::ui::ButtonVariant::Secondary).on_press(super::close(acc)).el(p))
        .child(Button::new(t("Save")).disabled(empty).submitting(st.form.busy).key(key.to_string()).on_press(go).el(p));
    Dialog::new(title, super::close(acc)).body(super::body(fields(st, p, acc))).footer(footer).el(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_names_and_tags() {
        let a: Attachment = serde_json::from_value(json!({"id": "1", "filename": "funny_cat-pic.png", "content_type": "image/png"})).unwrap();
        assert_eq!(default_name(&a), "funny cat pic");
        let a: Attachment = serde_json::from_value(json!({"id": "1", "filename": ".gif", "content_type": "image/gif"})).unwrap();
        assert_eq!(default_name(&a), "GIF");
        let mut l = vec![];
        assert!(add_tag(&mut l, " cat "));
        assert!(!add_tag(&mut l, "cat") && !add_tag(&mut l, "  ") && !add_tag(&mut l, &"x".repeat(31)));
        l.extend((0..9).map(|i| i.to_string()));
        assert!(!add_tag(&mut l, "eleventh"));
    }
}
