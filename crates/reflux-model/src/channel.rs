//! Channels, threads, forums and read states (`http-api/channels.mdx`, `threads.mdx`, `forums.mdx`,
//! `thread-members.mdx`, `read-states.mdx`).

use std::collections::HashMap;

use crate::{GuildMember, PartialUser, Permissions, Presence, Snowflake, Timestamp, user::MuteConfig};

model! {
    /// Channel object. Threads and forum/media channels are channels with extra fields.
    pub struct Channel {
        pub id: Snowflake,
        #[serde(rename = "type")]
        pub kind: ChannelType,
        pub guild_id: Option<Snowflake>,
        pub name: Option<String>,
        pub topic: Option<String>,
        /// Link channels.
        pub url: Option<String>,
        /// Group DM icon hash.
        pub icon: Option<String>,
        pub owner_id: Option<Snowflake>,
        pub position: i32,
        pub parent_id: Option<Snowflake>,
        pub bitrate: Option<u32>,
        pub user_limit: Option<u32>,
        pub voice_connection_limit: Option<u32>,
        pub rtc_region: Option<String>,
        pub rtc_p2p: bool,
        pub last_message_id: Option<Snowflake>,
        pub last_pin_timestamp: Option<Timestamp>,
        pub permission_overwrites: Vec<PermissionOverwrite>,
        pub recipients: Vec<PartialUser>,
        pub nsfw: bool,
        pub nsfw_override: Option<bool>,
        /// 0 inherit, 1 use `content_warning_text`.
        pub content_warning_level: u8,
        pub content_warning_text: Option<String>,
        pub rate_limit_per_user: u32,
        pub nicks: HashMap<Snowflake, String>,
        pub flags: ChannelFlags,
        // Threads
        pub thread_metadata: Option<ThreadMetadata>,
        pub message_count: u32,
        pub total_message_sent: u32,
        pub member_count: u32,
        pub applied_tags: Vec<Snowflake>,
        pub member_ids_preview: Vec<Snowflake>,
        pub member: Option<ThreadMember>,
        /// Only on `THREAD_CREATE`.
        #[serde(skip_serializing_if = "crate::is_false")]
        pub newly_created: bool,
        // Forum / media channels
        pub default_auto_archive_duration: Option<u32>,
        pub default_thread_rate_limit_per_user: u32,
        pub available_tags: Vec<ForumTag>,
        pub default_reaction_emoji: Option<DefaultReaction>,
        /// 0 latest activity, 1 creation time.
        pub default_sort_order: Option<u8>,
        /// 0 default, 1 list, 2 grid.
        pub default_forum_layout: u8,
        /// `match_some` or `match_all`.
        pub default_tag_setting: Option<String>,
    }

    pub struct PermissionOverwrite {
        pub id: Snowflake,
        #[serde(rename = "type")]
        pub kind: OverwriteType,
        pub allow: Permissions,
        pub deny: Permissions,
    }

    pub struct ThreadMetadata {
        pub archived: bool,
        /// Minutes: 60, 1440, 4320, 10080.
        pub auto_archive_duration: u32,
        pub archive_timestamp: Timestamp,
        pub locked: bool,
        pub invitable: Option<bool>,
        pub create_timestamp: Timestamp,
    }

    /// Thread member; also `THREAD_MEMBER_UPDATE` (with `guild_id`), Thread Members Update entries
    /// and Thread Member List Update items.
    pub struct ThreadMember {
        /// Thread ID; omitted inside Ready / Guild Create.
        pub id: Option<Snowflake>,
        pub user_id: Option<Snowflake>,
        pub join_timestamp: Timestamp,
        pub flags: ThreadMemberFlags,
        pub muted: bool,
        pub mute_config: Option<MuteConfig>,
        pub member: Option<GuildMember>,
        pub presence: Option<Presence>,
        pub guild_id: Option<Snowflake>,
    }

    pub struct ForumTag {
        pub id: Snowflake,
        pub name: String,
        pub moderated: bool,
        pub emoji_id: Option<Snowflake>,
        pub emoji_name: Option<String>,
    }

    pub struct DefaultReaction {
        pub emoji_id: Option<Snowflake>,
        pub emoji_name: Option<String>,
    }

    /// Per-channel read state (Ready `read_states`).
    pub struct ReadState {
        /// Channel ID.
        pub id: Snowflake,
        pub mention_count: u32,
        pub last_message_id: Option<Snowflake>,
        pub last_pin_timestamp: Option<Timestamp>,
        pub version: Option<String>,
        pub flags: ReadStateFlags,
    }

    /// Voice region (Ready `rtc_regions`, channel `rtc_region` IDs).
    pub struct RtcRegion {
        pub id: String,
        pub name: String,
        pub emoji: String,
    }
}

int_enum! {
    pub struct ChannelType(u16) {
        GUILD_TEXT = 0,
        DM = 1,
        GUILD_VOICE = 2,
        GROUP_DM = 3,
        GUILD_CATEGORY = 4,
        GUILD_ANNOUNCEMENT = 5,
        ANNOUNCEMENT_THREAD = 10,
        PUBLIC_THREAD = 11,
        PRIVATE_THREAD = 12,
        GUILD_FORUM = 15,
        GUILD_MEDIA = 16,
        GUILD_LINK = 998,
        DM_PERSONAL_NOTES = 999,
    }
    pub struct OverwriteType(u8) { ROLE = 0, MEMBER = 1 }
}

impl ChannelType {
    pub fn is_thread(self) -> bool {
        matches!(self, Self::ANNOUNCEMENT_THREAD | Self::PUBLIC_THREAD | Self::PRIVATE_THREAD)
    }
    pub fn is_private(self) -> bool {
        matches!(self, Self::DM | Self::GROUP_DM | Self::DM_PERSONAL_NOTES)
    }
}

int_flags! {
    pub struct ChannelFlags {
        const PINNED = 1 << 1;
        const REQUIRE_TAG = 1 << 4;
        const HIDE_MEDIA_DOWNLOAD_OPTIONS = 1 << 15;
    }
    pub struct ThreadMemberFlags {
        const HAS_INTERACTED = 1 << 0;
        const ALL_MESSAGES = 1 << 1;
        const ONLY_MENTIONS = 1 << 2;
        const NO_MESSAGES = 1 << 3;
    }
    pub struct ReadStateFlags {
        const IS_GUILD_CHANNEL = 1 << 0;
        const IS_THREAD = 1 << 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_example() {
        let c: Channel = serde_json::from_str(
            r#"{"id":"1501314428688998182","type":0,"guild_id":"1501314428688990000","name":"general",
            "topic":"Anything goes","position":3,"parent_id":"1501314428688991111",
            "permission_overwrites":[{"id":"1501314428688990000","type":0,"allow":"0","deny":"2048"}],
            "nsfw":false,"nsfw_override":null,"content_warning_level":0,"content_warning_text":null,
            "rate_limit_per_user":0,"last_message_id":"1501320000000000000","last_pin_timestamp":null}"#,
        )
        .unwrap();
        assert_eq!(c.kind, ChannelType::GUILD_TEXT);
        assert_eq!(c.permission_overwrites[0].deny, Permissions::SEND_MESSAGES);
    }

    #[test]
    fn thread_example() {
        let c: Channel = serde_json::from_str(
            r#"{"id":"1501320100000000000","type":11,"guild_id":"1501314428688990000",
            "parent_id":"1501314428688998182","owner_id":"1501314428688992222","name":"Release checklist",
            "last_message_id":"1501320500000000000","last_pin_timestamp":null,"rate_limit_per_user":0,
            "flags":0,"thread_metadata":{"archived":false,"auto_archive_duration":4320,
            "archive_timestamp":"2026-05-05T20:30:05.018Z","locked":false,
            "create_timestamp":"2026-05-05T20:30:05.018Z"},"message_count":12,"total_message_sent":14,
            "member_count":3}"#,
        )
        .unwrap();
        assert!(c.kind.is_thread());
        assert_eq!(c.thread_metadata.unwrap().auto_archive_duration, 4320);
    }
}
