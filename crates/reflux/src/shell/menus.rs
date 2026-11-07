//! Context menus for users, guilds and channels, the profile popout and the leave-guild
//! confirmation, drawn into the overlay layer (`ui/action_menu/{UserContextMenu,
//! GuildContextMenu,ChannelContextMenu}`, `items/{Guild,Channel}MenuData`).

use reflux_model::{ChannelType, RelationshipType, Snowflake};
use reflux_store::Store;
use rune::{El, Tx, row, text};

use super::{Intent, ShellAccess, ShellState, Target, pops, profile};
use crate::icons;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Menu, MenuItem, Modal};
use crate::i18n::t;

/// Everything the shell floats over itself for [`ShellState::target`].
pub fn layer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    match (st.target, st.popups.open.as_deref()) {
        (Some(t), Some("ctx-menu")) => {
            let menu = match t {
                Target::User { user, guild, dm } => user_menu(store, st, user, guild, dm, acc),
                Target::Guild(g) => guild_menu(store, st, g, acc),
                Target::Channel(c) => channel_menu(store, st, c, acc),
            };
            menu.el(&st.popups, pops(acc), p, layer);
        }
        (Some(Target::User { user, guild, .. }), Some("profile")) => layer.extend(profile::popout(store, st, user, guild, p, acc)),
        _ => {}
    }
    if let Some(g) = st.confirm_leave {
        layer.push(leave_modal(g, p, acc));
    }
}

/// Unread channels as (channel, last message) pairs to ack.
fn unread(store: &Store, channels: impl Iterator<Item = Snowflake>) -> Vec<(Snowflake, Snowflake)> {
    channels.filter(|&c| store.has_unread(c)).filter_map(|c| Some((c, store.channels.get(&c)?.last_message_id?))).collect()
}

fn mark_read<S: 'static>(acks: Vec<(Snowflake, Snowflake)>, acc: ShellAccess<S>) -> MenuItem<S> {
    let none = acks.is_empty();
    MenuItem::new("mark-read", t("Mark as read")).icon(&icons::EYE_FILL).disabled(none).on_select(move |s: &mut S| acc(s).intents.push(Intent::MarkRead(acks.clone())))
}

fn copy_id<S: 'static>(label: &str, id: Snowflake) -> MenuItem<S> {
    MenuItem::new(format!("copy-{label}"), label).icon(&icons::SNOWFLAKE).on_select_tx(move |_: &mut S, tx: &mut Tx| tx.copy(id.to_string()))
}

/// `UserContextMenu`: profile, mention, message, note, relationship and block, copy ID.
// shortcut: calls, invite-to-community and local mute aren't listed yet.
fn user_menu<S: 'static>(store: &Store, st: &ShellState, user: Snowflake, guild: Option<Snowflake>, dm: Option<Snowflake>, acc: ShellAccess<S>) -> Menu<S> {
    let me = user == store.my_id();
    let mut m = Menu::new("ctx-menu");
    if let Some(c) = dm {
        m = m.item(mark_read(unread(store, std::iter::once(c)), acc)).entry(crate::social::mute_entry(store, st, None, Some(c), "conversation", acc)).separator();
    }
    let note = store.notes.get(&user).cloned().unwrap_or_default();
    let profile = move |s: &mut S, focus_note: bool| {
        let st = acc(s);
        let anchor = st.profile_anchor.clone();
        st.popups.close();
        if let Some((key, side)) = anchor {
            st.open_profile(user, guild, note.clone(), key, side);
            st.note.focused = focus_note;
        }
    };
    let p2 = profile.clone();
    m = m.item(MenuItem::new("profile", t("View profile")).icon(&icons::USER_FILL).on_select(move |s: &mut S| p2(s, false)));
    if st.channel.is_some() && dm.is_none() {
        m = m.item(MenuItem::new("mention", t("Mention")).icon(&icons::AT_BOLD).on_select(move |s: &mut S| {
            let st = acc(s);
            st.composer = rune::TextEdit::new(format!("{}<@{user}> ", st.composer.text));
            st.composer.focused = true;
        }));
    }
    if !me && dm.is_none() {
        m = m.item(MenuItem::new("message", t("Message")).icon(&icons::CHAT_CIRCLE_FILL).on_select(move |s: &mut S| acc(s).intents.push(Intent::OpenDm(user))));
    }
    // shortcut: only your own nickname (CHANGE_NICKNAME); MANAGE_NICKNAMES on others isn't offered.
    if let Some(g) = guild.filter(|&g| me && crate::guild::can(store, g, reflux_model::Permissions::CHANGE_NICKNAME)) {
        m = m.item(MenuItem::new("nickname", t("Change nickname")).icon(&icons::PENCIL_SIMPLE).on_select(move |s: &mut S| super::status::open(acc(s), crate::settings::ShellModal::Nickname(g))));
    }
    if !me {
        m = m.separator().item(MenuItem::new("note", t("Add note")).icon(&icons::NOTE_PENCIL_FILL).on_select(move |s: &mut S| profile(s, true))).separator();
        let push = |i: Intent| move |s: &mut S| acc(s).intents.push(i.clone());
        let kind = store.relationships.get(&user).map(|r| r.kind);
        m = match kind {
            Some(RelationshipType::FRIEND) => {
                m.item(MenuItem::new("remove-friend", t("Remove friend")).icon(&icons::USER_MINUS_FILL).danger().on_select(push(Intent::RemoveRelationship(user))))
            }
            Some(RelationshipType::INCOMING_REQUEST) => m
                .item(MenuItem::new("accept", t("Accept friend request")).icon(&icons::CHECK_CIRCLE_FILL).on_select(push(Intent::AddFriend(user))))
                .item(MenuItem::new("ignore", t("Ignore friend request")).icon(&icons::X_FILL).on_select(push(Intent::RemoveRelationship(user)))),
            Some(RelationshipType::OUTGOING_REQUEST) => m.item(
                MenuItem::new("cancel", t("Cancel friend request")).icon(&icons::CLOCK_COUNTER_CLOCKWISE_FILL).on_select(push(Intent::RemoveRelationship(user))),
            ),
            Some(RelationshipType::BLOCKED) => m,
            _ => m.item(MenuItem::new("add-friend", t("Add friend")).icon(&icons::USER_PLUS_FILL).on_select(push(Intent::AddFriend(user)))),
        };
        m = if kind == Some(RelationshipType::BLOCKED) {
            m.item(MenuItem::new("unblock", t("Unblock")).icon(&icons::PROHIBIT_FILL).on_select(push(Intent::RemoveRelationship(user))))
        } else {
            m.item(MenuItem::new("block", t("Block")).icon(&icons::PROHIBIT_FILL).danger().on_select(push(Intent::Block(user))))
        };
    }
    for e in crate::social::group_items(store, st, user, acc) {
        m = m.entry(e);
    }
    if let Some(g) = guild {
        for e in crate::social::member_items(store, st, g, user, acc) {
            m = m.entry(e);
        }
    }
    if let Some(c) = dm {
        m = m.separator().item(MenuItem::new("close-dm", t("Close DM")).icon(&icons::X_BOLD).on_select(move |s: &mut S| close_dm(acc(s), c)));
    }
    if store.settings.developer_mode {
        m = m.separator().item(copy_id(t("Copy user ID"), user));
        if let Some(c) = dm {
            m = m.item(copy_id(t("Copy channel ID"), c));
        }
    }
    m
}

fn close_dm(st: &mut ShellState, c: Snowflake) {
    if st.channel == Some(c) {
        st.channel = None;
    }
    st.intents.push(Intent::CloseDm(c));
}

/// `GuildContextMenu` (`useGuildMenuData`).
// shortcut: privacy, the community profile, hide muted channels and delete-my-messages are only
// in the header menu.
fn guild_menu<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, acc: ShellAccess<S>) -> Menu<S> {
    let channels = store.channels.values().filter(|c| c.guild_id == Some(g) && store.can_view(c.id)).map(|c| c.id);
    let owner = store.guilds.get(&g).and_then(|gs| gs.info.as_ref()).is_some_and(|i| i.owner_id == store.my_id());
    let mut m = Menu::new("ctx-menu").item(mark_read(unread(store, channels), acc)).separator();
    if let Some(ch) = crate::guild::invitable_channel(store, st, g) {
        m = m.item(MenuItem::new("invite", t("Invite members")).icon(&icons::USER_PLUS_FILL).on_select(move |s: &mut S| crate::guild::open_invite(acc(s), ch))).separator();
    }
    m = m
        .entry(crate::social::mute_entry(store, st, Some(g), None, "community", acc))
        .item(MenuItem::new("notifications", t("Notification settings")).icon(&icons::BELL_FILL).on_select(move |s: &mut S| acc(s).open_dialog(crate::guild::Dialog::Notifications(g))))
        .separator();
    if let Some(tab) = crate::guild::first_settings_tab(store, g) {
        m = m.item(MenuItem::new("settings", t("Community settings")).icon(&icons::GEAR_FILL).on_select(move |s: &mut S| acc(s).open_guild_settings(g, tab)));
    }
    if crate::guild::can(store, g, reflux_model::Permissions::MANAGE_CHANNELS) {
        let create = move |category: bool| move |s: &mut S| acc(s).open_dialog(crate::guild::Dialog::CreateChannel { guild: g, parent: None, category });
        m = m
            .item(MenuItem::new("create-channel", t("Create channel")).icon(&icons::PLUS_CIRCLE_FILL).on_select(create(false)))
            .item(MenuItem::new("create-category", t("Create category")).icon(&icons::FOLDER_PLUS_FILL).on_select(create(true)));
    }
    m = m.separator().item(MenuItem::new("delete-mine", "Delete my messages").icon(&icons::TRASH_FILL).danger().on_select(move |s: &mut S| acc(s).nav.confirm_purge = Some(g)));
    if !owner {
        let r = crate::social::Modal::Report { target: "guild", guild: Some(g), channel: None, id: g };
        m = m.item(MenuItem::new("report-guild", "Report community").icon(&icons::FLAG_FILL).danger().on_select(move |s: &mut S| crate::social::open_report(acc(s), r.clone())));
        m = m.item(MenuItem::new("leave", t("Leave community")).icon(&icons::SIGN_OUT_FILL).danger().on_select(move |s: &mut S| acc(s).confirm_leave = Some(g)));
    }
    if store.settings.developer_mode {
        m = m.separator().item(copy_id(t("Copy community ID"), g));
    }
    m
}

/// `ChannelContextMenu` (`useChannelMenuData`): guild channels and group DMs.
fn channel_menu<S: 'static>(store: &Store, st: &ShellState, c: Snowflake, acc: ShellAccess<S>) -> Menu<S> {
    let Some(ch) = store.channels.get(&c) else { return Menu::new("ctx-menu") };
    let guild = ch.guild_id;
    let noun = if ch.kind == ChannelType::GROUP_DM { "conversation" } else if ch.kind == ChannelType::GUILD_CATEGORY { "category" } else { "channel" };
    let mut m = Menu::new("ctx-menu").item(mark_read(unread(store, std::iter::once(c)), acc)).separator().entry(crate::social::mute_entry(store, st, guild, Some(c), noun, acc));
    if let Some(g) = guild {
        let invitable = matches!(ch.kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT | ChannelType::GUILD_VOICE)
            && store.permissions(c).contains(reflux_model::Permissions::CREATE_INSTANT_INVITE);
        if invitable {
            m = m.separator().item(MenuItem::new("invite", t("Invite people")).icon(&icons::USER_PLUS_FILL).on_select(move |s: &mut S| crate::guild::open_invite(acc(s), c)));
        }
        let category = ch.kind == ChannelType::GUILD_CATEGORY;
        let manage = store.permissions(c).contains(reflux_model::Permissions::MANAGE_CHANNELS);
        if !category && manage {
            m = m.item(MenuItem::new("duplicate-channel", t("Duplicate channel")).icon(&icons::COPY_FILL).on_select(move |s: &mut S| acc(s).open_social(crate::social::Modal::Duplicate(c))));
        }
        if ch.kind == ChannelType::GUILD_ANNOUNCEMENT {
            m = m.item(MenuItem::new("follow-channel", t("Follow channel")).icon(&icons::BELL_RINGING_FILL).on_select(move |s: &mut S| acc(s).open_social(crate::social::Modal::Follow(c))));
        }
        if !category {
            let fav = crate::nav::favorites::is_favorite(st, c);
            m = m.item(
                MenuItem::new("favorite", if fav { "Remove from favorites" } else { "Add to favorites" })
                    .icon(&icons::STAR_FILL)
                    .on_select(move |s: &mut S| crate::nav::favorites::toggle(acc(s), g, c)),
            );
        }
        if category && manage {
            m = m.item(MenuItem::new("create-channel", t("Create channel")).icon(&icons::PLUS_CIRCLE_FILL).on_select(move |s: &mut S| {
                acc(s).open_dialog(crate::guild::Dialog::CreateChannel { guild: g, parent: Some(c), category: false })
            }));
        }
        if let Some(tab) = crate::guild::first_channel_tab(store, c) {
            let label = if category { t("Edit category") } else { t("Edit channel") };
            m = m.separator().item(MenuItem::new("edit-channel", label).icon(&icons::PENCIL_SIMPLE_FILL).on_select(move |s: &mut S| {
                acc(s).open_dialog(crate::guild::Dialog::Channel { channel: c, tab, target: None })
            }));
            if store.permissions(c).contains(reflux_model::Permissions::MANAGE_CHANNELS) {
                let label = if category { t("Delete category") } else { t("Delete channel") };
                m = m.item(MenuItem::new("delete-channel", label).icon(&icons::TRASH_FILL).danger().on_select(move |s: &mut S| {
                    let sh = acc(s);
                    sh.open_dialog(crate::guild::Dialog::Channel { channel: c, tab, target: None });
                    sh.form.confirm = Some(crate::guild::Confirm::DeleteChannel(c));
                }));
            }
        }
        let link = format!("{}/channels/{g}/{c}", st.web_base.trim_end_matches('/'));
        m = m.separator().item(MenuItem::new("copy-link", t("Copy channel link")).icon(&icons::LINK_BOLD).on_select_tx(move |_: &mut S, tx: &mut Tx| tx.copy(link.clone())));
    }
    if ch.kind == ChannelType::GROUP_DM {
        m = m
            .separator()
            .item(MenuItem::new("edit-group", t("Edit group")).icon(&icons::PENCIL_SIMPLE_FILL).on_select(move |s: &mut S| acc(s).open_social(crate::social::Modal::EditGroup(c))))
            .item(MenuItem::new("add-to-group", t("Add friends to group")).icon(&icons::USER_PLUS_FILL).on_select(move |s: &mut S| acc(s).open_social(crate::social::Modal::AddToGroup(c))));
        m = m.separator().item(MenuItem::new("leave-group", t("Leave group")).icon(&icons::SIGN_OUT_FILL).danger().on_select(move |s: &mut S| close_dm(acc(s), c)));
    }
    if store.settings.developer_mode {
        m = m.separator().item(copy_id(t("Copy channel ID"), c));
    }
    m
}

fn leave_modal<S: 'static>(g: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).confirm_leave = None;
    Modal::new(t("Leave community"), close)
        .body(text(t("Leave this community? You won't see its messages anymore.")).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap())
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("leave-cancel").on_press(close).el(p))
                .child(
                    Button::new(t("Leave community"))
                        .variant(ButtonVariant::Danger)
                        .key("leave-confirm")
                        .on_press(move |s: &mut S| {
                            let st = acc(s);
                            st.confirm_leave = None;
                            if st.guild == Some(g) {
                                (st.guild, st.channel) = (None, None);
                            }
                            st.intents.push(Intent::LeaveGuild(g));
                        })
                        .el(p),
                ),
        )
        .el(p)
}
