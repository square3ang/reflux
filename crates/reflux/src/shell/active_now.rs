//! `ActiveNowSidebar` beside the friends list: voice channels a friend who shares voice activity
//! (or you) is in, as `VoiceActivityCard`s, busiest first.
// shortcut: no stream preview thumbnail or "Watch stream" on streaming cards (the card says
// STREAMING and joins the channel); the sidebar doesn't hide below a 1100px window.

use std::collections::{HashMap, HashSet};

use reflux_model::{ChannelType, Permissions, RelationshipType, Snowflake};
use reflux_store::Store;
use rune::{El, col, icon, row, text};

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
            let owner = users.iter().position(|(u, _)| *u == me).or_else(|| users.iter().position(|(u, _)| friends.contains(u)))?;
            let streaming = users[owner].1;
            users.swap(0, owner);
            Some(Activity { guild, channel, users: users.into_iter().map(|(u, _)| u).collect(), streaming })
        })
        .collect();
    out.sort_by(|a, b| b.users.len().cmp(&a.users.len()).then(a.channel.cmp(&b.channel)));
    out
}

pub fn view<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if !crate::look::with(|l| l.active_now) {
        return None;
    }
    let list = activities(store);
    let header = row().w_full().px(16.0).pt(16.0).pb(12.0).child(text(t("Active now")).font(font(20.0, 700.0).lh(26.0)).color(p.text_primary).role("heading"));
    let body = if list.is_empty() {
        col()
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
        crate::ui::scroller(col().w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(list), false, p)
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
        stack = stack.child(if i == 0 { face } else { face.ml(-6.0) });
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
        .w_full()
        .gap(12.0)
        .py(12.0)
        .px(14.0)
        .radius(dim::RADIUS_MD)
        .fill(p.background_secondary)
        .border(1.0, crate::ui::rgba(255, 255, 255, 0.04))
        .child(col().w_full().gap(4.0).child(label).child(context))
        .child(stack)
        .child(button)
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
    }
}
