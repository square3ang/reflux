//! Voice: the connection panel over the user area (`voice/components/VoiceConnectionStatus*`,
//! `voice_connection_status/LocalParticipantControls.tsx`), the signal icon
//! (`SignalStrengthIcon.tsx`, `VoiceLatencySignal.ts`), the local mute/deafen rules
//! (`LocalVoiceStateMachine.ts`) and a minimal stream viewer.

use std::collections::HashSet;

use reflux_model::{Permissions, Snowflake};
use reflux_store::Store;
use rune::{Color, El, Icon, Pixels, col, el, icon, row, text};

use super::{Intent, ShellAccess, ShellState};
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::mix;

/// What the user area and sidebar show about voice (`MediaEngine` + `LocalVoiceState`).
#[derive(Default)]
pub struct Voice {
    /// (guild, channel) joined or being joined.
    pub channel: Option<(Option<Snowflake>, Snowflake)>,
    /// LiveKit room connected (else "Connecting...").
    pub connected: bool,
    /// The last join that failed ("Connection failed", retry / dismiss).
    pub failed: Option<(Option<Snowflake>, Snowflake)>,
    pub self_mute: bool,
    pub self_deaf: bool,
    /// Undeafening restores the mic (it was on when deafening muted it).
    pub unmute_on_undeafen: bool,
    /// Sharing the screen, or waiting for the portal picker.
    pub screen: bool,
    /// Users LiveKit reports as active speakers.
    pub speaking: HashSet<Snowflake>,
    /// Latest round-trip time and the history the signal icon compares it against.
    pub ping: Option<u32>,
    pub pings: Vec<u32>,
    /// The user whose screen share the viewer shows, and its frames.
    pub watching: Option<Snowflake>,
    pub stream: Pixels,
    /// Microphone and speaker names (`None` until listed), and the chosen ones.
    pub devices: Option<(Vec<String>, Vec<String>)>,
    pub input: Option<String>,
    pub output: Option<String>,
}

impl ShellState {
    /// Clicking a voice channel: join it (switching from another one).
    pub fn join_voice(&mut self, guild: Option<Snowflake>, channel: Snowflake) {
        let v = &mut self.voice;
        if v.channel == Some((guild, channel)) {
            return;
        }
        (v.channel, v.connected, v.failed, v.screen, v.ping, v.watching) = (Some((guild, channel)), false, None, false, None, None);
        v.speaking.clear();
        v.pings.clear();
        self.intents.push(Intent::JoinVoice { guild, channel });
    }

    pub fn leave_voice(&mut self) {
        self.voice.failed = None;
        if self.voice.channel.is_some() {
            self.intents.push(Intent::LeaveVoice);
        }
    }

    /// `toggleSelfMute`: unmuting while deafened undeafens too.
    pub fn toggle_mute(&mut self) {
        let v = &mut self.voice;
        if v.self_deaf && v.self_mute {
            (v.self_mute, v.self_deaf) = (false, false);
        } else {
            v.self_mute = !v.self_mute;
        }
        v.unmute_on_undeafen = false;
        self.intents.push(Intent::VoiceSelf);
    }

    /// `toggleSelfDeaf`: deafening mutes; undeafening unmutes only if deafening muted.
    pub fn toggle_deaf(&mut self) {
        let v = &mut self.voice;
        if v.self_deaf {
            v.self_deaf = false;
            if v.unmute_on_undeafen {
                v.self_mute = false;
            }
            v.unmute_on_undeafen = false;
        } else {
            v.unmute_on_undeafen = !v.self_mute;
            (v.self_deaf, v.self_mute) = (true, true);
        }
        self.intents.push(Intent::VoiceSelf);
    }
}

/// Whether the connected channel lets us speak (`Permissions.SPEAK`); true outside voice.
pub fn can(store: &Store, v: &Voice, perm: Permissions) -> bool {
    v.channel.is_none_or(|(g, c)| g.is_none() || store.permissions(c).contains(perm))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    Green,
    Yellow,
    Orange,
    Red,
}

/// `getLatencySignalState`: filled arcs and tone from how far `ping` exceeds the median of the
/// last 30 samples. `None` while measuring.
pub fn signal(ping: Option<u32>, history: &[u32]) -> Option<(usize, Tone)> {
    let ping = ping?;
    let mut recent: Vec<u32> = history[history.len().saturating_sub(30)..].iter().copied().filter(|&l| l > 0).collect();
    recent.sort_unstable();
    let n = recent.len();
    let baseline = match n {
        0 => ping,
        _ if n % 2 == 1 => recent[n / 2],
        _ => (recent[n / 2 - 1] + recent[n / 2]).div_ceil(2),
    };
    Some(match ping.saturating_sub(baseline) {
        e if e < 70 => (4, Tone::Green),
        e if e < 160 => (3, Tone::Yellow),
        e if e < 300 => (2, Tone::Orange),
        _ => (1, Tone::Red),
    })
}

// `SignalStrengthIcon` at size 16, stroke 2: a dot at (1, 15) and four quarter arcs around it.
static SIGNAL_DOT: Icon = Icon { name: "signal-dot", view: 16.0, body: r#"<circle cx="1" cy="15" r="1.5" fill="currentColor"/>"# };
static SIGNAL_ARCS: [Icon; 4] = [
    Icon { name: "signal-arc-1", view: 16.0, body: r#"<path d="M 1 11.5 A 3.5 3.5 0 0 1 4.5 15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"# },
    Icon { name: "signal-arc-2", view: 16.0, body: r#"<path d="M 1 8 A 7 7 0 0 1 8 15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"# },
    Icon { name: "signal-arc-3", view: 16.0, body: r#"<path d="M 1 4.5 A 10.5 10.5 0 0 1 11.5 15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"# },
    Icon { name: "signal-arc-4", view: 16.0, body: r#"<path d="M 1 1 A 14 14 0 0 1 15 15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>"# },
];

fn signal_icon(v: &Voice, p: &Palette) -> El {
    let state = signal(v.ping, &v.pings);
    let tone = state.map(|(_, t)| match t {
        Tone::Green => p.status_online,
        Tone::Yellow | Tone::Orange => p.status_idle,
        Tone::Red => p.status_danger,
    });
    let filled = state.map_or(0, |(n, _)| n);
    let mut glyph = el().size(16.0, 16.0).child(icon(&SIGNAL_DOT, 16.0).color(tone.unwrap_or(p.text_tertiary)).absolute(0.0, 0.0));
    for (i, arc) in SIGNAL_ARCS.iter().enumerate() {
        let c = if i < filled { tone.unwrap_or(p.text_tertiary_muted) } else { p.text_tertiary_muted };
        glyph = glyph.child(icon(arc, 16.0).color(c).absolute(0.0, 0.0));
    }
    let label = match v.ping {
        Some(ms) => format!("Ping: {ms}ms"),
        None => "Measuring latency...".into(),
    };
    row().size(24.0, 24.0).center().role("img").label(label).child(glyph)
}

/// Height the panel adds above the user row: padding 8 + status row 32 + gap 12 + source line 16
/// + gap 12 + media buttons 32 + padding 8, and the 1px divider under it.
pub const PANEL_HEIGHT: f32 = 8.0 + 32.0 + 12.0 + 16.0 + 12.0 + 32.0 + 8.0 + 1.0;
/// The failed variant has only the status row.
pub const FAILED_HEIGHT: f32 = 8.0 + 32.0 + 8.0 + 1.0;

pub fn panel_height(v: &Voice) -> f32 {
    if v.channel.is_some() {
        PANEL_HEIGHT
    } else if v.failed.is_some() {
        FAILED_HEIGHT
    } else {
        0.0
    }
}

/// `.controlButton`: 32px, transparent, hover tint.
fn control_button<S: 'static>(i: &'static Icon, label: &str, p: &Palette, on: impl Fn(&mut S) + 'static) -> El {
    row()
        .size(32.0, 32.0)
        .center()
        .radius(dim::RADIUS_MD)
        .hover(mix(p.control_button_normal_text, Color::TRANSPARENT, 10.0))
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(label.to_string())
        .on_press(on)
        .child(icon(i, 20.0).color(p.control_button_normal_text).group_color(p.control_button_hover_text))
}

/// `.mediaButton`: half the row, 32 tall, a 1px hover-coloured border; green while active.
fn media_button<S: 'static>(i: &'static Icon, label: &str, active: bool, disabled: bool, p: &Palette, on: impl Fn(&mut S) + 'static) -> El {
    let (bg, fg, border) = if active {
        (mix(p.status_online, Color::TRANSPARENT, 15.0), p.status_online, mix(p.status_online, Color::TRANSPARENT, 30.0))
    } else {
        (mix(p.background_modifier_hover, Color::TRANSPARENT, 70.0), p.control_button_normal_text, p.background_modifier_hover)
    };
    let mut b = row()
        .grow(1.0)
        .w(0.0)
        .h(32.0)
        .center()
        .radius(dim::RADIUS_MD)
        .fill(bg)
        .border(1.0, border)
        .role("button")
        .label(label.to_string())
        .hover_group()
        .child(icon(i, 20.0).color(fg).group_color(if active || disabled { fg } else { p.control_button_hover_text }));
    if disabled {
        b = b.opacity(0.5);
    } else {
        let hover = if active { mix(p.status_online, Color::TRANSPARENT, 20.0) } else { mix(p.control_button_normal_text, Color::TRANSPARENT, 10.0) };
        b = b.hover(hover).cursor(rune::Cursor::Pointer).on_press(on);
    }
    b
}

/// `VoiceConnectionStatusInner` (or its failed variant), with the divider under it.
pub fn panel<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let v = &st.voice;
    let status = |label: &str, c: Color| text(label.to_string()).font(font(14.0, 600.0).lh(18.0)).color(c).truncate().grow(1.0).shrink();
    let divider = el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(p.floating_surface_ring_color_strong);
    let shell = col().w_full().padding(8.0).gap(12.0).pb(9.0);
    let Some((guild, channel)) = v.channel else {
        let target = v.failed?;
        let retry = move |s: &mut S| acc(s).join_voice(target.0, target.1);
        let status_row = row()
            .w_full()
            .h(32.0)
            .gap(8.0)
            .center_y()
            .child(status("Connection failed", p.status_danger).cursor(rune::Cursor::Pointer).on_press(retry))
            .child(control_button(&icons::ARROW_CLOCKWISE_BOLD, "Try again", p, retry))
            .child(control_button(&icons::X_BOLD, "Dismiss", p, move |s: &mut S| acc(s).voice.failed = None));
        return Some(shell.child(status_row).child(divider));
    };
    let (label, color) = if v.connected { ("In voice chat", p.status_online) } else { ("Connecting...", p.status_idle) };
    let mut status_row = row().w_full().h(32.0).gap(8.0).center_y();
    if v.connected {
        status_row = status_row.child(signal_icon(v, p));
    }
    // shortcut: the noise-suppression button and the details popout aren't implemented.
    status_row = status_row
        .child(status(label, color))
        .child(control_button(&icons::PHONE_X_FILL, "Disconnect", p, move |s: &mut S| acc(s).leave_voice()));
    let channel_name = store.channels.get(&channel).and_then(|c| c.name.clone()).unwrap_or_default();
    let source = match guild.and_then(|g| store.guilds.get(&g)).and_then(|g| g.info.as_ref()) {
        Some(g) => format!("{channel_name} / {}", g.name),
        None => channel_name,
    };
    let source_row = row().w_full().h(16.0).center_y().child(
        text(source)
            .font(font(12.0, 400.0).lh(16.0))
            .color(p.text_secondary)
            .truncate()
            .shrink()
            .cursor(rune::Cursor::Pointer)
            .role("link")
            .on_press(move |s: &mut S| {
                let sh = acc(s);
                if let Some(g) = guild {
                    sh.guild = Some(g);
                }
                sh.select_channel(channel);
            }),
    );
    let can_stream = can(store, v, Permissions::STREAM);
    let screen_label = match (v.connected, can_stream, v.screen) {
        (false, _, _) => "Waiting for connection...",
        (_, false, _) => "You don't have permission to stream in this channel",
        (_, _, true) => "Stop sharing your screen",
        _ => "Share your screen",
    };
    // shortcut: camera publishing isn't implemented; the button is shown disabled.
    let media = row()
        .w_full()
        .gap(4.0)
        .child(media_button(&icons::CAMERA_SLASH_FILL, "Camera isn't supported yet", false, true, p, |_: &mut S| {}))
        .child(media_button(&icons::MONITOR_PLAY_FILL, screen_label, v.screen, !v.connected || !can_stream, p, move |s: &mut S| {
            acc(s).intents.push(Intent::ToggleScreen)
        }));
    Some(shell.child(status_row).child(source_row).child(media).child(divider))
}

/// The screen share being watched, in place of the chat column.
pub fn viewer<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let user = st.voice.watching?;
    let guild = st.voice.channel.and_then(|(g, _)| g);
    let name = store.display_name(guild, user);
    let header = row()
        .w_full()
        .h(dim::LAYOUT_HEADER_HEIGHT)
        .px(dim::SPACING_4)
        .gap(dim::SPACING_2)
        .center_y()
        .child(icon(&icons::MONITOR_PLAY_FILL, 20.0).color(p.text_tertiary))
        .child(text(format!("{name}'s screen")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).truncate().grow(1.0).shrink())
        .child(control_button(&icons::X_BOLD, "Stop watching", p, move |s: &mut S| {
            acc(s).voice.watching = None;
            acc(s).intents.push(Intent::Watch(None));
        }));
    let video = el().w_full().grow(1.0).min_h(0.0).pixels_fit(&st.voice.stream, rune::SurfaceFit::Fit);
    Some(col().grow(1.0).min_w(0.0).h_full().fill(Color::BLACK).child(header.fill(p.background_primary)).child(video))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_follows_excess_over_median() {
        assert_eq!(signal(None, &[40]), None);
        assert_eq!(signal(Some(40), &[]), Some((4, Tone::Green)));
        let h = [40, 42, 38, 41];
        assert_eq!(signal(Some(100), &h), Some((4, Tone::Green)));
        assert_eq!(signal(Some(150), &h), Some((3, Tone::Yellow)));
        assert_eq!(signal(Some(300), &h), Some((2, Tone::Orange)));
        assert_eq!(signal(Some(400), &h), Some((1, Tone::Red)));
    }

    #[test]
    fn mute_and_deafen_follow_fluxer() {
        let mut st = ShellState::default();
        st.toggle_deaf();
        assert!(st.voice.self_deaf && st.voice.self_mute);
        st.toggle_deaf();
        assert!(!st.voice.self_deaf && !st.voice.self_mute, "undeafen restores the mic deafening muted");
        st.toggle_mute();
        st.toggle_deaf();
        st.toggle_deaf();
        assert!(st.voice.self_mute, "stays muted when it was muted before deafening");
        st.toggle_deaf();
        st.toggle_mute();
        assert!(!st.voice.self_deaf && !st.voice.self_mute, "unmuting while deafened undeafens");
        assert_eq!(st.intents.iter().filter(|i| **i == Intent::VoiceSelf).count(), 7);
    }

    #[test]
    fn joining_twice_is_one_join() {
        let mut st = ShellState::default();
        st.join_voice(Some(Snowflake(1)), Snowflake(2));
        st.join_voice(Some(Snowflake(1)), Snowflake(2));
        assert_eq!(st.intents, [Intent::JoinVoice { guild: Some(Snowflake(1)), channel: Snowflake(2) }]);
        st.leave_voice();
        assert_eq!(st.intents.last(), Some(&Intent::LeaveVoice));
    }
}
