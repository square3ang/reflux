//! The composer's place when the member can't talk yet: `GuildVerification` (verification
//! level gates, timeouts, paused messaging, account tier gates), `VerificationBarrier` and
//! `barriers/BarrierComponents.tsx`, plus Developer tools' `mockVerificationBarrier`.
//! Fluxer has no phone or membership-screening gate: `getEffectiveGuildVerificationLevel` caps
//! the level at HIGH, so VERY_HIGH acts like HIGH.

use reflux_model::{Channel, DisabledOperations, User};
use reflux_store::Store;
use rune::{El, Icon, Shadow, col, icon, row, text};

use super::account_gate::{self, Action, Need};
use super::{Intent, ShellAccess, ShellState};
use crate::i18n::t;
use crate::icons;
use crate::theme::{Palette, dim, font};
use crate::ui::{Button, ButtonSize};

const FIVE_MINUTES: u64 = 5 * 60_000;
const TEN_MINUTES: u64 = 10 * 60_000;

/// `VerificationFailureReason` (and `AccountLimitedBarrier`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    AccountGate(Need, Action),
    /// Until (unix ms).
    TooNew(u64),
    NotLongEnough(u64),
    SendDisabled,
    TimedOut(u64),
    AccountLimited,
}

fn ms(ts: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(ts).ok().map(|d| d.timestamp_millis().max(0) as u64)
}

/// `GuildVerification.computeVerificationStatus`: why the member can't send in `guild`.
pub fn verification(store: &Store, me: Option<&User>, cfg: &account_gate::Cfg, guild: reflux_model::Snowflake, now: u64) -> Option<Reason> {
    let Some(me) = me else { return account_gate::denied(None, Action::Message, cfg).map(|n| Reason::AccountGate(n, Action::Message)) };
    let g = store.guilds.get(&guild)?;
    let info = g.info.as_ref()?;
    let member = store.member(guild, me.base.id);
    if let Some(until) = member.and_then(|m| m.communication_disabled_until.as_deref()).and_then(ms).filter(|u| *u > now) {
        return Some(Reason::TimedOut(until));
    }
    if info.disabled_operations.contains(DisabledOperations::SEND_MESSAGE) {
        return Some(Reason::SendDisabled);
    }
    // `user.reportsAccountTier`.
    if (me.account_tier.is_some() || me.claimed.is_some())
        && let Some(n) = account_gate::denied(Some(me), Action::Message, cfg)
    {
        return Some(Reason::AccountGate(n, Action::Message));
    }
    if me.base.id == info.owner_id {
        return None;
    }
    // `getEffectiveGuildVerificationLevel`: capped at HIGH, at least LOW when discoverable.
    let mut level = info.verification_level.min(3);
    if info.features.iter().any(|f| f == "DISCOVERABLE") {
        level = level.max(1);
    }
    if level == 0 || member.is_some_and(|m| !m.roles.is_empty()) {
        return None;
    }
    if let Some(n) = account_gate::denied(Some(me), Action::GuildVerification, cfg) {
        return Some(Reason::AccountGate(n, Action::GuildVerification));
    }
    let created = me.base.id.timestamp_ms();
    if level >= 2 && created + FIVE_MINUTES > now {
        return Some(Reason::TooNew(created + FIVE_MINUTES));
    }
    if level >= 3
        && let Some(joined) = member.and_then(|m| ms(&m.joined_at))
        && joined + TEN_MINUTES > now
    {
        return Some(Reason::NotLongEnough(joined + TEN_MINUTES));
    }
    None
}

/// `GuildChannelView.renderChatArea`: the barrier shown instead of the composer, if any.
pub fn reason(store: &Store, st: &ShellState, c: &Channel) -> Option<Reason> {
    let guild = c.guild_id?;
    let me = super::devtools::me(store, st);
    let me = me.as_deref();
    let cfg = &st.tiers;
    let o = &st.prefs.dev;
    // A mocked countdown runs from when the mock was picked.
    let mock_end = |default: u64| st.account_gate.mock_since + o.mock_barrier_time_remaining.unwrap_or(default);
    match o.mock_verification_barrier.as_str() {
        "unclaimed_account" => return account_gate::denied(None, Action::Message, cfg).map(|n| Reason::AccountGate(n, Action::Message)),
        "unverified_email" => return Some(Reason::AccountGate(Need::ConfirmEmail, Action::GuildVerification)),
        "account_too_new" => return Some(Reason::TooNew(mock_end(FIVE_MINUTES))),
        "not_member_long" => return Some(Reason::NotLongEnough(mock_end(TEN_MINUTES))),
        "account_limited" => return Some(Reason::AccountLimited),
        "send_message_disabled" => return Some(Reason::SendDisabled),
        _ => {}
    }
    if me.is_some_and(|m| m.account_limited) {
        return Some(Reason::AccountLimited);
    }
    verification(store, me, cfg, guild, st.now_ms)
}

/// Starts the mocked countdown and keeps a visible countdown ticking.
pub fn poll(store: &Store, st: &mut ShellState, tx: &mut rune::Tx) -> bool {
    let mock = st.prefs.dev.mock_verification_barrier.clone();
    let mut changed = false;
    if st.account_gate.mock_key != mock {
        (st.account_gate.mock_key, st.account_gate.mock_since) = (mock, st.now_ms);
        changed = true;
    }
    let ticking = st.channel.and_then(|c| store.channels.get(&c)).and_then(|c| reason(store, st, c)).is_some_and(|r| match r {
        Reason::TooNew(u) | Reason::NotLongEnough(u) | Reason::TimedOut(u) => u > st.now_ms,
        _ => false,
    });
    if ticking {
        tx.wake_after(std::time::Duration::from_millis(1000 - st.now_ms % 1000));
    }
    changed
}

/// `CountdownTimer`'s `m:ss`.
pub fn clock(left_ms: u64) -> String {
    let secs = left_ms.div_ceil(1000);
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// `BarrierBase`: the composer box with an icon, a muted line and an optional action.
fn base(glyph: &'static Icon, message: &str, action: Option<El>, p: &Palette) -> El {
    let inner = row()
        .w_full()
        .min_h(dim::FOOTER_BOX_HEIGHT)
        .py(dim::FOOTER_BOX_PADDING_Y)
        .px(dim::CHAT_HORIZONTAL_PADDING_DEFAULT - dim::FOOTER_BOX_INSET)
        .gap(dim::MESSAGE_GUTTER)
        .items_start()
        .child(row().px((dim::MESSAGE_AVATAR_SIZE - 32.0) / 2.0).child(row().size(32.0, 32.0).center().child(icon(glyph, 22.0).color(p.text_primary_muted))))
        .child(col().grow(1.0).min_w(0.0).pt(5.0).child(text(t(message).to_string()).font(font(14.0, 400.0).lh(22.0)).color(p.text_tertiary).wrap()))
        .children(action.map(|a| row().min_h(32.0).center_y().child(a)));
    let boxed = col()
        .w_full()
        .radius(dim::FOOTER_BOX_RADIUS)
        .fill(p.background_textarea)
        .box_shadow(Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 1.0, color: p.floating_surface_ring_color, inset: true })
        .child(inner);
    col().w_full().px(dim::FOOTER_BOX_INSET).pb(dim::COMPOSER_BOX_INSET).fill(p.composer_surface_color).key("barrier").role("status").child(boxed)
}

/// `.timer`.
fn timer(left_ms: u64, p: &Palette) -> Option<El> {
    (left_ms > 0).then(|| {
        row()
            .h(32.0)
            .px(12.0)
            .center_y()
            .radius(6.0)
            .fill(p.background_secondary_alt)
            .hover(p.background_tertiary)
            .child(text(clock(left_ms)).font(font(14.0, 600.0).lh(20.0).mono()).color(p.text_primary))
    })
}

/// `AnnouncementFollowBarrier`: an announcement channel the member can't post in.
pub fn follow<S: 'static>(store: &Store, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let id = c.id;
    super::gating::follow_only(store, c).then(|| {
        let b = Button::new(t("Follow")).variant(crate::ui::ButtonVariant::Secondary).size(ButtonSize::Small).key("barrier-follow").on_press(move |s: &mut S| acc(s).open_social(crate::social::Modal::Follow(id))).el(p);
        base(&icons::MEGAPHONE_SIMPLE_FILL, "Follow to get these announcements in a channel you choose.", Some(b), p)
    })
}

/// The barrier for `c`, if the member can't send there.
pub fn view<S: 'static>(store: &Store, st: &ShellState, c: &Channel, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let r = reason(store, st, c)?;
    let left = |u: u64| u.saturating_sub(st.now_ms);
    Some(match r {
        Reason::AccountGate(need, action) => {
            let (glyph, _, reason, label) = account_gate::presentation(need, action, &st.tiers);
            let me = super::devtools::me(store, st);
            // `useAccountGateAction`: a confirm-email gate resends the email, asks for the code, or
            // (bounced) changes the address.
            type Open<S> = Box<dyn Fn(&mut S)>;
            let (label, open): (&str, Open<S>) = match need {
                Need::ConfirmEmail if me.as_ref().is_some_and(|m| m.email_bounced) => ("Verify email", Box::new(move |s: &mut S| acc(s).open_settings(Some("account_security")))),
                Need::ConfirmEmail if me.as_ref().is_some_and(|m| m.account_tier.is_some() || m.claimed.is_some()) && me.as_ref().is_some_and(|m| account_gate::tier(m, &st.tiers) == account_gate::Tier::Unverified) => {
                    ("Enter code", Box::new(move |s: &mut S| acc(s).account_gate.open = Some(None)))
                }
                Need::ConfirmEmail if me.as_ref().is_some_and(|m| account_gate::needs_email_confirmation(m, &st.tiers)) => ("Resend email", Box::new(move |s: &mut S| acc(s).intents.push(Intent::ResendVerification))),
                Need::ConfirmEmail => ("Verify email", Box::new(move |s: &mut S| acc(s).open_settings(Some("account_security")))),
                _ => (label, Box::new(move |s: &mut S| acc(s).account_gate.open = Some(Some(t(reason).to_string())))),
            };
            let b = Button::new(t(label)).size(ButtonSize::Small).key("barrier-action").on_press(open).el(p);
            base(glyph, reason, Some(b), p)
        }
        Reason::TooNew(u) => base(&icons::CLOCK_FILL, "Your account is too new to send messages in this community.", timer(left(u), p), p),
        Reason::NotLongEnough(u) => base(&icons::CLOCK_FILL, "You haven't been a member of this community long enough to send messages.", timer(left(u), p), p),
        Reason::TimedOut(u) => base(&icons::TIMER_FILL, "You're timed out. Messaging, reactions, and voice are paused until the timeout expires.", timer(left(u), p), p),
        Reason::SendDisabled => base(&icons::WARNING_CIRCLE_FILL, "Messaging is temporarily paused in this community.", None, p),
        Reason::AccountLimited => {
            let help = (!st.tiers.self_hosted).then(|| Button::new(t("My text didn't go through")).size(ButtonSize::Small).key("barrier-help").on_press(move |s: &mut S| acc(s).account_gate.help = Some(Default::default())).el(p));
            base(&icons::INFO_FILL, "Messaging is paused on your account. Check your email for a quick step to continue.", help, p)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflux_model::{Guild, GuildMember, Snowflake};

    fn setup(level: u8) -> (Store, User) {
        let mut store = Store::default();
        let me = User { base: reflux_model::PartialUser { id: Snowflake(1 << 22), ..Default::default() }, account_tier: Some("verified".into()), ..Default::default() };
        let g = Guild { id: Snowflake(9), owner_id: Snowflake(1), verification_level: level, ..Default::default() };
        let mut gs = reflux_store::GuildState { id: g.id, info: Some(g), ..Default::default() };
        gs.members.insert(me.base.id, GuildMember { user: me.base.clone(), joined_at: "2026-01-01T00:00:00Z".into(), ..Default::default() });
        store.guilds.insert(Snowflake(9), gs);
        (store, me)
    }

    #[test]
    fn verification_levels_gate_by_age_and_membership() {
        let cfg = account_gate::Cfg::default();
        let (store, me) = setup(2);
        let created = me.base.id.timestamp_ms();
        let joined = ms("2026-01-01T00:00:00Z").unwrap();
        assert_eq!(verification(&store, Some(&me), &cfg, Snowflake(9), created + 1000), Some(Reason::TooNew(created + FIVE_MINUTES)), "medium: 5 minutes old");
        assert_eq!(verification(&store, Some(&me), &cfg, Snowflake(9), joined + 1000), None, "medium ignores how long they've been a member");
        let (store, me) = setup(4);
        assert_eq!(verification(&store, Some(&me), &cfg, Snowflake(9), joined + 1000), Some(Reason::NotLongEnough(joined + TEN_MINUTES)), "very high acts like high");
        assert_eq!(verification(&store, Some(&me), &cfg, Snowflake(9), joined + TEN_MINUTES), None);
        let unverified = User { account_tier: Some("unverified".into()), email: Some("a@b".into()), ..me.clone() };
        assert_eq!(verification(&store, Some(&unverified), &cfg, Snowflake(9), joined + TEN_MINUTES), Some(Reason::AccountGate(Need::ConfirmEmail, Action::GuildVerification)));
        let (store, _) = setup(0);
        assert_eq!(verification(&store, Some(&unverified), &cfg, Snowflake(9), joined), None, "no level, legacy tier: talk");
    }

    #[test]
    fn timeouts_and_paused_messaging_come_first() {
        let cfg = account_gate::Cfg::default();
        let (mut store, me) = setup(0);
        let until = "2030-01-01T00:00:00Z";
        store.guilds.get_mut(&Snowflake(9)).unwrap().members.insert(me.base.id, GuildMember { user: me.base.clone(), communication_disabled_until: Some(until.into()), ..Default::default() });
        assert_eq!(verification(&store, Some(&me), &cfg, Snowflake(9), 0), Some(Reason::TimedOut(ms(until).unwrap())));
        assert_eq!(clock(61_000), "1:01");
        assert_eq!(clock(999), "0:01");
    }
}
