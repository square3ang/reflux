//! Threads and forum posts: which the user sees, what they may do with them
//! (`@fluxer/constants/ThreadPermissionUtils`, `threads/utils/ThreadActionRules`) and their
//! notification and unread rules (`threads/utils/ThreadNotificationUtils`).

use reflux_model::{Channel, ChannelType, Message, NotificationLevel, Permissions as P, Snowflake, ThreadMember, ThreadMemberFlags};

use crate::Store;

/// `ThreadCreateKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateKind {
    FromMessage,
    Public,
    Private,
    ForumPost,
}

/// What the user may do with a thread (each `null` denial of `ThreadPermissionUtils`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ThreadRights {
    pub moderator: bool,
    pub member: bool,
    pub owner: bool,
    pub view: bool,
    pub send: bool,
    /// Rename, auto-archive, invitable (`threadPatchRequirement` with `name`).
    pub edit: bool,
    pub archive: bool,
    pub unarchive: bool,
    pub lock: bool,
    pub join: bool,
    pub leave: bool,
    pub delete: bool,
    pub remove_members: bool,
}

/// The thread-only parents: forum and media channels.
pub fn is_forum(kind: ChannelType) -> bool {
    matches!(kind, ChannelType::GUILD_FORUM | ChannelType::GUILD_MEDIA)
}

/// Snowflake of the newest activity: the last message, else the thread itself (`lastActivityId`).
pub fn last_activity(t: &Channel) -> Snowflake {
    t.last_message_id.filter(|&l| l > t.id).unwrap_or(t.id)
}

impl Store {
    /// Visible threads of a parent, oldest first (`ChannelThreads.getThreadsForParent`).
    pub fn threads(&self, parent: Snowflake) -> Vec<&Channel> {
        if !self.can_view(parent) {
            return vec![];
        }
        let mut v: Vec<_> = self.channels.values().filter(|c| c.kind.is_thread() && c.parent_id == Some(parent) && self.thread_rights(c).view).collect();
        v.sort_by_key(|c| c.id);
        v
    }

    /// Threads under a channel in the sidebar: the open one, and active ones the user joined.
    pub fn sidebar_threads(&self, parent: Snowflake, open: Option<Snowflake>) -> Vec<&Channel> {
        self.threads(parent).into_iter().filter(|t| Some(t.id) == open || (!is_archived(t) && t.member.is_some())).collect()
    }

    /// A thread the user can see (`ChannelThreads.getThread`).
    pub fn thread(&self, id: Snowflake) -> Option<&Channel> {
        let t = self.channels.get(&id).filter(|c| c.kind.is_thread())?;
        (t.parent_id.is_some_and(|p| self.can_view(p)) && self.thread_rights(t).view).then_some(t)
    }

    /// Guild owner, administrator, or `MANAGE_THREADS` in the parent while not timed out
    /// (`isThreadModerator`).
    pub fn is_thread_moderator(&self, parent: Snowflake) -> bool {
        let perms = self.permissions(parent);
        self.owns_guild_of(parent) || perms.contains(P::ADMINISTRATOR) || (!self.timed_out_in(parent) && perms.contains(P::MANAGE_THREADS))
    }

    /// The user's own timeout in the parent's guild (`GuildMember.isTimedOut`).
    fn timed_out_in(&self, channel: Snowflake) -> bool {
        let Some(g) = self.channels.get(&channel).and_then(|c| c.guild_id) else { return false };
        let until = self.member(g, self.my_id()).and_then(|m| m.communication_disabled_until.as_deref()).and_then(crate::parse_iso_ms);
        until.is_some_and(|t| t > crate::now_ms())
    }

    /// `isCommunicationBlocked`: timed out, and neither the owner nor an administrator.
    fn communication_blocked(&self, channel: Snowflake) -> bool {
        self.timed_out_in(channel) && !self.owns_guild_of(channel) && !self.permissions(channel).contains(P::ADMINISTRATOR)
    }

    /// `canCreateThread` in a parent.
    pub fn can_create_thread(&self, parent: &Channel, kind: CreateKind) -> bool {
        if kind == CreateKind::Private && parent.kind != ChannelType::GUILD_TEXT {
            return false;
        }
        if self.owns_guild_of(parent.id) {
            return true;
        }
        let p = self.permissions(parent.id);
        p.contains(P::VIEW_CHANNEL)
            && !self.communication_blocked(parent.id)
            && match kind {
                CreateKind::ForumPost => p.contains(P::SEND_MESSAGES),
                CreateKind::FromMessage => p.contains(P::CREATE_PUBLIC_THREADS | P::READ_MESSAGE_HISTORY),
                CreateKind::Public => p.contains(P::CREATE_PUBLIC_THREADS),
                CreateKind::Private => p.contains(P::CREATE_PRIVATE_THREADS),
            }
    }

    /// Text and announcement channels where the user can start a thread (`canStartThreadIn`).
    pub fn can_start_thread(&self, parent: &Channel) -> bool {
        matches!(parent.kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT)
            && (self.can_create_thread(parent, CreateKind::Public) || self.can_create_thread(parent, CreateKind::Private))
    }

    pub fn thread_rights(&self, t: &Channel) -> ThreadRights {
        let Some(parent) = t.parent_id else { return ThreadRights::default() };
        let owner_guild = self.owns_guild_of(parent);
        let moderator = self.is_thread_moderator(parent);
        let pp = self.permissions(parent);
        let meta = t.thread_metadata.clone().unwrap_or_default();
        let (archived, locked) = (meta.archived, meta.locked);
        let member = t.member.is_some();
        let owner = t.owner_id == Some(self.my_id());
        let view = (owner_guild || pp.contains(P::VIEW_CHANNEL)) && (t.kind != ChannelType::PRIVATE_THREAD || member || moderator);
        // Timed out: no writes but leaving (`COMMUNICATION_DISABLED`).
        let ok = !self.communication_blocked(parent);
        let unarchive = ok && (moderator || (!locked && (member || owner)));
        ThreadRights {
            moderator,
            member,
            owner,
            view,
            send: ok && (!locked || moderator),
            edit: ok && (!locked || moderator) && (moderator || owner) && !archived,
            archive: ok && !archived && (!locked || moderator) && (moderator || owner),
            unarchive: archived && unarchive,
            lock: ok && moderator,
            join: view && !archived && (!locked || moderator),
            leave: member && !archived,
            delete: ok && moderator,
            remove_members: ok && !archived && (moderator || (t.kind == ChannelType::PRIVATE_THREAD && owner)),
        }
    }

    /// The thread's own notification flag (`getThreadNotificationSetting`): `INHERIT` for the
    /// parent's default.
    pub fn thread_notify_setting(&self, t: &Channel) -> NotificationLevel {
        let f = t.member.as_ref().map_or(ThreadMemberFlags::empty(), |m| m.flags);
        if f.contains(ThreadMemberFlags::ALL_MESSAGES) {
            NotificationLevel::ALL_MESSAGES
        } else if f.contains(ThreadMemberFlags::ONLY_MENTIONS) {
            NotificationLevel::ONLY_MENTIONS
        } else if f.contains(ThreadMemberFlags::NO_MESSAGES) {
            NotificationLevel::NO_MESSAGES
        } else {
            NotificationLevel::INHERIT
        }
    }

    /// Muted itself, or its parent at any level (`isThreadMuted`).
    pub fn is_thread_muted(&self, t: &Channel) -> bool {
        t.member.as_ref().is_some_and(|m| m.muted) || t.parent_id.and_then(|p| self.channels.get(&p)).is_some_and(|p| self.is_muted(p))
    }

    /// Adds threads a REST call returned (archived lists, search, a created thread) with the
    /// user's memberships among `members`.
    pub fn add_threads(&mut self, threads: &[Channel], members: &[ThreadMember]) {
        for t in threads {
            let mut t = t.clone();
            if let Some(m) = members.iter().find(|m| m.id == Some(t.id)) {
                t.member = Some(m.clone());
            }
            self.upsert_channel(t.guild_id, &t);
        }
    }

    /// Thread objects riding on messages (`ChannelThreads.ingestMessageThreads`).
    pub(crate) fn ingest_message_threads(&mut self, msgs: &[Message]) {
        for m in msgs {
            if let Some(t) = m.thread.as_deref().filter(|t| t.kind.is_thread()) {
                self.upsert_channel(t.guild_id.or(m.guild_id), t);
            }
        }
    }

    fn owns_guild_of(&self, channel: Snowflake) -> bool {
        let g = self.channels.get(&channel).and_then(|c| c.guild_id);
        g.and_then(|g| self.guilds.get(&g)?.info.as_ref()).is_some_and(|i| i.owner_id == self.my_id())
    }
}

pub fn is_archived(t: &Channel) -> bool {
    t.thread_metadata.as_ref().is_some_and(|m| m.archived)
}

pub fn is_locked(t: &Channel) -> bool {
    t.thread_metadata.as_ref().is_some_and(|m| m.locked)
}

#[cfg(test)]
mod tests {
    use reflux_model::DispatchEvent;
    use serde_json::{Value, json};

    use super::*;

    const ME: u64 = 5;
    const G: u64 = 100;

    fn apply(s: &mut Store, t: &str, d: Value) {
        s.apply(&DispatchEvent::parse(t, &d).unwrap(), 0);
    }

    fn thread(id: u64, parent: u64, kind: u16, archived: bool, member: bool) -> Value {
        let mut t = json!({"id": id.to_string(), "type": kind, "guild_id": G.to_string(), "parent_id": parent.to_string(), "name": format!("t{id}"),
            "owner_id": "9", "thread_metadata": {"archived": archived, "auto_archive_duration": 4320, "archive_timestamp": "2026-01-01T00:00:00Z", "locked": false, "create_timestamp": "2026-01-01T00:00:00Z"}});
        if member {
            t["member"] = json!({"join_timestamp": "2026-01-01T00:00:00Z", "flags": 1});
        }
        t
    }

    /// One guild where @everyone can view, send and create public threads; channel 10 is text.
    fn store(perms: P) -> Store {
        let mut s = Store::default();
        apply(&mut s, "READY", json!({"session_id": "s", "user": {"id": ME.to_string(), "username": "me"},
            "guilds": [{"id": G.to_string(), "properties": {"id": G.to_string(), "name": "G", "owner_id": "1"},
                "roles": [{"id": G.to_string(), "name": "@everyone", "permissions": perms.bits().to_string(), "position": 0}],
                "channels": [{"id": "10", "type": 0, "name": "general"}, {"id": "11", "type": 15, "name": "help"}],
                "threads": [thread(20, 10, 11, false, true), thread(21, 10, 11, false, false), thread(22, 10, 12, false, false)],
                "members": [{"user": {"id": ME.to_string()}, "roles": []}]}]}));
        s
    }

    fn basic() -> P {
        P::VIEW_CHANNEL | P::SEND_MESSAGES | P::READ_MESSAGE_HISTORY | P::CREATE_PUBLIC_THREADS | P::SEND_MESSAGES_IN_THREADS
    }

    #[test]
    fn visibility_sidebar_and_rights() {
        let s = store(basic());
        let ids = |v: Vec<&Channel>| v.iter().map(|c| c.id.0).collect::<Vec<_>>();
        assert_eq!(ids(s.threads(Snowflake(10))), [20, 21], "a private thread needs membership or moderation");
        assert_eq!(ids(s.sidebar_threads(Snowflake(10), None)), [20], "joined only");
        assert_eq!(ids(s.sidebar_threads(Snowflake(10), Some(Snowflake(21)))), [20, 21], "plus the open one");
        let r = s.thread_rights(&s.channels[&Snowflake(21)]);
        assert!(r.view && r.join && r.send && !r.leave && !r.edit && !r.delete && !r.lock);
        assert!(s.thread_rights(&s.channels[&Snowflake(20)]).leave);
        assert!(s.can_create_thread(&s.channels[&Snowflake(10)], CreateKind::FromMessage));
        assert!(!s.can_create_thread(&s.channels[&Snowflake(10)], CreateKind::Private));
        assert!(!s.can_create_thread(&s.channels[&Snowflake(11)], CreateKind::Private), "private threads only in text channels");

        let m = store(basic() | P::MANAGE_THREADS);
        assert_eq!(ids(m.threads(Snowflake(10))), [20, 21, 22], "moderators see private threads");
        let r = m.thread_rights(&m.channels[&Snowflake(21)]);
        assert!(r.edit && r.archive && r.lock && r.delete && r.remove_members);

        // A timeout takes moderation and writes away, but not viewing or leaving.
        let mut t = store(basic() | P::MANAGE_THREADS);
        t.guilds.get_mut(&Snowflake(G)).unwrap().members.get_mut(&Snowflake(ME)).unwrap().communication_disabled_until = Some("2999-01-01T00:00:00Z".into());
        let r = t.thread_rights(&t.channels[&Snowflake(20)]);
        assert!(r.view && r.leave && !r.moderator && !r.send && !r.delete && !r.lock);
        assert!(!t.can_create_thread(&t.channels[&Snowflake(10)], CreateKind::Public));
    }

    #[test]
    fn members_events_and_parent_delete() {
        let mut s = store(basic());
        let gm = |u: u64| json!({"user": {"id": u.to_string(), "username": format!("u{u}")}, "roles": []});
        apply(&mut s, "THREAD_MEMBER_LIST_UPDATE", json!({"guild_id": G.to_string(), "thread_id": "21",
            "members": [{"user_id": "7", "join_timestamp": "2026-01-01T00:00:00Z", "flags": 0, "member": gm(7)}]}));
        assert_eq!(s.thread_members[&Snowflake(21)].len(), 1);
        assert!(s.member(Snowflake(G), Snowflake(7)).is_some(), "list members land in the guild cache");
        apply(&mut s, "THREAD_MEMBERS_UPDATE", json!({"id": "21", "guild_id": G.to_string(), "member_count": 2,
            "added_members": [{"user_id": ME.to_string(), "join_timestamp": "2026-01-01T00:00:00Z", "flags": 0, "member": gm(ME)}], "removed_member_ids": ["7"]}));
        let list: Vec<_> = s.thread_members[&Snowflake(21)].iter().map(|m| m.user_id.unwrap().0).collect();
        assert_eq!(list, [ME]);
        assert!(s.channels[&Snowflake(21)].member.is_some(), "joined");
        assert_eq!(s.channels[&Snowflake(21)].member_count, 2);

        // A sync replaces active threads but keeps archived ones loaded by REST.
        s.add_threads(&[serde_json::from_value(thread(23, 10, 11, true, false)).unwrap()], &[]);
        apply(&mut s, "THREAD_LIST_SYNC", json!({"guild_id": G.to_string(), "threads": [thread(20, 10, 11, false, true)], "members": []}));
        assert!(s.channels.contains_key(&Snowflake(23)) && !s.channels.contains_key(&Snowflake(21)));

        apply(&mut s, "CHANNEL_DELETE", json!({"id": "10", "type": 0, "guild_id": G.to_string()}));
        assert!(!s.channels.contains_key(&Snowflake(20)) && !s.channels.contains_key(&Snowflake(23)), "threads go with their parent");
    }

    #[test]
    fn notification_levels_mutes_and_unread() {
        let mut s = store(basic());
        let joined = s.channels[&Snowflake(20)].clone();
        // Guild default (all messages) through the parent.
        assert_eq!(s.notification_level(&joined), NotificationLevel::ALL_MESSAGES);
        assert_eq!(s.notification_level(&s.channels[&Snowflake(21)]), NotificationLevel::ONLY_MENTIONS, "not joined");
        apply(&mut s, "THREAD_MEMBER_UPDATE", json!({"id": "20", "guild_id": G.to_string(), "join_timestamp": "2026-01-01T00:00:00Z", "flags": 9, "muted": true}));
        let t = &s.channels[&Snowflake(20)];
        assert_eq!(s.thread_notify_setting(t), NotificationLevel::NO_MESSAGES);
        assert!(s.is_muted(t));

        // A message carrying its thread (the starter's MESSAGE_UPDATE) adds the thread.
        apply(&mut s, "MESSAGE_CREATE", json!({"id": "30", "channel_id": "10", "guild_id": G.to_string(), "author": {"id": "9", "username": "x"},
            "content": "", "timestamp": "2026-01-01T00:00:00Z", "thread": thread(30, 10, 11, false, false)}));
        assert!(s.thread(Snowflake(30)).is_some());

        // Unread joined, unmuted threads make the guild unread; others don't.
        apply(&mut s, "MESSAGE_CREATE", json!({"id": "40", "channel_id": "21", "author": {"id": "9", "username": "x"}, "content": "", "timestamp": "2026-01-01T00:00:00Z"}));
        s.read_states.clear();
        s.read_states.insert(Snowflake(21), reflux_model::ReadState { id: Snowflake(21), last_message_id: Some(Snowflake(1)), ..Default::default() });
        if let Some(c) = s.channels.get_mut(&Snowflake(10)) {
            c.last_message_id = None;
        }
        assert!(!s.guild_unread(Snowflake(G)).0, "not joined");
        apply(&mut s, "THREAD_MEMBER_UPDATE", json!({"id": "21", "guild_id": G.to_string(), "join_timestamp": "2026-01-01T00:00:00Z", "flags": 0}));
        assert!(s.guild_unread(Snowflake(G)).0);
    }
}
