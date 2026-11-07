//! Composer attachments: the "+" menu, files dropped on the chat or pasted, the upload tray
//! (`ChannelAttachmentArea`) with spoiler, description and remove, the size and count checks
//! (`FileSizeTooLargeModal`, `TooManyAttachmentsModal`) and the instance's limits.

use std::path::PathBuf;
use std::sync::Arc;

use reflux_model::LimitConfig;
use rune::{Color, El, Shadow, TextEdit, Tx, col, el, icon, row, text, text_input};

use super::{ShellAccess, ShellState};
use crate::icons;
use crate::media::Load;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Menu, MenuItem, Modal, Placement, mix, over};
use crate::i18n::t;

/// Where a file's bytes come from.
#[derive(Clone, PartialEq)]
pub enum Source {
    Path(PathBuf),
    /// A pasted image (PNG).
    Bytes(Arc<Vec<u8>>),
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Path(p) => write!(f, "Path({})", p.display()),
            Source::Bytes(b) => write!(f, "Bytes({})", b.len()),
        }
    }
}

impl Source {
    pub fn read(&self) -> Result<Vec<u8>, String> {
        match self {
            Source::Path(p) => std::fs::read(p).map_err(|e| e.to_string()),
            Source::Bytes(b) => Ok(b.to_vec()),
        }
    }
}

/// A file in the tray, waiting for the message to be sent.
#[derive(Clone, Debug, PartialEq)]
pub struct Upload {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub source: Source,
    pub spoiler: bool,
    /// Alt text.
    pub description: String,
    /// A voice message: whole seconds and the base64 waveform.
    pub voice: Option<(u32, String)>,
}

impl Upload {
    pub fn content_type(&self) -> &'static str {
        content_type(&self.name)
    }
    fn extension(&self) -> String {
        self.name.rsplit_once('.').map(|(_, e)| e.to_uppercase()).unwrap_or_default()
    }
}

/// The media type Fluxer derives from a file name.
pub fn content_type(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "ogg" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "txt" | "log" | "md" => "text/plain",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// The limits that matter to the composer (`http-api/instance` limit keys).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub message_length: usize,
    pub file_size: u64,
    pub files: usize,
    /// `feature_global_expressions`: custom emoji and stickers work outside their community.
    pub global_expressions: bool,
    /// `max_guild_emojis` / `max_guild_stickers` with no guild features; a community's own come
    /// from [`guild_limits`] (`Guild.resolveGuildLimit`).
    pub guild_emojis: u64,
    pub guild_stickers: u64,
}

impl Default for Limits {
    /// `DEFAULT_STOCK_LIMITS` without premium.
    fn default() -> Self {
        Limits { message_length: 2000, file_size: 25 * 1024 * 1024, files: 10, global_expressions: false, guild_emojis: 500, guild_stickers: 500 }
    }
}

/// Resolves the limits for the user's traits and a community's features (none for the
/// user scope): rules sorted by filter count, a rule without filters sets a key, a matching
/// filtered one only raises it (`http-api/instance` "Limit configuration", `ruleMatches`:
/// traits match case-insensitively, features exactly).
pub fn resolve(cfg: &LimitConfig, traits: &[String], features: &[String]) -> Limits {
    let mut rules: Vec<_> = cfg.rules.iter().collect();
    rules.sort_by_key(|r| r.filters.as_ref().map_or(0, |f| f.traits.len() + f.guild_features.len()));
    let mut l = Limits::default();
    for r in rules {
        let filtered = r.filters.as_ref().is_some_and(|f| !f.traits.is_empty() || !f.guild_features.is_empty());
        let has_trait = |t: &String| traits.iter().any(|x| x.eq_ignore_ascii_case(t));
        if r.filters.as_ref().is_some_and(|f| !f.guild_features.iter().all(|g| features.contains(g)) || !f.traits.iter().all(has_trait)) {
            continue;
        }
        let get = |k: &str| r.overrides.get(k).copied().filter(|v| *v >= 0);
        let set = |cur: u64, v: Option<i64>| match v {
            Some(v) if filtered => cur.max(v as u64),
            Some(v) => v as u64,
            None => cur,
        };
        l.message_length = set(l.message_length as u64, get("max_message_length")) as usize;
        l.file_size = set(l.file_size, get("max_attachment_file_size"));
        l.files = set(l.files as u64, get("max_attachments_per_message")) as usize;
        l.global_expressions = set(l.global_expressions as u64, get("feature_global_expressions")) > 0;
        l.guild_emojis = set(l.guild_emojis, get("max_guild_emojis"));
        l.guild_stickers = set(l.guild_stickers, get("max_guild_stickers"));
    }
    l
}

/// A community's emoji and sticker slots: [`resolve`] with its features.
pub fn guild_limits(st: &ShellState, features: &[String]) -> (u64, u64) {
    match &st.limit_rules {
        Some(r) => {
            let l = resolve(&r.0, &r.1, features);
            (l.guild_emojis, l.guild_stickers)
        }
        None => (st.limits.guild_emojis, st.limits.guild_stickers),
    }
}

/// `formatFileSize`: "25 MB", "1.5 KB".
pub fn format_size(n: u64) -> String {
    let (v, unit) = match n {
        n if n >= 1 << 30 => (n as f64 / (1u64 << 30) as f64, "GB"),
        n if n >= 1 << 20 => (n as f64 / (1u64 << 20) as f64, "MB"),
        n if n >= 1 << 10 => (n as f64 / 1024.0, "KB"),
        n => (n as f64, "B"),
    };
    let s = format!("{v:.2}");
    format!("{} {unit}", s.trim_end_matches('0').trim_end_matches('.'))
}

/// Adds files to the tray, refusing oversized ones and any past the per-message count.
pub fn add(st: &mut ShellState, files: Vec<(String, u64, Source)>) {
    let l = st.limits;
    if st.uploads.len() + files.len() > l.files {
        let msg = crate::i18n::tp("{count, plural, one {You can only upload # file at a time. Try again with fewer files.} other {You can only upload # files at a time. Try again with fewer files.}}", l.files as i64, &[]);
        st.notice = Some((t("Whoa, this is heavy").into(), msg));
        return;
    }
    let big = files.iter().filter(|f| f.1 > l.file_size).count();
    if big > 0 {
        let max = format_size(l.file_size);
        let body = if big > 1 {
            crate::i18n::tf("Some files you're trying to upload exceed the maximum size limit of {maxSizeFormatted} per file.", &[("maxSizeFormatted", &max.to_string())])
        } else {
            crate::i18n::tf("The file you're trying to upload exceeds the maximum size limit of {maxSizeFormatted}.", &[("maxSizeFormatted", &max.to_string())])
        };
        st.notice = Some(("File size too large".into(), format!("{body} This limit is configured by your instance administrator.")));
    }
    for (name, size, source) in files.into_iter().filter(|f| f.1 <= l.file_size) {
        st.upload_seq += 1;
        st.uploads.push(Upload { id: st.upload_seq, name, size, source, spoiler: false, description: String::new(), voice: None });
    }
}

/// `convertTextToFile`: the draft becomes `message.txt` and the field empties.
pub fn text_as_file(st: &mut ShellState) {
    let text = st.composer.text.to_string();
    if text.trim().is_empty() {
        return;
    }
    let before = st.uploads.len();
    add(st, vec![("message.txt".into(), text.len() as u64, Source::Bytes(Arc::new(text.into_bytes())))]);
    if st.uploads.len() > before {
        st.composer = rune::TextEdit::new("");
        st.composer.focused = true;
    }
}

/// Files picked or dropped from disk; folders and unreadable paths are skipped.
pub fn add_paths(st: &mut ShellState, paths: &[PathBuf]) {
    let files = paths
        .iter()
        .filter_map(|p| {
            let m = std::fs::metadata(p).ok().filter(|m| m.is_file())?;
            Some((p.file_name()?.to_string_lossy().into_owned(), m.len(), Source::Path(p.clone())))
        })
        .collect();
    add(st, files);
}

/// A pasted image.
pub fn add_png(st: &mut ShellState, png: Vec<u8>) {
    let n = png.len() as u64;
    add(st, vec![("image.png".into(), n, Source::Bytes(Arc::new(png)))]);
}

/// The composer's "+" (`TextareaPlusMenu`): Upload file, Send voice message.
/// "Upload your message as a file" (`convertTextToFile`, shown while there's text) and "Create
/// thread" (`canStartThreadIn`) follow Upload file.
// shortcut: no "Send gift" (needs the instance's premium pricing state; gifts are bought in the
// web app).
/// `pane`: the thread side panel's composer (its own menu, draft and tray).
#[allow(clippy::too_many_arguments)]
pub fn plus_menu<S: 'static>(store: &reflux_store::Store, st: &ShellState, c: &reflux_model::Channel, pane: bool, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    use super::chat::{Slot, in_slot};
    let slot = if pane { Slot::Pane } else { Slot::Main };
    let draft = if pane { &st.threads.composer } else { &st.composer };
    let has_text = !draft.text.trim().is_empty();
    let parent = c.id;
    let can_thread = !c.kind.is_thread()
        && c.guild_id.is_some()
        && [reflux_store::threads::CreateKind::Public, reflux_store::threads::CreateKind::Private].into_iter().any(|k| store.can_create_thread(c, k));
    let hint = super::gating::upload_hint(super::gating::can_send(store, st, c), super::gating::can_attach(store, st, c));
    let gated = |i: MenuItem<S>| match hint {
        Some(h) => i.disabled(true).hint(h),
        None => i,
    };
    let mut m = Menu::new(if pane { "thread-composer-plus" } else { "composer-plus" }).placement(Placement::Top);
    m = m.item(gated(MenuItem::new("upload", t("Upload file")).icon(&icons::UPLOAD_SIMPLE_FILL).on_select_tx(move |_: &mut S, tx: &mut Tx| {
        crate::file_picker::open_files(tx, "Upload file", None, acc, move |sh, r| match r {
            Ok(paths) => in_slot(sh, slot, |sh| add_paths(sh, &paths)),
            Err(e) => sh.notice = Some(("Upload failed".into(), e)),
        });
    })));
    if has_text {
        m = m.item(gated(MenuItem::new("upload-text", t("Upload your message as a file")).icon(&icons::FILE_TEXT_FILL).on_select(move |s: &mut S| in_slot(acc(s), slot, text_as_file))));
    }
    if can_thread {
        m = m.item(MenuItem::new("create-thread", t("Create thread")).icon(&icons::CHATS_FILL).on_select(move |s: &mut S| super::threads::start_create(acc(s), parent, None)));
    }
    m.item(gated(MenuItem::new("voice-message", t("Send voice message")).icon(&icons::MICROPHONE_BOLD).on_select(move |s: &mut S| {
            acc(s).intents.push(super::Intent::Voice(super::voice::Op::RecordStart(parent)));
        })))
        .submenu("customize", t("Customize"), customize(st, acc))
        .el(&st.popups, super::pops(acc), p, layer);
}

/// The composer button switches (`TextareaPlusMenu`'s "Customize").
fn customize<S: 'static>(st: &ShellState, acc: ShellAccess<S>) -> Vec<crate::ui::MenuEntry<S>> {
    type Field = fn(&mut crate::look::Prefs) -> &mut bool;
    let rows: [(&str, &str, Field); 5] = [
        ("show-gif", t("Show GIFs button"), |l| &mut l.show_gif_button),
        ("show-media", t("Show media button"), |l| &mut l.show_memes_button),
        ("show-stickers", t("Show stickers button"), |l| &mut l.show_stickers_button),
        ("show-emoji", t("Show emoji button"), |l| &mut l.show_emoji_button),
        ("show-send", t("Show send button"), |l| &mut l.show_send_button),
    ];
    let mut look = st.prefs.look.clone();
    rows.into_iter()
        .map(|(id, label, f)| {
            let on = *f(&mut look);
            crate::ui::MenuEntry::Item(MenuItem::new(id, label).checkbox(on).on_select(move |s: &mut S| {
                let sh = acc(s);
                let mut prefs = sh.prefs.clone();
                *f(&mut prefs.look) = !on;
                sh.intents.push(super::Intent::Prefs(Box::new(prefs)));
            }))
        })
        .collect()
}

/// A tray file by ID, in the main composer's tray or the thread panel's.
fn find_mut(st: &mut ShellState, id: u64) -> Option<&mut Upload> {
    st.uploads.iter_mut().chain(st.threads.uploads.iter_mut()).find(|u| u.id == id)
}

/// `ChannelAttachmentArea`: a row of 200×200 cards above the composer field.
pub fn tray<S: 'static>(st: &ShellState, pane: bool, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let list = if pane { &st.threads.uploads } else { &st.uploads };
    if list.is_empty() {
        return None;
    }
    let cards = list.iter().map(|u| card(st, u, p, acc));
    let strip = row().gap(24.0).pt(20.0).px(10.0).pb(10.0).children(cards);
    // The bar hangs above each card's top edge; the strip's top padding leaves it room.
    Some(row().w_full().ml(6.0).mb(2.0).scroll_x().child(strip))
}

fn card<S: 'static>(st: &ShellState, u: &Upload, p: &Palette, acc: ShellAccess<S>) -> El {
    let id = u.id;
    let preview = if u.content_type().starts_with("image/") {
        let src = u.source.clone();
        st.media.local(&format!("upload:{id}"), move || src.read())
    } else {
        Load::Failed
    };
    let mut media = col().w_full().grow(1.0).min_h(0.0).center().radius(4.0).fill(p.background_tertiary).clip_rounded(4.0);
    media = match preview {
        Load::Ready(px) => media.child(el().w_full().h_full().pixels_fit(&px, rune::SurfaceFit::Fill).opacity(if u.spoiler { 0.15 } else { 1.0 })),
        Load::Pending => media,
        Load::Failed => media.child(icon(&icons::FILE_TEXT, 48.0).color(p.text_tertiary)),
    };
    if u.spoiler {
        media = media.child(row().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().child(
            row()
                .py(8.0)
                .px(12.0)
                .radius(20.0)
                .fill(Color { a: 0.6, ..Color::BLACK })
                .child(text("SPOILER").font(font(15.0, 600.0)).color(crate::ui::rgb(0xe2, 0xe4, 0xe9))),
        ));
    }
    let details = row()
        .w_full()
        .justify_between()
        .center_y()
        .child(text(format_size(u.size)).font(font(11.0, 400.0)).color(p.text_tertiary).truncate())
        .child(text(u.extension()).font(font(11.0, 600.0)).color(p.brand_primary_light).ml(4.0));
    let names = col()
        .w_full()
        .gap(2.0)
        .mt(8.0)
        .px(4.0)
        .min_h(40.0)
        .child(text(u.name.clone()).font(font(13.0, 500.0).lh(15.6)).color(p.text_primary).truncate())
        .child(details);
    // `.actionBar`: a toolbar hanging off the top-right corner (translate 25%, -25%).
    let bg = p.form_surface_background;
    let button = |glyph: &'static rune::Icon, label: &str, danger: bool| {
        let fg = if danger { mix(p.status_danger, p.text_primary, 78.0) } else { p.text_tertiary };
        row()
            .size(28.0, 28.0)
            .center()
            .radius(6.0)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label.to_string())
            .hover_group()
            .hover(if danger { mix(p.status_danger, p.surface_interactive_hover_bg, 6.0) } else { p.surface_interactive_hover_bg })
            .child(icon(glyph, 18.0).color(fg).group_color(if danger { fg } else { p.text_primary }))
    };
    let (spoiler_icon, spoiler_label) = if u.spoiler { (&icons::EYE_FILL, t("Remove spoiler")) } else { (&icons::EYE_SLASH_FILL, t("Spoiler attachment")) };
    let bar = row()
        .padding(3.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .box_shadow(Shadow { x: 0.0, y: 6.0, blur: 14.0, spread: 0.0, color: Color { a: 0.18, ..Color::BLACK }, inset: false })
        .child(button(spoiler_icon, spoiler_label, false).key(format!("upload-spoiler-{id}")).on_press(move |s: &mut S| {
            if let Some(u) = find_mut(acc(s), id) {
                u.spoiler ^= true;
            }
        }))
        .child(button(&icons::PENCIL_SIMPLE_FILL, t("Edit attachment"), false).key(format!("upload-edit-{id}")).on_press(move |s: &mut S| {
            let st = acc(s);
            let d = find_mut(st, id).map(|u| u.description.clone()).unwrap_or_default();
            let mut e = TextEdit::new(d);
            e.focused = true;
            st.upload_edit = Some((id, e));
        }))
        .child(button(&icons::TRASH_FILL, t("Remove attachment"), true).key(format!("upload-remove-{id}")).on_press(move |s: &mut S| {
            let st = acc(s);
            st.uploads.retain(|u| u.id != id);
            st.threads.uploads.retain(|u| u.id != id);
        }));
    col()
        .size(200.0, 200.0)
        .padding(8.0)
        .radius(4.0)
        .fill(p.background_primary)
        .key(format!("upload-{id}"))
        .label(format!("Attachment {}", u.name))
        .child(media)
        .child(names)
        .child(bar.pin(None, Some(-9.0), Some(-25.0), None))
}

/// `AttachmentEditModal`: the alt text of a tray file.
pub fn edit_modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let (id, edit) = st.upload_edit.as_ref()?;
    let id = *id;
    let u = st.uploads.iter().chain(&st.threads.uploads).find(|u| u.id == id)?;
    let close = move |s: &mut S| acc(s).upload_edit = None;
    let save = move |s: &mut S| {
        let st = acc(s);
        if let Some((_, e)) = st.upload_edit.take()
            && let Some(u) = find_mut(st, id)
        {
            u.description = e.text.trim().chars().take(4096).collect();
        }
    };
    let field = row()
        .w_full()
        .h(40.0)
        .px(12.0)
        .center_y()
        .radius(8.0)
        .fill(p.form_surface_background)
        .border(1.0, over(p.background_modifier_accent, p.form_surface_background))
        .child(
            text_input(edit, font(14.0, 400.0).lh(20.0))
                .key("upload-description")
                .grow(1.0)
                .single_line()
                .color(p.text_primary)
                .placeholder("Describe this file for people who can't see it", p.text_tertiary)
                .caret(p.text_primary, p.text_selection)
                .on_edit(move |s: &mut S| -> &mut TextEdit { &mut acc(s).upload_edit.get_or_insert_with(|| (0, TextEdit::new(""))).1 })
                .on_submit(move |s: &mut S, _| save(s)),
        );
    let body = col()
        .w_full()
        .gap(8.0)
        .child(text(u.name.clone()).font(font(14.0, 500.0)).color(p.text_primary_muted).truncate())
        .child(text("Description (alt text)").font(font(12.0, 600.0)).color(p.text_primary))
        .child(field);
    Some(
        Modal::new(t("Edit attachment"), close)
            .body(body)
            .footer(
                row()
                    .gap(8.0)
                    .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("upload-edit-cancel").on_press(close).el(p))
                    .child(Button::new(t("Save")).key("upload-edit-save").on_press(save).el(p)),
            )
            .el(p),
    )
}

/// `GenericErrorModal` (a `ConfirmModal` with no actions: desktop has no footer, only the X).
pub fn notice_modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let (title, body) = st.notice.as_ref()?;
    let close = move |s: &mut S| acc(s).notice = None;
    Some(
        Modal::new(title.clone(), close)
            .body(text(body.clone()).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap())
            .el(p),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_resolve_like_the_docs() {
        let cfg: LimitConfig = serde_json::from_value(serde_json::json!({
            "version": 2, "traitDefinitions": ["premium"], "defaultsHash": "x",
            "rules": [
                {"id": "premium", "filters": {"traits": ["premium"]}, "overrides": {"max_message_length": 4000, "max_attachment_file_size": 1000}},
                {"id": "default", "overrides": {"max_attachment_file_size": 50000000, "max_attachments_per_message": -1}},
            ]
        }))
        .unwrap();
        let free = resolve(&cfg, &[], &[]);
        assert_eq!(free, Limits { message_length: 2000, file_size: 50_000_000, files: 10, global_expressions: false, ..Limits::default() }, "negative values are ignored");
        let premium = resolve(&cfg, &["PREMIUM".into()], &[]);
        assert_eq!((premium.message_length, premium.file_size), (4000, 50_000_000), "filtered rules only raise");
        let cfg: LimitConfig = serde_json::from_value(serde_json::json!({
            "version": 2, "traitDefinitions": [], "defaultsHash": "x",
            "rules": [{"id": "big", "filters": {"guildFeatures": ["MORE_EMOJI"]}, "overrides": {"max_guild_emojis": 1000}}]
        }))
        .unwrap();
        assert_eq!(resolve(&cfg, &[], &[]).guild_emojis, 500);
        assert_eq!(resolve(&cfg, &[], &["MORE_EMOJI".into()]).guild_emojis, 1000, "a community's features raise its slots");
        assert_eq!(resolve(&cfg, &[], &["more_emoji".into()]).guild_emojis, 500, "features match exactly");
    }

    #[test]
    fn tray_refuses_big_and_too_many_files() {
        let mut st = ShellState { limits: Limits { files: 2, file_size: 10, ..Default::default() }, ..Default::default() };
        let f = |n: &str, size| (n.to_string(), size, Source::Bytes(Arc::new(vec![])));
        add(&mut st, vec![f("a.png", 5), f("b.bin", 11)]);
        assert_eq!(st.uploads.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(), ["a.png"]);
        assert_eq!(st.notice.take().map(|n| n.0), Some("File size too large".into()));
        add(&mut st, vec![f("c", 1), f("d", 1)]);
        assert_eq!(st.uploads.len(), 1, "over the count adds none");
        assert_eq!(st.notice.map(|n| n.0), Some("Whoa, this is heavy".into()));
        assert_eq!(format_size(25 * 1024 * 1024), "25 MB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(content_type("A.JPG"), "image/jpeg");
        let mut st = ShellState { composer: rune::TextEdit::new("a long message"), ..Default::default() };
        text_as_file(&mut st);
        assert_eq!((st.uploads[0].name.as_str(), st.uploads[0].size, st.composer.text.is_empty()), ("message.txt", 14, true));
    }
}
