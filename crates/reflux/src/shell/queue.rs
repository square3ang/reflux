//! Failed sends (`MessageQueue`, `MessageRetryUtils`): a text message whose send failed stays in
//! the list in red with Retry / Delete in its action bar; the error modals Fluxer shows for it
//! (`MessageSendFailedModal`, `MessageSendTooQuickModal`, `SlowmodeRateLimitedModal`, …), and
//! `RateLimitedConfirmModal`'s wording for edits and publishing.

use reflux_model::{Message, Snowflake};
use rune::{El, row};

use super::{Intent, ShellAccess, ShellState};
use crate::i18n::{t, tf};
use crate::icons;
use crate::theme::Palette;
use crate::ui::{Tooltip, popup_access};

/// A send that failed: kept so Retry can send it again.
#[derive(Clone, Debug, PartialEq)]
pub struct Failed {
    pub channel: Snowflake,
    pub reply: Option<(Snowflake, bool)>,
}

/// `formatRateLimitDuration`: "N seconds", "N minutes", or "N minutes and M seconds".
pub fn rate_duration(secs: f64) -> String {
    let total = secs.ceil().max(1.0) as i64;
    if total < 60 {
        return tf("{seconds, plural, one {# second} other {# seconds}}", &[("seconds", &total.to_string())]);
    }
    let (m, s) = (total / 60, total % 60);
    if s == 0 {
        return tf("{minutes, plural, one {# minute} other {# minutes}}", &[("minutes", &m.to_string())]);
    }
    tf("{minutes, plural, one {# minute} other {# minutes}} and {seconds, plural, one {# second} other {# seconds}}", &[("minutes", &m.to_string()), ("seconds", &s.to_string())])
}

/// `RateLimitedConfirmModal`'s body: the wait, or the generic line without one.
pub fn rate_body(retry_after: Option<f64>) -> String {
    match retry_after {
        Some(s) => tf("You're being rate limited. Wait {duration} before trying again.", &[("duration", &rate_duration(s))]),
        None => t("Slow down and wait a moment before trying again.").into(),
    }
}

/// The modal (title, body) for a failed send (`MessageQueue.showErrorModal` and the system
/// messages it posts for DM / account / safety blocks). `kind`: the channel type.
pub fn send_error(e: &reflux_net::Error, files: bool, kind: u16) -> (String, String) {
    let api = match e {
        reflux_net::Error::Api { status, error } => Some((status.as_u16(), error.code.as_str(), error.retry_after)),
        _ => None,
    };
    let failed = || {
        let body = if files {
            t("One or more attachments couldn't be read or uploaded. The file may have been moved or deleted. Add it again and retry.")
        } else {
            t("Try sending your message again.")
        };
        (t("Your message didn't send").to_string(), body.to_string())
    };
    let undelivered = t("Your message didn't send").to_string();
    let Some((status, code, retry)) = api else { return failed() };
    match code {
        _ if status == 429 && code != "SLOWMODE_RATE_LIMITED" => (t("You're sending messages too quickly").into(), rate_body(retry)),
        "SLOWMODE_RATE_LIMITED" => match retry.map(|s| s.ceil() as i64).filter(|s| *s > 0) {
            None => failed(),
            Some(s) if s < 60 => (t("Slowmode active").into(), tf("Slowmode is on. Wait {seconds, plural, one {# second} other {# seconds}} before sending another.", &[("seconds", &s.to_string())])),
            Some(s) if s % 60 == 0 => (t("Slowmode active").into(), tf("Slowmode is on. Wait {minutes, plural, one {# minute} other {# minutes}} before sending another.", &[("minutes", &(s / 60).to_string())])),
            Some(s) => (
                t("Slowmode active").into(),
                tf(
                    "Slowmode is on. Wait {minutes, plural, one {# minute} other {# minutes}} and {seconds, plural, one {# second} other {# seconds}} before sending another.",
                    &[("minutes", &(s / 60).to_string()), ("seconds", &(s % 60).to_string())],
                ),
            ),
        },
        "FEATURE_TEMPORARILY_DISABLED" if status == 403 => (t("Feature temporarily disabled").into(), t("This feature is paused. Try again later.").into()),
        "EXPLICIT_CONTENT_CANNOT_BE_SENT" => (
            t("Mature content not allowed").into(),
            match kind {
                3 => t("This group is not marked for mature content. The group owner can turn on Mature content in the group settings if everyone in the group is 18 or older.").into(),
                1 => tf(
                    "You or the person you are messaging has chosen to block sensitive media in direct messages. Each of you can change this in {sensitiveContentSettingsPath}.",
                    &[("sensitiveContentSettingsPath", &settings_path("Privacy & Safety"))],
                ),
                _ => t("This channel is not marked for mature content. Mature content can only be sent in channels marked for mature content. Ask a moderator to update this channel if appropriate.").into(),
            },
        ),
        "FILE_SIZE_TOO_LARGE" => (t("File size too large").into(), t("This limit is configured by your instance administrator.").into()),
        // shortcut: Fluxer posts these as a local "Only you can see this" system message; here
        // they're the modal's body.
        "CANNOT_SEND_MESSAGES_TO_USER" => (
            undelivered,
            tf(
                "Your message could not be delivered. This is usually because you don't share a community with the recipient or the recipient is only accepting direct messages from friends. You may also need to adjust your own direct message privacy settings in {directMessagePrivacySettingsPath}.",
                &[("directMessagePrivacySettingsPath", &settings_path("Privacy & Safety"))],
            ),
        ),
        "UNCLAIMED_ACCOUNT_CANNOT_SEND_DIRECT_MESSAGES" => (undelivered, t("Your message could not be delivered. You need to claim your account to send direct messages.").into()),
        "UNCLAIMED_ACCOUNT_CANNOT_SEND_MESSAGES" => (undelivered, t("Your message could not be delivered. You need to claim your account to send messages.").into()),
        "CONTENT_BLOCKED" => (
            undelivered,
            t("Your message could not be delivered because it was flagged by our safety systems. If you believe this is a mistake, please contact the administrators of this instance.").into(),
        ),
        _ => failed(),
    }
}

/// `formatUserSettingsPath`: "User Settings > <tab>".
fn settings_path(tab: &str) -> String {
    format!("{} > {}", t("User Settings"), t(tab))
}

/// The action bar of a failed message: Try again, Delete message.
pub fn bar<S: 'static>(m: &Message, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) -> El {
    let (id, ch) = (m.id, m.channel_id);
    let pops = popup_access(move |s: &mut S| &mut acc(s).popups);
    let button = |glyph: &'static rune::Icon, danger: bool| {
        let fg = if danger { p.status_danger } else { p.text_tertiary };
        row().size(28.0, 28.0).center().radius(6.0).cursor(rune::Cursor::Pointer).role("button").hover_group().hover(p.surface_interactive_hover_bg).child(rune::icon(glyph, 20.0).color(fg).group_color(if danger { fg } else { p.text_primary }))
    };
    let retry = button(&icons::ARROWS_CLOCKWISE_BOLD, false).on_press(move |s: &mut S| acc(s).intents.push(Intent::RetrySend { channel: ch, nonce: id }));
    let delete = button(&icons::TRASH_FILL, true).on_press(move |s: &mut S| {
        let st = acc(s);
        st.failed.remove(&id);
        st.intents.push(Intent::DropLocal { channel: ch, nonce: id });
    });
    row()
        .padding(3.0)
        .center_y()
        .radius(8.0)
        .fill(p.form_surface_background)
        .border(1.0, crate::ui::over(p.background_modifier_accent, p.form_surface_background))
        .child(Tooltip::new(&format!("retry-{id}"), t("Try again")).el(retry, &st.popups, pops, p, layer))
        .child(Tooltip::new(&format!("drop-{id}"), t("Delete message")).el(delete, &st.popups, pops, p, layer))
        .pin(None, Some(-24.0), Some(14.0), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(status: u16, code: &str, retry: Option<f64>) -> reflux_net::Error {
        let error = reflux_model::ApiError { code: code.into(), retry_after: retry, ..Default::default() };
        reflux_net::Error::Api { status: reqwest::StatusCode::from_u16(status).unwrap(), error: Box::new(error) }
    }

    #[test]
    fn send_errors_pick_fluxers_modal() {
        assert_eq!(rate_duration(0.2), "1 second");
        assert_eq!(rate_duration(125.0), "2 minutes and 5 seconds");
        assert_eq!(send_error(&api(429, "RATE_LIMITED", Some(3.0)), false, 0).1, "You're being rate limited. Wait 3 seconds before trying again.");
        assert_eq!(send_error(&api(400, "SLOWMODE_RATE_LIMITED", Some(120.0)), false, 0).1, "Slowmode is on. Wait 2 minutes before sending another.");
        assert_eq!(send_error(&api(500, "x", None), true, 0).0, "Your message didn't send");
        assert_eq!(send_error(&reflux_net::Error::Upload("gone".into()), false, 0).1, "Try sending your message again.");
    }
}
