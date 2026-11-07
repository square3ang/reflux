//! Home's friends page (`direct_message/DMFriendsView`, `AddFriendView`, `AddFriendForm`,
//! `friends/FriendListItem`, `FriendsListSection`, `views/FriendsList`, `PendingFriendsView`).

use reflux_model::{Relationship, RelationshipType, Snowflake};
use reflux_store::Store;
use rune::{Color, El, ScrollSpec, Shadow, TextEdit, col, el, icon, row, text, text_input};

use super::{Intent, ShellAccess, ShellState, Target, avatar, pops, presence};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{Button, ButtonSize, Menu, MenuItem, Placement, Status, Tooltip, mention_badge, mix, over, rgba, scroller};
use crate::i18n::t;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FriendsTab {
    #[default]
    Online,
    All,
    Pending,
    Add,
}

/// A user's tag as Fluxer prints it: `name#0042`, or the bare name for `#0000`/unique names.
pub fn tag(store: &Store, user: Snowflake) -> String {
    let Some(u) = store.users.get(&user) else { return String::new() };
    match u.discriminator.as_str() {
        "" | "0" | "0000" => u.username.clone(),
        d => format!("{}#{d}", u.username),
    }
}

pub fn view<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let pending = store.relationships.values().filter(|r| r.kind == RelationshipType::INCOMING_REQUEST).count();
    let mut body = col().w_full().grow(1.0).min_h(0.0).gap(12.0);
    if st.friends_tab != FriendsTab::Add {
        body = body.child(row().w_full().px(16.0).pt(12.0).child(search(st, p, acc)));
    }
    body = body.child(match st.friends_tab {
        FriendsTab::Add => add_friend(st, p, acc),
        tab => lists(store, st, tab, p, acc, layer),
    });
    col().grow(1.0).min_w(0.0).h_full().fill(p.background_secondary_lighter).child(header(st.friends_tab, pending, p, acc)).child(body)
}

/// `ChannelHeader` with the title, a divider and the tab buttons.
fn header<S: 'static>(active: FriendsTab, pending: usize, p: &Palette, acc: ShellAccess<S>) -> El {
    let tab = |t: FriendsTab, label: &str, badge: usize| {
        let primary = t == FriendsTab::Add;
        let (bg, fg) = if primary {
            (Some(p.brand_primary), p.brand_primary_fill)
        } else if t == active {
            (Some(p.background_modifier_selected), p.text_primary)
        } else {
            (None, p.text_primary_muted)
        };
        let mut b = row()
            .px(12.0)
            .py(4.0)
            .gap(8.0)
            .center_y()
            .radius(6.0)
            .hover_group()
            .cursor(rune::Cursor::Pointer)
            .role("tab").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
            .checked(t == active)
            .label(label.to_string())
            .key(format!("friends-tab-{label}"))
            .on_press(move |s: &mut S| {
                let st = acc(s);
                st.friends_tab = t;
                st.add_friend_result = None;
            })
            .child(text(label).font(font(16.0, 600.0).lh(24.0)).color(fg).group_color(if bg.is_some() { fg } else { p.text_primary }));
        if let Some(m) = mention_badge(badge as u64, false, p) {
            b = b.child(m);
        }
        match bg {
            Some(c) => b.fill(c),
            None => b.hover(p.background_modifier_hover),
        }
    };
    let tabs = row()
        .gap(8.0)
        .padding(4.0)
        .center_y()
        .child(tab(FriendsTab::Online, t("Online"), 0))
        .child(tab(FriendsTab::All, t("All"), 0))
        .child(tab(FriendsTab::Pending, t("Pending"), pending))
        .child(tab(FriendsTab::Add, t("Add friend"), 0));
    row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::CHANNEL_HEADER_PADDING_INLINE)
        .gap(16.0)
        .center_y()
        .fill(p.background_secondary_lighter)
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(icon(&icons::USERS_THREE_FILL, 24.0).color(p.text_primary))
                .child(text(t("My friends")).font(font(16.0, 600.0).lh(24.0)).color(p.text_primary)),
        )
        .child(el().size(1.0, 24.0).fill(p.background_modifier_accent))
        .child(tabs)
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(over(p.user_area_divider_color, p.background_secondary_lighter)))
}

/// `<Input leftIcon>`: 44 tall, radius 8, a 20px bold magnifier.
fn search<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let placeholder = match st.friends_tab {
        FriendsTab::Online => t("Search online friends"),
        FriendsTab::Pending => t("Search pending requests"),
        _ => t("Search friends"),
    };
    let border = if st.friends_query.focused { p.background_modifier_accent_focus } else { p.background_modifier_accent };
    row()
        .w_full()
        .min_h(44.0)
        .pl(13.0)
        .pr(17.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .fill(p.form_surface_background)
        .border(1.0, over(border, p.form_surface_background))
        .cursor(rune::Cursor::Text)
        .child(icon(&icons::MAGNIFYING_GLASS_BOLD, 20.0).color(p.text_secondary))
        .child(
            text_input(&st.friends_query, font(14.0, 400.0).lh(20.0))
                .single_line()
                .key("friends-search")
                .grow(1.0)
                .shrink()
                .color(p.text_primary)
                .placeholder(placeholder, p.text_tertiary)
                .caret(p.text_primary, p.background_modifier_selected)
                .label(placeholder)
                .on_edit(move |s: &mut S| -> &mut TextEdit { &mut acc(s).friends_query }),
        )
}

/// Online / All / Pending: sections of rows, or the empty state.
fn lists<S: 'static>(store: &Store, st: &ShellState, tab: FriendsTab, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let q = st.friends_query.text.trim().to_lowercase();
    let matches = |r: &&Relationship| {
        q.is_empty() || store.display_name(None, r.id).to_lowercase().contains(&q) || tag(store, r.id).to_lowercase().contains(&q)
    };
    let mut rels: Vec<&Relationship> = store.relationships.values().filter(matches).collect();
    rels.sort_by_cached_key(|r| (store.display_name(None, r.id).to_lowercase(), r.id));
    let of = |k: RelationshipType| rels.iter().copied().filter(|r| r.kind == k).collect::<Vec<_>>();
    let sections: Vec<(&str, Vec<&Relationship>)> = match tab {
        FriendsTab::Online => {
            let on = of(RelationshipType::FRIEND).into_iter().filter(|r| presence(store, r.id) != Status::Offline).collect();
            vec![("Online", on)]
        }
        FriendsTab::All => vec![(t("All friends"), of(RelationshipType::FRIEND))],
        _ => vec![(t("Incoming friend requests"), of(RelationshipType::INCOMING_REQUEST)), ("Outgoing friend requests", of(RelationshipType::OUTGOING_REQUEST))],
    };
    let any = sections.iter().any(|s| !s.1.is_empty());
    if !any {
        let (title, sub) = match (tab, q.is_empty()) {
            (FriendsTab::Pending, true) => (t("No pending requests"), "Pending friend requests show up here."),
            (FriendsTab::Pending, false) => (t("No pending requests match that search"), "Try a different name."),
            (_, false) => (t("No friends match that search"), "Try another name or check your spelling."),
            (FriendsTab::Online, true) => (t("Your friends are stuck in another timeline"), "When they come online, they'll appear right here."),
            _ => (t("This friends list needs more power"), t("Where we're going, we need more friends.")),
        };
        // `EmptyStateView`: a 64px UsersThree (fill, tertiary), the 20/600 title, the subtitle.
        return col()
            .w_full()
            .grow(1.0)
            .center()
            .padding(32.0)
            .child(icon(&icons::USERS_THREE_FILL, 64.0).color(p.text_tertiary).mb(16.0))
            .child(text(title).font(font(20.0, 600.0).lh(28.0)).color(p.text_primary).mb(8.0))
            .child(text(sub).max_w(448.0).align(rune::Align::Center).font(font(16.0, 400.0).lh(24.0)).color(p.text_tertiary).wrap());
    }
    let mut c = col().w_full().padding(16.0);
    let n = sections.iter().filter(|s| !s.1.is_empty()).count();
    for (i, (title, rs)) in sections.into_iter().filter(|s| !s.1.is_empty()).enumerate() {
        c = c.child(text(format!("{title} — {}", rs.len())).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted).mb(16.0));
        let mut rows = col().w_full().mx(-16.0).mb(if i + 1 < n { 32.0 } else { 0.0 });
        for r in rs {
            rows = rows.child(friend_row(store, st, r, p, acc, layer));
        }
        c = c.child(rows);
    }
    let viewport = col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).key("friends-list").child(c);
    scroller(viewport, false, p)
}

/// `FriendListItem`: avatar, name (tag on hover), status line, and the round action buttons.
fn friend_row<S: 'static>(store: &Store, st: &ShellState, r: &Relationship, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let id = r.id;
    let status = presence(store, id);
    let friend = r.kind == RelationshipType::FRIEND;
    let sub = match r.kind {
        RelationshipType::INCOMING_REQUEST => t("Incoming friend request"),
        RelationshipType::OUTGOING_REQUEST => t("Friend request sent"),
        _ => match status {
            Status::Online => t("Online"),
            Status::Idle => t("Idle"),
            Status::Dnd => "Do Not Disturb",
            _ => t("Offline"),
        },
    };
    let custom = friend.then(|| store.presences.get(&id).and_then(|pr| pr.custom_status.as_ref()?.text.clone())).flatten().filter(|t| !t.is_empty());
    let sub_color = if friend && status != Status::Offline { p.text_primary_muted } else { p.text_tertiary };
    let details = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(1.0)
        .child(
            row()
                .min_w(0.0)
                .gap(6.0)
                .center_y()
                .child(text(store.display_name(None, id)).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).truncate().shrink())
                .child(text(tag(store, id)).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary).group_opacity(0.0, 1.0)),
        )
        .child(
            text(custom.unwrap_or_else(|| sub.into()))
                .font(font(11.0, 500.0).lh(16.0))
                .color(mix(sub_color, Color::TRANSPARENT, 85.0))
                .truncate()
                .mt(-1.0),
        );
    let note = store.notes.get(&id).cloned().unwrap_or_default();
    let info = row()
        .grow(1.0)
        .min_w(0.0)
        .gap(12.0)
        .center_y()
        .child(avatar(store, st, id, 36.0, Some(status), p.background_secondary_lighter, p).key(format!("friend-{id}")))
        .child(details);

    let hover = p.background_modifier_hover;
    let neutral = (mix(hover, p.text_primary_muted, 94.0), p.text_primary_muted, mix(hover, p.background_modifier_selected, 84.0), p.text_primary);
    let accept = (
        mix(hover, p.brand_primary, 86.0),
        mix(p.brand_primary_foreground, p.text_primary, 76.0),
        mix(p.background_modifier_selected, p.brand_primary, 78.0),
        mix(p.brand_primary_foreground, p.text_primary, 82.0),
    );
    let ignore = (mix(hover, p.text_primary_muted, 90.0), p.text_primary, mix(hover, p.background_modifier_selected, 80.0), p.text_primary);
    let cancel = (
        mix(hover, p.status_danger, 86.0),
        mix(p.status_danger, p.text_primary, 82.0),
        mix(p.background_modifier_selected, p.status_danger, 76.0),
        mix(p.status_danger, p.text_primary, 88.0),
    );
    type Look = (Color, Color, Color, Color);
    let action = |key: String, tip: &str, glyph: &'static rune::Icon, (bg, fg, hbg, hfg): Look, intent: Option<Intent>, layer: &mut Vec<El>| {
        let mut b = row()
            .size(36.0, 36.0)
            .center()
            .radius(dim::RADIUS_FULL)
            .fill(bg)
            .hover(hbg)
            .hover_group()
            .box_shadow(Shadow { x: 0.0, y: 1.0, blur: 2.0, spread: 0.0, color: rgba(0, 0, 0, 0.12), inset: false })
            .cursor(rune::Cursor::Pointer)
            .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
            .label(tip.to_string())
            .child(icon(glyph, 20.0).color(fg).group_color(hfg));
        b = match intent {
            Some(i) => b.on_press(move |s: &mut S| acc(s).intents.push(i.clone())),
            // More: the friend menu, anchored to this button.
            None => {
                let k = key.clone();
                b.on_press(move |s: &mut S| acc(s).popups.toggle(&k))
            }
        };
        Tooltip::new(&key, tip).el(b, &st.popups, pops(acc), p, layer)
    };
    let buttons = match r.kind {
        RelationshipType::FRIEND => vec![
            action(format!("friend-msg-{id}"), t("Send message"), &icons::CHAT_TEARDROP_FILL, neutral, Some(Intent::OpenDm(id)), layer),
            action(format!("friend-more-{id}"), t("More"), &icons::DOTS_THREE_VERTICAL_BOLD, neutral, None, layer),
        ],
        RelationshipType::INCOMING_REQUEST => vec![
            action(format!("friend-accept-{id}"), t("Accept"), &icons::CHECK_BOLD, accept, Some(Intent::AddFriend(id)), layer),
            action(format!("friend-ignore-{id}"), t("Ignore"), &icons::X_BOLD, ignore, Some(Intent::RemoveRelationship(id)), layer),
        ],
        RelationshipType::OUTGOING_REQUEST => {
            vec![action(format!("friend-cancel-{id}"), "Cancel", &icons::X_BOLD, cancel, Some(Intent::RemoveRelationship(id)), layer)]
        }
        _ => vec![],
    };
    let actions = row().gap(8.0).center_y().children(buttons);
    let more = format!("friend-more-{id}");
    if st.popups.is_open(&more) {
        // Start voice / video call (not with bots or while a call with them runs), then Remove.
        let bot = store.users.get(&id).is_some_and(|u| u.bot);
        let in_call = store.private_channels().into_iter().any(|c| c.kind == reflux_model::ChannelType::DM && c.recipients.iter().any(|r| r.id == id) && store.calls.contains_key(&c.id));
        let mut menu = Menu::new(more);
        if !bot && !in_call {
            menu = menu
                .item(MenuItem::new("call", t("Start voice call")).icon(&icons::PHONE).on_select(move |s: &mut S| acc(s).intents.push(Intent::DmCall(id, false))))
                .item(MenuItem::new("video", t("Start video call")).icon(&icons::VIDEO_CAMERA).on_select(move |s: &mut S| acc(s).intents.push(Intent::DmCall(id, true))))
                .separator();
        }
        menu
            .item(MenuItem::new("remove", t("Remove friend")).icon(&icons::USER_MINUS_FILL).danger().on_select(move |s: &mut S| acc(s).intents.push(Intent::RemoveRelationship(id))))
            .el(&st.popups, pops(acc), p, layer);
    }

    // `::before`: a top rule inset 8 / 6.4 that turns into the hover fill.
    let inset = |e: El| e.pin(Some(8.0), Some(0.0), Some(6.4), Some(0.0)).radius(6.0);
    row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .center_y()
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
        .key(format!("friend-row-{id}"))
        .on_press(move |s: &mut S| acc(s).open_profile(id, None, note.clone(), format!("friend-{id}"), Placement::RightStart))
        .on_secondary_press(move |s: &mut S, at, _| {
            acc(s).open_user_menu(Target::User { user: id, guild: None, dm: None }, [at.x, at.y], format!("friend-{id}"), Placement::RightStart)
        })
        .child(inset(el()).fill(hover).group_opacity(0.0, 1.0))
        .child(inset(el()).child(el().w_full().h(1.0).fill(p.background_modifier_accent)).group_opacity(1.0, 0.0))
        .child(info)
        .child(actions)
}

/// `AddFriendView`: a hero over a radial brand glow, and the username form.
fn add_friend<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let send = move |s: &mut S| {
        let st = acc(s);
        let t = st.add_friend.text.trim().to_string();
        match parse_tag(&t) {
            Some(_) => {
                st.intents.push(Intent::FriendRequest(t));
                st.add_friend_result = None;
            }
            None => st.add_friend_result = Some(Err(crate::i18n::t("No user found with that username.").into())),
        }
    };
    let empty = st.add_friend.text.trim().is_empty();
    let error = matches!(st.add_friend_result, Some(Err(_)));
    let border = if error {
        p.status_danger
    } else if st.add_friend.focused {
        p.background_modifier_accent_focus
    } else {
        p.background_modifier_accent
    };
    let field = row()
        .w_full()
        .min_h(44.0)
        .pl(17.0)
        .pr(6.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .fill(p.form_surface_background)
        .border(1.0, over(border, p.form_surface_background))
        .cursor(rune::Cursor::Text)
        .child(
            text_input(&st.add_friend, font(14.0, 400.0).lh(20.0))
                .single_line()
                .key("add-friend-input")
                .grow(1.0)
                .shrink()
                .color(p.text_primary)
                .placeholder("Fluxer#0000", p.text_tertiary)
                .caret(p.text_primary, p.background_modifier_selected)
                .label(t("Friend's username"))
                .on_edit(move |s: &mut S| -> &mut TextEdit {
                    let st = acc(s);
                    st.add_friend_result = None;
                    &mut st.add_friend
                })
                .on_submit(move |s: &mut S, _| send(s)),
        )
        .child(Button::new(t("Send request")).size(ButtonSize::Compact).disabled(empty).key("add-friend-send").on_press(send).el(p));
    let mut form = col().w_full().child(field);
    match &st.add_friend_result {
        Some(Err(e)) => form = form.child(text(e.clone()).font(font(14.0, 400.0).lh(20.0)).color(p.status_danger).mt(8.0).wrap()),
        Some(Ok(())) => form = form.child(text(t("Friend request sent")).font(font(14.0, 400.0).lh(20.0)).color(p.status_online).mt(8.0)),
        None => {}
    }
    let card = col()
        .w_full()
        .max_w(480.0)
        .padding(32.0)
        .items_center()
        .child(icon(&icons::USER_CIRCLE_PLUS_FILL, 64.0).color(if crate::ui::is_light(p) { p.brand_primary_foreground } else { p.brand_primary_light }).mb(16.0))
        .child(text(t("Add friend")).font(font(24.0, 700.0).lh(32.0)).color(p.text_primary).mb(8.0))
        .child(text(t("You can add friends with their username.")).font(font(15.0, 400.0).lh(22.0)).color(p.text_secondary).mb(24.0))
        .child(form);
    // `radial-gradient(ellipse at 50% 0%, brand 15% 0%, transparent 70%)`: Rune's radius is in
    // points, so size it from the laid-out box (the first frame falls back to the body).
    let [w, h] = rune::memo::layout_size("friends-add").unwrap_or([1280.0, 744.0]);
    let reach = 0.7 * ((w * 0.5).powi(2) + h * h).sqrt();
    col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(24.0)
        .pb(24.0 + 48.0)
        .center()
        .key("friends-add")
        .radial_gradient([0.5, 0.0], reach, [(0.0, mix(p.brand_primary, Color::TRANSPARENT, 15.0)), (1.0, Color::TRANSPARENT)])
        .child(card)
}

/// `AddFriendForm.parseInput`: `name#1234` → (name, 1234); a bare name gets `0000`.
pub fn parse_tag(input: &str) -> Option<(&str, &str)> {
    let (name, disc) = input.split_once('#').unwrap_or((input, "0000"));
    let disc = if disc.chars().all(|c| c == '0') && !disc.is_empty() && disc.len() <= 4 { "0000" } else { disc };
    (!name.is_empty() && disc.len() == 4 && disc.bytes().all(|b| b.is_ascii_digit())).then_some((name, disc))
}

#[cfg(test)]
mod tests {
    use super::parse_tag;

    #[test]
    fn tags_parse_like_the_add_friend_form() {
        assert_eq!(parse_tag("ada"), Some(("ada", "0000")));
        assert_eq!(parse_tag("ada#0042"), Some(("ada", "0042")));
        assert_eq!(parse_tag("ada#0"), Some(("ada", "0000")));
        assert_eq!(parse_tag("ada#12"), None);
        assert_eq!(parse_tag("#0042"), None);
    }
}
