//! Calls: `VoiceCallView` (participant tiles in a grid or one focused, video and stream tiles,
//! speaking borders, mute icons, `VoiceControlBar`), the DM call strip (`CompactVoiceCallView`)
//! and its header buttons (`CallButtons`), `IncomingCallUI`, the voice dialogs
//! (`CameraPreviewModal`, `ScreenSharePickerModal`, `VoiceChannelFullModal`,
//! `VoiceConnectionConfirmModal`, `CallNotRingableModal`), `VoiceDetailsPopout` and the
//! participant menu (`VoiceParticipantMenuItems`: local mute, focus, watch, ring).

use reflux_model::{Channel, ChannelType, Permissions, Snowflake, VoiceState};
use reflux_store::Store;
use rune::{Color, El, Pixels, col, el, icon, row, text};

use super::voice::{Modal, Op};
use super::{Intent, ShellAccess, ShellState, avatar, pops};
use crate::i18n::t;
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::overlay::{anchor, popout_shadow};
use crate::ui::{Button, ButtonVariant, Menu, MenuItem, Placement, rgb, rgba};

// `VoiceCallView.module.css` / `VoiceControlBar.module.css` colours.
fn surface_0() -> Color {
    rgb(0x0d, 0x0d, 0x0f)
}
fn surface_1() -> Color {
    rgb(0x15, 0x15, 0x17)
}
fn surface_2() -> Color {
    rgb(0x1c, 0x1c, 0x1e)
}
fn surface_3() -> Color {
    rgb(0x23, 0x23, 0x26)
}
fn hud_border() -> Color {
    rgba(255, 255, 255, 0.1)
}
fn danger() -> Color {
    rgb(0xff, 0x45, 0x3a)
}
fn success() -> Color {
    rgb(0x30, 0xd1, 0x58)
}

pub const TILE_MENU: &str = "voice-tile";
pub const DETAILS: &str = "voice-details";

fn op<S: 'static>(acc: ShellAccess<S>, o: Op) -> impl Fn(&mut S) + 'static {
    move |s: &mut S| acc(s).intents.push(Intent::Voice(o.clone()))
}

/// One tile: a participant (camera or avatar) or their screen share.
#[derive(Clone, Copy, PartialEq)]
struct Tile {
    user: Snowflake,
    screen: bool,
}

/// Participants of a channel's call, ordered by user ID (`VoiceParticipantSortUtils`' fallback),
/// each followed by their screen share when they stream.
fn participants(store: &Store, guild: Option<Snowflake>, channel: Snowflake) -> Vec<(Snowflake, &VoiceState)> {
    let mut v: Vec<_> = store.voice_states.iter().filter(|((g, _), v)| *g == guild && v.channel_id == Some(channel)).map(|((_, u), v)| (*u, v)).collect();
    v.sort_by_key(|(u, _)| *u);
    v
}

fn tiles(people: &[(Snowflake, &VoiceState)]) -> Vec<Tile> {
    people.iter().flat_map(|(u, v)| std::iter::once(Tile { user: *u, screen: false }).chain(v.self_stream.then_some(Tile { user: *u, screen: true }))).collect()
}

/// The video a tile shows, if any.
fn video(st: &ShellState, me: Snowflake, t: Tile) -> Option<&Pixels> {
    let v = &st.voice;
    match (t.screen, t.user == me) {
        (true, _) => (v.watching == Some(t.user)).then_some(&v.stream),
        (false, true) => v.camera.then_some(&v.self_view),
        (false, false) => v.videos.get(&t.user),
    }
}

/// `VoiceParticipantTile`.
#[allow(clippy::too_many_arguments)]
fn tile<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, t: Tile, vs: &VoiceState, big: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let me = store.my_id();
    let speaking = !t.screen && st.voice.speaking.contains(&t.user);
    let name = store.display_name(guild, t.user);
    let mut body = el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).radius(dim::RADIUS_LG).clip_rounded(dim::RADIUS_LG);
    match video(st, me, t) {
        Some(px) => body = body.fill(surface_0()).pixels_fit(px, rune::SurfaceFit::Fit),
        None if t.screen => {
            // `watchStreamOverlay`: the stream isn't subscribed until asked for.
            let u = t.user;
            let watch = row()
                .h(32.0)
                .px(12.0)
                .gap(6.0)
                .center_y()
                .radius(8.0)
                .fill(rgb(0, 0, 0))
                .border(1.0, hud_border())
                .hover(rgb(0x11, 0x11, 0x11))
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(crate::i18n::t("Watch stream"))
                .child(icon(&icons::MONITOR_PLAY_FILL, 16.0).color(Color::WHITE))
                .child(text(crate::i18n::t("Watch stream")).font(font(13.0, 600.0).lh(15.6)).color(Color::WHITE))
                .on_press(move |s: &mut S| {
                    let sh = acc(s);
                    sh.voice.watching = Some(u);
                    sh.intents.push(Intent::Watch(Some(u)));
                });
            body = body.fill(rgba(0, 0, 0, 0.78)).child(row().w_full().h_full().center().child(watch));
        }
        None => {
            let size = if big { 96.0 } else { 64.0 };
            let face = el().size(size, size).child(avatar(store, st, t.user, size, None, surface_3(), p));
            body = body.fill(surface_3()).child(row().w_full().h_full().center().child(face));
        }
    }
    // `lkParticipantMetadata`: the name pill with the mute / deafen icons.
    let glyph = |i: &'static rune::Icon, c: Color| icon(i, 14.0).color(c);
    let mut pill = row()
        .px(6.0)
        .py(3.0)
        .gap(3.0)
        .center_y()
        .radius(9.0)
        .fill(rgb(0, 0, 0))
        .border(1.0, hud_border())
        .max_w(260.0)
        .min_w(0.0);
    if !t.screen {
        if vs.mute || vs.self_mute {
            pill = pill.child(glyph(&icons::MICROPHONE_SLASH_FILL, if vs.mute { danger() } else { rgba(255, 255, 255, 0.72) }));
        }
        if vs.deaf || vs.self_deaf {
            pill = pill.child(glyph(&icons::SPEAKER_SLASH_FILL, if vs.deaf { danger() } else { rgba(255, 255, 255, 0.72) }));
        }
        if st.voice.local_mutes.contains(&t.user) {
            pill = pill.child(glyph(&icons::SPEAKER_X_FILL, danger()));
        }
    } else {
        pill = pill.child(glyph(&icons::MONITOR_PLAY_FILL, Color::WHITE));
    }
    let label = if t.screen { crate::i18n::tf("{streamerDisplayName}'s screen", &[("streamerDisplayName", &name.to_string())]) } else { name.clone() };
    pill = pill.child(text(label).font(font(12.0, 500.0).lh(14.4)).color(Color::WHITE).truncate().shrink().ml(3.0));
    if st.voice.settings.show_connection_ids
        && let Some(c) = &vs.connection_id
    {
        pill = pill.child(text(format!(" · {c}")).font(font(12.0, 500.0).lh(14.4)).color(rgba(255, 255, 255, 0.72)).truncate().shrink());
    }
    let key = format!("tile-{}-{}", t.user, t.screen as u8);
    let mut outer = el()
        .key(key.clone())
        .w_full()
        .h_full()
        .radius(12.0)
        .fill(surface_2())
        .child(body)
        .child(el().pin(Some(6.0), None, None, Some(6.0)).child(pill))
        .role("group")
        .label(name.clone())
        .cursor(rune::Cursor::Pointer);
    if speaking {
        // `[data-speaking='true']::after`: a 3.5px status-online border inside the tile.
        outer = outer.child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).radius(dim::RADIUS_LG).border(3.5, p.status_online));
    }
    let focus = (st.voice.focus != Some(t.user)).then_some(t.user);
    let user = t.user;
    outer
        .on_press(move |s: &mut S| acc(s).voice.focus = focus)
        .on_secondary_press(move |s: &mut S, at, _| {
            let sh = acc(s);
            sh.popups.open_at(TILE_MENU, [at.x, at.y]);
            sh.target = Some(super::Target::User { user, guild, dm: None });
        })
}

/// A 16:9 slot that fits inside its cell.
fn slot(content: El) -> El {
    row().grow(1.0).min_w(0.0).h_full().center().child(el().aspect_fit(16.0 / 9.0).max_w(9999.0).max_h(9999.0).child(content))
}

/// The tile area: a near-square grid, or the focused tile over a strip of the rest.
fn layout<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, people: &[(Snowflake, &VoiceState)], p: &Palette, acc: ShellAccess<S>) -> El {
    let all = tiles(people);
    let state = |u: Snowflake| people.iter().find(|(x, _)| *x == u).map(|(_, v)| *v);
    let make = |t: Tile, big: bool| state(t.user).map(|vs| tile(store, st, guild, t, vs, big, p, acc)).unwrap_or_else(el);
    let focused = st.voice.focus.and_then(|u| all.iter().copied().find(|t| t.user == u && (t.screen || !all.iter().any(|x| x.user == u && x.screen))));
    if let Some(f) = focused {
        let rest: Vec<Tile> = all.iter().copied().filter(|t| *t != f).collect();
        let mut c = col().w_full().h_full().gap(8.0).padding(16.0).child(row().w_full().grow(1.0).min_h(0.0).child(slot(make(f, true))));
        if !rest.is_empty() {
            c = c.child(row().w_full().h(112.0).gap(8.0).justify_center().children(rest.into_iter().map(|t| el().w(199.0).h(112.0).child(make(t, false)))));
        }
        return c;
    }
    let n = all.len().max(1);
    let cols = (n as f32).sqrt().ceil() as usize;
    let mut grid = col().w_full().h_full().gap(8.0).padding(16.0);
    for chunk in all.chunks(cols) {
        grid = grid.child(row().w_full().grow(1.0).min_h(0.0).gap(8.0).justify_center().children(chunk.iter().map(|t| slot(make(*t, false)))));
    }
    grid
}

/// `VoiceControlBar`: 44px buttons on a black pill.
fn control_bar<S: 'static>(store: &Store, st: &ShellState, _p: &Palette, acc: ShellAccess<S>) -> El {
    let v = &st.voice;
    let button = |i: &'static rune::Icon, label: &str, bg: Color, hover: Color, fg: Color, on: Box<dyn Fn(&mut S)>| {
        row()
            .size(44.0, 44.0)
            .center()
            .radius(16.0)
            .fill(bg)
            .hover(hover)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label.to_string())
            .child(icon(i, 22.0).color(fg))
            .on_press(move |s: &mut S| on(s))
    };
    let neutral = (rgba(255, 255, 255, 0.05), rgba(255, 255, 255, 0.12), rgba(255, 255, 255, 0.9));
    let red = (rgba(255, 69, 58, 0.16), rgba(255, 69, 58, 0.24), danger());
    let success = (rgba(48, 209, 88, 0.16), rgba(48, 209, 88, 0.24), success());
    let pick = |on: bool, a: (Color, Color, Color), b: (Color, Color, Color)| if on { a } else { b };
    let mic = pick(v.self_mute, red, neutral);
    let deaf = pick(v.self_deaf, red, neutral);
    let cam = pick(v.camera, success, neutral);
    let scr = pick(v.screen, success, neutral);
    let can_video = super::voice::can(store, v, Permissions::STREAM) && crate::gst::available();
    let mut bar = row()
        .padding(6.0)
        .gap(4.0)
        .center_y()
        .radius(16.0 + 6.0 + 2.0)
        .fill(rgb(0, 0, 0))
        .border(1.0, hud_border())
        .child(button(if v.self_mute { &icons::MICROPHONE_SLASH_FILL } else { &icons::MICROPHONE_FILL }, if v.self_mute { t("Unmute") } else { t("Mute") }, mic.0, mic.1, mic.2, Box::new(move |s: &mut S| acc(s).toggle_mute())))
        .child(button(if v.self_deaf { &icons::SPEAKER_SLASH_FILL } else { &icons::SPEAKER_HIGH_FILL }, if v.self_deaf { t("Undeafen") } else { t("Deafen") }, deaf.0, deaf.1, deaf.2, Box::new(move |s: &mut S| acc(s).toggle_deaf())));
    if can_video {
        bar = bar.child(button(if v.camera { &icons::VIDEO_CAMERA_FILL } else { &icons::VIDEO_CAMERA_SLASH_FILL }, if v.camera { t("Turn off camera") } else { t("Turn on camera") }, cam.0, cam.1, cam.2, Box::new(op(acc, Op::ToggleCamera))));
    }
    bar = bar.child(button(&icons::MONITOR_ARROW_UP_FILL, if v.screen { "Stop sharing your screen" } else { t("Share your screen") }, scr.0, scr.1, scr.2, Box::new(move |s: &mut S| {
        let sh = acc(s);
        if sh.voice.screen { sh.intents.push(Intent::ToggleScreen) } else { sh.voice.modal = Some(Modal::ScreenShare) }
    })));
    bar.child(button(&icons::PHONE_X_FILL, t("Disconnect"), rgb(0xff, 0x3b, 0x30), danger(), Color::WHITE, Box::new(move |s: &mut S| acc(s).leave_voice())))
}

/// `VoiceCallView` for the connected channel, in place of the chat column.
pub fn view<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let (guild, channel) = st.voice.channel?;
    let people = participants(store, guild, channel);
    let name = store.channels.get(&channel).map(|c| super::chat::channel_title(store, c)).unwrap_or_default();
    let mut header = row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::SPACING_4)
        .gap(dim::SPACING_2)
        .center_y()
        .child(icon(&icons::SPEAKER_HIGH_FILL, 20.0).color(rgba(255, 255, 255, 0.72)))
        .child(text(name).font(font(16.0, 600.0).lh(20.0)).color(Color::WHITE).truncate().shrink());
    if st.voice.encrypted {
        header = header.child(e2ee_badge());
    }
    header = header.child(el().grow(1.0)).child(
        row()
            .size(32.0, 32.0)
            .center()
            .radius(8.0)
            .hover(rgba(255, 255, 255, 0.12))
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(t("Show chat"))
            .child(icon(&icons::CHAT_CIRCLE_TEXT_FILL, 20.0).color(rgba(255, 255, 255, 0.9)))
            .on_press(move |s: &mut S| acc(s).voice.call_view = false),
    );
    let body = if st.voice.connected {
        layout(store, st, guild, &people, p, acc)
    } else {
        row().w_full().h_full().center().child(text(if st.voice.reconnecting { t("Reconnecting…") } else { t("Connecting…") }).font(font(16.0, 500.0).lh(20.0)).color(rgba(255, 255, 255, 0.72)))
    };
    Some(
        col()
            .grow(1.0)
            .min_w(0.0)
            .h_full()
            .fill(surface_1())
            .child(header)
            .child(col().w_full().grow(1.0).min_h(0.0).child(body))
            .child(row().w_full().pb(16.0).pt(8.0).justify_center().child(control_bar(store, st, p, acc))),
    )
}

/// `VoiceE2EEIndicator`: a lock and "Encrypted".
pub fn e2ee_badge() -> El {
    row()
        .gap(4.0)
        .px(6.0)
        .py(2.0)
        .center_y()
        .radius(6.0)
        .fill(rgba(48, 209, 88, 0.16))
        .role("img")
        .label("Call media is encrypted")
        .child(icon(&icons::LOCK_SIMPLE_FILL, 12.0).color(success()))
        .child(text("Encrypted").font(font(11.0, 600.0).lh(13.0)).color(success()))
}

/// `CompactVoiceCallView` over a DM / group DM with an active call: tiles, then Join or the
/// control bar.
pub fn dm_strip<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if !matches!(c.kind, ChannelType::DM | ChannelType::GROUP_DM) {
        return None;
    }
    let call = store.calls.get(&c.id)?;
    let people = participants(store, None, c.id);
    let joined = st.voice.channel == Some((None, c.id));
    let me = store.my_id();
    let ringing: Vec<Snowflake> = call.ringing.iter().copied().filter(|u| *u != me).collect();
    let mut row_tiles = row().w_full().h(150.0).gap(8.0).justify_center();
    for (u, vs) in &people {
        row_tiles = row_tiles.child(el().w(240.0).h(135.0).child(tile(store, st, None, Tile { user: *u, screen: false }, vs, false, p, acc)));
    }
    // Recipients being rung show dimmed with a ring button state.
    for u in ringing.iter().filter(|u| !people.iter().any(|(x, _)| x == *u)) {
        row_tiles = row_tiles.child(
            col()
                .w(135.0)
                .h(135.0)
                .center()
                .gap(8.0)
                .radius(12.0)
                .fill(surface_3())
                .opacity(0.6)
                .child(avatar(store, st, *u, 64.0, None, surface_3(), p))
                .child(text("Ringing…").font(font(12.0, 500.0).lh(14.4)).color(rgba(255, 255, 255, 0.72))),
        );
    }
    let ch = c.id;
    let footer = if joined {
        control_bar(store, st, p, acc)
    } else {
        Button::new(t("Join call")).icon(&icons::PHONE_FILL).key("dm-join-call").on_press(op(acc, Op::AcceptCall(ch))).el(p)
    };
    Some(col().w_full().pt(16.0).pb(12.0).gap(12.0).items_center().fill(surface_1()).child(row_tiles).child(footer))
}

/// `CallButtons`: start a voice / video call from the DM header (join it when one runs).
pub fn header_action<S: 'static>(store: &Store, st: &ShellState, c: &Channel, video: bool, el_: El, acc: ShellAccess<S>) -> El {
    let ch = c.id;
    let active = store.calls.contains_key(&ch);
    let joined = st.voice.channel == Some((None, ch));
    if joined {
        return el_.opacity(0.5);
    }
    el_.on_press(move |s: &mut S| {
        let sh = acc(s);
        if !active && crate::look::with(|l| l.confirm_calls) {
            // shortcut: Shift-click doesn't skip the confirmation.
            sh.voice.modal = Some(Modal::StartCall { channel: ch, video });
        } else {
            start_call(sh, ch, active, video);
        }
    })
}

fn start_call(sh: &mut ShellState, ch: Snowflake, active: bool, video: bool) {
    let o = if active { Op::AcceptCall(ch) } else { Op::StartCall(ch) };
    sh.intents.push(Intent::Voice(o));
    if video {
        // The camera turns on once connected (`startCall` with video).
        sh.voice.modal = Some(Modal::CameraPreview);
        sh.intents.push(Intent::Voice(Op::LoadCameras));
        sh.intents.push(Intent::Voice(Op::Preview(true)));
    }
}

/// `IncomingCallUI`: the first call ringing us that we haven't answered or dismissed.
pub fn incoming<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let me = store.my_id();
    let call = store
        .calls
        .values()
        .filter(|c| c.ringing.contains(&me) && !st.voice.dismissed_calls.contains(&c.channel_id) && st.voice.channel != Some((None, c.channel_id)))
        .min_by_key(|c| c.channel_id)?;
    let ch = store.channels.get(&call.channel_id);
    let caller = call.voice_states.first().and_then(|v| v.user_id).or_else(|| ch.and_then(|c| c.recipients.iter().map(|u| u.id).find(|u| *u != me)))?;
    let name = match ch {
        Some(c) if c.kind == ChannelType::GROUP_DM => super::chat::channel_title(store, c),
        _ => store.display_name(None, caller),
    };
    let channel = call.channel_id;
    let secondary = rgba(235, 235, 245, 0.55);
    let circle = |i: &'static rune::Icon, bg: Color, hover: Color, label: &str, on: Box<dyn Fn(&mut S)>| {
        col()
            .gap(10.0)
            .items_center()
            .child(
                row()
                    .size(64.0, 64.0)
                    .center()
                    .radius(32.0)
                    .fill(bg)
                    .hover(hover)
                    .cursor(rune::Cursor::Pointer)
                    .role("button")
                    .label(label.to_string())
                    .child(icon(i, 28.0).color(Color::WHITE))
                    .on_press(move |s: &mut S| on(s)),
            )
            .child(text(label.to_string()).font(font(13.0, 500.0).lh(16.0)).color(rgba(235, 235, 245, 0.7)))
    };
    let card = col()
        .w(320.0)
        .pt(26.0)
        .px(24.0)
        .pb(22.0)
        .gap(16.0)
        .items_center()
        .radius(28.0)
        .linear_gradient(std::f32::consts::PI, [(0.0, rgb(0x2c, 0x2c, 0x2e)), (1.0, rgb(0x1c, 0x1c, 0x1e))])
        .border(1.0, rgba(255, 255, 255, 0.08))
        .box_shadow(rune::Shadow { x: 0.0, y: 24.0, blur: 60.0, spread: 0.0, color: rgba(0, 0, 0, 0.55), inset: false })
        .role("dialog")
        .label(t("Incoming call"))
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(icon(&icons::PHONE_INCOMING_FILL, 14.0).color(secondary))
                .child(text(t("Incoming call")).font(font(13.0, 500.0).lh(16.0)).color(secondary)),
        )
        .child(avatar(store, st, caller, 96.0, None, rgb(0x2c, 0x2c, 0x2e), p))
        .child(
            col()
                .w_full()
                .gap(4.0)
                .items_center()
                .child(text(name).font(font(22.0, 600.0).lh(26.4)).color(Color::WHITE).truncate())
                .child(text(t("Voice call")).font(font(14.0, 400.0).lh(18.0)).color(secondary)),
        )
        .child(
            row()
                .gap(56.0)
                .pt(8.0)
                .child(circle(&icons::PHONE_X_FILL, rgb(0xff, 0x3b, 0x30), rgb(0xff, 0x45, 0x3a), t("Decline"), Box::new(op(acc, Op::DeclineCall(channel)))))
                .child(circle(&icons::PHONE_FILL, rgb(0x34, 0xc7, 0x59), rgb(0x30, 0xd1, 0x58), t("Accept"), Box::new(op(acc, Op::AcceptCall(channel))))),
        )
        .child(
            text(t("Ignore"))
                .font(font(13.0, 500.0).lh(16.0))
                .color(rgba(235, 235, 245, 0.5))
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .on_press(move |s: &mut S| {
                    acc(s).voice.dismissed_calls.insert(channel);
                }),
        );
    Some(el().pin(Some(0.0), Some(24.0), Some(0.0), None).child(row().w_full().justify_center().child(card)))
}

// ---- Dialogs -----------------------------------------------------------------------------------

fn body_text(s: &str, p: &Palette) -> El {
    text(s.to_string()).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap()
}

/// The voice dialogs and the camera preview.
pub fn modals<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let Some(m) = &st.voice.modal else { return };
    let close = move |s: &mut S| {
        let sh = acc(s);
        if sh.voice.modal == Some(Modal::CameraPreview) {
            sh.intents.push(Intent::Voice(Op::Preview(false)));
        }
        sh.voice.modal = None;
    };
    let ok = |label: &str| Button::new(label.to_string()).key("voice-modal-ok").on_press(close).el(p);
    let modal = match m {
        Modal::ChannelFull => crate::ui::Modal::new("Channel full", close)
            .body(body_text("This voice channel has reached its user limit. Try again later.", p))
            .footer(ok(t("Okay"))),
        Modal::NotRingable => crate::ui::Modal::new("Can't start a call", close)
            .body(body_text("You can't call this person right now. They may not accept calls from you.", p))
            .footer(ok(t("Okay"))),
        Modal::Refused(msg) => crate::ui::Modal::new("Couldn't join voice", close).body(body_text(msg, p)).footer(ok(t("Okay"))),
        Modal::JoinConfirm { guild, channel } => {
            let (g, c) = (*guild, *channel);
            crate::ui::Modal::new("Already in voice", close)
                .body(body_text("You're connected to voice on another device. Joining here will move your voice connection to this device.", p))
                .footer(
                    row()
                        .gap(8.0)
                        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("voice-confirm-cancel").on_press(close).el(p))
                        .child(Button::new(t("Join")).key("voice-confirm-join").on_press(move |s: &mut S| {
                            let sh = acc(s);
                            sh.voice.modal = None;
                            sh.intents.push(Intent::Voice(Op::ConfirmJoin(g, c)));
                        }).el(p)),
                )
        }
        Modal::StartCall { channel, video } => {
            let (ch, video) = (*channel, *video);
            let title = t(if video { "Start video call" } else { "Start voice call" });
            crate::ui::Modal::new(title, close)
                .body(body_text(t(if video { "Start a video call?" } else { "Start a voice call?" }), p))
                .footer(
                    row()
                        .gap(8.0)
                        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("call-confirm-cancel").on_press(close).el(p))
                        .child(Button::new(title).key("call-confirm-start").on_press(move |s: &mut S| {
                            let sh = acc(s);
                            sh.voice.modal = None;
                            start_call(sh, ch, false, video);
                        }).el(p)),
                )
        }
        Modal::CameraPreview => camera_preview(st, p, acc, close, layer),
        Modal::ScreenShare => screen_picker(st, p, acc, close, layer),
    };
    let _ = store;
    layer.push(modal.el(p));
}

/// `CameraPreviewModal`: the self view, the camera picker, then "Turn on camera".
fn camera_preview<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, close: impl Fn(&mut S) + Copy + 'static, layer: &mut Vec<El>) -> crate::ui::Modal<S> {
    let v = &st.voice;
    let preview = el().w(400.0).h(225.0).radius(dim::RADIUS_LG).clip_rounded(dim::RADIUS_LG).fill(surface_0());
    let preview = if v.previewing { preview.pixels_fit(&v.self_view, rune::SurfaceFit::Fit) } else { preview.child(row().w_full().h_full().center().child(text(t("Camera preview")).font(font(14.0, 500.0).lh(18.0)).color(rgba(255, 255, 255, 0.72)))) };
    let names = v.cameras.clone().unwrap_or_default();
    let opts: Vec<(&str, &str)> = std::iter::once(("", "Default")).chain(names.iter().map(|n| (n.as_str(), n.as_str()))).collect();
    let pick = crate::ui::Select::new("camera-device", &opts, Some(v.settings.camera.as_deref().unwrap_or("")))
        .label(t("Camera"))
        .width(400.0)
        .on_choose(move |s: &mut S, val| {
            let sh = acc(s);
            sh.voice.settings.camera = (!val.is_empty()).then(|| val.to_string());
            sh.intents.push(Intent::Voice(Op::Settings));
            // Reopen the capture on the new device.
            sh.intents.push(Intent::Voice(Op::Preview(false)));
            sh.intents.push(Intent::Voice(Op::Preview(true)));
        })
        .el(&st.popups, pops(acc), p, layer);
    let can_turn_on = v.connected && !v.camera;
    crate::ui::Modal::new(t("Camera preview"), close)
        .body(col().w_full().gap(16.0).child(preview).child(pick))
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("cam-cancel").on_press(close).el(p))
                .child(
                    Button::new(t("Turn on camera"))
                        .key("cam-on")
                        .disabled(!can_turn_on)
                        .on_press(move |s: &mut S| {
                            let sh = acc(s);
                            sh.voice.modal = None;
                            sh.voice.previewing = false;
                            sh.intents.push(Intent::Voice(Op::ToggleCamera));
                        })
                        .el(p),
                ),
        )
}

/// `ScreenSharePickerModal`: quality and frame rate, then the system picker (the XDG portal on
/// Linux lists screens and windows with their own thumbnails).
// shortcut: no in-app source grid with thumbnails; WebRTC's portal capturer can't enumerate
// sources before the portal dialog. Windows / macOS could list them via DesktopCapturer.
fn screen_picker<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, close: impl Fn(&mut S) + Copy + 'static, layer: &mut Vec<El>) -> crate::ui::Modal<S> {
    let set = &st.voice.settings;
    let res = [("720", "720p"), ("1080", "1080p"), ("1440", "1440p"), ("source", "Source")];
    let fps = [("15", "15 FPS"), ("30", "30 FPS"), ("60", "60 FPS")];
    let res_v = set.screen_height.map_or("source".to_string(), |h| h.to_string());
    let fps_v = set.screen_fps.to_string();
    let r = crate::ui::Select::new("share-res", &res, Some(&res_v))
        .label(t("Resolution"))
        .width(190.0)
        .on_choose(move |s: &mut S, v| {
            let sh = acc(s);
            sh.voice.settings.screen_height = v.parse().ok();
            sh.intents.push(Intent::Voice(Op::Settings));
        })
        .el(&st.popups, pops(acc), p, layer);
    let f = crate::ui::Select::new("share-fps", &fps, Some(&fps_v))
        .label(t("Frame rate"))
        .width(190.0)
        .on_choose(move |s: &mut S, v| {
            let sh = acc(s);
            sh.voice.settings.screen_fps = v.parse().unwrap_or(30);
            sh.intents.push(Intent::Voice(Op::Settings));
        })
        .el(&st.popups, pops(acc), p, layer);
    crate::ui::Modal::new(t("Share your screen"), close)
        .body(
            col()
                .w_full()
                .gap(16.0)
                .child(body_text("Choose the stream quality, then pick a screen or window in the system dialog.", p))
                .child(row().w_full().gap(16.0).child(r).child(f)),
        )
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("share-cancel").on_press(close).el(p))
                .child(Button::new("Go live").key("share-go").on_press(move |s: &mut S| {
                    let sh = acc(s);
                    sh.voice.modal = None;
                    sh.intents.push(Intent::ToggleScreen);
                }).el(p)),
        )
}

// ---- Popouts and menus ---------------------------------------------------------------------------

/// `VoiceDetailsPopout`: server, room, connection, encryption and the ping graph.
pub fn details<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !st.popups.is_open(DETAILS) {
        return;
    }
    let v = &st.voice;
    let line = |k: &str, val: String| {
        row()
            .w_full()
            .gap(8.0)
            .child(text(k.to_string()).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary).w(90.0))
            .child(text(val).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary).truncate().shrink())
    };
    let (host, room) = v.server.clone().unwrap_or_default();
    // The ping graph: one bar per sample, scaled to the slowest.
    let max = v.pings.iter().copied().max().unwrap_or(1).max(1) as f32;
    let graph = row()
        .w_full()
        .h(48.0)
        .gap(1.0)
        .items_end()
        .padding(4.0)
        .radius(6.0)
        .fill(p.background_tertiary)
        .children(v.pings.iter().map(|&ms| el().grow(1.0).h((ms as f32 / max * 40.0).max(2.0)).radius(1.0).fill(p.status_online)));
    let surface = col()
        .w(300.0)
        .padding(16.0)
        .gap(8.0)
        .radius(8.0)
        .fill(p.background_secondary)
        .border(1.0, p.background_modifier_accent)
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .role("dialog")
        .label("Voice details")
        .child(text(t("Voice connection")).font(font(14.0, 600.0).lh(18.0)).color(p.text_primary))
        .child(line("Server", host))
        .child(line("Room", room))
        .child(line(t("Ping"), v.ping.map_or("—".into(), |ms| format!("{ms} ms"))))
        .child(line(t("Encryption"), if v.encrypted { "Encrypted (server key)".into() } else { "Transport only (DTLS-SRTP)".into() }))
        .child(graph)
        .modal(true, move |s: &mut S| acc(s).popups.close());
    layer.push(anchor(surface, DETAILS, Placement::Top, 8.0));
}

/// `VoiceParticipantMenuItems` for a tile.
pub fn tile_menu<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !st.popups.is_open(TILE_MENU) {
        return;
    }
    let Some(super::Target::User { user, guild, .. }) = st.target else { return };
    let me = store.my_id();
    let mut m = Menu::new(TILE_MENU);
    let focused = st.voice.focus == Some(user);
    m = m.item(MenuItem::new("focus", if focused { t("Unfocus") } else { t("Focus") }).icon(&icons::CORNERS_OUT).on_select(move |s: &mut S| {
        acc(s).voice.focus = (!focused).then_some(user);
    }));
    let streaming = store.voice_states.get(&(guild, user)).is_some_and(|v| v.self_stream);
    if streaming && user != me {
        m = m.item(MenuItem::new("watch", t("Watch stream")).icon(&icons::MONITOR_PLAY_FILL).on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.watching = Some(user);
            sh.intents.push(Intent::Watch(Some(user)));
        }));
    }
    if user != me {
        let muted = st.voice.local_mutes.contains(&user);
        m = m.separator().item(MenuItem::new("local-mute", t("Mute")).checkbox(muted).on_select(op(acc, Op::LocalMute(user, !muted))));
        if let Some((None, ch)) = st.voice.channel
            && let Some(call) = store.calls.get(&ch)
            && !store.voice_states.contains_key(&(None, user))
        {
            let ringing = call.ringing.contains(&user);
            m = m.item(MenuItem::new("ring", if ringing { t("Stop ringing") } else { t("Ring") }).icon(&icons::PHONE_FILL).on_select(op(acc, Op::Ring(ch, user, !ringing))));
        }
    }
    m.el(&st.popups, pops(acc), p, layer);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vs(user: u64, stream: bool) -> VoiceState {
        VoiceState { user_id: Some(Snowflake(user)), channel_id: Some(Snowflake(9)), self_stream: stream, ..Default::default() }
    }

    #[test]
    fn streams_get_their_own_tile() {
        let (a, b) = (vs(1, true), vs(2, false));
        let people = vec![(Snowflake(1), &a), (Snowflake(2), &b)];
        let t = tiles(&people);
        assert_eq!(t.len(), 3);
        assert!(t[1].screen && t[1].user == Snowflake(1));
    }
}
