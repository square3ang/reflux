//! Message embeds (`channel/components/embeds/ChannelEmbed.tsx`, `channel_embed/RichEmbed.tsx`,
//! `EmbedParts.tsx`, `media/EmbedYouTube.tsx`): media-only embeds as bare media, rich embeds
//! as the bordered card (provider, author, title, description, fields grid, media or gallery,
//! inline thumbnail, footer), duplicates of an earlier link folded into its gallery.

use std::collections::HashSet;
use std::rc::Rc;

use reflux_model::{AttachmentFlags, Attachment, Embed, EmbedMedia, Message, Snowflake};
use reflux_store::Store;
use rune::{Color, El, Tx, col, el, icon, row, text};

use super::attachments::{self, Tile};
use super::lightbox::Item;
use super::markdown::{self, Md};
use super::{ShellAccess, ShellState, role_color};
use crate::icons;
use crate::media::Load;
use crate::theme::{Palette, dim, font};
use crate::ui::{over, rgba};
use crate::i18n::t;

/// `EMBED_MAX_WIDTH` × `EMBED_MAX_HEIGHT` (and `EMBED_TALL_MAX_HEIGHT`).
const EMBED_W: f32 = 400.0;
const EMBED_H: f32 = 300.0;
const EMBED_TALL_H: f32 = 450.0;
/// `EMBED_MEDIA_MAX_WIDTH` and `EMBED_TEXT_OUTER_WIDTH`.
const MEDIA_OUTER_W: f32 = 432.0;
const TEXT_OUTER_W: f32 = 516.0;
/// `MAX_GALLERY_MEDIA`.
const MAX_GALLERY: usize = 10;

/// `isValidMedia`: a proxy URL and both dimensions.
fn valid(m: &Option<EmbedMedia>) -> Option<&EmbedMedia> {
    m.as_ref().filter(|m| m.proxy_url.is_some() && m.width.is_some() && m.height.is_some())
}

fn dims(m: &EmbedMedia) -> (u32, u32) {
    (m.width.unwrap_or(1), m.height.unwrap_or(1))
}

/// `hasRichEmbedContent`.
fn has_rich(e: &Embed) -> bool {
    e.title.is_some() || e.description.as_deref().is_some_and(|d| !d.is_empty()) || e.author.is_some() || e.footer.is_some() || !e.fields.is_empty() || (e.provider.is_some() && e.kind != "gifv")
}

/// `normalizeUrl`: parsed, without a trailing slash.
fn normalized(url: Option<&str>) -> Option<String> {
    url::Url::parse(url?).ok().map(|u| u.as_str().trim_end_matches('/').to_owned())
}

fn is_youtube(e: &Embed) -> bool {
    e.provider.as_ref().and_then(|p| p.url.as_deref()).and_then(super::links::host).as_deref() == Some("www.youtube.com")
}

fn animated(m: &EmbedMedia) -> bool {
    m.content_type.as_deref() == Some("image/gif") || m.flags.contains(AttachmentFlags::IS_ANIMATED)
}

fn flagged(m: &EmbedMedia) -> bool {
    m.flags.contains(AttachmentFlags::CONTAINS_EXPLICIT_MEDIA)
}

fn item(m: &EmbedMedia, video: bool) -> Item {
    let (w, h) = dims(m);
    let name = url::Url::parse(&m.url).ok().and_then(|u| u.path_segments()?.next_back().map(str::to_owned)).filter(|s| !s.is_empty()).unwrap_or_else(|| "embed-media".into());
    Item {
        url: m.url.clone(),
        proxy: m.proxy_url.clone().unwrap_or_else(|| m.url.clone()),
        w,
        h,
        name,
        size: 0,
        alt: m.description.clone(),
        video,
        animated: animated(m),
    }
}

/// A media tile for embed media, opening the viewer on it.
#[allow(clippy::too_many_arguments)]
fn tile<S: 'static>(store: &Store, st: &ShellState, msg: &Message, key: String, m: &EmbedMedia, size: (f32, f32), video: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let level = attachments::filter_level(store, msg.channel_id);
    let t = Tile {
        key,
        proxy: m.proxy_url.as_deref(),
        url: &m.url,
        size,
        video,
        animated: animated(m),
        alt: m.description.as_deref(),
        spoiler: false,
        mature: (flagged(m) && level == 1) || st.prefs.dev.mature_media(),
        saved: None,
        deletable: false,
        menu: Some((msg.id, attachments::MenuMedia::embed(&m.url, m.proxy_url.as_deref(), video, animated(m)))),
    };
    let (it, at) = (item(m, video), (msg.channel_id, msg.id));
    let hovered = st.hovered_msg() == Some(msg.id);
    attachments::media_tile(store, st, t, hovered, false, p, acc, Rc::new(move |s: &mut S| super::lightbox::open(acc(s), vec![it.clone()], 0, Some(at))))
}

/// The message's embeds, in order.
pub fn embeds<S: 'static>(store: &Store, st: &ShellState, m: &Message, p: &Palette, acc: ShellAccess<S>) -> Vec<El> {
    let level = attachments::filter_level(store, m.channel_id);
    let urls: Vec<Option<String>> = m.embeds.iter().map(|e| normalized(e.url.as_deref())).collect();
    let mut out = Vec::new();
    for (i, e) in m.embeds.iter().enumerate() {
        // `isDuplicateEmbedAtIndex`: a later embed of the same link only adds to the gallery.
        if urls[i].is_some() && urls[..i].contains(&urls[i]) {
            continue;
        }
        // Blocked sensitive media drops the embed's media.
        fn blocked(x: &Option<EmbedMedia>, level: u8) -> Option<&EmbedMedia> {
            valid(x).filter(|x| !(flagged(x) && level == 2))
        }
        let key = format!("emb-{}-{i}", m.id);
        // `inlineEmbedMedia` off: links to media stay links, and rich embeds drop their images.
        let inline = store.settings.inline_embed_media;
        if !has_rich(e) {
            if !inline {
                continue;
            }
            let thumb = blocked(&e.thumbnail, level);
            let el = if let (Some(a), "audio") = (e.audio.as_ref().filter(|a| a.proxy_url.is_some()), e.kind.as_str()) {
                Some(audio(st, &key, a, e.title.as_deref(), p, acc))
            } else if let (Some(v), "video") = (blocked(&e.video, level), e.kind.as_str()) {
                Some(if is_youtube(e) { youtube(st, e, v, p, acc) } else { video(store, st, m, &key, v, thumb, p, acc) })
            } else if let (Some(t), "gifv") = (thumb, e.kind.as_str()) {
                // The muted looping video over the still while GIFs autoplay (or it's hovered).
                // Windows has no player (see `gifv`), so the still stays.
                let v = blocked(&e.video, level);
                let (w, h) = v.map_or(dims(t), dims);
                let size = super::chat::fit(w, h, (EMBED_W, EMBED_H));
                let auto = (store.settings.gif_auto_play && !crate::look::reduced_motion()) || st.hovered_msg() == Some(m.id);
                let playing = v.filter(|_| auto).and_then(|v| super::gifv::want(st, &key, v.proxy_url.as_deref().unwrap_or(&v.url), size));
                let gif = EmbedMedia { content_type: Some("image/gif".into()), ..t.clone() };
                let still = tile(store, st, m, key, &gif, size, false, p, acc);
                Some(match playing {
                    Some(video) => still.child(video.absolute(0.0, 0.0).radius(dim::MEDIA_BORDER_RADIUS).clip_rounded(dim::MEDIA_BORDER_RADIUS).inert()),
                    None => still,
                })
            } else {
                thumb.map(|t| {
                    let (w, h) = dims(t);
                    tile(store, st, m, key, t, super::chat::fit(w, h, (EMBED_W, EMBED_H)), false, p, acc)
                })
            };
            out.extend(el.map(|e| e.key(format!("emb-{i}"))));
            continue;
        }
        // `collectGalleryImages`: images (else thumbnails) of every embed of this link.
        let mut seen = HashSet::new();
        let gallery: Vec<&EmbedMedia> = m
            .embeds
            .iter()
            .enumerate()
            .filter(|(j, _)| inline && urls[i].is_some() && urls[*j] == urls[i])
            .filter_map(|(_, x)| blocked(&x.image, level).or_else(|| blocked(&x.thumbnail, level)))
            .filter(|x| seen.insert(x.content_hash.clone().unwrap_or_else(|| x.url.clone())))
            .take(MAX_GALLERY)
            .collect();
        out.push(rich(store, st, m, i, e, &gallery, p, acc).key(format!("emb-{i}")));
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn video<S: 'static>(store: &Store, st: &ShellState, m: &Message, key: &str, v: &EmbedMedia, thumb: Option<&EmbedMedia>, p: &Palette, acc: ShellAccess<S>) -> El {
    let (w, h) = dims(v);
    let max_h = if h > w { EMBED_TALL_H } else { EMBED_H };
    // The poster comes from the thumbnail when there is one.
    let poster = EmbedMedia { proxy_url: thumb.and_then(|t| t.proxy_url.clone()).or_else(|| v.proxy_url.clone()), ..v.clone() };
    let t = tile(store, st, m, key.to_owned(), &poster, super::chat::fit(w, h, (EMBED_W, max_h)), true, p, acc);
    if st.media_ui.playing.as_ref().is_some_and(|(k, _)| k == key) || super::clip::get(st, key).is_some() {
        return t;
    }
    // The tile plays `poster.url`; the clip is the video's own.
    let (k, url) = (key.to_owned(), v.proxy_url.clone().unwrap_or_else(|| v.url.clone()));
    t.on_press(move |s: &mut S| super::clip::start(acc(s), k.clone(), url.clone(), false))
}

/// `EmbedAudio`: the inline audio player for an audio link.
fn audio<S: 'static>(st: &ShellState, key: &str, a: &EmbedMedia, title: Option<&str>, p: &Palette, acc: ShellAccess<S>) -> El {
    let name = title.map(str::to_owned).unwrap_or_else(|| item(a, false).name);
    let fake = Attachment { filename: name, url: a.proxy_url.clone(), proxy_url: a.proxy_url.clone(), duration: a.duration, content_type: "audio/".into(), ..Default::default() };
    attachments::audio_player(st, key, &fake, p, acc)
}

/// `EmbedYouTube` before it's played: the poster with a frosted play / open pair.
// shortcut: play opens the video in the browser; Fluxer swaps in the YouTube iframe.
fn youtube<S: 'static>(st: &ShellState, e: &Embed, v: &EmbedMedia, p: &Palette, acc: ShellAccess<S>) -> El {
    let (vw, vh) = (v.width.unwrap_or(400), v.height.unwrap_or(400));
    let (w, h) = super::chat::fit(vw, vh, (EMBED_W, if vh > vw { EMBED_TALL_H } else { EMBED_H }));
    let poster = e.thumbnail.as_ref().and_then(|t| t.proxy_url.as_deref()).map_or(Load::Failed, |u| st.media.get(&crate::media::fit_url(u, w, st.media.scale())));
    let url = e.url.clone().unwrap_or_default();
    let button = |glyph: &'static rune::Icon, label: &str, url: String| {
        row()
            .size(32.0, 32.0)
            .center()
            .radius(10.0)
            .hover(rgba(255, 255, 255, 0.12))
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label.to_owned())
            .child(icon(glyph, 18.0).color(rgba(255, 255, 255, 0.9)))
            .on_press_tx(move |s: &mut S, tx: &mut Tx| super::links::open(acc(s), &url, tx))
    };
    let group = row()
        .gap(1.0)
        .padding(3.0)
        .radius(14.0)
        .fill(rgba(3, 3, 4, 0.86))
        .border(1.0, rgba(255, 255, 255, 0.12))
        .backdrop_blur(16.0)
        .child(button(&icons::PLAY_FILL, t("Play video"), url.clone()))
        .child(button(&icons::ARROW_SQUARE_OUT_FILL, t("Open in new tab"), url));
    super::picture(poster, w, h, 4.0, p).fill(p.background_primary).child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().child(group))
}

/// A small round icon from an embed's `proxy_icon_url`.
fn round_icon(st: &ShellState, url: Option<&str>, size: f32) -> Option<El> {
    let u = url?;
    match st.media.get(&crate::media::fit_url(u, size, st.media.scale())) {
        Load::Ready(px) => Some(el().size(size, size).radius(size / 2.0).pixels_fit(&px, rune::SurfaceFit::Fill)),
        _ => Some(el().size(size, size).radius(size / 2.0)),
    }
}

/// A pressable line of text that opens `url` (`EmbedLink`).
fn link<S: 'static>(t: El, url: Option<&str>, acc: ShellAccess<S>) -> El {
    match url {
        Some(u) => {
            let u = u.to_owned();
            t.cursor(rune::Cursor::Pointer).role("link").on_press_tx(move |s: &mut S, tx: &mut Tx| super::links::open(acc(s), &u, tx))
        }
        None => t,
    }
}

/// `groupFields`: inline fields three to a row, others alone.
fn group_fields(fields: &[reflux_model::EmbedField]) -> Vec<Vec<&reflux_model::EmbedField>> {
    let mut out: Vec<Vec<_>> = Vec::new();
    let mut cur = Vec::new();
    for f in fields {
        if f.inline {
            cur.push(f);
            if cur.len() == 3 {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(vec![f]);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `RichEmbed`.
#[allow(clippy::too_many_arguments)]
fn rich<S: 'static>(store: &Store, st: &ShellState, m: &Message, i: usize, e: &Embed, gallery: &[&EmbedMedia], p: &Palette, acc: ShellAccess<S>) -> El {
    let youtube = is_youtube(e);
    // A YouTube video is an iframe URL with no proxy copy; its size is enough.
    let video = valid(&e.video).or_else(|| e.video.as_ref().filter(|v| youtube && v.width.is_some() && v.height.is_some()));
    let image = valid(&e.image);
    let thumb = valid(&e.thumbnail);
    let show_gallery = gallery.len() > 1 || (video.is_none() && image.is_none() && thumb.is_none() && !gallery.is_empty());
    let has_media = video.is_some() || image.is_some() || thumb.is_some() || show_gallery;
    let promotes = matches!(e.kind.as_str(), "article" | "image");
    let inline_thumb = video.is_none() && thumb.is_some() && image.is_none() && !promotes && !show_gallery;
    let outer_w = if has_media && !inline_thumb { MEDIA_OUTER_W } else { TEXT_OUTER_W };
    // `getBorderColor`: the embed colour, else `brand-primary-foreground`.
    let bar = e.color.filter(|c| *c != 0).map_or(p.brand_primary_foreground, role_color);
    let rich_md = matches!(e.kind.as_str(), "rich" | "bluesky");
    let md = Md::new(store, &st.media, m.guild_id, p, format!("m{}-embed{i}", m.id));
    let mut content = col().grow(1.0).min_w(0.0).gap(8.0);
    let first = std::cell::Cell::new(true);
    let mut push = |c: El| {
        content = std::mem::replace(&mut content, col()).child(if first.replace(false) { c.mt(4.0) } else { c });
    };
    if let Some(pr) = &e.provider {
        let t = text(pr.name.clone()).font(font(12.0, 500.0).lh(16.0)).color(p.text_tertiary).wrap();
        push(link(t, pr.url.as_deref(), acc));
    }
    if let Some(a) = &e.author {
        let name = link(text(a.name.clone()).font(font(14.0, 600.0).lh(18.0)).color(p.text_primary).truncate(), a.url.as_deref(), acc);
        push(row().min_w(0.0).center_y().children(round_icon(st, a.proxy_icon_url.as_deref(), 24.0).map(|i| i.mr(8.0))).child(name.shrink()));
    }
    if let Some(t) = e.title.as_deref().filter(|t| !t.is_empty()) {
        let style = font(16.0, 600.0).lh(20.0);
        let c = if e.url.is_some() { p.text_link } else { p.text_primary };
        let body = if rich_md { markdown::inline_text(t, &md, style, c) } else { text(t.to_owned()).font(style).color(c).wrap() };
        push(link(body, e.url.as_deref(), acc));
    }
    if let Some(d) = e.description.as_deref().filter(|d| !d.is_empty() && !youtube) {
        push(if rich_md { markdown::render(d, &md).font(font(14.0, 400.0).lh(18.0)) } else { text(d.to_owned()).font(font(14.0, 400.0).lh(18.0)).color(p.text_primary).wrap() });
    }
    let groups = group_fields(&e.fields);
    let fit = groups.is_empty() && !has_media;
    if !groups.is_empty() {
        let mut grid = col().w_full().gap(8.0).mt(8.0);
        for g in groups {
            let mut r = row().w_full().gap(8.0).items_start();
            for f in g {
                r = r.child(
                    col()
                        .w(0.0)
                        .grow(1.0)
                        .min_w(0.0)
                        .child(markdown::inline_text(&f.name, &md, font(14.0, 600.0).lh(18.0), p.text_primary).mb(2.0))
                        .child(markdown::render(&f.value, &md).font(font(14.0, 400.0).lh(18.0))),
                );
            }
            grid = grid.child(r);
        }
        push(grid);
    }
    if !inline_thumb && has_media {
        let key = format!("emb-{}-{i}", m.id);
        let media = if show_gallery {
            gallery_mosaic(store, st, m, gallery, p, acc)
        } else if let Some(v) = video {
            if youtube { self::youtube(st, e, v, p, acc) } else { self::video(store, st, m, &key, v, thumb, p, acc) }
        } else {
            let x = image.or(thumb).expect("has media");
            let (w, h) = dims(x);
            tile(store, st, m, key, x, super::chat::fit(w, h, (EMBED_W, EMBED_H)), false, p, acc)
        };
        push(el().radius(4.0).clip_rounded(4.0).child(media));
    }
    if e.footer.is_some() || e.timestamp.is_some() {
        let mut line = row().min_w(0.0).center_y();
        if let Some(i) = round_icon(st, e.footer.as_ref().and_then(|f| f.proxy_icon_url.as_deref()), 20.0) {
            line = line.child(i.mr(8.0));
        }
        let small = font(12.0, 600.0).lh(16.0);
        if let Some(f) = &e.footer {
            line = line.child(text(f.text.clone()).font(small).color(p.text_chat).truncate().shrink());
        }
        let when = e.timestamp.as_deref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).map(|d| super::format_relative(d.timestamp_millis() as u64, st.now_ms));
        if let Some(w) = when {
            if e.footer.is_some() {
                line = line.child(el().size(4.0, 4.0).radius(2.0).fill(p.text_chat).mx(4.0));
            }
            line = line.child(text(w).font(small).color(p.text_chat));
        }
        push(line);
    }
    let mut grid = if fit { row() } else { row().w_full() }.items_start().pt(12.0).pr(12.0).pb(14.0).pl(12.0).child(content);
    if inline_thumb && let Some(t) = thumb {
        // `.embedThumbnail`: up to 80×80, 16 from the text, 8 down.
        let (w, h) = dims(t);
        let size = super::chat::fit(w, h, (80.0, 80.0));
        grid = grid.child(el().ml(16.0).mt(8.0).child(tile(store, st, m, format!("emb-{}-{i}-thumb", m.id), t, size, false, p, acc)));
    }
    // `.embed.embedFull`: `background-primary`, 1px accent border, a 4px colour bar, radius 8.
    let bg = p.background_primary;
    // `inline-grid`: as wide as its content, up to the max (fields keep the full width).
    let r = row();
    let r = if fit { r } else { r.w_full() };
    r.max_w(outer_w)
        .radius(8.0)
        .clip_rounded(8.0)
        .fill(over(p.background_modifier_accent, bg))
        .child(el().w(4.0).self_stretch().fill(bar))
        .child(if fit { col().shrink() } else { col().grow(1.0).min_w(0.0) }.my(1.0).mr(1.0).corners([0.0, 7.0, 7.0, 0.0]).fill(bg).child(grid))
}

/// A rich embed's gallery as the attachment mosaic, opening the viewer across it.
fn gallery_mosaic<S: 'static>(store: &Store, st: &ShellState, m: &Message, gallery: &[&EmbedMedia], p: &Palette, acc: ShellAccess<S>) -> El {
    let fake = Message {
        id: m.id,
        channel_id: m.channel_id,
        attachments: gallery
            .iter()
            .enumerate()
            .map(|(j, g)| Attachment {
                id: Snowflake(m.id.0.wrapping_add(j as u64 + 1)),
                filename: item(g, false).name,
                description: g.description.clone(),
                content_type: g.content_type.clone().unwrap_or_else(|| "image/".into()),
                url: Some(g.url.clone()),
                proxy_url: g.proxy_url.clone(),
                width: g.width,
                height: g.height,
                flags: g.flags,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    col().gap(4.0).children(attachments::attachments(store, st, &fake, p, acc)).max_w(MEDIA_OUTER_W - 24.0).fill(Color::TRANSPARENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_group_three_inline_per_row() {
        let f = |inline: bool| reflux_model::EmbedField { name: "n".into(), value: "v".into(), inline };
        let fields = [f(true), f(true), f(true), f(true), f(false), f(true)];
        assert_eq!(group_fields(&fields).iter().map(Vec::len).collect::<Vec<_>>(), [3, 1, 1, 1]);
        assert_eq!(normalized(Some("https://a.b/x/")), Some("https://a.b/x".into()));
        let e = Embed { kind: "gifv".into(), provider: Some(Default::default()), ..Default::default() };
        assert!(!has_rich(&e), "a gifv's provider alone isn't rich content");
    }
}
