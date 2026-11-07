//! Our own typing notices (`RollingTypingSender`, `TypingSendThrottle`,
//! `RollingComposerTypingGate`): a change to the main composer schedules a notice 1.5 s later
//! (at once after 16 s without one), at most one per 8 s; clearing the draft cancels a pending
//! one, and none is sent while more than 5 people are typing.

use std::sync::LazyLock;

use regex::Regex;
use reflux_model::Snowflake;
use reflux_store::Store;

use super::{Intent, ShellState};

const SEND_DELAY_MS: u64 = 1500;
const THROTTLE_MS: u64 = 8000;
const IMMEDIATE_SEND_IDLE_MS: u64 = 16_000;
const SKIP_ABOVE_TRACKED: usize = 5;

/// `TypingSendSlot`: the channel, the last (scheduled) send, and when a pending send is due.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Slot {
    pub channel: Snowflake,
    pub prev_send: u64,
    pub due: Option<u64>,
}

/// `planTypingSend`: `None` = throttled, else the delay before sending.
fn plan(slot: Option<&Slot>, channel: Snowflake, now: u64) -> Option<u64> {
    let Some(s) = slot.filter(|s| s.channel == channel) else { return Some(SEND_DELAY_MS) };
    if s.due.is_some() || s.prev_send + THROTTLE_MS > now {
        return None;
    }
    Some(if s.prev_send + IMMEDIATE_SEND_IDLE_MS <= now { 0 } else { SEND_DELAY_MS })
}

/// `startTyping`.
pub fn start(st: &mut ShellState, channel: Snowflake) {
    let now = st.now_ms;
    if let Some(delay) = plan(st.typing.as_ref(), channel, now) {
        st.typing = Some(Slot { channel, prev_send: now, due: Some(now + delay) });
    }
}

/// `stopTyping`: a pending notice for `channel` is dropped.
pub fn stop(st: &mut ShellState, channel: Snowflake) {
    if st.typing.is_some_and(|s| s.channel == channel && s.due.is_some()) {
        st.typing = None;
    }
}

#[derive(Debug, PartialEq)]
enum Decision {
    Start,
    Stop,
    None,
}

/// `decideComposerTyping`: reaction shorthands (`+:emoji:`), slash commands and `s/a/b`
/// replacements don't count as typing.
fn decide(prev: &str, value: &str) -> Decision {
    static SHORTHAND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\+(?::(?:\w+:?)?)?$|(?i)^\s*\+:[a-z0-9_+~-]*:?$").expect("pattern"));
    static REPLACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^s/(.+?)/(.*?)(?:/(g)?)?$").expect("pattern"));
    if value == prev {
        return Decision::None;
    }
    let content = value.trim();
    if content.is_empty() {
        Decision::Stop
    } else if SHORTHAND.is_match(value) || value.starts_with('/') || REPLACE.is_match(content) {
        Decision::None
    } else {
        Decision::Start
    }
}

/// Each frame: watch the main composer's draft, and send a notice that came due (waking the
/// window for one still pending).
pub fn tick(store: &Store, st: &mut ShellState, tx: &mut rune::Tx) {
    if let Some(ch) = st.channel {
        match &st.typing_seen {
            Some((c, prev)) if *c == ch && *prev == st.composer.text => {}
            // `previousValue === null`: the first value seen for a channel starts nothing.
            Some((c, prev)) if *c == ch => {
                match decide(prev, &st.composer.text) {
                    Decision::Start => start(st, ch),
                    Decision::Stop => stop(st, ch),
                    Decision::None => {}
                }
                st.typing_seen = Some((ch, st.composer.text.clone()));
            }
            _ => st.typing_seen = Some((ch, st.composer.text.clone())),
        }
    }
    let Some(slot) = st.typing.as_mut() else { return };
    let Some(due) = slot.due else { return };
    if due > st.now_ms {
        tx.wake_after(std::time::Duration::from_millis(due - st.now_ms));
        return;
    }
    slot.due = None;
    let channel = slot.channel;
    if store.typing.get(&channel).map_or(0, Vec::len) <= SKIP_ABOVE_TRACKED {
        st.intents.push(Intent::Typing(channel));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_like_typing_send_throttle() {
        let ch = Snowflake(1);
        assert_eq!(plan(None, ch, 0), Some(SEND_DELAY_MS));
        let pending = Slot { channel: ch, prev_send: 0, due: Some(1500) };
        assert_eq!(plan(Some(&pending), ch, 100), None, "already pending");
        assert_eq!(plan(Some(&pending), Snowflake(2), 100), Some(SEND_DELAY_MS), "another channel");
        let sent = Slot { due: None, ..pending };
        assert_eq!(plan(Some(&sent), ch, THROTTLE_MS - 1), None, "throttled");
        assert_eq!(plan(Some(&sent), ch, THROTTLE_MS), Some(SEND_DELAY_MS));
        assert_eq!(plan(Some(&sent), ch, IMMEDIATE_SEND_IDLE_MS), Some(0), "idle long enough");
    }

    #[test]
    fn decides_like_the_composer_gate() {
        assert_eq!(decide("", "h"), Decision::Start);
        assert_eq!(decide("h", "h"), Decision::None);
        assert_eq!(decide("h", "  "), Decision::Stop);
        for v in ["+", "+:", "+:smile", "+:smile:", " +:Thumbs_Up:", "/nick", "s/teh/the/", "s/a/b/g"] {
            assert_eq!(decide("x", v), Decision::None, "{v}");
        }
        assert_eq!(decide("x", "+1 agreed"), Decision::Start);
    }

    #[test]
    fn start_stop_and_send() {
        let ch = Snowflake(3);
        let mut st = ShellState { now_ms: 10_000, ..Default::default() };
        start(&mut st, ch);
        assert_eq!(st.typing.and_then(|s| s.due), Some(11_500));
        stop(&mut st, ch);
        assert_eq!(st.typing, None, "cleared before it went out");
        start(&mut st, ch);
        st.typing.as_mut().unwrap().due = None; // sent
        st.now_ms += 1000;
        start(&mut st, ch);
        assert_eq!(st.typing.and_then(|s| s.due), None, "throttled for 8 s");
    }
}
