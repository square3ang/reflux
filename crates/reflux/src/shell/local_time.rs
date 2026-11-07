//! `UserProfileTimezoneInfo` (`popouts/UserProfileShared.tsx`): "Local time", the time at the
//! profile's UTC offset and how far it is ahead of or behind the viewer.

use rune::{El, col, text};

use crate::i18n::{t, tf};
use crate::theme::{Palette, font};

/// `formatOffsetDifferenceDuration`.
fn duration(minutes: i32) -> String {
    let a = minutes.unsigned_abs();
    let (h, m) = ((a / 60).to_string(), (a % 60).to_string());
    match (a / 60, a % 60) {
        (0, _) => tf("{minutes, plural, one {# minute} other {# minutes}}", &[("minutes", &m)]),
        (_, 0) => tf("{hours, plural, one {# hour} other {# hours}}", &[("hours", &h)]),
        _ => tf("{hours, plural, one {# hour} other {# hours}} {minutes, plural, one {# minute} other {# minutes}}", &[("hours", &h), ("minutes", &m)]),
    }
}

/// The difference line for a profile `offset` minutes from UTC seen from `viewer`'s offset.
fn difference(offset: i32, viewer: i32) -> String {
    let d = offset - viewer;
    match d.signum() {
        0 => t("Same time as you").into(),
        1 => tf("{duration} ahead of you", &[("duration", &duration(d))]),
        _ => tf("{duration} behind you", &[("duration", &duration(d))]),
    }
}

/// The section for a profile `offset` minutes from UTC.
pub fn info(offset: i32, p: &Palette) -> El {
    let now = chrono::Utc::now();
    let at = now.with_timezone(&chrono::FixedOffset::east_opt(offset * 60).unwrap_or(chrono::FixedOffset::east_opt(0).unwrap()));
    let viewer = chrono::Local::now().offset().local_minus_utc() / 60;
    col()
        .w_full()
        .gap(2.0)
        .child(text(t("Local time")).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary))
        .child(text(crate::desktop::time::time(&at, crate::i18n::locale())).font(font(14.0, 400.0).lh(20.0)).color(p.text_chat))
        .child(text(difference(offset, viewer)).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_ahead_behind_and_same() {
        assert_eq!(difference(540, 540), "Same time as you");
        assert_eq!(difference(540, 0), "9 hours ahead of you");
        assert_eq!(difference(0, 330), "5 hours 30 minutes behind you");
        assert_eq!(difference(61, 60), "1 minute ahead of you");
    }
}
