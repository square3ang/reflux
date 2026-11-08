//! The people connected to a guild voice channel, under its row in the channel list
//! (`VoiceParticipantsList`, `VoiceParticipantItem`, `GroupedVoiceParticipant`,
//! `VoiceStateIcons`, `LiveBadge`, `StreamWatchHoverPopout`), the avatar stacks of collapsed
//! categories (`CollapsedCategoryVoiceParticipants`) and `VoiceChannelUserCount`.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use reflux_model::{Channel, Permissions, Snowflake, VoiceState};
use reflux_store::Store;
use rune::{Color, El, Pixels, col, el, icon, row, text};

use super::{Intent, ShellAccess, ShellState, Target, avatar};
use crate::i18n::{t, tf, tp};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{Placement, rgb, rgba};

/// `getAllVoiceStatesInChannel` grouped by user: each user's connections, by connection ID.
pub(super) fn connections(store: &Store, guild: Option<Snowflake>, channel: Snowflake) -> Vec<(Snowflake, Vec<&VoiceState>)> {
    let mut by: Vec<(Snowflake, Vec<&VoiceState>)> = Vec::new();
    for v in store.voice_connections.values().filter(|v| v.guild_id == guild && v.channel_id == Some(channel)) {
        let Some(u) = v.user_id else { continue };
        match by.iter_mut().find(|e| e.0 == u) {
            Some(e) => e.1.push(v),
            None => by.push((u, vec![v])),
        }
    }
    // A state without a connection ID only has its per-user entry.
    for ((g, u), v) in &store.voice_states {
        if *g == guild && v.channel_id == Some(channel) && !by.iter().any(|e| e.0 == *u) {
            by.push((*u, vec![v]));
        }
    }
    for e in &mut by {
        e.1.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
    }
    by.sort_by_key(|e| e.0);
    by
}

#[derive(Default)]
struct Snapshot {
    /// User -> (name when first seen, join order, the name resolved).
    entries: HashMap<Snowflake, (String, u64, bool)>,
    next: u64,
}

thread_local! {
    /// `createVoiceParticipantSortSnapshot` per list (a channel, or a collapsed category).
    static SNAPSHOTS: RefCell<HashMap<Snowflake, Snapshot>> = RefCell::default();
}

/// `sortVoiceParticipantItemsWithSnapshot`: by the name a user had when the list first saw
/// them, then by join order; a user who leaves drops out of the snapshot.
fn sort_users<T>(store: &Store, list: Snowflake, guild: Option<Snowflake>, mut items: Vec<T>, user: impl Fn(&T) -> Snowflake) -> Vec<T> {
    SNAPSHOTS.with_borrow_mut(|all| {
        let snap = all.entry(list).or_default();
        let present: HashSet<Snowflake> = items.iter().map(&user).collect();
        snap.entries.retain(|u, _| present.contains(u));
        for it in &items {
            let u = user(it);
            let resolved = store.users.contains_key(&u);
            match snap.entries.get_mut(&u) {
                Some(e) if !e.2 && resolved => *e = (store.display_name(guild, u), e.1, true),
                Some(_) => {}
                None => {
                    let name = if resolved { store.display_name(guild, u) } else { format!("~{u}") };
                    snap.entries.insert(u, (name, snap.next, resolved));
                    snap.next += 1;
                }
            }
        }
        items.sort_by(|a, b| {
            let (ua, ub) = (user(a), user(b));
            let (ea, eb) = (&snap.entries[&ua], &snap.entries[&ub]);
            collate(&ea.0, &eb.0).then(ea.1.cmp(&eb.1)).then_with(|| ua.to_string().cmp(&ub.to_string()))
        });
        items
    })
}

/// `DISPLAY_NAME_COLLATOR` (`numeric: true, sensitivity: 'base'`): digit runs compare as
/// numbers, letters case-insensitively.
// shortcut: no accent folding or ICU punctuation order, upgrade if names with accents sort wrong.
fn collate(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    let digits = |it: &mut std::iter::Peekable<std::str::Chars>| {
        let mut s = String::new();
        while let Some(c) = it.next_if(|c| c.is_ascii_digit()) {
            s.push(c);
        }
        s
    };
    loop {
        let o = match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let (m, n) = (digits(&mut x), digits(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                m.len().cmp(&n.len()).then_with(|| m.cmp(n))
            }
            (Some(c), Some(d)) => {
                x.next();
                y.next();
                c.to_lowercase().cmp(d.to_lowercase())
            }
        };
        if o != Ordering::Equal {
            return o;
        }
    }
}

/// `resolveVoiceParticipantDisplayState` with `isParticipantVoicePermissionMuted`.
#[derive(Clone, Copy, Default)]
struct Shown {
    speaking: bool,
    self_mute: bool,
    self_deaf: bool,
    guild_mute: bool,
    guild_deaf: bool,
    perm_mute: bool,
    video: bool,
    live: bool,
}

fn shown(store: &Store, st: &ShellState, channel: Snowflake, user: Snowflake, v: &VoiceState, local: bool) -> Shown {
    let perm_mute = v.suppress || (user == store.my_id() && !store.permissions(channel).contains(Permissions::SPEAK));
    let self_mute = (if local { st.voice.self_mute } else { v.self_mute }) || perm_mute;
    Shown {
        // shortcut: LiveKit speakers are tracked per user, so all of a user's connections speak; upgrade when speaking is per connection.
        speaking: st.voice.speaking.contains(&user) && !self_mute && !v.mute,
        self_mute,
        self_deaf: if local { st.voice.self_deaf } else { v.self_deaf },
        guild_mute: v.mute,
        guild_deaf: v.deaf,
        perm_mute,
        video: v.self_video || (local && st.voice.camera),
        live: v.self_stream || (local && st.voice.screen),
    }
}

fn green() -> Color {
    rgb(0x22, 0xc5, 0x5e)
}

/// A `size` avatar with `AvatarWithPresence`'s speaking outline (#22c55e, min(3px, 10%)).
fn face(store: &Store, st: &ShellState, user: Snowflake, size: f32, speaking: bool, p: &Palette) -> El {
    let mut f = el().size(size, size).child(avatar(store, st, user, size, None, p.background_secondary, p));
    if speaking {
        let w = (size * 0.1).min(3.0);
        f = f.child(el().absolute(-w, -w).size(size + 2.0 * w, size + 2.0 * w).radius(size / 2.0 + w).border(w, green()));
    }
    f
}

/// A `<Tooltip text>` over `trigger`, keyed `key`.
fn tip<S: 'static>(key: String, label: &str, trigger: El, p: &Palette, acc: ShellAccess<S>) -> El {
    super::badges::tip(key, super::badges::label_tip(label, p), 190.0, trigger, acc)
}

/// `VoiceStateIcons`: camera, mute, deafen (16px, 4 apart) and `LiveBadge`, each with its
/// tooltip; moderator and permission mutes are red.
fn state_icons<S: 'static>(s: Shown, me: bool, key: &str, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if !(s.video || s.live || s.self_mute || s.self_deaf || s.guild_mute || s.guild_deaf) {
        return None;
    }
    let red = rgb(239, 68, 68);
    let glyph = |i: &'static rune::Icon, c: Color| icon(i, 16.0).color(c);
    let mut r = row().gap(4.0).center_y();
    if s.video {
        r = r.child(tip(format!("{key}-video"), t("Camera on"), glyph(&icons::VIDEO_CAMERA_FILL, p.text_primary_muted), p, acc));
    }
    if s.guild_mute || s.perm_mute || s.self_mute {
        let label = if s.guild_mute {
            t("Muted by moderators")
        } else if s.perm_mute {
            if me { t("You can't speak in this channel") } else { t("This participant can't speak in this channel") }
        } else {
            t("Muted")
        };
        let c = if s.guild_mute || s.perm_mute { red } else { p.text_primary_muted };
        r = r.child(tip(format!("{key}-mute"), label, glyph(&icons::MICROPHONE_SLASH_FILL, c), p, acc));
    }
    if s.guild_deaf || s.self_deaf {
        let label = match (s.guild_deaf, me) {
            (true, true) => t("You were deafened by moderators"),
            (true, false) => t("This participant was deafened by moderators"),
            (false, true) => t("You are deafened"),
            (false, false) => t("This participant is deafened"),
        };
        r = r.child(tip(format!("{key}-deaf"), label, glyph(&icons::SPEAKER_SLASH_FILL, if s.guild_deaf { red } else { p.text_primary_muted }), p, acc));
    }
    if s.live {
        // `LiveBadge`: a red pill, 10/600 uppercase white, line-height 1.
        let badge = row().px(6.0).py(2.0).radius(999.0).fill(rgb(220, 38, 38)).child(text(t("Live").to_uppercase()).font(font(10.0, 600.0).lh(10.0)).color(Color::WHITE));
        r = r.child(tip(format!("{key}-live"), t("Screen sharing"), badge, p, acc));
    }
    Some(r)
}

/// `useStreamWatchDoubleClick` / `openAndWatchStream`: watch `user`'s stream, joining the
/// channel first if needed, and go to the channel.
fn watch(sh: &mut ShellState, guild: Option<Snowflake>, channel: Snowflake, user: Snowflake) {
    sh.popups.close();
    sh.stream_card = (None, None);
    if sh.voice.channel == Some((guild, channel)) {
        sh.voice.watching = Some(user);
        sh.intents.push(Intent::Watch(Some(user)));
        if sh.channel != Some(channel) {
            sh.select_channel(channel);
        }
    } else {
        sh.join_voice(guild, channel);
        sh.voice.watch_on_join = Some(user);
    }
}

/// The stream picture the hover card shows; `useStreamPreview` isn't fetched yet.
fn card_preview(_st: &ShellState, _user: Snowflake) -> Option<&Pixels> {
    None
}

/// `StreamWatchHoverPopout`: while the LIVE row keyed `key` (or this card) is hovered, the
/// list variant of `StreamWatchHoverCard` on the row's right, 16 away.
#[allow(clippy::too_many_arguments)]
fn stream_card<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, channel: Snowflake, user: Snowflake, key: &str, p: &Palette, acc: ShellAccess<S>) {
    let own = user == store.my_id() && st.voice.channel == Some((guild, channel));
    let watching = st.voice.watching == Some(user);
    let pending = st.voice.watch_on_join == Some(user);
    let can_watch = store.permissions(channel).contains(Permissions::CONNECT);
    let disabled = own || watching || pending || !can_watch;
    let label = if own {
        t("You're streaming!")
    } else if watching {
        t("Watching stream")
    } else {
        t("Watch stream")
    };
    let hover_label = if watching { t("Watching stream") } else { t("Watch stream") };
    // `.previewList`: 120 tall on voice-surface-2; the picture (contain, brightness 0.6) or the
    // fallback text, and the hover label over a voice-overlay scrim.
    let mut preview = el().w_full().h(120.0).fill(p.voice_surface_2).hover_group().cursor(rune::Cursor::Pointer).role("button").label(hover_label.to_string());
    preview = match card_preview(st, user) {
        Some(px) => preview.child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(p.voice_surface_0).child(el().w_full().h_full().pixels_fit(px, rune::SurfaceFit::Fit).opacity(0.6))),
        None => preview.child(
            row().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).padding(12.0).center().child(text(t("No preview yet")).font(font(13.6, 400.0)).color(p.text_primary_muted).align(rune::Align::Center).wrap()),
        ),
    };
    preview = preview.child(
        row()
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .center()
            .fill(p.voice_overlay)
            .group_opacity(0.0, 1.0)
            .child(text(hover_label).font(font(13.6, 600.0)).color(p.voice_text_strong)),
    );
    let focus_only = watching;
    preview = preview.on_press(move |s: &mut S| {
        let sh = acc(s);
        if focus_only {
            // `applyStreamWatchFocus`.
            sh.voice.focus = Some(user);
            sh.voice.stream_pinned = true;
        } else if !disabled {
            watch(sh, guild, channel, user);
        }
    });
    let button = crate::ui::button::Button::<S>::new(label)
        .icon(&icons::MONITOR_PLAY_FILL)
        .fit_container()
        .disabled(disabled)
        .submitting(pending)
        .on_press(move |s: &mut S| watch(acc(s), guild, channel, user))
        .el(p);
    // `.actionRow`: 12 / 13.6 padding on voice-surface-3, the protip under the button.
    let mut actions = col().w_full().px(13.6).py(12.0).fill(p.voice_surface_3).child(button);
    if !watching {
        actions = actions.child(
            text(t("Double-click someone to watch their stream."))
                .font(font(11.2, 400.0).lh(14.56))
                .color(p.text_primary_muted)
                .align(rune::Align::Center)
                .wrap()
                .w_full()
                .mt(6.4),
        );
    }
    let anchor = key.to_string();
    let card = col()
        .key("stream-watch-card")
        .w(240.0)
        .radius(dim::RADIUS_MD)
        .clip_rounded(dim::RADIUS_MD)
        .fill(p.background_secondary)
        // `--shadow-lg`.
        .box_shadow(rune::Shadow { x: 0.0, y: 4.0, blur: 8.0, spread: 0.0, color: rgba(0, 0, 0, 0.15), inset: false })
        .box_shadow(rune::Shadow { x: 0.0, y: 2.0, blur: 4.0, spread: 0.0, color: rgba(0, 0, 0, 0.1), inset: false })
        .child(preview)
        .child(actions)
        .hover_delays(Duration::ZERO, Duration::from_millis(150))
        .on_hover_change(move |s: &mut S, inside, _| {
            let c = &mut acc(s).stream_card.1;
            if inside {
                *c = Some(anchor.clone());
            } else if c.as_deref() == Some(anchor.as_str()) {
                *c = None;
            }
        });
    crate::ui::overlay::late(crate::ui::overlay::anchor(card, key, Placement::Right, 16.0));
}

/// What a participant row looks like and does.
struct Row<'a> {
    key: String,
    user: Snowflake,
    shown: Shown,
    /// Our own connection in this client (`isCurrentUserConnection`).
    local: bool,
    /// A connection under a grouped user (`isGroupedItem`).
    device: Option<&'a VoiceState>,
}

/// `VoiceParticipantItem` (a user with one connection, or one connection of a grouped user).
fn participant<S: 'static>(store: &Store, st: &ShellState, c: &Channel, r: Row, p: &Palette, acc: ShellAccess<S>) -> El {
    let (gid, cid, u, s) = (c.guild_id, c.id, r.user, r.shown);
    let me = u == store.my_id();
    let menu = st.popups.is_open(super::call::TILE_MENU) && st.target == Some(Target::User { user: u, guild: gid, dm: None });
    let profile = r.device.is_none() && st.popups.is_open("profile") && st.profile_anchor.as_ref().is_some_and(|(k, _)| *k == r.key);
    let card = s.live && (st.stream_card.0.as_deref() == Some(r.key.as_str()) || st.stream_card.1.as_deref() == Some(r.key.as_str()));
    let current = r.local && !s.speaking;
    // `.participantRow`: text-primary-muted, text-primary and the hover fill while speaking,
    // on our connection, with its profile, menu or stream card open, and on hover.
    let lit = s.speaking || current || profile || menu || card;
    let name_on = s.speaking || if r.device.is_some() { current } else { me };
    let fg = if lit || name_on { p.text_primary } else { p.text_primary_muted };
    let name = store.display_name(gid, u);
    let mut item = row().key(r.key.clone()).w_full().px(8.0).py(4.0).gap(6.0).center_y().radius(6.0).cursor(rune::Cursor::Pointer).hover_group();
    item = if lit { item.fill(p.background_modifier_hover) } else { item.hover(p.background_modifier_hover) };
    item = match r.device {
        Some(v) => {
            // `.deviceIcon`: 20px Desktop / DeviceMobile, green while speaking.
            let glyph = if v.is_mobile { &icons::DEVICE_MOBILE } else { &icons::DESKTOP };
            let ic = if s.speaking { green() } else { fg };
            let label = match &v.connection_id {
                Some(id) if st.voice.settings.show_connection_ids => id.clone(),
                _ if v.is_mobile => t("Mobile device").to_string(),
                _ => t("Desktop device").to_string(),
            };
            let name_el = text(label.clone()).font(font(14.0, 500.0).lh(20.0)).color(fg).group_color(p.text_primary).truncate();
            item.child(icon(glyph, 20.0).color(ic).group_color(if s.speaking { ic } else { p.text_primary }))
                .child(row().grow(1.0).shrink().min_w(0.0).child(tip(format!("{}-name", r.key), &label, name_el, p, acc)))
        }
        None => item
            .child(face(store, st, u, 24.0, s.speaking, p))
            .child(text(name.clone()).font(font(14.0, 500.0).lh(20.0)).color(fg).group_color(p.text_primary).truncate().grow(1.0).shrink()),
    };
    if let Some(icons_row) = state_icons(s, me, &r.key, p, acc) {
        item = item.child(icons_row);
    }
    if r.device.is_none() {
        // `PreloadableUserPopout position="right-start"`, a focusable button.
        let note = store.notes.get(&u).cloned().unwrap_or_default();
        let anchor = r.key.clone();
        item = item
            .role("button")
            .focusable(true)
            // shortcut: Rune rounds the ring from the row's radius, not `.channelItemFocusRing`'s 8px; upgrade when rings take a radius.
            .focus_ring(4.0, -2.0, p.focus_primary)
            .label(tf("Open profile for {displayName}", &[("displayName", &name)]))
            .on_press(move |s: &mut S| {
                let sh = acc(s);
                if sh.popups.is_open("profile") && sh.profile_anchor.as_ref().is_some_and(|(k, _)| *k == anchor) {
                    sh.popups.close();
                } else {
                    sh.open_profile(u, gid, note.clone(), anchor.clone(), Placement::RightStart);
                }
            });
    }
    if s.live {
        item = item.on_double_press(move |s: &mut S| watch(acc(s), gid, cid, u));
        item = live_hover(item, r.key.clone(), acc);
        if card {
            stream_card(store, st, gid, cid, u, &r.key, p, acc);
        }
    }
    super::call::ParticipantMenu::participant_menu(item, u, gid, acc)
}

/// `StreamWatchHoverPopout`'s `useHover` (200ms open, 150ms close) on a LIVE row.
fn live_hover<S: 'static>(item: El, key: String, acc: ShellAccess<S>) -> El {
    item.hover_delays(Duration::from_millis(200), Duration::from_millis(150)).on_hover_change(move |s: &mut S, inside, _| {
        let c = &mut acc(s).stream_card.0;
        if inside {
            *c = Some(key.clone());
        } else if c.as_deref() == Some(key.as_str()) {
            *c = None;
        }
    })
}

/// `GroupedVoiceParticipant`: one row for a user with several connections, a "(N)" toggle
/// after the name, and the connections under it while expanded.
#[allow(clippy::too_many_arguments)]
fn grouped<S: 'static>(store: &Store, st: &ShellState, c: &Channel, u: Snowflake, states: &[&VoiceState], local_conn: Option<&str>, p: &Palette, acc: ShellAccess<S>) -> El {
    let (gid, cid) = (c.guild_id, c.id);
    let me = u == store.my_id();
    let key = format!("vu-{cid}-{u}");
    let each: Vec<(Shown, bool)> = states
        .iter()
        .map(|v| {
            let local = me && local_conn.is_some() && v.connection_id.as_deref() == local_conn;
            (shown(store, st, cid, u, v, local), local)
        })
        .collect();
    let any = |f: fn(&Shown) -> bool| each.iter().any(|(s, _)| f(s));
    let all = |f: fn(&Shown) -> bool| each.iter().all(|(s, _)| f(s));
    let (guild_mute, guild_deaf, perm_mute) = (any(|s| s.guild_mute), any(|s| s.guild_deaf), any(|s| s.perm_mute));
    let agg = Shown {
        speaking: any(|s| s.speaking),
        self_mute: all(|s| s.self_mute) && !guild_mute && !perm_mute,
        self_deaf: all(|s| s.self_deaf) && !guild_deaf,
        guild_mute,
        guild_deaf,
        perm_mute,
        video: any(|s| s.video),
        live: any(|s| s.live),
    };
    let menu = st.popups.is_open(super::call::TILE_MENU) && st.target == Some(Target::User { user: u, guild: gid, dm: None });
    let card = agg.live && (st.stream_card.0.as_deref() == Some(key.as_str()) || st.stream_card.1.as_deref() == Some(key.as_str()));
    let lit = agg.speaking || menu || card;
    let fg = if lit || me { p.text_primary } else { p.text_primary_muted };
    let name = store.display_name(gid, u);
    let open = st.voice_expanded.contains(&u);
    let n = states.len() as i64;
    let toggle_label = if open { tp("{count, plural, one {Hide # device} other {Hide # devices}}", n, &[]) } else { tp("{count, plural, one {Show # device} other {Show # devices}}", n, &[]) };
    // `.deviceCountButton`: 12px text-link, underlined on hover.
    let toggle = row()
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .focusable(true)
        .focus_ring(4.0, -2.0, p.focus_primary)
        .label(toggle_label.clone())
        .on_press(move |s: &mut S| {
            let e = &mut acc(s).voice_expanded;
            if !e.remove(&u) {
                e.insert(u);
            }
        })
        .child(text(format!("({n})")).font(font(12.0, 400.0).lh(16.0)).color(p.text_link))
        .child(el().pin(Some(0.0), None, Some(0.0), Some(2.0)).h(1.0).fill(p.text_link).group_opacity(0.0, 1.0));
    let names = row()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(4.0)
        .center_y()
        .child(text(name.clone()).font(font(14.0, 400.0).lh(20.0)).color(fg).group_color(p.text_primary).truncate().shrink())
        .child(tip(format!("{key}-count"), &toggle_label, toggle, p, acc));
    let note = store.notes.get(&u).cloned().unwrap_or_default();
    let anchor = key.clone();
    let mut item = row()
        .key(key.clone())
        .w_full()
        .px(8.0)
        .py(4.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .cursor(rune::Cursor::Pointer)
        .hover_group()
        .role("button")
        .focusable(true)
        .focus_ring(4.0, -2.0, p.focus_primary)
        .label(tf("Open profile for {nickname}", &[("nickname", &name)]))
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            if sh.popups.is_open("profile") && sh.profile_anchor.as_ref().is_some_and(|(k, _)| *k == anchor) {
                sh.popups.close();
            } else {
                sh.open_profile(u, gid, note.clone(), anchor.clone(), Placement::RightStart);
            }
        })
        .child(face(store, st, u, 24.0, agg.speaking, p))
        .child(names);
    item = if lit { item.fill(p.background_modifier_hover) } else { item.hover(p.background_modifier_hover) };
    if let Some(icons_row) = state_icons(agg, me, &key, p, acc) {
        item = item.child(icons_row);
    }
    if agg.live {
        item = item.on_double_press(move |s: &mut S| watch(acc(s), gid, cid, u));
        item = live_hover(item, key.clone(), acc);
        if card {
            stream_card(store, st, gid, cid, u, &key, p, acc);
        }
    }
    let mut group = col().w_full().child(super::call::ParticipantMenu::participant_menu(item, u, gid, acc));
    if open {
        // `.devicesContainer`: 2 below, 16 in, 2 apart.
        group = group.child(col().w_full().mt(2.0).pl(16.0).gap(2.0).children(states.iter().zip(&each).map(|(v, (s, local))| {
            let conn = v.connection_id.clone().unwrap_or_default();
            participant(store, st, c, Row { key: format!("{key}-{conn}"), user: u, shown: *s, local: *local, device: Some(v) }, p, acc)
        })));
    }
    group
}

/// `VoiceParticipantsList`: the channel's people, 4 below its row and 24 in, 2 apart.
pub(super) fn list<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let (gid, cid) = (c.guild_id, c.id);
    let users = sort_users(store, cid, gid, connections(store, gid, cid), |e| e.0);
    if users.is_empty() {
        return None;
    }
    let me = store.my_id();
    // shortcut: the shell doesn't know this client's connection ID, our latest voice state's stands in; upgrade when `ShellState` mirrors it.
    let local_conn = (st.voice.channel == Some((gid, cid))).then(|| store.voice_states.get(&(gid, me))?.connection_id.as_deref()).flatten();
    Some(col().w_full().mt(4.0).pl(24.0).gap(2.0).children(users.into_iter().map(|(u, states)| {
        if states.len() > 1 {
            return grouped(store, st, c, u, &states, local_conn, p, acc);
        }
        let v = states[0];
        let local = u == me && st.voice.channel == Some((gid, cid)) && (local_conn.is_none() || v.connection_id.as_deref() == local_conn);
        participant(store, st, c, Row { key: format!("vu-{cid}-{u}"), user: u, shown: shown(store, st, cid, u, v, local), local, device: None }, p, acc)
    })))
}

/// `AvatarStack` of `StackUserAvatar`s (28px, at most 5): 18 apart (overlap -10), each in a
/// 1px background-tertiary ring, then "+N"; 1 above, 6 below.
fn stack<S: 'static>(store: &Store, st: &ShellState, list: &str, guild: Option<Snowflake>, users: &[(Snowflake, bool)], p: &Palette, acc: ShellAccess<S>) -> El {
    let ring = || el().pin(Some(-1.0), Some(-1.0), Some(-1.0), Some(-1.0)).radius(999.0).border(1.0, p.background_tertiary);
    let mut r = row().pt(1.0).pb(6.0).pr(1.0).center_y();
    for (i, &(u, speaking)) in users.iter().take(5).enumerate() {
        let key = format!("{list}-{u}");
        let name = store.display_name(guild, u);
        let note = store.notes.get(&u).cloned().unwrap_or_default();
        let anchor = key.clone();
        let button = face(store, st, u, 28.0, speaking, p)
            .child(ring())
            .radius(999.0)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .focusable(true)
            .focus_ring(4.0, -2.0, p.focus_primary)
            .label(tf("Open profile for {displayName}", &[("displayName", &name)]))
            .on_press(move |s: &mut S| acc(s).open_profile(u, guild, note.clone(), anchor.clone(), Placement::RightStart));
        r = r.child(tip(key, &name, button, p, acc).ml(if i == 0 { 0.0 } else { -10.0 }));
    }
    if users.len() > 5 {
        r = r.child(
            row()
                .key(format!("{list}-more"))
                .size(28.0, 28.0)
                .ml(-10.0)
                .center()
                .radius(999.0)
                .fill(p.background_tertiary)
                .border(1.0, p.background_tertiary)
                .child(text(format!("+{}", users.len() - 5)).font(font(10.0, 600.0).lh(12.0)).color(p.text_secondary)),
        );
    }
    r
}

/// `StackUserAvatar`'s `speaking`: any of the user's connections there speaks.
fn speaking(st: &ShellState, user: Snowflake, states: &[&VoiceState]) -> bool {
    st.voice.speaking.contains(&user) && states.iter().any(|v| !v.self_mute && !v.mute)
}

/// `CollapsedChannelAvatarStack`: the connected channel's people under its row in a collapsed
/// category (4 below, 24 in, 8 from the right, 4 / 8 padding).
pub(super) fn collapsed_channel<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> El {
    let users = sort_users(store, c.id, c.guild_id, connections(store, c.guild_id, c.id), |e| e.0);
    let users: Vec<(Snowflake, bool)> = users.iter().map(|(u, s)| (*u, speaking(st, *u, s))).collect();
    row().w_full().mt(4.0).pl(24.0).pr(8.0).child(row().grow(1.0).min_w(0.0).px(8.0).py(4.0).gap(6.0).center_y().radius(6.0).child(stack(store, st, &format!("stack-{}", c.id), c.guild_id, &users, p, acc)))
}

/// `CollapsedCategoryVoiceParticipants`: under a collapsed category we aren't connected in, a
/// 20px SpeakerHigh and everyone in its voice channels (once each).
pub(super) fn collapsed_category<S: 'static>(store: &Store, st: &ShellState, category: Snowflake, guild: Option<Snowflake>, voice: &[&Channel], p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let mut users: Vec<(Snowflake, bool)> = Vec::new();
    for c in voice {
        for (u, s) in connections(store, guild, c.id) {
            if !users.iter().any(|x| x.0 == u) {
                users.push((u, speaking(st, u, &s)));
            }
        }
    }
    if users.is_empty() {
        return None;
    }
    let users = sort_users(store, category, guild, users, |e| e.0);
    Some(
        row()
            .w_full()
            .px(8.0)
            .child(
                row()
                    .grow(1.0)
                    .min_w(0.0)
                    .px(8.0)
                    .py(4.0)
                    .gap(6.0)
                    .center_y()
                    .radius(6.0)
                    .child(icon(&icons::SPEAKER_HIGH, 20.0).color(p.text_primary_muted))
                    .child(stack(store, st, &format!("stack-{category}"), guild, &users, p, acc)),
            ),
    )
}

/// `VoiceChannelUserCount`: the count (28 wide, background-secondary) and the limit (20 wide,
/// the row's background-modifier-selected, after a 5px wedge) in a 16-tall pill with a 1px
/// `--theme-border` (transparent) and radius 4; 12/500, 14 line height, two-digit tabular numbers.
pub(super) fn user_count(count: usize, limit: u32, p: &Palette) -> El {
    let num = |n: usize| text(format!("{n:02}")).font(font(12.0, 500.0).lh(14.0).features("'tnum' 1")).color(p.text_tertiary);
    // `ChannelItemSurface` maps --background-modifier-selected to surface-interactive-selected-bg.
    let selected = p.surface_interactive_selected_bg;
    // `.total::after`: border-top 16 transparent over border-inline-end 5, a wedge rising right.
    let wedge = el().absolute(23.0, 0.0).size(5.0, 16.0).linear_gradient(
        (16.0f32).atan2(-5.0),
        [(0.0, Color::TRANSPARENT), (0.5, Color::TRANSPARENT), (0.5, selected), (1.0, selected)],
    );
    let inner = row()
        .pin(Some(1.0), Some(1.0), Some(1.0), Some(1.0))
        .clip_rounded(3.0)
        .child(row().w(28.0).h_full().center().child(num(count)))
        .child(row().w(20.0).h_full().pl(2.0).pr(4.0).center().fill(selected).child(num(limit as usize)))
        .child(wedge);
    el().size(50.0, 16.0).mr(-4.0).radius(4.0).fill(p.background_secondary).child(inner)
}

#[cfg(test)]
mod tests {
    use super::collate;
    use std::cmp::Ordering::*;

    #[test]
    fn collates_numbers_and_case() {
        assert_eq!(collate("user2", "User10"), Less);
        assert_eq!(collate("alice", "Bob"), Less);
        assert_eq!(collate("Bob", "bob"), Equal);
        assert_eq!(collate("a01", "a1"), Equal);
        assert_eq!(collate("ab", "abc"), Less);
    }
}
