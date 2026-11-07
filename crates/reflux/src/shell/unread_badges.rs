//! `UserGuildSettings.resolvedUnreadBadgesLevel` and `getChannelUnreadState`: with Advanced →
//! "Unread badge customization" on, a channel's, its category's or its community's
//! `unread_badges` level decides how its unread state shows in the channel list.

use reflux_model::{Channel, NotificationLevel};
use reflux_store::Store;

/// The first explicit level (not inherit) of the channel, its parent, then the community.
pub fn level(store: &Store, c: &Channel) -> Option<NotificationLevel> {
    if !crate::look::with(|l| l.unread_badge_customization) {
        return None;
    }
    let s = store.guild_settings.get(&c.guild_id)?;
    let explicit = |l: Option<NotificationLevel>| l.filter(|l| *l != NotificationLevel::INHERIT);
    let ov = |id| explicit(s.channel_overrides.as_ref()?.get(&id)?.unread_badges);
    ov(c.id).or_else(|| c.parent_id.and_then(ov)).or_else(|| explicit(s.unread_badges))
}

/// What the row shows: (unread indicator, mention count, highlighted name, dimmed indicator).
pub fn apply(level: Option<NotificationLevel>, unread: bool, mentions: u32) -> (bool, u32, bool, bool) {
    match level {
        Some(NotificationLevel::NO_MESSAGES) => (false, 0, false, false),
        Some(NotificationLevel::ONLY_MENTIONS) => (unread, mentions, mentions > 0, unread),
        _ => (unread, mentions, unread || mentions > 0, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_shape_the_unread_state() {
        assert_eq!(apply(None, true, 2), (true, 2, true, false));
        assert_eq!(apply(Some(NotificationLevel::NO_MESSAGES), true, 2), (false, 0, false, false));
        assert_eq!(apply(Some(NotificationLevel::ONLY_MENTIONS), true, 0), (true, 0, false, true), "a dimmed dot, no highlight");
        assert_eq!(apply(Some(NotificationLevel::ALL_MESSAGES), true, 0), (true, 0, true, false));
    }
}
