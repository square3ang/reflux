//! `ActiveNowSidebar` beside the friends list: voice channels a friend who shares voice activity
//! (or you) is in, as `VoiceActivityCard`s, busiest first.
//! Streaming cards show the stream's preview (`useStreamPreview`) and "Watch stream"; the
//! sidebar hides below a 1100px window.

use crate::ui::SmoothWheel;
use std::collections::{HashMap, HashSet};

use reflux_model::{ChannelType, Permissions, RelationshipType, Snowflake};
use reflux_store::Store;
use rune::{El, col, el, row, text};
use crate::ui::icon;

use super::{ShellAccess, ShellState};
use crate::i18n::t;
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{Button, ButtonSize};

/// One active voice channel.
#[derive(Debug, PartialEq)]
pub struct Activity {
    pub guild: Option<Snowflake>,
    pub channel: Snowflake,
    /// Everyone in the channel, the friend (or you) first.
    pub users: Vec<Snowflake>,
    pub streaming: bool,
    /// The owner's `{guild|dm}:{channel}:{connection}` (`getStreamKey`) when streaming.
    pub stream_key: Option<String>,
}

/// `STREAM_PREVIEW_REFRESH_INTERVAL_MS`, `STREAM_PREVIEW_INITIAL_UPLOAD_INTERVAL_MS` and
/// `STREAM_PREVIEW_INITIAL_UPLOAD_MAX_ATTEMPTS`.
const REFRESH_MS: u64 = 60_000;
const RETRY_MS: u64 = 1_000;
const RETRIES: u8 = 10;

/// One stream's preview JPEG (`GET /streams/{key}/preview`).
#[derive(Clone, Debug, Default)]
pub struct Preview {
    pub bytes: Option<std::sync::Arc<Vec<u8>>>,
    pub fetched_ms: u64,
    /// 404s in a row (the streamer hasn't uploaded one yet).
    pub missing: u8,
    pub loading: bool,
}

impl Preview {
    /// Fetch now: a minute after the last one, or every second while still missing.
    pub fn due(&self, now: u64) -> bool {
        let wait = if self.bytes.is_none() && self.missing < RETRIES { RETRY_MS } else { REFRESH_MS };
        !self.loading && (self.fetched_ms == 0 || now >= self.fetched_ms + wait)
    }
}

/// `ActiveNowSidebar` is hidden below this window width.
pub const MIN_WINDOW_WIDTH: f32 = 1100.0;

/// Stream keys whose previews the visible sidebar shows (`canViewStreamPreview`: CONNECT in guilds).
/// `useStreamPreview`'s key for a live share: `{guild|dm}:{channel}:{connection}`.
pub fn stream_key(store: &Store, guild: Option<Snowflake>, channel: Snowflake, user: Snowflake) -> Option<String> {
    let v = store.voice_states.get(&(guild, user))?;
    if !v.self_stream || v.channel_id != Some(channel) {
        return None;
    }
    Some(format!("{}:{channel}:{}", guild.map_or("dm".to_string(), |g| g.to_string()), v.connection_id.clone()?))
}

/// The fetched still for `key`, decoded (cached by its fetch time).
pub fn preview_pixels(st: &ShellState, key: &str) -> Option<rune::Pixels> {
    let entry = st.voice.previews.get(key)?;
    let bytes = entry.bytes.clone()?;
    match st.media.local(&format!("stream-preview:{key}:{}", entry.fetched_ms), move || Ok(bytes.to_vec())) {
        crate::media::Load::Ready(px) => Some(px),
        _ => None,
    }
}

pub fn wanted_previews(store: &Store, st: &ShellState) -> Vec<String> {
    // `useStreamPreview` in the call tiles and on a sidebar row's hover card: every live share
    // we may draw a still for, as a thumbnail until we watch the stream.
    let me = store.my_id();
    let mut out = Vec::new();
    let mut want = |guild: Option<Snowflake>, channel: Snowflake| {
        out.extend(
            store
                .voice_states
                .iter()
                .filter(|((g, u), v)| *g == guild && *u != me && v.channel_id == Some(channel) && v.self_stream)
                .filter_map(|(_, v)| v.connection_id.as_ref().map(|c| format!("{}:{channel}:{c}", guild.map_or("dm".to_string(), |g| g.to_string())))),
        );
    };
    if let Some((guild, channel)) = st.voice.channel {
        want(guild, channel);
    }
    if let Some(guild) = st.guild {
        for c in store.channels.values().filter(|c| c.guild_id == Some(guild) && c.kind == ChannelType::GUILD_VOICE) {
            if store.permissions(c.id).contains(Permissions::CONNECT) {
                want(Some(guild), c.id);
            }
        }
    }
    if st.guild.is_some() || st.channel.is_some() || !shown(st) {
        return out;
    }
    out.extend(
        activities(store)
            .into_iter()
            .filter(|a| a.guild.is_none() || store.permissions(a.channel).contains(Permissions::CONNECT))
            .filter_map(|a| a.stream_key),
    );
    out
}

fn shown(st: &ShellState) -> bool {
    crate::look::with(|l| l.active_now) && !(st.desktop.width > 0.0 && st.desktop.width < MIN_WINDOW_WIDTH)
}

/// (guild, channel) → (user, streaming).
type ByChannel = HashMap<(Option<Snowflake>, Snowflake), Vec<(Snowflake, bool)>>;

/// `useActiveFriendVoiceStates`.
pub fn activities(store: &Store) -> Vec<Activity> {
    let me = store.my_id();
    let friends: HashSet<Snowflake> = store.relationships.values().filter(|r| r.kind == RelationshipType::FRIEND && r.friend_shares_voice_activity).map(|r| r.id).collect();
    let mut by_channel: ByChannel = HashMap::new();
    for ((g, u), v) in &store.voice_states {
        if let Some(c) = v.channel_id {
            by_channel.entry((*g, c)).or_default().push((*u, v.self_stream));
        }
    }
    let mut out: Vec<Activity> = by_channel
        .into_iter()
        .filter_map(|((guild, channel), mut users)| {
            users.sort();
            // You, else a streaming friend, else any friend.
            let owner = users
                .iter()
                .position(|(u, _)| *u == me)
                .or_else(|| users.iter().position(|(u, stream)| *stream && friends.contains(u)))
                .or_else(|| users.iter().position(|(u, _)| friends.contains(u)))?;
            let streaming = users[owner].1;
            users.swap(0, owner);
            let stream_key = streaming
                .then(|| store.voice_states.get(&(guild, users[0].0))?.connection_id.clone())
                .flatten()
                .map(|conn| format!("{}:{channel}:{conn}", guild.map_or("dm".to_string(), |g| g.to_string())));
            Some(Activity { guild, channel, users: users.into_iter().map(|(u, _)| u).collect(), streaming, stream_key })
        })
        .collect();
    out.sort_by(|a, b| b.users.len().cmp(&a.users.len()).then(a.channel.cmp(&b.channel)));
    out
}

pub fn view<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if !shown(st) {
        return None;
    }
    let list = activities(store);
    let header = row().w_full().px(16.0).pt(16.0).pb(12.0).child(text(t("Active now")).font(font(20.0, 700.0).lh(26.0)).color(p.text_primary).role("heading"));
    let body = if list.is_empty() {
        col()
            .key("empty")
            .w_full()
            .grow(1.0)
            .padding(24.0)
            .gap(12.0)
            .center()
            .child(icon(&QUIET, 48.0).color(p.text_tertiary).opacity(0.6))
            .child(text(t("It's quiet for now...")).font(font(14.0, 600.0).lh(20.0)).color(p.text_secondary))
            .child(text(t("When friends are active in voice channels, their activity will appear here.")).font(font(12.0, 400.0).lh(16.8)).color(p.text_tertiary).wrap().align(rune::Align::Center))
    } else {
        let list = col().w_full().px(12.0).pb(12.0).gap(8.0).children(list.iter().map(|a| card(store, st, a, p, acc)));
        crate::ui::scroller(col().w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).smooth_wheel::<S>("active-now").key("active-now").child(list), false, p).key("list")
    };
    Some(
        col()
            .w(352.0)
            .min_w(352.0)
            .h_full()
            .fill(p.background_secondary_lighter)
            .role("complementary")
            .label(t("Active now").to_string())
            .child(header)
            .child(body)
            .child(rune::el().pin(Some(0.0), Some(0.0), None, Some(0.0)).w(1.0).fill(p.background_modifier_accent)),
    )
}

/// `VoiceActivityCard`.
fn card<S: 'static>(store: &Store, st: &ShellState, a: &Activity, p: &Palette, acc: ShellAccess<S>) -> El {
    let (g, c) = (a.guild, a.channel);
    let joined = st.voice.channel == Some((g, c));
    let ch = store.channels.get(&c);
    let can_join = joined || ch.is_some_and(|ch| ch.guild_id.is_none() || store.permissions(c).contains(Permissions::CONNECT));
    let label = if a.streaming {
        text(t("Streaming").to_uppercase()).font(font(12.0, 600.0).lh(16.0)).color(p.status_danger)
    } else {
        text(t("In voice").to_uppercase()).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary)
    };
    let name = |s: String| text(s).font(font(13.0, 400.0).lh(18.0)).color(p.text_primary_muted).truncate().min_w(0.0);
    let open = move |s: &mut S| acc(s).select_guild(g, Some(c));
    let context = match (g, ch) {
        (Some(gid), Some(ch)) => row()
            .gap(6.0)
            .center_y()
            .min_w(0.0)
            .cursor(rune::Cursor::Pointer)
            .key(format!("active-now-open-{c}"))
            .on_press(open)
            .child(super::guilds::guild_face(store, st, gid, 16.0, false, p))
            .child(icon(&icons::CARET_RIGHT_BOLD, 12.0).color(p.text_tertiary))
            .child(icon(&icons::SPEAKER_HIGH_FILL, 16.0).color(p.text_tertiary))
            .child(name(ch.name.clone().unwrap_or_default())),
        (None, Some(ch)) => {
            let face = if ch.kind == ChannelType::GROUP_DM { super::group_avatar(store, st, ch, 16.0, p.background_secondary, p) } else { super::avatar(store, st, ch.recipients.iter().map(|u| u.id).find(|&u| u != a.users[0]).unwrap_or(a.users[0]), 16.0, None, p.background_secondary, p) };
            row().gap(6.0).center_y().min_w(0.0).cursor(rune::Cursor::Pointer).key(format!("active-now-open-{c}")).on_press(open).child(face).child(name(super::channel_title(store, ch)))
        }
        _ => row(),
    };
    // `AvatarStack`: 20px faces, at most 5, then "+N".
    let mut stack = row().center_y();
    for (i, u) in a.users.iter().take(5).enumerate() {
        let face = row().padding(2.0).radius(12.0).fill(p.background_secondary).child(super::avatar(store, st, *u, 20.0, None, p.background_secondary, p));
        stack = stack.child(if i == 0 { face } else { face.ml(-6.0) }.key(format!("u-{u}")));
    }
    if a.users.len() > 5 {
        stack = stack.child(text(format!("+{}", a.users.len() - 5)).font(font(12.0, 600.0).lh(16.0)).color(p.text_primary_muted).ml(4.0));
    }
    let button = Button::new(if joined { t("Open voice") } else { t("Join voice") })
        .size(ButtonSize::Compact)
        .icon(&icons::PHONE_FILL)
        .fit_container()
        .disabled(!can_join)
        .key(format!("active-now-join-{c}"))
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            if sh.voice.channel == Some((g, c)) {
                sh.select_guild(g, Some(c));
            } else {
                sh.join_voice(g, c);
            }
        })
        .el(p);
    col()
        .key(format!("an-{c}"))
        .w_full()
        .gap(12.0)
        .py(12.0)
        .px(14.0)
        .radius(dim::RADIUS_MD)
        .fill(p.background_secondary)
        .border(1.0, crate::ui::rgba(255, 255, 255, 0.04))
        .child(col().w_full().gap(4.0).child(label).child(context))
        .child(stack)
        .children(a.stream_key.as_ref().map(|k| preview(store, st, a, k, p, acc)))
        .child(button)
}

/// `StreamPreviewSection`: the 16:9 preview, "Watch stream" on hover.
fn preview<S: 'static>(store: &Store, st: &ShellState, a: &Activity, key: &str, p: &Palette, acc: ShellAccess<S>) -> El {
    let (g, c, user) = (a.guild, a.channel, a.users[0]);
    let (w, h) = (300.0, 300.0 * 9.0 / 16.0);
    let joined = st.voice.channel == Some((g, c));
    let watching = joined && st.voice.watching == Some(user);
    let pending = st.voice.watch_on_join == Some(user) && !watching;
    let can_watch = user != store.my_id() && (joined || a.guild.is_none() || store.permissions(c).contains(Permissions::CONNECT));
    let label = if watching { t("Watching") } else { t("Watch stream") };
    let entry = st.voice.previews.get(key);
    let shown = entry.and_then(|e| Some((e.bytes.clone()?, e.fetched_ms))).map(|(bytes, at)| st.media.local(&format!("stream-preview:{key}:{at}"), move || Ok(bytes.to_vec())));
    let mut body = el().size(w, h).radius(dim::RADIUS_SM).clip_rounded(dim::RADIUS_SM).fill(p.background_tertiary).role("button").label(label.to_string()).key(format!("active-now-preview-{c}"));
    body = match shown {
        Some(crate::media::Load::Ready(px)) => body.child(el().size(w, h).pixels_fit(&px, rune::SurfaceFit::Fill).opacity(0.6)),
        _ if entry.is_none_or(|e| e.loading && e.fetched_ms == 0) => body.child(row().size(w, h).center().child(crate::ui::misc::spinner(crate::ui::SpinnerSize::Small, p))),
        _ => body.child(row().size(w, h).center().child(text(t("No preview yet")).font(font(12.8, 400.0).lh(16.0)).color(p.text_primary_muted))),
    };
    if pending {
        return body.opacity(0.6);
    }
    if !can_watch || watching {
        return body;
    }
    let overlay = row()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .center()
        .fill(crate::ui::rgba(0, 0, 0, 0.55))
        .group_opacity(0.0, 1.0)
        .child(text(label).font(font(12.8, 600.0).lh(16.0)).color(rune::Color::WHITE));
    body.hover_group().cursor(rune::Cursor::Pointer).child(overlay).on_press(move |s: &mut S| {
        let sh = acc(s);
        if sh.voice.channel == Some((g, c)) {
            sh.voice.watching = Some(user);
            sh.intents.push(super::Intent::Watch(Some(user)));
        } else {
            sh.join_voice(g, c);
            sh.voice.watch_on_join = Some(user);
        }
        sh.select_guild(g, Some(c));
    })
}

/// The empty state's zig-zag glyph (24×24, stroke 2).
static QUIET: rune::Icon = rune::Icon {
    name: "active-now-quiet",
    view: 24.0,
    body: r#"<path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M4 12h6l-6 8h6M14 4h6l-6 8h6"/>"#,
};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn friends_channels_busiest_first() {
        let mut store = Store::default();
        let ready = json!({"v": 1, "user": {"id": "1", "username": "me", "discriminator": "0"}, "session_id": "s", "guilds": [], "private_channels": [],
            "relationships": [{"id": "2", "type": 1, "friend_shares_voice_activity": true}, {"id": "3", "type": 1, "friend_shares_voice_activity": false}]});
        store.apply(&reflux_model::DispatchEvent::parse("READY", &ready).unwrap(), 0);
        for (u, c, stream) in [(2, 10, true), (4, 10, false), (5, 10, false), (3, 11, false), (6, 12, false), (7, 12, false), (8, 12, false)] {
            let v = json!({"guild_id": "100", "channel_id": c.to_string(), "user_id": u.to_string(), "self_stream": stream});
            store.apply(&reflux_model::DispatchEvent::parse("VOICE_STATE_UPDATE", &v).unwrap(), 0);
        }
        let a = activities(&store);
        assert_eq!(a.len(), 1, "only the friend sharing voice activity counts: {a:?}");
        assert_eq!(a[0].channel, Snowflake(10));
        assert_eq!(a[0].users[0], Snowflake(2));
        assert!(a[0].streaming);
        assert_eq!(a[0].stream_key, None, "no connection id, no preview");
    }

    #[test]
    fn preview_schedule() {
        let mut p = Preview::default();
        assert!(p.due(5));
        p.loading = true;
        assert!(!p.due(5));
        (p.loading, p.fetched_ms, p.missing) = (false, 1_000, 1);
        assert!(!p.due(1_500) && p.due(2_000), "missing: retry each second");
        p.missing = RETRIES;
        assert!(!p.due(2_000) && p.due(61_000), "gave up: once a minute");
        (p.missing, p.bytes) = (0, Some(Default::default()));
        assert!(!p.due(30_000) && p.due(61_000));
    }
}
