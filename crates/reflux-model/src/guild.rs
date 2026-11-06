//! Guilds, roles, members, expressions, bans, invites and voice states.

use crate::{Channel, PartialUser, Permissions, Presence, Snowflake, Timestamp, user::MentionPreference};

model! {
    /// Guild object (`http-api/guilds.mdx`). Also Guild Ready `properties` and `GUILD_UPDATE`.
    pub struct Guild {
        pub id: Snowflake,
        pub name: String,
        pub icon: Option<String>,
        pub banner: Option<String>,
        pub banner_width: Option<u32>,
        pub banner_height: Option<u32>,
        pub splash: Option<String>,
        pub splash_width: Option<u32>,
        pub splash_height: Option<u32>,
        /// 0 center, 1 left, 2 right.
        pub splash_card_alignment: u8,
        pub embed_splash: Option<String>,
        pub embed_splash_width: Option<u32>,
        pub embed_splash_height: Option<u32>,
        pub vanity_url_code: Option<String>,
        pub owner_id: Snowflake,
        pub system_channel_id: Option<Snowflake>,
        pub system_channel_flags: SystemChannelFlags,
        pub rules_channel_id: Option<Snowflake>,
        pub afk_channel_id: Option<Snowflake>,
        pub afk_timeout: u32,
        pub features: Vec<String>,
        pub badge_ids: Vec<String>,
        /// 0 none .. 3 high.
        pub verification_level: u8,
        /// 0 none, 1 elevated.
        pub mfa_level: u8,
        /// 0 safe, 3 age restricted.
        pub nsfw_level: u8,
        pub nsfw: bool,
        pub content_warning_level: u8,
        pub content_warning_text: Option<String>,
        pub explicit_content_filter: u8,
        /// 0 all messages, 1 only mentions.
        pub default_message_notifications: u8,
        pub disabled_operations: DisabledOperations,
        pub message_history_cutoff: Option<Timestamp>,
        /// Only from List current user guilds.
        pub permissions: Option<Permissions>,
        pub roles: Vec<Role>,
        pub emojis: Vec<Emoji>,
        pub stickers: Vec<Sticker>,
        pub channels: Vec<Channel>,
        pub member_count: Option<u32>,
        pub online_count: Option<u32>,
        pub approximate_member_count: Option<u32>,
        pub approximate_presence_count: Option<u32>,
        pub threads_active: bool,
        pub visiting: bool,
    }

    /// Partial guild object (invites).
    pub struct PartialGuild {
        pub id: Snowflake,
        pub name: String,
        pub icon: Option<String>,
        pub banner: Option<String>,
        pub banner_width: Option<u32>,
        pub banner_height: Option<u32>,
        pub splash: Option<String>,
        pub splash_width: Option<u32>,
        pub splash_height: Option<u32>,
        pub splash_card_alignment: u8,
        pub embed_splash: Option<String>,
        pub embed_splash_width: Option<u32>,
        pub embed_splash_height: Option<u32>,
        pub features: Vec<String>,
        pub badge_ids: Vec<String>,
        pub visits_enabled: Option<bool>,
    }

    /// Guild state in Ready, `GUILD_CREATE` and `GUILD_SYNC`.
    pub struct GuildReady {
        pub id: Snowflake,
        /// Absent on unavailable guilds.
        pub properties: Option<Guild>,
        pub roles: Vec<Role>,
        pub channels: Vec<Channel>,
        pub emojis: Vec<Emoji>,
        pub stickers: Vec<Sticker>,
        /// Only the own member and members in `voice_states`.
        pub members: Vec<GuildMember>,
        pub member_count: u32,
        pub online_count: u32,
        pub presences: Vec<Presence>,
        pub voice_states: Vec<VoiceState>,
        pub joined_at: Option<Timestamp>,
        pub unavailable: bool,
        pub visiting: bool,
        pub unavailable_hidden: bool,
        pub threads: Vec<Channel>,
    }

    /// Guild role (`http-api/permissions.mdx`).
    pub struct Role {
        pub id: Snowflake,
        pub name: String,
        pub color: u32,
        pub position: i32,
        pub hoist_position: Option<i32>,
        pub permissions: Permissions,
        pub hoist: bool,
        pub mentionable: bool,
        pub icon: Option<String>,
        pub unicode_emoji: Option<String>,
    }

    /// Guild member. In Ready / bot Guild Create, `user` has only `id`.
    pub struct GuildMember {
        pub user: PartialUser,
        pub nick: Option<String>,
        pub avatar: Option<String>,
        pub banner: Option<String>,
        pub accent_color: Option<u32>,
        /// Never lists @everyone.
        pub roles: Vec<Snowflake>,
        pub joined_at: Timestamp,
        pub mute: bool,
        pub deaf: bool,
        /// Timeout is active only while this is in the future.
        pub communication_disabled_until: Option<Timestamp>,
        pub profile_flags: MemberProfileFlags,
        pub mention_flags: Option<MentionPreference>,
        /// Only on member list items (`GUILD_MEMBER_LIST_UPDATE`).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub presence: Option<Box<Presence>>,
    }

    /// Guild emoji.
    pub struct Emoji {
        pub id: Snowflake,
        pub name: String,
        pub animated: bool,
        pub user: Option<PartialUser>,
    }

    pub struct Sticker {
        pub id: Snowflake,
        pub name: String,
        pub description: String,
        pub tags: Vec<String>,
        pub animated: bool,
        pub user: Option<PartialUser>,
    }

    pub struct Ban {
        pub user: PartialUser,
        pub reason: Option<String>,
        pub moderator_id: Snowflake,
        pub banned_at: Timestamp,
        pub expires_at: Option<Timestamp>,
    }

    /// Invite object, with the metadata fields when listed/created (`INVITE_CREATE`).
    pub struct Invite {
        pub code: String,
        /// 0 guild, 1 group DM.
        #[serde(rename = "type")]
        pub kind: u8,
        pub guild: Option<PartialGuild>,
        pub channel: Option<InviteChannel>,
        pub inviter: Option<PartialUser>,
        pub member_count: Option<u32>,
        pub presence_count: Option<u32>,
        pub expires_at: Option<Timestamp>,
        pub temporary: bool,
        pub created_at: Option<Timestamp>,
        pub uses: u32,
        pub max_uses: u32,
        pub max_age: Option<u32>,
        pub guild_id: Option<Snowflake>,
        pub channel_id: Option<Snowflake>,
    }

    pub struct InviteChannel {
        pub id: Snowflake,
        pub name: Option<String>,
        #[serde(rename = "type")]
        pub kind: crate::ChannelType,
        pub recipients: Vec<InviteRecipient>,
    }

    pub struct InviteRecipient {
        pub username: String,
    }

    /// Voice state object (`VOICE_STATE_UPDATE`); null `channel_id` means the user left.
    pub struct VoiceState {
        pub guild_id: Option<Snowflake>,
        pub channel_id: Option<Snowflake>,
        pub user_id: Option<Snowflake>,
        pub connection_id: Option<String>,
        pub session_id: Option<String>,
        pub member: Option<GuildMember>,
        pub mute: bool,
        pub deaf: bool,
        pub self_mute: bool,
        pub self_deaf: bool,
        pub self_video: bool,
        pub self_stream: bool,
        pub is_mobile: bool,
        pub suppress: bool,
        pub viewer_stream_keys: Vec<String>,
        pub e2ee_capable: bool,
        pub p2p: bool,
        pub version: u64,
    }
}

int_flags! {
    pub struct SystemChannelFlags {
        const SUPPRESS_JOIN_NOTIFICATIONS = 1 << 0;
    }
    pub struct DisabledOperations {
        const PUSH_NOTIFICATIONS = 1 << 0;
        const EVERYONE_MENTIONS = 1 << 1;
        const TYPING_EVENTS = 1 << 2;
        const INSTANT_INVITES = 1 << 3;
        const SEND_MESSAGE = 1 << 4;
        const REACTIONS = 1 << 5;
        const MEMBER_LIST_UPDATES = 1 << 6;
    }
    pub struct MemberProfileFlags {
        const AVATAR_UNSET = 1 << 0;
        const BANNER_UNSET = 1 << 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guild_example() {
        let g: Guild = serde_json::from_str(
            r#"{"id":"1489002177550843904","name":"Ada's Workshop","icon":"a_9f2c1d4e","banner":null,
            "splash":null,"vanity_url_code":"workshop","owner_id":"1489002177550843905",
            "system_channel_id":"1489002177550843907","system_channel_flags":0,"afk_channel_id":null,
            "afk_timeout":300,"features":["ANIMATED_ICON","BANNER","VANITY_URL"],"verification_level":1,
            "mfa_level":0,"nsfw_level":0,"nsfw":false,"content_warning_level":0,"content_warning_text":null,
            "explicit_content_filter":1,"default_message_notifications":1,"disabled_operations":0,
            "message_history_cutoff":null}"#,
        )
        .unwrap();
        assert_eq!(g.afk_timeout, 300);
        assert_eq!(g.features.len(), 3);
    }

    #[test]
    fn role_member_ban_invite_examples() {
        let r: Role = serde_json::from_str(
            r#"{"id":"1489002177550843904","name":"Moderator","color":3447003,"position":4,
            "hoist_position":1,"permissions":"1099511637006","hoist":true,"mentionable":false,
            "icon":"5d4c7a1e","unicode_emoji":null}"#,
        )
        .unwrap();
        assert!(r.permissions.contains(Permissions::MODERATE_MEMBERS));

        let m: GuildMember = serde_json::from_str(
            r#"{"user":{"id":"1489002177550843906","username":"ada","discriminator":"0042"},
            "nick":"Ada L.","avatar":null,"banner":null,"accent_color":3447003,
            "roles":["1489002177550843910"],"joined_at":"2026-08-14T09:12:44.000Z","mute":false,
            "deaf":false,"communication_disabled_until":null}"#,
        )
        .unwrap();
        assert_eq!(m.nick.as_deref(), Some("Ada L."));

        let b: Ban = serde_json::from_str(
            r#"{"user":{"id":"1489002177550843906","username":"spammer","discriminator":"0042"},
            "reason":"Advertising","moderator_id":"1489002177550843905",
            "banned_at":"2026-08-14T09:12:44.000Z","expires_at":null}"#,
        )
        .unwrap();
        assert_eq!(b.reason.as_deref(), Some("Advertising"));

        let i: Invite = serde_json::from_str(
            r#"{"code":"aB3dEf7h","type":0,"guild":{"id":"1489002177550843904","name":"Example guild"},
            "channel":{"id":"1489002177550843905","name":"general","type":0},
            "inviter":{"id":"1501314428688998182","username":"example"},"member_count":412,
            "presence_count":87,"expires_at":null,"temporary":false}"#,
        )
        .unwrap();
        assert_eq!(i.guild.unwrap().name, "Example guild");
    }
}
