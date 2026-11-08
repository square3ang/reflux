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
use rune::{Color, El, Pixels, col, el, icon, row, text};

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
}

impl Default for CallUi {
    fn default() -> Self {
        Self {
            layout: LayoutMode::Grid,
            show_own_camera: true,
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

/// `heightToggleUnreadBadge`: 1..9, then `9+`.
pub fn unread_badge(n: u32) -> String {
    if n > 9 { "9+".into() } else { n.to_string() }
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
        (true, true) if v.screen && v.own_share.size().0 > 0 => Some(&v.own_share),
        (true, _) => (v.watching == Some(t.user)).then_some(&v.stream),
        // `showMyOwnCamera`: ours is only hidden from our own view, the track stays published.
        (false, true) => (v.camera && st.call.show_own_camera).then_some(&v.self_view),
        (false, false) => v.videos.get(&t.user),
    }
}

/// `useStreamPreview`: the Active Now preview frame for a participant's stream, if one is loaded.
fn stream_preview(store: &Store, st: &ShellState, guild: Option<Snowflake>, user: Snowflake) -> Option<Pixels> {
    // shortcut: previews reuse Active Now's cached stream thumbnail; a call without one shows
    // the Watch stream placeholder until watched. Subscribe a preview track when LiveKit exposes
    // a cheap subscription for it.
    let channel = st.voice.channel?.1;
    let conn = store.voice_states.get(&(guild, user))?.connection_id.clone()?;
    let key = format!("{}:{channel}:{conn}", guild.map_or("dm".to_string(), |g| g.to_string()));
    let entry = st.voice.previews.get(&key)?;
    let bytes = entry.bytes.clone()?;
    match st.media.local(&format!("stream-preview:{key}:{}", entry.fetched_ms), move || Ok(bytes.to_vec())) {
        crate::media::Load::Ready(px) => Some(px),
        _ => None,
    }
}

/// `VoiceParticipantTile`.
#[allow(clippy::too_many_arguments)]
fn tile<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, t: Tile, vs: &VoiceState, big: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let me = store.my_id();
    let speaking = !t.screen && st.voice.speaking.contains(&t.user);
    let name = store.display_name(guild, t.user);
    let preview = (t.screen && video(st, me, t).is_none()).then(|| stream_preview(store, st, guild, t.user)).flatten();
    let mut body = el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).radius(dim::RADIUS_LG).clip_rounded(dim::RADIUS_LG);
    match (video(st, me, t), &preview) {
        (Some(px), _) => body = body.fill(surface_0()).pixels_fit(px, rune::SurfaceFit::Fit),
        (None, Some(px)) => body = body.fill(surface_0()).pixels_fit(px, rune::SurfaceFit::Fit),
        _ if t.screen => {
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
        _ => {
            // `lkParticipantPlaceholder` on `getPlaceholderAvatarColor` dimmed by 12%, and
            // `.tileAvatarRing`'s size from the tile's (last laid out) size.
            let _ = big;
            let [w, h] = rune::memo::layout_size(&format!("tile-{}-0", t.user)).unwrap_or([300.0, 170.0]);
            let size = (w.min(h) * 0.32).clamp(80.0, 192.0).min(w - 24.0).min(h - 24.0).max(44.0);
            let mut face = el().size(size, size).radius(999.0).child(avatar(store, st, t.user, size, None, surface_3(), p));
            if speaking {
                face = face.child(el().pin(Some(-3.0), Some(-3.0), Some(-3.0), Some(-3.0)).radius(999.0).border(3.0, p.status_online));
            }
            body = body.fill(placeholder_color(store, t.user)).child(row().w_full().h_full().center().child(face));
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
    if st.call.layout == LayoutMode::Focus && st.voice.focus == Some(t.user) {
        // `pinnedParticipant`: the pin glyph marks the focused tile.
        pill = pill.child(glyph(&icons::PUSH_PIN_FILL, rgba(255, 255, 255, 0.9)));
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
    let can_watch = t.screen && preview.is_some();
    // `handleTileClick`: clicking the pinned tile unpins it (back to the grid); any other tile
    // pins it, and an unwatched stream also starts watching (`handleWatch`).
    let was_focused = st.call.pinned(st.voice.focus) == Some(t.user);
    let user = t.user;
    outer
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            if was_focused {
                sh.voice.focus = None;
                sh.call.pin(None);
                return;
            }
            sh.voice.focus = Some(user);
            sh.call.pin(Some(user));
            if can_watch {
                sh.voice.watching = Some(user);
                sh.intents.push(Intent::Watch(Some(user)));
            }
        })
        .on_secondary_press(move |s: &mut S, at, _| {
            let sh = acc(s);
            sh.popups.open_at(TILE_MENU, [at.x, at.y]);
            sh.target = Some(super::Target::User { user, guild, dm: None });
        })
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
fn slot(content: El) -> El {
    row().grow(1.0).min_w(0.0).h_full().center().child(el().aspect_fit(16.0 / 9.0).max_w(9999.0).max_h(9999.0).child(content))
}

/// The tile area: a near-square grid, or one focused/pinned tile over a strip of the rest.
/// Tiles without video show the avatar on its colour, as Fluxer's full call view does.
fn layout<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, people: &[(Snowflake, &VoiceState)], p: &Palette, acc: ShellAccess<S>) -> El {
    let me = store.my_id();
    let people = visible_people(people, me, st.call.show_non_video, &st.voice.videos, &st.voice.speaking);
    let people = order_participants(&people, &st.voice.speaking, st.call.prioritize_speakers);
    let all = tiles(&people);
    let state = |u: Snowflake| people.iter().find(|(x, _)| *x == u).map(|(_, v)| *v);
    let make = |t: Tile, big: bool| state(t.user).map(|vs| tile(store, st, guild, t, vs, big, p, acc)).unwrap_or_else(el);
    let focused = st.call.pinned(st.voice.focus).and_then(|u| all.iter().copied().find(|t| t.user == u && (t.screen || !all.iter().any(|x| x.user == u && x.screen))));
    if let Some(f) = focused {
        let rest: Vec<Tile> = all.iter().copied().filter(|t| *t != f).collect();
        let mut c = col().w_full().h_full().gap(8.0).padding(16.0).child(row().w_full().grow(1.0).min_h(0.0).child(slot(make(f, true))));
        if !rest.is_empty() {
            // `.focusLayoutMembersRow`: 7.25rem high, 0.375rem padding, scrolls sideways when it
            // overflows; `.focusLayoutMembersRowInner` centres (`margin-inline: auto`), so the
            // inner row is at least the viewport's last width.
            const ROW_H: f32 = 116.0;
            let tile_h = ROW_H - 12.0;
            let inner_w = rune::memo::layout_size("call-strip").map_or(0.0, |[w, _]| w);
            let inner = row().min_w(inner_w).h_full().gap(8.0).center_y().justify_center().children(rest.into_iter().map(|t| el().w(tile_h * 16.0 / 9.0).h(tile_h).child(make(t, false))));
            c = c.child(row().key("call-strip").w_full().h(ROW_H).scroll_x().child(inner));
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

/// `.settingsButton`: the caret badge on a control's top-right corner (17px, 2px outside it,
/// black with a light border and shadow) that opens its in-call menu.
fn settings_button<S: 'static>(id: &'static str, label: &str, _p: &Palette, acc: ShellAccess<S>) -> El {
    row()
        .key(id)
        .pin(None, Some(-2.0), Some(-2.0), None)
        .size(17.0, 17.0)
        .center()
        .radius(7.0)
        .fill(rgb(0, 0, 0))
        .border(1.0, rgba(255, 255, 255, 0.14))
        .shadow(6.0, 16.0, rgba(0, 0, 0, 0.5))
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.to_string())
        .hover_group()
        .child(icon(&icons::CARET_DOWN_BOLD, 10.0).color(rgba(255, 255, 255, 0.88)).group_color(Color::WHITE))
        .on_press(move |s: &mut S| acc(s).popups.toggle(id))
}

/// `.buttonContainer`: a control with its caret badge over its corner.
fn control_pair<S: 'static>(button: El, id: &'static str, label: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    el().size(44.0, 44.0).child(button).child(settings_button(id, label, p, acc))
}

/// `VoiceControlBar`: 44px buttons on a black pill, each settings control with its caret menu.
fn control_bar<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
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
    let can_video = super::voice::can(store, v, Permissions::STREAM) && crate::camera::available();
    let mut bar = row()
        .padding(6.0)
        .gap(4.0)
        .center_y()
        .radius(16.0 + 6.0 + 2.0)
        .fill(rgb(0, 0, 0))
        .border(1.0, hud_border());
    let mic_button = button(if v.self_mute { &icons::MICROPHONE_SLASH_FILL } else { &icons::MICROPHONE_FILL }, if v.self_mute { t("Unmute") } else { t("Mute") }, mic.0, mic.1, mic.2, Box::new(move |s: &mut S| acc(s).toggle_mute()));
    bar = bar.child(control_pair(mic_button, MENU_INPUT, t("Input settings"), p, acc));
    let deaf_button = button(if v.self_deaf { &icons::SPEAKER_SLASH_FILL } else { &icons::SPEAKER_HIGH_FILL }, if v.self_deaf { t("Undeafen") } else { t("Deafen") }, deaf.0, deaf.1, deaf.2, Box::new(move |s: &mut S| acc(s).toggle_deaf()));
    bar = bar.child(control_pair(deaf_button, MENU_OUTPUT, t("Output settings"), p, acc));
    if can_video {
        let cam_button = button(if v.camera { &icons::VIDEO_CAMERA_FILL } else { &icons::VIDEO_CAMERA_SLASH_FILL }, if v.camera { t("Turn off camera") } else { t("Turn on camera") }, cam.0, cam.1, cam.2, Box::new(op(acc, Op::ToggleCamera)));
        bar = bar.child(control_pair(cam_button, MENU_CAMERA, t("Camera settings"), p, acc));
    }
    let scr_button = button(&icons::MONITOR_PLAY_FILL, if v.screen { t("End screen share") } else { t("Share your screen") }, scr.0, scr.1, scr.2, Box::new(move |s: &mut S| {
        let sh = acc(s);
        if sh.voice.screen { sh.intents.push(Intent::ToggleScreen) } else { open_screen_picker(sh) }
    }));
    bar = bar.child(control_pair(scr_button, MENU_SCREEN, t("Screen share settings"), p, acc));
    // `VoiceMoreOptionsMenu`: layout, own camera, speakers, region and settings.
    bar = bar.child(
        row()
            .key(MENU_MORE)
            .size(44.0, 44.0)
            .center()
            .radius(16.0)
            .fill(neutral.0)
            .hover(neutral.1)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(t("More options"))
            .child(icon(&icons::DOTS_THREE_BOLD, 22.0).color(neutral.2))
            .on_press(move |s: &mut S| {
                let sh = acc(s);
                sh.popups.toggle(MENU_MORE);
                if sh.popups.is_open(MENU_MORE) {
                    ensure_regions(sh);
                }
            }),
    );
    // `.buttonSeparator`: 1×22, 12% white, before Disconnect.
    bar = bar.child(el().size(1.0, 22.0).radius(999.0).fill(rgba(255, 255, 255, 0.12)));
    bar.child(button(&icons::PHONE_X_FILL, t("Disconnect"), rgb(0xff, 0x3b, 0x30), danger(), Color::WHITE, Box::new(move |s: &mut S| acc(s).leave_voice())))
}

/// `ChannelHeaderIcon`: a 32px header button.
fn header_button<S: 'static>(i: &'static rune::Icon, label: &str, p: &Palette, _acc: ShellAccess<S>, on: impl Fn(&mut S) + 'static) -> El {
    let _ = p;
    row()
        .size(32.0, 32.0)
        .center()
        .radius(8.0)
        .hover(rgba(255, 255, 255, 0.12))
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.to_string())
        .child(icon(i, 20.0).color(rgba(255, 255, 255, 0.9)))
        .on_press(move |s: &mut S| on(s))
}

/// `CompactCallHeightToggleButton`'s chat pill: opens the full call view (chat hidden) and
/// carries the unread badge. The full call view uses the same pill to go back to the list.
pub fn chat_pill<S: 'static>(store: &Store, _st: &ShellState, channel: Snowflake, in_call: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let unread = unread_messages(store, channel);
    let label = if in_call {
        if unread > 0 {
            crate::i18n::tf("Show chat with {count, plural, one {# unread message} other {# unread messages}}", &[("count", &unread.to_string())])
        } else {
            t("Show chat").to_string()
        }
    } else {
        t("Expand call view").to_string()
    };
    let mut pill = row()
        .key("voice-chat-toggle")
        .h(32.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .radius(8.0)
        .hover(rgba(255, 255, 255, 0.12))
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label)
        .child(icon(&icons::CHAT_CIRCLE_TEXT_FILL, 18.0).color(rgba(255, 255, 255, 0.9)));
    if unread > 0 {
        pill = pill.child(
            row()
                .min_w(16.0)
                .h(16.0)
                .px(3.0)
                .center()
                .radius(8.0)
                .fill(danger())
                .child(text(unread_badge(unread)).font(font(10.0, 700.0).lh(12.0)).color(Color::WHITE)),
        );
    }
    let _ = p;
    pill.on_press(move |s: &mut S| {
        let sh = acc(s);
        sh.voice.call_view = !in_call;
    })
}

/// `VoiceParticipantSpeakingAvatarStack`: up to five avatars, the rest behind a `+N` popout.
fn participant_stack<S: 'static>(store: &Store, st: &ShellState, guild: Option<Snowflake>, people: &[(Snowflake, &VoiceState)], p: &Palette, acc: ShellAccess<S>) -> El {
    let mut stack = row().gap(2.0).center_y().role("group").label(t("Voice participants"));
    for (u, _) in people.iter().take(5) {
        let user = *u;
        let speaking = st.voice.speaking.contains(u);
        let mut face = el().size(24.0, 24.0).radius(12.0).clip_rounded(12.0).child(avatar(store, st, user, 24.0, None, surface_3(), p));
        if speaking {
            face = face.border(2.0, p.status_online);
        }
        stack = stack.child(face.cursor(rune::Cursor::Pointer).role("button").label(store.display_name(guild, user)).on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.focus = Some(user);
            sh.call.pin(Some(user));
        }));
    }
    if people.len() > 5 {
        let extra = people.len() - 5;
        stack = stack.child(
            row()
                .min_w(24.0)
                .h(24.0)
                .px(5.0)
                .center()
                .radius(12.0)
                .fill(rgba(255, 255, 255, 0.12))
                .hover(rgba(255, 255, 255, 0.2))
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(crate::i18n::tf("Show all {count} participants", &[("count", &people.len().to_string())]))
                .child(text(format!("+{extra}")).font(font(11.0, 700.0).lh(14.0)).color(Color::WHITE))
                .on_press(move |s: &mut S| acc(s).popups.toggle(PARTICIPANTS)),
        );
    }
    stack
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

/// `VoiceCallView` for the connected channel, in place of the chat column.
pub fn view<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let (guild, channel) = st.voice.channel?;
    let people = participants(store, guild, channel);
    let name = store.channels.get(&channel).map(|c| super::chat::channel_title(store, c)).unwrap_or_default();
    let hud_visible = st.call.hud_until.is_none_or(|t| t > Instant::now());
    let mut header = row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::SPACING_4)
        .gap(dim::SPACING_2)
        .center_y()
        .child(header_button(&icons::LIST_BOLD, t("Back to voice channel"), p, acc, move |s: &mut S| acc(s).voice.call_view = false))
        .child(icon(&icons::SPEAKER_HIGH_FILL, 20.0).color(rgba(255, 255, 255, 0.72)))
        .child(text(name).font(font(16.0, 600.0).lh(20.0)).color(Color::WHITE).truncate().shrink());
    if st.voice.encrypted {
        header = header.child(e2ee_badge());
    }
    let mut right = row().gap(4.0).center_y();
    if !people.is_empty() {
        right = right.child(participant_stack(store, st, guild, &people, p, acc));
    }
    right = right
        .child(
            row()
                .key(PARTICIPANTS)
                .h(32.0)
                .px(6.0)
                .gap(4.0)
                .center_y()
                .radius(8.0)
                .hover(rgba(255, 255, 255, 0.12))
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Voice participants"))
                .child(icon(&icons::USERS_FILL, 18.0).color(rgba(255, 255, 255, 0.9)))
                .child(text(people.len().to_string()).font(font(12.0, 600.0).lh(16.0)).color(rgba(255, 255, 255, 0.9)))
                .on_press(move |s: &mut S| acc(s).popups.toggle(PARTICIPANTS)),
        )
        .child(
            row()
                .key(DETAILS)
                .size(32.0, 32.0)
                .center()
                .radius(8.0)
                .hover(rgba(255, 255, 255, 0.12))
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .label(t("Connection stats"))
                .child(icon(&icons::CHART_BAR_FILL, 20.0).color(if st.popups.is_open(DETAILS) { success() } else { rgba(255, 255, 255, 0.9) }))
                .on_press(move |s: &mut S| acc(s).popups.toggle(DETAILS)),
        )
        .child(header_button(if st.call.app_fullscreen { &icons::CORNERS_IN_BOLD } else { &icons::CORNERS_OUT_BOLD }, if st.call.app_fullscreen { t("Exit fullscreen") } else { t("Enter fullscreen") }, p, acc, move |s: &mut S| {
            let sh = acc(s);
            sh.call.app_fullscreen = !sh.call.app_fullscreen;
        }))
        .child(chat_pill(store, st, channel, true, p, acc));
    header = header.child(el().grow(1.0)).child(right);
    let body = if st.voice.connected {
        layout(store, st, guild, &people, p, acc)
    } else {
        row().w_full().h_full().center().child(text(if st.voice.reconnecting { t("Reconnecting…") } else { t("Connecting…") }).font(font(16.0, 500.0).lh(20.0)).color(rgba(255, 255, 255, 0.72)))
    };
    let mut body = body;
    if let Some(until) = st.call.teleport_until
        && until > Instant::now()
    {
        // shortcut: no region-teleport event from the engine; the overlay runs for a few seconds
        // after the region PATCH. Replace with the reconnect event when reflux-voice exposes it.
        body = el().w_full().grow(1.0).min_h(0.0).child(body).child(teleport_overlay(p));
    }
    let mut root = col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .fill(surface_1());
    if hud_visible {
        root = root.child(header);
    }
    root = root.child(col().w_full().grow(1.0).min_h(0.0).child(body));
    if hud_visible {
        // shortcut: Rune has no pointer-move event, so the chrome hides after the idle timeout
        // and comes back when the pointer enters the call (or Show controls is pressed).
        root = root.child(row().w_full().pb(16.0).pt(8.0).justify_center().child(control_bar(store, st, p, acc)));
    } else {
        root = root.child(row().w_full().py(8.0).justify_center().child(header_button(&icons::CORNERS_OUT_BOLD, t("View call controls"), p, acc, move |s: &mut S| {
            acc(s).call.hud_until = Some(Instant::now() + Duration::from_secs(8));
        })));
    }
    Some(root.on_hover_change(move |s: &mut S, inside, tx: &mut rune::Tx| {
        let sh = acc(s);
        // shortcut: `on_hover_change` reports enter/leave only; Fluxer also re-shows the chrome
        // on every pointer move. A click or hover enter brings it back here.
        sh.call.hud_until = if inside { Some(Instant::now() + HUD_IDLE) } else { Some(Instant::now() - Duration::from_millis(1)) };
        if inside {
            tx.wake_after(HUD_IDLE);
        }
    }))
}

// ---- In-call settings menus (`VoiceSettingsMenus`) ----------------------------------------------

fn device_entries<S: 'static>(devices: Option<&Vec<String>>, current: Option<&str>, input: bool, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let prefix = if input { "input" } else { "output" };
    let mut out = vec![MenuEntry::Item(
        MenuItem::new(format!("{prefix}-default"), t("Default"))
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
            MenuEntry::Item(MenuItem::new(format!("profile-{id}"), t(label)).radio(current == id).on_select(move |s: &mut S| {
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

fn camera_quality_entries<S: 'static>(higher: bool, current: &str, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    [("low", "480p"), ("medium", "720p"), ("high", "1080p")]
        .into_iter()
        .map(|(id, label)| {
            let locked = id == "high" && !higher;
            let item = MenuItem::new(format!("camera-quality-{id}"), t(label)).radio(current == id);
            if locked {
                MenuEntry::Item(item.disabled(true).hint(t("Plutonium")))
            } else {
                MenuEntry::Item(item.on_select(move |s: &mut S| {
                    let sh = acc(s);
                    sh.voice.settings.camera_quality = id.into();
                    sh.intents.push(Intent::Voice(Op::Settings));
                    sh.intents.push(Intent::Voice(Op::Preview(false)));
                    sh.intents.push(Intent::Voice(Op::Preview(true)));
                }))
            }
        })
        .collect()
}

fn screen_mode_entries<S: 'static>(current: &str, higher: bool, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    [("gaming", if higher { "Gaming (up to 1440p60)" } else { "Gaming (up to 720p30)" }), ("screenshare", "Screen share"), ("custom", "Custom")]
        .into_iter()
        .map(|(id, label)| {
            MenuEntry::Item(MenuItem::new(format!("screen-mode-{id}"), t(label)).radio(current == id).on_select(move |s: &mut S| {
                let sh = acc(s);
                sh.voice.settings.streaming_mode = id.into();
                sh.intents.push(Intent::Voice(Op::Settings));
            }))
        })
        .collect()
}

/// The camera's in-call menu (`VoiceCameraSettingsMenu`).
fn camera_menu<S: 'static>(st: &ShellState, _p: &Palette, acc: ShellAccess<S>) -> Menu<S> {
    let v = &st.voice;
    let cameras = v.cameras.as_ref();
    let current = v.settings.camera.as_deref();
    let higher = st.limits.higher_video_quality;
    let quality = camera_quality_entries(higher, &v.settings.camera_quality, acc);
    let mut menu = Menu::new(MENU_CAMERA).placement(Placement::Top);
    menu = menu.entry(MenuEntry::SubmenuHint {
        id: "cameras".into(),
        label: t("Camera").to_string(),
        hint: Some(current.unwrap_or(t("Default")).to_string()),
        disabled: false,
        entries: camera_device_entries(cameras, current, acc),
    });
    menu = menu
        .entry(MenuEntry::Submenu("camera-quality".into(), t("Camera quality").to_string(), quality))
        .item(MenuItem::new("mirror-camera", t("Mirror camera")).checkbox(v.settings.mirror_camera).keep_open().on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.settings.mirror_camera ^= true;
            sh.intents.push(Intent::Voice(Op::Settings));
        }))
        .separator()
        .item(MenuItem::new("preview-camera", t("Preview camera")).icon(&icons::EYE_FILL).on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.modal = Some(Modal::CameraPreview);
            sh.intents.push(Intent::Voice(Op::LoadCameras));
            sh.intents.push(Intent::Voice(Op::Preview(true)));
        }))
        .item(MenuItem::new("video-settings", t("Video settings")).icon(&icons::GEAR_FILL).on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.popups.close();
            sh.open_settings(Some("voice_video"));
        }));
    menu
}

fn screen_menu<S: 'static>(st: &ShellState, _p: &Palette, acc: ShellAccess<S>) -> Menu<S> {
    let v = &st.voice;
    let higher = st.limits.higher_video_quality;
    let mut menu = Menu::new(MENU_SCREEN).placement(Placement::Top);
    if v.screen {
        menu = menu
            .item(MenuItem::new("stop-stream", t("End screen share")).icon(&icons::MONITOR_PLAY_FILL).danger().on_select(move |s: &mut S| acc(s).intents.push(Intent::ToggleScreen)))
            .item(MenuItem::new("change-stream", t("Change stream")).icon(&icons::ARROWS_LEFT_RIGHT_FILL).on_select(move |s: &mut S| open_screen_picker(acc(s))));
        for e in stream_quality_entries(st, acc) {
            menu = menu.entry(e);
        }
    } else {
        menu = menu.entry(MenuEntry::Group(t("Streaming mode").to_string()));
        for e in screen_mode_entries(&v.settings.streaming_mode, higher, acc) {
            menu = menu.entry(e);
        }
        for e in stream_quality_entries(st, acc) {
            menu = menu.entry(e);
        }
        menu = menu
            .separator()
            .item(MenuItem::new("share-now", t("Share your screen")).icon(&icons::MONITOR_ARROW_UP_FILL).on_select(move |s: &mut S| open_screen_picker(acc(s))));
    }
    menu.item(MenuItem::new("screen-settings", t("Screen share settings")).icon(&icons::GEAR_FILL).on_select(move |s: &mut S| {
        let sh = acc(s);
        sh.popups.close();
        sh.open_settings(Some("voice_video"));
    }))
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
        .item(MenuItem::new("push-to-talk", t("Push-to-talk")).checkbox(set.push_to_talk).keep_open().on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.settings.push_to_talk ^= true;
            sh.intents.push(Intent::Voice(Op::Settings));
        }))
        .item(MenuItem::new("deafen", t("Deafen")).checkbox(v.self_deaf).keep_open().on_select(move |s: &mut S| acc(s).toggle_deaf()))
        .separator()
        .item(MenuItem::new("voice-settings", t("Voice settings")).icon(&icons::GEAR_FILL).on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.popups.close();
            sh.open_settings(Some("voice_video"));
        }))
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
        .item(MenuItem::new("deafen", t("Deafen")).checkbox(v.self_deaf).keep_open().on_select(move |s: &mut S| acc(s).toggle_deaf()))
        .item(MenuItem::new("voice-settings", t("Voice settings")).icon(&icons::GEAR_FILL).on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.popups.close();
            sh.open_settings(Some("voice_video"));
        }))
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

/// The region list from `GET /channels/:id/call/regions` (Automatic always first).
pub fn region_options(st: &ShellState) -> Vec<(String, String)> {
    let Some((None, ch)) = st.voice.channel else { return Vec::new() };
    let key = format!("voice-regions-{ch}");
    let mut out = vec![("automatic".to_string(), t("Automatic").to_string())];
    if let Some(super::expr::Fetched::Done(serde_json::Value::Array(list))) = st.expr.fetched.get(&key) {
        for r in list {
            let Some(id) = r["id"].as_str() else { continue };
            if id == "automatic" {
                continue;
            }
            let name = r["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(id);
            out.push((id.to_string(), name.to_string()));
        }
    }
    out
}

fn region_entries<S: 'static>(st: &ShellState, acc: ShellAccess<S>) -> Vec<MenuEntry<S>> {
    let Some((_, ch)) = st.voice.channel else { return Vec::new() };
    let current = st.call.region.clone();
    let options = region_options(st);
    if options.len() == 1 && !matches!(st.expr.fetched.get(&format!("voice-regions-{ch}")), Some(super::expr::Fetched::Done(_))) {
        return vec![MenuEntry::Item(MenuItem::new("regions-loading", t("Loading…")).disabled(true))];
    }
    options
        .into_iter()
        .map(|(id, name)| {
            let pick = id.clone();
            MenuEntry::Item(MenuItem::new(format!("region-{id}"), name).radio(current.as_deref() == Some(id.as_str())).on_select(move |s: &mut S| {
                let sh = acc(s);
                sh.call.teleport_until = Some(Instant::now() + Duration::from_secs(3));
                sh.call.region = (pick != "automatic").then(|| pick.clone());
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

/// `VoiceMoreOptionsMenu`: region, layout, own camera, non-video participants, speaker
/// priority, standard-call switch and the settings entry.
fn more_menu<S: 'static>(store: &Store, st: &ShellState, _p: &Palette, acc: ShellAccess<S>) -> Menu<S> {
    let v = &st.voice;
    let mut menu = Menu::new(MENU_MORE).placement(Placement::Top).min_w(300.0);
    if matches!(v.channel, Some((None, _))) {
        let regions = region_entries(st, acc);
        let hint = st.call.region.as_ref().and_then(|id| region_options(st).into_iter().find(|(i, _)| i == id).map(|(_, n)| n));
        menu = menu.entry(MenuEntry::SubmenuHint {
            id: "voice-region".into(),
            label: t("Voice region").to_string(),
            hint: Some(hint.unwrap_or_else(|| t("Automatic").to_string())),
            disabled: false,
            entries: regions,
        });
    }
    let is_grid = st.call.layout == LayoutMode::Grid;
    let first = v.channel.and_then(|(g, c)| participants(store, g, c).first().map(|(u, _)| *u));
    menu = menu
        .item(
            MenuItem::new("grid-view", t("Grid view")).icon(&icons::GRID_FOUR_FILL).checkbox(is_grid).keep_open().on_select(move |s: &mut S| {
                let sh = acc(s);
                if sh.call.layout == LayoutMode::Grid {
                    sh.call.set_layout(LayoutMode::Focus);
                    sh.voice.focus = first.or(sh.voice.focus);
                } else {
                    sh.call.set_layout(LayoutMode::Grid);
                    sh.voice.focus = None;
                }
            }),
        )
        .item(
            MenuItem::new("show-own-camera", t("Show my own camera")).checkbox(st.call.show_own_camera).on_select(move |s: &mut S| {
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
            }),
        )
        .item(
            MenuItem::new("show-non-video", t("Show non-video participants"))
                .checkbox(st.call.show_non_video)
                .keep_open()
                .on_select(move |s: &mut S| acc(s).call.show_non_video ^= true),
        )
        .item(
            MenuItem::new("prioritize-speakers", t("Prioritize speakers"))
                .checkbox(st.call.prioritize_speakers)
                .keep_open()
                .on_select(move |s: &mut S| acc(s).call.prioritize_speakers ^= true),
        );
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
                }),
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

/// Keeps the menus' side effects in step each frame: the input menu runs the mic meter.
pub fn poll(st: &mut ShellState) {
    let channel = st.voice.channel.map(|(_, c)| c);
    if channel != st.call.region_channel {
        st.call.region_channel = channel;
        st.call.region = None;
    }
    if channel.is_none() {
        st.call.app_fullscreen = false;
        st.call.hide_camera_confirm = None;
        st.call.stats = None;
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

/// `CompactVoiceCallView` over a DM / group DM with an active call: tiles, then Join or the
/// control bar.
pub fn dm_strip<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> Option<El> {
    if !matches!(c.kind, ChannelType::DM | ChannelType::GROUP_DM) {
        return None;
    }
    let call = store.calls.get(&c.id)?;
    let people = participants(store, None, c.id);
    let joined = st.voice.channel == Some((None, c.id));
    let me = store.my_id();
    let ringing: Vec<Snowflake> = call.ringing.iter().copied().filter(|u| *u != me).collect();
    let visible = visible_people(&people, me, st.call.show_non_video, &st.voice.videos, &st.voice.speaking);
    let visible = order_participants(&visible, &st.voice.speaking, st.call.prioritize_speakers);
    let all = tiles(&visible);
    let has_media = all.iter().any(|t| t.screen && stream_preview(store, st, None, t.user).is_some() || video(st, me, *t).is_some());
    let mut row_tiles = row().w_full().h(150.0).gap(8.0).justify_center().items_center();
    if !has_media {
        for (u, vs) in &visible {
            let user = *u;
            let name = store.display_name(None, user);
            let mut face = col()
                .w(135.0)
                .h_full()
                .center()
                .gap(6.0)
                .child(avatar(store, st, user, 64.0, None, surface_3(), p))
                .child(
                    row()
                        .gap(4.0)
                        .center_y()
                        .child(text(name).font(font(12.0, 500.0).lh(14.4)).color(rgba(255, 255, 255, 0.9)).max_w(110.0).truncate())
                        .child(if vs.mute || vs.self_mute { icon(&icons::MICROPHONE_SLASH_FILL, 12.0).color(danger()) } else { el() }),
                );
            if st.voice.speaking.contains(u) {
                face = face.child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).radius(12.0).border(2.0, p.status_online));
            }
            row_tiles = row_tiles.child(face);
        }
    } else {
        for t in &all {
            let vs = people.iter().find(|(u, _)| *u == t.user).map(|(_, v)| *v);
            if let Some(vs) = vs {
                row_tiles = row_tiles.child(el().w(240.0).h_full().child(tile(store, st, None, *t, vs, false, p, acc)));
            }
        }
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
    let mut strip = col().w_full().pt(16.0).pb(12.0).gap(12.0).items_center().fill(surface_1()).child(row_tiles).child(footer);
    if super::voice::is_p2p(store, None, ch) {
        let pops = crate::ui::popup_access(move |s: &mut S| &mut acc(s).popups);
        let tip = crate::ui::Tooltip::new("dm-call-p2p", t("Direct connection. Audio and video go directly between the people in the call, who can see each other's IP address."));
        strip = strip.child(tip.el(p2p_indicator(joined && !st.voice.p2p_failed.is_empty(), p), &st.popups, pops, p, layer));
    }
    Some(strip)
}

/// `ChannelCompactCallSurface`: the call above a voice channel's text chat — the channel's
/// participants and a Join button, or, once joined, the tiles with the call controls. The
/// surface resizes by its bottom handle and remembers that height per channel.
pub fn guild_strip<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if c.kind != ChannelType::GUILD_VOICE {
        return None;
    }
    let guild = c.guild_id?;
    let people = participants(store, Some(guild), c.id);
    let joined = st.voice.channel == Some((Some(guild), c.id));
    let ch = c.id;
    if joined {
        // `getCompactHeightMax`: the window keeps room for the chat under the call.
        let window_h = rune::memo::layout_size(super::BODY_KEY).map_or(800.0, |[_, h]| h);
        let max_h = (window_h - COMPACT_CHAT_RESERVE).clamp(COMPACT_HEIGHT_MIN, COMPACT_HEIGHT_MAX);
        let height = compact_height(st.prefs.compact_call_heights.get(&ch).copied().unwrap_or(COMPACT_HEIGHT_DEFAULT)).min(max_h);
        let width = rune::memo::layout_size(&format!("compact-call-{ch}")).map_or(640.0, |[w, _]| w);
        let me = store.my_id();
        let visible = visible_people(&people, me, st.call.show_non_video, &st.voice.videos, &st.voice.speaking);
        let visible = order_participants(&visible, &st.voice.speaking, st.call.prioritize_speakers);
        let tiles_all = tiles(&visible);
        let has_media = tiles_all.iter().any(|t| t.screen && stream_preview(store, st, Some(guild), t.user).is_some() || video(st, me, *t).is_some());
        // `CompactVoiceCallView.module.css`'s reserves: the edge padding, the control bar's
        // (56px buttons + padding) and what the content keeps clear of it.
        let edge = (height * 0.045).clamp(12.0, 24.0);
        let control_h = 58.0 + (height * 0.04).clamp(16.0, 24.0) + 12.0;
        let control_reserve = control_h + (height * 0.055).clamp(16.0, 28.0);
        let footer = control_reserve + (height * 0.052).clamp(14.0, 22.0);
        let pad_bottom = edge.max(footer.min(height * 0.34));
        let pad_top = edge.max((height * 0.065).clamp(18.0, 28.0).min(control_reserve * 0.34));
        let mut content = row().w_full().grow(1.0).min_h(0.0).center().fill(rgb(0, 0, 0)).px(edge).pt(pad_top).pb(pad_bottom);
        if people.is_empty() {
            content = content.child(text(t("No one else is here yet.")).font(font(14.0, 500.0).lh(18.0)).color(rgba(255, 255, 255, 0.55)));
        } else if !has_media {
            // `audioAvatarLayout` + `VoiceParticipantWrappedAvatarList`: wrapping avatars,
            // clamp(44, 18% of the smaller side, 88), with their mute / deafen badges.
            let content_h = height - pad_top - pad_bottom;
            let size = (width.min(content_h + pad_top + pad_bottom) * 0.18).clamp(44.0, 88.0);
            let gap = (height * 0.024).clamp(8.0, 16.0);
            let mut list = row().w_full().max_w(768.0).flex_wrap().justify_center().items_center().gaps(gap, gap);
            for (u, vs) in &visible {
                let user = *u;
                let name = store.display_name(Some(guild), user);
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
                list = list.child(face);
            }
            content = content.child(list);
        } else {
            let mut row_tiles = row().w_full().h_full().gap(8.0).justify_center().items_center();
            for t in &tiles_all {
                let vs = people.iter().find(|(u, _)| *u == t.user).map(|(_, v)| *v);
                if let Some(vs) = vs {
                    row_tiles = row_tiles.child(el().grow(1.0).w(0.0).min_w(120.0).max_w(360.0).h_full().child(tile(store, st, Some(guild), *t, vs, false, p, acc)));
                }
            }
            content = content.child(row_tiles);
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
        });
        let controls = row().pin(Some(0.0), None, Some(0.0), Some(0.0)).pt((height * 0.04).clamp(16.0, 24.0)).px(16.0).pb(12.0).justify_center().child(control_bar(store, st, p, acc));
        let handle = el()
            .key(format!("compact-call-resize-{ch}"))
            .w_full()
            .h(8.0)
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
            });
        // `.resizeHandle`: the 16px strip along the bottom edge.
        let handle = handle.pin(Some(0.0), None, Some(0.0), Some(0.0)).h(16.0);
        return Some(
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
                .child(handle),
        );
    }
    let mut strip = col().w_full().pt(12.0).pb(10.0).gap(10.0).items_center().fill(surface_1());
    let mut faces = row().gap(6.0).center_y();
    for (u, _) in people.iter().take(8) {
        faces = faces.child(avatar(store, st, *u, 32.0, None, surface_3(), p));
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

/// `CallButtons`: start a voice / video call from the DM header (join it when one runs).
pub fn header_action<S: 'static>(store: &Store, st: &ShellState, c: &Channel, video: bool, el_: El, acc: ShellAccess<S>) -> El {
    let ch = c.id;
    let active = store.calls.contains_key(&ch);
    let joined = st.voice.channel == Some((None, ch));
    if joined {
        return el_.opacity(0.5);
    }
    // `canOfferP2pCall`: the rollout is on and everyone fits in a P2P call.
    let (p2p_on, cap) = st.voice.p2p_rollout;
    let offer_p2p = !active && p2p_on && c.recipients.iter().filter(|u| u.id != store.my_id()).count() < cap;
    el_.on_press(move |s: &mut S| {
        let sh = acc(s);
        // `getCallStartRequestOptions`: Shift skips the P2P offer and the confirmation.
        let bypass = sh.shift_held;
        if offer_p2p && !bypass {
            sh.voice.modal = Some(Modal::P2pConsent { guild: None, channel: ch, start: true, allow_standard: true, video, always: false });
        } else if !active && !bypass && crate::look::with(|l| l.confirm_calls) {
            sh.voice.modal = Some(Modal::StartCall { channel: ch, video, tip: true });
        } else {
            start_call(sh, ch, active, video);
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

/// `IncomingCallUI`: the first call ringing us that we haven't answered or dismissed.
/// The developer tools' `MockIncomingCall` comes first; its buttons only dismiss it.
pub fn incoming<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let me = store.my_id();
    let mock = super::devtools::mock_call(store, st);
    let mocked = mock.is_some();
    let (caller, name, channel) = match mock {
        Some((caller, name)) => (caller, name, Snowflake(0)),
        None => {
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
            (caller, name, call.channel_id)
        }
    };
    let end_mock = move |s: &mut S| acc(s).devtools.mock_call = None;
    let decline: Box<dyn Fn(&mut S)> = if mocked { Box::new(end_mock) } else { Box::new(op(acc, Op::DeclineCall(channel))) };
    let accept: Box<dyn Fn(&mut S)> = if mocked { Box::new(end_mock) } else { Box::new(op(acc, Op::AcceptCall(channel))) };
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
                .child(circle(&icons::PHONE_X_FILL, rgb(0xff, 0x3b, 0x30), rgb(0xff, 0x45, 0x3a), t("Decline"), decline))
                .child(circle(&icons::PHONE_FILL, rgb(0x34, 0xc7, 0x59), rgb(0x30, 0xd1, 0x58), t("Accept"), accept)),
        )
        .child(
            text(t("Ignore"))
                .font(font(13.0, 500.0).lh(16.0))
                .color(rgba(235, 235, 245, 0.5))
                .cursor(rune::Cursor::Pointer)
                .role("button")
                .on_press(move |s: &mut S| {
                    let st = acc(s);
                    if mocked {
                        st.devtools.mock_call = None;
                    } else {
                        st.voice.dismissed_calls.insert(channel);
                    }
                }),
        );
    Some(el().pin(Some(0.0), Some(24.0), Some(0.0), None).child(row().w_full().justify_center().child(card)))
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
                body = body.child(body_text(t("No capture devices found."), p));
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
                            .child(icon(&icons::VIDEO_CAMERA, 18.0).color(if picked { p.brand_primary } else { p.text_primary_muted }))
                            .child(text(name).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary))
                            .on_press(move |s: &mut S| {
                                let sh = acc(s);
                                sh.voice.screen_device = Some(pick.clone());
                                sh.voice.screen_pick = None;
                            }),
                    );
                }
                body = body.child(cards);
            }
        }
        _ if !listed => {
            body = body.child(body_text("Choose the stream quality, then pick a screen or window in the system dialog.", p))
        }
        _ => match &v.screen_sources {
            None => body = body.child(body_text(t("Loading…"), p)),
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
                    body = body.child(body_text(t(if windows { "No application windows found." } else { "No displays found." }), p));
                } else {
                    body = body.child(row().w_full().flex_wrap().gap(8.0).children(cards));
                }
            }
        },
    }
    body = body.child(row().w_full().gap(16.0).child(r).child(f));
    if let Some(preset) = set.screen_preset() {
        let note = crate::i18n::tf("Set by the {preset} preset. Picking a value here switches to Custom.", &[("preset", t(preset.0))]);
        body = body.child(text(note).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted).wrap());
    }
    if !higher && st.premium {
        let note = crate::i18n::tf("Frame rates above 30 FPS require {premiumProductName}", &[("premiumProductName", "Plutonium")]);
        body = body.child(text(note).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted).wrap());
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
                .el(&st.popups, pops(acc), p, layer),
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
            .child(crate::ui::Switch::new(set.noise_suppression).label(t("Noise suppression")).description("RNNoise").on_toggle(change(|s| s.noise_suppression ^= true)).el(p))
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

/// A labelled slider row in a menu (`MenuSliderType`), 0..200 %.
fn menu_slider<S: 'static>(key: String, label: &str, value: f32, p: &Palette, on: impl Fn(&mut S, f32) + 'static) -> crate::ui::MenuEntry<S> {
    crate::ui::menu_slider(key, label, value, 0.0, 200.0, format!("{value:.0}%"), p, on)
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

/// `VoiceParticipantMenuItems` for a tile.
pub fn tile_menu<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if !st.popups.is_open(TILE_MENU) {
        return;
    }
    let Some(super::Target::User { user, guild, .. }) = st.target else { return };
    let me = store.my_id();
    let mut m = Menu::new(TILE_MENU);
    let focused = st.call.layout == LayoutMode::Focus && st.voice.focus == Some(user);
    m = m.item(MenuItem::new("pin", if focused { t("Unpin") } else { t("Pin") }).icon(if focused { &icons::PUSH_PIN_SIMPLE_SLASH } else { &icons::PUSH_PIN_SIMPLE }).on_select(move |s: &mut S| {
        let sh = acc(s);
        sh.voice.focus = (!focused).then_some(user);
        sh.call.pin((!focused).then_some(user));
    }));
    let streaming = store.voice_states.get(&(guild, user)).is_some_and(|v| v.self_stream);
    if streaming && user == me {
        m = m
            .item(MenuItem::new("stop-stream", t("Stop streaming")).danger().on_select(move |s: &mut S| acc(s).intents.push(Intent::ToggleScreen)))
            .item(MenuItem::new("change-stream", t("Change stream")).icon(&icons::MONITOR_PLAY_FILL).on_select(move |s: &mut S| open_screen_picker(acc(s))))
            .submenu("stream-quality", t("Stream quality"), stream_quality_entries(st, acc));
    }
    if streaming && user != me {
        m = m.item(MenuItem::new("watch", t("Watch stream")).icon(&icons::MONITOR_PLAY_FILL).on_select(move |s: &mut S| {
            let sh = acc(s);
            sh.voice.watching = Some(user);
            sh.intents.push(Intent::Watch(Some(user)));
        }));
    }
    // `VoicePopoutHost`: the participant's camera or stream in its own window.
    let watching = st.voice.watching == Some(user);
    if streaming && watching {
        m = m
            .item(MenuItem::new("popout-stream", t("Pop out stream")).icon(&icons::ARROW_SQUARE_OUT).on_select(op(acc, Op::PopOut { user: Some(user), screen: true, pip: false })))
            .item(MenuItem::new("pip-stream", "Picture in picture").icon(&icons::PICTURE_IN_PICTURE).on_select(op(acc, Op::PopOut { user: Some(user), screen: true, pip: true })));
    }
    if st.voice.videos.contains_key(&user) || (user == me && st.voice.camera) {
        m = m.item(MenuItem::new("popout-camera", t("Pop out camera")).icon(&icons::ARROW_SQUARE_OUT).on_select(op(acc, Op::PopOut { user: Some(user), screen: false, pip: false })));
    }
    m = m.item(MenuItem::new("popout-call", "Pop out call").icon(&icons::ARROW_SQUARE_OUT).on_select(op(acc, Op::PopOut { user: None, screen: false, pip: false })));
    let set = &st.voice.settings;
    // Settings changes save and reach the mixer through `Op::Settings`.
    let tweak = move |s: &mut S, f: &dyn Fn(&mut super::voice::Settings)| {
        let sh = acc(s);
        f(&mut sh.voice.settings);
        sh.intents.push(Intent::Voice(Op::Settings));
    };
    if streaming && watching && user != me {
        let vol = set.stream_volumes.get(&user.0).copied().unwrap_or(100.0);
        let muted = set.stream_mutes.contains(&user.0);
        m = m
            .separator()
            .entry(menu_slider(format!("stream-vol-{user}"), t("Stream volume"), vol, p, move |s: &mut S, x| tweak(s, &|st| { st.stream_volumes.insert(user.0, x); })))
            .item(MenuItem::new("stream-mute", t("Mute stream")).checkbox(muted).on_select(move |s: &mut S| {
                tweak(s, &|st| {
                    if !st.stream_mutes.remove(&user.0) {
                        st.stream_mutes.insert(user.0);
                    }
                })
            }));
    }
    if user != me {
        for e in participant_items(st, user, p, acc) {
            m = m.entry(e);
        }
        if let Some((None, ch)) = st.voice.channel
            && let Some(call) = store.calls.get(&ch)
            && !store.voice_states.contains_key(&(None, user))
        {
            let ringing = call.ringing.contains(&user);
            m = m.item(MenuItem::new("ring", if ringing { t("Stop ringing") } else { t("Ring") }).icon(&icons::PHONE_FILL).on_select(op(acc, Op::Ring(ch, user, !ringing))));
        }
    }
    if let Some(g) = guild {
        for e in crate::social::moderation::voice_items(store, g, user, acc) {
            m = m.entry(e);
        }
    }
    m.el(&st.popups, pops(acc), p, layer);
}

/// `StreamSettingsMenuContent`'s compactLive variant ("Stream quality"): frame rate and
/// resolution radios. Options above the instance's higher-quality tier are visible but locked
/// when the instance sells Plutonium, like Fluxer's premium badge rows.
fn stream_quality_entries<S: 'static>(st: &ShellState, acc: ShellAccess<S>) -> Vec<crate::ui::MenuEntry<S>> {
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
        let mut item = MenuItem::new(format!("stream-fps-{value}"), label).radio(quality.fps == value);
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
        let mut item = MenuItem::new(format!("stream-res-{value}"), label).radio(current == value);
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
    vec![
        crate::ui::MenuEntry::Submenu("stream-quality-fps".into(), t("Frame rate").to_string(), fps),
        crate::ui::MenuEntry::Submenu("stream-quality-res".into(), t("Resolution").to_string(), res),
    ]
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

    #[test]
    fn compact_height_clamps_and_unread_badge_caps() {
        assert_eq!(compact_height(10.0), COMPACT_HEIGHT_MIN);
        assert_eq!(compact_height(2000.0), COMPACT_HEIGHT_MAX);
        assert_eq!(compact_height(400.0), 400.0);
        assert_eq!(unread_badge(0), "0");
        assert_eq!(unread_badge(3), "3");
        assert_eq!(unread_badge(9), "9");
        assert_eq!(unread_badge(10), "9+");
        assert_eq!(unread_badge(120), "9+");
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
