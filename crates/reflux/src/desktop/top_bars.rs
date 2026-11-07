//! The bars above the layout, picked like Fluxer's `selectVisibleNagbars` (`AppLayoutHooks.ts`):
//! at most one non-dismissible and one dismissible bar, by priority. Also the account bars
//! (`UnclaimedAccountNagbar`, `AccountLimitedNagbar`, `DesktopNotificationNagbar`,
//! `VisionaryMfaNagbar`, `GuildMembershipCtaNagbar`, `VoiceSessionRestoreNagbar`) and the
//! community bars under them (`GuildLayout.tsx`'s `StaffOnlyGuildNagbar`,
//! `GuildMfaRequirementNagbar` and `InvitesDisabledNagbar`).

use reflux_model::{ChannelType, Permissions, PremiumType, Snowflake};
use reflux_store::Store;
use rune::{El, col, text};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::nagbar::{bar, button};
use crate::i18n::{t, tf};
use crate::shell::account_gate;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Modal, rgb};

const PRODUCT: &str = "Fluxer";
const DAY: u64 = 86_400_000;
/// `VOICE_SESSION_RESTORE_DECAY_MS` and `VOICE_SESSION_RESTORE_HEARTBEAT_MS`.
const RESTORE_DECAY: u64 = 5 * 60_000;
const RESTORE_HEARTBEAT: u64 = 60_000;

/// One bar and its `NagbarState` priority (lower shows first).
pub struct Bar {
    pub prio: f32,
    pub dismissible: bool,
    /// Kept out of the selection (Fluxer's build-environment bar; Reflux's desktop update bar).
    pub pinned: bool,
    pub el: El,
}

impl Bar {
    pub fn new(prio: f32, dismissible: bool, el: El) -> Self {
        Self { prio, dismissible, pinned: false, el }
    }
}

/// `selectVisibleNagbars`: the pinned bars, the first non-dismissible and the first
/// dismissible one, in priority order.
pub fn select(mut bars: Vec<Bar>) -> Vec<El> {
    bars.sort_by(|a, b| a.prio.total_cmp(&b.prio));
    let keep = pick(&bars.iter().map(|b| (b.pinned, b.dismissible)).collect::<Vec<_>>());
    bars.into_iter().enumerate().filter(|(i, _)| keep.contains(i)).map(|(_, b)| b.el).collect()
}

/// The indices [`select`] keeps of bars sorted by priority, as (pinned, dismissible).
fn pick(bars: &[(bool, bool)]) -> Vec<usize> {
    let first = |d: bool| bars.iter().position(|&(pinned, dismissible)| !pinned && dismissible == d);
    (0..bars.len()).filter(|&i| bars[i].0 || Some(i) == first(false) || Some(i) == first(true)).collect()
}

/// What Developer tools' override says, else the real condition.
fn shown(st: &ShellState, key: &str, actual: impl FnOnce() -> bool) -> bool {
    crate::shell::devtools::nagbar(st, key).unwrap_or_else(actual)
}

fn dismissed(st: &ShellState, key: &str) -> bool {
    st.prefs.dismissed_nagbars.iter().any(|k| k == key)
}

fn dismiss_key(sh: &mut ShellState, key: &str) {
    let mut p = sh.prefs.clone();
    if !p.dismissed_nagbars.iter().any(|k| k == key) {
        p.dismissed_nagbars.push(key.into());
    }
    sh.intents.push(Intent::Prefs(Box::new(p)));
}

/// The top bars (and how many show, for the community bars' slots).
pub fn view<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Vec<El> {
    let mut bars = crate::desktop::nagbar::bars(st, p, acc);
    bars.extend(crate::nav::terms::nagbar(store, st, p, acc));
    bars.extend(crate::desktop::notices::instance(st, p, acc));
    bars.extend(crate::shell::verify_nagbar(store, st, p, acc));
    bars.extend(crate::desktop::notices::premium(store, st, p, acc));
    bars.extend(account(store, st, p, acc));
    select(bars)
}

// ---- Account bars -------------------------------------------------------------------------------

/// `isUnclaimedNagbarSnoozed`.
pub fn unclaimed_snoozed(st: &ShellState, user: Snowflake) -> bool {
    st.prefs.unclaimed_snoozed_until.get(&user).is_some_and(|&u| u > st.now_ms)
}

fn account<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Vec<Bar> {
    let mut out = Vec::new();
    let me = crate::shell::devtools::me(store, st);
    let me = me.as_deref();
    let neutral = rgb(0x4e, 0x50, 0x58);
    let cfg = &st.tiers;
    if shown(st, "forceUnclaimedAccount", || me.is_some_and(|m| account_gate::tier(m, cfg) == account_gate::Tier::Unclaimed && !st.account_gate.unclaimed_dismissed && !unclaimed_snoozed(st, m.base.id))) {
        let (message, label) = account_gate::unclaimed_nagbar(me.and_then(|m| account_gate::need(m, cfg)));
        let uid = me.map(|m| m.base.id);
        let close = move |s: &mut S| {
            let sh = acc(s);
            sh.account_gate.unclaimed_dismissed = true;
            if let Some(id) = uid {
                let mut p = sh.prefs.clone();
                p.unclaimed_snoozed_until.insert(id, sh.now_ms + 3 * DAY);
                sh.intents.push(Intent::Prefs(Box::new(p)));
            }
        };
        let claim = button("nagbar-claim", t(label), p, move |s: &mut S| acc(s).account_gate.open = Some(None));
        out.push(Bar::new(-4.0, true, bar("nagbar-unclaimed", neutral, t(message).into(), vec![claim], Some(Box::new(close)))));
    }
    if shown(st, "forceAccountLimited", || me.is_some_and(|m| m.account_limited)) {
        let help = (!cfg.self_hosted).then(|| button("nagbar-limited-help", t("My text didn't go through"), p, move |s: &mut S| acc(s).account_gate.help = Some(Default::default())));
        out.push(Bar::new(-3.75, false, bar::<S>("nagbar-limited", neutral, t("Messaging is paused on your account. Check your email for a quick step to continue.").into(), help.into_iter().collect(), None)));
    }
    out.extend(voice_restore(store, st, p, acc));
    // `shouldShowDesktopNotification` is false in the desktop app (`isDesktop()`): only the
    // developer override shows it.
    if shown(st, "forceDesktopNotification", || false) {
        let enable = button("nagbar-notifications", t("Enable notifications"), p, move |s: &mut S| enable_notifications(acc(s)));
        let close = move |s: &mut S| acc(s).account_gate.bar_confirm = Some(Confirm::DesktopNotifications);
        out.push(Bar::new(8.0, true, bar("nagbar-desktop-notification", p.brand_primary, t("Enable desktop notifications to stay updated on new messages.").into(), vec![enable], Some(Box::new(close)))));
    }
    if !cfg.self_hosted && shown(st, "forceVisionaryMfa", || me.is_some_and(|m| m.premium_type == Some(PremiumType::LIFETIME) && !m.mfa_enabled) && !dismissed(st, "visionary-mfa")) {
        let open = button("nagbar-visionary-mfa", t("Enable two-factor auth"), p, move |s: &mut S| acc(s).open_settings(Some("account_security")));
        let close = move |s: &mut S| dismiss_key(acc(s), "visionary-mfa");
        let msg = tf("Protect your Visionary account by enabling two-factor authentication. Thanks for supporting {productName}!", &[("productName", PRODUCT)]);
        out.push(Bar::new(7.0, true, bar("nagbar-visionary-mfa", p.brand_primary, msg, vec![open], Some(Box::new(close)))));
    }
    // `GuildMembershipCtaNagbar`: hidden once in Fluxer HQ (found by its vanity code).
    let in_hq = store.guilds.values().any(|g| !g.visiting && g.info.as_ref().is_some_and(|i| i.vanity_url_code.as_deref() == Some(HQ_INVITE)));
    if !cfg.self_hosted && shown(st, "forceGuildMembershipCta", || me.is_some_and(|m| account_gate::tier(m, cfg) == account_gate::Tier::Verified) && !in_hq && !dismissed(st, "guild-membership-cta")) {
        let community = format!("{PRODUCT} HQ");
        let join = button("nagbar-hq-join", &tf("Join {communityName}", &[("communityName", &community)]), p, move |s: &mut S| acc(s).open_social(crate::social::Modal::InviteAccept(HQ_INVITE.into())));
        let close = move |s: &mut S| dismiss_key(acc(s), "guild-membership-cta");
        let msg = tf("Join {communityName} to chat with the team and stay up to date.", &[("communityName", &community)]);
        out.push(Bar::new(6.0, true, bar("nagbar-hq", p.brand_primary, msg, vec![join], Some(Box::new(close)))));
    }
    out
}

/// `FLUXER_HQ_INVITE_CODE`.
const HQ_INVITE: &str = "fluxer-hq";

/// `applyGrantedPermission` (the desktop app always has permission), then the dismissal.
fn enable_notifications(sh: &mut ShellState) {
    sh.account_gate.bar_confirm = None;
    let mut p = sh.prefs.clone();
    p.notifications = true;
    if !p.dismissed_nagbars.iter().any(|k| k == "desktop-notification") {
        p.dismissed_nagbars.push("desktop-notification".into());
    }
    sh.intents.push(Intent::Prefs(Box::new(p)));
    crate::desktop::notify::show(t("Access granted").into(), t("Huzzah! Desktop notifications are enabled").into(), None, || {});
}

// ---- Voice session restore ----------------------------------------------------------------------

/// `VoiceSessionRestoreSnapshot`, kept in the prefs while connected.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VoiceSnapshot {
    pub user: Snowflake,
    pub guild: Option<Snowflake>,
    pub channel: Snowflake,
    pub video: bool,
    pub stream: bool,
    pub updated_at: u64,
}

/// Saves the snapshot while connected (on change and every minute), drops it when the user
/// leaves, and remembers the one found at startup (`startupVoiceSessionRestoreSnapshotKey`).
pub fn poll(store: &Store, st: &mut ShellState) -> bool {
    let Some(me) = store.me.as_ref().map(|m| m.base.id) else { return false };
    let saved = st.prefs.voice_restore.clone().filter(|s| s.user == me && st.now_ms.saturating_sub(s.updated_at) <= RESTORE_DECAY);
    let g = &mut st.account_gate;
    if g.voice_startup.is_none() {
        g.voice_startup = Some(saved.clone());
    }
    let v = &st.voice;
    let mut next = st.prefs.voice_restore.clone();
    match (v.channel, v.connected) {
        (Some((guild, channel)), true) => {
            let snap = VoiceSnapshot { user: me, guild, channel, video: v.camera, stream: v.screen, updated_at: st.now_ms };
            let stale = saved.as_ref().is_none_or(|s| (s.guild, s.channel, s.video, s.stream) != (guild, channel, snap.video, snap.stream) || st.now_ms.saturating_sub(s.updated_at) >= RESTORE_HEARTBEAT);
            if stale {
                next = Some(snap);
            }
            st.account_gate.was_connected = true;
        }
        (None, _) if std::mem::take(&mut st.account_gate.was_connected) => next = None,
        _ => {}
    }
    if next == st.prefs.voice_restore {
        return false;
    }
    st.prefs.voice_restore = next;
    st.intents.push(Intent::Prefs(Box::new(st.prefs.clone())));
    true
}

/// `getRestoreNagbarMessage`.
fn restore_message(store: &Store, snap: &VoiceSnapshot) -> Option<String> {
    let c = store.channels.get(&snap.channel)?;
    match c.kind {
        ChannelType::DM | ChannelType::GROUP_DM => {
            store.calls.get(&c.id)?;
            let name = crate::shell::chat::channel_title(store, c);
            Some(tf("{productName} restarted while you were connected to the call in {directCallName}.", &[("productName", PRODUCT), ("directCallName", &name)]))
        }
        ChannelType::GUILD_VOICE => {
            let channel = c.name.as_deref().unwrap_or_default().trim().to_string();
            let community = store.guilds.get(&c.guild_id?)?.info.as_ref()?.name.trim().to_string();
            (!channel.is_empty() && !community.is_empty()).then(|| {
                tf("{productName} restarted while you were connected to the {channelName} channel in the {communityName} community.", &[("productName", PRODUCT), ("channelName", &channel), ("communityName", &community)])
            })
        }
        _ => None,
    }
}

fn voice_restore<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<Bar> {
    let force = crate::shell::devtools::nagbar(st, "forceVoiceSessionRestore");
    let snap = match force {
        Some(false) => return None,
        // The forced bar names the first voice channel at hand.
        Some(true) => st.prefs.voice_restore.clone().or_else(|| {
            let c = store.channels.values().filter(|c| c.kind == ChannelType::GUILD_VOICE).min_by_key(|c| c.id)?;
            Some(VoiceSnapshot { user: store.my_id(), guild: c.guild_id, channel: c.id, ..Default::default() })
        })?,
        None => {
            let snap = st.prefs.voice_restore.clone()?;
            let fresh = snap.user == store.my_id() && st.now_ms.saturating_sub(snap.updated_at) <= RESTORE_DECAY;
            let startup = st.account_gate.voice_startup.as_ref().and_then(|s| s.as_ref()).is_some_and(|s| (s.user, s.channel, s.updated_at) == (snap.user, snap.channel, snap.updated_at));
            if !fresh || !startup || st.voice.channel.is_some() {
                return None;
            }
            snap
        }
    };
    let message = restore_message(store, &snap)?;
    let clear = move |s: &mut S| {
        let sh = acc(s);
        sh.account_gate.voice_startup = Some(None);
        let mut p = sh.prefs.clone();
        p.voice_restore = None;
        sh.intents.push(Intent::Prefs(Box::new(p)));
    };
    let (guild, channel, media) = (snap.guild, snap.channel, (snap.video, snap.stream));
    let reconnect = move |s: &mut S| {
        clear(s);
        let sh = acc(s);
        sh.voice.restore_media = media;
        sh.intents.push(Intent::JoinVoice { guild, channel });
    };
    let actions = vec![button("nagbar-voice-not-now", t("Not now"), p, clear), button("nagbar-voice-reconnect", t("Reconnect"), p, reconnect)];
    Some(Bar::new(-3.5, true, bar::<S>("nagbar-voice-restore", rgb(0x15, 0x80, 0x3d), message, actions, None)))
}

// ---- Community bars -----------------------------------------------------------------------------

/// The open community's own bars, after the preview bar, in the slots the top bars left
/// (`availableGuildNagbarSlots`).
pub fn guild<S: 'static>(store: &Store, st: &ShellState, top: usize, p: &Palette, acc: ShellAccess<S>) -> Vec<El> {
    let (Some(g), Some(_)) = (st.guild, st.channel) else { return Vec::new() };
    let Some(info) = store.guilds.get(&g).and_then(|gs| gs.info.as_ref()) else { return Vec::new() };
    let preview = store.guilds.get(&g).is_some_and(|gs| gs.visiting);
    let mut slots = 2usize.saturating_sub(top + preview as usize);
    let mut out = Vec::new();
    let me = crate::shell::devtools::me(store, st);
    let name = info.name.clone();
    // `StaffOnlyGuildNagbar`.
    if slots > 0 && me.as_ref().is_some_and(|m| m.is_staff) && info.features.iter().any(|f| f == "UNAVAILABLE_FOR_EVERYONE_BUT_STAFF") {
        let url = format!("{}/guilds/{g}?tab=features", st.admin_base);
        let manage = button("nagbar-staff-manage", t("Manage community features"), p, move |s: &mut S| acc(s).intents.push(Intent::OpenLink(url.clone())));
        let msg = tf("{communityName} is currently only accessible to {productName} staff members", &[("communityName", &name), ("productName", PRODUCT)]);
        out.push(bar::<S>("nagbar-staff-only", p.status_danger, msg, vec![manage], None));
        slots -= 1;
    }
    if slots > 0 && shown(st, "forceGuildMfaRequirement", || info.mfa_level == 1 && me.as_ref().is_some_and(|m| !m.mfa_enabled) && mfa_gated(store, g) && !dismissed(st, &format!("guild-mfa:{g}"))) {
        let open = button("nagbar-guild-mfa", t("Enable two-factor auth"), p, move |s: &mut S| acc(s).open_settings(Some("account_security")));
        let close = move |s: &mut S| dismiss_key(acc(s), &format!("guild-mfa:{g}"));
        let msg = tf("Moderation actions in {communityName} require two-factor authentication. Enable 2FA to kick, ban, time out, or delete messages here.", &[("communityName", &name)]);
        out.push(bar("nagbar-guild-mfa", rgb(0xb4, 0x53, 0x09), msg, vec![open], Some(Box::new(close))));
        slots -= 1;
    }
    let perms = store.guild_permissions(g);
    let can_manage = perms.contains(Permissions::MANAGE_GUILD);
    let has = |f: &str| info.features.iter().any(|x| x == f);
    let can_invite = st.channel.is_some_and(|c| store.permissions(c).contains(Permissions::CREATE_INSTANT_INVITE));
    if slots > 0 && shown(st, "forceInvitesDisabled", || has("INVITES_DISABLED") && (can_invite || can_manage) && !dismissed(st, &format!("invites-disabled:{g}"))) {
        let msg = if has("RAID_DETECTED") {
            tf("Invites to {communityName} are currently disabled because {productName} detected a potential raid. New people cannot join right now.", &[("communityName", &name), ("productName", PRODUCT)])
        } else {
            tf("Invites to {communityName} are currently disabled", &[("communityName", &name)])
        };
        let enable = can_manage.then(|| button("nagbar-invites-enable", t("Enable invites again"), p, move |s: &mut S| acc(s).account_gate.bar_confirm = Some(Confirm::EnableInvites(g))));
        let close = move |s: &mut S| dismiss_key(acc(s), &format!("invites-disabled:{g}"));
        out.push(bar("nagbar-invites-disabled", rgb(0xea, 0x58, 0x0c), msg, enable.into_iter().collect(), Some(Box::new(close))));
    }
    out
}

/// `hasMfaGatedGuildPermissions`: the member's roles grant an MFA-gated permission (before
/// the MFA requirement strips it).
fn mfa_gated(store: &Store, g: Snowflake) -> bool {
    let Some(gs) = store.guilds.get(&g) else { return false };
    let Some(info) = gs.info.as_ref() else { return false };
    let me = store.my_id();
    reflux_model::compute_permissions(&reflux_model::PermissionInput {
        guild_id: g,
        owner_id: info.owner_id,
        user_id: me,
        roles: &gs.roles,
        member_roles: gs.members.get(&me).map(|m| m.roles.as_slice()),
        overwrites: &[],
        visiting: gs.visiting,
        strip_elevated: false,
    })
    .intersects(Permissions::ELEVATED)
}

// ---- Confirmations ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// The desktop notification bar's close button.
    DesktopNotifications,
    /// "Enable invites again".
    EnableInvites(Snowflake),
}

/// The bars' `ConfirmModal`s.
pub fn modal<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let c = st.account_gate.bar_confirm?;
    let close = move |s: &mut S| acc(s).account_gate.bar_confirm = None;
    let para = |s: String| text(s).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap();
    let (title, body, primary, secondary): (&str, El, El, El) = match c {
        Confirm::DesktopNotifications => {
            let path = format!("{} > {}", t("User settings"), t("Notifications"));
            let body = col()
                .gap(8.0)
                .child(para(t("Enable notifications to stay updated on mentions when you're away from the app.").into()))
                .child(para(tf("If you dismiss this, you can always enable desktop notifications later under {notificationSettingsPath}.", &[("notificationSettingsPath", &path)])));
            let dismiss = move |s: &mut S| {
                let sh = acc(s);
                sh.account_gate.bar_confirm = None;
                dismiss_key(sh, "desktop-notification");
            };
            (
                "Disable desktop notifications?",
                body,
                Button::new(t("Enable notifications")).key("confirm-primary").on_press(move |s: &mut S| enable_notifications(acc(s))).el(p),
                Button::new(t("Dismiss anyway")).variant(ButtonVariant::Secondary).key("confirm-secondary").on_press(dismiss).el(p),
            )
        }
        Confirm::EnableInvites(g) => {
            let features: Vec<String> = store.guilds.get(&g).and_then(|gs| gs.info.as_ref()).map(|i| i.features.iter().filter(|f| *f != "INVITES_DISABLED").cloned().collect()).unwrap_or_default();
            let go = move |s: &mut S| {
                let sh = acc(s);
                sh.account_gate.bar_confirm = None;
                sh.social(reqwest::Method::PATCH, format!("/guilds/{g}"), Some(json!({ "features": features.clone() })), crate::social::Done::Nothing);
            };
            (
                "Enable invites for this community",
                para(t("Are you sure you want to enable invites? This will allow users to join this community through invite links again.").into()),
                Button::new(t("Enable")).key("confirm-primary").on_press(go).el(p),
                Button::new(t("Cancel")).variant(ButtonVariant::Secondary).key("confirm-secondary").on_press(close).el(p),
            )
        }
    };
    let footer = rune::row().gap(8.0).child(secondary).child(primary);
    Some(Modal::new(t(title), close).body(body).footer(footer).el(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_of_each_kind_by_priority() {
        // Sorted: terms (nd), unclaimed (d), account limited (nd), verify (nd), streamer (d), gifts (d).
        let bars = [(false, false), (false, true), (false, false), (false, false), (false, true), (false, true)];
        assert_eq!(pick(&bars), vec![0, 1], "the first non-dismissible and the first dismissible");
        assert_eq!(pick(&[(true, true), (false, true), (false, true)]), vec![0, 1], "the pinned update bar stays outside the pick");
        assert_eq!(pick(&[(false, true), (false, true)]), vec![0], "visionary MFA (7) beats the desktop notification bar (8)");
        assert_eq!(pick(&[]), Vec::<usize>::new());
    }

    #[test]
    fn restore_snapshot_follows_the_call() {
        let store = Store { me: Some(reflux_model::User { base: reflux_model::PartialUser { id: Snowflake(5), ..Default::default() }, ..Default::default() }), ..Default::default() };
        let mut st = ShellState { now_ms: 1_000_000, ..Default::default() };
        poll(&store, &mut st);
        assert_eq!(st.account_gate.voice_startup, Some(None), "nothing to restore at startup");
        st.voice.channel = Some((Some(Snowflake(9)), Snowflake(10)));
        st.voice.connected = true;
        assert!(poll(&store, &mut st));
        assert_eq!(st.prefs.voice_restore.as_ref().map(|s| (s.channel, s.updated_at)), Some((Snowflake(10), 1_000_000)));
        st.now_ms += 30_000;
        assert!(!poll(&store, &mut st), "no write between heartbeats");
        st.now_ms += 30_000;
        assert!(poll(&store, &mut st), "the minute heartbeat");
        st.voice.channel = None;
        st.voice.connected = false;
        assert!(poll(&store, &mut st));
        assert_eq!(st.prefs.voice_restore, None, "leaving drops it");
    }
}
