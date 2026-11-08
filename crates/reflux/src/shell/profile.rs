//! User profiles: the popout (`user/components/popouts/UserProfilePopout`,
//! `profile/profile_card/*`), the full `UserProfileModal` (overview, mutual communities and
//! friends) and the card the Profile settings tab previews. Banner, accent colour, bio and
//! badges come from `GET /users/{id}/profile` (`users.mdx`), fetched when a profile opens.

use reflux_model::{RelationshipType, Snowflake};
use reflux_store::Store;
use rune::{Color, El, Pixels, TextEdit, col, el, row, text, text_input};
use serde_json::Value;

use super::{Intent, ShellAccess, ShellState, avatar, friends::tag, presence, role_color};
use crate::i18n::t;
use crate::media::{self, Load};
use crate::theme::{Palette, font};
use crate::ui::overlay::{anchor, popout_shadow};
use crate::ui::{Button, ButtonSize, ButtonVariant, mix, rgb};

/// `PROFILE_POPOUT_GEOMETRY`: 300 content + 2.5 border, 105 banner in a 140 header, an 80px
/// avatar at (10, 55).
pub(crate) const WIDTH: f32 = 300.0;
const BORDER: f32 = 2.5;

/// What a profile card shows.
pub struct Card {
    pub user: Snowflake,
    pub name: String,
    pub tag: String,
    pub pronouns: Option<String>,
    pub bio: Option<String>,
    pub accent: Color,
    /// The banner picture (else the accent colour fills it).
    pub banner: Option<Pixels>,
    /// The avatar in its ring, `avatar_size` across.
    pub avatar: El,
    pub avatar_size: f32,
    pub custom_status: Option<String>,
    /// The custom status emoji, drawn before its text.
    pub status_emoji: Option<El>,
    /// `profile_badges` as the API sends them.
    pub badges: Vec<Value>,
    /// The badge row the caller built from `badges` ([`with_badges`]); inline under the names
    /// in the full profile, else over the header's corner.
    pub badge_row: Option<El>,
    pub badges_inline: bool,
    pub guild: Option<Snowflake>,
    /// The shared UTC offset in minutes (`timezone_offset`), shown as "Local time".
    pub local_time: Option<i32>,
}

/// The fetched profile, if it loaded.
pub fn fetched(st: &ShellState, user: Snowflake, guild: Option<Snowflake>) -> Option<&Value> {
    st.account.profiles.get(&(user, guild)).and_then(|r| r.as_ref().ok())
}

/// `getUserAccentColor`: the guild profile's, then the account's accent colour, the avatar
/// colour, else `DEFAULT_ACCENT_COLOR` (#4641D9).
fn accent(store: &Store, prof: Option<&Value>, user: Snowflake) -> Color {
    let pick = |v: Option<&Value>, k: &str| v.and_then(|v| v[k].as_u64()).map(|c| c as u32);
    prof.and_then(|v| pick(Some(&v["guild_member_profile"]), "accent_color").or(pick(Some(&v["user_profile"]), "accent_color")))
        .or_else(|| store.users.get(&user).and_then(|u| u.avatar_color))
        .map_or(rgb(0x46, 0x41, 0xD9), role_color)
}

/// A banner by hash (user banners share the `banners/{id}` media route with guild banners).
pub fn banner_px(st: &ShellState, owner: Snowflake, hash: &str) -> Option<Pixels> {
    match st.media.asset(|b, s| media::guild_banner_url(b, owner, hash, s)) {
        Load::Ready(px) => Some(px),
        _ => None,
    }
}

/// Builds the card for `user` from the store and the fetched profile.
pub fn card_for(store: &Store, st: &ShellState, user: Snowflake, guild: Option<Snowflake>, size: f32, p: &Palette) -> Card {
    let prof = fetched(st, user, guild);
    let gp = prof.map(|v| &v["guild_member_profile"]).filter(|v| v.is_object());
    let up = prof.map(|v| &v["user_profile"]).filter(|v| v.is_object());
    let field = |k: &str| gp.and_then(|v| v[k].as_str()).or(up.and_then(|v| v[k].as_str())).filter(|s| !s.is_empty()).map(str::to_string);
    let banner = match (gp.and_then(|v| v["banner"].as_str()), up.and_then(|v| v["banner"].as_str())) {
        (Some(h), _) => guild.and_then(|g| match st.media.asset(|b, s| media::member_banner_url(b, g, user, h, s)) {
            Load::Ready(px) => Some(px),
            _ => None,
        }),
        (None, Some(h)) => banner_px(st, user, h),
        _ => None,
    };
    let cs = store.presences.get(&user).and_then(|pr| pr.custom_status.as_ref());
    let badges = prof.and_then(|v| v["profile_badges"].as_array()).cloned().unwrap_or_default();
    Card {
        user,
        name: store.display_name(guild, user),
        tag: tag(store, user),
        pronouns: field("pronouns"),
        bio: field("bio"),
        accent: accent(store, prof, user),
        banner,
        avatar: avatar(store, st, user, size, Some(presence(store, user)), p.background_primary, p),
        avatar_size: size,
        custom_status: cs.and_then(|c| c.text.clone()).filter(|t| !t.is_empty()),
        status_emoji: cs.and_then(|c| super::forum::forum_emoji(st, c.emoji_id, c.emoji_name.as_deref(), 16.0)),
        badges,
        badge_row: None,
        badges_inline: false,
        guild,
        local_time: prof.and_then(|v| v["timezone_offset"].as_i64()).map(|m| m as i32),
    }
}

/// Builds `c`'s badge row (`UserProfileBadges`): the full profile shows 15 inline, the
/// popout 7 over the header. The pencil shows on the viewer's own badges.
pub fn with_badges<S: 'static>(store: &Store, st: &ShellState, mut c: Card, inline: bool, p: &Palette, acc: ShellAccess<S>) -> Card {
    let own = c.user == store.my_id() && !c.badges.is_empty();
    let limit = if inline { super::badges::MODAL_VISIBLE } else { super::badges::POPOUT_VISIBLE };
    c.badge_row = super::badges::profile_row(st, &format!("profile-{}-{inline}", c.user), &c.badges, limit, own, p, acc);
    c.badges_inline = inline;
    c
}

fn section_title(t: &str, p: &Palette) -> El {
    text(t.to_string()).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary)
}

/// The banner header, avatar, names, pronouns, status, badges and bio of a card `w` wide;
/// `extra` goes under the bio.
pub fn card_body(store: &Store, st: &ShellState, c: Card, w: f32, extra: Vec<El>, p: &Palette) -> El {
    let banner_h = (w * 105.0 / WIDTH).round();
    let banner = el().w_full().h(banner_h).corners([6.0 - BORDER, 6.0 - BORDER, 0.0, 0.0]);
    let banner = match &c.banner {
        Some(px) => banner.pixels_fit(px, rune::SurfaceFit::Fill),
        None => banner.fill(c.accent),
    };
    let head_h = banner_h - 50.0 + c.avatar_size + 12.0 - 7.0;
    let (corner, inline) = match c.badge_row {
        Some(r) if c.badges_inline => (None, Some(r)),
        // `.containerPopout`: its bottom edge 145px down the card, 10px in from the right.
        // A wrapped row grows downward from where it was placed, so a 145-tall column bottom-aligns it.
        Some(r) => (Some(col().pin(None, Some(0.0), Some(10.0), None).w(w - 112.0).h(145.0).justify_end().items_end().child(super::badges::popout_plate(r, w, p))), None),
        None => (None, None),
    };
    let head = el()
        .w_full()
        // The ringed avatar (6px ring) hangs 50px into the banner.
        .h(head_h)
        .child(banner)
        .child(el().absolute(10.0, banner_h - 50.0).padding(6.0).radius(999.0).fill(p.background_primary).child(c.avatar))
        .children(corner);
    let mut names = col()
        .w_full()
        .child(text(c.name).font(font(20.0, 500.0).lh(24.0)).color(p.text_primary).truncate())
        .child(text(c.tag).font(font(14.0, 400.0).lh(18.0)).color(p.text_tertiary).truncate());
    if let Some(pr) = c.pronouns {
        names = names.child(text(pr).font(font(14.0, 400.0).lh(18.0)).color(p.text_primary_muted).truncate());
    }
    let mut content = col().w_full().gap(8.0).pt(8.0).px(16.0).child(names.children(inline.map(|r| r.mt(4.0))));
    if c.custom_status.is_some() || c.status_emoji.is_some() {
        let mut r = row().w_full().gap(4.0).center_y().children(c.status_emoji);
        if let Some(s) = c.custom_status {
            r = r.child(text(s).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).wrap().shrink());
        }
        content = content.child(r);
    }
    if let Some(bio) = c.bio {
        let md = super::markdown::Md::new(store, &st.media, c.guild, p, format!("bio-{}", c.user));
        content = content.child(col().w_full().gap(4.0).child(section_title(t("About me"), p)).child(super::markdown::inline_text(&bio, &md, font(14.0, 400.0).lh(18.0), p.text_chat)));
    }
    if let Some(m) = c.local_time {
        content = content.child(super::local_time::info(m, p));
    }
    col().w_full().child(head).child(content.children(extra))
}

pub(crate) fn frame(body: El, accent: Color, w: f32, p: &Palette) -> El {
    col()
        .w(w + 2.0 * BORDER)
        .gap(4.0)
        .radius(6.0)
        .fill(p.background_primary)
        .border(BORDER, accent)
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .padding(BORDER)
        .pb(12.0 + BORDER)
        .child(body)
}

/// The Profile settings tab's preview: the popout card for the given values.
pub fn preview(store: &Store, st: &ShellState, c: Card, p: &Palette) -> El {
    let accent = c.accent;
    frame(card_body(store, st, c, WIDTH, vec![], p), accent, WIDTH, p)
}

pub fn popout<S: 'static>(store: &Store, st: &ShellState, user: Snowflake, guild: Option<Snowflake>, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let (key, placement) = st.profile_anchor.clone()?;
    // `showProfileSkeleton`: forced, or the profile is still loading.
    if st.prefs.dev.force_profile_skeletons || matches!(st.account.profiles.get(&(user, guild)), Some(Ok(Value::Null))) {
        let card = super::skeleton::profile_popout(accent(store, None, user), p).key("profile-popout").modal(true, move |s: &mut S| acc(s).popups.close());
        return Some(anchor(card, &key, placement, 8.0));
    }
    let me = store.my_id();
    let c = with_badges(store, st, card_for(store, st, user, guild, 80.0, p), false, p, acc);
    let border = c.accent;
    let mut extra = Vec::new();
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
            extra.push(col().w_full().gap(4.0).child(section_title(t("Roles"), p)).child(list));
        }
    }
    if let Some(v) = fetched(st, user, guild).filter(|_| user != me) {
        let (guilds, friends) = (v["mutual_guilds"].as_array().map_or(0, Vec::len), v["mutual_friends"].as_array().map_or(0, Vec::len));
        if guilds + friends > 0 {
            let open = move |tab: &'static str| {
                move |s: &mut S| {
                    let sh = acc(s);
                    sh.popups.close();
                    sh.account.profile_modal = Some((user, guild, tab));
                }
            };
            let link = |label: String, tab: &'static str| text(label).font(font(12.0, 500.0).lh(16.0)).color(p.text_link).cursor(rune::Cursor::Pointer).role("button").on_press(open(tab));
            let mut r = row().gap(12.0);
            if guilds > 0 {
                r = r.child(link(format!("{guilds} mutual communities"), "guilds"));
            }
            if friends > 0 {
                r = r.child(link(format!("{friends} mutual friends"), "friends"));
            }
            extra.push(r);
        }
    }
    // `UserNoteEditor`: streamer mode hides notes.
    if user != me && !store.mask_names {
        let field = text_input(&st.note, font(12.0, 400.0).lh(16.0))
            .key("profile-note")
            .w_full()
            .color(p.text_chat)
            .placeholder(t("Click to add a note"), p.text_tertiary)
            .caret(p.text_primary, p.background_modifier_selected)
            .label(t("Note"))
            .on_edit(move |s: &mut S| -> &mut TextEdit { &mut acc(s).note })
            .on_submit(move |s: &mut S, _| {
                let st = acc(s);
                let note = st.note.text.trim().to_string();
                st.note.focused = false;
                st.intents.push(Intent::SetNote { user, note });
            });
        extra.push(col().w_full().gap(4.0).child(section_title(t("Note"), p)).child(field));
    }
    let footer = if user == me {
        Button::new(t("Edit profile")).size(ButtonSize::Small).fit_container().key("profile-edit").on_press(move |s: &mut S| {
            let st = acc(s);
            st.popups.close();
            st.open_settings(Some("my_profile"));
        })
    } else {
        let label = if store.relationships.get(&user).is_some_and(|r| r.kind == RelationshipType::BLOCKED) { t("Open DM") } else { t("Message") };
        Button::new(label).size(ButtonSize::Small).fit_container().key("profile-message").on_press(move |s: &mut S| {
            let st = acc(s);
            st.popups.close();
            st.intents.push(Intent::OpenDm(user));
        })
    };
    // The avatar opens the full profile.
    let body = card_body(store, st, c, WIDTH, extra, p);
    let view_full = row()
        .w_full()
        .px(12.0)
        .pt(8.0)
        .child(Button::new(t("View full profile")).variant(ButtonVariant::Secondary).size(ButtonSize::Small).fit_container().key("profile-full").on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.popups.close();
            sh.account.profile_modal = Some((user, guild, "overview"));
        }).el(p));
    let card = frame(body, border, WIDTH, p)
        .key("profile-popout")
        .child(row().w_full().pt(16.0).px(12.0).child(footer.el(p)))
        .child(view_full)
        .modal(true, move |s: &mut S| acc(s).popups.close());
    Some(anchor(card, &key, placement, 8.0))
}

/// `UserProfileModal`: the card on the left, tabs (overview, mutual communities, mutual friends)
/// on the right.
pub fn modal<S: 'static>(store: &Store, st: &ShellState, user: Snowflake, guild: Option<Snowflake>, tab: &'static str, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).account.profile_modal = None;
    let prof = fetched(st, user, guild);
    let c = with_badges(store, st, card_for(store, st, user, guild, 120.0, p), true, p, acc);
    let accent = c.accent;
    let left = col().w(340.0).h_full().child(card_body(store, st, c, 340.0, vec![], p));
    let guilds = prof.and_then(|v| v["mutual_guilds"].as_array()).cloned().unwrap_or_default();
    let friends = prof.and_then(|v| v["mutual_friends"].as_array()).cloned().unwrap_or_default();
    let tabs: Vec<(&'static str, String)> = vec![
        ("overview", t("Overview").to_string()),
        ("guilds", crate::i18n::tf("Mutual communities ({count})", &[("count", &(guilds.len()).to_string())])),
        ("friends", crate::i18n::tf("Mutual friends ({count})", &[("count", &(friends.len()).to_string())])),
    ];
    let tab_bar = row().w_full().gap(16.0).children(tabs.into_iter().map(|(id, label)| {
        let on = id == tab;
        col()
            .gap(6.0)
            .cursor(rune::Cursor::Pointer)
            .role("tab")
            .checked(on)
            .on_press(move |s: &mut S| acc(s).account.profile_modal = Some((user, guild, id)))
            .child(text(label).font(font(14.0, 500.0).lh(20.0)).color(if on { p.text_primary } else { p.text_tertiary }))
            .child(el().w_full().h(2.0).fill(if on { accent } else { Color::TRANSPARENT }))
    }));
    let muted = |s: String| text(s).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap();
    let item = |icon: El, name: String, sub: Option<String>| {
        row()
            .w_full()
            .py(8.0)
            .gap(12.0)
            .center_y()
            .child(icon)
            .child(col().grow(1.0).min_w(0.0).child(text(name).font(font(15.0, 500.0).lh(20.0)).color(p.text_primary).truncate()).children(sub.map(|s| text(s).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary).truncate())))
    };
    let body = match tab {
        "guilds" if guilds.is_empty() => muted(t("No mutual communities found.").into()),
        "guilds" => col().w_full().children(guilds.iter().filter_map(|g| {
            let id: Snowflake = g["id"].as_str()?.parse().ok()?;
            let name = store.guilds.get(&id).and_then(|g| g.info.as_ref()).map(|i| i.name.clone()).unwrap_or_default();
            let nick = g["nick"].as_str().map(|n| format!("{}{n}", t("Nickname: ")));
            let ch = super::first_channel(store, id);
            Some(item(super::guilds::guild_face(store, st, id, 40.0, false, p), name, nick).cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| {
                let sh = acc(s);
                sh.account.profile_modal = None;
                sh.select_guild(Some(id), ch);
            }))
        })),
        "friends" if friends.is_empty() => muted(t("No mutual friends found.").into()),
        "friends" => col().w_full().children(friends.iter().filter_map(|f| {
            let id: Snowflake = f["id"].as_str()?.parse().ok()?;
            let name = f["global_name"].as_str().or(f["username"].as_str()).unwrap_or_default().to_string();
            Some(item(avatar(store, st, id, 40.0, Some(presence(store, id)), p.background_secondary, p), name, f["username"].as_str().map(str::to_string)))
        })),
        _ => {
            let mut c = col().w_full().gap(16.0);
            c = c.child(col().gap(4.0).child(section_title(t("Member since"), p)).child(muted(super::chat::format_full_date(user.timestamp_ms()))));
            let conns: Vec<String> = prof.and_then(|v| v["connected_accounts"].as_array()).map(|a| a.iter().filter_map(|c| c["name"].as_str().map(str::to_string)).collect()).unwrap_or_default();
            // `UserProfileConnections`: hidden in streamer mode.
            if !conns.is_empty() && !store.mask_names {
                c = c.child(col().gap(6.0).child(section_title(t("Connections"), p)).children(conns.into_iter().map(|n| {
                    row().gap(8.0).center_y().child(rune::icon(&crate::icons::GLOBE, 16.0).color(p.text_secondary)).child(text(n).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary))
                })));
            }
            if prof.is_some_and(|v| v["profile_limited"].as_bool() == Some(true)) {
                c = c.child(muted(t("This person's privacy settings hide the rest of their profile.").into()));
            }
            if st.account.profiles.get(&(user, guild)).is_some_and(Result::is_err) || st.prefs.dev.force_profile_data_warning {
                c = c.child(muted(t("Couldn't load this profile.").into()));
            }
            c
        }
    };
    let right = col().grow(1.0).min_w(0.0).h_full().padding(16.0).gap(16.0).child(tab_bar).child(el().w_full().h(1.0).fill(mix(p.background_modifier_accent, Color::TRANSPARENT, 100.0))).child(body);
    let card = row()
        .w(860.0)
        .h(560.0)
        .radius(8.0)
        .fill(p.background_primary)
        .border(BORDER, accent)
        .clip_rounded(8.0)
        .role("dialog")
        .label(t("User profile"))
        .child(left)
        .child(right)
        .modal(true, close);
    el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(Color::BLACK).opacity(0.85))
        .child(col().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().child(card))
}
