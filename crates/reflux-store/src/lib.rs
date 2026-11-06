//! In-memory cache of Gateway state plus the queries the UI needs.
//!
//! Plain data owned by the UI thread: feed every Dispatch to [`Store::apply`], feed REST message
//! pages to the `*_messages` helpers, and read through the query methods. Behaviour mirrors the
//! fluxer_app stores (GuildList, ChannelOrganization, ReadStates, ChannelMessages, RollingTyping).

use std::collections::HashMap;

use reflux_model::{
    Channel, ChannelType, DispatchEvent, Emoji, Guild, GuildFolder, GuildMember, GuildReady,
    MemberListGroup, Message, MessageReaction, NotificationLevel, PartialUser, PermissionInput, Permissions, Presence,
    ReactionEmoji, ReadState, Relationship, RelationshipType, Role, SessionPresence, Snowflake,
    Sticker, ThreadMember, Timestamp, User, UserGuildSettings, UserSettings, VoiceState, compute_permissions,
    thread_permissions,
};

pub mod threads;
pub use threads::{CreateKind, ThreadRights};

/// Live window size cap (fluxer `MAX_LOADED_MESSAGES`).
pub const MAX_WINDOW: usize = 200;
/// REST page size the app should request (fluxer `MAX_MESSAGES_PER_CHANNEL`).
pub const PAGE_SIZE: usize = 50;
/// A typing indicator lasts this long after the last `TYPING_START` (fluxer rolling expiry).
pub const TYPING_EXPIRY_MS: u64 = 10_000;

#[derive(Clone, Debug, Default)]
pub struct GuildState {
    pub id: Snowflake,
    /// `None` while the guild has never been available.
    pub info: Option<Guild>,
    pub unavailable: bool,
    pub visiting: bool,
    pub joined_at: Option<Timestamp>,
    pub roles: Vec<Role>,
    pub emojis: Vec<Emoji>,
    pub stickers: Vec<Sticker>,
    /// Partial: own member, voice members, chunks, member-list rows, message authors.
    pub members: HashMap<Snowflake, GuildMember>,
    pub member_count: u32,
    pub online_count: u32,
}

/// Messages of one channel, ascending by ID.
#[derive(Clone, Debug, Default)]
pub struct MessageWindow {
    pub messages: Vec<Message>,
    pub has_more_before: bool,
    /// The window is scrolled into history; live messages are not appended.
    pub has_more_after: bool,
}

/// `GUILD_MEMBER_LIST_UPDATE` state for one list (channel).
#[derive(Clone, Debug, Default)]
pub struct MemberList {
    pub member_count: u32,
    pub online_count: u32,
    pub groups: Vec<MemberListGroup>,
    /// `None` = row not synced yet.
    pub rows: Vec<Option<MemberRow>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MemberRow {
    Group(MemberListGroup),
    Member(Snowflake),
}

#[derive(Clone, Debug, PartialEq)]
pub enum GuildListItem<'a> {
    Guild(Snowflake),
    Folder { folder: &'a GuildFolder, guilds: Vec<Snowflake> },
}

/// One sidebar section: the uncategorized group first (no category), then each category.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelGroup<'a> {
    pub category: Option<&'a Channel>,
    /// Text, announcement, link, forum and media channels.
    pub text: Vec<&'a Channel>,
    pub voice: Vec<&'a Channel>,
}

#[derive(Clone, Debug, Default)]
pub struct Store {
    /// Streaming privacy: [`Store::display_name`] shortens names to their first letter.
    pub mask_names: bool,
    pub me: Option<User>,
    pub session_id: String,
    pub users: HashMap<Snowflake, PartialUser>,
    pub guilds: HashMap<Snowflake, GuildState>,
    /// Every channel: guild channels, threads and private channels.
    pub channels: HashMap<Snowflake, Channel>,
    pub relationships: HashMap<Snowflake, Relationship>,
    pub presences: HashMap<Snowflake, Presence>,
    pub sessions: Vec<SessionPresence>,
    pub read_states: HashMap<Snowflake, ReadState>,
    pub settings: UserSettings,
    /// Keyed by guild ID; `None` = DM settings.
    pub guild_settings: HashMap<Option<Snowflake>, UserGuildSettings>,
    pub notes: HashMap<Snowflake, String>,
    pub pinned_dms: Vec<Snowflake>,
    /// Keyed by (guild, user); DM calls use `None`.
    pub voice_states: HashMap<(Option<Snowflake>, Snowflake), VoiceState>,
    pub messages: HashMap<Snowflake, MessageWindow>,
    pub member_lists: HashMap<(Snowflake, String), MemberList>,
    /// Channel → (user, expires at ms).
    pub typing: HashMap<Snowflake, Vec<(Snowflake, u64)>>,
    /// Members of threads subscribed with `thread_member_lists` (`THREAD_MEMBER_LIST_UPDATE`).
    pub thread_members: HashMap<Snowflake, Vec<ThreadMember>>,
}

impl Store {
    pub fn my_id(&self) -> Snowflake {
        self.me.as_ref().map(|u| u.id).unwrap_or_default()
    }

    /// Applies one Dispatch. `now_ms` (Unix ms) is only used for typing expiry.
    pub fn apply(&mut self, ev: &DispatchEvent, now_ms: u64) {
        use DispatchEvent as E;
        match ev {
            E::Ready(r) => {
                *self = Store { session_id: r.session_id.clone(), ..Default::default() };
                self.upsert_user(&r.user.base);
                self.me = Some(r.user.clone());
                r.users.iter().for_each(|u| self.upsert_user(u));
                r.guilds.iter().for_each(|g| self.guild_create(g));
                for c in &r.private_channels {
                    c.recipients.iter().for_each(|u| self.upsert_user(u));
                    self.channels.insert(c.id, c.clone());
                }
                for rel in &r.relationships {
                    self.relationship(rel);
                }
                r.presences.iter().for_each(|p| self.presence(p));
                self.sessions = r.sessions.clone();
                self.read_states = r.read_states.iter().map(|s| (s.id, s.clone())).collect();
                self.settings = r.user_settings.clone().unwrap_or_default();
                for s in &r.user_guild_settings {
                    self.guild_settings.insert(s.guild_id, s.clone());
                }
                self.notes = r.notes.clone();
                self.pinned_dms = r.pinned_dms.clone();
            }
            E::SessionsReplace(s) => self.sessions = s.clone(),
            E::UserUpdate(u) => {
                self.upsert_user(&u.base);
                self.me = Some((**u).clone());
            }
            E::UserSettingsUpdate(s) => self.settings = (**s).clone(),
            E::UserGuildSettingsUpdate(s) => {
                self.guild_settings.insert(s.guild_id, s.clone());
            }
            E::UserNoteUpdate(n) => {
                self.notes.insert(n.id, n.note.clone());
            }
            E::UserPinnedDmsUpdate(ids) => self.pinned_dms = ids.clone(),
            E::RelationshipAdd(r) | E::RelationshipUpdate(r) => self.relationship(r),
            E::RelationshipRemove(r) => {
                self.relationships.remove(&r.id);
            }

            E::GuildCreate(g) | E::GuildSync(g) => self.guild_create(g),
            E::GuildUpdate(g) => {
                if let Some(s) = self.guilds.get_mut(&g.id) {
                    s.info = Some((**g).clone());
                }
            }
            E::GuildDelete(d) => {
                if d.unavailable {
                    if let Some(g) = self.guilds.get_mut(&d.id) {
                        g.unavailable = true;
                    }
                } else {
                    self.remove_guild(d.id);
                }
            }
            E::GuildRoleCreate(r) | E::GuildRoleUpdate(r) => self.upsert_role(r.guild_id, &r.role),
            E::GuildRoleUpdateBulk(r) => r.roles.iter().for_each(|role| self.upsert_role(r.guild_id, role)),
            E::GuildRoleDelete(r) => {
                if let Some(g) = self.guilds.get_mut(&r.guild_id) {
                    g.roles.retain(|x| x.id != r.role_id);
                    g.members.values_mut().for_each(|m| m.roles.retain(|&x| x != r.role_id));
                }
            }
            E::GuildEmojisUpdate(e) => {
                if let Some(g) = self.guilds.get_mut(&e.guild_id) {
                    g.emojis = e.emojis.clone();
                }
            }
            E::GuildStickersUpdate(e) => {
                if let Some(g) = self.guilds.get_mut(&e.guild_id) {
                    g.stickers = e.stickers.clone();
                }
            }

            E::ChannelCreate(c) | E::ChannelUpdate(c) | E::ThreadCreate(c) | E::ThreadUpdate(c) => {
                self.upsert_channel(c.guild_id, c)
            }
            E::ChannelUpdateBulk(b) => b.channels.iter().for_each(|c| self.upsert_channel(Some(b.guild_id), c)),
            E::ChannelDelete(c) => {
                // Its threads go with it (`ChannelThreads.handleParentDelete`).
                let threads: Vec<_> = self.channels.values().filter(|t| t.kind.is_thread() && t.parent_id == Some(c.id)).map(|t| t.id).collect();
                threads.into_iter().chain([c.id]).for_each(|id| self.remove_channel(id));
            }
            E::ThreadDelete(t) => self.remove_channel(t.id),
            E::ChannelRecipientAdd(r) => {
                self.upsert_user(&r.user);
                if let Some(c) = self.channels.get_mut(&r.channel_id)
                    && !c.recipients.iter().any(|u| u.id == r.user.id)
                {
                    c.recipients.push(r.user.clone());
                }
            }
            E::ChannelRecipientRemove(r) => {
                if let Some(c) = self.channels.get_mut(&r.channel_id) {
                    c.recipients.retain(|u| u.id != r.user.id);
                }
            }
            E::ChannelPinsUpdate(p) => {
                if let Some(c) = self.channels.get_mut(&p.channel_id) {
                    c.last_pin_timestamp = p.last_pin_timestamp.clone();
                }
            }
            E::ChannelPinsAck(p) => {
                self.read_state(p.channel_id).last_pin_timestamp = Some(p.timestamp.clone());
            }
            E::ThreadListSync(s) => {
                let gone: Vec<_> = self
                    .channels
                    .values()
                    // Archived threads stay (fluxer keeps those it loaded by REST).
                    .filter(|c| c.guild_id == Some(s.guild_id) && c.kind.is_thread() && !threads::is_archived(c))
                    .filter(|c| s.channel_ids.as_ref().is_none_or(|ids| c.parent_id.is_some_and(|p| ids.contains(&p))))
                    .map(|c| c.id)
                    .collect();
                gone.into_iter().for_each(|id| self.remove_channel(id));
                s.threads.iter().for_each(|t| self.upsert_channel(Some(s.guild_id), t));
                for m in &s.members {
                    if let Some(t) = m.id.and_then(|id| self.channels.get_mut(&id)) {
                        t.member = Some(m.clone());
                    }
                }
            }
            E::ThreadMemberUpdate(m) => {
                if let Some(t) = m.id.and_then(|id| self.channels.get_mut(&id)) {
                    t.member = Some((**m).clone());
                }
            }
            E::ThreadMembersUpdate(u) => {
                let me = self.my_id();
                if let Some(t) = self.channels.get_mut(&u.id) {
                    t.member_count = u.member_count;
                    if u.removed_member_ids.contains(&me) {
                        t.member = None;
                    }
                    if let Some(m) = u.added_members.iter().find(|m| m.user_id == Some(me)) {
                        t.member = Some(m.clone());
                    }
                }
                for m in &u.added_members {
                    self.thread_member_side_effects(u.guild_id, m);
                }
                if let Some(list) = self.thread_members.get_mut(&u.id) {
                    list.retain(|m| m.user_id.is_none_or(|id| !u.removed_member_ids.contains(&id)));
                    for m in &u.added_members {
                        if !list.iter().any(|x| x.user_id == m.user_id) {
                            list.push(m.clone());
                        }
                    }
                }
            }
            E::ThreadMemberListUpdate(u) => {
                u.members.iter().for_each(|m| self.thread_member_side_effects(u.guild_id, m));
                self.thread_members.insert(u.thread_id, u.members.clone());
            }

            E::GuildMemberAdd(m) => {
                self.upsert_member(m.guild_id, &m.inner);
                if let Some(g) = self.guilds.get_mut(&m.guild_id) {
                    g.member_count += 1;
                }
            }
            E::GuildMemberUpdate(m) => self.upsert_member(m.guild_id, &m.inner),
            E::GuildMemberRemove(r) => {
                if let Some(g) = self.guilds.get_mut(&r.guild_id) {
                    g.members.remove(&r.user.id);
                    g.member_count = g.member_count.saturating_sub(1);
                }
            }
            E::GuildMembersChunk(c) => {
                c.members.iter().for_each(|m| self.upsert_member(c.guild_id, m));
                c.presences.iter().for_each(|p| self.presence(p));
            }
            E::GuildMemberListUpdate(u) => self.member_list_update(u),
            E::GuildCountsUpdate(c) => {
                for e in c.counts.iter().filter(|e| e.channel_id.is_none()) {
                    if let Some(g) = self.guilds.get_mut(&e.guild_id) {
                        (g.member_count, g.online_count) = (e.member_count, e.online_count);
                    }
                }
            }
            E::PresenceUpdate(p) => self.presence(p),
            E::PresenceUpdateBulk(b) => b.presences.iter().for_each(|p| self.presence(p)),
            E::PassiveUpdates(p) => {
                for (&ch, &id) in &p.channels {
                    self.bump_last_message(ch, id);
                }
                p.voice_states.iter().for_each(|v| self.voice_state(v));
            }

            E::MessageCreate(m) => self.message_create(m),
            E::MessageUpdate(m) => self.message_update(m),
            E::MessageDelete(d) => self.remove_messages(d.channel_id, &[d.id]),
            E::MessageDeleteBulk(d) => self.remove_messages(d.channel_id, &d.ids),
            E::MessageAck(a) => {
                let rs = self.read_state(a.channel_id);
                // Ignore stale acks for an older message (fluxer `ignoreOlderMessage`) unless manual.
                if a.manual || rs.last_message_id.is_none_or(|ack| a.message_id >= ack) {
                    rs.last_message_id = Some(a.message_id);
                    rs.mention_count = a.mention_count;
                }
            }
            E::MessageReactionAdd(r) => self.reaction(r, true),
            E::MessageReactionRemove(r) => self.reaction(r, false),
            E::MessageReactionAddMany(r) => {
                for a in &r.reactions {
                    let one = MessageReaction {
                        user_id: a.user_id,
                        channel_id: r.channel_id,
                        message_id: r.message_id,
                        emoji: a.emoji.clone(),
                        ..Default::default()
                    };
                    self.reaction(&one, true);
                }
            }
            E::MessageReactionRemoveAll(r) | E::MessageReactionRemoveEmoji(r) => {
                if let Some(m) = self.message_mut(r.channel_id, r.message_id) {
                    match &r.emoji {
                        Some(e) => m.reactions.retain(|x| !same_emoji(&x.emoji, e)),
                        None => m.reactions.clear(),
                    }
                }
            }
            E::TypingStart(t) => {
                if let (Some(g), Some(m)) = (t.guild_id, &t.member) {
                    self.upsert_member(g, m);
                }
                let list = self.typing.entry(t.channel_id).or_default();
                list.retain(|&(u, exp)| u != t.user_id && exp > now_ms);
                list.push((t.user_id, now_ms + TYPING_EXPIRY_MS));
            }

            E::VoiceStateUpdate(v) => self.voice_state(v),
            E::CallCreate(c) | E::CallUpdate(c) => c.voice_states.iter().for_each(|v| self.voice_state(v)),
            _ => {}
        }
    }

    // ---- REST message pages ----------------------------------------------------------------

    /// Replaces the window with the newest page (`GET /messages?limit=`).
    pub fn set_latest_messages(&mut self, channel: Snowflake, page: Vec<Message>, limit: usize) {
        self.ingest_message_threads(&page);
        let more = page.len() >= limit;
        let w = self.messages.entry(channel).or_default();
        *w = MessageWindow { has_more_before: more, ..Default::default() };
        merge(&mut w.messages, page);
        trim_oldest(w);
    }

    /// Adds an older page (`before=` the oldest loaded ID); drops the newest beyond the cap.
    pub fn prepend_messages(&mut self, channel: Snowflake, page: Vec<Message>, limit: usize) {
        self.ingest_message_threads(&page);
        let w = self.messages.entry(channel).or_default();
        w.has_more_before = page.len() >= limit;
        merge(&mut w.messages, page);
        if w.messages.len() > MAX_WINDOW {
            w.messages.truncate(MAX_WINDOW);
            w.has_more_after = true;
        }
    }

    /// Adds a newer page (`after=` the newest loaded ID) while scrolled into history.
    pub fn append_messages(&mut self, channel: Snowflake, page: Vec<Message>, limit: usize) {
        self.ingest_message_threads(&page);
        let w = self.messages.entry(channel).or_default();
        w.has_more_after = page.len() >= limit;
        merge(&mut w.messages, page);
        trim_oldest(w);
    }

    /// Replaces the window with a page around a message (`around=`, jump to message). It's the
    /// newest only when it reaches the channel's last message.
    pub fn set_around_messages(&mut self, channel: Snowflake, page: Vec<Message>, limit: usize) {
        self.ingest_message_threads(&page);
        let last = self.channels.get(&channel).and_then(|c| c.last_message_id);
        let newest = page.iter().map(|m| m.id).max();
        let w = self.messages.entry(channel).or_default();
        *w = MessageWindow { has_more_before: page.len() >= limit, has_more_after: newest.is_some_and(|n| last.is_some_and(|l| l > n)), ..Default::default() };
        merge(&mut w.messages, page);
    }

    /// Inserts an optimistic own message. Its `id` must be the nonce parsed as a snowflake and
    /// `nonce` must be set; the matching `MESSAGE_CREATE` replaces it.
    pub fn add_local_message(&mut self, m: Message) {
        let w = self.messages.entry(m.channel_id).or_default();
        merge(&mut w.messages, vec![m]);
    }

    // ---- Queries -----------------------------------------------------------------------------

    pub fn messages(&self, channel: Snowflake) -> &[Message] {
        self.messages.get(&channel).map_or(&[], |w| &w.messages)
    }

    pub fn typing_users(&self, channel: Snowflake, now_ms: u64) -> Vec<Snowflake> {
        let me = self.my_id();
        self.typing
            .get(&channel)
            .into_iter()
            .flatten()
            .filter(|&&(u, exp)| exp > now_ms && u != me)
            .map(|&(u, _)| u)
            .collect()
    }

    /// Guild sidebar order (fluxer `GuildList.computeOrganizedGuildList`).
    pub fn guild_list(&self) -> Vec<GuildListItem<'_>> {
        let usable = |id: &Snowflake| self.guilds.get(id).is_some_and(|g| !g.visiting);
        let mut placed = std::collections::HashSet::new();
        let mut emitted = std::collections::HashSet::new();
        let mut out = Vec::new();
        for f in &self.settings.guild_folders {
            if f.id != Some(-1) && emitted.contains(&f.id) {
                continue;
            }
            let guilds: Vec<_> = f.guild_ids.iter().copied().filter(|id| usable(id) && placed.insert(*id)).collect();
            if guilds.is_empty() {
                continue;
            }
            emitted.insert(f.id);
            if f.id == Some(-1) {
                out.extend(guilds.into_iter().map(GuildListItem::Guild));
            } else {
                out.push(GuildListItem::Folder { folder: f, guilds });
            }
        }
        let mut rest: Vec<_> =
            self.guilds.values().filter(|g| !g.visiting && !g.unavailable && !placed.contains(&g.id)).collect();
        // Fluxer sorts unplaced guilds by name, then prepends them in reverse.
        rest.sort_by(|a, b| guild_name(a).cmp(guild_name(b)).then(a.id.cmp(&b.id)));
        rest.iter().rev().map(|g| GuildListItem::Guild(g.id)).chain(out).collect()
    }

    /// DMs and group DMs, most recent activity first (fluxer `sortDMs`).
    pub fn private_channels(&self) -> Vec<&Channel> {
        let mut v: Vec<_> = self
            .channels
            .values()
            .filter(|c| matches!(c.kind, ChannelType::DM | ChannelType::GROUP_DM))
            .collect();
        v.sort_by_key(|c| std::cmp::Reverse((c.last_message_id.unwrap_or(c.id), c.id)));
        v
    }

    /// The current user's permissions in a channel (guild, thread or private).
    pub fn permissions(&self, channel: Snowflake) -> Permissions {
        let Some(c) = self.channels.get(&channel) else { return Permissions::empty() };
        let Some(gid) = c.guild_id else { return Permissions::all() };
        if c.kind.is_thread() {
            return c.parent_id.map_or(Permissions::empty(), |p| thread_permissions(self.permissions(p)));
        }
        self.compute(gid, &c.permission_overwrites)
    }

    pub fn guild_permissions(&self, guild: Snowflake) -> Permissions {
        self.compute(guild, &[])
    }

    pub fn can_view(&self, channel: Snowflake) -> bool {
        self.permissions(channel).contains(Permissions::VIEW_CHANNEL)
    }

    /// Visible guild channels grouped for the sidebar (fluxer `organizeChannels`).
    pub fn channel_groups(&self, guild: Snowflake) -> Vec<ChannelGroup<'_>> {
        let mut chans: Vec<_> = self
            .channels
            .values()
            .filter(|c| c.guild_id == Some(guild) && !c.kind.is_thread() && self.can_view(c.id))
            .collect();
        chans.sort_by_key(|c| (c.position, c.id));
        let mut groups = vec![ChannelGroup { category: None, text: vec![], voice: vec![] }];
        let mut index = HashMap::new();
        for c in chans.iter().filter(|c| c.kind == ChannelType::GUILD_CATEGORY) {
            index.insert(c.id, groups.len());
            groups.push(ChannelGroup { category: Some(c), text: vec![], voice: vec![] });
        }
        for c in chans {
            // Parent category missing or hidden: fluxer never emits that bucket, so drop it.
            let Some(&gi) = c.parent_id.map_or(Some(&0), |p| index.get(&p)) else { continue };
            match c.kind {
                ChannelType::GUILD_TEXT
                | ChannelType::GUILD_ANNOUNCEMENT
                | ChannelType::GUILD_LINK
                | ChannelType::GUILD_FORUM
                | ChannelType::GUILD_MEDIA => groups[gi].text.push(c),
                ChannelType::GUILD_VOICE => groups[gi].voice.push(c),
                _ => {}
            }
        }
        groups
    }

    pub fn member(&self, guild: Snowflake, user: Snowflake) -> Option<&GuildMember> {
        self.guilds.get(&guild)?.members.get(&user)
    }

    /// Nick, then global name, then username.
    pub fn display_name(&self, guild: Option<Snowflake>, user: Snowflake) -> String {
        let nick = guild.and_then(|g| self.member(g, user)?.nick.clone());
        let u = self.users.get(&user);
        let name = nick.or_else(|| u.and_then(|u| u.global_name.clone())).or_else(|| u.map(|u| u.username.clone())).unwrap_or_default();
        match name.trim().chars().next() {
            Some(c) if self.mask_names => format!("{c}…"),
            _ => name,
        }
    }

    /// Colour of the highest coloured role, else the @everyone colour (fluxer `getColorString`).
    pub fn member_color(&self, guild: Snowflake, user: Snowflake) -> Option<u32> {
        let g = self.guilds.get(&guild)?;
        let m = g.members.get(&user)?;
        g.roles
            .iter()
            .filter(|r| r.color != 0 && m.roles.contains(&r.id))
            .min_by_key(|r| (std::cmp::Reverse(r.position), r.id))
            .or_else(|| g.roles.iter().find(|r| r.id == guild && r.color != 0))
            .map(|r| r.color)
    }

    pub fn is_blocked(&self, user: Snowflake) -> bool {
        self.relationships.get(&user).is_some_and(|r| r.kind == RelationshipType::BLOCKED)
    }

    /// Fluxer `ReadStateEntry.hasUnread` (with `canBeUnread`).
    pub fn has_unread(&self, channel: Snowflake) -> bool {
        let Some(c) = self.channels.get(&channel) else { return false };
        if !self.tracks_unread(c) {
            return false;
        }
        let Some(last) = c.last_message_id else { return false };
        match self.read_states.get(&channel).and_then(|r| r.last_message_id) {
            Some(ack) => ack < last,
            None => {
                let joined = c.guild_id.and_then(|g| self.guilds.get(&g)?.joined_at.as_deref().and_then(parse_iso_ms));
                last.timestamp_ms() > joined.unwrap_or(channel.timestamp_ms())
            }
        }
    }

    pub fn mention_count(&self, channel: Snowflake) -> u32 {
        match self.channels.get(&channel) {
            Some(c) if self.tracks_unread(c) => self.read_states.get(&channel).map_or(0, |r| r.mention_count),
            _ => 0,
        }
    }

    /// (unread, mention total) over the viewable channels of a guild. Muted channels don't make
    /// the guild unread but their mentions still count.
    pub fn guild_unread(&self, guild: Snowflake) -> (bool, u32) {
        let mut unread = false;
        let mut mentions = 0;
        for c in self.channels.values().filter(|c| c.guild_id == Some(guild)) {
            if c.kind == ChannelType::GUILD_CATEGORY || !self.can_view(c.id) {
                continue;
            }
            mentions += self.mention_count(c.id);
            // Threads count only when joined (`getThreadContribution`).
            unread |= (!c.kind.is_thread() || c.member.is_some()) && !self.is_muted(c) && self.has_unread(c.id);
        }
        (unread, mentions)
    }

    /// Muted through guild settings, the channel override or its category's override.
    // shortcut: ignores `mute_config.end_time` expiry; pass `now` in if timed mutes matter.
    pub fn is_muted(&self, c: &Channel) -> bool {
        if c.kind.is_thread() {
            return self.is_thread_muted(c);
        }
        let Some(s) = self.guild_settings.get(&c.guild_id) else { return false };
        let ov = |id: Option<Snowflake>| {
            id.and_then(|id| s.channel_overrides.as_ref()?.get(&id)).is_some_and(|o| o.muted)
        };
        (c.guild_id.is_some() && s.muted) || ov(Some(c.id)) || ov(c.parent_id)
    }

    /// Fluxer `resolveEffectiveMessageNotifications`: the channel override, its category's, the
    /// guild setting, then the guild default (DMs default to all messages).
    pub fn notification_level(&self, c: &Channel) -> NotificationLevel {
        // A joined thread: its own flag, else its parent's level (`resolveThreadNotificationLevel`);
        // others only notify on mentions.
        if c.kind.is_thread() {
            let parent = c.parent_id.and_then(|p| self.channels.get(&p));
            return match (c.member.is_some(), self.thread_notify_setting(c), parent) {
                (false, ..) | (true, NotificationLevel::INHERIT, None) => NotificationLevel::ONLY_MENTIONS,
                (true, NotificationLevel::INHERIT, Some(p)) => self.notification_level(p),
                (true, l, _) => l,
            };
        }
        let s = self.guild_settings.get(&c.guild_id);
        let explicit = |l: NotificationLevel| (l != NotificationLevel::INHERIT).then_some(l);
        let ov = |id: Option<Snowflake>| explicit(id.and_then(|id| s?.channel_overrides.as_ref()?.get(&id))?.message_notifications);
        let parent = c.parent_id.filter(|_| c.guild_id.is_some());
        let guild_default = || {
            let d = c.guild_id.and_then(|g| self.guilds.get(&g)?.info.as_ref()).map(|g| g.default_message_notifications);
            NotificationLevel(d.unwrap_or(0))
        };
        ov(Some(c.id)).or_else(|| ov(parent)).or_else(|| explicit(s?.message_notifications)).unwrap_or_else(guild_default)
    }

    /// Whether a new message from someone else should alert (sound + desktop notification),
    /// leaving focus to the caller: Fluxer `validateNotificationData` + `shouldNotifyBasedOnSettings`.
    // shortcut: forum "new post" levels aren't modelled (unjoined posts notify on mentions).
    pub fn should_notify(&self, m: &Message) -> bool {
        let Some(c) = self.channels.get(&m.channel_id) else { return false };
        if m.author.id == self.my_id()
            || self.is_blocked(m.author.id)
            || self.settings.status == reflux_model::Status::Dnd
            || m.flags.contains(reflux_model::MessageFlags::SUPPRESS_NOTIFICATIONS)
            || self.is_muted(c)
        {
            return false;
        }
        match self.notification_level(c) {
            NotificationLevel::NO_MESSAGES => false,
            NotificationLevel::ALL_MESSAGES => true,
            _ => self.mentions_me(m),
        }
    }

    // ---- Internals ---------------------------------------------------------------------------

    fn compute(&self, guild: Snowflake, overwrites: &[reflux_model::PermissionOverwrite]) -> Permissions {
        let Some(g) = self.guilds.get(&guild) else { return Permissions::empty() };
        let info = g.info.as_ref();
        let me = self.my_id();
        compute_permissions(&PermissionInput {
            guild_id: guild,
            owner_id: info.map(|i| i.owner_id).unwrap_or_default(),
            user_id: me,
            roles: &g.roles,
            member_roles: g.members.get(&me).map(|m| m.roles.as_slice()),
            overwrites,
            visiting: g.visiting,
            strip_elevated: info.is_some_and(|i| i.mfa_level == 1)
                && !self.me.as_ref().is_some_and(|u| u.mfa_enabled),
        })
    }

    fn tracks_unread(&self, c: &Channel) -> bool {
        if c.guild_id.and_then(|g| self.guilds.get(&g)).is_some_and(|g| g.visiting) {
            return false;
        }
        // A DM with a blocked recipient never shows unread.
        !(c.kind == ChannelType::DM && c.recipients.first().is_some_and(|u| self.is_blocked(u.id)))
    }

    fn read_state(&mut self, channel: Snowflake) -> &mut ReadState {
        self.read_states.entry(channel).or_insert_with(|| ReadState { id: channel, ..Default::default() })
    }

    /// Keeps known fields when the incoming user is only `{id}`.
    fn upsert_user(&mut self, u: &PartialUser) {
        if !u.username.is_empty() || !self.users.contains_key(&u.id) {
            self.users.insert(u.id, u.clone());
        }
    }

    fn upsert_member(&mut self, guild: Snowflake, m: &GuildMember) {
        self.upsert_user(&m.user);
        if let Some(p) = &m.presence {
            let mut p = (**p).clone();
            p.user.id = m.user.id;
            self.presence(&p);
        }
        let Some(g) = self.guilds.get_mut(&guild) else { return };
        let mut m = m.clone();
        m.presence = None;
        if m.user.username.is_empty()
            && let Some(u) = self.users.get(&m.user.id)
        {
            m.user = u.clone();
        }
        g.members.insert(m.user.id, m);
    }

    fn presence(&mut self, p: &Presence) {
        self.upsert_user(&p.user);
        self.presences.insert(p.user.id, p.clone());
    }

    fn relationship(&mut self, r: &Relationship) {
        let mut r = r.clone();
        match &r.user {
            Some(u) => self.upsert_user(u),
            None => r.user = self.users.get(&r.id).cloned(),
        }
        self.relationships.insert(r.id, r);
    }

    fn voice_state(&mut self, v: &VoiceState) {
        let Some(user) = v.user_id.or(v.member.as_ref().map(|m| m.user.id)) else { return };
        if let (Some(g), Some(m)) = (v.guild_id, &v.member) {
            self.upsert_member(g, m);
        }
        let key = (v.guild_id, user);
        if v.channel_id.is_none() {
            self.voice_states.remove(&key);
        } else {
            self.voice_states.insert(key, v.clone());
        }
    }

    fn guild_create(&mut self, r: &GuildReady) {
        if r.unavailable {
            self.guilds.entry(r.id).or_insert_with(|| GuildState { id: r.id, ..Default::default() }).unavailable = true;
            return;
        }
        let old_members = self.guilds.remove(&r.id).map(|g| g.members).unwrap_or_default();
        self.guilds.insert(
            r.id,
            GuildState {
                id: r.id,
                info: r.properties.clone(),
                unavailable: false,
                visiting: r.visiting,
                joined_at: r.joined_at.clone(),
                roles: r.roles.clone(),
                emojis: r.emojis.clone(),
                stickers: r.stickers.clone(),
                members: old_members,
                member_count: r.member_count,
                online_count: r.online_count,
            },
        );
        r.members.iter().for_each(|m| self.upsert_member(r.id, m));
        r.presences.iter().for_each(|p| self.presence(p));
        self.voice_states.retain(|(g, _), _| *g != Some(r.id));
        r.voice_states.iter().for_each(|v| self.voice_state(&VoiceState { guild_id: Some(r.id), ..v.clone() }));
        let gone: Vec<_> = self.channels.values().filter(|c| c.guild_id == Some(r.id)).map(|c| c.id).collect();
        let keep: std::collections::HashSet<_> = r.channels.iter().chain(&r.threads).map(|c| c.id).collect();
        for id in gone.into_iter().filter(|id| !keep.contains(id)) {
            self.remove_channel(id);
        }
        r.channels.iter().chain(&r.threads).for_each(|c| self.upsert_channel(Some(r.id), c));
    }

    fn remove_guild(&mut self, id: Snowflake) {
        self.guilds.remove(&id);
        let gone: Vec<_> = self.channels.values().filter(|c| c.guild_id == Some(id)).map(|c| c.id).collect();
        gone.into_iter().for_each(|c| self.remove_channel(c));
        self.voice_states.retain(|(g, _), _| *g != Some(id));
        self.member_lists.retain(|(g, _), _| *g != id);
    }

    fn upsert_role(&mut self, guild: Snowflake, role: &Role) {
        let Some(g) = self.guilds.get_mut(&guild) else { return };
        match g.roles.iter_mut().find(|r| r.id == role.id) {
            Some(r) => *r = role.clone(),
            None => g.roles.push(role.clone()),
        }
    }

    fn upsert_channel(&mut self, guild: Option<Snowflake>, c: &Channel) {
        let mut c = c.clone();
        c.guild_id = c.guild_id.or(guild);
        c.newly_created = false;
        if let Some(old) = self.channels.get(&c.id) {
            // Updates may carry a stale last_message_id (fluxer keeps the newer one).
            c.last_message_id = c.last_message_id.max(old.last_message_id);
            if c.member.is_none() {
                c.member = old.member.clone();
            }
        }
        c.recipients.iter().for_each(|u| self.upsert_user(u));
        self.channels.insert(c.id, c);
    }

    fn remove_channel(&mut self, id: Snowflake) {
        self.channels.remove(&id);
        self.thread_members.remove(&id);
        self.messages.remove(&id);
        self.typing.remove(&id);
    }

    fn bump_last_message(&mut self, channel: Snowflake, id: Snowflake) {
        if let Some(c) = self.channels.get_mut(&channel) {
            c.last_message_id = c.last_message_id.max(Some(id));
        }
    }

    fn message_mut(&mut self, channel: Snowflake, id: Snowflake) -> Option<&mut Message> {
        let w = self.messages.get_mut(&channel)?;
        let i = w.messages.binary_search_by_key(&id, |m| m.id).ok()?;
        Some(&mut w.messages[i])
    }

    fn thread_member_side_effects(&mut self, guild: Snowflake, m: &ThreadMember) {
        if let Some(gm) = &m.member {
            self.upsert_member(guild, gm);
        }
        if let Some(p) = &m.presence {
            self.presence(p);
        }
    }

    fn message_side_effects(&mut self, m: &Message) {
        self.ingest_message_threads(std::slice::from_ref(m));
        self.upsert_user(&m.author);
        if let (Some(g), Some(member)) = (m.guild_id, &m.member) {
            self.upsert_member(g, &GuildMember { user: m.author.clone(), ..member.clone() });
        }
    }

    fn message_create(&mut self, m: &Message) {
        self.message_side_effects(m);
        if let Some(t) = self.typing.get_mut(&m.channel_id) {
            t.retain(|&(u, _)| u != m.author.id);
        }
        let covered = self.channels.get(&m.channel_id).and_then(|c| c.last_message_id).is_some_and(|l| l >= m.id);
        self.bump_last_message(m.channel_id, m.id);

        // A channel never loaded keeps no window, so opening it fetches its history (fluxer
        // drops messages for channels it hasn't loaded).
        if let Some(w) = self.messages.get_mut(&m.channel_id) {
            let mut m = m.clone();
            // Replace our optimistic copy (fluxer `replaceNonceMessage`).
            let pending = m.nonce.as_ref().and_then(|n| {
                w.messages.iter().position(|p| p.id.to_string() == *n && p.author.id == m.author.id && p.id != m.id)
            });
            if let Some(i) = pending {
                let old = w.messages.remove(i);
                if m.reactions.is_empty() {
                    m.reactions = old.reactions;
                }
                merge(&mut w.messages, vec![m]);
            } else if !w.has_more_after {
                merge(&mut w.messages, vec![m]);
                trim_oldest(w);
            }
        }

        // Read state (fluxer `handleIncomingMessage`).
        let me = self.my_id();
        if m.author.id == me {
            let rs = self.read_state(m.channel_id);
            rs.last_message_id = rs.last_message_id.max(Some(m.id));
            rs.mention_count = 0;
            return;
        }
        if self.is_blocked(m.author.id) || covered {
            return;
        }
        let acked = self.read_states.get(&m.channel_id).and_then(|r| r.last_message_id).is_some_and(|a| a >= m.id);
        if !acked && self.mentions_me(m) {
            self.read_state(m.channel_id).mention_count += 1;
        }
    }

    /// Fluxer `ReadStateEntry.shouldMentionFor` (author blocking is checked by the caller).
    fn mentions_me(&self, m: &Message) -> bool {
        let me = self.my_id();
        let Some(c) = self.channels.get(&m.channel_id) else { return false };
        let s = self.guild_settings.get(&c.guild_id);
        if m.mentions.iter().any(|u| u.id == me) {
            return true;
        }
        let non_member_thread = c.kind.is_thread() && c.member.is_none();
        if m.mention_everyone && !s.is_some_and(|s| s.suppress_everyone) && !non_member_thread {
            return true;
        }
        let my_roles = c.guild_id.and_then(|g| self.member(g, me)).map(|mm| &mm.roles);
        if !s.is_some_and(|s| s.suppress_roles) && my_roles.is_some_and(|r| m.mention_roles.iter().any(|x| r.contains(x))) {
            return true;
        }
        c.guild_id.is_none() && !self.is_muted(c)
    }

    fn message_update(&mut self, m: &Message) {
        self.message_side_effects(m);
        let Some(old) = self.message_mut(m.channel_id, m.id) else { return };
        let mut m = m.clone();
        // shortcut: typed payloads can't tell an omitted `reactions` from an empty one; keep the
        // old list (reaction changes arrive as their own events).
        if m.reactions.is_empty() {
            m.reactions = std::mem::take(&mut old.reactions);
        }
        *old = m;
    }

    fn remove_messages(&mut self, channel: Snowflake, ids: &[Snowflake]) {
        if let Some(w) = self.messages.get_mut(&channel) {
            w.messages.retain(|m| !ids.contains(&m.id));
            // Replies to them now show the original as deleted (fluxer `markReferenceDeleted`).
            for m in &mut w.messages {
                if m.message_reference.as_ref().and_then(|r| r.message_id).is_some_and(|r| ids.contains(&r)) {
                    m.referenced_message = Some(None);
                }
            }
        }
    }

    fn reaction(&mut self, r: &MessageReaction, add: bool) {
        if let (Some(g), Some(m)) = (r.guild_id, &r.member) {
            self.upsert_member(g, m);
        }
        let mine = r.user_id == self.my_id();
        let Some(m) = self.message_mut(r.channel_id, r.message_id) else { return };
        let i = m.reactions.iter().position(|x| same_emoji(&x.emoji, &r.emoji));
        match (i, add) {
            (Some(i), true) => {
                let x = &mut m.reactions[i];
                // Our own add already applied optimistically.
                if !(mine && x.me) {
                    x.count += 1;
                    x.me |= mine;
                }
            }
            (None, true) => m.reactions.push(reflux_model::Reaction { emoji: r.emoji.clone(), count: 1, me: mine }),
            (Some(i), false) => {
                let x = &mut m.reactions[i];
                if !(mine && !x.me) {
                    x.count = x.count.saturating_sub(1);
                    x.me &= !mine;
                }
                if x.count == 0 {
                    m.reactions.remove(i);
                }
            }
            (None, false) => {}
        }
    }

    fn member_list_update(&mut self, u: &reflux_model::GuildMemberListUpdate) {
        let mut rows_out = Vec::new();
        for op in &u.ops {
            let [start, end] = op.range.map(|x| x as usize);
            let mut rows = Vec::with_capacity(op.items.len());
            for item in &op.items {
                if let Some(m) = &item.member {
                    self.upsert_member(u.guild_id, m);
                    rows.push(Some(MemberRow::Member(m.user.id)));
                } else {
                    rows.push(item.group.clone().map(MemberRow::Group));
                }
            }
            rows_out.push((start, end, rows));
        }
        let l = self.member_lists.entry((u.guild_id, u.id.clone())).or_default();
        l.member_count = u.member_count;
        l.online_count = u.online_count;
        l.groups = u.groups.clone();
        for (start, end, rows) in rows_out {
            if l.rows.len() <= end {
                l.rows.resize(end + 1, None);
            }
            l.rows[start..=end].fill(None);
            for (i, row) in rows.into_iter().enumerate().take(end + 1 - start) {
                l.rows[start + i] = row;
            }
        }
        let total = l.groups.len() + l.groups.iter().map(|g| g.count as usize).sum::<usize>();
        l.rows.truncate(total);
    }
}

fn guild_name(g: &GuildState) -> &str {
    g.info.as_ref().map_or("", |i| i.name.as_str())
}

fn same_emoji(a: &ReactionEmoji, b: &ReactionEmoji) -> bool {
    match (a.id, b.id) {
        (Some(x), Some(y)) => x == y,
        (None, None) => a.name == b.name,
        _ => false,
    }
}

/// Merges `incoming` into an ID-ascending list, replacing duplicates.
fn merge(list: &mut Vec<Message>, incoming: Vec<Message>) {
    for m in incoming {
        match list.binary_search_by_key(&m.id, |x| x.id) {
            Ok(i) => list[i] = m,
            Err(i) => list.insert(i, m),
        }
    }
}

fn trim_oldest(w: &mut MessageWindow) {
    if w.messages.len() > MAX_WINDOW {
        w.messages.drain(..w.messages.len() - MAX_WINDOW);
        w.has_more_before = true;
    }
}

/// `YYYY-MM-DDTHH:MM:SS[.fff](Z|+00:00)` → Unix ms. Offsets other than UTC are ignored.
fn parse_iso_ms(s: &str) -> Option<u64> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, sec) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let ms = s.get(19..).filter(|t| t.starts_with('.')).map_or(0, |t| {
        let digits: String = t[1..].chars().take_while(char::is_ascii_digit).chain("000".chars()).take(3).collect();
        digits.parse().unwrap_or(0)
    });
    // Days from civil (Howard Hinnant).
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * ((mo + 9) % 12) + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    u64::try_from(((days * 24 + h) * 60 + mi) * 60_000 + sec * 1000 + ms).ok()
}

#[cfg(test)]
mod tests;
