//! Messages, attachments, embeds and reactions (`http-api/messages.mdx`).
//!
//! Fluxer documents no message components.

use std::collections::HashMap;

use crate::{Channel, ChannelType, GuildMember, PartialUser, Snowflake, Timestamp};

model! {
    /// Message object, plus the extra fields `MESSAGE_CREATE`/`MESSAGE_UPDATE` add.
    pub struct Message {
        pub id: Snowflake,
        pub channel_id: Snowflake,
        pub author: PartialUser,
        pub webhook_id: Option<Snowflake>,
        #[serde(rename = "type")]
        pub kind: MessageType,
        pub flags: MessageFlags,
        pub content: String,
        pub timestamp: Timestamp,
        pub edited_timestamp: Option<Timestamp>,
        pub pinned: bool,
        pub mention_everyone: bool,
        pub tts: bool,
        pub mentions: Vec<PartialUser>,
        pub mention_roles: Vec<Snowflake>,
        pub mention_channels: Vec<ChannelMention>,
        /// Referred-to users that weren't mentioned (resolve snapshot mentions here).
        pub users: Vec<PartialUser>,
        pub embeds: Vec<Embed>,
        pub attachments: Vec<Attachment>,
        pub stickers: Vec<StickerItem>,
        #[serde(deserialize_with = "crate::null_default")]
        pub reactions: Vec<Reaction>,
        pub message_reference: Option<MessageReference>,
        pub message_snapshots: Vec<MessageSnapshot>,
        pub nonce: Option<String>,
        pub call: Option<MessageCall>,
        /// Omitted when not a reply, `Some(None)` when the replied-to message is gone.
        #[serde(deserialize_with = "crate::double_option", skip_serializing_if = "Option::is_none")]
        pub referenced_message: Option<Option<Box<Message>>>,
        pub thread: Option<Box<Channel>>,
        // Gateway-only additions
        pub channel_type: Option<ChannelType>,
        pub nicks: HashMap<Snowflake, String>,
        pub mention_here: bool,
        pub guild_id: Option<Snowflake>,
        /// Author's member object without `user`.
        pub member: Option<GuildMember>,
        /// A `MESSAGE_UPDATE` carried `reactions` (even `null`): it replaces the list, as
        /// `'reactions' in updates` does; set by `DispatchEvent::parse`.
        #[serde(skip)]
        pub has_reactions: bool,
    }

    pub struct Attachment {
        pub id: Snowflake,
        pub filename: String,
        pub title: Option<String>,
        pub description: Option<String>,
        pub content_type: String,
        pub content_hash: Option<String>,
        pub size: u64,
        /// Signed; null once expired.
        pub url: Option<String>,
        pub proxy_url: Option<String>,
        pub width: Option<u32>,
        pub height: Option<u32>,
        pub placeholder: Option<String>,
        pub flags: AttachmentFlags,
        pub nsfw: bool,
        pub duration: Option<f64>,
        /// Base64 voice waveform.
        pub waveform: Option<String>,
        pub expires_at: Option<Timestamp>,
        pub expired: bool,
    }

    /// Emoji in reactions (`id` absent for Unicode emoji).
    pub struct ReactionEmoji {
        pub id: Option<Snowflake>,
        pub name: String,
        /// Absent does not mean false on gateway reaction events.
        pub animated: Option<bool>,
    }

    pub struct Reaction {
        pub emoji: ReactionEmoji,
        pub count: u32,
        pub me: bool,
    }

    pub struct MessageReference {
        pub channel_id: Snowflake,
        pub message_id: Option<Snowflake>,
        pub guild_id: Option<Snowflake>,
        /// 0 default (reply), 1 forward.
        #[serde(rename = "type")]
        pub kind: u8,
    }

    pub struct MessageSnapshot {
        pub content: String,
        pub timestamp: Timestamp,
        pub edited_timestamp: Option<Timestamp>,
        pub mentions: Vec<Snowflake>,
        pub mention_roles: Vec<Snowflake>,
        pub mention_channels: Vec<ChannelMention>,
        pub embeds: Vec<Embed>,
        pub attachments: Vec<Attachment>,
        pub stickers: Vec<StickerItem>,
        #[serde(rename = "type")]
        pub kind: MessageType,
        pub flags: MessageFlags,
    }

    pub struct MessageCall {
        pub participants: Vec<Snowflake>,
        pub ended_timestamp: Option<Timestamp>,
    }

    pub struct ChannelMention {
        pub id: Snowflake,
        pub name: String,
        #[serde(rename = "type")]
        pub kind: ChannelType,
    }

    pub struct StickerItem {
        pub id: Snowflake,
        pub name: String,
        pub animated: bool,
    }

    /// Embed. Also the rich embed input (only `rich` fields may be sent).
    pub struct Embed {
        /// `rich`, `link`, `image`, `video`, `audio`, `gifv`, `article`, `bluesky`.
        #[serde(rename = "type")]
        pub kind: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub timestamp: Option<Timestamp>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub color: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub author: Option<EmbedAuthor>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub provider: Option<EmbedProvider>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub thumbnail: Option<EmbedMedia>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub image: Option<EmbedMedia>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub video: Option<EmbedMedia>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub audio: Option<EmbedMedia>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub footer: Option<EmbedFooter>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pub fields: Vec<EmbedField>,
        #[serde(skip_serializing_if = "crate::is_false")]
        pub nsfw: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub html: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub html_width: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub html_height: Option<u32>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pub children: Vec<Embed>,
    }

    pub struct EmbedAuthor {
        pub name: String,
        pub url: Option<String>,
        pub icon_url: Option<String>,
        pub proxy_icon_url: Option<String>,
    }

    pub struct EmbedMedia {
        pub url: String,
        pub proxy_url: Option<String>,
        pub width: Option<u32>,
        pub height: Option<u32>,
        pub duration: Option<f64>,
        pub description: Option<String>,
        pub content_type: Option<String>,
        pub content_hash: Option<String>,
        pub placeholder: Option<String>,
        pub flags: AttachmentFlags,
    }

    pub struct EmbedFooter {
        pub text: String,
        pub icon_url: Option<String>,
        pub proxy_icon_url: Option<String>,
    }

    pub struct EmbedField {
        pub name: String,
        pub value: String,
        pub inline: bool,
    }

    pub struct EmbedProvider {
        pub name: String,
        pub url: Option<String>,
    }

    /// `POST /channels/{id}/messages` JSON body (or multipart `payload_json`).
    pub struct CreateMessage {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub nonce: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub content: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pub embeds: Vec<Embed>,
        /// Pre-uploaded or multipart attachment metadata; shapes vary, so kept as JSON.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pub attachments: Vec<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub message_reference: Option<MessageReferenceInput>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub allowed_mentions: Option<AllowedMentions>,
        #[serde(skip_serializing_if = "MessageFlags::is_empty")]
        pub flags: MessageFlags,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub favorite_meme_id: Option<Snowflake>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pub sticker_ids: Vec<Snowflake>,
        #[serde(skip_serializing_if = "crate::is_false")]
        pub tts: bool,
    }

    pub struct MessageReferenceInput {
        pub message_id: Snowflake,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub channel_id: Option<Snowflake>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub guild_id: Option<Snowflake>,
        /// 0 reply, 1 forward.
        #[serde(rename = "type")]
        pub kind: u8,
    }

    pub struct AllowedMentions {
        /// `users`, `roles`, `everyone`.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub parse: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pub users: Vec<Snowflake>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pub roles: Vec<Snowflake>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub replied_user: Option<bool>,
    }
}

int_enum! {
    pub struct MessageType(u16) {
        DEFAULT = 0,
        RECIPIENT_ADD = 1,
        RECIPIENT_REMOVE = 2,
        CALL = 3,
        CHANNEL_NAME_CHANGE = 4,
        CHANNEL_ICON_CHANGE = 5,
        CHANNEL_PINNED_MESSAGE = 6,
        USER_JOIN = 7,
        CHANNEL_FOLLOW_ADD = 12,
        THREAD_CREATED = 18,
        REPLY = 19,
        THREAD_STARTER_MESSAGE = 21,
    }
}

int_flags! {
    pub struct MessageFlags {
        const CROSSPOSTED = 1 << 0;
        const IS_CROSSPOST = 1 << 1;
        const SUPPRESS_EMBEDS = 1 << 2;
        const SOURCE_MESSAGE_DELETED = 1 << 3;
        const HAS_THREAD = 1 << 5;
        const FAILED_TO_MENTION_SOME_ROLES_IN_THREAD = 1 << 8;
        const SUPPRESS_NOTIFICATIONS = 1 << 12;
        const VOICE_MESSAGE = 1 << 13;
    }
    pub struct AttachmentFlags {
        const IS_SPOILER = 1 << 3;
        const CONTAINS_EXPLICIT_MEDIA = 1 << 4;
        const IS_ANIMATED = 1 << 5;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_example() {
        let m: Message = serde_json::from_str(
            r#"{"id":"1501320000000000000","channel_id":"1501314428688998182",
            "author":{"id":"1501314428688990000","username":"ada","discriminator":"0001",
            "avatar":"a1b2c3d4e5f60718293a4b5c6d7e8f90"},"type":0,"flags":0,"content":"shipping it",
            "timestamp":"2026-08-31T09:14:02.000Z","edited_timestamp":null,"pinned":false,
            "mention_everyone":false,"tts":false,"mentions":[],"mention_roles":[],"embeds":[],
            "attachments":[{"id":"1501320000000000001","filename":"diagram.png","title":null,
            "description":"Sequence diagram","content_type":"image/png","content_hash":"9f86",
            "size":18422,"url":"https://m/x.png","proxy_url":"https://m/x.png","width":1280,
            "height":720,"flags":8,"expires_at":null}],"stickers":[]}"#,
        )
        .unwrap();
        assert_eq!(m.content, "shipping it");
        assert_eq!(m.referenced_message, None);
        assert!(m.attachments[0].flags.contains(AttachmentFlags::IS_SPOILER));
    }

    #[test]
    fn reply_to_deleted_and_embed() {
        let m: Message = serde_json::from_str(
            r#"{"id":"1","channel_id":"2","type":19,"referenced_message":null,
            "embeds":[{"type":"rich","title":"Release 2.4.0","url":"https://example.com/releases/2.4.0",
            "color":5793266,"footer":{"text":"example.com"},
            "fields":[{"name":"Platform","value":"Desktop","inline":true}]}]}"#,
        )
        .unwrap();
        assert_eq!(m.referenced_message, Some(None));
        assert_eq!(m.kind, MessageType::REPLY);
        assert!(m.embeds[0].fields[0].inline);
    }

    #[test]
    fn create_message_serializes_minimal() {
        let body = CreateMessage { content: Some("hi".into()), ..Default::default() };
        assert_eq!(serde_json::to_string(&body).unwrap(), r#"{"content":"hi"}"#);
    }
}
