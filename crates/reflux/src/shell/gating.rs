//! Composer permission gating: `ChannelTextarea`'s `disabled`, `canAttachFilesInChannel`,
//! `AttachmentPermissionDeniedModal` and `AnnouncementFollowBarrier`, with Developer tools'
//! "Force no SEND_MESSAGES / ATTACH_FILES permission" overrides.

use reflux_model::{Channel, ChannelType, Permissions};
use reflux_store::Store;

use super::ShellState;
use crate::i18n::t;

/// `ChannelTextarea`: a DM only obeys the override; a community channel also needs
/// `SEND_MESSAGES` (a thread its own send right). Timeouts are the barrier's job.
pub fn can_send(store: &Store, st: &ShellState, c: &Channel) -> bool {
    let forced = st.prefs.dev.force_no_send_messages;
    if c.guild_id.is_none() {
        return !forced;
    }
    !forced && if c.kind.is_thread() { store.thread_rights(c).send } else { store.permissions(c.id).contains(Permissions::SEND_MESSAGES) }
}

/// `canAttachFilesInChannel`.
pub fn can_attach(store: &Store, st: &ShellState, c: &Channel) -> bool {
    !st.prefs.dev.force_no_attach_files && (c.guild_id.is_none() || store.permissions(c.id).contains(Permissions::ATTACH_FILES))
}

/// `GuildChannelView.renderComposer`: an announcement channel the member can't post in offers
/// Follow instead of a disabled composer (the real permission only, as in Fluxer).
pub fn follow_only(store: &Store, c: &Channel) -> bool {
    c.kind == ChannelType::GUILD_ANNOUNCEMENT && !store.permissions(c.id).contains(Permissions::SEND_MESSAGES)
}

/// `TextareaPlusMenu`'s hint on Upload file / Send voice message, `None` when allowed.
pub fn upload_hint(send: bool, attach: bool) -> Option<&'static str> {
    if !send {
        Some(t("You can't send messages in this channel."))
    } else if !attach {
        Some(t("You can't upload files in this channel."))
    } else {
        None
    }
}

/// `AttachmentPermissionDeniedModal`.
pub fn deny_attach(st: &mut ShellState, only_images: bool, can_send: bool) {
    let title = if only_images { t("Can't paste image here") } else { t("Can't upload files here") };
    let body = if !can_send {
        t("You can't send messages in this channel.")
    } else if only_images {
        t("Images pasted from your clipboard are sent as attachments, and you don't have permission to attach files in this channel.")
    } else {
        t("You can't attach files in this channel.")
    };
    st.notice = Some((title.to_string(), body.to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_and_denials_follow_fluxer() {
        assert_eq!(upload_hint(true, true), None);
        assert_eq!(upload_hint(false, false), Some("You can't send messages in this channel."), "send wins");
        assert_eq!(upload_hint(true, false), Some("You can't upload files in this channel."));
        let mut st = ShellState::default();
        let dm = Channel { kind: ChannelType::DM, ..Default::default() };
        let store = Store::default();
        assert!(can_send(&store, &st, &dm) && can_attach(&store, &st, &dm), "DMs need no permission");
        st.prefs.dev.force_no_send_messages = true;
        st.prefs.dev.force_no_attach_files = true;
        assert!(!can_send(&store, &st, &dm) && !can_attach(&store, &st, &dm), "the overrides apply to DMs too");
        deny_attach(&mut st, true, true);
        assert_eq!(st.notice.as_ref().map(|n| n.0.as_str()), Some("Can't paste image here"));
        deny_attach(&mut st, false, false);
        assert_eq!(st.notice.as_ref().map(|n| n.1.as_str()), Some("You can't send messages in this channel."));
    }
}
