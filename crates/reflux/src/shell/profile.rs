//! The user profile popout (`user/components/popouts/UserProfilePopout`, `profile/profile_card/*`,
//! `UserProfileShared` roles, `RoleList`).

use reflux_model::{RelationshipType, Snowflake};
use reflux_store::Store;
use rune::{Color, El, TextEdit, col, el, row, text, text_input};

use super::{Intent, ShellAccess, ShellState, avatar, friends::tag, presence, role_color};
use crate::theme::{Palette, font};
use crate::ui::overlay::{anchor, popout_shadow};
use crate::ui::{Button, ButtonSize, rgb};

/// `PROFILE_POPOUT_GEOMETRY`: 300 content + 2.5 border, 105 banner in a 140 header, an 80px
/// avatar at (10, 55).
const WIDTH: f32 = 300.0;
const BORDER: f32 = 2.5;

/// `getUserAccentColor`: the avatar colour, else `DEFAULT_ACCENT_COLOR` (#4641D9).
// shortcut: profile accent colours and banners need `GET /users/{id}/profile`; not fetched yet.
fn accent(store: &Store, user: Snowflake) -> Color {
    store.users.get(&user).and_then(|u| u.avatar_color).map_or(rgb(0x46, 0x41, 0xD9), role_color)
}

pub fn popout<S: 'static>(store: &Store, st: &ShellState, user: Snowflake, guild: Option<Snowflake>, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let (key, placement) = st.profile_anchor.clone()?;
    let me = store.my_id();
    let border = accent(store, user);
    let status = presence(store, user);
    // `.avatarButton`: an 80px avatar in a 6px background-primary ring.
    let ring = 6.0;
    let head = el()
        .w_full()
        .h(140.0)
        .child(el().w_full().h(105.0).corners([6.0 - BORDER, 6.0 - BORDER, 0.0, 0.0]).fill(border))
        .child(
            el().absolute(10.0, 55.0).padding(ring).radius(999.0).fill(p.background_primary).child(avatar(store, st, user, 80.0, Some(status), p.background_primary, p)),
        );

    let name = store.display_name(guild, user);
    let mut content = col()
        .w_full()
        .gap(8.0)
        .pt(8.0)
        .px(16.0)
        .child(
            col()
                .w_full()
                .child(text(name).font(font(20.0, 500.0).lh(24.0)).color(p.text_primary).truncate())
                .child(text(tag(store, user)).font(font(14.0, 400.0).lh(18.0)).color(p.text_tertiary).truncate()),
        );
    if let Some(t) = store.presences.get(&user).and_then(|pr| pr.custom_status.as_ref()?.text.clone()).filter(|t| !t.is_empty()) {
        content = content.child(text(t).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).wrap());
    }
    // `UserProfileRoles`: shown when the member has roles.
    if let (Some(g), Some(m)) = (guild.and_then(|g| store.guilds.get(&g)), guild.and_then(|g| store.member(g, user))) {
        let mut roles: Vec<_> = g.roles.iter().filter(|r| m.roles.contains(&r.id)).collect();
        roles.sort_by_key(|r| (std::cmp::Reverse(r.position), r.id));
        if !roles.is_empty() {
            let list = row().w_full().flex_wrap().gaps(4.0, 4.0).children(roles.into_iter().map(|r| {
                let dot = if r.color == 0 { p.text_tertiary } else { role_color(r.color) };
                row()
                    .h(24.0)
                    .max_w(268.0)
                    .padding(4.0)
                    .gap(4.0)
                    .center_y()
                    .radius(4.0)
                    .fill(p.background_primary)
                    .border(1.0, p.background_header_secondary)
                    .child(el().size(12.0, 12.0).radius(6.0).fill(dot))
                    .child(text(r.name.clone()).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary).truncate().max_w(200.0).mr(2.0))
            }));
            content = content.child(col().w_full().gap(4.0).child(section_title("Roles", p)).child(list));
        }
    }
    if user != me {
        let field = text_input(&st.note, font(12.0, 400.0).lh(16.0))
            .key("profile-note")
            .w_full()
            .color(p.text_chat)
            .placeholder("Click to add a note", p.text_tertiary)
            .caret(p.text_primary, p.background_modifier_selected)
            .label("Note")
            .on_edit(move |s: &mut S| -> &mut TextEdit { &mut acc(s).note })
            .on_submit(move |s: &mut S, _| {
                let st = acc(s);
                let note = st.note.text.trim().to_string();
                st.note.focused = false;
                st.intents.push(Intent::SetNote { user, note });
            });
        content = content.child(col().w_full().gap(4.0).child(section_title("Note", p)).child(field));
    }

    let footer = if user == me {
        Button::new("Edit profile").size(ButtonSize::Small).fit_container().key("profile-edit").on_press(move |s: &mut S| {
            let st = acc(s);
            st.popups.close();
            st.settings = Some(crate::settings::SettingsUi::new(Some("profile")));
        })
    } else {
        let label = if store.relationships.get(&user).is_some_and(|r| r.kind == RelationshipType::BLOCKED) { "Open DM" } else { "Message" };
        Button::new(label).size(ButtonSize::Small).fit_container().key("profile-message").on_press(move |s: &mut S| {
            let st = acc(s);
            st.popups.close();
            st.intents.push(Intent::OpenDm(user));
        })
    };
    let card = col()
        .w(WIDTH + 2.0 * BORDER)
        .gap(4.0)
        .pb(12.0)
        .radius(6.0)
        .fill(p.background_primary)
        .border(BORDER, border)
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .padding(BORDER)
        .pb(12.0 + BORDER)
        .key("profile-popout")
        .child(head)
        .child(content)
        .child(row().w_full().pt(16.0).px(12.0).child(footer.el(p)))
        .modal(true, move |s: &mut S| acc(s).popups.close());
    Some(anchor(card, &key, placement, 8.0))
}

fn section_title(t: &str, p: &Palette) -> El {
    text(t).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary)
}

