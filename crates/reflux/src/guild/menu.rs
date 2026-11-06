//! The guild header popout (`guild/components/popouts/GuildHeaderPopout.{tsx,module.css}`).

use reflux_model::{ChannelType, Permissions, Snowflake};
use reflux_store::Store;
use rune::{El, Icon, col, icon, row, text};
use serde_json::json;

use super::{Dialog, can};
use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, dim, font};
use crate::ui::{Checkbox, Placement, mix, over};
use crate::ui::overlay::{anchor, popout_shadow};

/// Popup id, and the key of the header row it hangs off.
pub const HEADER_MENU: &str = "guild-header";

/// `getInvitableChannelId`: the open channel if it's a text channel you can invite to, else the
/// first one.
pub fn invitable_channel(store: &Store, st: &ShellState, guild: Snowflake) -> Option<Snowflake> {
    let ok = |c: Snowflake| {
        store.channels.get(&c).is_some_and(|ch| ch.guild_id == Some(guild) && matches!(ch.kind, ChannelType::GUILD_TEXT | ChannelType::GUILD_ANNOUNCEMENT))
            && store.permissions(c).contains(Permissions::CREATE_INSTANT_INVITE | Permissions::VIEW_CHANNEL)
    };
    st.channel.filter(|&c| ok(c)).or_else(|| store.channel_groups(guild).iter().flat_map(|g| g.text.iter()).map(|c| c.id).find(|&c| ok(c)))
}

/// Opens the invite modal for `channel`, creating the default 7-day invite like Fluxer.
pub fn open_invite(st: &mut ShellState, channel: Snowflake) {
    st.open_dialog(Dialog::Invite(channel, false));
    create_invite(st, channel, 604_800, 0, false);
}

pub(super) fn create_invite(st: &mut ShellState, channel: Snowflake, max_age: u32, max_uses: u32, temporary: bool) {
    let body = json!({ "max_age": max_age, "max_uses": max_uses, "temporary": temporary });
    st.api(reqwest::Method::POST, format!("/channels/{channel}/invites"), Some(body), super::Then::Keep("invite".into()));
}

/// The first community settings tab you can open, if any.
pub fn first_settings_tab(store: &Store, guild: Snowflake) -> Option<&'static str> {
    super::settings::tabs(store, guild).next().map(|t| t.0)
}

pub fn header_menu<S: 'static>(store: &Store, st: &ShellState, g: Snowflake, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let owner = store.guilds.get(&g).and_then(|gs| gs.info.as_ref()).is_some_and(|i| i.owner_id == store.my_id());
    let channels = can(store, g, Permissions::MANAGE_CHANNELS);
    let mut items: Vec<El> = Vec::new();
    if let Some(ch) = invitable_channel(store, st, g) {
        items.push(item("Invite members", &icons::USER_PLUS, false, p).on_press(move |s: &mut S| open_invite(acc(s), ch)));
    }
    if let Some(tab) = first_settings_tab(store, g) {
        items.push(item("Community settings", &icons::GEAR, false, p).on_press(move |s: &mut S| acc(s).open_guild_settings(g, tab)));
    }
    if channels {
        items.push(item("Create channel", &icons::PLUS_CIRCLE, false, p).on_press(move |s: &mut S| {
            acc(s).open_dialog(Dialog::CreateChannel { guild: g, parent: None, category: false })
        }));
        items.push(item("Create category", &icons::FOLDER_PLUS, false, p).on_press(move |s: &mut S| {
            acc(s).open_dialog(Dialog::CreateChannel { guild: g, parent: None, category: true })
        }));
    }
    items.push(item("Notification settings", &icons::BELL, false, p).on_press(move |s: &mut S| acc(s).open_dialog(Dialog::Notifications(g))));
    items.push(item("Privacy settings", &icons::SHIELD, false, p).on_press(move |s: &mut S| acc(s).open_dialog(Dialog::Privacy(g))));
    // shortcut: Reflux's profile tab has no per-community profile editor yet; this opens the
    // account profile.
    items.push(item("Edit community profile", &icons::USER_CIRCLE, false, p).on_press(move |s: &mut S| {
        let sh = acc(s);
        sh.popups.close();
        sh.settings = Some(crate::settings::SettingsUi::new(Some("my_profile")));
    }));
    let hide = store.guild_settings.get(&Some(g)).is_some_and(|s| s.hide_muted_channels);
    let toggle_hide = move |s: &mut S| acc(s).intents.push(Intent::PatchGuildSettings { guild: Some(g), patch: json!({ "hide_muted_channels": !hide }) });
    items.push(
        item_shell("Hide muted channels", p)
            .role("menuitemcheckbox")
            .checked(hide)
            .on_press(toggle_hide)
            .child(row().size(20.0, 20.0).center().child(Checkbox::<S>::new(hide).size(18.75).el(p).inert())),
    );
    // shortcut: "Delete my messages" (bulk-delete-mine) isn't offered yet.
    if !owner {
        items.push(item("Leave community", &icons::SIGN_OUT, true, p).on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.popups.close();
            sh.confirm_leave = Some(g);
        }));
    }
    let bg = p.form_surface_background;
    let popup = col()
        .min_w(dim::LAYOUT_SIDEBAR_WIDTH - 32.0)
        .max_w(352.0)
        .padding(9.0)
        .gap(2.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .role("menu")
        .label("Community menu")
        .focus_group()
        .children(items)
        .modal(true, move |s: &mut S| acc(s).popups.close());
    layer.push(anchor(popup, HEADER_MENU, Placement::Bottom, 8.0));
}

/// `.itemButton`: 32 tall, padding 5 8 (+1 border), 13/500 title, gap 16, radius 6.
fn item_shell(title: &str, p: &Palette) -> El {
    row()
        .w_full()
        .min_h(32.0)
        .px(9.0)
        .py(6.0)
        .gap(16.0)
        .center_y()
        .justify_between()
        .radius(6.0)
        .cursor(rune::Cursor::Pointer)
        .focusable(true)
        .label(title.to_string())
        .hover(p.surface_interactive_hover_bg)
        .focus_highlight(p.surface_interactive_hover_bg)
        .child(text(title.to_string()).font(font(13.0, 500.0).lh(18.0)).color(p.text_primary).truncate().shrink())
}

fn item(title: &str, i: &'static Icon, danger: bool, p: &Palette) -> El {
    let fg = if danger { mix(p.status_danger, p.text_primary, 78.0) } else { p.text_primary };
    let r = row()
        .w_full()
        .min_h(32.0)
        .px(9.0)
        .py(6.0)
        .gap(16.0)
        .center_y()
        .justify_between()
        .radius(6.0)
        .cursor(rune::Cursor::Pointer)
        .role("menuitem")
        .focusable(true)
        .label(title.to_string())
        .child(text(title.to_string()).font(font(13.0, 500.0).lh(18.0)).color(fg).truncate().shrink())
        .child(icon(i, 20.0).color(fg));
    let hover = if danger { mix(p.status_danger, p.surface_interactive_hover_bg, 6.0) } else { p.surface_interactive_hover_bg };
    r.hover(hover).focus_highlight(hover)
}
