//! Fluxer's grouped permission lists (`permissions/utils/PermissionUtils.ts`
//! `generatePermissionSpec` / `generateChannelPermissionSpecs`, labels from
//! `PermissionLabelDescriptors.ts`).

use reflux_model::{ChannelType, Permissions as P};

/// (bit, title, description)
pub type Entry = (P, &'static str, &'static str);

/// Role editor groups: (title, entries).
pub const GUILD: &[(&str, &[Entry])] = &[
    (
        "Community-wide",
        &[
            (P::ADMINISTRATOR, "Administrator", "Grants all permissions and bypasses channel restrictions. Highly sensitive."),
            (P::VIEW_AUDIT_LOG, "View activity log", "Read the community's activity log of changes and moderation actions."),
            (P::MANAGE_GUILD, "Manage community", "Edit global settings like name, description, and icon."),
            (P::MANAGE_ROLES, "Manage roles", "Create, edit, or delete roles below your highest role. Also allows editing channel permission overwrites."),
            (P::MANAGE_CHANNELS, "Manage channels", "Create, edit, or delete channels and categories."),
            (P::KICK_MEMBERS, "Kick members", "Remove members from the community. They can rejoin with a new invite."),
            (P::BAN_MEMBERS, "Ban members", "Ban members from the community, with the option to delete their recent messages."),
            (P::CREATE_INSTANT_INVITE, "Create invite links", "Invite new people to the community with an invite link."),
            (P::CHANGE_NICKNAME, "Change own nickname", "Update your own nickname."),
            (P::MANAGE_NICKNAMES, "Manage nicknames", "Change other members' nicknames."),
            (P::CREATE_EXPRESSIONS, "Create emoji & stickers", "Upload new emoji and stickers, and manage your own creations."),
            (P::MANAGE_EXPRESSIONS, "Manage emoji & stickers", "Edit or delete emoji and stickers created by other members."),
            (P::MANAGE_WEBHOOKS, "Manage webhooks", "Create, edit, or delete webhooks."),
        ],
    ),
    (
        "Channel access",
        &[
            (P::VIEW_CHANNEL, "View channel", "See channels by default. Channels that deny it stay hidden."),
            (P::VIEW_CHANNEL_MEMBERS, "View channel members", "See the member list for channels in this community."),
        ],
    ),
    (
        "Messages & media",
        &[
            (P::SEND_MESSAGES, "Send messages", "Send messages in channels."),
            (P::SEND_TTS_MESSAGES, "Send TTS messages", "Send text-to-speech messages with /tts. Members who turned text-to-speech on hear them read aloud."),
            (P::MANAGE_MESSAGES, "Manage messages", "Delete other members' messages. Pinning is controlled separately."),
            (P::PIN_MESSAGES, "Pin messages", "Pin or unpin any message."),
            (P::EMBED_LINKS, "Embed links", "Show embedded previews for links sent."),
            (P::ATTACH_FILES, "Attach files", "Upload files and media in messages."),
            (P::READ_MESSAGE_HISTORY, "Read message history", "Read messages sent before opening a channel. Without this permission, members only see messages that arrive while they have the channel open."),
            (P::MENTION_EVERYONE, "Use @everyone/@here and @roles", "Mention everyone or any role (even if the role isn't set to be mentionable)."),
            (P::USE_EXTERNAL_EMOJIS, "Use external emoji", "Use emoji from other communities."),
            (P::USE_EXTERNAL_STICKERS, "Use external stickers", "Use stickers from other communities."),
            (P::ADD_REACTIONS, "Add reactions", "Add new reactions to messages."),
            (P::BYPASS_SLOWMODE, "Bypass slowmode", "Ignore per-channel message rate limits."),
        ],
    ),
    ("Moderation", &[(P::MODERATE_MEMBERS, "Time out members", "Prevent members from sending messages, reacting, and joining voice for a duration.")]),
    (
        "Audio & video",
        &[
            (P::CONNECT, "Connect to voice", "Join voice channels and hear others."),
            (P::SPEAK, "Speak", "Talk in voice channels. Without this permission, members stay muted until someone with the \"Mute members\" permission unmutes them."),
            (P::STREAM, "Stream video", "Turn on a camera or share a screen in voice channels."),
            (P::USE_VAD, "Use voice activity", "Without this permission, push-to-talk is required."),
            (P::PRIORITY_SPEAKER, "Priority speaker", "Be heard over other members while holding the push to talk (priority) keybind."),
            (P::MUTE_MEMBERS, "Mute members", "Mute other members in voice channels for everyone."),
            (P::DEAFEN_MEMBERS, "Deafen members", "Deafen other members in voice channels, so they cannot hear or speak."),
            (P::MOVE_MEMBERS, "Move members", "Drag members between voice channels they can access, and disconnect them from voice."),
            (P::UPDATE_RTC_REGION, "Set voice region", "Change which voice region a voice channel uses."),
        ],
    ),
];

const CHANNEL_ACCESS: (&str, &[Entry]) = (
    "Channel access",
    &[
        (P::VIEW_CHANNEL, "View channel", "See this channel. Denying it for @everyone makes the channel private."),
        (P::VIEW_CHANNEL_MEMBERS, "View channel members", "See the member list for this channel."),
    ],
);
const CHANNEL_MANAGEMENT: (&str, &[Entry]) = (
    "Channel management",
    &[
        (P::CREATE_INSTANT_INVITE, "Create invite links", "Invite new people to the community with a link to this channel."),
        (P::MANAGE_CHANNELS, "Manage channel", "Rename and edit this channel's settings."),
        (P::MANAGE_ROLES, "Manage permissions", "Edit overwrites for roles and members in this channel."),
        (P::MANAGE_WEBHOOKS, "Manage webhooks", "Create, edit, or delete webhooks for this channel."),
    ],
);
const CHANNEL_TEXT: (&str, &[Entry]) = (
    "Messages & media",
    &[
        (P::SEND_MESSAGES, "Send messages", "Send messages in this channel."),
        (P::MANAGE_MESSAGES, "Manage messages", "Delete other members' messages in this channel. Pinning is controlled separately."),
        (P::PIN_MESSAGES, "Pin messages", "Pin or unpin any message in this channel."),
        (P::EMBED_LINKS, "Embed links", "Show embedded previews for links sent in this channel."),
        (P::ATTACH_FILES, "Attach files", "Upload files and media in this channel."),
        (P::READ_MESSAGE_HISTORY, "Read message history", "Read messages sent before opening this channel."),
        (P::MENTION_EVERYONE, "Use @everyone/@here and @roles", "Mention everyone or any role in this channel (even if the role isn't set to be mentionable)."),
        (P::USE_EXTERNAL_EMOJIS, "Use external emoji", "Use emoji from other communities in this channel."),
        (P::USE_EXTERNAL_STICKERS, "Use external stickers", "Use stickers from other communities in this channel."),
        (P::ADD_REACTIONS, "Add reactions", "Add new reactions to messages in this channel."),
        (P::BYPASS_SLOWMODE, "Bypass slowmode", "Ignore this channel's slowmode cooldown."),
    ],
);
const CHANNEL_VOICE: (&str, &[Entry]) = (
    "Audio & video",
    &[
        (P::CONNECT, "Connect to voice", "Join this voice channel and hear others."),
        (P::SPEAK, "Speak", "Talk in this voice channel."),
        (P::STREAM, "Stream video", "Turn on a camera or share a screen in this voice channel."),
        (P::USE_VAD, "Use voice activity", "Without this permission, push-to-talk is required in this channel."),
        (P::PRIORITY_SPEAKER, "Priority speaker", "Be heard over other members in this channel while holding the push to talk (priority) keybind."),
        (P::MUTE_MEMBERS, "Mute members", "Mute other members in this voice channel for everyone."),
        (P::DEAFEN_MEMBERS, "Deafen members", "Deafen other members in this voice channel, so they cannot hear or speak."),
        (P::MOVE_MEMBERS, "Move members", "Drag members out of this voice channel into channels they can access, and disconnect them from voice."),
        (P::UPDATE_RTC_REGION, "Set voice region", "Change which voice region this channel uses."),
    ],
);

const THREADS: [Entry; 4] = [
    (P::CREATE_PUBLIC_THREADS, "Create public threads", "Allows members to start threads that everyone in the channel can see."),
    (P::CREATE_PRIVATE_THREADS, "Create private threads", "Allows members to start threads that only invited members and moderators can see."),
    (P::SEND_MESSAGES_IN_THREADS, "Send messages in threads", "Allows members to send messages in threads."),
    (P::MANAGE_THREADS, "Manage threads", "Allows members to rename, close, lock and delete threads, and to view private threads."),
];
/// `generateForumPermissionSpec`: posts in place of messages and threads.
const FORUM_POSTS: [Entry; 3] = [
    (P::SEND_MESSAGES, "Create posts", "Allows members to create posts in this channel."),
    (P::SEND_MESSAGES_IN_THREADS, "Send messages in posts", "Allows members to send messages in posts."),
    (P::MANAGE_THREADS, "Manage posts", "Allows members to rename, close, lock, pin and delete posts, and to edit their tags."),
];

/// Overwrite editor groups for a channel type (`generateChannelPermissionSpecs`); `threads` when
/// the community has threads on.
pub fn channel(kind: ChannelType, threads: bool) -> Vec<(&'static str, Vec<Entry>)> {
    let mut v = vec![(CHANNEL_ACCESS.0, CHANNEL_ACCESS.1.to_vec()), (CHANNEL_MANAGEMENT.0, CHANNEL_MANAGEMENT.1.to_vec())];
    if threads && matches!(kind, ChannelType::GUILD_FORUM | ChannelType::GUILD_MEDIA) {
        let mut posts = FORUM_POSTS.to_vec();
        posts.extend(CHANNEL_TEXT.1.iter().filter(|e| e.0 != P::SEND_MESSAGES && e.0 != P::PIN_MESSAGES));
        v.push((CHANNEL_TEXT.0, posts));
        return v;
    }
    let mut text = CHANNEL_TEXT.1.to_vec();
    if threads && matches!(kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_CATEGORY) {
        text.extend(THREADS);
    } else if threads && kind == ChannelType::GUILD_ANNOUNCEMENT {
        text.extend(THREADS.iter().filter(|e| e.0 != P::CREATE_PRIVATE_THREADS));
    }
    v.push((CHANNEL_TEXT.0, text));
    if kind == ChannelType::GUILD_VOICE || kind == ChannelType::GUILD_CATEGORY {
        v.push((CHANNEL_VOICE.0, CHANNEL_VOICE.1.to_vec()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_rows_follow_the_channel_type() {
        let bits = |k, th| channel(k, th).into_iter().flat_map(|g| g.1).map(|e| e.0).collect::<Vec<_>>();
        assert!(!bits(ChannelType::GUILD_TEXT, false).contains(&P::MANAGE_THREADS));
        assert!(bits(ChannelType::GUILD_TEXT, true).contains(&P::CREATE_PRIVATE_THREADS));
        assert!(!bits(ChannelType::GUILD_ANNOUNCEMENT, true).contains(&P::CREATE_PRIVATE_THREADS));
        let forum = channel(ChannelType::GUILD_FORUM, true);
        assert_eq!(forum[2].1[0].1, "Create posts");
        assert!(!forum[2].1.iter().any(|e| e.0 == P::PIN_MESSAGES));
    }
}
