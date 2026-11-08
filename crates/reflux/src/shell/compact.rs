//! The dense message layout (`CompactMessageLayout.tsx`, `.messageCompact` in
//! `Message.module.css`): every message is one line led by `[time]`, then (optionally a 16px avatar)
//! the author's name and a colon, then the text; attachments, embeds and reactions sit under it at
//! `--message-compact-text-start`.
// shortcut: the text wraps under its own start, not 32px under the name
// (`--message-compact-text-indent-from-username` with a negative text-indent); Rune lays the name
// and the markdown body out as separate boxes.

use reflux_model::Message;
use reflux_store::Store;
use rune::{El, col, el, row, text};

use super::chat::{accessories, author_color, author_name, editor, format_time, frame, message_ms, reply_preview};
use super::markdown::{self, Md};
use super::Target;
use super::{ShellAccess, ShellState};
use crate::theme::{Palette, dim, font};
use crate::ui::Placement;

/// `[12:34]` at `--message-timestamp-compact-font-size`, and its width.
fn timestamp(m: &Message) -> (String, f32) {
    let ts = format!("[{}]", format_time(message_ms(m)));
    let w = rune::text::advance(&ts, font(dim::MESSAGE_TIMESTAMP_COMPACT_FONT_SIZE, 400.0)).ceil();
    (ts, w)
}

pub(super) fn message<S: 'static>(store: &Store, st: &ShellState, m: &Message, md: &Md, p: &Palette, acc: ShellAccess<S>) -> El {
    let pad = dim::MESSAGE_COMPACT_HORIZONTAL_PADDING;
    let root = col().w_full().pr(pad + dim::MESSAGE_ACTION_BAR_OFFSET).py(dim::MESSAGE_SPACING_Y).hover_group();
    let mut root = frame(store, st, m, root, p);
    let (ts, ts_w) = timestamp(m);
    let body_x = pad + ts_w + dim::MESSAGE_COMPACT_GAP;
    if m.kind == reflux_model::MessageType::REPLY {
        // `.repliedMessageCompact`: the spine starts 4px in and runs to the body start.
        root = root.child(reply_preview(store, st, m, md, p, acc, pad + 4.0, body_x).key("reply"));
    }

    let (author, guild, mid) = (m.author.id, store.channels.get(&m.channel_id).and_then(|c| c.guild_id), m.id);
    let note = store.notes.get(&author).cloned().unwrap_or_default();
    let open = move |s: &mut S| acc(s).open_profile(author, guild, note.clone(), format!("avatar-{mid}"), Placement::RightStart);
    let menu = move |s: &mut S, at: rune::PointerPress, _: &mut rune::Tx| {
        acc(s).open_user_menu(Target::User { user: author, guild, dm: None }, [at.x, at.y], format!("avatar-{mid}"), Placement::RightStart)
    };

    // `.compactAuthorPrefix`: [avatar] name ":" then `--message-compact-username-gap`.
    // `UnknownMessage` (compact): the name prefix without an avatar, the warning, no accessories.
    let unknown = super::devtools::unknown(store, st, m);
    let mut prefix = row().items_center().shrink().min_w(0.0);
    if crate::look::with(|l| l.compact_avatars) && !unknown {
        prefix = prefix.child(
            super::avatar_anim(store, st, author, dim::MESSAGE_AVATAR_SIZE_COMPACT, None, p.background_secondary_lighter, st.hovered_msg() == Some(mid), p)
                .key(format!("avatar-{mid}"))
                .mr(dim::MESSAGE_COMPACT_GAP)
                .cursor(rune::Cursor::Pointer)
                .on_press(open.clone()),
        );
    }
    prefix = prefix
        .child(
            text(author_name(store, m))
                .font(font(16.0, 500.0).lh(dim::MESSAGE_LINE_HEIGHT))
                .color(author_color(store, m, p))
                .truncate()
                .shrink()
                .cursor(rune::Cursor::Pointer)
                .on_press(open)
                .on_secondary_press(menu)
                .key("name"),
        )
        .child(text(":").font(font(16.0, 400.0).lh(dim::MESSAGE_LINE_HEIGHT)).color(p.text_primary).key("colon"))
        .child(el().w(dim::MESSAGE_COMPACT_USERNAME_GAP).key("gap"));

    let mut body = col().grow(1.0).min_w(0.0);
    if unknown {
        body = body.child(super::devtools::unknown_warning(p));
    } else if let Some((_, edit)) = st.editing.as_ref().filter(|e| e.0 == m.id) {
        body = body.child(editor(edit, m, p, acc).key("editor"));
    } else if !m.content.is_empty() {
        body = body.child(markdown::render(&m.content, md).key("content"));
    }
    if !unknown && m.edited_timestamp.is_some() && st.editing.as_ref().is_none_or(|e| e.0 != m.id) {
        body = body.child(text(crate::i18n::t("(edited)")).font(font(dim::MESSAGE_EDITED_LABEL_FONT_SIZE, 400.0).lh(16.0)).color(p.message_timestamp_color));
    }

    // The timestamp is 11px in the 22px line; its baseline sits ~2px lower than its box centre to
    // line up with the 16px text (`align-items: baseline`).
    let stamp = text(ts).font(font(dim::MESSAGE_TIMESTAMP_COMPACT_FONT_SIZE, 400.0).lh(dim::MESSAGE_LINE_HEIGHT)).color(p.message_timestamp_color).offset(0.0, 1.5).w(ts_w);
    root = root.child(
        row()
            .key("line")
            .w_full()
            .items_start()
            .child(el().w(pad))
            .child(stamp)
            .child(el().w(dim::MESSAGE_COMPACT_GAP))
            .child(prefix)
            .child(body),
    );
    if let Some(a) = accessories(store, st, m, p, acc).filter(|_| !unknown) {
        // `.messageCompact > .container`: at the text start (timestamp + gap + indent).
        root = root.child(row().key("accessories").w_full().child(el().w(body_x + dim::MESSAGE_COMPACT_TEXT_INDENT_FROM_USERNAME)).child(a.shrink().min_w(0.0)));
    }
    if md.copy.is_some() {
        root = root.children(super::threads::chip(store, st, m, p, acc).into_iter().map(|c| row().key("thread").w_full().child(el().w(body_x + dim::MESSAGE_COMPACT_TEXT_INDENT_FROM_USERNAME)).child(c)));
    }
    root
}

/// "Announce new messages": a polite live region holding the open channel's newest message from
/// someone else, so screen readers read it when it arrives.
// Kept here rather than in chat.rs to keep that shared file small.
pub(super) fn announcer(store: &Store, c: &reflux_model::Channel) -> Option<El> {
    if !crate::look::with(|l| l.announce_messages) {
        return None;
    }
    let m = store.messages.get(&c.id)?.messages.iter().rev().find(|m| m.author.id != store.my_id())?;
    let line = crate::i18n::tf("{authorName} said: {formatted}", &[("authorName", &author_name(store, m)), ("formatted", &m.content)]);
    Some(el().size(0.0, 0.0).key("announce").role("status").label(line).live_region(rune::LiveRegion::Polite))
}

#[cfg(test)]
mod tests {
    #[test]
    fn timestamp_is_bracketed() {
        let m: reflux_model::Message = serde_json::from_value(serde_json::json!({
            "id": "1", "channel_id": "2", "author": {"id": "3", "username": "a", "discriminator": "0"},
            "content": "", "timestamp": "2026-11-06T12:34:00Z"
        }))
        .unwrap();
        let (ts, w) = super::timestamp(&m);
        assert!(ts.starts_with('[') && ts.ends_with(']'), "{ts}");
        assert!(w > 20.0, "{w}");
    }
}
