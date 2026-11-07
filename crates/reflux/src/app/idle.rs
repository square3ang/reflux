//! Auto-idle (`ui/state/Idle.ts`, `presence/state/LocalPresence.ts`): after 10 minutes without
//! input an Online account publishes `idle` (op 3), and `afk` once the account's AFK timeout
//! passes; the next input publishes the saved status again. Input anywhere on the system counts
//! too, like Fluxer's desktop build (`desktop::os_idle`).

use std::time::{Duration, Instant};

use reflux_model::{GatewayPayload, OpCode, Status};
use reflux_net::Command;
use serde_json::json;

use crate::State;

/// `IDLE_DURATION_MS`.
pub(crate) const IDLE: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
pub struct Idle {
    /// When the session became ready (stands in for input before the first one).
    since: Option<Instant>,
    /// The (status, afk) last published this session; `app::publish_presence` also updates it.
    pub(super) sent: Option<(Status, bool)>,
}

/// What to publish for the saved status after `inactive` without input (`LocalPresence.updatePresence`).
fn presence(saved: Status, inactive: Duration, afk_timeout: Duration) -> (Status, bool) {
    let status = if saved == Status::Online && inactive >= IDLE { Status::Idle } else { saved };
    (status, inactive >= afk_timeout)
}

/// A new session publishes the saved presence itself.
pub fn ready(s: &mut State) {
    s.app.idle = Idle { since: Some(Instant::now()), sent: None };
    crate::desktop::os_idle::start();
}

pub fn tick(s: &mut State, tx: &mut rune::Tx) {
    let Some(since) = s.app.idle.since else { return };
    let last = [rune::last_input(), crate::desktop::os_idle::last_activity()].into_iter().flatten().fold(since, Instant::max);
    let inactive = last.elapsed();
    let saved = s.app.store.settings.status;
    let afk = Duration::from_secs(u64::from(s.app.store.settings.afk_timeout.max(60)));
    let now = presence(saved, inactive, afk);
    let (status, _) = now;
    // Nothing to say until idle starts: a fresh session already has the saved status.
    let changed = match s.app.idle.sent {
        None => status != saved || now.1,
        Some(prev) => prev != now,
    };
    if changed && let Some(gw) = &s.app.gateway {
        let d = json!({ "status": status, "afk": now.1, "mobile": false });
        if gw.send(Command::Send(GatewayPayload::command(OpCode::PRESENCE_UPDATE, d))).is_ok() {
            s.app.idle.sent = Some(now);
        }
    }
    // Wake when the next threshold passes, so an untouched window still goes idle.
    // While idle, look again when the OS may have seen input elsewhere.
    match [IDLE, afk].into_iter().filter(|&d| d > inactive).min() {
        Some(next) => tx.wake_after((next - inactive).min(crate::desktop::os_idle::CHECK)),
        None => tx.wake_after(crate::desktop::os_idle::CHECK),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_goes_idle_after_ten_minutes_and_afk_after_the_timeout() {
        let afk = Duration::from_secs(600);
        assert_eq!(presence(Status::Online, Duration::from_secs(599), afk), (Status::Online, false));
        assert_eq!(presence(Status::Online, IDLE, afk), (Status::Idle, true));
        assert_eq!(presence(Status::Online, IDLE, Duration::from_secs(3600)), (Status::Idle, false));
        assert_eq!(presence(Status::Dnd, Duration::from_secs(3600), afk), (Status::Dnd, true), "only Online turns idle");
    }
}
