//! Gateway envelope, opcodes, close codes, client commands and typed dispatch events
//! (`gateway/overview.md`, `opcodes-and-close-codes.md`, `commands.md`, `events.md`, `threads.md`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    Channel, ChannelType, CustomStatus, Emoji, Guild, GuildMember, GuildReady, GuildScoped, Invite,
    Meme, Message, PartialUser, Presence, ReactionEmoji, ReadState, ReadStateFlags, Relationship,
    Role, RtcRegion, SessionPresence, Snowflake, Status, Sticker, ThreadMember, Timestamp, User,
    UserGuildSettings, UserSettings, VoiceState, WebAuthnCredential, user::Connection,
};

// ---------------------------------------------------------------------------------------------
// Envelope, opcodes, close codes
// ---------------------------------------------------------------------------------------------

model! {
    /// Every Gateway message: `{op, d?, s?, t?}`.
    pub struct GatewayPayload {
        pub op: OpCode,
        pub d: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub s: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub t: Option<String>,
    }

    /// Opcode 10 Hello.
    pub struct Hello {
        pub heartbeat_interval: u64,
    }

    /// Opcode 12 Gateway Error (refused voice join).
    pub struct GatewayError {
        /// `VOICE_P2P_CONSENT_REQUIRED`, `VOICE_P2P_UNAVAILABLE`, `VOICE_CHANNEL_FULL`.
        pub code: String,
        pub message: String,
    }
}

impl GatewayPayload {
    /// A client command. Heartbeat uses `d` = last sequence or `null`.
    pub fn command(op: OpCode, d: impl Serialize) -> Self {
        Self { op, d: serde_json::to_value(d).unwrap_or(Value::Null), s: None, t: None }
    }
}

int_enum! {
    pub struct OpCode(u8) {
        DISPATCH = 0,
        HEARTBEAT = 1,
        IDENTIFY = 2,
        PRESENCE_UPDATE = 3,
        VOICE_STATE_UPDATE = 4,
        VOICE_SERVER_PING = 5,
        RESUME = 6,
        RECONNECT = 7,
        REQUEST_GUILD_MEMBERS = 8,
        INVALID_SESSION = 9,
        HELLO = 10,
        HEARTBEAT_ACK = 11,
        GATEWAY_ERROR = 12,
        VOICE_SIGNAL = 13,
        LAZY_REQUEST = 14,
        REQUEST_GUILD_COUNTS = 15,
        REQUEST_CHANNEL_MEMBER_COUNTS = 16,
        REQUEST_FORUM_UNREADS = 28,
    }
    pub struct CloseCode(u16) {
        UNKNOWN_ERROR = 4000,
        UNKNOWN_OPCODE = 4001,
        DECODE_ERROR = 4002,
        NOT_AUTHENTICATED = 4003,
        AUTHENTICATION_FAILED = 4004,
        ALREADY_AUTHENTICATED = 4005,
        INVALID_SEQUENCE = 4007,
        RATE_LIMITED = 4008,
        SESSION_TIMEOUT = 4009,
        INVALID_SHARD = 4010,
        SHARDING_REQUIRED = 4011,
        INVALID_API_VERSION = 4012,
    }
}

impl CloseCode {
    /// Whether to reconnect at all. The session stays resumable for 60 s after any close;
    /// these need a different token / shard / version first.
    pub fn can_reconnect(self) -> bool {
        !matches!(
            self,
            Self::AUTHENTICATION_FAILED | Self::INVALID_SHARD | Self::SHARDING_REQUIRED | Self::INVALID_API_VERSION
        )
    }
}

int_flags! {
    /// Identify `flags`.
    pub struct SessionFlags {
        const DEBOUNCE_MESSAGE_REACTIONS = 1 << 1;
        const CHANNEL_THREADS = 1 << 2;
        const GUILD_VISITS = 1 << 3;
    }
}

// ---------------------------------------------------------------------------------------------
// Client commands
// ---------------------------------------------------------------------------------------------

model! {
    /// Opcode 2.
    pub struct Identify {
        pub token: String,
        pub properties: IdentifyProperties,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub presence: Option<InitialPresence>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pub ignored_events: Vec<String>,
        pub flags: SessionFlags,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub initial_guild_id: Option<Snowflake>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub shard: Option<[u32; 2]>,
    }

    pub struct IdentifyProperties {
        pub os: String,
        pub browser: String,
        pub device: String,
        #[serde(skip_serializing_if = "crate::is_false")]
        pub e2ee_capable: bool,
        #[serde(skip_serializing_if = "crate::is_false")]
        pub mobile: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub latitude: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub longitude: Option<String>,
    }

    pub struct InitialPresence {
        pub status: Status,
        pub afk: bool,
        pub mobile: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub custom_status: Option<CustomStatus>,
    }

    /// Opcode 6.
    pub struct Resume {
        pub token: String,
        pub session_id: String,
        pub seq: u64,
    }

    /// Opcode 3. `custom_status`: `None` keeps it, `Some(None)` clears it.
    pub struct PresenceUpdate {
        pub status: Status,
        pub afk: bool,
        pub mobile: bool,
        #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "crate::double_option")]
        pub custom_status: Option<Option<CustomStatus>>,
    }

    /// Opcode 4. `guild_id`/`channel_id` are always sent (null is meaningful).
    pub struct UpdateVoiceState {
        pub guild_id: Option<Snowflake>,
        pub channel_id: Option<Snowflake>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub connection_id: Option<String>,
        pub self_mute: bool,
        pub self_deaf: bool,
        pub self_video: bool,
        pub self_stream: bool,
        #[serde(skip_serializing_if = "crate::is_false")]
        pub is_mobile: bool,
        /// `None` keeps the list, `Some(None)` clears it.
        #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "crate::double_option")]
        pub viewer_stream_keys: Option<Option<Vec<String>>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub latitude: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub longitude: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub p2p: Option<bool>,
    }

    /// Opcode 13 (send) / `VOICE_SIGNAL` (receive, with `from` and `user_id`).
    pub struct VoiceSignal {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub guild_id: Option<Snowflake>,
        pub channel_id: Snowflake,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub to: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub from: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub user_id: Option<Snowflake>,
        pub data: Value,
    }

    /// Opcode 8.
    pub struct RequestGuildMembers {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub guild_id: Option<Snowflake>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pub guild_ids: Vec<Snowflake>,
        pub query: String,
        pub limit: u32,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pub user_ids: Vec<Snowflake>,
        pub presences: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub nonce: Option<String>,
    }

    /// Opcode 14.
    pub struct LazyRequest {
        pub subscriptions: HashMap<Snowflake, GuildSubscription>,
    }

    pub struct GuildSubscription {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub active: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub sync: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub typing: Option<bool>,
        /// Channel ID → inclusive `[start, end]` ranges (each ≤ 100 wide).
        #[serde(skip_serializing_if = "HashMap::is_empty")]
        pub member_list_channels: HashMap<Snowflake, Vec<[u32; 2]>>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        pub members: Vec<Snowflake>,
        #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "crate::double_option")]
        pub focused_channel_id: Option<Option<Snowflake>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub threads: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub thread_member_lists: Option<Vec<Snowflake>>,
    }

    /// Opcode 15.
    pub struct RequestGuildCounts {
        pub guild_ids: Vec<Snowflake>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub nonce: Option<String>,
    }

    /// Opcode 16.
    pub struct RequestChannelMemberCounts {
        pub guild_id: Snowflake,
        pub channel_ids: Vec<Snowflake>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub nonce: Option<String>,
    }

    /// Opcode 28.
    pub struct RequestForumUnreads {
        pub guild_id: Snowflake,
        pub channel_id: Snowflake,
        pub threads: Vec<ForumUnreadRequest>,
    }

    pub struct ForumUnreadRequest {
        pub thread_id: Snowflake,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub ack_message_id: Option<Snowflake>,
    }
}

// ---------------------------------------------------------------------------------------------
// Dispatch payloads
// ---------------------------------------------------------------------------------------------

model! {
    pub struct Ready {
        pub session_id: String,
        pub version: u32,
        pub user: User,
        pub guilds: Vec<GuildReady>,
        pub private_channels: Vec<Channel>,
        /// No `user` here; resolve through `users`.
        pub relationships: Vec<Relationship>,
        pub presences: Vec<Presence>,
        pub users: Vec<PartialUser>,
        pub sessions: Vec<SessionPresence>,
        pub read_states: Vec<ReadState>,
        pub user_settings: Option<UserSettings>,
        pub user_guild_settings: Vec<UserGuildSettings>,
        pub notes: HashMap<Snowflake, String>,
        pub pinned_dms: Vec<Snowflake>,
        pub favorite_memes: Vec<Meme>,
        pub webauthn_credentials: Vec<WebAuthnCredential>,
        pub rtc_regions: Vec<RtcRegion>,
        pub country_code: String,
        pub latitude: Option<String>,
        pub longitude: Option<String>,
        pub auth_session_id_hash: Option<String>,
        pub shard: Option<[u32; 2]>,
    }

    pub struct Resumed {}

    pub struct AuthSessionChange {
        pub old_auth_session_id_hash: String,
        pub new_auth_session_id_hash: String,
    }

    pub struct RateLimited {
        pub opcode: OpCode,
        pub retry_after: f64,
        pub meta: Value,
    }

    pub struct UserNoteUpdate {
        pub id: Snowflake,
        pub note: String,
    }

    pub struct UserConnectionsUpdate {
        pub connections: Vec<Connection>,
    }

    /// Payloads that are just `{id}` (e.g. `RELATIONSHIP_REMOVE`).
    pub struct IdOnly {
        pub id: Snowflake,
    }

    /// `SAVED_MESSAGE_DELETE`, `RECENT_MENTION_DELETE`.
    pub struct MessageIdOnly {
        pub message_id: Snowflake,
    }

    pub struct FavoriteMemeDelete {
        pub meme_id: Snowflake,
    }

    pub struct GuildDelete {
        pub id: Snowflake,
        pub guild_id: Option<Snowflake>,
        /// Absent: remove the guild. True: keep it as unavailable.
        pub unavailable: bool,
        pub unavailable_hidden: bool,
    }

    pub struct GuildRoleEvent {
        pub guild_id: Snowflake,
        pub role: Role,
    }

    pub struct GuildRoleUpdateBulk {
        pub guild_id: Snowflake,
        pub roles: Vec<Role>,
    }

    pub struct GuildRoleDelete {
        pub guild_id: Snowflake,
        pub role_id: Snowflake,
    }

    pub struct GuildEmojisUpdate {
        pub guild_id: Snowflake,
        pub emojis: Vec<Emoji>,
    }

    pub struct GuildStickersUpdate {
        pub guild_id: Snowflake,
        pub stickers: Vec<Sticker>,
    }

    pub struct ChannelUpdateBulk {
        pub guild_id: Snowflake,
        pub channels: Vec<Channel>,
    }

    pub struct ChannelRecipient {
        pub channel_id: Snowflake,
        pub user: PartialUser,
    }

    pub struct WebhooksUpdate {
        pub guild_id: Snowflake,
        pub channel_id: Snowflake,
    }

    pub struct InviteDelete {
        pub code: String,
        pub channel_id: Option<Snowflake>,
        pub guild_id: Option<Snowflake>,
    }

    pub struct ThreadDelete {
        pub id: Snowflake,
        pub guild_id: Snowflake,
        pub parent_id: Snowflake,
        #[serde(rename = "type")]
        pub kind: ChannelType,
    }

    pub struct ThreadListSync {
        pub guild_id: Snowflake,
        /// Absent: the whole guild's threads are replaced.
        pub channel_ids: Option<Vec<Snowflake>>,
        pub threads: Vec<Channel>,
        pub members: Vec<ThreadMember>,
    }

    pub struct ThreadMembersUpdate {
        pub id: Snowflake,
        pub guild_id: Snowflake,
        pub member_count: u32,
        pub added_members: Vec<ThreadMember>,
        pub removed_member_ids: Vec<Snowflake>,
    }

    pub struct ThreadMemberListUpdate {
        pub guild_id: Snowflake,
        pub thread_id: Snowflake,
        pub members: Vec<ThreadMember>,
    }

    pub struct ForumUnreads {
        pub guild_id: Snowflake,
        pub channel_id: Snowflake,
        pub threads: Vec<ForumUnread>,
    }

    pub struct ForumUnread {
        pub thread_id: Snowflake,
        pub count: Option<u32>,
        pub missing: bool,
    }

    /// `GUILD_MEMBER_REMOVE`, `GUILD_BAN_ADD`, `GUILD_BAN_REMOVE` (`user` has only `id`).
    pub struct GuildUserEvent {
        pub guild_id: Snowflake,
        pub user: PartialUser,
    }

    pub struct GuildMembersChunk {
        pub guild_id: Snowflake,
        pub members: Vec<GuildMember>,
        pub chunk_index: u32,
        pub chunk_count: u32,
        pub presences: Vec<Presence>,
        pub nonce: Option<String>,
    }

    pub struct GuildMemberListUpdate {
        pub guild_id: Snowflake,
        /// List identifier (the channel ID).
        pub id: String,
        pub channel_id: Option<Snowflake>,
        pub member_count: u32,
        pub online_count: u32,
        pub groups: Vec<MemberListGroup>,
        pub ops: Vec<MemberListOp>,
    }

    pub struct MemberListGroup {
        /// Hoisted role ID, `online` or `offline`.
        pub id: String,
        pub count: u32,
    }

    pub struct MemberListOp {
        /// Always `SYNC`.
        pub op: String,
        /// Inclusive `[start, end]`.
        pub range: [u32; 2],
        pub items: Vec<MemberListItem>,
    }

    /// Exactly one of `group` / `member`; the member carries `presence`.
    pub struct MemberListItem {
        pub group: Option<MemberListGroup>,
        pub member: Option<GuildMember>,
    }

    pub struct AuditLogEntry {
        pub id: Snowflake,
        pub guild_id: Snowflake,
        pub action_type: u32,
        pub user_id: Snowflake,
        pub target_id: Option<String>,
        pub reason: Option<String>,
        pub options: Value,
        pub changes: Vec<Value>,
    }

    pub struct PresenceUpdateBulk {
        pub guild_id: Snowflake,
        pub presences: Vec<Presence>,
    }

    pub struct PassiveUpdates {
        pub guild_id: Snowflake,
        /// Channel ID → new `last_message_id`.
        pub channels: HashMap<Snowflake, Snowflake>,
        pub voice_states: Vec<VoiceState>,
    }

    pub struct MessageDelete {
        pub id: Snowflake,
        pub channel_id: Snowflake,
        pub content: Option<String>,
        pub author_id: Option<Snowflake>,
        pub guild_id: Option<Snowflake>,
        pub member: Option<GuildMember>,
    }

    pub struct MessageDeleteBulk {
        pub ids: Vec<Snowflake>,
        pub channel_id: Snowflake,
        pub guild_id: Option<Snowflake>,
    }

    pub struct MessageAck {
        pub channel_id: Snowflake,
        pub message_id: Snowflake,
        pub mention_count: u32,
        pub manual: bool,
        pub version: Option<String>,
        pub flags: ReadStateFlags,
    }

    /// `MESSAGE_REACTION_ADD` / `MESSAGE_REACTION_REMOVE`.
    pub struct MessageReaction {
        pub user_id: Snowflake,
        pub channel_id: Snowflake,
        pub message_id: Snowflake,
        pub emoji: ReactionEmoji,
        pub guild_id: Option<Snowflake>,
        pub member: Option<GuildMember>,
        pub session_id: Option<String>,
    }

    pub struct MessageReactionAddMany {
        pub channel_id: Snowflake,
        pub message_id: Snowflake,
        pub guild_id: Option<Snowflake>,
        pub reactions: Vec<ReactionAddition>,
    }

    pub struct ReactionAddition {
        pub user_id: Snowflake,
        pub emoji: ReactionEmoji,
        pub member: Option<GuildMember>,
    }

    /// `MESSAGE_REACTION_REMOVE_ALL` (no emoji) / `MESSAGE_REACTION_REMOVE_EMOJI`.
    pub struct MessageReactionRemoveBulk {
        pub channel_id: Snowflake,
        pub message_id: Snowflake,
        pub emoji: Option<ReactionEmoji>,
        pub guild_id: Option<Snowflake>,
    }

    pub struct TypingStart {
        pub channel_id: Snowflake,
        pub user_id: Snowflake,
        /// Unix seconds.
        pub timestamp: u64,
        pub guild_id: Option<Snowflake>,
        pub member: Option<GuildMember>,
    }

    pub struct ChannelPinsUpdate {
        pub channel_id: Snowflake,
        pub last_pin_timestamp: Option<Timestamp>,
        pub guild_id: Option<Snowflake>,
    }

    pub struct ChannelPinsAck {
        pub channel_id: Snowflake,
        pub timestamp: Timestamp,
    }

    /// LiveKit grant (`token` + `endpoint`) or peer-to-peer grant (`p2p` + `ice_servers`).
    pub struct VoiceServerUpdate {
        pub token: Option<String>,
        pub endpoint: Option<String>,
        pub p2p: bool,
        pub ice_servers: Vec<IceServer>,
        pub connection_id: String,
        pub channel_id: Snowflake,
        pub guild_id: Option<Snowflake>,
        pub e2ee_key: Option<String>,
    }

    pub struct IceServer {
        pub urls: Vec<String>,
    }

    pub struct EntranceSoundPlay {
        pub user_id: Snowflake,
        pub channel_id: Snowflake,
        pub guild_id: Option<Snowflake>,
        pub sound_id: Snowflake,
        pub hash: String,
        pub url: String,
        pub duration_ms: u64,
        pub content_type: String,
    }

    /// `CALL_CREATE` / `CALL_UPDATE`.
    pub struct Call {
        pub channel_id: Snowflake,
        pub message_id: Snowflake,
        pub region: Option<String>,
        pub ringing: Vec<Snowflake>,
        pub voice_states: Vec<VoiceState>,
        pub recipients: Option<Vec<Snowflake>>,
        /// Unix milliseconds.
        pub created_at: Option<u64>,
    }

    pub struct CallDelete {
        pub channel_id: Snowflake,
        pub unavailable: bool,
    }

    pub struct GuildCountsUpdate {
        pub counts: Vec<CountEntry>,
        pub nonce: Option<String>,
    }

    /// Guild count entry (no `channel_id`) or channel count entry.
    pub struct CountEntry {
        pub guild_id: Snowflake,
        pub channel_id: Option<Snowflake>,
        pub member_count: u32,
        pub online_count: u32,
    }
}

macro_rules! dispatch_events {
    ($($name:literal => $variant:ident($ty:ty),)*) => {
        /// A typed Dispatch (`op` 0). Unknown event names fall back to [`DispatchEvent::Unknown`].
        #[derive(Clone, Debug, PartialEq)]
        pub enum DispatchEvent {
            $($variant($ty),)*
            Unknown { t: String, d: Value },
        }

        impl DispatchEvent {
            /// Parses one Dispatch. An unknown `t` gives `Ok(Unknown)`; a malformed payload gives
            /// `Err`, which the caller logs and skips so the connection survives.
            pub fn parse(t: &str, d: &Value) -> Result<Self, serde_json::Error> {
                Ok(match t {
                    $($name => Self::$variant(Deserialize::deserialize(d)?),)*
                    _ => Self::Unknown { t: t.to_owned(), d: d.clone() },
                })
            }

            /// The event name as sent in `t`.
            pub fn name(&self) -> &str {
                match self {
                    $(Self::$variant(_) => $name,)*
                    Self::Unknown { t, .. } => t,
                }
            }
        }
    };
}

dispatch_events! {
    "READY" => Ready(Box<Ready>),
    "RESUMED" => Resumed(Resumed),
    "SESSIONS_REPLACE" => SessionsReplace(Vec<SessionPresence>),
    "AUTH_SESSION_CHANGE" => AuthSessionChange(AuthSessionChange),
    "RATE_LIMITED" => RateLimited(RateLimited),
    "USER_UPDATE" => UserUpdate(Box<User>),
    "USER_SETTINGS_UPDATE" => UserSettingsUpdate(Box<UserSettings>),
    "USER_GUILD_SETTINGS_UPDATE" => UserGuildSettingsUpdate(UserGuildSettings),
    "USER_NOTE_UPDATE" => UserNoteUpdate(UserNoteUpdate),
    "USER_PINNED_DMS_UPDATE" => UserPinnedDmsUpdate(Vec<Snowflake>),
    "USER_CONNECTIONS_UPDATE" => UserConnectionsUpdate(UserConnectionsUpdate),
    "WEBAUTHN_CREDENTIALS_UPDATE" => WebAuthnCredentialsUpdate(Vec<WebAuthnCredential>),
    "RELATIONSHIP_ADD" => RelationshipAdd(Relationship),
    "RELATIONSHIP_UPDATE" => RelationshipUpdate(Relationship),
    "RELATIONSHIP_REMOVE" => RelationshipRemove(IdOnly),
    "SAVED_MESSAGE_CREATE" => SavedMessageCreate(Box<Message>),
    "SAVED_MESSAGE_DELETE" => SavedMessageDelete(MessageIdOnly),
    "RECENT_MENTION_DELETE" => RecentMentionDelete(MessageIdOnly),
    "FAVORITE_MEME_CREATE" => FavoriteMemeCreate(Box<Meme>),
    "FAVORITE_MEME_UPDATE" => FavoriteMemeUpdate(Box<Meme>),
    "FAVORITE_MEME_DELETE" => FavoriteMemeDelete(FavoriteMemeDelete),
    "GUILD_CREATE" => GuildCreate(Box<GuildReady>),
    "GUILD_SYNC" => GuildSync(Box<GuildReady>),
    "GUILD_UPDATE" => GuildUpdate(Box<Guild>),
    "GUILD_DELETE" => GuildDelete(GuildDelete),
    "GUILD_ROLE_CREATE" => GuildRoleCreate(GuildRoleEvent),
    "GUILD_ROLE_UPDATE" => GuildRoleUpdate(GuildRoleEvent),
    "GUILD_ROLE_UPDATE_BULK" => GuildRoleUpdateBulk(GuildRoleUpdateBulk),
    "GUILD_ROLE_DELETE" => GuildRoleDelete(GuildRoleDelete),
    "GUILD_EMOJIS_UPDATE" => GuildEmojisUpdate(GuildEmojisUpdate),
    "GUILD_STICKERS_UPDATE" => GuildStickersUpdate(GuildStickersUpdate),
    "CHANNEL_CREATE" => ChannelCreate(Box<Channel>),
    "CHANNEL_UPDATE" => ChannelUpdate(Box<Channel>),
    "CHANNEL_UPDATE_BULK" => ChannelUpdateBulk(ChannelUpdateBulk),
    "CHANNEL_DELETE" => ChannelDelete(Box<Channel>),
    "CHANNEL_RECIPIENT_ADD" => ChannelRecipientAdd(ChannelRecipient),
    "CHANNEL_RECIPIENT_REMOVE" => ChannelRecipientRemove(ChannelRecipient),
    "WEBHOOKS_UPDATE" => WebhooksUpdate(WebhooksUpdate),
    "INVITE_CREATE" => InviteCreate(Box<Invite>),
    "INVITE_DELETE" => InviteDelete(InviteDelete),
    "THREAD_CREATE" => ThreadCreate(Box<Channel>),
    "THREAD_UPDATE" => ThreadUpdate(Box<Channel>),
    "THREAD_DELETE" => ThreadDelete(ThreadDelete),
    "THREAD_LIST_SYNC" => ThreadListSync(ThreadListSync),
    "THREAD_MEMBER_UPDATE" => ThreadMemberUpdate(Box<ThreadMember>),
    "THREAD_MEMBERS_UPDATE" => ThreadMembersUpdate(ThreadMembersUpdate),
    "THREAD_MEMBER_LIST_UPDATE" => ThreadMemberListUpdate(ThreadMemberListUpdate),
    "FORUM_UNREADS" => ForumUnreads(ForumUnreads),
    "GUILD_MEMBER_ADD" => GuildMemberAdd(Box<GuildScoped<GuildMember>>),
    "GUILD_MEMBER_UPDATE" => GuildMemberUpdate(Box<GuildScoped<GuildMember>>),
    "GUILD_MEMBER_REMOVE" => GuildMemberRemove(GuildUserEvent),
    "GUILD_MEMBERS_CHUNK" => GuildMembersChunk(GuildMembersChunk),
    "GUILD_MEMBER_LIST_UPDATE" => GuildMemberListUpdate(GuildMemberListUpdate),
    "GUILD_AUDIT_LOG_ENTRY_CREATE" => GuildAuditLogEntryCreate(AuditLogEntry),
    "GUILD_BAN_ADD" => GuildBanAdd(GuildUserEvent),
    "GUILD_BAN_REMOVE" => GuildBanRemove(GuildUserEvent),
    "PRESENCE_UPDATE" => PresenceUpdate(Box<Presence>),
    "PRESENCE_UPDATE_BULK" => PresenceUpdateBulk(PresenceUpdateBulk),
    "PASSIVE_UPDATES" => PassiveUpdates(PassiveUpdates),
    "MESSAGE_CREATE" => MessageCreate(Box<Message>),
    "MESSAGE_UPDATE" => MessageUpdate(Box<Message>),
    "MESSAGE_DELETE" => MessageDelete(Box<MessageDelete>),
    "MESSAGE_DELETE_BULK" => MessageDeleteBulk(MessageDeleteBulk),
    "MESSAGE_ACK" => MessageAck(MessageAck),
    "MESSAGE_REACTION_ADD" => MessageReactionAdd(Box<MessageReaction>),
    "MESSAGE_REACTION_ADD_MANY" => MessageReactionAddMany(MessageReactionAddMany),
    "MESSAGE_REACTION_REMOVE" => MessageReactionRemove(Box<MessageReaction>),
    "MESSAGE_REACTION_REMOVE_ALL" => MessageReactionRemoveAll(MessageReactionRemoveBulk),
    "MESSAGE_REACTION_REMOVE_EMOJI" => MessageReactionRemoveEmoji(MessageReactionRemoveBulk),
    "TYPING_START" => TypingStart(Box<TypingStart>),
    "CHANNEL_PINS_UPDATE" => ChannelPinsUpdate(ChannelPinsUpdate),
    "CHANNEL_PINS_ACK" => ChannelPinsAck(ChannelPinsAck),
    "VOICE_STATE_UPDATE" => VoiceStateUpdate(Box<VoiceState>),
    "VOICE_SERVER_UPDATE" => VoiceServerUpdate(VoiceServerUpdate),
    "VOICE_SIGNAL" => VoiceSignal(VoiceSignal),
    "ENTRANCE_SOUND_PLAY" => EntranceSoundPlay(EntranceSoundPlay),
    "CALL_CREATE" => CallCreate(Call),
    "CALL_UPDATE" => CallUpdate(Call),
    "CALL_DELETE" => CallDelete(CallDelete),
    "GUILD_COUNTS_UPDATE" => GuildCountsUpdate(GuildCountsUpdate),
    "CHANNEL_MEMBER_COUNTS_UPDATE" => ChannelMemberCountsUpdate(GuildCountsUpdate),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MessageType, Permissions};

    fn parse(text: &str) -> DispatchEvent {
        let p: GatewayPayload = serde_json::from_str(text).unwrap();
        assert_eq!(p.op, OpCode::DISPATCH);
        DispatchEvent::parse(p.t.as_deref().unwrap(), &p.d).unwrap()
    }

    #[test]
    fn hello_and_commands() {
        let p: GatewayPayload =
            serde_json::from_str(r#"{"op":10,"d":{"heartbeat_interval":41250}}"#).unwrap();
        assert_eq!(p.op, OpCode::HELLO);
        assert_eq!(Hello::deserialize(&p.d).unwrap().heartbeat_interval, 41250);

        let ack: GatewayPayload = serde_json::from_str(r#"{"op":11}"#).unwrap();
        assert_eq!((ack.op, ack.d), (OpCode::HEARTBEAT_ACK, Value::Null));

        let hb = GatewayPayload::command(OpCode::HEARTBEAT, Some(42u64));
        assert_eq!(serde_json::to_string(&hb).unwrap(), r#"{"op":1,"d":42}"#);
        let hb = GatewayPayload::command(OpCode::HEARTBEAT, None::<u64>);
        assert_eq!(serde_json::to_string(&hb).unwrap(), r#"{"op":1,"d":null}"#);

        let id = Identify {
            token: "T".into(),
            properties: IdentifyProperties {
                os: "Linux".into(),
                browser: "Fluxer Client".into(),
                device: "desktop".into(),
                ..Default::default()
            },
            flags: SessionFlags::CHANNEL_THREADS,
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(GatewayPayload::command(OpCode::IDENTIFY, id)).unwrap(),
            serde_json::json!({"op":2,"d":{"token":"T","properties":
                {"os":"Linux","browser":"Fluxer Client","device":"desktop"},"flags":4}})
        );

        let clear = PresenceUpdate { status: Status::Idle, custom_status: Some(None), ..Default::default() };
        assert_eq!(
            serde_json::to_value(clear).unwrap(),
            serde_json::json!({"status":"idle","afk":false,"mobile":false,"custom_status":null})
        );
        let mut subs = HashMap::new();
        subs.insert(
            Snowflake(1),
            GuildSubscription {
                active: Some(true),
                member_list_channels: [(Snowflake(2), vec![[0, 99]])].into(),
                ..Default::default()
            },
        );
        assert_eq!(
            serde_json::to_value(LazyRequest { subscriptions: subs }).unwrap(),
            serde_json::json!({"subscriptions":{"1":{"active":true,"member_list_channels":{"2":[[0,99]]}}}})
        );
    }

    #[test]
    fn ready() {
        let ev = parse(
            r#"{"op":0,"t":"READY","s":1,"d":{
            "session_id":"6f1d0b7c9a2e4f83b5c1d9e7a4f20b13","version":1,
            "user":{"id":"5","username":"me","discriminator":"0001","flags":0,"email":"me@x.y",
                "verified":true,"mfa_enabled":false,"premium_type":0},
            "guilds":[
              {"id":"100","properties":{"id":"100","name":"G","owner_id":"5","features":[]},
               "roles":[{"id":"100","name":"@everyone","color":0,"position":0,"permissions":"3072",
                 "hoist":false,"mentionable":false,"icon":null,"unicode_emoji":null,"hoist_position":null}],
               "channels":[{"id":"101","type":0,"guild_id":"100","name":"general","position":0,
                 "permission_overwrites":[]}],
               "emojis":[],"stickers":[],
               "members":[{"user":{"id":"5"},"nick":null,"roles":[],"joined_at":"2026-01-01T00:00:00.000Z",
                 "mute":false,"deaf":false,"communication_disabled_until":null}],
               "member_count":1,"online_count":1,"presences":[],"voice_states":[],
               "joined_at":"2026-01-01T00:00:00.000Z"},
              {"id":"200","unavailable":true}],
            "private_channels":[{"id":"300","type":1,"recipients":[{"id":"6","username":"bob",
              "discriminator":"0002"}],"last_message_id":null}],
            "relationships":[{"id":"6","type":1,"nickname":null,"share_voice_activity":false,
              "friend_shares_voice_activity":true}],
            "presences":[{"user":{"id":"6"},"status":"online","mobile":false,"afk":false,
              "custom_status":{"text":"hi","expires_at":null,"emoji_id":null,"emoji_name":null,
              "emoji_animated":false}}],
            "users":[{"id":"6","username":"bob","discriminator":"0002"}],
            "sessions":[{"session_id":"all","status":"online","afk":false,"mobile":false}],
            "read_states":[{"id":"101","mention_count":3,"last_message_id":"1501320000000000000",
              "last_pin_timestamp":null,"version":"0"}],
            "user_settings":{"status":"online","theme":"dark","locale":"en-US","guild_folders":[
              {"id":-1,"name":null,"color":null,"flags":0,"icon":"folder","guild_ids":["100"]}],
              "custom_status":null},
            "user_guild_settings":[],"notes":{"6":"friend"},"pinned_dms":["300"],
            "favorite_memes":[],"webauthn_credentials":[],
            "rtc_regions":[{"id":"automatic","name":"Automatic","emoji":"🌐"}],
            "country_code":"US","_timings":{"x":1}}}"#,
        );
        let DispatchEvent::Ready(r) = ev else { panic!("not ready: {ev:?}") };
        assert_eq!(r.user.username, "me");
        assert_eq!(r.guilds.len(), 2);
        let g = &r.guilds[0];
        assert_eq!(g.properties.as_ref().unwrap().name, "G");
        assert_eq!(g.roles[0].permissions, Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES);
        assert!(r.guilds[1].unavailable);
        assert_eq!(r.relationships[0].user, None);
        assert_eq!(r.presences[0].status, Status::Online);
        assert_eq!(r.notes[&Snowflake(6)], "friend");
        assert_eq!(r.user_settings.as_ref().unwrap().guild_folders[0].id, Some(-1));
        assert_eq!(r.read_states[0].mention_count, 3);
    }

    #[test]
    fn message_create() {
        let ev = parse(
            r#"{"op":0,"t":"MESSAGE_CREATE","s":17,"d":{"id":"1501320000000000000",
            "channel_id":"1501314428688998182","author":{"id":"1501314428688990000","username":"ada",
            "discriminator":"0001","avatar":"a1b2"},"type":19,"flags":4096,"content":"<@5> hi",
            "timestamp":"2026-08-31T09:14:02.000Z","edited_timestamp":null,"pinned":false,
            "mention_everyone":false,"tts":false,"mentions":[{"id":"5","username":"me"}],
            "mention_roles":[],"embeds":[],"attachments":[],"stickers":[],"nonce":"n1",
            "message_reference":{"channel_id":"1501314428688998182","message_id":"1","type":0},
            "referenced_message":null,"channel_type":0,"guild_id":"100",
            "member":{"roles":["101"],"joined_at":"2026-01-01T00:00:00.000Z","mute":false,"deaf":false}}}"#,
        );
        let DispatchEvent::MessageCreate(m) = ev else { panic!() };
        assert_eq!(m.kind, MessageType::REPLY);
        assert!(m.flags.contains(crate::MessageFlags::SUPPRESS_NOTIFICATIONS));
        assert_eq!(m.guild_id, Some(Snowflake(100)));
        assert_eq!(m.member.as_ref().unwrap().roles, [Snowflake(101)]);
        assert_eq!(m.referenced_message, Some(None));
    }

    #[test]
    fn guild_create_and_scoped() {
        let ev = parse(
            r#"{"op":0,"t":"GUILD_CREATE","s":2,"d":{"id":"100","properties":{"id":"100","name":"G",
            "owner_id":"5"},"roles":[],"channels":[],"emojis":[{"id":"7","name":"party_parrot",
            "animated":true}],"stickers":[],"members":[],"member_count":10,"online_count":2,
            "presences":[],"voice_states":[{"guild_id":"100","channel_id":"102","user_id":"5",
            "connection_id":"c","session_id":"s","member":null,"mute":false,"deaf":false,
            "self_mute":true,"self_deaf":false,"self_video":false,"self_stream":false,
            "is_mobile":false,"suppress":false,"viewer_stream_keys":[],"e2ee_capable":true,
            "p2p":false,"version":3}],"joined_at":null,"threads":[]}}"#,
        );
        let DispatchEvent::GuildCreate(g) = ev else { panic!() };
        assert_eq!(g.member_count, 10);
        assert!(g.voice_states[0].self_mute);
        assert!(g.emojis[0].animated);

        let ev = parse(
            r#"{"op":0,"t":"GUILD_MEMBER_ADD","s":3,"d":{"guild_id":"100","user":{"id":"6",
            "username":"bob"},"roles":[],"joined_at":"2026-01-01T00:00:00.000Z"}}"#,
        );
        let DispatchEvent::GuildMemberAdd(m) = ev else { panic!() };
        assert_eq!((m.guild_id, m.inner.user.id), (Snowflake(100), Snowflake(6)));
    }

    #[test]
    fn misc_events() {
        let ev = parse(
            r#"{"op":0,"t":"GUILD_MEMBER_LIST_UPDATE","s":4,"d":{"guild_id":"100","id":"101",
            "member_count":2,"online_count":1,"groups":[{"id":"online","count":1}],
            "ops":[{"op":"SYNC","range":[0,99],"items":[{"group":{"id":"online","count":1}},
            {"member":{"user":{"id":"6"},"roles":[],"joined_at":"x","presence":{"status":"offline",
            "mobile":false,"afk":false}}}]}]}}"#,
        );
        let DispatchEvent::GuildMemberListUpdate(l) = ev else { panic!() };
        let m = l.ops[0].items[1].member.as_ref().unwrap();
        assert_eq!(m.presence.as_ref().unwrap().status, Status::Offline);

        let ev = parse(
            r#"{"op":0,"t":"MESSAGE_REACTION_ADD","s":5,"d":{"user_id":"5","channel_id":"1",
            "message_id":"2","emoji":{"name":"🔥"}}}"#,
        );
        assert!(matches!(ev, DispatchEvent::MessageReactionAdd(ref r) if r.emoji.id.is_none()));

        let ev = parse(r#"{"op":0,"t":"USER_PINNED_DMS_UPDATE","s":6,"d":["1","2"]}"#);
        assert_eq!(ev, DispatchEvent::UserPinnedDmsUpdate(vec![Snowflake(1), Snowflake(2)]));

        let ev = parse(
            r#"{"op":0,"t":"VOICE_SERVER_UPDATE","s":7,"d":{"token":"lk","endpoint":"wss://v",
            "connection_id":"c","channel_id":"102","guild_id":"100"}}"#,
        );
        assert!(matches!(ev, DispatchEvent::VoiceServerUpdate(ref v) if !v.p2p && v.endpoint.is_some()));
        assert_eq!(ev.name(), "VOICE_SERVER_UPDATE");
    }

    #[test]
    fn unknown_and_malformed() {
        let d = serde_json::json!({"x": 1});
        let ev = DispatchEvent::parse("SOMETHING_NEW", &d).unwrap();
        assert_eq!(ev, DispatchEvent::Unknown { t: "SOMETHING_NEW".into(), d: d.clone() });
        assert_eq!(ev.name(), "SOMETHING_NEW");
        // Wrong type for a field: an error, not a panic.
        assert!(DispatchEvent::parse("MESSAGE_DELETE", &serde_json::json!({"id": [1]})).is_err());
        assert!(DispatchEvent::parse("READY", &serde_json::json!("nope")).is_err());
    }

    #[test]
    fn close_codes() {
        assert!(CloseCode::SESSION_TIMEOUT.can_reconnect());
        assert!(!CloseCode::AUTHENTICATION_FAILED.can_reconnect());
        assert!(CloseCode(1000).can_reconnect());
    }
}
