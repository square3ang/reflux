//! `CLIENT_SYSTEM` messages from Fluxerbot (`createSystemMessage`): local notes only this client
//! has, such as the `/nick` confirmation or a `/msg` that couldn't be delivered. They show with
//! the reply highlight, a System tag and "Only you can see this message. Dismiss"
//! (`UserMessage`'s `CLIENT_SYSTEM` branch).

use reflux_model::{Message, MessageType, PartialUser, Snowflake};
use reflux_store::Store;
use rune::{El, Span, col, el, icon, rich, row, text};

use super::markdown::{self, Md};
use super::{Intent, ShellAccess, ShellState};
use crate::i18n::t;
use crate::icons;
use crate::theme::{Palette, dim, font};

/// `MessageTypes.CLIENT_SYSTEM` and `FLUXERBOT_ID`.
pub const KIND: MessageType = MessageType(99);
pub const FLUXERBOT: Snowflake = Snowflake(0);

pub fn is(m: &Message) -> bool {
    m.kind == KIND && m.author.id == FLUXERBOT
}

/// The message `createSystemMessage` builds, for [`reflux_store::Store::add_local_message`].
pub fn build(channel: Snowflake, content: String, now_ms: u64) -> Message {
    let id = Snowflake(Snowflake::from_timestamp_ms(now_ms).0 | fastrand::u64(..1 << 22));
    let author = PartialUser { id: FLUXERBOT, username: "Fluxerbot".into(), discriminator: "0000".into(), bot: true, system: true, ..Default::default() };
    Message { id, channel_id: channel, author, kind: KIND, content, timestamp: crate::app::iso8601(now_ms), nonce: Some(id.to_string()), ..Default::default() }
}

/// The message row: avatar, "Fluxerbot SYSTEM — time", the content, and the notice.
pub fn view<S: 'static>(store: &Store, st: &ShellState, m: &Message, md: &Md, p: &Palette, acc: ShellAccess<S>) -> El {
    let (ch, id) = (m.channel_id, m.id);
    let ms = super::chat::message_ms(m);
    let name = "Fluxerbot";
    let time = format!(" — {}", super::chat::format_relative(ms, st.now_ms));
    // `UserTag` (system): uppercase 10/600 on brand, 4px corners.
    let tag = row()
        .ml(4.0)
        .px(5.6)
        .py(2.0)
        .min_h(15.0)
        .radius(4.0)
        .center()
        .fill(p.brand_primary)
        .child(text(t("System").to_uppercase()).font(font(10.0, 600.0).lh(10.0)).color(p.brand_primary_fill));
    let head = row()
        .center_y()
        .child(text(name).font(font(16.0, 500.0).lh(dim::MESSAGE_LINE_HEIGHT)).color(p.text_primary))
        .child(tag)
        .child(text(time).font(font(dim::MESSAGE_TIMESTAMP_FONT_SIZE, 400.0).lh(dim::MESSAGE_LINE_HEIGHT)).color(p.message_timestamp_color));
    // `.systemMessageContainer`: an eye, then the notice with a "Dismiss" link.
    let notice = t("Only you can see this message.");
    let dismiss = t("Dismiss");
    let line = format!("{notice} {dismiss}");
    let at = notice.len() + 1;
    let link = rich(line.clone(), vec![Span::new(at, line.len()).color(p.text_link)])
        .font(font(dim::MESSAGE_TIMESTAMP_FONT_SIZE, 400.0).lh(16.0))
        .color(p.text_tertiary)
        .link(rune::Link::new(at..line.len(), "dismiss").hover(rune::LinkPaint::new().underline(p.text_link)))
        .on_link(move |s: &mut S, _: &str, _: &mut rune::Tx| acc(s).intents.push(Intent::DropLocal { channel: ch, nonce: id }));
    let footer = row().gap(4.0).py(4.0).center_y().child(icon(&icons::EYE, 16.0).color(p.text_tertiary)).child(link);
    let content = col().grow(1.0).min_w(0.0).child(head).child(markdown::render(&m.content, md)).child(footer);
    let gutter = col().w(dim::MESSAGE_AVATAR_SIZE).pt(dim::MESSAGE_AVATAR_ALIGN_OFFSET).child(super::avatar(store, st, FLUXERBOT, dim::MESSAGE_AVATAR_SIZE, None, p.background_secondary_lighter, p));
    // `.messageClientSystem`: the reply tint and a brand bar, hovered or not.
    col()
        .w_full()
        .pr(dim::CHAT_HORIZONTAL_PADDING_DEFAULT + dim::MESSAGE_ACTION_BAR_OFFSET)
        .py(dim::MESSAGE_SPACING_Y)
        .fill(p.message_reply_bg)
        .child(el().pin(Some(0.0), Some(0.0), None, Some(0.0)).w(dim::MESSAGE_HIGHLIGHT_BAR_WIDTH).fill(p.brand_primary_light))
        .child(row().w_full().items_start().child(el().w(dim::CHAT_HORIZONTAL_PADDING_DEFAULT)).child(gutter).child(el().w(dim::MESSAGE_GUTTER)).child(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_fluxerbot_note() {
        let m = build(Snowflake(5), "hi".into(), 1_800_000_000_000);
        assert!(is(&m) && m.author.system && m.channel_id == Snowflake(5));
        assert!(!super::super::chat::is_user_message(&m), "no reply, edit or pin");
    }
}
