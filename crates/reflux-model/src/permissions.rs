//! Permission bits and computation (`http-api/permissions.mdx`, `http-api/threads.mdx`).

use crate::{OverwriteType, PermissionOverwrite, Role, Snowflake};

bitflags::bitflags! {
    /// 64-bit permission mask. Sent as a decimal string, accepted as a string or number.
    // Snowflake already parses/prints a u64 as a decimal string, so reuse it for serde.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    #[serde(from = "Snowflake", into = "Snowflake")]
    pub struct Permissions: u64 {
        const CREATE_INSTANT_INVITE = 1 << 0;
        const KICK_MEMBERS = 1 << 1;
        const BAN_MEMBERS = 1 << 2;
        const ADMINISTRATOR = 1 << 3;
        const MANAGE_CHANNELS = 1 << 4;
        const MANAGE_GUILD = 1 << 5;
        const ADD_REACTIONS = 1 << 6;
        const VIEW_AUDIT_LOG = 1 << 7;
        const PRIORITY_SPEAKER = 1 << 8;
        const STREAM = 1 << 9;
        const VIEW_CHANNEL = 1 << 10;
        const SEND_MESSAGES = 1 << 11;
        const SEND_TTS_MESSAGES = 1 << 12;
        const MANAGE_MESSAGES = 1 << 13;
        const EMBED_LINKS = 1 << 14;
        const ATTACH_FILES = 1 << 15;
        const READ_MESSAGE_HISTORY = 1 << 16;
        const MENTION_EVERYONE = 1 << 17;
        const USE_EXTERNAL_EMOJIS = 1 << 18;
        const CONNECT = 1 << 20;
        const SPEAK = 1 << 21;
        const MUTE_MEMBERS = 1 << 22;
        const DEAFEN_MEMBERS = 1 << 23;
        const MOVE_MEMBERS = 1 << 24;
        const USE_VAD = 1 << 25;
        const CHANGE_NICKNAME = 1 << 26;
        const MANAGE_NICKNAMES = 1 << 27;
        const MANAGE_ROLES = 1 << 28;
        const MANAGE_WEBHOOKS = 1 << 29;
        const MANAGE_EXPRESSIONS = 1 << 30;
        const MANAGE_THREADS = 1 << 34;
        const CREATE_PUBLIC_THREADS = 1 << 35;
        const CREATE_PRIVATE_THREADS = 1 << 36;
        const USE_EXTERNAL_STICKERS = 1 << 37;
        const SEND_MESSAGES_IN_THREADS = 1 << 38;
        const MODERATE_MEMBERS = 1 << 40;
        const CREATE_EXPRESSIONS = 1 << 43;
        const PIN_MESSAGES = 1 << 51;
        const BYPASS_SLOWMODE = 1 << 52;
        const UPDATE_RTC_REGION = 1 << 53;
        const VIEW_CHANNEL_MEMBERS = 1 << 54;
    }
}

impl From<Snowflake> for Permissions {
    fn from(v: Snowflake) -> Self {
        Self::from_bits_retain(v.0)
    }
}

impl From<Permissions> for Snowflake {
    fn from(v: Permissions) -> Self {
        Snowflake(v.bits())
    }
}

impl Permissions {
    /// Bits that need 2FA in a guild with the elevated MFA level (docs list; the official
    /// client's list has MANAGE_EXPRESSIONS instead of MANAGE_THREADS, so both are included).
    pub const ELEVATED: Self = Self::KICK_MEMBERS
        .union(Self::BAN_MEMBERS)
        .union(Self::ADMINISTRATOR)
        .union(Self::MANAGE_CHANNELS)
        .union(Self::MANAGE_GUILD)
        .union(Self::MANAGE_MESSAGES)
        .union(Self::MANAGE_ROLES)
        .union(Self::MANAGE_WEBHOOKS)
        .union(Self::MODERATE_MEMBERS)
        .union(Self::MANAGE_THREADS)
        .union(Self::MANAGE_EXPRESSIONS);

    /// What a visitor (non-member viewing a guild) can ever have, from the official client.
    pub const VISITOR_MASK: Self = Self::VIEW_CHANNEL.union(Self::READ_MESSAGE_HISTORY);
}

/// Inputs for [`compute_permissions`]. Borrowed straight from the cache.
pub struct PermissionInput<'a> {
    pub guild_id: Snowflake,
    pub owner_id: Snowflake,
    pub user_id: Snowflake,
    /// All guild roles (the @everyone role has the guild's ID).
    pub roles: &'a [Role],
    /// The member's role IDs, or `None` when the user isn't a member.
    pub member_roles: Option<&'a [Snowflake]>,
    /// Channel overwrites; empty for guild-level permissions.
    pub overwrites: &'a [PermissionOverwrite],
    /// The account is visiting the guild without membership.
    pub visiting: bool,
    /// Guild MFA level is ELEVATED and the user has no authenticator: strip [`Permissions::ELEVATED`].
    pub strip_elevated: bool,
}

/// Computes guild or channel permissions as `permissions.mdx` "Permission computation" describes:
/// owner → all; non-member → none; @everyone | member roles; ADMINISTRATOR → all;
/// then overwrites everyone → roles (combined) → member, deny before allow at each step.
///
/// A communication timeout does not change the mask (docs: check `communication_disabled_until`
/// separately before offering send/react/typing/etc).
pub fn compute_permissions(i: &PermissionInput) -> Permissions {
    let role = |id: Snowflake| i.roles.iter().find(|r| r.id == id).map(|r| r.permissions);
    let ow = |id: Snowflake, kind: OverwriteType| {
        i.overwrites.iter().find(|o| o.id == id && o.kind == kind)
    };
    let everyone = role(i.guild_id).unwrap_or_default();

    if i.visiting {
        let mut p = everyone - Permissions::ADMINISTRATOR;
        if let Some(o) = ow(i.guild_id, OverwriteType::ROLE) {
            p = (p - o.deny) | o.allow;
        }
        return p & Permissions::VISITOR_MASK;
    }

    let elevated = |p: Permissions| if i.strip_elevated { p - Permissions::ELEVATED } else { p };
    if i.user_id == i.owner_id {
        // Owner is exempt from the MFA requirement.
        return Permissions::all();
    }
    let Some(member_roles) = i.member_roles else {
        return Permissions::empty();
    };

    let mut p = member_roles.iter().filter_map(|&r| role(r)).fold(everyone, |a, b| a | b);
    if p.contains(Permissions::ADMINISTRATOR) {
        return elevated(Permissions::all());
    }
    if let Some(o) = ow(i.guild_id, OverwriteType::ROLE) {
        p = (p - o.deny) | o.allow;
    }
    let (mut allow, mut deny) = (Permissions::empty(), Permissions::empty());
    for o in member_roles.iter().filter_map(|&r| ow(r, OverwriteType::ROLE)) {
        allow |= o.allow;
        deny |= o.deny;
    }
    p = (p - deny) | allow;
    if let Some(o) = ow(i.user_id, OverwriteType::MEMBER) {
        p = (p - o.deny) | o.allow;
    }
    elevated(p)
}

/// Thread permissions: the parent's, with `SEND_MESSAGES_IN_THREADS` in place of `SEND_MESSAGES`.
pub fn thread_permissions(parent: Permissions) -> Permissions {
    let mut p = parent - Permissions::SEND_MESSAGES;
    p.set(Permissions::SEND_MESSAGES, parent.contains(Permissions::SEND_MESSAGES_IN_THREADS));
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use Permissions as P;

    const G: Snowflake = Snowflake(100);
    const MOD: Snowflake = Snowflake(101);
    const MUTED: Snowflake = Snowflake(102);
    const ME: Snowflake = Snowflake(7);

    fn role(id: Snowflake, p: P) -> Role {
        Role { id, permissions: p, ..Default::default() }
    }

    fn ow(id: Snowflake, kind: OverwriteType, allow: P, deny: P) -> PermissionOverwrite {
        PermissionOverwrite { id, kind, allow, deny }
    }

    fn compute(member_roles: Option<&[Snowflake]>, overwrites: &[PermissionOverwrite]) -> P {
        let roles = [
            role(G, P::VIEW_CHANNEL | P::SEND_MESSAGES),
            role(MOD, P::MANAGE_MESSAGES | P::KICK_MEMBERS),
            role(MUTED, P::empty()),
        ];
        compute_permissions(&PermissionInput {
            guild_id: G,
            owner_id: Snowflake(1),
            user_id: ME,
            roles: &roles,
            member_roles,
            overwrites,
            visiting: false,
            strip_elevated: false,
        })
    }

    #[test]
    fn serde_decimal_string() {
        let p: P = serde_json::from_str("\"1099511637006\"").unwrap();
        assert_eq!(p.bits(), 1099511637006);
        assert_eq!(serde_json::from_str::<P>("8").unwrap(), P::ADMINISTRATOR);
        assert_eq!(serde_json::to_string(&P::ADMINISTRATOR).unwrap(), "\"8\"");
    }

    #[test]
    fn base_and_roles() {
        assert_eq!(compute(None, &[]), P::empty());
        assert_eq!(compute(Some(&[]), &[]), P::VIEW_CHANNEL | P::SEND_MESSAGES);
        assert!(compute(Some(&[MOD]), &[]).contains(P::MANAGE_MESSAGES | P::SEND_MESSAGES));
    }

    #[test]
    fn overwrite_order() {
        // @everyone denies send, MOD role allows it back, MUTED role denies: allow beats deny across roles.
        let ows = [
            ow(G, OverwriteType::ROLE, P::empty(), P::SEND_MESSAGES),
            ow(MOD, OverwriteType::ROLE, P::SEND_MESSAGES, P::empty()),
            ow(MUTED, OverwriteType::ROLE, P::empty(), P::SEND_MESSAGES),
        ];
        assert!(!compute(Some(&[]), &ows).contains(P::SEND_MESSAGES));
        assert!(!compute(Some(&[MUTED]), &ows).contains(P::SEND_MESSAGES));
        assert!(compute(Some(&[MOD, MUTED]), &ows).contains(P::SEND_MESSAGES));
        // Member overwrite applies last.
        let mut ows = ows.to_vec();
        ows.push(ow(ME, OverwriteType::MEMBER, P::empty(), P::SEND_MESSAGES | P::VIEW_CHANNEL));
        assert_eq!(compute(Some(&[MOD]), &ows) & (P::SEND_MESSAGES | P::VIEW_CHANNEL), P::empty());
        // A role overwrite with a member's ID isn't a member overwrite.
        let ows = [ow(ME, OverwriteType::ROLE, P::empty(), P::VIEW_CHANNEL)];
        assert!(compute(Some(&[]), &ows).contains(P::VIEW_CHANNEL));
    }

    #[test]
    fn owner_and_admin() {
        let roles = [role(G, P::ADMINISTRATOR)];
        let deny_all = [ow(G, OverwriteType::ROLE, P::empty(), P::all())];
        let mut i = PermissionInput {
            guild_id: G,
            owner_id: ME,
            user_id: ME,
            roles: &roles,
            member_roles: Some(&[]),
            overwrites: &deny_all,
            visiting: false,
            strip_elevated: true,
        };
        assert_eq!(compute_permissions(&i), P::all());
        i.owner_id = Snowflake(1);
        // Admin ignores overwrites but loses elevated bits without 2FA.
        assert_eq!(compute_permissions(&i), P::all() - P::ELEVATED);
        i.strip_elevated = false;
        assert_eq!(compute_permissions(&i), P::all());
    }

    #[test]
    fn visitor_and_threads() {
        let roles = [role(G, P::VIEW_CHANNEL | P::SEND_MESSAGES | P::READ_MESSAGE_HISTORY)];
        let i = PermissionInput {
            guild_id: G,
            owner_id: Snowflake(1),
            user_id: ME,
            roles: &roles,
            member_roles: None,
            overwrites: &[],
            visiting: true,
            strip_elevated: false,
        };
        assert_eq!(compute_permissions(&i), P::VIEW_CHANNEL | P::READ_MESSAGE_HISTORY);

        assert!(!thread_permissions(P::SEND_MESSAGES).contains(P::SEND_MESSAGES));
        assert!(thread_permissions(P::SEND_MESSAGES_IN_THREADS).contains(P::SEND_MESSAGES));
    }
}
