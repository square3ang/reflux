//! Calls: `VoiceCallView` (participant tiles in a grid or one focused/pinned, video and stream
//! tiles, speaking borders, mute icons, layout modes, participant list popout, connection stats,
//! app fullscreen, auto-hiding chrome, `VoiceControlBar`), the in-call settings menus
//! (`VoiceSettingsMenus`: devices, input profile/level, volumes, voice region, camera and screen
//! share menus), the DM call strip (`CompactVoiceCallView`) and its header buttons (`CallButtons`,
//! persisted compact height, unread chat badge, live stream previews, audio-only avatars),
//! `IncomingCallUI`, the voice dialogs (`CameraPreviewModal` with device/quality/mirror controls,
//! `HideOwnCameraConfirmModal`, `ScreenSharePickerModal`, `VoiceChannelFullModal`,
//! `VoiceConnectionConfirmModal`, `CallNotRingableModal`), `VoiceDetailsPopout` and the
//! participant menu (`VoiceParticipantMenuItems`: local mute, focus/pin, watch, ring).

use std::time::{Duration, Instant};

use reflux_model::{Channel, ChannelType, Permissions, Snowflake, VoiceState};
use reflux_store::Store;
use rune::{Color, El, Pixels, Shadow, col, el, icon, row, text};

use super::voice::{Modal, Op};
use super::{Intent, ShellAccess, ShellState, avatar, pops};
use crate::i18n::t;
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::overlay::{anchor, popout_shadow};
use crate::ui::{Button, ButtonVariant, Menu, MenuEntry, MenuItem, Placement, Switch, rgb, rgba};

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
pub const PARTICIPANTS: &str = "voice-participants";
pub const MENU_INPUT: &str = "voice-input-settings";
pub const MENU_OUTPUT: &str = "voice-output-settings";
pub const MENU_CAMERA: &str = "voice-camera-settings";
pub const MENU_SCREEN: &str = "voice-screen-settings";
pub const MENU_MORE: &str = "voice-more-options";

/// `COMPACT_VOICE_CALL_HEIGHT_MIN` / `MAX` from `CompactVoiceCallHeight.ts`; the min is smaller
/// here because our caller surface sits in the channel header, not the whole chat column.
/// `COMPACT_VOICE_CALL_HEIGHT_MIN` / `_MAX`; with no saved height the view starts at the min.
pub const COMPACT_HEIGHT_MIN: f32 = 320.0;
pub const COMPACT_HEIGHT_MAX: f32 = 1049.0;
pub const COMPACT_HEIGHT_DEFAULT: f32 = COMPACT_HEIGHT_MIN;
/// `COMPACT_HEIGHT_CHAT_AREA_RESERVATION`: the chat keeps this much of the window.
const COMPACT_CHAT_RESERVE: f32 = 220.0;

/// `VoiceCallLayout.layoutMode`: `grid` or one focused/pinned participant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutMode {
    #[default]
    Grid,
    Focus,
}

/// The numbers `VoiceDetailsPopout`'s Advanced section shows. `None` until the engine reports
/// them (the shell has no WebRTC stats access).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CallStats {
    pub audio_loss: f64,
    pub video_loss: f64,
    pub jitter_ms: f64,
    pub send_bitrate: f64,
    pub recv_bitrate: f64,
}

/// In-call UI state that isn't part of the voice engine (`VoiceCallLayout`, `VoicePrompts`,
/// `VoiceSettings.showMyOwnCamera` and the compact-call persistence, kept per device).
pub struct CallUi {
    pub layout: LayoutMode,
    /// `showMyOwnCamera`: false keeps our camera published but shows our avatar instead.
    pub show_own_camera: bool,
    /// `showMyOwnScreenShare`: false hides our own stream's tile from our view.
    pub show_own_screen: bool,
    /// `showNonVideoParticipants`.
    pub show_non_video: bool,
    /// `prioritizeSpeakingParticipants`.
    pub prioritize_speakers: bool,
    /// `HideOwnCameraConfirmModal` is open; the value is its "Don't ask me again" checkbox.
    pub hide_camera_confirm: Option<bool>,
    /// The input menu's level meter is running (`Op::MicTest`).
    pub meter_on: bool,
    /// A device enumeration was asked for by an open menu.
    pub devices_loading: bool,
    pub cameras_loading: bool,
    /// The DM call's chosen region (`VoiceRegionSelect`), `None` = Automatic.
    pub region: Option<String>,
    /// The channel `region` belongs to, so switching calls drops the stale pick.
    pub region_channel: Option<Snowflake>,
    /// The latest WebRTC stats, if the engine reported them.
    pub stats: Option<CallStats>,
    /// When the call connected (the stats popout's duration).
    pub connected_at: Option<Instant>,
    /// `VoiceRegionTeleportOverlay` is up until this instant.
    pub teleport_until: Option<Instant>,
    /// Call chrome is shown until this instant (`VOICE_HUD_IDLE_TIMEOUT_MS`); `None` = always.
    pub hud_until: Option<Instant>,
    /// `useVoiceCallAppFullscreen`: the call fills the app window.
    pub app_fullscreen: bool,
    /// Fullscreen was entered from the compact call view, so leaving it goes back there.
    pub fullscreen_from_compact: bool,
    /// `VoiceCallLayout.focusMembersRowVisible`: focus layout's row of the other participants.
    pub focus_members_row: bool,
    /// `IncomingCallUI`'s drag past its 6pt threshold: the pointer's travel so far.
    pub incoming_drag: Option<[f32; 2]>,
    /// The PiP overlay's drag in progress: the pointer's travel so far.
    pub pip_drag: Option<[f32; 2]>,
    /// `VoiceCallLayout.focusMembersRowHeight`: the row's dragged height, `None` = automatic.
    // shortcut: kept for the session; Fluxer persists it. Persist with the other layout prefs.
    pub members_row_h: Option<f32>,
    /// `FocusMembersRowDivider` is being dragged.
    pub members_dragging: bool,
    /// `trackActivityStateRef`: when each speaker last spoke and the order they were promoted in.
    pub activity: std::collections::HashMap<Snowflake, (Instant, u64)>,
    pub next_order: u64,
    /// The open tile menu is a stream tile's.
    pub menu_screen: bool,
}

impl Default for CallUi {
    fn default() -> Self {
        Self {
            layout: LayoutMode::Grid,
            show_own_camera: true,
            show_own_screen: true,
            show_non_video: true,
            prioritize_speakers: false,
            hide_camera_confirm: None,
            meter_on: false,
            devices_loading: false,
            cameras_loading: false,
            region: None,
            region_channel: None,
            stats: None,
            connected_at: None,
            teleport_until: None,
            hud_until: None,
            app_fullscreen: false,
            fullscreen_from_compact: false,
            focus_members_row: true,
            incoming_drag: None,
            pip_drag: None,
            members_row_h: None,
            members_dragging: false,
            activity: Default::default(),
            next_order: 0,
            menu_screen: false,
        }
    }
}

/// `VOICE_HUD_IDLE_TIMEOUT_MS`.
pub const HUD_IDLE: Duration = Duration::from_millis(2500);

impl CallUi {
    /// `setPinnedParticipant`: a pin implies focus mode; clearing it goes back to the grid.
    pub fn pin(&mut self, user: Option<Snowflake>) {
        self.layout = if user.is_some() { LayoutMode::Focus } else { LayoutMode::Grid };
    }

    /// Switches the layout from the More options menu; grid drops the pin (`setLayoutMode`).
    pub fn set_layout(&mut self, mode: LayoutMode) {
        self.layout = mode;
    }

    /// `VoiceCallLayout.pinnedParticipantIdentity`: remembered even while the grid is shown,
    /// but the tile is only enlarged in focus mode.
    pub fn pinned(&self, focus: Option<Snowflake>) -> Option<Snowflake> {
        (self.layout == LayoutMode::Focus).then_some(focus).flatten()
    }
}

/// `COMPACT_VOICE_CALL_HEIGHT_MIN..MAX`.
pub fn compact_height(h: f32) -> f32 {
    h.clamp(COMPACT_HEIGHT_MIN, COMPACT_HEIGHT_MAX)
}

/// The chat pill's unread count: messages past the read marker in the loaded window.
pub fn unread_messages(store: &Store, channel: Snowflake) -> u32 {
    // shortcut: the store keeps a 200-message window and no total unread count, so a channel with
    // more than the window clamps at 200 (the badge shows 9+ anyway).
    let ack = store.read_states.get(&channel).and_then(|r| r.last_message_id);
    store.messages(channel).iter().filter(|m| ack.is_none_or(|a| m.id > a)).count() as u32
}

/// `VoiceParticipantSortUtils`: speaking participants first when the setting is on, user ID
/// order otherwise (stable, so a speaker does not jump between frames while still talking).
pub fn order_participants<'a>(people: &[(Snowflake, &'a VoiceState)], speaking: &std::collections::HashSet<Snowflake>, prioritize: bool) -> Vec<(Snowflake, &'a VoiceState)> {
    let mut v = people.to_vec();
    if prioritize {
        v.sort_by_key(|(u, _)| !speaking.contains(u));
    }
    v
}

/// The queue stays non-empty for our own tile: `show_non_video` hides only others without media.
fn visible_people<'a>(people: &[(Snowflake, &'a VoiceState)], me: Snowflake, show_non_video: bool, videos: &std::collections::HashMap<Snowflake, Pixels>, speaking: &std::collections::HashSet<Snowflake>) -> Vec<(Snowflake, &'a VoiceState)> {
    if show_non_video {
        return people.to_vec();
    }
    people
        .iter()
        .filter(|(u, v)| *u == me || v.self_video || v.self_stream || videos.contains_key(u) || speaking.contains(u))
        .copied()
        .collect()
}

/// `formatPacketLossPercent` / `formatBitrateKbps` for the stats popout.
pub fn stat_percent(v: f64) -> String {
    format!("{v:.1}%")
}

pub fn stat_bitrate_kbps(bps: f64) -> String {
    format!("{:.0} kbps", bps / 1000.0)
}

pub fn stat_ms(v: f64) -> String {
    format!("{v:.0} ms")
}

/// RTT variation as a jitter stand-in: the mean absolute difference of the ping samples.
pub fn jitter_from_pings(pings: &[u32]) -> f64 {
    let n = pings.len();
    if n < 2 {
        return 0.0;
    }
    pings.windows(2).map(|w| w[0].abs_diff(w[1]) as f64).sum::<f64>() / (n - 1) as f64
}

/// Minutes:seconds for the connection duration.
pub fn duration_label(d: Duration) -> String {
    let secs = d.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}


fn op<S: 'static>(acc: ShellAccess<S>, o: Op) -> impl Fn(&mut S) + 'static {
    move |s: &mut S| acc(s).intents.push(Intent::Voice(o.clone()))
}

/// One tile: a participant (camera or avatar) or their screen share.
#[derive(Clone, Copy, PartialEq)]
struct Tile {
    user: Snowflake,
    screen: bool,
}

/// Participants of a channel's call, ordered by user ID (`VoiceParticipantSortUtils`' fallback).
fn participants(store: &Store, guild: Option<Snowflake>, channel: Snowflake) -> Vec<(Snowflake, &VoiceState)> {
    let mut v: Vec<_> = store.voice_states.iter().filter(|((g, _), v)| *g == guild && v.channel_id == Some(channel)).map(|((_, u), v)| (*u, v)).collect();
    v.sort_by_key(|(u, _)| *u);
    v
}

/// `gridTracks`' base order: every screen share, then every camera.
fn tiles(people: &[(Snowflake, &VoiceState)]) -> Vec<Tile> {
    let shares = people.iter().filter(|(_, v)| v.self_stream).map(|(u, _)| Tile { user: *u, screen: true });
    shares.chain(people.iter().map(|(u, _)| Tile { user: *u, screen: false })).collect()
}

/// `VOICE_GRID_SPEAKER_PRIORITY_HOLD_MS`: how long a speaker keeps their promoted place.
const SPEAKER_HOLD: Duration = Duration::from_millis(4500);

/// `useVoiceCallTracksAndLayout`'s `gridTracks`: the shares, then the cameras, which
/// `showNonVideoParticipants` off narrows to cameras that are on. With Prioritize speakers on,
/// streaming ranks 3, speaking 2 and recently spoke 1, each rank in promotion order.
fn call_tiles(st: &ShellState, me: Snowflake, people: &[(Snowflake, &VoiceState)]) -> Vec<Tile> {
    let camera_on = |u: Snowflake| if u == me { st.voice.camera } else { st.voice.videos.contains_key(&u) };
    let mut all: Vec<Tile> = tiles(people).into_iter().filter(|t| t.screen || st.call.show_non_video || camera_on(t.user)).collect();
    if st.call.prioritize_speakers {
        all.sort_by_key(|t| {
            let streaming = t.screen || people.iter().any(|(u, v)| *u == t.user && v.self_stream);
            let held = st.call.activity.get(&t.user).map(|a| a.1);
            let rank = if streaming {
                3
            } else if st.voice.speaking.contains(&t.user) {
                2
            } else {
                held.is_some() as u8
            };
            (std::cmp::Reverse(rank), if rank > 0 { held.unwrap_or(u64::MAX) } else { u64::MAX })
        });
    }
    all
}

/// The video a tile shows, if any.
fn video(st: &ShellState, me: Snowflake, t: Tile) -> Option<&Pixels> {
    let v = &st.voice;
    match (t.screen, t.user == me) {
        (true, true) if v.screen && v.own_share.size().0 > 0 => Some(&v.own_share),
        (true, _) => (v.watching == Some(t.user)).then_some(&v.stream),
        // `showMyOwnCamera`: ours is only hidden from our own view, the track stays published.
        (false, true) => (v.camera && st.call.show_own_camera).then_some(&v.self_view),
        (false, false) => v.videos.get(&t.user),
    }
}

/// `useStreamPreview`: the uploaded preview frame for a participant's stream, if one is loaded.
fn stream_preview(store: &Store, st: &ShellState, guild: Option<Snowflake>, user: Snowflake) -> Option<Pixels> {
    let channel = st.voice.channel?.1;
    let key = super::active_now::stream_key(store, guild, channel, user)?;
    super::active_now::preview_pixels(st, &key)
}

/// `VoiceParticipantTile`'s `presentation`: a grid tile, the focused tile, or one in the focus
/// layout's members row.
#[derive(Clone, Copy, PartialEq)]
enum Pres {
    Grid,
    FocusMain,
    FocusSecondary,
}

/// `useVoiceCallHudVisibility`: the call chrome is up (no idle timeout, or not yet spent).
fn hud_visible(st: &ShellState) -> bool {
    st.call.hud_until.is_none_or(|t| t > Instant::now())
}

/// `VoiceParticipantTile`: the video, the stream's still or watch prompt, or the avatar on its
/// colour, with the tile's overlays and its name pill. `hud` is the call chrome's visibility,
/// which the name and control pills fade with.
#[allow(clippy::too_many_arguments)]
fn tile_in<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, t: Tile, vs: &VoiceState, pres: Pres, hud: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    use super::voice::WatchState;
    let me = store.my_id();
    let v = &st.voice;
    let key = format!("tile-{}-{}", t.user, t.screen as u8);
    // The tile's last laid-out size stands in for its `@container` queries.
    let [w, h] = rune::memo::layout_size(&key).unwrap_or([300.0, 170.0]);
    let speaking = !t.screen && v.speaking.contains(&t.user);
    let name = store.display_name(guild, t.user);
    let full = || el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0));
    let r = if pres == Pres::FocusMain { 0.0 } else { dim::RADIUS_LG };
    let fit = rune::SurfaceFit::Fit;
    let mut body = full().radius(r).clip_rounded(r);
    let mut overlays: Vec<El> = Vec::new();
    let watched = t.screen && t.user != me && v.watching == Some(t.user);
    // `useOwnScreenSharePreviewState`: our share freezes while the window is in the background.
    let own_paused = t.screen && t.user == me && st.desktop.unfocused && crate::look::with(|l| l.pause_preview_unfocused);
    let shown = video(st, me, t).filter(|px| px.size() != (0, 0));
    let mut has_video = false;
    let mut live_info = None;
    if watched {
        // `ScreenShareBufferingFrame` (corner over the dimmed last frame, else full), the failed
        // frame, or `streamEndedOverlay`.
        let state = if v.watch_state == WatchState::Idle { WatchState::Buffering } else { v.watch_state };
        let last = v.stream.size() != (0, 0);
        body = body.fill(surface_0());
        if last {
            body = body.pixels_fit(&v.stream, fit);
        }
        match state {
            WatchState::Failed(code) => overlays.push(super::voice::failure_frame(code, p).key("failed")),
            WatchState::Ended => overlays.push(super::voice::ended_overlay(h, p, acc).key("ended")),
            WatchState::Buffering if last => {
                overlays.push(full().fill(rgba(0, 0, 0, 0.5)).key("dimmed"));
                overlays.push(super::voice::buffering_frame(true, w).key("buffering"));
            }
            WatchState::Buffering => overlays.push(super::voice::buffering_frame(false, w).key("buffering")),
            _ => {
                has_video = last;
                live_info = super::voice::stream_info(&v.stream, v.watch_fps);
            }
        }
    } else if let Some(px) = shown {
        has_video = true;
        body = body.fill(surface_0()).pixels_fit(px, fit);
        if own_paused {
            overlays.push(super::voice::self_preview(h, p).key("paused"));
        }
    } else if t.screen && t.user != me {
        // `screensharePreviewBackground` (the uploaded still at brightness .4), else
        // `streamPreviewUnavailableSurface` (surface-4 80% over surface-0), under the prompt.
        match stream_preview(store, st, guild, t.user) {
            Some(px) => body = body.fill(surface_0()).pixels_fit(&px, fit).child(full().fill(rgba(0, 0, 0, 0.6))),
            None => body = body.fill(rgb(0x24, 0x24, 0x27)),
        }
        overlays.push(watch_overlay(t.user, w, p, acc).key("watch"));
    } else if t.screen {
        body = body.fill(surface_0());
    } else {
        // `lkParticipantPlaceholder` on `getPlaceholderAvatarColor` dimmed by 12%, and
        // `.tileAvatarRing`'s size from the tile's size; speaking rings it 3px.
        let size = (w.min(h) * 0.32).clamp(80.0, 192.0).min(w - 24.0).min(h - 24.0).max(44.0);
        let mut face = el().size(size, size).radius(999.0).child(avatar(store, st, t.user, size, None, surface_3(), p));
        if speaking {
            face = face.child(el().pin(Some(-3.0), Some(-3.0), Some(-3.0), Some(-3.0)).radius(999.0).border(3.0, p.status_online));
        }
        body = body.fill(placeholder_color(store, t.user)).child(row().w_full().h_full().center().child(face));
    }
    // `isOwnCameraHidden`: `FeedHiddenOverlay` over our own camera.
    if t.user == me && !t.screen && v.camera && !st.call.show_own_camera {
        overlays.push(feed_hidden(p, acc).key("feed-hidden"));
    }
    let n280 = w <= 280.0;
    let inset = if n280 { 5.0 } else { 6.0 };
    // `streamInfoHud`: the tile's LIVE badge, after its resolution while watched.
    let ended = watched && matches!(v.watch_state, WatchState::Ended | WatchState::Failed(_));
    if t.screen && pres == Pres::Grid && !ended && !own_paused {
        let mut hud_row = row().gap(4.0).h(20.0).center_y();
        if let Some(info) = live_info.filter(|_| watched) {
            hud_row = hud_row.child(tile_badge(text(info), Color::BLACK).opacity(if hud { 1.0 } else { 0.0 }));
        }
        hud_row = hud_row.child(tile_badge(text("LIVE"), rgb(220, 38, 38)).role("img").label(crate::i18n::t("Screen sharing")));
        overlays.push(el().pin(None, Some(6.0), Some(6.0), None).child(hud_row).inert().key("stream-info"));
    }
    // `focusOverlay`: a members-row tile of the focused participant.
    if pres == Pres::FocusSecondary && st.call.pinned(v.focus) == Some(t.user) {
        overlays.push(full().radius(r).fill(rgba(0, 0, 0, 0.46)).child(el().pin(None, Some(8.0), Some(8.0), None).child(icon(&icons::EYE_FILL, 24.0).color(Color::WHITE))).inert().key("focused"));
    }
    // `tileControlPill` with the spectator count (persistent while anyone watches).
    let watchers = if t.screen && pres == Pres::Grid { super::voice::spectators_for(store, guild, t.user).len() } else { 0 };
    if watchers > 0 {
        let ph = if n280 { 22.0 } else { 24.0 };
        let slot = row().h_full().min_w(ph).px(6.0).gap(3.0).center().child(icon(&icons::EYE_FILL, 14.0).color(Color::WHITE)).child(text(watchers.to_string()).font(font(10.0, 600.0).lh(12.0)).color(Color::WHITE));
        let pill = row()
            .pin(Some(inset), Some(inset), None, None)
            .h(ph)
            .radius(if n280 { 7.0 } else { 8.0 })
            .clip_rounded(if n280 { 7.0 } else { 8.0 })
            .fill(Color::BLACK)
            .border(1.0, hud_border())
            .box_shadow(Shadow { x: 0.0, y: 2.0, blur: 6.0, spread: 0.0, color: rgba(0, 0, 0, 0.24), inset: false })
            .role("img")
            .label(crate::i18n::tf("{length, plural, one {# watching} other {# watching}}", &[("length", &watchers.to_string())]))
            .child(slot);
        overlays.push(pill.key("viewers"));
    }
    // `lkParticipantMetadata`: the ⋯ menu, mute / deafen and device icons, the name and the
    // connection ID, narrowing with the tile.
    let item_font: f32 = if n280 { 11.0 } else { 12.0 };
    let mut pill = row()
        .px(if n280 { 5.0 } else { 6.0 })
        .py(if n280 { 2.0 } else { 3.0 })
        .center_y()
        .radius(if n280 { 8.0 } else { 9.0 })
        .fill(Color::BLACK)
        .border(1.0, hud_border())
        .box_shadow(Shadow { x: 0.0, y: 2.0, blur: 6.0, spread: 0.0, color: rgba(0, 0, 0, 0.18), inset: false })
        .max_w((w - 12.0).max(0.0))
        .min_w(0.0);
    if pres == Pres::Grid && t.user != me && w > 220.0 {
        let user = t.user;
        let bk = format!("tile-menu-{}-{}", t.user, t.screen as u8);
        let anchor = bk.clone();
        let size = if n280 { 18.0 } else { 20.0 };
        pill = pill.child(
            row()
                .key(bk)
                .size(size, size)
                .ml(-3.0)
                .mr(if n280 { 2.0 } else { 3.0 })
                .my(-2.0)
                .center()
                .radius(7.0)
                .hover(rgb(0x11, 0x11, 0x11))
                .hover_group()
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(crate::i18n::tf("Participant options for {participantDisplayName}", &[("participantDisplayName", &name)]))
                .child(icon(&icons::DOTS_THREE_BOLD, 15.0).color(rgba(255, 255, 255, 0.78)).group_color(Color::WHITE))
                .on_press(move |s: &mut S| {
                    // `handleMenuButtonKeyDown`: the menu opens from the button's centre.
                    let at = rune::memo::window_layout(&anchor).map_or([0.0, 0.0], |[x, y, w, h]| [x + w / 2.0, y + h / 2.0]);
                    let sh = acc(s);
                    sh.popups.open_at(TILE_MENU, at);
                    sh.target = Some(super::Target::User { user, guild, dm: None });
                }),
        );
    }
    let mut icons_row = row().gap(if w <= 220.0 { 4.0 } else { 6.0 }).center_y();
    // `muteStatusClassName`: red for a moderator mute or no Speak permission, muted otherwise.
    // shortcut: only our own Speak permission is known here; others show red for a server mute only.
    let no_speak = t.user == me && !super::voice::can(store, v, Permissions::SPEAK);
    if vs.mute || vs.self_mute || no_speak {
        icons_row = icons_row.child(icon(&icons::MICROPHONE_SLASH_FILL, 14.0).color(if vs.mute || no_speak { p.status_danger } else { p.text_primary_muted }).key("mute"));
    }
    if vs.deaf || vs.self_deaf {
        icons_row = icons_row.child(icon(&icons::SPEAKER_SLASH_FILL, 14.0).color(if vs.deaf { p.status_danger } else { p.text_primary_muted }).key("deaf"));
    }
    icons_row = icons_row.child(icon(if vs.is_mobile { &icons::DEVICE_MOBILE } else { &icons::DESKTOP }, 14.0).color(Color::WHITE).key("device"));
    pill = pill.child(icons_row);
    let mut label = row().gap(3.0).center_y().min_w(0.0).shrink().max_w((34.0 * item_font * 0.55).min(w - 16.0).max(0.0));
    if w > 170.0 {
        let nf = if w <= 240.0 { 11.0 } else { 12.0 };
        label = label.child(text(name.clone()).key("name").font(font(nf, 500.0).lh(nf * 1.2)).color(Color::WHITE).truncate().shrink().ml(3.0));
    }
    if st.voice.settings.show_connection_ids
        && w > 320.0
        && let Some(c) = &vs.connection_id
    {
        label = label.child(text(format!("({c})")).key("conn").font(font(11.0, 400.0).lh(13.2)).color(rgba(255, 255, 255, 0.72)).truncate().shrink());
    }
    pill = pill.child(label);
    let mut meta = el().pin(Some(inset), None, None, Some(inset)).child(pill).key("meta");
    // `.lkParticipantMetadata` fades with the HUD; focus tiles' pills never take the pointer.
    if !hud {
        meta = meta.opacity(0.0).inert();
    } else if pres != Pres::Grid {
        meta = meta.inert();
    }
    let radius = match pres {
        Pres::FocusMain => 0.0,
        Pres::FocusSecondary => 8.0,
        Pres::Grid => 12.0,
    };
    let mut outer = el()
        .key(key)
        .w_full()
        .h_full()
        .radius(radius)
        .clip_rounded(radius)
        .fill(if pres == Pres::FocusMain { Color::TRANSPARENT } else if t.screen { surface_0() } else { surface_2() })
        .child(body)
        .children(overlays)
        .child(meta)
        .role("group")
        .label(name.clone())
        .cursor(rune::Cursor::Pointer);
    // `[data-speaking='true']::after`: 3.5px inside a tile showing camera video, never on the
    // pinned tile; an avatar tile has only its ring.
    if speaking && has_video && pres != Pres::FocusMain {
        outer = outer.child(full().radius(dim::RADIUS_LG).border(3.5, p.status_online).inert());
    }
    // `handleTileClick`: clicking the pinned tile unpins it (back to the grid); any other tile
    // pins it.
    let was_focused = st.call.pinned(v.focus) == Some(t.user);
    let user = t.user;
    outer
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            let next = (!was_focused).then_some(user);
            sh.voice.focus = next;
            sh.call.pin(next);
        })
        .participant_menu_for(user, guild, t.screen, acc)
}

/// `.pillOnTile` / `.liveBadgeOnVoiceTile`: a 20px HUD badge (10/600 white).
fn tile_badge(label: El, fill: Color) -> El {
    row()
        .h(20.0)
        .px(6.0)
        .center_y()
        .radius(7.0)
        .fill(fill)
        .border(1.0, hud_border())
        .box_shadow(Shadow { x: 0.0, y: 2.0, blur: 6.0, spread: 0.0, color: rgba(0, 0, 0, 0.24), inset: false })
        .child(label.font(font(10.0, 600.0).lh(12.0)).color(Color::WHITE))
}

/// `WatchStreamOverlay`: the primary Watch stream button on `--voice-overlay-strong`; narrow tiles
/// shrink it, then drop its label.
fn watch_overlay<S: 'static>(user: Snowflake, w: f32, p: &Palette, acc: ShellAccess<S>) -> El {
    // shortcut: no "Add stream" button; reflux watches one stream at a time. Add it with
    // multi-stream watching.
    let small = w <= 230.0;
    let mut button = row()
        .h(if small { 30.0 } else { 32.0 })
        .gap(6.0)
        .center()
        .radius(8.0)
        .fill(p.button_primary_fill)
        .hover(p.button_primary_active_fill)
        .shadow(1.0, 2.0, rgba(0, 0, 0, 0.1))
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(t("Watch stream"))
        .child(icon(&icons::MONITOR_PLAY_FILL, 18.0).color(p.button_primary_text));
    if w <= 190.0 {
        button = button.w(32.0);
    } else {
        let f = if small { 12.0 } else { 13.0 };
        button = button.px(if small { 10.0 } else { 12.0 }).child(text(t("Watch stream")).font(font(f, 600.0).lh(f * 1.2)).color(p.button_primary_text).truncate());
    }
    el()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(rgba(0, 0, 0, 0.78))
        .center()
        .child(button.key("watch-stream").on_press(move |s: &mut S| {
            // `handleWatch`: watch and pin the share.
            let sh = acc(s);
            sh.voice.watching = Some(user);
            sh.intents.push(Intent::Watch(Some(user)));
            sh.voice.focus = Some(user);
            sh.call.pin(Some(user));
        }))
}

/// `FeedHiddenOverlay`: "Camera hidden" and a secondary "Show camera" button.
fn feed_hidden<S: 'static>(p: &Palette, acc: ShellAccess<S>) -> El {
    col()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .center()
        .gap(12.0)
        .fill(p.background_tertiary)
        .child(text(t("Camera hidden")).font(font(14.0, 500.0).lh(18.0)).color(p.text_secondary).align(rune::Align::Center).px(16.0))
        .child(
            row()
                .key("show-camera")
                .px(16.0)
                .py(8.0)
                .gap(8.0)
                .center()
                .radius(8.0)
                .fill(p.form_surface_background)
                .border(1.0, p.background_modifier_accent)
                .hover(crate::ui::mix(p.form_surface_background, p.text_primary, 94.0))
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Show camera"))
                .child(icon(&icons::MONITOR_PLAY_FILL, 18.0).color(p.button_secondary_text))
                .child(text(t("Show camera")).font(font(14.0, 600.0).lh(18.0)).color(p.button_secondary_text))
                .on_press(move |s: &mut S| acc(s).call.show_own_camera = true),
        )
}

/// `VoiceParticipantContextMenu` on right-click: the tile menu ([`tile_menu`]) for whoever a
/// call tile, avatar or participant row stands for.
pub trait ParticipantMenu: Sized {
    fn participant_menu<S: 'static>(self, user: Snowflake, guild: Option<Snowflake>, acc: ShellAccess<S>) -> El {
        self.participant_menu_for(user, guild, false, acc)
    }
    /// `screen`: the menu of the user's stream tile (`source.kind === 'screen-share'`).
    fn participant_menu_for<S: 'static>(self, user: Snowflake, guild: Option<Snowflake>, screen: bool, acc: ShellAccess<S>) -> El;
}

impl ParticipantMenu for El {
    fn participant_menu_for<S: 'static>(self, user: Snowflake, guild: Option<Snowflake>, screen: bool, acc: ShellAccess<S>) -> El {
        self.on_secondary_press(move |s: &mut S, at, _| {
            let sh = acc(s);
            sh.popups.open_at(TILE_MENU, [at.x, at.y]);
            sh.target = Some(super::Target::User { user, guild, dm: None });
            sh.call.menu_screen = screen;
        })
    }
}

/// `getPlaceholderAvatarColor` + `dimColor(…, 0.12)`: the avatar colour, a default avatar's
/// colour, else the accent colour, else `DEFAULT_ACCENT_COLOR`.
fn placeholder_color(store: &Store, user: Snowflake) -> Color {
    const DEFAULTS: [u32; 6] = [0x4641d9, 0xf0b100, 0x00bba7, 0x2b7fff, 0xad46ff, 0x6a7282];
    let u = store.users.get(&user);
    let c = match u {
        Some(u) if u.avatar_color.is_some() => u.avatar_color.unwrap_or_default(),
        Some(u) if u.avatar.is_none() => DEFAULTS[(user.0 % 6) as usize],
        // shortcut: other users' accent colours only arrive with their profile, which the tile
        // doesn't fetch; ours is on the session user.
        Some(_) => store.me.as_ref().filter(|m| m.base.id == user).and_then(|m| m.accent_color).unwrap_or(0x4641d9),
        None => 0x4641d9,
    };
    let f = |shift: u32| (((c >> shift) & 0xff) as f32 * 0.88).round() as u8;
    rgb(f(16), f(8), f(0))
}

/// A 16:9 slot that fits inside its cell.
/// `.focusLayoutMainWrapper`: the focused tile as the largest 16:9 box inside the area (last
/// frame's size, keyed). `aspect_fit` only followed the width, so a wide window (fullscreen)
/// made the tile taller than the area and pushed the header and strip out.
fn slot(content: El, key: &'static str) -> El {
    let [w, h] = rune::memo::layout_size(key).unwrap_or([0.0, 0.0]);
    let tw = w.min(h * 16.0 / 9.0).max(0.0);
    row().key(key).w_full().grow(1.0).min_w(0.0).min_h(0.0).center().child(el().size(tw, tw * 9.0 / 16.0).child(content))
}

/// `--voice-focus-control-clearance`: the focus layout keeps this much clear for the control bar.
const FOCUS_CLEARANCE: f32 = 72.0;

/// The focus layout's tiles, the members row and its toggle (`VoiceCallLayoutContent`).
struct FocusInfo {
    /// The members row shows (`focusLayoutContentFullscreenMembersVisible`).
    members_row: bool,
    /// The focused tile is a screen share (`isFocusedOnScreenShare`).
    screen: Option<Snowflake>,
}

/// The grid's area. The full view's comes from the view's size (between the chrome, less the
/// 48px and 1rem paddings), so it is known while the focus layout shows instead of the grid.
fn grid_area(compact: bool) -> [f32; 2] {
    if compact {
        return rune::memo::layout_size("call-grid-compact").unwrap_or([960.0, 540.0]);
    }
    let chrome = dim::LAYOUT_HEADER_HEIGHT + rune::memo::layout_size("call-footer").map_or(78.0, |[_, h]| h) + 96.0 + 32.0;
    rune::memo::layout_size("call-view").map_or([960.0, 540.0], |[w, h]| [w, (h - chrome).max(0.0)])
}

/// Whether the call view shows the focus layout and what it focuses, as `layout` decides.
fn focus_of(store: &Store, st: &ShellState, people: &[(Snowflake, &VoiceState)], compact: bool) -> (Vec<Tile>, Option<Tile>) {
    let all = call_tiles(st, store.my_id(), people);
    let [gw, gh] = grid_area(compact);
    let packed = pack_grid(all.len(), gw, gh, compact);
    let focused = st.call.pinned(st.voice.focus).and_then(|u| all.iter().copied().find(|t| t.user == u && (t.screen || !all.iter().any(|x| x.user == u && x.screen))));
    // `gridOverflow`: a grid that can't fit every tile at its minimum size shows focus instead.
    let focused = focused.or_else(|| (packed.visible < all.len()).then(|| all.first().copied()).flatten());
    (all, focused)
}

/// The view-level facts the call view's chrome needs about its layout.
fn focus_info(store: &Store, st: &ShellState, people: &[(Snowflake, &VoiceState)]) -> Option<FocusInfo> {
    let (all, f) = focus_of(store, st, people, false);
    let f = f?;
    Some(FocusInfo { members_row: st.call.focus_members_row && all.len() > 1, screen: f.screen.then_some(f.user) })
}

/// The tile area: the packed grid, or one focused/pinned tile with a row of the rest.
/// Tiles without video show the avatar on its colour, as Fluxer's full call view does.
fn layout<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, people: &[(Snowflake, &VoiceState)], compact: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let (all, focused) = focus_of(store, st, people, compact);
    let state = |u: Snowflake| people.iter().find(|(x, _)| *x == u).map(|(_, v)| *v);
    let hud = compact || hud_visible(st);
    let make = |t: Tile, pres: Pres| state(t.user).map(|vs| tile_in(store, st, guild, t, vs, pres, hud, p, acc)).unwrap_or_else(el);
    let grid_key = if compact { "call-grid-compact" } else { "call-grid" };
    if let Some(f) = focused {
        if compact {
            // The compact view shows just the focused tile.
            return col().w_full().h_full().gap(8.0).padding(16.0).items_center().child(slot(make(f, Pres::FocusMain), "call-focus-compact"));
        }
        return focus_layout(f, &all, st, &make, acc);
    }
    // `VoiceGridLayout` in `.gridLayoutWrapper` (1rem above and below): the packed columns, each
    // tile the computed 16:9 size, rows centred.
    let [gw, gh] = grid_area(compact);
    let packed = pack_grid(all.len(), gw, gh, compact);
    let mut grid = col().key(grid_key).w_full().h_full().min_w(0.0).min_h(0.0).gap(packed.gap).center();
    for chunk in all.chunks(packed.columns.max(1)) {
        grid = grid.child(row().gap(packed.gap).justify_center().children(chunk.iter().map(|t| el().key(format!("g-{}-{}", t.user, t.screen as u8)).size(packed.tile[0], packed.tile[1]).child(make(*t, Pres::Grid)))));
    }
    if compact { grid } else { col().w_full().h_full().min_w(0.0).min_h(0.0).py(16.0).child(grid) }
}

/// `focusLayoutContentFullscreen`: the focused tile as large a 16:9 box as fits above what the
/// members row reserves, the row 72px up from the bottom (resizable by its divider), and the
/// Show / Hide members toggle; the header and control bar float over it.
fn focus_layout<S: 'static>(f: Tile, all: &[Tile], st: &ShellState, make: &dyn Fn(Tile, Pres) -> El, acc: ShellAccess<S>) -> El {
    let rest: Vec<Tile> = all.iter().copied().filter(|t| *t != f).collect();
    let [aw, ah] = rune::memo::layout_size("call-focus").unwrap_or([960.0, 600.0]);
    let show_row = !rest.is_empty() && st.call.focus_members_row;
    // `--voice-focus-members-row-height`: the dragged height, else clamp(86px, 13cqh, 112px),
    // kept within 64px..40cqh.
    let max_h = (ah * 0.4).floor().max(64.0);
    let row_h = st.call.members_row_h.unwrap_or((ah * 0.13).clamp(86.0, 112.0)).min(max_h).max(64.0);
    let reserved = if show_row { FOCUS_CLEARANCE + row_h + 8.0 } else { 0.0 };
    let tw = aw.min((ah - reserved).max(0.0) * 16.0 / 9.0);
    let main = el().size(tw, tw * 9.0 / 16.0).fill(surface_0()).child(make(f, Pres::FocusMain));
    let mut c = el().key("call-focus").w_full().h_full().child(row().pin(Some(0.0), Some(0.0), Some(0.0), Some(reserved)).center().child(main).key("main"));
    if show_row {
        // `.focusLayoutMembersRow`: 6px clamp(12px, 2vw, 24px) padding, scrolling sideways with no
        // scrollbar; `.focusLayoutMembersRowInner` centres (`margin-inline: auto`).
        let vw = rune::memo::layout_size(super::BODY_KEY).map_or(1280.0, |[w, _]| w);
        let pad = (vw * 0.02).clamp(12.0, 24.0);
        let tile_h = row_h - 12.0;
        let inner_w = rune::memo::layout_size("call-strip").map_or(0.0, |[w, _]| w) - pad * 2.0;
        let inner = row().min_w(inner_w.max(0.0)).h_full().gap(8.0).center_y().justify_center().children(rest.iter().copied().map(|t| el().key(format!("s-{}-{}", t.user, t.screen as u8)).w(tile_h * 16.0 / 9.0).h(tile_h).child(make(t, Pres::FocusSecondary))));
        c = c.child(row().key("call-strip").pin(Some(0.0), None, Some(0.0), Some(FOCUS_CLEARANCE)).h(row_h).py(6.0).px(pad).scroll_x().child(inner));
        // `FocusMembersRowDivider`: a 16px row-resize strip on the row's top edge; its 2px line
        // shows at .5 on hover and .8 while dragging. Dragging up grows the row (64px..40%).
        let dragging = st.call.members_dragging;
        let line = el().pin(Some(pad), Some(7.0), Some(pad), None).h(2.0).radius(1.0).fill(Color::WHITE);
        let line = if dragging { line.opacity(0.8) } else { line.group_opacity(0.0, 0.5) };
        c = c.child(
            el()
                .key("members-divider")
                .pin(Some(0.0), None, Some(0.0), Some(FOCUS_CLEARANCE + row_h - 4.0))
                .h(16.0)
                .cursor(rune::Cursor::ResizeRow)
                .role("separator")
                .label(t("Resize members row"))
                .hover_group()
                .child(line)
                .on_drag(move |s: &mut S, d: &mut rune::Drag, _| {
                    let sh = acc(s);
                    if d.phase == rune::Phase::Down {
                        d.data = row_h;
                    }
                    sh.call.members_dragging = d.phase != rune::Phase::Up;
                    sh.call.members_row_h = Some((d.data - d.dy).round().min(max_h).max(64.0));
                }),
        );
    }
    // `focusMembersToggle`: centred, 8px above the clearance or above the row.
    if !rest.is_empty() {
        let shown = st.call.focus_members_row;
        let label = if shown { t("Hide members") } else { t("Show members") }.to_string();
        let [w, _] = rune::memo::layout_size("focus-members-toggle").unwrap_or([66.0, 32.0]);
        let bottom = if show_row { FOCUS_CLEARANCE + row_h + 8.0 } else { FOCUS_CLEARANCE + 8.0 };
        let mut toggle = row()
            .key("focus-members-toggle")
            .pin(Some(((aw - w) / 2.0).max(0.0)), None, None, Some(bottom))
            .h(32.0)
            .px(12.0)
            .gap(6.0)
            .center_y()
            .radius(999.0)
            .fill(Color::BLACK)
            .border(1.0, hud_border())
            .box_shadow(Shadow { x: 0.0, y: 2.0, blur: 6.0, spread: 0.0, color: rgba(0, 0, 0, 0.24), inset: false })
            .hover(rgb(17, 17, 17))
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label)
            .child(icon(&icons::USERS_BOLD, 16.0).color(Color::WHITE))
            .child(icon(if shown { &icons::CARET_DOWN_BOLD } else { &icons::CARET_UP_BOLD }, 16.0).color(Color::WHITE))
            .on_press(move |s: &mut S| acc(s).call.focus_members_row = !shown);
        // Part of the chrome (`--voice-hud-opacity`): it fades with the control bar.
        if !hud_visible(st) {
            toggle = toggle.opacity(0.0).inert();
        }
        c = c.child(toggle);
    }
    c
}

/// `resolveVoiceGridPackedLayoutMetrics`'s result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Packed {
    pub columns: usize,
    pub gap: f32,
    pub tile: [f32; 2],
    /// How many tiles fit at the minimum size (fewer than the count = overflow).
    pub visible: usize,
}

/// `resolveVoiceGridPackedLayoutMetrics`: the most tiles that fit at the minimum tile size
/// (220px wide, 148 compact), and for that count the column count with the largest 16:9 tile.
pub fn pack_grid(count: usize, width: f32, height: f32, compact: bool) -> Packed {
    let gap = match count {
        n if n >= 40 => 4.0,
        n if compact && n >= 24 => 5.0,
        n if n >= 24 => 6.0,
        n if compact && n >= 12 => 6.0,
        n if n >= 12 => 8.0,
        n if n >= 6 => 10.0,
        _ if compact => (height * 0.024).clamp(6.0, 10.0),
        _ => 12.0,
    };
    let side = if compact {
        (height * 0.02).clamp(6.0, 12.0)
    } else if width <= 419.0 {
        6.0
    } else if width <= 759.0 {
        8.0
    } else {
        12.0
    };
    let vert = if compact {
        (height * 0.018).clamp(5.0, 10.0)
    } else if height <= 359.0 {
        8.0
    } else if height <= 519.0 {
        10.0
    } else {
        14.0
    };
    let (aw, ah) = ((width - side * 2.0).max(0.0), (height - vert * 2.0).max(0.0));
    let min_w = if compact { 148.0 } else { 220.0 };
    let min_h = min_w * 9.0 / 16.0;
    for visible in (1..=count).rev() {
        let mut best: Option<Packed> = None;
        for columns in 1..=visible {
            let rows = visible.div_ceil(columns);
            let col_w = ((aw - gap * (columns - 1) as f32) / columns as f32).max(0.0);
            let row_h = ((ah - gap * (rows - 1) as f32) / rows as f32).max(0.0);
            let tw = col_w.min(row_h * 16.0 / 9.0);
            let th = tw * 9.0 / 16.0;
            if tw < min_w || th < min_h {
                continue;
            }
            if best.is_none_or(|b| tw * th > b.tile[0] * b.tile[1]) {
                best = Some(Packed { columns, gap, tile: [tw, th], visible });
            }
        }
        if let Some(b) = best {
            return b;
        }
    }
    Packed { columns: 1, gap, tile: [0.0, 0.0], visible: 0 }
}

/// `.settingsButton`: the caret badge on a control's top-right corner (17px, 2px outside it,
/// black with a light border and shadow) that opens its in-call menu. Open from here it turns
/// white (`[data-context-menu-open]`); `.settingsButton:disabled` dims it.
fn settings_button<S: 'static>(st: &ShellState, id: &'static str, label: &str, disabled: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let open = st.popups.is_open(id) && st.popups.at.is_none();
    let b = row()
        .pin(None, Some(-2.0), Some(-2.0), None)
        .size(17.0, 17.0)
        .center()
        .radius(7.0)
        .fill(rgb(0, 0, 0))
        .border(1.0, rgba(255, 255, 255, 0.14))
        .shadow(6.0, 16.0, rgba(0, 0, 0, 0.5))
        .role("button")
        .label(label.to_string())
        .hover_group()
        .child(icon(&icons::CARET_DOWN_BOLD, 10.0).color(if open { Color::WHITE } else { rgba(255, 255, 255, 0.88) }).group_color(Color::WHITE));
    let b = if disabled { b.opacity(0.45).cursor(rune::Cursor::NotAllowed).disabled(true) } else { b.cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| acc(s).popups.toggle(id)) };
    super::badges::tip(id.to_string(), super::badges::label_tip(label, p), 190.0, b, acc)
}

/// `.buttonContainer`: a control with its caret badge over its corner.
fn control_pair(button: El, caret: El) -> El {
    el().size(44.0, 44.0).child(button).child(caret)
}

/// (background, hover background, icon) of a `VoiceControlBar` button.
type Look = (Color, Color, Color);

/// `.button`: 44px, 16 radius, the inset top highlight. `:disabled` dims it to .45 with a
/// not-allowed cursor and no hover; `open` (`[data-context-menu-open]`) keeps the hover fill.
fn bar_button(i: &'static rune::Icon, label: &str, look: Look, disabled: bool, open: bool) -> El {
    let b = row()
        .size(44.0, 44.0)
        .center()
        .radius(16.0)
        .fill(if open { look.1 } else { look.0 })
        .box_shadow(Shadow { x: 0.0, y: 1.0, blur: 0.0, spread: 0.0, color: rgba(255, 255, 255, 0.045), inset: true })
        .role("button")
        .label(label.to_string())
        .child(icon(i, 22.0).color(look.2));
    if disabled { b.opacity(0.45).cursor(rune::Cursor::NotAllowed).disabled(true) } else { b.hover(look.1).cursor(rune::Cursor::Pointer) }
}

/// `TooltipWithKeybind`: the label over the action's key caps (`.key`: 20 tall, 5 across,
/// 11/600 uppercase on background-secondary), unless keyboard hints are hidden.
fn keybind_tip(label: &str, combo: Option<crate::keybinds::Combo>, p: &Palette) -> El {
    let mut c = col().items_center().gap(6.0).py(7.0).px(10.0).child(text(label.to_string()).font(font(13.0, 500.0).lh(18.0)).color(p.text_primary).align(rune::Align::Center).wrap());
    if let Some(combo) = combo.filter(|_| !crate::look::with(|l| l.hide_keyboard_hints)) {
        c = c.child(row().gap(2.0).center_y().children(combo.caps().into_iter().map(|k| {
            row().min_w(20.0).h(20.0).px(5.0).center().radius(4.0).fill(p.background_secondary).child(text(k.to_uppercase()).font(font(11.0, 600.0).lh(11.0)).color(p.text_secondary))
        })));
    }
    c
}

/// `formatKeyCombo` for the recorded push-to-talk key (`keybinds::key_name`'s `{:?}` form).
fn ptt_key_label(k: &str) -> String {
    if let Some(c) = k.strip_prefix("Text(\"").and_then(|r| r.strip_suffix("\")")) {
        return c.to_uppercase();
    }
    if let Some(m) = k.strip_prefix("Modifier { modifier: ").and_then(|r| r.split(',').next()) {
        return m.to_string();
    }
    k.to_string()
}

/// `VOICE_CHANNEL_CAMERA_USER_LIMIT`.
const CAMERA_USER_LIMIT: usize = 25;

/// `VoiceControlBar`: 44px buttons on a black pill, each settings control with its caret menu.
/// Their state follows `selectVoiceControlBarState`; a right-click opens a control's menu at the
/// pointer (`openPointerMenu`).
fn control_bar<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let v = &st.voice;
    let me = store.my_id();
    let neutral = (rgba(255, 255, 255, 0.05), rgba(255, 255, 255, 0.12), rgba(255, 255, 255, 0.9));
    let red = (rgba(255, 69, 58, 0.16), rgba(255, 69, 58, 0.24), danger());
    let success = (rgba(48, 209, 88, 0.16), rgba(48, 209, 88, 0.24), success());
    let pick = |on: bool, a: Look, b: Look| if on { a } else { b };
    let at_pointer = |id: &str| st.popups.is_open(id) && st.popups.at.is_some();
    let menu_at = |id: &'static str| move |s: &mut S, at: rune::PointerPress, _: &mut rune::Tx| acc(s).popups.open_at(id, [at.x, at.y]);
    let tip = |key: &str, content: El, trigger: El| super::badges::tip(key.to_string(), content, 190.0, trigger, acc);
    // Signals.
    let (guild, channel) = v.channel.unzip();
    let guild = guild.flatten();
    let mine = store.voice_states.get(&(guild, me)).filter(|s| s.channel_id == channel);
    let (server_mute, server_deaf) = mine.map_or((false, false), |s| (s.mute, s.deaf));
    let no_speak = !super::voice::can(store, v, Permissions::SPEAK);
    let can_stream = super::voice::can(store, v, Permissions::STREAM);
    let mic_locked = server_mute || server_deaf || no_speak;
    let muted = v.self_mute || mic_locked;
    let people = channel.map(|c| participants(store, guild, c)).unwrap_or_default();
    let cam_limit = !v.camera && people.len() > CAMERA_USER_LIMIT;
    let cam_cap = !v.camera && people.iter().filter(|(_, s)| s.self_video).count() >= CAMERA_USER_LIMIT;
    let cam_ok = crate::camera::available();
    let prefs = crate::nav::mine(st);
    let combo = |a| crate::nav::keys::active(&prefs, a).first().copied();
    let deafened_by_mods = t("You were deafened by moderators");
    // Mute.
    let mute_label = if server_deaf {
        deafened_by_mods.to_string()
    } else if server_mute {
        t("Muted by moderators").to_string()
    } else if no_speak {
        t("You can't speak in this channel").to_string()
    } else if v.settings.ptt() && !v.self_mute {
        match v.settings.ptt_key.as_deref() {
            Some(k) => crate::i18n::tf("Push-to-talk is on. Hold {pushToTalkHint} to speak.", &[("pushToTalkHint", &ptt_key_label(k))]),
            None => t("Push-to-talk is on. Hold your push-to-talk key to speak.").to_string(),
        }
    } else if muted {
        t("Unmute").to_string()
    } else {
        t("Mute").to_string()
    };
    let mut mic = bar_button(if muted { &icons::MICROPHONE_SLASH_FILL } else { &icons::MICROPHONE_FILL }, &mute_label, pick(muted, red, neutral), mic_locked, at_pointer(MENU_INPUT));
    if !mic_locked {
        mic = mic.on_press(move |s: &mut S| acc(s).toggle_mute()).on_secondary_press(menu_at(MENU_INPUT));
    }
    let mic = tip("mic-button", keybind_tip(&mute_label, combo(crate::keybinds::Action::ToggleMute).filter(|_| !mic_locked), p), mic);
    // Deafen.
    let deaf = v.self_deaf || server_deaf;
    let deaf_label = if server_deaf { deafened_by_mods } else if deaf { t("Undeafen") } else { t("Deafen") };
    let mut deaf_button = bar_button(if deaf { &icons::SPEAKER_SLASH_FILL } else { &icons::SPEAKER_HIGH_FILL }, deaf_label, pick(deaf, red, neutral), server_deaf, at_pointer(MENU_OUTPUT));
    if !server_deaf {
        deaf_button = deaf_button.on_press(move |s: &mut S| acc(s).toggle_deaf()).on_secondary_press(menu_at(MENU_OUTPUT));
    }
    let deaf_button = tip("deaf-button", keybind_tip(deaf_label, combo(crate::keybinds::Action::ToggleDeafen).filter(|_| !server_deaf), p), deaf_button);
    // Camera.
    let limit_label;
    let cam_label = if !can_stream && !v.camera {
        t("You can't turn on your camera in this channel")
    } else if cam_limit {
        limit_label = crate::i18n::tf("Cameras are off above {voiceChannelCameraUserLimit} participants", &[("voiceChannelCameraUserLimit", &CAMERA_USER_LIMIT.to_string())]);
        &limit_label
    } else if cam_cap {
        limit_label = crate::i18n::tf("This channel is at its camera limit. Up to {voiceChannelCameraUserLimit} people can share video in it.", &[("voiceChannelCameraUserLimit", &CAMERA_USER_LIMIT.to_string())]);
        &limit_label
    } else if !cam_ok && !v.camera {
        "Camera needs Reflux built with GStreamer"
    } else if v.camera {
        t("Turn off camera")
    } else {
        t("Turn on camera")
    };
    let cam_disabled = !v.camera && (!can_stream || cam_limit || cam_cap || !cam_ok);
    let mut cam = bar_button(if v.camera { &icons::CAMERA_FILL } else { &icons::CAMERA_SLASH_FILL }, cam_label, pick(v.camera, success, neutral), cam_disabled, at_pointer(MENU_CAMERA));
    if !cam_disabled {
        cam = cam.on_press(op(acc, Op::ToggleCamera)).on_secondary_press(menu_at(MENU_CAMERA));
    }
    let cam = tip("camera-button", super::badges::label_tip(cam_label, p), cam);
    // Screen share.
    let scr_disabled = !can_stream && !v.screen;
    let scr_label = if scr_disabled {
        t("You can't share your screen in this channel")
    } else if v.screen {
        t("End screen share")
    } else {
        t("Share your screen")
    };
    let mut scr = bar_button(&icons::MONITOR_PLAY_FILL, scr_label, pick(v.screen, success, neutral), scr_disabled, at_pointer(MENU_SCREEN));
    if !scr_disabled {
        scr = scr
            .on_press(move |s: &mut S| {
                let sh = acc(s);
                if sh.voice.screen { sh.intents.push(Intent::ToggleScreen) } else { open_screen_picker(sh) }
            })
            .on_secondary_press(menu_at(MENU_SCREEN));
    }
    let scr = tip("screen-button", super::badges::label_tip(scr_label, p), scr);
    let mut bar = row()
        .padding(6.0)
        .gap(4.0)
        .center_y()
        .radius(16.0 + 6.0 + 2.0)
        .fill(rgb(0, 0, 0))
        .border(1.0, hud_border())
        .shadow(8.0, 24.0, rgba(0, 0, 0, 0.45))
        .child(control_pair(mic, settings_button(st, MENU_INPUT, t("Input settings"), false, p, acc)).key("mic"))
        .child(control_pair(deaf_button, settings_button(st, MENU_OUTPUT, t("Output settings"), false, p, acc)).key("deaf"))
        .child(control_pair(cam, settings_button(st, MENU_CAMERA, t("Camera settings"), false, p, acc)).key("camera"))
        .child(control_pair(scr, settings_button(st, MENU_SCREEN, t("Screen share settings"), scr_disabled, p, acc)).key("screen"));
    // `VoiceMoreOptionsMenu`: layout, own camera, speakers, region and settings.
    let more = bar_button(&icons::DOTS_THREE_BOLD, t("More options"), neutral, false, st.popups.is_open(MENU_MORE)).on_press(move |s: &mut S| {
        let sh = acc(s);
        sh.popups.toggle(MENU_MORE);
        if sh.popups.is_open(MENU_MORE) {
            ensure_regions(sh);
        }
    });
    bar = bar.child(tip(MENU_MORE, super::badges::label_tip(t("More options"), p), more));
    // `.buttonSeparator`: 1×22, 12% white, before Disconnect.
    bar = bar.child(el().key("separator").size(1.0, 22.0).radius(999.0).fill(rgba(255, 255, 255, 0.12)));
    // `.buttonDisconnect`: solid #ff3b30, #ff453a on hover.
    let disconnect = bar_button(&icons::PHONE_X_FILL, t("Disconnect"), (rgb(0xff, 0x3b, 0x30), danger(), Color::WHITE), false, false).on_press(move |s: &mut S| acc(s).leave_voice());
    bar = bar.child(tip("disconnect", super::badges::label_tip(t("Disconnect"), p), disconnect));
    // `canStopWatchingFocusedStream`: the focused tile is a stream we watch from elsewhere.
    if v.watching.is_some_and(|w| w != me) && st.call.pinned(v.focus) == v.watching {
        // `.stopWatchingButton`: no inset highlight.
        let stop = row()
            .size(44.0, 44.0)
            .center()
            .radius(16.0)
            .fill(red.0)
            .hover(red.1)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(t("Stop watching stream"))
            .child(icon(&icons::EYE_SLASH_FILL, 22.0).color(red.2))
            .on_press(move |s: &mut S| super::voice::stop_watching(s, acc));
        bar = bar.child(tip("stop-watching", super::badges::label_tip(t("Stop watching stream"), p), stop));
    }
    bar
}

/// `.voiceHeaderIconButton` / `.voiceHeaderBackButton`: 34px, radius 13, black with a faint
/// border and a 20px icon at .84; hover (or selected) is #111 with a brighter border and white.
fn header_button<S: 'static>(i: &'static rune::Icon, label: &str, selected: bool, on: impl Fn(&mut S) + 'static) -> El {
    let (bg, border, fg) = if selected { (rgb(0x11, 0x11, 0x11), rgba(255, 255, 255, 0.14), Color::WHITE) } else { (Color::BLACK, rgba(255, 255, 255, 0.08), rgba(255, 255, 255, 0.84)) };
    row()
        .size(34.0, 34.0)
        .center()
        .radius(13.0)
        .fill(bg)
        .border(1.0, border)
        .hover(rgb(0x11, 0x11, 0x11))
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.to_string())
        .child(el().pin(Some(-1.0), Some(-1.0), Some(-1.0), Some(-1.0)).radius(13.0).border(1.0, rgba(255, 255, 255, 0.14)).group_opacity(0.0, 1.0))
        .child(icon(i, 20.0).color(fg).group_color(Color::WHITE))
        .on_press(move |s: &mut S| on(s))
}

/// `.fullscreenButtonWrap button`: 42px, radius 14, black, a 20.8px white icon; hover #111.
fn corner_button<S: 'static>(i: &'static rune::Icon, label: &str, on: impl Fn(&mut S) + 'static) -> El {
    row()
        .size(42.0, 42.0)
        .center()
        .radius(14.0)
        .fill(Color::BLACK)
        .border(1.0, rgba(255, 255, 255, 0.12))
        .shadow(3.0, 10.0, rgba(0, 0, 0, 0.26))
        .hover(rgb(0x11, 0x11, 0x11))
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.to_string())
        .child(el().pin(Some(-1.0), Some(-1.0), Some(-1.0), Some(-1.0)).radius(14.0).border(1.0, rgba(255, 255, 255, 0.2)).group_opacity(0.0, 1.0))
        .child(icon(i, 20.8).color(Color::WHITE))
        .on_press(move |s: &mut S| on(s))
}

/// `.connectionStatusContainer`: Connecting / Reconnecting in the warning colour on black, with
/// its dot pulsing (only while it shows).
fn connection_pill(st: &ShellState, p: &Palette) -> Option<El> {
    let label = if st.voice.reconnecting {
        t("Reconnecting...")
    } else if !st.voice.connected {
        t("Connecting...")
    } else {
        return None;
    };
    let c = p.status_warning;
    Some(
        row()
            .px(12.0)
            .py(4.0)
            .gap(8.0)
            .center_y()
            .radius(999.0)
            .fill(Color::BLACK)
            .border(1.0, crate::ui::mix(c, Color::TRANSPARENT, 18.0))
            .role("status")
            .child(el().size(8.0, 8.0).radius(999.0).fill(c).looping(rune::LoopSpec::opacity(2.0, 0.0, 0.5, 1.0)))
            .child(text(label).font(font(12.0, 600.0).lh(16.0)).color(c)),
    )
}

/// `StreamFocusHeaderInfo`: the streamer's 20px avatar, "{name}'s screen", the stream's
/// resolution pill (at .92, no LIVE) once known, and up to five spectators.
pub(super) fn stream_focus_info<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, user: Snowflake, p: &Palette, acc: ShellAccess<S>) -> El {
    let name = store.display_name(guild, user);
    let mut streamer = row()
        .h_full()
        .gap(6.0)
        .center_y()
        .min_w(0.0)
        .shrink()
        .child(avatar(store, st, user, 20.0, None, Color::BLACK, p))
        .child(text(crate::i18n::tf("{streamerDisplayName}'s screen", &[("streamerDisplayName", &name)])).font(font(14.0, 500.0).lh(20.0)).color(Color::WHITE).truncate().shrink());
    if st.voice.watching == Some(user)
        && let Some(info) = super::voice::stream_info(&st.voice.stream, st.voice.watch_fps)
    {
        // `StreamInfoPill`'s default tone, `.streamFocusTrackInfo { transform: scale(0.92) }`.
        let k = 0.92;
        streamer = streamer.child(row().px(8.0 * k).py(2.0 * k).radius(999.0).fill(rgb(0x2a, 0x2a, 0x2d)).child(text(info).font(font(11.0 * k, 500.0).lh(13.2 * k)).color(p.text_secondary)).key("track-info"));
    }
    let mut info = row().h(32.0).gap(8.0).center_y().min_w(0.0).shrink().child(streamer);
    let watchers = super::voice::spectators_for(store, guild, user);
    if !watchers.is_empty() {
        // `AvatarStack` (20px, at most 5) opening `StreamSpectatorsPopout`.
        let mut stack = row().h(32.0).center_y().cursor(rune::Cursor::Pointer).role("button").label(crate::i18n::tf("Spectators - {count}", &[("count", &watchers.len().to_string())]));
        for (i, w) in watchers.iter().take(5).enumerate() {
            let face = row().padding(2.0).radius(12.0).fill(Color::BLACK).child(avatar(store, st, w.user, 20.0, None, Color::BLACK, p));
            stack = stack.child(if i == 0 { face } else { face.ml(-6.0) }.key(format!("w-{}-{}", w.user, w.connection)));
        }
        if st.voice.watching == Some(user) {
            stack = stack.key(super::voice::SPECTATORS_BTN).on_press(move |s: &mut S| acc(s).popups.toggle(super::voice::SPECTATORS_BTN));
        }
        info = info.child(stack);
    }
    info
}

/// `audioAvatarFallback` + `VoiceParticipantWrappedAvatarList`: with no tile to show, the
/// participants' avatars wrap in the middle (72px, smaller as more join), 12px apart.
fn avatar_fallback<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, people: &[(Snowflake, &VoiceState)], p: &Palette, acc: ShellAccess<S>) -> El {
    let size = match people.len() {
        n if n >= 12 => 50.0,
        n if n >= 8 => 56.0,
        n if n >= 5 => 64.0,
        _ => 72.0,
    };
    let vw = rune::memo::layout_size(super::BODY_KEY).map_or(1280.0, |[w, _]| w);
    let mut list = row().w_full().max_w(896.0).flex_wrap().justify_center().items_center().gaps(12.0, 0.0);
    for (u, vs) in people {
        let user = *u;
        let face = super::voice::voice_avatar(store, st, user, size, st.voice.speaking.contains(&user), vs.self_mute, vs.self_deaf, p.background_tertiary, p)
            .role("img")
            .label(store.display_name(guild, user))
            .participant_menu(user, guild, acc);
        list = list.child(row().w(size + 12.0).h(size).center().child(face).key(format!("a-{user}")));
    }
    row().key("audio-avatars").w_full().h_full().center().px((vw * 0.06).clamp(16.0, 64.0)).child(list)
}

/// `VoiceRegionTeleportOverlay`: while a call moves to another region.
pub fn teleport_overlay(p: &Palette) -> El {
    let label = t("Teleporting you to another voice region...");
    let _ = p;
    el()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .center()
        .fill(rgba(0, 0, 0, 0.72))
        .role("alert")
        .label(label)
        .child(
            col()
                .items_center()
                .gap(12.0)
                .padding(24.0)
                .radius(16.0)
                .fill(surface_2())
                .border(1.0, hud_border())
                .child(icon(&icons::GLOBE_FILL, 36.0).color(rgba(255, 255, 255, 0.72)))
                .child(text(label).font(font(15.0, 600.0).lh(19.0)).color(Color::WHITE)),
        )
}

/// `VoiceCallView` for the connected channel, in place of the chat column: the tile area on
/// black, the header and control bar over gradients at its edges (floating over the tiles in the
/// focus layout), and the pop-out / fullscreen corner.
pub fn view<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let (guild, channel) = st.voice.channel?;
    let people = participants(store, guild, channel);
    let name = store.channels.get(&channel).map(|c| super::chat::channel_title(store, c)).unwrap_or_default();
    let hud = hud_visible(st);
    let focus = focus_info(store, st, &people);
    let header_h = dim::LAYOUT_HEADER_HEIGHT;
    let footer_h = rune::memo::layout_size("call-footer").map_or(78.0, |[_, h]| h);
    let vw = rune::memo::layout_size(super::BODY_KEY).map_or(1280.0, |[w, _]| w);
    // `--voice-header-gradient` / `--voice-footer-gradient`, running 2.5rem past the chrome.
    let stops = [(0.0, rgba(0, 0, 0, 0.84)), (0.38, rgba(0, 0, 0, 0.58)), (0.7, rgba(0, 0, 0, 0.24)), (1.0, rgba(0, 0, 0, 0.0))];
    let mut left = row().grow(1.0).min_w(0.0).center_y();
    // `.backButtonDesktop` hides from 768px up.
    if vw < 768.0 {
        left = left.child(header_button(&icons::LIST_BOLD, t("Back to voice channel"), false, move |s: &mut S| acc(s).voice.call_view = false).mr(dim::SPACING_2).key("back"));
    }
    left = left.child(icon(&icons::SPEAKER_HIGH_FILL, 24.0).color(Color::WHITE)).child(text(name).font(font(16.0, 500.0).lh(24.0)).color(Color::WHITE).truncate().shrink().ml(dim::SPACING_3));
    let mut right = row().gap(dim::SPACING_2).center_y().min_w(0.0).shrink();
    if let Some(user) = focus.as_ref().and_then(|f| f.screen) {
        // `.voiceCallHeaderSupplement`: at most min(50%, 26rem) of the header.
        let half = rune::memo::layout_size("call-view").map_or(640.0, |[w, _]| w / 2.0);
        right = right.child(stream_focus_info(store, st, guild, user, p, acc).max_w(half.min(416.0)).key("stream-focus"));
    }
    // shortcut: reflux keeps favorites for community channels only, so a DM call has no star.
    if let Some(g) = guild.filter(|_| crate::look::with(|l| l.show_favorites)) {
        let fav = crate::nav::favorites::is_favorite(st, channel);
        right = right.child(header_button(if fav { &icons::STAR_FILL } else { &icons::STAR_BOLD }, if fav { t("Remove from favorites") } else { t("Add to favorites") }, fav, move |s: &mut S| crate::nav::favorites::toggle(acc(s), g, channel)).key("favorite"));
    }
    if let Some(pill) = connection_pill(st, p) {
        right = right.child(pill.key("connection"));
    }
    right = right.child(header_button(&icons::CHART_BAR_FILL, t("Connection stats"), st.popups.is_open(DETAILS), move |s: &mut S| acc(s).popups.toggle(DETAILS)).key(DETAILS));
    let inbox_open = st.social.inbox_open;
    right = right.child(header_button(&crate::social::INBOX_ICON, t("Inbox"), inbox_open, move |s: &mut S| crate::social::toggle_inbox(acc(s))).key(crate::social::INBOX));
    let header = el()
        .key("call-header")
        .pin(Some(0.0), Some(0.0), Some(0.0), None)
        .h(header_h)
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(-40.0)).linear_gradient(std::f32::consts::PI, stops).inert())
        .child(row().w_full().h_full().px(dim::SPACING_4).gap(dim::SPACING_4).center_y().child(left).child(right.key("right")));
    let footer = el()
        .key("call-footer")
        .pin(Some(0.0), None, Some(0.0), Some(0.0))
        .child(el().pin(Some(0.0), Some(-40.0), Some(0.0), Some(0.0)).linear_gradient(0.0, stops).inert())
        .child(row().w_full().px(24.0).pt(8.0).pb(12.0).justify_center().child(control_bar(store, st, p, acc)));
    // `VoiceCallCornerControls`: the focused stream's volume, pop out and fullscreen.
    let mut corner = row().pin(None, None, Some((vw * 0.02).clamp(16.0, 32.0)), Some(12.0)).gap(dim::SPACING_2).center_y();
    if let Some(user) = focus.as_ref().and_then(|f| f.screen).filter(|u| st.voice.watching == Some(*u)) {
        // shortcut: `MediaVerticalVolumeControl`'s slider isn't here; the button mutes the stream.
        let muted = st.voice.settings.stream_mutes.contains(&user.0);
        corner = corner.child(
            corner_button(if muted { &icons::SPEAKER_SLASH_FILL } else { &icons::SPEAKER_HIGH_FILL }, if muted { t("Unmute stream") } else { t("Mute stream") }, move |s: &mut S| {
                let sh = acc(s);
                if !sh.voice.settings.stream_mutes.remove(&user.0) {
                    sh.voice.settings.stream_mutes.insert(user.0);
                }
                sh.intents.push(Intent::Voice(Op::Settings));
            })
            .key("stream-volume"),
        );
    }
    corner = corner
        .child(corner_button(&icons::ARROW_SQUARE_OUT_BOLD, t("Pop out call"), op(acc, Op::PopOut { user: None, screen: false, pip: false })).key("popout"))
        .child(
            corner_button(if st.call.app_fullscreen { &icons::CORNERS_IN_BOLD } else { &icons::CORNERS_OUT_BOLD }, if st.call.app_fullscreen { t("Exit fullscreen") } else { t("Enter fullscreen") }, move |s: &mut S| {
                let sh = acc(s);
                if sh.call.app_fullscreen { exit_fullscreen(sh) } else { sh.call.app_fullscreen = true }
            })
            .key("fullscreen"),
        );
    let me = store.my_id();
    let has_tiles = call_tiles(st, me, &people).iter().any(|t| t.screen) || st.call.show_non_video || people.iter().any(|(u, _)| if *u == me { st.voice.camera } else { st.voice.videos.contains_key(u) });
    let mut body = if has_tiles { layout(store, st, guild, &people, false, p, acc) } else { avatar_fallback(store, st, guild, &people, p, acc) };
    if let Some(until) = st.call.teleport_until
        && until > Instant::now()
    {
        // shortcut: no region-teleport event from the engine; the overlay runs for a few seconds
        // after the region PATCH. Replace with the reconnect event when reflux-voice exposes it.
        body = el().key("teleport").w_full().h_full().child(body).child(teleport_overlay(p));
    }
    // `.mainContent`: between the chrome with 48px kept clear above and below in the grid; the
    // focus layout fills the view (below the header while its members row shows).
    let main = match &focus {
        Some(f) => el().pin(Some(0.0), Some(if f.members_row { header_h } else { 0.0 }), Some(0.0), Some(0.0)),
        None => el().pin(Some(0.0), Some(header_h), Some(0.0), Some(footer_h)).py(48.0),
    };
    // `--voice-hud-opacity`: idle chrome fades out over the tiles.
    let chrome = |e: El| if hud { e } else { e.opacity(0.0).inert() };
    let root = el()
        .key("call-view")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .fill(Color::BLACK)
        .child(main.key("main").clip_rounded(0.0).child(body))
        .child(chrome(header))
        .child(chrome(footer))
        .child(chrome(corner.key("corner")));
    // `useVoiceCallHudVisibility`: any pointer move shows the chrome for the idle timeout;
    // leaving the call hides it. Moves only re-arm it once it is half spent (no rebuild per move).
    Some(
        root.on_pointer_move_local(move |s: &mut S, _, tx: &mut rune::Tx| {
            let sh = acc(s);
            let now = Instant::now();
            if sh.call.hud_until.is_none_or(|t| t < now + HUD_IDLE / 2) {
                sh.call.hud_until = Some(now + HUD_IDLE);
                tx.wake_after(HUD_IDLE);
            }
        })
        .on_hover_change(move |s: &mut S, inside, tx: &mut rune::Tx| {
            let sh = acc(s);
            sh.call.hud_until = if inside { Some(Instant::now() + HUD_IDLE) } else { Some(Instant::now() - Duration::from_millis(1)) };
            if inside {
                tx.wake_after(HUD_IDLE);
            }
        }),
    )
}

// ---- In-call settings menus (`VoiceSettingsMenus`) ----------------------------------------------

fn device_entries<S: 'static>(devices: Option<&Vec<String>>, current: Option<&str>, input: bool, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let prefix = if input { "input" } else { "output" };
    let mut out = vec![MenuEntry::Item(
        MenuItem::new(format!("{prefix}-default"), t("Default"))
            .keep_open()
            .radio(current.is_none())
            .on_select(move |s: &mut S| {
                let sh = acc(s);
                if input {
                    sh.voice.input = None;
                } else {
                    sh.voice.output = None;
                }
                sh.intents.push(Intent::VoiceDevices);
            }),
    )];
    for name in devices.into_iter().flatten() {
        let pick = name.clone();
        out.push(MenuEntry::Item(
            MenuItem::new(format!("{prefix}-{name}"), name.clone())
                .keep_open()
                .radio(current == Some(name.as_str()))
                .on_select(move |s: &mut S| {
                    let sh = acc(s);
                    if input {
                        sh.voice.input = Some(pick.clone());
                    } else {
                        sh.voice.output = Some(pick.clone());
                    }
                    sh.intents.push(Intent::VoiceDevices);
                }),
        ));
    }
    out
}

fn profile_entries<S: 'static>(current: &str, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    [("voice", "Focused voice"), ("studio", "Direct input"), ("custom", "Custom")]
        .into_iter()
        .map(|(id, label)| {
            MenuEntry::Item(MenuItem::new(format!("profile-{id}"), t(label)).radio(current == id).keep_open().on_select(move |s: &mut S| {
                let sh = acc(s);
                sh.voice.settings.processing = id.into();
                sh.intents.push(Intent::Voice(Op::Settings));
            }))
        })
        .collect()
}

/// `VoiceInputLevelItem`: the live meter under the input volume.
fn meter_entry<S: 'static>(level_db: f32, p: &Palette) -> MenuEntry<S> {
    let level = ((level_db + 60.0) / 60.0).clamp(0.0, 1.0);
    let bar = row()
        .w_full()
        .h(6.0)
        .radius(3.0)
        .fill(p.background_tertiary)
        .child(el().w((196.0 * level).max(0.0)).h_full().radius(3.0).fill(if level > 0.85 { p.status_danger } else { p.status_online }))
        .role("meter")
        .label(t("Input level"));
    MenuEntry::El(Box::new(
        col()
            .w_full()
            .px(8.0)
            .py(6.0)
            .gap(5.0)
            .child(text(t("Input level")).font(font(12.0, 500.0).lh(16.0)).color(p.text_tertiary))
            .child(bar),
    ))
}

fn camera_device_entries<S: 'static>(cameras: Option<&Vec<String>>, current: Option<&str>, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let mut out = vec![MenuEntry::Item(
        MenuItem::new("camera-default", t("Default"))
            .lead(&icons::CAMERA_FILL)
            .keep_open()
            .radio(current.is_none())
            .on_select(move |s: &mut S| {
                let sh = acc(s);
                sh.voice.settings.camera = None;
                sh.intents.push(Intent::Voice(Op::Settings));
                sh.intents.push(Intent::Voice(Op::Preview(false)));
                sh.intents.push(Intent::Voice(Op::Preview(true)));
            }),
    )];
    for name in cameras.into_iter().flatten() {
        let pick = name.clone();
        out.push(MenuEntry::Item(
            MenuItem::new(format!("camera-{name}"), name.clone())
                .lead(&icons::CAMERA_FILL)
                .keep_open()
                .radio(current == Some(name.as_str()))
                .on_select(move |s: &mut S| {
                    let sh = acc(s);
                    sh.voice.settings.camera = Some(pick.clone());
                    sh.intents.push(Intent::Voice(Op::Settings));
                    sh.intents.push(Intent::Voice(Op::Preview(false)));
                    sh.intents.push(Intent::Voice(Op::Preview(true)));
                }),
        ));
    }
    out
}

/// `StreamSettingsMenuContent`'s mode radios: a title and its `.modeDescription`.
fn screen_mode_entries<S: 'static>(current: &str, higher: bool, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let gaming = if higher { "Smooth 60 FPS at up to 1440p" } else { "Smooth 30 FPS at up to 720p" };
    let screenshare = if higher { "Sharp text at up to 1080p, 30 FPS" } else { "Sharp text at 720p, up to 30 FPS" };
    [("gaming", "Gaming", gaming), ("screenshare", "Screen share", screenshare), ("custom", "Custom", "Dial in your own numbers")]
        .into_iter()
        .map(|(id, label, desc)| {
            MenuEntry::Item(MenuItem::new(format!("screen-mode-{id}"), t(label)).desc(t(desc)).radio(current == id).keep_open().on_select(move |s: &mut S| {
                let sh = acc(s);
                if sh.voice.settings.streaming_mode != id {
                    sh.voice.settings.streaming_mode = id.into();
                    sh.intents.push(Intent::Voice(Op::Settings));
                }
            }))
        })
        .collect()
}

/// `openVoiceVideoSettings`: the Voice & Video tab.
fn voice_settings_item<S: 'static>(id: &str, label: &str, acc: ShellAccess<S>) -> MenuItem<S> {
    MenuItem::new(id, label).lead(&icons::GEAR_FILL).on_select(move |s: &mut S| {
        let sh = acc(s);
        sh.popups.close();
        sh.open_settings(Some("voice_video"));
    })
}

/// The camera's in-call menu (`VoiceCameraSettingsMenu`). Camera quality lives in the preview
/// modal and the Video settings it links to, as in Fluxer.
fn camera_menu<S: 'static>(st: &ShellState, _p: &Palette, acc: ShellAccess<S>) -> Menu<S> {
    let v = &st.voice;
    let cameras = v.cameras.as_ref();
    let current = v.settings.camera.as_deref();
    Menu::new(MENU_CAMERA)
        .placement(Placement::Top)
        .entry(MenuEntry::SubmenuHint {
            id: "cameras".into(),
            label: t("Camera").to_string(),
            hint: Some(current.unwrap_or(t("Default")).to_string()),
            disabled: false,
            entries: camera_device_entries(cameras, current, acc),
        })
        .item(MenuItem::new("mirror-camera", t("Mirror camera")).checkbox(v.settings.mirror_camera).keep_open().on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.settings.mirror_camera ^= true;
            sh.intents.push(Intent::Voice(Op::Settings));
        }))
        .separator()
        .item(MenuItem::new("preview-camera", t("Preview camera")).lead(&icons::EYE_FILL).on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.modal = Some(Modal::CameraPreview);
            sh.intents.push(Intent::Voice(Op::LoadCameras));
            sh.intents.push(Intent::Voice(Op::Preview(true)));
        }))
        .separator()
        .item(voice_settings_item("video-settings", t("Video settings"), acc))
}

/// The screen share control's menu: `ActiveScreenShareMenu` while sharing, else
/// `StreamSettingsMenuContent`; both end in "Screen share settings".
fn screen_menu<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Menu<S> {
    let v = &st.voice;
    let mut menu = Menu::new(MENU_SCREEN).placement(Placement::Top);
    if v.screen {
        for e in active_share_entries(st, p, acc) {
            menu = menu.entry(e);
        }
    } else {
        menu = menu.group(t("Streaming mode"));
        for e in screen_mode_entries(&v.settings.streaming_mode, st.limits.higher_video_quality, acc) {
            menu = menu.entry(e);
        }
        if v.settings.streaming_mode == "custom" {
            let (fps, res) = stream_quality_entries(st, acc);
            menu = menu.separator().submenu("stream-quality-res", t("Resolution"), res).submenu("stream-quality-fps", t("Frame rate"), fps);
        }
        menu = menu.separator();
        if let Some(e) = stream_audio_entry(st, t("Capture app audio"), acc) {
            menu = menu.entry(e);
        }
        let hide = crate::look::with(|l| l.hide_stream_preview);
        menu = menu.item(MenuItem::new("hide-stream-preview", t("Hide preview thumbnail")).checkbox(hide).keep_open().on_select(move |s: &mut S| {
            let sh = acc(s);
            let mut prefs = sh.prefs.clone();
            prefs.look.hide_stream_preview = !hide;
            sh.intents.push(Intent::Prefs(Box::new(prefs)));
        }));
    }
    menu.separator().item(voice_settings_item("screen-settings", t("Screen share settings"), acc))
}

/// `ActiveScreenShareMenu`: Stop streaming, Change stream and the compact live "Stream
/// quality" submenu (frame rate and resolution groups).
pub fn active_share_entries<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let (fps, res) = stream_quality_entries(st, acc);
    // `.compactLiveGroupLabel`: 14/600 text-tertiary, not uppercase.
    let label = |l: &str| MenuEntry::El(Box::new(text(l.to_string()).font(font(14.0, 600.0).lh(18.0)).color(p.text_tertiary).pt(6.0).px(8.0).pb(4.0)));
    let mut quality = vec![label(t("Frame rate"))];
    quality.extend(fps);
    quality.push(MenuEntry::Separator);
    quality.push(label(t("Resolution")));
    quality.extend(res);
    let mut out = vec![
        MenuEntry::Item(MenuItem::new("stop-stream", t("Stop streaming")).icon(&icons::STOP_CIRCLE_FILL).danger().on_select(move |s: &mut S| acc(s).intents.push(Intent::ToggleScreen))),
        MenuEntry::Item(MenuItem::new("change-stream", t("Change stream")).icon(&icons::MONITOR_PLAY_FILL).on_select(move |s: &mut S| open_screen_picker(acc(s)))),
        MenuEntry::Submenu("stream-quality".into(), t("Stream quality").to_string(), quality),
    ];
    out.extend(stream_audio_entry(st, t("Share stream audio"), acc));
    out
}

/// `StreamSettingsAudioGroup`'s capture checkbox on Reflux's `stream_audio` (the PipeWire app
/// the share sends audio from). Unchecking drops it; a live share restarts without it.
fn stream_audio_entry<S: 'static>(st: &ShellState, label: &str, acc: ShellAccess<S>) -> Option<MenuEntry<S>> {
    if !crate::audio_share::available() {
        return None;
    }
    let on = st.voice.settings.stream_audio.is_some();
    // shortcut: checking it needs an app, which is picked in the share dialog; with none picked
    // the box is disabled. Remember the last app if an on/off capture is wanted here.
    Some(MenuEntry::Item(MenuItem::new("stream-audio", label).checkbox(on).disabled(!on).keep_open().on_select(move |s: &mut S| {
        let sh = acc(s);
        sh.voice.settings.stream_audio = None;
        sh.intents.push(Intent::Voice(Op::Settings));
    })))
}

fn input_menu<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Menu<S> {
    let v = &st.voice;
    let set = &v.settings;
    let inputs = v.devices.as_ref().map(|(i, _)| i);
    let current = v.input.as_deref();
    let level = if v.mic_test || st.call.meter_on { v.level } else { -60.0 };
    let profile = match set.processing.as_str() {
        "studio" => "Direct input",
        "custom" => "Custom",
        _ => "Focused voice",
    };
    Menu::new(MENU_INPUT)
        .placement(Placement::Top)
        .entry(MenuEntry::SubmenuHint {
            id: "input-device".into(),
            label: t("Input device").to_string(),
            hint: Some(current.unwrap_or(t("Default")).to_string()),
            disabled: false,
            entries: device_entries(inputs, current, true, acc),
        })
        .entry(MenuEntry::SubmenuHint {
            id: "input-profile".into(),
            label: t("Input profile").to_string(),
            hint: Some(t(profile).to_string()),
            disabled: false,
            entries: profile_entries(&set.processing, acc),
        })
        .separator()
        .entry(menu_slider("input-volume".into(), t("Input volume"), set.input_volume, p, move |s: &mut S, x| {
            let sh = acc(s);
            sh.voice.settings.input_volume = x;
            sh.intents.push(Intent::Voice(Op::Settings));
        }))
        .entry(meter_entry(level, p))
        .separator()
        .item(voice_settings_item("voice-settings", t("Voice settings"), acc))
}

fn output_menu<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Menu<S> {
    let v = &st.voice;
    let outputs = v.devices.as_ref().map(|(_, o)| o);
    let current = v.output.as_deref();
    Menu::new(MENU_OUTPUT)
        .placement(Placement::Top)
        .entry(MenuEntry::SubmenuHint {
            id: "output-device".into(),
            label: t("Output device").to_string(),
            hint: Some(current.unwrap_or(t("Default")).to_string()),
            disabled: false,
            entries: device_entries(outputs, current, false, acc),
        })
        .separator()
        .entry(menu_slider("output-volume".into(), t("Output volume"), v.settings.output_volume, p, move |s: &mut S, x| {
            let sh = acc(s);
            sh.voice.settings.output_volume = x;
            sh.intents.push(Intent::Voice(Op::Settings));
        }))
        .separator()
        .item(voice_settings_item("voice-settings", t("Voice settings"), acc))
}

/// `fetchCallRegions` for a DM call, through the generic REST intent.
pub fn ensure_regions(sh: &mut ShellState) {
    let Some((None, ch)) = sh.voice.channel else { return };
    let key = format!("voice-regions-{ch}");
    if matches!(sh.expr.fetched.get(&key), Some(super::expr::Fetched::Failed(_))) {
        sh.expr.fetched.remove(&key);
    }
    if sh.expr.fetched.contains_key(&key) {
        return;
    }
    sh.expr.fetched.insert(key.clone(), super::expr::Fetched::Loading);
    sh.intents.push(Intent::Fetch { key, method: reqwest::Method::GET, path: format!("/channels/{ch}/call/regions"), body: None });
}

/// `getRegionDisplayName`: the server's name, else the ID title-cased ("us-east" → "US East").
fn region_display_name(id: &str, name: &str) -> String {
    if !name.is_empty() && name != id {
        return name.to_string();
    }
    id.split('-')
        .map(|part| match part.to_lowercase().as_str() {
            "us" => "US".to_string(),
            "eu" => "EU".to_string(),
            l => l.chars().take(1).flat_map(char::to_uppercase).chain(l.chars().skip(1)).collect(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The region list from `GET /channels/:id/call/regions`: Automatic, then the rest by name.
pub fn region_options(st: &ShellState) -> Vec<(String, String)> {
    let Some((None, ch)) = st.voice.channel else { return Vec::new() };
    let key = format!("voice-regions-{ch}");
    let mut out = Vec::new();
    if let Some(super::expr::Fetched::Done(serde_json::Value::Array(list))) = st.expr.fetched.get(&key) {
        for r in list {
            let Some(id) = r["id"].as_str().filter(|id| *id != "automatic") else { continue };
            out.push((id.to_string(), region_display_name(id, r["name"].as_str().unwrap_or_default())));
        }
    }
    // shortcut: `localeCompare` is approximated by a case-insensitive compare.
    out.sort_by_key(|(_, n)| n.to_lowercase());
    out.insert(0, ("automatic".to_string(), t("Automatic").to_string()));
    out
}

/// Whether the region PATCH is in flight (`isChangingRegion`).
fn region_changing(st: &ShellState, ch: Snowflake) -> bool {
    matches!(st.expr.fetched.get(&format!("voice-region-set-{ch}")), Some(super::expr::Fetched::Loading))
}

fn region_entries<S: 'static>(st: &ShellState, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let Some((_, ch)) = st.voice.channel else { return Vec::new() };
    let current = st.call.region.clone().unwrap_or_else(|| "automatic".into());
    let changing = region_changing(st, ch);
    region_options(st)
        .into_iter()
        .map(|(id, name)| {
            let selected = current == id;
            let pick = id.clone();
            MenuEntry::Item(MenuItem::new(format!("region-{id}"), name).radio(selected).disabled(changing).keep_open().on_select(move |s: &mut S| {
                if selected {
                    return;
                }
                let sh = acc(s);
                sh.call.teleport_until = Some(Instant::now() + Duration::from_secs(3));
                sh.call.region = (pick != "automatic").then(|| pick.clone());
                sh.expr.fetched.insert(format!("voice-region-set-{ch}"), super::expr::Fetched::Loading);
                sh.intents.push(Intent::Fetch {
                    key: format!("voice-region-set-{ch}"),
                    method: reqwest::Method::PATCH,
                    path: format!("/channels/{ch}/call"),
                    body: Some(serde_json::json!({ "region": pick })),
                });
            }))
        })
        .collect()
}

/// `VoiceMoreOptionsMenu`: region (DM calls), grid view (community calls), own camera and
/// screen share, non-video participants, speaker priority, standard-call switch and settings.
/// `CheckboxItem` drops its `icon`, so the checkboxes show none.
fn more_menu<S: 'static>(store: &Store, st: &ShellState, _p: &Palette, acc: ShellAccess<S>) -> Menu<S> {
    let v = &st.voice;
    let mut menu = Menu::new(MENU_MORE).placement(Placement::Top).min_w(300.0);
    let dm = matches!(v.channel, Some((None, _)));
    if let Some((None, ch)) = v.channel {
        let hint = match &st.call.region {
            None => t("Automatic").to_string(),
            Some(id) => region_options(st).into_iter().find(|(i, _)| i == id).map_or_else(|| id.clone(), |(_, n)| n),
        };
        menu = menu.entry(MenuEntry::SubmenuHint { id: "voice-region".into(), label: t("Voice region").to_string(), hint: Some(hint), disabled: region_changing(st, ch), entries: region_entries(st, acc) });
    }
    if !dm {
        let is_grid = st.call.layout == LayoutMode::Grid;
        let first = v.channel.and_then(|(g, c)| participants(store, g, c).first().map(|(u, _)| *u));
        menu = menu.item(MenuItem::new("grid-view", t("Grid view")).checkbox(is_grid).keep_open().on_select(move |s: &mut S| {
            let sh = acc(s);
            if sh.call.layout == LayoutMode::Grid {
                sh.call.set_layout(LayoutMode::Focus);
                sh.voice.focus = first.or(sh.voice.focus);
            } else {
                sh.call.set_layout(LayoutMode::Grid);
                sh.voice.focus = None;
            }
        }));
    }
    menu = menu
        .item(MenuItem::new("show-own-camera", t("Show my own camera")).checkbox(st.call.show_own_camera).on_select(move |s: &mut S| {
            let sh = acc(s);
            if sh.call.show_own_camera {
                if sh.prefs.skip_hide_own_camera_confirm {
                    sh.call.show_own_camera = false;
                } else {
                    sh.call.hide_camera_confirm = Some(false);
                }
            } else {
                sh.call.show_own_camera = true;
            }
        }))
        .item(MenuItem::new("show-own-screen", t("Show my screen share")).checkbox(st.call.show_own_screen).keep_open().on_select(move |s: &mut S| acc(s).call.show_own_screen ^= true))
        .item(MenuItem::new("show-non-video", t("Show non-video participants")).checkbox(st.call.show_non_video).keep_open().on_select(move |s: &mut S| acc(s).call.show_non_video ^= true))
        .item(MenuItem::new("prioritize-speakers", t("Prioritize speakers")).checkbox(st.call.prioritize_speakers).keep_open().on_select(move |s: &mut S| acc(s).call.prioritize_speakers ^= true));
    if let Some((g, c)) = v.channel
        && super::voice::is_p2p(store, g, c)
    {
        menu = menu.item(MenuItem::new("standard-call", t("Switch to standard call")).icon(&icons::ARROWS_LEFT_RIGHT_FILL).on_select(op(acc, Op::SwitchToStandard)));
    }
    menu.separator()
        .item(MenuItem::new("voice-video-settings", t("Voice & video settings")).icon(&icons::GEAR_FILL).on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.popups.close();
            sh.open_settings(Some("voice_video"));
        }))
}

/// The gear menus (`VoiceSettingsMenus`), rendered into the overlay layer while open.
pub fn menus<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if st.voice.channel.is_none() {
        return;
    }
    let pops = pops(acc);
    input_menu(st, p, acc).el(&st.popups, pops, p, layer);
    output_menu(st, p, acc).el(&st.popups, pops, p, layer);
    camera_menu(st, p, acc).el(&st.popups, pops, p, layer);
    screen_menu(st, p, acc).el(&st.popups, pops, p, layer);
    more_menu(store, st, p, acc).el(&st.popups, pops, p, layer);
    participants_popout(store, st, p, acc, layer);
}

/// The `+N` participant popout (`VoiceParticipantAvatarList`'s remaining-content list).
fn participants_popout<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !st.popups.is_open(PARTICIPANTS) {
        return;
    }
    let Some((guild, channel)) = st.voice.channel else { return };
    let people = participants(store, guild, channel);
    let mut rows = col().w_full().gap(2.0);
    for (u, vs) in &people {
        let user = *u;
        rows = rows.child(
            row()
                .w_full()
                .h(32.0)
                .px(6.0)
                .gap(8.0)
                .center_y()
                .radius(6.0)
                .hover(p.background_modifier_hover)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(store.display_name(guild, user))
                .child(avatar(store, st, user, 24.0, None, p.background_tertiary, p))
                .child(text(store.display_name(guild, user)).font(font(14.0, 500.0).lh(18.0)).color(p.text_primary).truncate().grow(1.0))
                .child(if vs.mute || vs.self_mute { icon(&icons::MICROPHONE_SLASH_FILL, 14.0).color(p.status_danger) } else { el() })
                .child(if vs.deaf || vs.self_deaf { icon(&icons::SPEAKER_SLASH_FILL, 14.0).color(p.status_danger) } else { el() })
                .on_press(move |s: &mut S| {
                    let sh = acc(s);
                    sh.voice.focus = Some(user);
                    sh.call.pin(Some(user));
                    sh.popups.close();
                })
                .participant_menu(user, guild, acc),
        );
    }
    let surface = col()
        .w(240.0)
        .padding(8.0)
        .gap(2.0)
        .radius(8.0)
        .fill(p.background_secondary)
        .border(1.0, p.background_modifier_accent)
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .role("dialog")
        .label(t("Voice participants"))
        .child(text(t("Voice participants")).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary).px(6.0).pb(4.0))
        .child(rows)
        .modal(true, move |s: &mut S| acc(s).popups.close());
    layer.push(anchor(surface, PARTICIPANTS, Placement::BottomStart, 8.0));
}

/// Leaves app fullscreen, back to the compact view when it was entered from there (the compact
/// view's fullscreen is the same element going fullscreen in Fluxer).
pub fn exit_fullscreen(sh: &mut ShellState) {
    sh.call.app_fullscreen = false;
    if std::mem::take(&mut sh.call.fullscreen_from_compact) {
        sh.voice.call_view = false;
    }
}

/// Keeps the menus' side effects in step each frame: the input menu runs the mic meter.
pub fn poll(st: &mut ShellState) {
    let channel = st.voice.channel.map(|(_, c)| c);
    if channel != st.call.region_channel {
        st.call.region_channel = channel;
        st.call.region = None;
    }
    if channel.is_none() {
        st.call.app_fullscreen = false;
        st.call.fullscreen_from_compact = false;
        st.call.hide_camera_confirm = None;
        st.call.stats = None;
    }
    // `gridTracks`' speaker promotion: who spoke within `SPEAKER_HOLD`, in first-spoke order.
    // shortcut: a hold that runs out re-sorts on the next rebuild, not on a timer of its own.
    let now = Instant::now();
    let call = &mut st.call;
    if call.prioritize_speakers && st.voice.connected {
        for u in &st.voice.speaking {
            let order = &mut call.next_order;
            call.activity.entry(*u).and_modify(|a| a.0 = now).or_insert_with(|| {
                *order += 1;
                (now, *order - 1)
            });
        }
        call.activity.retain(|_, a| now.duration_since(a.0) <= SPEAKER_HOLD);
    } else {
        call.activity.clear();
    }
    if call.activity.is_empty() {
        call.next_order = 0;
    }
    let meter = st.popups.is_open(MENU_INPUT) && st.voice.channel.is_some();
    if meter != st.call.meter_on {
        st.call.meter_on = meter;
        st.intents.push(Intent::Voice(Op::MicTest(meter)));
    }
    if st.voice.connected {
        if st.call.connected_at.is_none() {
            st.call.connected_at = Some(Instant::now());
        }
    } else {
        st.call.connected_at = None;
    }
    // The mic/output menus list the OS devices; the camera menu lists capture devices.
    let audio_menu = st.popups.is_open(MENU_INPUT) || st.popups.is_open(MENU_OUTPUT);
    if audio_menu && st.voice.devices.is_none() && !st.call.devices_loading {
        st.call.devices_loading = true;
        st.intents.push(Intent::LoadDevices);
    } else if !audio_menu {
        st.call.devices_loading = false;
    }
    let camera_menu = st.popups.is_open(MENU_CAMERA);
    if camera_menu && st.voice.cameras.is_none() && !st.call.cameras_loading {
        st.call.cameras_loading = true;
        st.intents.push(Intent::Voice(Op::LoadCameras));
    } else if !camera_menu {
        st.call.cameras_loading = false;
    }
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

/// `getCallParticipantLayoutMetrics`: the largest avatar (40..88) whose wrapped rows fit the
/// row's box, with a gap of an 18th of its smaller side (8..16).
fn call_avatar_metrics(count: usize, w: f32, h: f32) -> (f32, f32) {
    if count == 0 || w <= 0.0 || h <= 0.0 {
        return (64.0, 12.0);
    }
    let (w, h) = (w.max(40.0), h.max(40.0));
    let gap = (w.min(h) / 18.0).round().clamp(8.0, 16.0);
    let mut best = 40.0f32;
    for cols in 1..=count {
        let rows = count.div_ceil(cols);
        let by_w = (w - gap * (cols - 1) as f32) / cols as f32;
        let by_h = (h - gap * (rows - 1) as f32) / rows as f32;
        best = best.max(by_w.min(by_h).min(88.0).floor());
    }
    (best.clamp(40.0, 88.0), gap)
}

/// `CallParticipantsRow`: us, the call's participants, then the recipients being rung, as
/// wrapped avatars sized to the row. Someone being rung is dimmed with a pulsing ring.
/// Right-click opens the voice participant menu, or the user menu for someone not in the call.
#[allow(clippy::too_many_arguments)]
fn call_participants_row<S: 'static>(store: &Store, st: &ShellState, c: &Channel, ringing: &[Snowflake], people: &[(Snowflake, &VoiceState)], surface: Color, p: &Palette, acc: ShellAccess<S>) -> El {
    let me = store.my_id();
    let in_call = |u: Snowflake| people.iter().find(|(x, _)| *x == u).map(|(_, v)| *v);
    let mut ids: Vec<Snowflake> = Vec::new();
    for u in std::iter::once(me).chain(people.iter().map(|(u, _)| *u)).chain(c.recipients.iter().map(|u| u.id)).chain(ringing.iter().copied()) {
        if !ids.contains(&u) && (in_call(u).is_some() || ringing.contains(&u)) {
            ids.push(u);
        }
    }
    let key = format!("call-participants-{}", c.id);
    let [w, h] = rune::memo::layout_size(&key).unwrap_or([0.0, 0.0]);
    let (size, gap) = call_avatar_metrics(ids.len(), w, h);
    let mut list = row().w_full().flex_wrap().justify_center().items_center().gaps(gap, gap);
    for u in ids {
        let name = store.display_name(None, u);
        let face = match in_call(u) {
            Some(vs) => super::voice::voice_avatar(store, st, u, size, st.voice.speaking.contains(&u), vs.mute || vs.self_mute, vs.deaf || vs.self_deaf, surface, p).participant_menu(u, None, acc),
            None => {
                // `.callParticipantRinging`: a 2.2s white ring spreading max(8, 18%) and fading.
                // shortcut: Rune loops opacity only, so the ring fades in and out at half its spread
                // instead of growing; brightness(.55) saturate(.85) is a .55 dim over the dark banner.
                let spread = (size * 0.18).round().max(8.0) / 2.0;
                el().size(size, size)
                    .child(el().pin(Some(-spread), Some(-spread), Some(-spread), Some(-spread)).radius(999.0).fill(rgba(255, 255, 255, 0.25)).looping(rune::LoopSpec::opacity(2.2, 0.0, 0.0, 1.0)))
                    .child(avatar(store, st, u, size, None, surface, p).dim(0.55))
                    .on_secondary_press(move |s: &mut S, at, _| acc(s).open_menu(super::Target::User { user: u, guild: None, dm: None }, [at.x, at.y]))
            }
        };
        list = list.child(face.key(format!("p-{u}")).role("button").label(name));
    }
    row().key(key).w_full().h_full().min_h(0.0).center().role("group").label(t("Call participants")).child(list)
}

/// `DMChannelView`'s call surface: once this device is in the call, the compact call view
/// (`ChannelCompactCallSurface`) with `CallParticipantsRow` as its avatar fallback; before that
/// the resizable `.callBanner`: the participants row over `CallControls`.
pub fn dm_strip<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> Option<El> {
    if !matches!(c.kind, ChannelType::DM | ChannelType::GROUP_DM) {
        return None;
    }
    let call = store.calls.get(&c.id)?;
    let ch = c.id;
    let people = participants(store, None, ch);
    let me = store.my_id();
    let here = st.voice.channel == Some((None, ch));
    // `callExistsAndOngoing`: someone's in it, someone's being rung, or we started it.
    if people.is_empty() && call.ringing.is_empty() && !here {
        return None;
    }
    if here && st.voice.connected {
        // `.compactCallParticipantsRow`: at most 42rem wide.
        let fallback = call_participants_row(store, st, c, &call.ringing, &people, rgb(0, 0, 0), p, acc).max_w(672.0);
        return Some(compact_call(store, st, None, ch, &people, Some(fallback), p, acc));
    }
    // `useCallHeaderState`'s controls variant.
    let incoming = call.ringing.contains(&me) && !st.voice.dismissed_calls.contains(&ch);
    let mut controls = row().gaps(12.0, 12.0).flex_wrap().justify_center().items_center();
    controls = if here {
        controls.child(Button::<S>::new(t("Connecting...")).variant(ButtonVariant::Secondary).icon(&icons::PHONE_FILL).submitting(true).key("dm-call-connecting").el(p))
    } else if incoming {
        // shortcut: Button draws every icon at 16px; Reject's X and Ignore's bell are 18px in Fluxer.
        controls
            .child(Button::new(t("Accept")).variant(ButtonVariant::Secondary).icon(&icons::PHONE_FILL).key("dm-call-accept").on_press(op(acc, Op::AcceptCall(ch))).el(p))
            .child(Button::new(t("Reject")).variant(ButtonVariant::Danger).icon(&icons::X_BOLD).key("dm-call-reject").on_press(op(acc, Op::DeclineCall(ch))).el(p))
            .child(Button::new(t("Ignore")).variant(ButtonVariant::Secondary).icon(&icons::BELL_SLASH_FILL).key("dm-call-ignore").on_press(ignore_call(acc, ch, me)).el(p))
    } else if people.iter().any(|(u, _)| *u == me) {
        controls.child(Button::new(t("Join on this device")).variant(ButtonVariant::Secondary).icon(&icons::PHONE_FILL).key("dm-call-join-here").on_press(op(acc, Op::AcceptCall(ch))).el(p))
    } else {
        controls.child(Button::new(t("Join call")).icon(&icons::PHONE_FILL).key("dm-join-call").on_press(op(acc, Op::AcceptCall(ch))).el(p))
    };
    let mut inner = col().w_full().gap(8.0).items_center().child(controls);
    // `VoiceE2EEIndicator variant="call"`: "P2P" for a peer-to-peer call; a DM call is only
    // known to be encrypted once connecting to it.
    if super::voice::is_p2p(store, None, ch) {
        let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
        let tip = crate::ui::Tooltip::new("dm-call-p2p", t("Direct connection. Audio and video go directly between the people in the call, who can see each other's IP address."));
        inner = inner.child(tip.el(p2p_indicator(here && !st.voice.p2p_failed.is_empty(), p), &st.popups, pops, p, layer));
    } else if here && st.voice.encrypted {
        inner = inner.child(e2ee_badge());
    }
    // `.callBanner` (`--voice-surface-0`) in `.compactCallWrapper` (black, a 1px
    // `--voice-overlay-light` bottom border), resized like the compact call view.
    let banner = col()
        .w_full()
        .h_full()
        .fill(surface_0())
        .child(row().w_full().grow(1.0).min_h(0.0).center().px(16.0).py(12.0).child(call_participants_row(store, st, c, &call.ringing, &people, surface_0(), p, acc)))
        .child(row().w_full().justify_center().px(16.0).pb(24.0).child(inner));
    // `.compactCallResizePill`: 36x4, `--voice-overlay-light-strong`.
    let handle = compact_resize_handle(ch, acc).child(row().w_full().h_full().center().child(el().size(36.0, 4.0).radius(999.0).fill(rgba(255, 255, 255, 0.2))));
    Some(
        col()
            .key(format!("dm-call-{ch}"))
            .w_full()
            .h(compact_call_height(st, ch))
            .fill(rgb(0, 0, 0))
            .child(banner)
            .child(handle)
            .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(rgba(255, 255, 255, 0.12))),
    )
}

/// `CallCommands.ignoreCall`: stop ringing just us.
fn ignore_call<S: 'static>(acc: ShellAccess<S>, ch: Snowflake, me: Snowflake) -> impl Fn(&mut S) + 'static {
    move |s: &mut S| {
        let sh = acc(s);
        sh.voice.dismissed_calls.insert(ch);
        sh.intents.push(Intent::Voice(Op::Ring(ch, me, false)));
    }
}

/// The compact call's height: its saved height, kept under `getCompactHeightMax` (the window
/// leaves room for the chat under it).
fn compact_call_height(st: &ShellState, ch: Snowflake) -> f32 {
    let window_h = rune::memo::layout_size(super::BODY_KEY).map_or(800.0, |[_, h]| h);
    let max_h = (window_h - COMPACT_CHAT_RESERVE).clamp(COMPACT_HEIGHT_MIN, COMPACT_HEIGHT_MAX);
    compact_height(st.prefs.compact_call_heights.get(&ch).copied().unwrap_or(COMPACT_HEIGHT_DEFAULT)).min(max_h)
}

/// `.resizeHandle` / `.compactCallResizeHandle`: the 16px strip along the bottom edge; dragging
/// or the arrow keys resize the call and save its height.
fn compact_resize_handle<S: 'static>(ch: Snowflake, acc: ShellAccess<S>) -> El {
    el()
        .key(format!("compact-call-resize-{ch}"))
        .pin(Some(0.0), None, Some(0.0), Some(0.0))
        .h(16.0)
        .cursor(rune::Cursor::ResizeRow)
        .role("separator")
        .label(t("Resize call view"))
        .focusable(true)
        .on_step(move |s: &mut S, delta, _| {
            let sh = acc(s);
            let h = sh.prefs.compact_call_heights.get(&ch).copied().unwrap_or(COMPACT_HEIGHT_DEFAULT);
            sh.prefs.compact_call_heights.insert(ch, compact_height(h + delta as f32 * 16.0));
            let prefs = sh.prefs.clone();
            sh.intents.push(Intent::Prefs(Box::new(prefs)));
        })
        .on_drag(move |s: &mut S, d: &mut rune::Drag, _| {
            let sh = acc(s);
            if d.phase == rune::Phase::Down {
                d.data = compact_height(sh.prefs.compact_call_heights.get(&ch).copied().unwrap_or(COMPACT_HEIGHT_DEFAULT));
            }
            sh.prefs.compact_call_heights.insert(ch, compact_height(d.data + d.dy));
            if d.phase == rune::Phase::Up {
                let prefs = sh.prefs.clone();
                sh.intents.push(Intent::Prefs(Box::new(prefs)));
            }
        })
}

/// Whether the joined compact call shows only avatars (`data-voice-call-avatar-mode`): no
/// screen share and no camera on (`hasRenderableCallMedia`).
pub fn compact_avatar_mode(store: &Store, st: &ShellState, guild: Option<Snowflake>, ch: Snowflake) -> bool {
    let me = store.my_id();
    let people = participants(store, guild, ch);
    let visible = visible_people(&people, me, st.call.show_non_video, &st.voice.videos, &st.voice.speaking);
    !tiles(&visible).iter().any(|t| t.screen || video(st, me, *t).is_some())
}

/// `CompactVoiceCallView`: the joined call above a channel's chat — the participants' avatars
/// (or `avatar_fallback`), or the call layout once someone has a camera or stream on, with the
/// control bar, the chat / pop-out / fullscreen corner buttons and the resize handle.
#[allow(clippy::too_many_arguments)]
fn compact_call<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, ch: Snowflake, people: &[(Snowflake, &VoiceState)], avatar_fallback: Option<El>, p: &Palette, acc: ShellAccess<S>) -> El {
    let height = compact_call_height(st, ch);
    let width = rune::memo::layout_size(&format!("compact-call-{ch}")).map_or(640.0, |[w, _]| w);
    let me = store.my_id();
    let visible = visible_people(people, me, st.call.show_non_video, &st.voice.videos, &st.voice.speaking);
    let visible = order_participants(&visible, &st.voice.speaking, st.call.prioritize_speakers);
    let tiles_all = tiles(&visible);
    // `hasRenderableCallMedia`: any screen share, or a camera that's on.
    let has_media = tiles_all.iter().any(|t| t.screen || video(st, me, *t).is_some());
    // `CompactVoiceCallView.module.css`'s reserves: the edge padding, the control bar's
    // (56px buttons + padding) and what the content keeps clear of it.
    let edge = (height * 0.045).clamp(12.0, 24.0);
    let control_h = 58.0 + (height * 0.04).clamp(16.0, 24.0) + 12.0;
    let control_reserve = control_h + (height * 0.055).clamp(16.0, 28.0);
    let footer = control_reserve + (height * 0.052).clamp(14.0, 22.0);
    let pad_bottom = edge.max(footer.min(height * 0.34));
    let pad_top = edge.max((height * 0.065).clamp(18.0, 28.0).min(control_reserve * 0.34));
    // `.gridLayoutWrapperCompact`: its box starts at the window top (23 above this strip, under
    // the floating header), so its safe top is 30% and its safe bottom 34% of that taller box;
    // measured in Fluxer at a 320 call: 103.2 top, 116.96 bottom, 124 of grid (tiles 202×114).
    // Reflux's strip starts below the header, so the 23 is taken off the top.
    let host_h = height + 23.0;
    let safe_top = host_h * 0.3 - 23.0;
    let safe_bottom = host_h * 0.34;
    let grid = has_media && !people.is_empty();
    let (px, pt, pb) = if grid { (0.0, safe_top, safe_bottom) } else { (edge, pad_top, pad_bottom) };
    let mut content = row().w_full().grow(1.0).min_h(0.0).center().fill(rgb(0, 0, 0)).px(px).pt(pt).pb(pb);
    if people.is_empty() {
        content = content.child(text(t("No one else is here yet.")).font(font(14.0, 500.0).lh(18.0)).color(rgba(255, 255, 255, 0.55)).key("empty"));
    } else if !has_media && let Some(fallback) = avatar_fallback {
        content = content.child(fallback.key("avatars"));
    } else if !has_media {
        // `audioAvatarLayout` + `VoiceParticipantWrappedAvatarList`: wrapping avatars,
        // clamp(44, 18% of the smaller side, 88), with their mute / deafen badges.
        let content_h = height - pad_top - pad_bottom;
        let size = (width.min(content_h + pad_top + pad_bottom) * 0.18).clamp(44.0, 88.0);
        let gap = (height * 0.024).clamp(8.0, 16.0);
        let mut list = row().w_full().max_w(768.0).flex_wrap().justify_center().items_center().gaps(gap, gap);
        for (u, vs) in &visible {
            let user = *u;
            let name = store.display_name(guild, user);
            let face = super::voice::voice_avatar(store, st, user, size, st.voice.speaking.contains(&user), vs.mute || vs.self_mute, vs.deaf || vs.self_deaf, rgb(0, 0, 0), p)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(name)
                .on_press(move |s: &mut S| {
                    let sh = acc(s);
                    let next = (sh.call.pinned(sh.voice.focus) != Some(user)).then_some(user);
                    sh.voice.focus = next;
                    sh.call.pin(next);
                });
            list = list.child(face.key(format!("a-{user}")));
        }
        content = content.child(list.key("avatars"));
    } else {
        // `VoiceCallLayoutContent compact`: the call view's grid / focus layout, whose tiles
        // shrink to fit (a single row of fixed-minimum tiles overflowed the window).
        content = content.child(col().key("layout").w_full().h_full().min_w(0.0).child(layout(store, st, guild, people, true, p, acc)));
    }
    // `.heightTogglePill` / `.fullscreenButtonWrap button`: 42px black squares, radius 14.
    let corner = |key: &'static str, glyph: &'static rune::Icon, label: String| {
        row()
            .key(key)
            .size(42.0, 42.0)
            .center()
            .radius(14.0)
            .fill(rgb(0, 0, 0))
            .border(1.0, rgba(255, 255, 255, 0.12))
            .shadow(3.0, 10.0, rgba(0, 0, 0, 0.26))
            .hover(rgb(0x11, 0x11, 0x11))
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(label)
            .hover_group()
            .child(icon(glyph, 21.0).color(rgba(255, 255, 255, 0.9)).group_color(Color::WHITE))
    };
    let unread = unread_messages(store, ch);
    let mut chat = corner("voice-chat-toggle", &icons::CHAT_TEARDROP_FILL, t("Hide chat").to_string()).on_press(move |s: &mut S| acc(s).voice.call_view = true);
    if unread > 0 {
        chat = chat.child(
            row()
                .pin(None, Some(-6.4), Some(-6.4), None)
                .min_w(14.0)
                .h(14.0)
                .px(4.0)
                .center()
                .radius(999.0)
                .fill(p.status_danger)
                .border(1.5, rgb(0, 0, 0))
                .child(text(if unread > 9 { "9+".to_string() } else { unread.to_string() }).font(font(9.0, 700.0).lh(9.0)).color(Color::WHITE)),
        );
    }
    let popout = corner("compact-call-popout", &icons::ARROW_SQUARE_OUT_BOLD, t("Pop out call").to_string()).on_press(op(acc, Op::PopOut { user: None, screen: false, pip: false }));
    let full = corner("compact-call-fullscreen", &icons::CORNERS_OUT_BOLD, t("Enter fullscreen").to_string()).on_press(move |s: &mut S| {
        let sh = acc(s);
        sh.voice.call_view = true;
        sh.call.app_fullscreen = true;
        sh.call.fullscreen_from_compact = true;
    });
    let controls = row().pin(Some(0.0), None, Some(0.0), Some(0.0)).pt((height * 0.04).clamp(16.0, 24.0)).px(16.0).pb(12.0).justify_center().child(control_bar(store, st, p, acc));
    col()
        .key(format!("compact-call-{ch}"))
        .w_full()
        .h(height)
        .fill(rgb(0, 0, 0))
        .clip_rounded(0.0)
        .child(content)
        .child(controls)
        .child(el().pin(Some(12.0), None, None, Some(12.0)).child(chat))
        .child(row().pin(None, None, Some(12.0), Some(12.0)).gap(8.0).child(popout).child(full))
        .child(compact_resize_handle(ch, acc))
        // `.compactCallWrapper`'s 1px `--voice-overlay-light` bottom border.
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(rgba(255, 255, 255, 0.12)))
}

/// `ChannelCompactCallSurface`: the call above a voice channel's text chat — the channel's
/// participants and a Join button, or, once joined, the compact call view. The surface resizes
/// by its bottom handle and remembers that height per channel.
pub fn guild_strip<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if c.kind != ChannelType::GUILD_VOICE {
        return None;
    }
    let guild = c.guild_id?;
    let people = participants(store, Some(guild), c.id);
    let joined = st.voice.channel == Some((Some(guild), c.id));
    let ch = c.id;
    if joined {
        return Some(compact_call(store, st, Some(guild), ch, &people, None, p, acc));
    }
    let mut strip = col().w_full().pt(12.0).pb(10.0).gap(10.0).items_center().fill(surface_1());
    let mut faces = row().gap(6.0).center_y();
    for (u, _) in people.iter().take(8) {
        faces = faces.child(avatar(store, st, *u, 32.0, None, surface_3(), p).key(format!("f-{u}")));
    }
    let mut left = row().gap(12.0).center_y().child(faces);
    if !people.is_empty() {
        left = left.child(text(format!("{} in voice", people.len())).font(font(14.0, 500.0).lh(18.0)).color(rgba(255, 255, 255, 0.72)));
    }
    let join = Button::new(t("Join voice channel"))
        .icon(&icons::SPEAKER_HIGH_FILL)
        .key("voice-channel-join")
        .on_press(move |s: &mut S| acc(s).join_voice(Some(guild), ch))
        .el(p);
    strip = strip.child(el().w_full().h(112.0).px(24.0).child(row().w_full().h_full().justify_center().gap(24.0).children([left, el().grow(1.0), join])));
    Some(strip)
}

/// `CallButtons`: the DM header's voice and video call buttons. Voice starts, joins or (selected,
/// in the call) leaves it; video starts the call with the camera or joins it, and once in the
/// call turns the camera on or off (selected while it's on). A system DM can't host calls.
pub fn header_action<S: 'static>(store: &Store, st: &ShellState, c: &Channel, video: bool, action: &dyn Fn(&'static rune::Icon, &str, bool) -> El, acc: ShellAccess<S>) -> El {
    let ch = c.id;
    let active = store.calls.contains_key(&ch);
    let in_call = st.voice.connected && st.voice.channel == Some((None, ch));
    let count = participants(store, None, ch).len();
    let system = c.kind == ChannelType::DM && c.recipients.iter().any(|u| u.id == super::client_system::FLUXERBOT);
    let label = match video {
        _ if system => t(if video { "System DMs cannot host video calls" } else { "System DMs cannot host voice calls" }).to_string(),
        false if in_call => t("Leave voice call").to_string(),
        false if active => t("Join voice call").to_string(),
        false => t("Start voice call").to_string(),
        true if in_call => t(if st.voice.camera { "Turn off camera" } else { "Turn on camera" }).to_string(),
        true if active && count > 0 => crate::i18n::tp("{count, plural, one {Join video call (# participant)} other {Join video call (# participants)}}", count as i64, &[]),
        true if active => t("Join video call").to_string(),
        true => t("Start video call").to_string(),
    };
    let selected = in_call && (!video || st.voice.camera);
    let b = action(if video { &icons::VIDEO_CAMERA_FILL } else { &icons::PHONE_FILL }, &label, selected);
    if system {
        return b.disabled(true).opacity(0.5).cursor(rune::Cursor::NotAllowed);
    }
    // `canOfferP2pCall`: the rollout is on and everyone fits in a P2P call.
    let (p2p_on, cap) = st.voice.p2p_rollout;
    let offer_p2p = !active && p2p_on && c.recipients.iter().filter(|u| u.id != store.my_id()).count() < cap;
    b.on_press(move |s: &mut S| {
        let sh = acc(s);
        // `getCallStartRequestOptions`: Shift skips the P2P offer and the confirmation.
        let bypass = sh.shift_held;
        if in_call {
            sh.intents.push(if video { Intent::Voice(Op::ToggleCamera) } else { Intent::LeaveVoice });
        } else if active {
            sh.intents.push(Intent::Voice(Op::AcceptCall(ch)));
        } else if offer_p2p && !bypass {
            sh.voice.modal = Some(Modal::P2pConsent { guild: None, channel: ch, start: true, allow_standard: true, video, always: false });
        } else if !bypass && crate::look::with(|l| l.confirm_calls) {
            sh.voice.modal = Some(Modal::StartCall { channel: ch, video, tip: true });
        } else {
            start_call(sh, ch, false, video);
        }
    })
}

/// `requestStartCall` without a button: the confirmation (when on) or the call itself.
pub fn request_call(store: &Store, sh: &mut ShellState, ch: Snowflake, video: bool) {
    let active = store.calls.contains_key(&ch);
    if !active && crate::look::with(|l| l.confirm_calls) {
        sh.voice.modal = Some(Modal::StartCall { channel: ch, video, tip: false });
    } else {
        start_call(sh, ch, active, video);
    }
}

/// `ParticipantVolumeSlider`, `LocalMuteParticipantMenuItem` and `EntranceSoundListenerSubmenu`.
pub fn participant_items<S: 'static>(st: &ShellState, user: Snowflake, p: &Palette, acc: ShellAccess<S>) -> Vec<crate::ui::MenuEntry<S>> {
    let set = &st.voice.settings;
    // Settings changes save and reach the mixer through `Op::Settings`.
    let tweak = move |s: &mut S, f: &dyn Fn(&mut super::voice::Settings)| {
        let sh = acc(s);
        f(&mut sh.voice.settings);
        sh.intents.push(Intent::Voice(Op::Settings));
    };
    let muted = st.voice.local_mutes.contains(&user);
    let vol = set.user_volumes.get(&user.0).copied().unwrap_or(100.0);
    let (emuted, evol) = (set.entrance_mutes.contains(&user.0), set.entrance_volumes.get(&user.0).copied().unwrap_or(100.0));
    use crate::ui::MenuEntry;
    vec![
        MenuEntry::Separator,
        menu_slider(format!("user-vol-{user}"), t("User volume"), vol, p, move |s: &mut S, x| tweak(s, &|st| { st.user_volumes.insert(user.0, x); })),
        MenuEntry::Separator,
        MenuEntry::Item(MenuItem::new("local-mute", t("Mute")).checkbox(muted).on_select(op(acc, Op::LocalMute(user, !muted)))),
        MenuEntry::Submenu(
            "entrance".into(),
            t("Entrance sound").into(),
            vec![
                MenuEntry::Item(MenuItem::new("entrance-mute", t("Mute entrance sound")).checkbox(emuted).on_select(move |s: &mut S| {
                    tweak(s, &|st| {
                        if !st.entrance_mutes.remove(&user.0) {
                            st.entrance_mutes.insert(user.0);
                        }
                    })
                })),
                menu_slider(format!("entrance-vol-{user}"), t("Entrance sound volume"), evol, p, move |s: &mut S, x| tweak(s, &|st| { st.entrance_volumes.insert(user.0, x); })),
            ],
        ),
    ]
}

fn start_call(sh: &mut ShellState, ch: Snowflake, active: bool, video: bool) {
    let o = if active { Op::AcceptCall(ch) } else { Op::StartCall(ch) };
    sh.intents.push(Intent::Voice(o));
    camera_after_join(sh, video);
}

fn camera_after_join(sh: &mut ShellState, video: bool) {
    if video {
        // The camera turns on once connected (`startCall` with video).
        sh.voice.modal = Some(Modal::CameraPreview);
        sh.intents.push(Intent::Voice(Op::LoadCameras));
        sh.intents.push(Intent::Voice(Op::Preview(true)));
    }
}

/// `INCOMING_CALL_OVERLAY_WIDTH` / `_HEIGHT`.
const INCOMING_W: f32 = 320.0;
const INCOMING_H: f32 = 400.0;

/// `IncomingCallManager`'s card position: where it was dragged to (else the window's centre),
/// plus the drag in progress, kept inside the window.
fn incoming_pos(st: &ShellState) -> (f32, f32) {
    let [w, h] = rune::memo::layout_size(super::BODY_KEY).unwrap_or([1280.0, 800.0]);
    let (x, y) = st.prefs.incoming_call_pos.unwrap_or(((w / 2.0 - INCOMING_W / 2.0).max(0.0), (h / 2.0 - INCOMING_H / 2.0).max(0.0)));
    let [dx, dy] = st.call.incoming_drag.unwrap_or_default();
    ((x + dx).clamp(0.0, (w - INCOMING_W).max(0.0)), (y + dy).clamp(0.0, (h - INCOMING_H).max(0.0)))
}

/// `IncomingCallUI`: the first call ringing us that we haven't answered or ignored, in a card
/// dragged by its top strip. The developer tools' `MockIncomingCall` comes first; its buttons
/// only dismiss it.
pub fn incoming<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let me = store.my_id();
    let mock = super::devtools::mock_call(store, st);
    let mocked = mock.is_some();
    let (caller, name, channel, group) = match mock {
        Some((caller, name)) => (caller, name, Snowflake(0), None),
        None => {
            let call = store
                .calls
                .values()
                .filter(|c| c.ringing.contains(&me) && !st.voice.dismissed_calls.contains(&c.channel_id) && st.voice.channel != Some((None, c.channel_id)))
                .min_by_key(|c| c.channel_id)?;
            let ch = store.channels.get(&call.channel_id);
            // `resolveInitiatorUserId`: a DM's other recipient, else whoever else is being rung.
            let other = ch.filter(|c| c.kind == ChannelType::DM).and_then(|c| c.recipients.iter().map(|u| u.id).find(|u| *u != me));
            let caller = other.or_else(|| call.ringing.iter().copied().find(|u| *u != me)).or_else(|| call.ringing.first().copied())?;
            match ch {
                Some(c) if c.kind == ChannelType::GROUP_DM => (caller, super::chat::channel_title(store, c), call.channel_id, Some(c)),
                _ => (caller, store.display_name(None, caller), call.channel_id, None),
            }
        }
    };
    let end_mock = move |s: &mut S| acc(s).devtools.mock_call = None;
    let decline: Box<dyn Fn(&mut S)> = if mocked { Box::new(end_mock) } else { Box::new(op(acc, Op::DeclineCall(channel))) };
    let accept: Box<dyn Fn(&mut S)> = if mocked { Box::new(end_mock) } else { Box::new(op(acc, Op::AcceptCall(channel))) };
    let ignore: std::rc::Rc<dyn Fn(&mut S)> = if mocked { std::rc::Rc::new(end_mock) } else { std::rc::Rc::new(ignore_call(acc, channel, me)) };
    let secondary = rgba(235, 235, 245, 0.55);
    let shadow = |y: f32, blur: f32, color: Color| Shadow { x: 0.0, y, blur, spread: 0.0, color, inset: false };
    // `.circleButton`: 64px, its colour's glow; hover is the lighter colour.
    // shortcut: Rune has no brightness filter or pressed offset, so hover only swaps the colour
    // and a press doesn't move the button 1px down.
    let circle = |glyph: El, bg: Color, hover: Color, glow: Color, label: &str, on: Box<dyn Fn(&mut S)>| {
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
                    .box_shadow(shadow(6.0, 16.0, glow))
                    .cursor(rune::Cursor::Pointer)
                    .role("button")
                    .label(label.to_string())
                    .child(glyph)
                    .on_press(move |s: &mut S| on(s)),
            )
            .child(text(label.to_string()).font(font(13.0, 500.0).lh(16.0)).color(rgba(235, 235, 245, 0.7)))
    };
    let face = match group {
        Some(c) => super::group_avatar(store, st, c, 80.0, rgb(0x2c, 0x2c, 0x2e), p),
        None => avatar(store, st, caller, 80.0, None, rgb(0x2c, 0x2c, 0x2e), p),
    };
    // `.avatar`: a 96px shell with a 2.2s ring pulse spreading 10px.
    // shortcut: Rune loops opacity only, so the ring fades in and out at half its spread.
    let face = el()
        .size(96.0, 96.0)
        .child(el().pin(Some(-5.0), Some(-5.0), Some(-5.0), Some(-5.0)).radius(999.0).fill(rgba(255, 255, 255, 0.18)).looping(rune::LoopSpec::opacity(2.2, 0.0, 0.0, 0.5)))
        .child(row().w_full().h_full().center().radius(999.0).fill(rgba(255, 255, 255, 0.04)).box_shadow(shadow(12.0, 28.0, rgba(0, 0, 0, 0.45))).clip_rounded(48.0).child(face));
    let stack = col()
        .w_full()
        .pt(26.0)
        .px(24.0)
        .pb(22.0)
        .items_center()
        .child(
            row()
                .gap(6.0)
                .center_y()
                .mb(18.0)
                .child(icon(&icons::PHONE_INCOMING_FILL, 14.0).color(secondary))
                .child(text(t("Incoming call")).font(font(13.0, 500.0).lh(16.0)).color(secondary)),
        )
        .child(face.mb(16.0))
        .child(
            col()
                .w_full()
                .items_center()
                .mb(28.0)
                .child(text(name).font(font(22.0, 600.0).lh(26.4).tracking(-0.22)).color(Color::WHITE).truncate())
                .child(text(t("Voice call")).font(font(14.0, 400.0).lh(18.0)).color(secondary).mt(4.0)),
        )
        .child(
            row()
                .gap(56.0)
                .mb(14.0)
                // `.circleButtonIconRotated`: the phone turned 135°.
                .child(circle(icon(&icons::PHONE_FILL, 28.0).color(Color::WHITE).rotate(std::f32::consts::FRAC_PI_4 * 3.0), rgb(0xff, 0x3b, 0x30), rgb(0xff, 0x45, 0x3a), rgba(255, 59, 48, 0.45), t("Reject"), decline))
                .child(circle(icon(&icons::PHONE_FILL, 28.0).color(Color::WHITE), rgb(0x34, 0xc7, 0x59), rgb(0x30, 0xd1, 0x58), rgba(52, 199, 89, 0.45), t("Accept"), accept)),
        )
        .child({
            let ignore = ignore.clone();
            // `.ignoreLink`: 14/500, white on a faint pill while hovered.
            row()
                .mt(6.0)
                .px(14.0)
                .py(8.0)
                .radius(999.0)
                .hover(rgba(255, 255, 255, 0.06))
                .hover_group()
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Ignore"))
                .child(text(t("Ignore")).font(font(14.0, 500.0).lh(18.0)).color(rgba(235, 235, 245, 0.5)).group_color(Color::WHITE))
                .on_press(move |s: &mut S| ignore(s))
        });
    // `.dragHandle`: an 18px strip with a 36x5 pill 4px above its bottom; the card moves once
    // the pointer has gone 6pt, and lands (saved) where it's let go.
    let handle = row()
        .pin(Some(0.0), Some(0.0), Some(0.0), None)
        .h(18.0)
        .pb(4.0)
        .justify_center()
        .items_end()
        .cursor(if st.call.incoming_drag.is_some() { rune::Cursor::Grabbing } else { rune::Cursor::Grab })
        .child(el().size(36.0, 5.0).radius(999.0).fill(rgba(235, 235, 245, 0.28)))
        .on_drag(move |s: &mut S, d: &mut rune::Drag, _| {
            let sh = acc(s);
            if d.phase == rune::Phase::Up {
                if sh.call.incoming_drag.is_some() && !d.cancelled() {
                    sh.prefs.incoming_call_pos = Some(incoming_pos(sh));
                    let prefs = sh.prefs.clone();
                    sh.intents.push(Intent::Prefs(Box::new(prefs)));
                }
                sh.call.incoming_drag = None;
            } else if sh.call.incoming_drag.is_some() || d.dx * d.dx + d.dy * d.dy > 36.0 {
                sh.call.incoming_drag = Some([d.dx, d.dy]);
            }
        });
    let card = col()
        .w(INCOMING_W)
        .radius(28.0)
        .linear_gradient(std::f32::consts::PI, [(0.0, rgb(0x2c, 0x2c, 0x2e)), (1.0, rgb(0x1c, 0x1c, 0x1e))])
        .border(1.0, rgba(255, 255, 255, 0.08))
        .box_shadow(shadow(24.0, 60.0, rgba(0, 0, 0, 0.55)))
        .box_shadow(shadow(4.0, 12.0, rgba(0, 0, 0, 0.35)))
        .box_shadow(Shadow { inset: true, ..shadow(1.0, 0.0, rgba(255, 255, 255, 0.06)) })
        .clip_rounded(28.0)
        // `CARD_MOTION`: fades in from .94 scale, 8px lower.
        .transition(rune::Transition::opacity().combined(rune::Transition::scale(0.94)).combined(rune::Transition::offset(0.0, 8.0)))
        .key(format!("incoming-call-{channel}"))
        .role("dialog")
        .label(t("Incoming call"))
        // `.card`'s highlight: radial-gradient(140% 100% at 50% 0%, white 6%, transparent 60%).
        // shortcut: no backdrop blur behind the card.
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).inert().radial_gradient([0.5, 0.0], [INCOMING_W * 1.4, INCOMING_H], [(0.0, rgba(255, 255, 255, 0.06)), (0.6, rgba(255, 255, 255, 0.0))]))
        .child(stack)
        .child(handle)
        // Escape is Ignore while focus is in the card.
        .on_key(move |s: &mut S, k: &rune::KeyEvent, _| {
            if k.key != rune::Key::Escape {
                return false;
            }
            ignore(s);
            true
        });
    let (x, y) = incoming_pos(st);
    Some(el().pin(Some(x), Some(y), None, None).child(card))
}

const PIP_MENU: &str = "pip-overlay-menu";
/// `PiPOverlay`'s default width (16:9) and `EDGE_PADDING`.
const PIP_W: f32 = 320.0;
const PIP_H: f32 = PIP_W * 9.0 / 16.0;
const PIP_PAD: f32 = 20.0;

/// The PiP's top-left in the window: its saved corner (`Corner`, bottom-right by default), plus
/// the drag in progress, kept inside the window.
fn pip_pos(st: &ShellState) -> (f32, f32) {
    let [w, h] = rune::memo::layout_size(super::BODY_KEY).unwrap_or([1280.0, 800.0]);
    let corner = st.prefs.pip_corner.unwrap_or(3);
    let x = if corner & 1 == 1 { w - PIP_W - PIP_PAD } else { PIP_PAD };
    let y = if corner & 2 == 2 { h - PIP_H - PIP_PAD } else { PIP_PAD };
    let [dx, dy] = st.call.pip_drag.unwrap_or_default();
    ((x + dx).clamp(0.0, (w - PIP_W).max(0.0)), (y + dy).clamp(0.0, (h - PIP_H).max(0.0)))
}

/// The corner nearest to where the PiP was let go: bit 1 right, bit 2 bottom.
fn pip_corner(x: f32, y: f32, w: f32, h: f32) -> u8 {
    u8::from(x + PIP_W / 2.0 > w / 2.0) | (u8::from(y + PIP_H / 2.0 > h / 2.0) << 1)
}

/// `PiPOverlay` (`ScreenSharePiPStateMachine`): the watched stream, small in a corner, while
/// the call's own view isn't on screen. Hovering shows "Back to call" with the channel's name
/// and the streamer with their viewer count; a double-click goes back to the call, right-click
/// offers to disable these popouts, and a drag moves it to another corner.
// shortcut: no edge resizing (240px .. 70% of the window) or fling physics; it keeps 320px and
// snaps to the nearest corner. A DM call's watched stream always fills the main column, so this
// shows for community calls only.
pub fn pip<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let (Some((Some(guild), ch)), Some(user)) = (st.voice.channel, st.voice.watching) else { return };
    if !st.voice.connected || st.channel == Some(ch) || st.voice.call_view || st.voice.stream_fullscreen || crate::look::with(|l| l.disable_screen_pip) {
        return;
    }
    let channel = store.channels.get(&ch).map(|c| c.name.clone().unwrap_or_default()).unwrap_or_default();
    let viewers = super::voice::spectators_for(store, Some(guild), user).len();
    let back = move |s: &mut S| acc(s).select_guild(Some(guild), Some(ch));
    let header = row()
        .pin(Some(0.0), Some(0.0), Some(0.0), None)
        .h(48.0)
        .px(20.0)
        .py(8.0)
        .items_start()
        .linear_gradient(std::f32::consts::PI, [(0.0, rgba(0, 0, 0, 0.78)), (1.0, rgba(0, 0, 0, 0.0))])
        .child(
            row()
                .gap(6.0)
                .center_y()
                .min_w(0.0)
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Back to call"))
                .on_press(back)
                .child(icon(&icons::ARROW_LEFT_BOLD, 14.0).color(Color::WHITE))
                .child(text(channel).font(font(13.0, 600.0).lh(15.6)).color(Color::WHITE).truncate().shrink()),
        );
    let mut right = row().gap(6.0).center_y();
    if viewers > 0 {
        right = right.child(
            row()
                .gap(3.0)
                .center_y()
                .child(icon(&icons::EYE_FILL, 14.0).color(rgba(255, 255, 255, 0.72)))
                .child(text(viewers.to_string()).font(font(12.0, 500.0).lh(14.4)).color(rgba(255, 255, 255, 0.72))),
        );
    }
    let footer = row()
        .pin(Some(0.0), None, Some(0.0), Some(0.0))
        .h(48.0)
        .px(20.0)
        .py(8.0)
        .items_end()
        .gap(8.0)
        .linear_gradient(0.0, [(0.0, rgba(0, 0, 0, 0.78)), (1.0, rgba(0, 0, 0, 0.0))])
        .child(text(store.display_name(Some(guild), user)).font(font(12.0, 500.0).lh(14.4)).color(Color::WHITE).truncate().grow(1.0).min_w(0.0))
        .child(right);
    // `.hoverOverlay`: header and footer fade in while the pointer is on the PiP (or dragging it).
    let dragging = st.call.pip_drag.is_some();
    let hud = el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).child(header).child(footer);
    let hud = if dragging { hud } else { hud.group_opacity(0.0, 1.0) };
    let card = el()
        .key("pip-overlay")
        .size(PIP_W, PIP_H)
        .radius(12.0)
        .clip_rounded(12.0)
        .fill(surface_2())
        .box_shadow(Shadow { x: 0.0, y: 8.0, blur: 32.0, spread: 0.0, color: rgba(0, 0, 0, 0.4), inset: false })
        .box_shadow(Shadow { x: 0.0, y: 2.0, blur: 8.0, spread: 0.0, color: rgba(0, 0, 0, 0.2), inset: false })
        .cursor(if dragging { rune::Cursor::Grabbing } else { rune::Cursor::Grab })
        .hover_group()
        .child(el().w_full().h_full().pixels_fit(&st.voice.stream, rune::SurfaceFit::Fit))
        .child(hud)
        .on_double_press(back)
        .on_secondary_press(move |s: &mut S, at, _| acc(s).popups.open_at(PIP_MENU, [at.x, at.y]))
        .on_drag(move |s: &mut S, d: &mut rune::Drag, _| {
            let sh = acc(s);
            if d.phase == rune::Phase::Up {
                if sh.call.pip_drag.is_some() && !d.cancelled() {
                    let (x, y) = pip_pos(sh);
                    let [w, h] = rune::memo::layout_size(super::BODY_KEY).unwrap_or([1280.0, 800.0]);
                    sh.prefs.pip_corner = Some(pip_corner(x, y, w, h));
                    let prefs = sh.prefs.clone();
                    sh.intents.push(Intent::Prefs(Box::new(prefs)));
                }
                sh.call.pip_drag = None;
            } else if sh.call.pip_drag.is_some() || d.dx * d.dx + d.dy * d.dy > 16.0 {
                sh.call.pip_drag = Some([d.dx, d.dy]);
            }
        });
    let (x, y) = pip_pos(st);
    layer.push(el().pin(Some(x), Some(y), None, None).child(card));
    // `PiPOverlayContextMenu`.
    Menu::new(PIP_MENU)
        .item(MenuItem::new("pip-disable", t("Disable screen share picture-in-picture popouts")).icon(&icons::PICTURE_IN_PICTURE_FILL).on_select(move |s: &mut S| acc(s).voice.modal = Some(Modal::DisableScreenPip)))
        .el(&st.popups, pops(acc), p, layer);
}

// ---- Dialogs -----------------------------------------------------------------------------------

fn body_text(s: &str, p: &Palette) -> El {
    text(s.to_string()).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap()
}

/// `NewDeviceMonitoring`'s `ConfirmModal`: the device name in bold, "Don't suggest … again"
/// under it.
fn new_device<S: 'static>(d: &super::voice::NewDevice, p: &Palette, acc: ShellAccess<S>, close: impl Fn(&mut S) + Copy + 'static) -> crate::ui::Modal<S> {
    use rune::text::Span;
    let msg = match (&d.input, &d.output) {
        (Some(_), Some(_)) => "{PRODUCT_NAME} has found a new audio device called <0>{deviceName}</0>. Do you want to switch to it?",
        (Some(_), None) => "{PRODUCT_NAME} has found a new audio input device called <0>{deviceName}</0>. Do you want to switch to it?",
        _ => "{PRODUCT_NAME} has found a new audio output device called <0>{deviceName}</0>. Do you want to switch to it?",
    };
    let msg = crate::i18n::tf(msg, &[("PRODUCT_NAME", "Fluxer"), ("deviceName", &d.name)]);
    let (before, rest) = msg.split_once("<0>").unwrap_or((&msg, ""));
    let (name, after) = rest.split_once("</0>").unwrap_or((rest, ""));
    let body = format!("{before}{name}{after}");
    let bold = Span::new(before.len(), before.len() + name.len()).weight(600.0).color(p.text_primary);
    let desc = rune::rich(body, vec![bold]).w_full().font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap();
    let card = d.name.clone();
    let ignore = text(crate::i18n::tf("Don't suggest {deviceName} again", &[("deviceName", &d.name)]))
        .key("new-device-ignore")
        .font(font(14.0, 400.0).lh(19.6))
        .color(p.text_link)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.modal = None;
            sh.intents.push(Intent::Voice(Op::IgnoreDevice(card.clone())));
        });
    let (input, output) = (d.input.clone(), d.output.clone());
    let buttons = row()
        .gap(8.0)
        .child(Button::new(t("Not now")).variant(ButtonVariant::Secondary).key("new-device-not-now").on_press(close).el(p))
        .child(Button::new(t("Switch device")).key("new-device-switch").on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.modal = None;
            if input.is_some() {
                sh.voice.input = input.clone();
            }
            if output.is_some() {
                sh.voice.output = output.clone();
            }
            sh.intents.push(Intent::VoiceDevices);
        }).el(p));
    crate::ui::Modal::new(t("New audio device detected!"), close).body(col().w_full().gap(16.0).child(desc).child(ignore)).footer(buttons)
}

/// `HideOwnCameraConfirmModal`: turning our own tile off only hides it for us.
fn hide_own_camera<S: 'static>(dont_ask: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let close = move |s: &mut S| acc(s).call.hide_camera_confirm = None;
    let toggle = move |s: &mut S| {
        let sh = acc(s);
        if let Some(v) = &mut sh.call.hide_camera_confirm {
            *v = !*v;
        }
    };
    let confirm = move |s: &mut S| {
        let sh = acc(s);
        let skip = sh.call.hide_camera_confirm == Some(true);
        sh.call.hide_camera_confirm = None;
        sh.call.show_own_camera = false;
        if skip && !sh.prefs.skip_hide_own_camera_confirm {
            let mut prefs = sh.prefs.clone();
            prefs.skip_hide_own_camera_confirm = true;
            sh.intents.push(Intent::Prefs(Box::new(prefs)));
        }
    };
    crate::ui::Modal::new(t("Hide your own camera?"), close)
        .body(
            col()
                .w_full()
                .gap(16.0)
                .child(body_text(
                    t("Turning this off only hides your camera from your own view. Others in the call can still see your camera feed."),
                    p,
                ))
                .child(crate::ui::Checkbox::new(dont_ask).size(18.0).label(t("Don't ask me again")).on_toggle(toggle).el(p)),
        )
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("hide-camera-cancel").on_press(close).el(p))
                .child(Button::new(t("Hide")).key("hide-camera-confirm").on_press(confirm).el(p)),
        )
        .el(p)
}

/// The voice dialogs and the camera preview.
pub fn modals<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if let Some(dont_ask) = st.call.hide_camera_confirm {
        layer.push(hide_own_camera(dont_ask, p, acc));
    }
    let Some(m) = &st.voice.modal else { return };
    let mut pops = Vec::new();
    let close = move |s: &mut S| {
        let sh = acc(s);
        if sh.voice.modal == Some(Modal::CameraPreview) {
            sh.intents.push(Intent::Voice(Op::Preview(false)));
        }
        sh.voice.modal = None;
    };
    let ok = |label: &str| Button::new(label.to_string()).key("voice-modal-ok").on_press(close).el(p);
    let modal = match m {
        // `DisableScreenSharePiPConfirmModal`.
        Modal::DisableScreenPip => {
            let path = format!("{} > {}", t("User Settings"), t("Voice & video"));
            let confirm = move |s: &mut S| {
                let sh = acc(s);
                sh.voice.modal = None;
                let mut prefs = sh.prefs.clone();
                prefs.look.disable_screen_pip = true;
                sh.intents.push(Intent::Prefs(Box::new(prefs)));
            };
            crate::ui::Modal::new(t("Disable screen share picture-in-picture popouts?"), close)
                .body(body_text(&crate::i18n::tf("This will close the current popout and stop screen share picture-in-picture popouts from opening automatically. You can turn them back on in {audioVideoSettingsPath}.", &[("audioVideoSettingsPath", &path)]), p))
                .footer(
                    row()
                        .gap(8.0)
                        .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("pip-disable-cancel").on_press(close).el(p))
                        .child(Button::new(t("Disable popouts")).key("pip-disable-confirm").on_press(confirm).el(p)),
                )
        }
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
        Modal::StartCall { channel, video, tip } => {
            let (ch, video) = (*channel, *video);
            let title = t(if video { "Start video call" } else { "Start voice call" });
            let mut body = col().w_full().gap(16.0).child(body_text(t(if video { "Start a video call?" } else { "Start a voice call?" }), p));
            if *tip {
                let tip = t("Pro tip: Hold Shift while choosing this action to skip this confirmation.");
                body = body.child(rune::rich(tip.to_string(), vec![rune::Span::new(0, tip.find(':').map_or(0, |i| i + 1)).weight(700.0).color(p.accent_success)]).font(font(13.0, 400.0).lh(18.2)).color(p.text_primary_muted).wrap());
            }
            crate::ui::Modal::new(title, close)
                .body(body)
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
        Modal::P2pConsent { guild, channel, start, allow_standard, video, always } => p2p_consent(st, p, acc, close, (*guild, *channel), *start, *allow_standard, *video, *always),
        Modal::P2pFailed { names, allow_switch } => {
            let title = crate::i18n::tf("Couldn't connect to {names}", &[("names", &crate::i18n::list_and(names))]);
            let mut desc = t("A direct connection couldn't be made. You can't hear or see each other.").to_string();
            if *allow_switch {
                desc = format!("{desc} {}", t("A standard call goes through a server and works for everyone."));
            }
            let mut buttons = row()
                .gap(8.0)
                .child(Button::new(t("Stay")).variant(ButtonVariant::Secondary).key("p2p-failed-stay").on_press(close).el(p))
                .child(Button::new(t("Leave call")).variant(ButtonVariant::Danger).key("p2p-failed-leave").on_press(move |s: &mut S| {
                    let sh = acc(s);
                    sh.voice.modal = None;
                    sh.intents.push(Intent::LeaveVoice);
                }).el(p));
            if *allow_switch {
                buttons = buttons.child(Button::new(t("Switch to standard call")).key("p2p-failed-switch").on_press(move |s: &mut S| {
                    let sh = acc(s);
                    sh.voice.modal = None;
                    sh.intents.push(Intent::Voice(Op::SwitchToStandard));
                }).el(p));
            }
            crate::ui::Modal::new(title, close).body(body_text(&desc, p)).footer(buttons)
        }
        Modal::NewDevice(d) => new_device(d, p, acc, close),
        Modal::CameraPreview => camera_preview(st, p, acc, close, &mut pops),
        Modal::ScreenShare => screen_picker(st, p, acc, close, &mut pops),
        Modal::AudioProcessing => audio_processing(st, p, acc, close),
        Modal::Trim => trimmer(st, p, acc, close),
    };
    let _ = store;
    layer.push(modal.el(p));
    // A select's list anchors to its trigger inside the modal, so it must come after it: before
    // it, the anchor isn't placed yet (the list lands at the window's corner) and the dimmer covers it.
    layer.extend(pops);
}

/// `VoiceP2pConsentModal`: what a P2P call means, then start / join it (or a standard call).
#[allow(clippy::too_many_arguments)]
fn p2p_consent<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, close: impl Fn(&mut S) + Copy + 'static, (guild, channel): (Option<Snowflake>, Snowflake), start: bool, allow_standard: bool, video: bool, always: bool) -> crate::ui::Modal<S> {
    let cap = st.voice.p2p_rollout.1.max(2).to_string();
    let points = [
        t("Lower latency.").to_string(),
        t("Everyone in the call gets Plutonium stream quality for free, with up to 4K source resolution and 60 fps.").into(),
        t("Other participants can see your IP address.").into(),
        crate::i18n::tf("Up to {maxParticipants, plural, one {# person} other {# people}}.", &[("maxParticipants", &cap)]),
        t("Anyone who cannot connect directly cannot take part until someone switches the call to a standard call.").into(),
    ];
    // `.points`: a disc list, gap 6, 20 in, text-primary, line-height 1.45.
    let list = col().w_full().gap(6.0).children(points.into_iter().map(|s| {
        row()
            .w_full()
            .items_start()
            .child(row().w(20.0).h(20.3).center().child(el().size(5.0, 5.0).radius(2.5).fill(p.text_primary)))
            .child(text(s).font(font(14.0, 400.0).lh(20.3)).color(p.text_primary).wrap().grow(1.0).min_w(0.0))
    }));
    let mut body = col().w_full().gap(16.0).child(body_text(t("Audio and video go directly between participants."), p)).child(list);
    if !start {
        body = body.child(
            crate::ui::Checkbox::new(always)
                .size(18.0)
                .label(t("Do not ask again, always agree"))
                .on_toggle(move |s: &mut S| {
                    if let Some(Modal::P2pConsent { always, .. }) = &mut acc(s).voice.modal {
                        *always = !*always;
                    }
                })
                .el(p),
        );
    }
    let answer = move |p2p: bool| {
        move |s: &mut S| {
            let sh = acc(s);
            if p2p && !start && always {
                let mut prefs = sh.prefs.clone();
                prefs.look.skip_p2p_join = true;
                sh.intents.push(Intent::Prefs(Box::new(prefs)));
            }
            sh.voice.modal = None;
            sh.intents.push(Intent::Voice(Op::P2pConsent { guild, channel, p2p, start }));
            camera_after_join(sh, video);
        }
    };
    let mut buttons = row().gap(8.0).child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("p2p-cancel").on_press(close).el(p));
    if start && allow_standard {
        buttons = buttons.child(Button::new(t("Start standard call")).variant(ButtonVariant::Secondary).key("p2p-standard").on_press(answer(false)).el(p));
    }
    let confirm = if start { t("Start in P2P mode") } else { t("Join") };
    buttons = buttons.child(Button::new(confirm).key("p2p-confirm").on_press(answer(true)).el(p));
    crate::ui::Modal::new(t(if start { "Start in P2P mode?" } else { "Join a P2P call?" }), close).body(body).footer(buttons)
}

/// `VoiceE2EEIndicator` for a peer-to-peer call: "P2P" (red once a direct connection failed),
/// with what it means on hover.
pub fn p2p_indicator(broken: bool, p: &Palette) -> El {
    text(t("P2P"))
        .font(font(13.0, 500.0).lh(16.9).tracking(0.065))
        .color(if broken { p.status_danger } else { p.status_online })
        .role("status")
        .label(t("Direct connection. Audio and video go directly between the people in the call, who can see each other's IP address."))
}

/// `CameraPreviewModal`: the self view, the camera picker, then "Turn on camera".
fn camera_preview<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, close: impl Fn(&mut S) + Copy + 'static, layer: &mut Vec<El>) -> crate::ui::Modal<S> {
    let v = &st.voice;
    let higher = st.limits.higher_video_quality;
    let preview = el().w(400.0).h(225.0).radius(dim::RADIUS_LG).clip_rounded(dim::RADIUS_LG).fill(surface_0());
    let preview = if v.previewing { preview.pixels_fit(&v.self_view, rune::SurfaceFit::Fit) } else { preview.child(row().w_full().h_full().center().child(text(t("Camera preview")).font(font(14.0, 500.0).lh(18.0)).color(rgba(255, 255, 255, 0.72)))) };
    let names = v.cameras.clone().unwrap_or_default();
    let opts: Vec<(&str, &str)> = std::iter::once(("", "Default")).chain(names.iter().map(|n| (n.as_str(), n.as_str()))).collect();
    let pick = crate::ui::Select::new("camera-device", &opts, Some(v.settings.camera.as_deref().unwrap_or("")))
        .label(t("Camera"))
        .width(195.0)
        .on_choose(move |s: &mut S, val| {
            let sh = acc(s);
            sh.voice.settings.camera = (!val.is_empty()).then(|| val.to_string());
            sh.intents.push(Intent::Voice(Op::Settings));
            // Reopen the capture on the new device.
            sh.intents.push(Intent::Voice(Op::Preview(false)));
            sh.intents.push(Intent::Voice(Op::Preview(true)));
        })
        .el(&st.popups, pops(acc), p, layer);
    let quality = [
        ("low", t("480p")),
        ("medium", t("720p")),
        ("high", if higher { "1080p" } else { "1080p (Plutonium)" }),
    ];
    let q = crate::ui::Select::new("camera-quality", &quality, Some(v.settings.camera_quality.as_str()))
        .label(t("Camera quality"))
        .width(195.0)
        .on_choose(move |s: &mut S, val| {
            let sh = acc(s);
            sh.voice.settings.camera_quality = val.to_string();
            sh.intents.push(Intent::Voice(Op::Settings));
            sh.intents.push(Intent::Voice(Op::Preview(false)));
            sh.intents.push(Intent::Voice(Op::Preview(true)));
        })
        .el(&st.popups, pops(acc), p, layer);
    let mirror = Switch::new(v.settings.mirror_camera).label(t("Mirror camera")).on_toggle(move |s: &mut S| {
        let sh = acc(s);
        sh.voice.settings.mirror_camera ^= true;
        sh.intents.push(Intent::Voice(Op::Settings));
    });
    let can_turn_on = v.connected && !v.camera;
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(preview)
        .child(row().w_full().gap(8.0).child(pick).child(q))
        .child(mirror.el(p));
    if !higher {
        body = body.child(text(t("1080p requires Plutonium; without it the camera stays at 720p.")).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted).wrap());
    }
    let mut footer = row().gap(8.0).child(Button::new(if v.camera { t("Close") } else { t("Cancel") }).variant(ButtonVariant::Secondary).key("cam-cancel").on_press(close).el(p));
    if !v.camera {
        footer = footer.child(
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
        );
    }
    crate::ui::Modal::new(t("Camera preview"), close).body(body).footer(footer)
}

/// Opens the screen share picker and starts listing what it offers.
pub fn open_screen_picker(sh: &mut ShellState) {
    sh.voice.modal = Some(Modal::ScreenShare);
    sh.voice.screen_tab = "apps".into();
    sh.voice.screen_device = None;
    sh.voice.screen_sources = None;
    // Wayland: the portal picker, opened on the Applications tab (`preferredDisplaySurface`).
    sh.voice.screen_pick = (cfg!(target_os = "linux") && !reflux_voice::can_list_sources()).then(|| reflux_voice::ScreenSource::portal(true));
    sh.intents.push(Intent::Voice(Op::LoadScreenSources));
    sh.intents.push(Intent::Voice(Op::LoadCameras));
    sh.intents.push(Intent::Voice(Op::LoadAudioSources));
}

/// `ScreenSharePickerModal`: screens and windows with thumbnails where the platform lets us
/// list them (X11, Windows; Wayland and macOS go through the system picker), quality and frame
/// rate, and on Linux the application whose audio goes with the stream
/// (`AudioSourcePickerLinux`).
fn screen_picker<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, close: impl Fn(&mut S) + Copy + 'static, layer: &mut Vec<El>) -> crate::ui::Modal<S> {
    let v = &st.voice;
    let set = &v.settings;
    let higher = st.limits.higher_video_quality;
    let quality = set.screen_quality(&st.limits, higher);
    let listed = reflux_voice::can_list_sources() || v.screen_sources.is_some();
    let tab = match v.screen_tab.as_str() {
        "displays" => "displays",
        "devices" => "devices",
        _ => "apps",
    };
    let mut body = col().w_full().gap(16.0);
    // `ScreenSharePickerModal` tabs: Applications / Entire screen / Devices (default Applications).
    let mut tabs = row().gap(8.0);
    for (id, label) in [("apps", "Applications"), ("displays", "Entire screen"), ("devices", "Devices")] {
        let on = tab == id;
        tabs = tabs.child(
            Button::new(t(label))
                .variant(if on { ButtonVariant::Primary } else { ButtonVariant::Secondary })
                .key(format!("share-tab-{id}"))
                .on_press(move |s: &mut S| {
                    let sh = acc(s);
                    sh.voice.screen_tab = id.into();
                    sh.voice.screen_device = None;
                    // Wayland's portal needs its preference set before the system dialog.
                    sh.voice.screen_pick = (!reflux_voice::can_list_sources() && id != "devices").then(|| reflux_voice::ScreenSource::portal(id == "apps"));
                })
                .el(p),
        );
    }
    body = body.child(tabs);

    // `StreamSettingsMenuContent`'s streaming modes. The descriptions are Fluxer's, including
    // the higher-quality "Sharp text at up to 4K, up to 15 FPS" copy.
    let modes = [
        ("gaming", "Gaming", Some(if higher { "Smooth 60 FPS at up to 1440p" } else { "Smooth 30 FPS at up to 720p" })),
        ("screenshare", "Screen share", Some(if higher { "Sharp text at up to 4K, up to 15 FPS" } else { "Sharp text at 720p, up to 30 FPS" })),
        ("custom", "Custom", Some("Dial in your own numbers")),
    ];
    body = body.child(
        crate::ui::RadioGroup::new(&modes, Some(set.streaming_mode.as_str()))
            .on_change(move |s: &mut S, val| {
                acc(s).voice.settings.streaming_mode = val.to_string();
                acc(s).intents.push(Intent::Voice(Op::Settings));
            })
            .el(p),
    );

    // Picking either value switches to Custom, like `resolveScreenShareQualityPick`.
    let res = crate::shell::voice::screen_resolutions(higher);
    let res: Vec<(&str, &str)> = res.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
    let fps = crate::shell::voice::screen_frame_rates(higher);
    let fps: Vec<(&str, &str)> = fps.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
    let res_v = quality.max_height.map_or("source".to_string(), |h| h.to_string());
    let fps_v = quality.fps.to_string();
    let r = crate::ui::Select::new("share-res", &res, Some(&res_v))
        .label(t("Resolution"))
        .width(190.0)
        .on_choose(move |s: &mut S, v| {
            let sh = acc(s);
            (sh.voice.settings.screen_height, sh.voice.settings.streaming_mode) = (v.parse().ok(), "custom".into());
            sh.intents.push(Intent::Voice(Op::Settings));
        })
        .el(&st.popups, pops(acc), p, layer);
    let f = crate::ui::Select::new("share-fps", &fps, Some(&fps_v))
        .label(t("Frame rate"))
        .width(190.0)
        .on_choose(move |s: &mut S, v| {
            let sh = acc(s);
            (sh.voice.settings.screen_fps, sh.voice.settings.streaming_mode) = (v.parse().unwrap_or(30), "custom".into());
            sh.intents.push(Intent::Voice(Op::Settings));
        })
        .el(&st.popups, pops(acc), p, layer);
    match tab {
        "devices" => {
            let cams = v.cameras.clone().unwrap_or_default();
            if cams.is_empty() {
                body = body.child(body_text(t("No capture devices found."), p).key("none"));
            } else {
                // shortcut: device cards are names only (Fluxer shows live previews, which needs
                // camera permission and a capture per device); add previews if users ask.
                let mut cards = col().w_full().gap(6.0);
                for name in cams {
                    let picked = v.screen_device.as_deref() == Some(name.as_str());
                    let pick = name.clone();
                    cards = cards.child(
                        row()
                            .key(format!("share-device-{name}"))
                            .w_full()
                            .padding(10.0)
                            .gap(10.0)
                            .center_y()
                            .radius(8.0)
                            .border(2.0, if picked { p.brand_primary } else { p.background_modifier_accent })
                            .hover(p.background_modifier_hover)
                            .cursor(rune::Cursor::Pointer)
                            .role("radio")
                            .label(name.clone())
                            .child(icon(&icons::VIDEO_CAMERA_FILL, 18.0).color(if picked { p.brand_primary } else { p.text_primary_muted }))
                            .child(text(name).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary))
                            .on_press(move |s: &mut S| {
                                let sh = acc(s);
                                sh.voice.screen_device = Some(pick.clone());
                                sh.voice.screen_pick = None;
                            }),
                    );
                }
                body = body.child(cards.key("devices"));
            }
        }
        _ if !listed => {
            body = body.child(body_text("Choose the stream quality, then pick a screen or window in the system dialog.", p).key("portal"))
        }
        _ => match &v.screen_sources {
            None => body = body.child(body_text(t("Loading…"), p).key("loading")),
            Some(list) => {
                let windows = tab == "apps";
                let card = |src: &reflux_voice::ScreenSource, px: &Pixels| {
                    let picked = v.screen_pick.as_ref().is_some_and(|x| x.id == src.id && x.window == src.window);
                    let pick = src.clone();
                    col()
                        .key(format!("share-src-{}-{}", src.window as u8, src.id))
                        .w(176.0)
                        .gap(6.0)
                        .padding(6.0)
                        .radius(8.0)
                        .border(2.0, if picked { p.brand_primary } else { Color::TRANSPARENT })
                        .hover(p.background_modifier_hover)
                        .cursor(rune::Cursor::Pointer)
                        .role("radio")
                        .label(src.title.clone())
                        .child(el().w(160.0).h(90.0).radius(6.0).clip_rounded(6.0).fill(surface_0()).pixels_fit(px, rune::SurfaceFit::Fit))
                        .child(text(src.title.clone()).font(font(12.0, 500.0).lh(16.0)).color(p.text_primary).truncate())
                        .on_press(move |s: &mut S| {
                            let sh = acc(s);
                            sh.voice.screen_pick = Some(pick.clone());
                            sh.voice.screen_device = None;
                        })
                };
                let cards: Vec<El> = list.iter().filter(|(s, _)| s.window == windows).map(|(s, px)| card(s, px)).collect();
                if cards.is_empty() {
                    body = body.child(body_text(t(if windows { "No application windows found." } else { "No displays found." }), p).key("none"));
                } else {
                    body = body.child(row().key("sources").w_full().flex_wrap().gap(8.0).children(cards));
                }
            }
        },
    }
    body = body.child(row().key("quality").w_full().gap(16.0).child(r).child(f));
    if let Some(preset) = set.screen_preset() {
        let note = crate::i18n::tf("Set by the {preset} preset. Picking a value here switches to Custom.", &[("preset", t(preset.0))]);
        body = body.child(text(note).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted).wrap().key("preset"));
    }
    if !higher && st.premium {
        let note = crate::i18n::tf("Frame rates above 30 FPS require {premiumProductName}", &[("premiumProductName", "Plutonium")]);
        body = body.child(text(note).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted).wrap().key("premium"));
    }
    if let Some(apps) = &v.audio_sources {
        let opts: Vec<(&str, &str)> = std::iter::once(("", "No audio")).chain(apps.iter().map(|(id, name)| (id.as_str(), name.as_str()))).collect();
        let cur = set.stream_audio.clone().filter(|id| apps.iter().any(|(x, _)| x == id)).unwrap_or_default();
        body = body.child(
            crate::ui::Select::new("share-audio", &opts, Some(&cur))
                .label(t("Shared window audio"))
                .width(396.0)
                .on_choose(move |s: &mut S, v| {
                    let sh = acc(s);
                    sh.voice.settings.stream_audio = (!v.is_empty()).then(|| v.to_string());
                    sh.intents.push(Intent::Voice(Op::Settings));
                })
                .el(&st.popups, pops(acc), p, layer)
                .key("audio"),
        );
    }
    // A camera/device share or a listed source; on Wayland the portal's default pick counts.
    let ready = if tab == "devices" { v.screen_device.is_some() } else { v.screen_pick.is_some() };
    crate::ui::Modal::new(t("Share your screen"), close)
        .size(if listed || tab == "devices" { crate::ui::ModalSize::Medium } else { crate::ui::ModalSize::Small })
        .body(body)
        .footer(
            row()
                .gap(8.0)
                .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("share-cancel").on_press(close).el(p))
                .child(Button::new("Go live").key("share-go").disabled(!ready).on_press(move |s: &mut S| {
                    let sh = acc(s);
                    let changing = sh.voice.screen;
                    sh.voice.modal = None;
                    sh.voice.screen_sources = None;
                    sh.intents.push(if changing { Intent::Voice(Op::ChangeScreen) } else { Intent::ToggleScreen });
                }).el(p)),
        )
}

/// `CompactComboboxRow` "Noise suppression": the backend for the custom profile, with
/// `getNoiseSuppressionFallbackMessage` under it while RNNoise stands in for DeepFilterNet.
pub fn noise_select<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let opts: Vec<(&str, &str)> = super::voice::NOISE_BACKENDS.iter().map(|(id, l)| (*id, if *id == "none" { t(l) } else { *l })).collect();
    let pick = crate::ui::Select::new("noise-backend", &opts, Some(st.voice.settings.noise_backend.as_str()))
        .label(t("Noise suppression"))
        .width(195.0)
        .on_choose(move |s: &mut S, val| {
            let sh = acc(s);
            sh.voice.settings.noise_backend = val.to_string();
            sh.intents.push(Intent::Voice(Op::Settings));
        })
        .el(&st.popups, pops(acc), p, layer);
    let mut c = col().w_full().gap(4.0).child(pick);
    if let Some(msg) = super::voice::noise_fallback(&st.voice) {
        c = c.child(text(msg).font(font(12.0, 400.0).lh(16.0)).color(p.text_secondary).wrap());
    }
    c
}

/// `AudioProcessingModal`: the processing profile; Custom shows the individual switches.
fn audio_processing<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, close: impl Fn(&mut S) + Copy + 'static) -> crate::ui::Modal<S> {
    let set = &st.voice.settings;
    let change = move |f: fn(&mut super::voice::Settings)| {
        move |s: &mut S| {
            let sh = acc(s);
            f(&mut sh.voice.settings);
            sh.intents.push(Intent::Voice(Op::Settings));
        }
    };
    let profiles = [
        ("voice", "Focused voice", Some(t("Cleans up your mic for clear speech."))),
        ("studio", "Direct input", Some(t("Sends raw mic audio with no processing."))),
        ("custom", "Custom", Some(t("Tune the processing yourself."))),
    ];
    let mut body = col().w_full().gap(16.0).child(
        crate::ui::RadioGroup::new(&profiles, Some(set.processing.as_str()))
            .on_change(move |s: &mut S, val| {
                let sh = acc(s);
                sh.voice.settings.processing = val.to_string();
                sh.intents.push(Intent::Voice(Op::Settings));
            })
            .el(p),
    );
    if set.processing == "custom" {
        body = body
            .child(noise_select(st, p, acc, &mut Vec::new()))
            .child(crate::ui::Switch::new(set.echo_cancellation).label(t("Echo cancellation")).description(t("Stops your speakers from looping back into your mic.")).on_toggle(change(|s| s.echo_cancellation ^= true)).el(p))
            .child(crate::ui::Switch::new(set.auto_gain_control).label(t("Automatic gain control")).description(t("Evens out your mic volume so you are not too quiet.")).on_toggle(change(|s| s.auto_gain_control ^= true)).el(p));
    }
    crate::ui::Modal::new(t("Audio processing"), close).body(body)
}

/// `EntranceSoundTrimmerModal`: the waveform, the selected span (at most 5.2 s), play it, then
/// upload it.
fn trimmer<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, close: impl Fn(&mut S) + Copy + 'static) -> crate::ui::Modal<S> {
    let Some(tr) = &st.voice.trim else { return crate::ui::Modal::new(t("Trim entrance sound"), close) };
    let total = tr.secs().max(0.01);
    // 120 peak bars; the selected ones in the brand colour.
    let n = 120;
    let per = (tr.samples.len() / n).max(1);
    let bars = tr.samples.chunks(per).take(n).enumerate().map(|(i, c)| {
        let peak = c.iter().fold(0f32, |m, s| m.max(s.abs()));
        let at = i as f32 * per as f32 / reflux_voice::audio::RATE as f32;
        let on = at >= tr.start && at <= tr.end;
        el().grow(1.0).h((peak * 64.0).max(2.0)).radius(1.0).fill(if on { p.brand_primary } else { p.text_tertiary })
    });
    let wave = row().w_full().h(72.0).gap(1.0).center_y().padding(4.0).radius(8.0).fill(p.background_tertiary).children(bars);
    let handle = |key: &'static str, label: &str, value: f32, start: bool| {
        col().w_full().gap(4.0).child(text(format!("{label}: {value:.2} s")).font(font(12.0, 500.0).lh(16.0)).color(p.text_secondary)).child(
            crate::ui::Slider::new(key, value, 0.0, total)
                .step(0.01)
                .on_change(move |s: &mut S, x| {
                    if let Some(tr) = &mut acc(s).voice.trim {
                        tr.set(start, x);
                    }
                })
                .el(p),
        )
    };
    let mut body = col()
        .w_full()
        .gap(12.0)
        .child(body_text(t("Drag the handles to choose up to 5.2 seconds of audio. The selected region is what gets uploaded."), p))
        .child(wave)
        .child(handle("trim-start", "Start", tr.start, true))
        .child(handle("trim-end", "End", tr.end, false));
    if let Some(e) = &tr.error {
        body = body.child(text(e.clone()).font(font(14.0, 400.0).lh(20.0)).color(p.status_danger).wrap());
    }
    crate::ui::Modal::new(t("Trim entrance sound"), move |s: &mut S| {
        acc(s).voice.trim = None;
        close(s)
    })
    .body(body)
    .footer(
        row()
            .gap(8.0)
            .child(Button::new(t("Play selection")).variant(ButtonVariant::Secondary).key("trim-play").on_press(op(acc, Op::PreviewTrim)).el(p))
            .child(Button::new(t("Use selection")).key("trim-use").disabled(tr.uploading).on_press(op(acc, Op::UploadTrim)).el(p)),
    )
}

/// A labelled slider row in a menu (`MenuSliderType`), 0..`VOICE_VOLUME_MAX_PERCENT`.
fn menu_slider<S: 'static>(key: String, label: &str, value: f32, p: &Palette, on: impl Fn(&mut S, f32) + 'static) -> crate::ui::MenuEntry<S> {
    crate::ui::menu_slider(key, label, value, 0.0, super::voice::VOICE_VOLUME_MAX_PERCENT, format!("{value:.0}%"), p, on)
}

// ---- Popouts and menus ---------------------------------------------------------------------------

/// `VoiceDetailsPopout`: server, room, connection, encryption and the ping graph.
/// `VoiceDetailsPopout`: server, room, connection, duration, ping graph and the Advanced
/// packet-loss/jitter/bitrate section.
pub fn details<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !st.popups.is_open(DETAILS) {
        return;
    }
    let v = &st.voice;
    let line = |k: &str, val: String| {
        row()
            .w_full()
            .gap(8.0)
            .child(text(k.to_string()).font(font(12.0, 600.0).lh(16.0)).color(p.text_tertiary).w(110.0))
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
    let avg = (!v.pings.is_empty()).then(|| v.pings.iter().copied().sum::<u32>() as f64 / v.pings.len() as f64);
    let stats = st.call.stats;
    let jitter = stats.map_or_else(|| jitter_from_pings(&v.pings), |s| s.jitter_ms);
    let duration = st.call.connected_at.map(|t| duration_label(t.elapsed()));
    let count = v.channel.map_or(0, |(g, c)| participants(store, g, c).len());
    let dash = "—".to_string();
    let mut surface = col()
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
        .child(line(t("Ping"), v.ping.map_or(dash.clone(), |ms| format!("{ms} ms"))))
        .child(line(t("Average ping"), avg.map_or(dash.clone(), |ms| format!("{ms:.0} ms"))))
        .child(line(t("Duration"), duration.unwrap_or_else(|| dash.clone())))
        .child(line(t("Participants"), count.to_string()))
        .child(line(t("Encryption"), if v.encrypted { "Encrypted (server key)".into() } else { "Transport only (DTLS-SRTP)".into() }))
        .child(graph)
        .child(text(t("Advanced")).font(font(11.0, 600.0).lh(16.0).tracking(0.22)).color(p.text_tertiary).mt(4.0))
        .child(line(t("Packet loss (audio)"), stats.map_or_else(|| dash.clone(), |s| stat_percent(s.audio_loss))))
        .child(line(t("Packet loss (video)"), stats.map_or_else(|| dash.clone(), |s| stat_percent(s.video_loss))))
        .child(line(t("Jitter"), stat_ms(jitter)))
        .child(line(t("Send bitrate"), stats.map_or_else(|| dash.clone(), |s| stat_bitrate_kbps(s.send_bitrate))))
        .child(line(t("Receive bitrate"), stats.map_or(dash, |s| stat_bitrate_kbps(s.recv_bitrate))))
        .modal(true, move |s: &mut S| acc(s).popups.close());
    if st.call.stats.is_none() {
        // shortcut: the shell has no WebRTC stats handle; values stay dashes until the engine
        // reports them (packet loss / bitrate). Jitter falls back to RTT variation.
        surface = surface.child(text(t("Packet loss and bitrate appear once the voice engine reports them.")).font(font(11.0, 400.0).lh(15.0)).color(p.text_primary_muted).wrap());
    }
    layer.push(anchor(surface, DETAILS, Placement::Top, 8.0));
}

/// `VoiceParticipantContextMenu` (`useVoiceParticipantMenuData`) for a tile, avatar or row:
/// its groups for another user or ourselves, or a stream tile's menu.
pub fn tile_menu<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !st.popups.is_open(TILE_MENU) {
        return;
    }
    let Some(super::Target::User { user, guild, .. }) = st.target else { return };
    let groups = if st.call.menu_screen { stream_tile_groups(store, st, user, p, acc) } else { participant_groups(store, st, user, guild, p, acc) };
    let mut m = Menu::new(TILE_MENU);
    for g in groups.into_iter().filter(|g| !g.is_empty()) {
        m = m.separator();
        for e in g {
            m = m.entry(e);
        }
    }
    m.el(&st.popups, pops(acc), p, layer);
}

/// `buildVoiceParticipantStreamMenu`, or `ActiveScreenShareMenu` with
/// `VoiceParticipantOwnStreamMenuTail` for our own stream.
// shortcut: no "Copy stats JSON" or "Screen share privacy" (Reflux has neither).
fn stream_tile_groups<S: 'static>(store: &Store, st: &ShellState, user: Snowflake, p: &Palette, acc: ShellAccess<S>) -> Vec<Vec<MenuEntry<S>>> {
    let popout = || MenuEntry::Item(MenuItem::new("popout-stream", t("Pop out stream")).icon(&icons::ARROW_SQUARE_OUT_FILL).on_select(op(acc, Op::PopOut { user: Some(user), screen: true, pip: false })));
    if user == store.my_id() {
        if !st.voice.screen {
            return Vec::new();
        }
        let paused = crate::look::with(|l| l.pause_preview_unfocused);
        let more = vec![
            MenuEntry::Item(MenuItem::new("show-own-screen", t("Show my screen share")).checkbox(st.call.show_own_screen).keep_open().on_select(move |s: &mut S| acc(s).call.show_own_screen ^= true)),
            MenuEntry::Item(MenuItem::new("pause-preview", crate::i18n::tf("Pause preview when {productName} isn't focused", &[("productName", "Fluxer")])).checkbox(paused).keep_open().on_select(move |s: &mut S| {
                let sh = acc(s);
                let mut prefs = sh.prefs.clone();
                prefs.look.pause_preview_unfocused = !paused;
                sh.intents.push(Intent::Prefs(Box::new(prefs)));
            })),
        ];
        return vec![active_share_entries(st, p, acc), vec![popout()], vec![MenuEntry::Submenu("stream-more".into(), t("More options").into(), more)]];
    }
    let watching = st.voice.watching == Some(user);
    let state = if watching {
        MenuEntry::Item(MenuItem::new("stop-watching", t("Stop watching")).icon(&icons::MONITOR_PLAY_FILL).on_select(move |s: &mut S| super::voice::stop_watching(s, acc)))
    } else {
        MenuEntry::Item(MenuItem::new("watch", t("Watch stream")).icon(&icons::MONITOR_PLAY_FILL).on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.watching = Some(user);
            sh.intents.push(Intent::Watch(Some(user)));
        }))
    };
    let mut groups = vec![vec![state]];
    // `buildStreamAudioControls`: only for a watched stream with audio.
    if watching && st.voice.watch_audio {
        let set = &st.voice.settings;
        let (vol, muted) = (set.stream_volumes.get(&user.0).copied().unwrap_or(100.0), set.stream_mutes.contains(&user.0));
        let tweak = move |s: &mut S, f: &dyn Fn(&mut super::voice::Settings)| {
            let sh = acc(s);
            f(&mut sh.voice.settings);
            sh.intents.push(Intent::Voice(Op::Settings));
        };
        groups.push(vec![
            MenuEntry::Item(MenuItem::new("stream-mute", t("Mute")).checkbox(muted).on_select(move |s: &mut S| {
                tweak(s, &|st| {
                    if !st.stream_mutes.remove(&user.0) {
                        st.stream_mutes.insert(user.0);
                    }
                })
            })),
            menu_slider(format!("stream-vol-{user}"), t("Stream volume"), vol, p, move |s: &mut S, x| tweak(s, &|st| { st.stream_volumes.insert(user.0, x); })),
        ]);
    }
    if watching {
        groups.push(vec![popout()]);
    }
    groups
}

/// `useVoiceParticipantMenuData`'s groups for a participant: profile actions, volume, playback
/// and relationship, community, moderation, display, guild voice moderation and the ID.
// shortcut: no "Pop out user", "Disable video locally" or "Change friend nickname" (no
// backing feature in Reflux); device-scoped (multi-connection) items are not split out.
fn participant_groups<S: 'static>(store: &Store, st: &ShellState, user: Snowflake, guild: Option<Snowflake>, p: &Palette, acc: ShellAccess<S>) -> Vec<Vec<MenuEntry<S>>> {
    let me = store.my_id();
    let is_me = user == me;
    let bot = store.users.get(&user).is_some_and(|u| u.bot);
    let item = |i: MenuItem<S>| MenuEntry::Item(i);
    let profile = move |focus_note: bool| {
        move |s: &mut S| {
            let sh = acc(s);
            sh.popups.close();
            sh.account.profile_modal = Some((user, guild, "overview"));
            sh.fetch_profile(user, guild);
            sh.note.focused = focus_note;
        }
    };
    let mut primary = vec![item(MenuItem::new("profile", t("View profile")).icon(&icons::USER_FILL).on_select(profile(false)))];
    let dm = store.private_channels().into_iter().find(|c| c.kind == ChannelType::DM && c.recipients.iter().any(|r| r.id == user)).map(|c| c.id);
    if !is_me {
        primary.push(item(MenuItem::new("message", t("Message")).icon(&icons::CHAT_CIRCLE_FILL).on_select(move |s: &mut S| acc(s).intents.push(Intent::OpenDm(user)))));
        if !bot && dm.is_none_or(|c| !store.calls.contains_key(&c)) {
            primary.push(item(MenuItem::new("start-call", t("Start voice call")).icon(&icons::PHONE_FILL).on_select(move |s: &mut S| acc(s).intents.push(Intent::DmCall(user, false)))));
        }
        primary.push(item(MenuItem::new("note", t("Add note")).icon(&icons::NOTE_PENCIL_FILL).on_select(profile(true))));
    }
    let focused = st.call.layout == LayoutMode::Focus && st.voice.focus == Some(user);
    let focus = item(MenuItem::new("focus", if focused { t("Unfocus") } else { t("Focus this person") }).icon(if focused { &icons::PUSH_PIN_SIMPLE_SLASH_FILL } else { &icons::PUSH_PIN_SIMPLE_FILL }).on_select(move |s: &mut S| {
        let sh = acc(s);
        sh.voice.focus = (!focused).then_some(user);
        sh.call.pin((!focused).then_some(user));
    }));
    let has_video = st.voice.videos.contains_key(&user) || (is_me && st.voice.camera);
    let mut display = Vec::new();
    if has_video {
        display.push(item(MenuItem::new("popout-camera", t("Pop out video")).icon(&icons::ARROW_SQUARE_OUT_FILL).on_select(op(acc, Op::PopOut { user: Some(user), screen: false, pip: false }))));
    }
    display.push(item(MenuItem::new("show-non-video", t("Show non-video participants")).checkbox(st.call.show_non_video).keep_open().on_select(move |s: &mut S| acc(s).call.show_non_video ^= true)));
    let (voice_mod, moves, disconnect) = guild.map_or((Vec::new(), None, None), |g| crate::social::moderation::voice_parts(store, g, user, acc));
    let copy = store.settings.developer_mode.then(|| item(super::menus::copy_id(t("Copy user ID"), user))).into_iter().collect::<Vec<_>>();
    if is_me {
        let v = &st.voice;
        let mut media = vec![
            item(MenuItem::new("self-mute", t("Mute")).checkbox(v.self_mute).on_select(move |s: &mut S| acc(s).toggle_mute())),
            item(MenuItem::new("self-deafen", t("Deafen")).checkbox(v.self_deaf).on_select(move |s: &mut S| acc(s).toggle_deaf())),
            focus,
        ];
        if !v.camera {
            media.push(item(MenuItem::new("preview-camera", t("Preview camera")).icon(&icons::EYE_FILL).on_select(move |s: &mut S| {
                let sh = acc(s);
                sh.voice.modal = Some(Modal::CameraPreview);
                sh.intents.push(Intent::Voice(Op::LoadCameras));
                sh.intents.push(Intent::Voice(Op::Preview(true)));
            })));
        }
        return vec![primary, media, display, voice_mod, copy];
    }
    // `participant_items`: user volume, then local mute and the entrance sound.
    let mut parts = participant_items(st, user, p, acc).into_iter().filter(|e| !matches!(e, MenuEntry::Separator));
    let volume: Vec<MenuEntry<S>> = parts.next().into_iter().collect();
    let mut playback: Vec<MenuEntry<S>> = parts.collect();
    playback.push(focus);
    playback.extend(super::menus::relationship_items(store, user, acc).into_iter().map(item));
    let report = crate::social::Modal::Report { target: "user", guild, channel: None, id: user };
    playback.push(item(MenuItem::new("report", t("Report")).icon(&icons::FLAG_FILL).danger().on_select(move |s: &mut S| crate::social::open_report(acc(s), report.clone()))));
    if let Some((None, ch)) = st.voice.channel
        && let Some(call) = store.calls.get(&ch)
        && !store.voice_states.contains_key(&(None, user))
    {
        let ringing = call.ringing.contains(&user);
        playback.push(item(MenuItem::new("ring", if ringing { t("Stop ringing") } else { t("Ring") }).icon(&icons::PHONE_FILL).on_select(op(acc, Op::Ring(ch, user, !ringing)))));
    }
    let mut community: Vec<MenuEntry<S>> = super::menus::invite_entry(store, st, user, acc).filter(|_| !bot).into_iter().collect();
    if let Some(c) = dm {
        community.push(crate::social::mute_entry(store, st, None, Some(c), "conversation", acc));
    }
    let mut manage: Vec<MenuEntry<S>> = guild.and_then(|g| crate::social::moderation::roles_entry(store, g, user, acc)).into_iter().collect();
    manage.extend(moves);
    let mut moderation = voice_mod;
    moderation.extend(disconnect);
    if let Some(g) = guild {
        moderation.extend(crate::social::moderation::punish_items(store, st, g, user, acc).into_iter().map(item));
    }
    vec![primary, volume, playback, community, manage, display, moderation, copy]
}

/// `StreamSettingsMenuContent`'s compactLive variant ("Stream quality"): frame rate and
/// resolution radios. Options above the instance's higher-quality tier are visible but locked
/// when the instance sells Plutonium, like Fluxer's premium badge rows.
fn stream_quality_entries<S: 'static>(st: &ShellState, acc: ShellAccess<S>) -> (Vec<MenuEntry<S>>, Vec<MenuEntry<S>>) {
    let higher = st.limits.higher_video_quality;
    let premium = st.premium;
    let quality = st.voice.settings.screen_quality(&st.limits, st.limits.higher_video_quality);
    let mut fps = Vec::new();
    for value in [15u32, 30, 60] {
        let locked = value > 30 && !higher;
        if locked && !premium {
            continue;
        }
        let label = crate::i18n::tf("{frameRate} FPS", &[("frameRate", &value.to_string())]);
        let mut item = MenuItem::new(format!("stream-fps-{value}"), label).radio(quality.fps == value).keep_open();
        item = if locked {
            item.disabled(true).hint(t("Plutonium"))
        } else {
            item.on_select(move |s: &mut S| {
                let sh = acc(s);
                (sh.voice.settings.screen_fps, sh.voice.settings.streaming_mode) = (value, "custom".into());
                sh.intents.push(Intent::Voice(Op::Settings));
            })
        };
        fps.push(crate::ui::MenuEntry::Item(item));
    }
    let current = quality.max_height.map_or("source".to_string(), |h| h.to_string());
    let mut res = Vec::new();
    for (value, label) in crate::shell::voice::screen_resolutions(true) {
        let locked = value != "480" && value != "720" && !higher;
        if locked && !premium {
            continue;
        }
        let mut item = MenuItem::new(format!("stream-res-{value}"), label).radio(current == value).keep_open();
        item = if locked {
            item.disabled(true).hint(t("Plutonium"))
        } else {
            item.on_select(move |s: &mut S| {
                let sh = acc(s);
                (sh.voice.settings.screen_height, sh.voice.settings.streaming_mode) = (value.parse().ok(), "custom".into());
                sh.intents.push(Intent::Voice(Op::Settings));
            })
        };
        res.push(crate::ui::MenuEntry::Item(item));
    }
    (fps, res)
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
        assert!(t[0].screen && t[0].user == Snowflake(1), "shares come before cameras");
    }

    #[test]
    fn prioritized_tiles_rank_streaming_then_speaking_then_held() {
        let (a, b, c, d) = (vs(1, false), vs(2, true), vs(3, false), vs(4, false));
        let people = vec![(Snowflake(1), &a), (Snowflake(2), &b), (Snowflake(3), &c), (Snowflake(4), &d)];
        let mut st = ShellState::default();
        st.call.prioritize_speakers = true;
        st.voice.speaking.insert(Snowflake(4));
        st.call.activity.insert(Snowflake(3), (Instant::now(), 0));
        st.call.activity.insert(Snowflake(4), (Instant::now(), 1));
        let order: Vec<_> = call_tiles(&st, Snowflake(9), &people).iter().map(|t| (t.user.0, t.screen)).collect();
        assert_eq!(order, [(2, true), (2, false), (4, false), (3, false), (1, false)]);
        st.call.show_non_video = false;
        st.voice.videos.insert(Snowflake(1), Pixels::default());
        let order: Vec<_> = call_tiles(&st, Snowflake(9), &people).iter().map(|t| (t.user.0, t.screen)).collect();
        assert_eq!(order, [(2, true), (1, false)], "only shares and cameras that are on");
    }

    #[test]
    fn pip_snaps_to_the_nearest_corner() {
        assert_eq!(pip_corner(10.0, 10.0, 1280.0, 800.0), 0);
        assert_eq!(pip_corner(900.0, 10.0, 1280.0, 800.0), 1);
        assert_eq!(pip_corner(10.0, 600.0, 1280.0, 800.0), 2);
        assert_eq!(pip_corner(900.0, 600.0, 1280.0, 800.0), 3);
    }

    #[test]
    fn call_avatar_metrics_fit_the_row() {
        assert_eq!(call_avatar_metrics(0, 600.0, 200.0), (64.0, 12.0));
        // Two avatars in a wide row hit the 88 cap; the gap is 200/18 rounded.
        assert_eq!(call_avatar_metrics(2, 600.0, 200.0), (88.0, 11.0));
        // Six in a small box: 3x2 at (200-16)/3 wide or (100-8)/2 tall, floored, min 40.
        assert_eq!(call_avatar_metrics(6, 200.0, 100.0), (46.0, 8.0));
        assert_eq!(call_avatar_metrics(9, 100.0, 60.0).0, 40.0);
    }

    #[test]
    fn compact_height_clamps_and_unread_badge_caps() {
        assert_eq!(compact_height(10.0), COMPACT_HEIGHT_MIN);
        assert_eq!(compact_height(2000.0), COMPACT_HEIGHT_MAX);
        assert_eq!(compact_height(400.0), 400.0);
    }

    #[test]
    fn speaking_participants_go_first_without_reordering_the_rest() {
        let (a, b, c) = (vs(1, false), vs(2, false), vs(3, false));
        let people = vec![(Snowflake(1), &a), (Snowflake(2), &b), (Snowflake(3), &c)];
        let mut speaking = std::collections::HashSet::new();
        speaking.insert(Snowflake(3));
        let ordered = order_participants(&people, &speaking, true);
        assert_eq!(ordered.iter().map(|(u, _)| u.0).collect::<Vec<_>>(), [3, 1, 2]);
        let ordered = order_participants(&people, &speaking, false);
        assert_eq!(ordered.iter().map(|(u, _)| u.0).collect::<Vec<_>>(), [1, 2, 3], "off keeps user-ID order");
    }

    #[test]
    fn pinning_switches_layout_and_grid_clears_it() {
        let mut ui = CallUi::default();
        assert_eq!(ui.pinned(Some(Snowflake(7))), None, "grid hides the pin");
        ui.pin(Some(Snowflake(7)));
        assert_eq!((ui.layout, ui.pinned(Some(Snowflake(7)))), (LayoutMode::Focus, Some(Snowflake(7))));
        ui.set_layout(LayoutMode::Grid);
        assert_eq!(ui.pinned(Some(Snowflake(7))), None);
        ui.set_layout(LayoutMode::Focus);
        ui.pin(None);
        assert_eq!(ui.layout, LayoutMode::Grid, "unpinning returns to the grid");
    }

    #[test]
    fn non_video_participants_can_be_hidden_but_self_stays() {
        let mut camera = vs(1, false);
        camera.self_video = true;
        let mut streaming = vs(2, false);
        streaming.self_stream = true;
        let silent = vs(3, false);
        let people = vec![(Snowflake(1), &camera), (Snowflake(2), &streaming), (Snowflake(3), &silent)];
        let videos = std::collections::HashMap::new();
        let speaking = std::collections::HashSet::new();
        let shown = visible_people(&people, Snowflake(9), false, &videos, &speaking);
        assert_eq!(shown.iter().map(|(u, _)| u.0).collect::<Vec<_>>(), [1, 2], "camera and stream stay, silent goes");
        assert_eq!(visible_people(&people, Snowflake(3), false, &videos, &speaking).len(), 3, "our own tile always stays");
        assert_eq!(visible_people(&people, Snowflake(9), true, &videos, &speaking).len(), 3);
    }

    #[test]
    fn stats_formatting_and_jitter() {
        assert_eq!(stat_percent(0.44), "0.4%");
        assert_eq!(stat_bitrate_kbps(184_000.0), "184 kbps");
        assert!(stat_ms(7.0).starts_with("7 ms"));
        assert_eq!(jitter_from_pings(&[]), 0.0);
        assert_eq!(jitter_from_pings(&[42]), 0.0);
        assert_eq!(jitter_from_pings(&[40, 44, 40]), 4.0);
        assert_eq!(duration_label(Duration::from_secs(23 * 60 + 41)), "23:41");
    }
}
