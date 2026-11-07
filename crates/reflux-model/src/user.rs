//! Users, settings, relationships and presence (`http-api/users*.mdx`, `gateway/events.md`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{GuildMember, Snowflake, Timestamp};

model! {
    /// Public account representation (partial user object).
    pub struct PartialUser {
        pub id: Snowflake,
        pub username: String,
        pub discriminator: String,
        pub global_name: Option<String>,
        /// Hash; animated when it starts with `a_`.
        pub avatar: Option<String>,
        pub avatar_color: Option<u32>,
        #[serde(skip_serializing_if = "crate::is_false")]
        pub bot: bool,
        #[serde(skip_serializing_if = "crate::is_false")]
        pub system: bool,
        pub flags: UserFlags,
        pub mention_flags: Option<MentionPreference>,
    }

    /// The current account as its owner sees it (user object, private representation).
    pub struct User {
        #[serde(flatten)]
        pub base: PartialUser,
        pub is_staff: bool,
        pub acls: Vec<String>,
        pub traits: Vec<String>,
        pub email: Option<String>,
        pub email_bounced: bool,
        pub bio: Option<String>,
        pub pronouns: Option<String>,
        pub accent_color: Option<u32>,
        pub timezone: Option<String>,
        pub timezone_privacy_flags: ProfileFieldPrivacyFlags,
        pub banner: Option<String>,
        pub banner_color: Option<u32>,
        pub mfa_enabled: bool,
        pub authenticator_types: Vec<AuthenticatorType>,
        pub verified: bool,
        pub account_limited: bool,
        pub premium_type: Option<PremiumType>,
        pub premium_since: Option<Timestamp>,
        pub premium_until: Option<Timestamp>,
        pub premium_will_cancel: bool,
        pub premium_billing_cycle: Option<String>,
        pub premium_lifetime_sequence: Option<i64>,
        pub premium_grace_ends_at: Option<Timestamp>,
        pub premium_discriminator: bool,
        pub hidden_profile_badges: Vec<String>,
        pub premium_badge_masked: bool,
        pub premium_badge_hidden: bool,
        pub premium_badge_timestamp_hidden: bool,
        pub premium_badge_sequence_hidden: bool,
        pub operator_pass_badge_timestamp_hidden: bool,
        pub operator_pass_badge_count_hidden: bool,
        pub has_verified_phone: bool,
        pub required_actions: Vec<String>,
        pub premium_purchase_disabled: bool,
        pub premium_enabled_override: bool,
        pub premium_perks_disabled: bool,
        pub password_last_changed_at: Option<Timestamp>,
        pub last_voice_activity_sharing_change_at: Option<Timestamp>,
        pub nsfw_allowed: bool,
        pub has_dismissed_premium_onboarding: bool,
        pub has_ever_purchased: bool,
        pub has_unread_gift_inventory: bool,
        pub unread_gift_inventory_count: u32,
        pub pending_bulk_message_deletion: Option<PendingBulkMessageDeletion>,
        pub age_verified_adult: bool,
        pub terms_agreed_at: Option<Timestamp>,
        pub privacy_agreed_at: Option<Timestamp>,
        pub claimed: Option<bool>,
        /// `unclaimed`, `unverified` or `verified`.
        pub account_tier: Option<String>,
        /// `add_email`, `confirm_email` or `password`.
        pub verification_need: Option<String>,
        pub has_password: Option<bool>,
        pub date_of_birth_required: bool,
        pub profile_restricted: bool,
    }

    pub struct PendingBulkMessageDeletion {
        pub scheduled_at: Timestamp,
        pub channel_count: u32,
        pub message_count: u32,
    }

    /// Account-wide settings (`USER_SETTINGS_UPDATE`, Ready `user_settings`).
    pub struct UserSettings {
        pub status: Status,
        pub status_resets_at: Option<Timestamp>,
        pub status_resets_to: Option<Status>,
        /// `dark`, `dark_legacy`, `coal`, `light` or `system`.
        pub theme: String,
        pub locale: String,
        pub restricted_guilds: Vec<Snowflake>,
        pub bot_restricted_guilds: Vec<Snowflake>,
        pub default_guilds_restricted: bool,
        pub bot_default_guilds_restricted: bool,
        pub inline_attachment_media: bool,
        pub inline_embed_media: bool,
        pub gif_auto_play: bool,
        pub render_embeds: bool,
        pub render_reactions: bool,
        pub animate_emoji: bool,
        /// 0 always, 1 on interaction, 2 never.
        pub animate_stickers: u8,
        /// 0 always, 1 on click, 2 if moderator.
        pub render_spoilers: u8,
        pub message_display_compact: bool,
        pub friend_source_flags: FriendSourceFlags,
        pub incoming_call_flags: IncomingCallFlags,
        pub group_dm_add_permission_flags: GroupDmAddPermissionFlags,
        pub guild_folders: Vec<GuildFolder>,
        pub custom_status: Option<CustomStatus>,
        pub afk_timeout: u32,
        /// 0 auto, 1 12-hour, 2 24-hour.
        pub time_format: u8,
        pub developer_mode: bool,
        pub trusted_domains: Vec<String>,
        pub default_hide_muted_channels: bool,
        /// 0 show, 1 blur, 2 block.
        pub sensitive_content_friend_dm_filter: u8,
        pub sensitive_content_non_friend_dm_filter: u8,
        pub sensitive_content_guild_filter: u8,
        pub allow_mature_content_on_ios: bool,
        pub suppress_unprivileged_self_mentions: bool,
        pub suppress_unprivileged_self_mentions_bypass_user_ids: Vec<Snowflake>,
        pub staff_dm_access_user_ids: Vec<Snowflake>,
        /// Base64 protobuf (`settings-protobuf.md`).
        pub synced_preferences: String,
        /// 0 all guilds, 1 small guilds only, 2 friends only.
        pub profile_privacy: u8,
        pub default_share_voice_activity: bool,
        pub privacy_setup_version: u32,
        pub privacy_setup_completed_at: Option<Timestamp>,
    }

    /// Custom status. Also the custom status input of Identify / Presence Update.
    pub struct CustomStatus {
        pub text: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub expires_at: Option<Timestamp>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub emoji_id: Option<Snowflake>,
        pub emoji_name: Option<String>,
        #[serde(skip_serializing_if = "crate::is_false")]
        pub emoji_animated: bool,
    }

    pub struct GuildFolder {
        /// `-1` for the uncategorized folder.
        pub id: Option<i64>,
        pub name: Option<String>,
        pub color: Option<u32>,
        /// `1 << 0` = SHOW_ICON_WHEN_COLLAPSED.
        pub flags: u32,
        /// `folder`, `star`, `heart`, `bookmark`, `game_controller`, `shield`, `music_note`.
        pub icon: String,
        pub guild_ids: Vec<Snowflake>,
    }

    /// Relationship object; also the `RELATIONSHIP_ADD`/`UPDATE` payload.
    pub struct Relationship {
        pub id: Snowflake,
        #[serde(rename = "type")]
        pub kind: RelationshipType,
        /// Absent in Ready; resolve from Ready `users`.
        pub user: Option<PartialUser>,
        pub since: Option<Timestamp>,
        pub nickname: Option<String>,
        pub share_voice_activity: bool,
        pub friend_shares_voice_activity: bool,
    }

    /// Presence object (`PRESENCE_UPDATE`).
    pub struct Presence {
        pub user: PartialUser,
        pub status: Status,
        pub mobile: bool,
        pub afk: bool,
        pub custom_status: Option<CustomStatus>,
        pub guild_id: Option<Snowflake>,
    }

    /// Ready `sessions` / `SESSIONS_REPLACE` entry.
    pub struct SessionPresence {
        /// `all` for the combined first entry.
        pub session_id: String,
        pub status: Status,
        pub afk: bool,
        pub mobile: bool,
    }

    pub struct MuteConfig {
        pub end_time: Option<Timestamp>,
        pub selected_time_window: i64,
    }

    pub struct ChannelOverride {
        pub collapsed: bool,
        pub message_notifications: NotificationLevel,
        pub muted: bool,
        pub mute_config: Option<MuteConfig>,
        pub unread_badges: Option<NotificationLevel>,
        /// NEW_FORUM_THREADS_OFF `1 << 13`, NEW_FORUM_THREADS_ON `1 << 14`.
        pub flags: u32,
    }

    /// Per-guild notification settings (`USER_GUILD_SETTINGS_UPDATE`); `guild_id` null for DMs.
    pub struct UserGuildSettings {
        pub guild_id: Option<Snowflake>,
        pub message_notifications: NotificationLevel,
        pub muted: bool,
        pub mute_config: Option<MuteConfig>,
        pub mobile_push: bool,
        pub suppress_everyone: bool,
        pub suppress_roles: bool,
        pub hide_muted_channels: bool,
        pub channel_overrides: Option<HashMap<Snowflake, ChannelOverride>>,
        pub unread_badges: Option<NotificationLevel>,
        pub version: u64,
    }

    /// External connection (`USER_CONNECTIONS_UPDATE`, profiles).
    pub struct Connection {
        pub id: String,
        /// `bsky` or `domain`.
        #[serde(rename = "type")]
        pub kind: String,
        pub name: String,
        pub verified: bool,
        pub visibility_flags: u32,
        pub sort_order: u32,
    }

    /// Saved meme (`FAVORITE_MEME_*`, Ready `favorite_memes`).
    pub struct Meme {
        pub id: Snowflake,
        pub user_id: Snowflake,
        pub name: String,
        pub alt_text: Option<String>,
        pub tags: Vec<String>,
        pub attachment_id: Snowflake,
        pub filename: String,
        pub content_type: String,
        pub content_hash: Option<String>,
        pub size: u64,
        pub width: Option<u32>,
        pub height: Option<u32>,
        pub duration: Option<f64>,
        pub url: String,
        pub is_gifv: bool,
        pub gif_slug: Option<String>,
        pub gif_provider: Option<String>,
        pub media: Option<serde_json::Value>,
    }

    /// Registered passkey (Ready `webauthn_credentials`).
    pub struct WebAuthnCredential {
        pub id: String,
        pub name: String,
        pub created_at: Timestamp,
        pub last_used_at: Option<Timestamp>,
        pub rp_id: String,
    }

    /// `GET /users/{id}/profile`.
    pub struct UserProfileResponse {
        pub user: PartialUser,
        pub user_profile: UserProfile,
        pub guild_member: Option<GuildMember>,
        pub guild_member_profile: Option<UserProfile>,
        pub premium_type: Option<PremiumType>,
        pub premium_since: Option<Timestamp>,
        pub premium_lifetime_sequence: Option<i64>,
        pub profile_badges: Vec<ProfileBadge>,
        pub hidden_profile_badge_count: Option<u32>,
        pub mutual_friends: Option<Vec<PartialUser>>,
        pub mutual_guilds: Option<Vec<MutualGuild>>,
        pub connected_accounts: Vec<Connection>,
        pub timezone_offset: Option<i32>,
        pub profile_limited: bool,
    }

    /// Account-wide or guild profile (the guild one has no `banner_color`).
    pub struct UserProfile {
        pub bio: Option<String>,
        pub pronouns: Option<String>,
        pub banner: Option<String>,
        pub banner_color: Option<u32>,
        pub accent_color: Option<u32>,
    }

    pub struct MutualGuild {
        pub id: Snowflake,
        pub nick: Option<String>,
    }

    pub struct ProfileBadge {
        pub key: String,
        pub kind: String,
        pub label: String,
        pub asset: Option<String>,
        pub asset_url: Option<String>,
        pub aspect_ratio: Option<f64>,
        pub caption: Option<String>,
        pub href: Option<String>,
        pub small_asset: Option<String>,
        pub count: Option<i64>,
        pub accessible_label: Option<String>,
        pub details: Option<BadgeDetails>,
    }

    pub struct BadgeDetails {
        pub title: String,
        pub lines: Vec<String>,
    }
}

impl std::ops::Deref for User {
    type Target = PartialUser;
    fn deref(&self) -> &PartialUser {
        &self.base
    }
}

/// Presence status. Unknown strings map to `Unknown`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Online,
    Idle,
    Dnd,
    Invisible,
    #[default]
    Offline,
    #[serde(other)]
    Unknown,
}

int_enum! {
    pub struct MentionPreference(u8) { NO_PREFERENCE = 0, PREFER_MENTION = 1, PREFER_NO_MENTION = 2 }
    pub struct AuthenticatorType(u8) { TOTP = 0, WEBAUTHN = 2 }
    pub struct PremiumType(u8) { NONE = 0, SUBSCRIPTION = 1, LIFETIME = 2 }
    pub struct RelationshipType(u8) { FRIEND = 1, BLOCKED = 2, INCOMING_REQUEST = 3, OUTGOING_REQUEST = 4 }
    pub struct NotificationLevel(u8) { ALL_MESSAGES = 0, ONLY_MENTIONS = 1, NO_MESSAGES = 2, INHERIT = 3 }
}

int_flags! {
    /// Public user flags.
    pub struct UserFlags {
        const STAFF = 1 << 0;
        const PARTNER = 1 << 2;
        const BUG_HUNTER = 1 << 3;
        const FRIENDLY_BOT = 1 << 4;
        const FRIENDLY_BOT_MANUAL_APPROVAL = 1 << 5;
        const SPAMMER = 1 << 6;
        const PROFILE_HIDDEN = 1 << 7;
    }
    pub struct ProfileFieldPrivacyFlags {
        const EVERYONE = 1 << 0;
        const FRIENDS = 1 << 1;
        const MUTUAL_GUILDS = 1 << 2;
    }
    pub struct FriendSourceFlags {
        const MUTUAL_FRIENDS = 1 << 0;
        const MUTUAL_GUILDS = 1 << 1;
        const NO_RELATION = 1 << 2;
    }
    pub struct IncomingCallFlags {
        const FRIENDS_OF_FRIENDS = 1 << 0;
        const GUILD_MEMBERS = 1 << 1;
        const EVERYONE = 1 << 2;
        const FRIENDS_ONLY = 1 << 3;
        const NOBODY = 1 << 4;
        const SILENT_EVERYONE = 1 << 5;
    }
    pub struct GroupDmAddPermissionFlags {
        const FRIENDS_OF_FRIENDS = 1 << 0;
        const GUILD_MEMBERS = 1 << 1;
        const EVERYONE = 1 << 2;
        const FRIENDS_ONLY = 1 << 3;
        const NOBODY = 1 << 4;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_user_example() {
        let u: PartialUser = serde_json::from_str(
            r#"{"id":"1501314428688998182","username":"aria","discriminator":"0042",
            "global_name":"Aria","avatar":"a_2f1c9d7be4a350c8","avatar_color":5793266,"flags":8}"#,
        )
        .unwrap();
        assert_eq!(u.global_name.as_deref(), Some("Aria"));
        assert!(u.flags.contains(UserFlags::BUG_HUNTER));
        assert!(!u.bot);
    }

    #[test]
    fn private_user_flattens() {
        let u: User = serde_json::from_str(
            r#"{"id":"5","username":"me","discriminator":"0001","flags":0,"email":"a@b.c",
            "verified":true,"mfa_enabled":true,"authenticator_types":[0,2],"premium_type":0}"#,
        )
        .unwrap();
        assert_eq!(u.username, "me");
        assert_eq!(u.email.as_deref(), Some("a@b.c"));
        assert_eq!(u.authenticator_types, [AuthenticatorType::TOTP, AuthenticatorType::WEBAUTHN]);
    }

    #[test]
    fn status_unknown() {
        let s: Status = serde_json::from_str("\"streaming\"").unwrap();
        assert_eq!(s, Status::Unknown);
        assert_eq!(serde_json::to_string(&Status::Dnd).unwrap(), "\"dnd\"");
    }

    #[test]
    fn user_guild_settings_example() {
        let s: UserGuildSettings = serde_json::from_str(
            r#"{"guild_id":null,"message_notifications":1,"muted":false,"mute_config":null,
            "mobile_push":true,"suppress_everyone":false,"suppress_roles":false,
            "hide_muted_channels":false,"channel_overrides":null,"unread_badges":null,"version":4}"#,
        )
        .unwrap();
        assert_eq!(s.message_notifications, NotificationLevel::ONLY_MENTIONS);
        assert_eq!(s.version, 4);
    }
}
