//! `BouncedEmailChangeModal`: when mail to the account's address bounces, replace it without
//! the usual original-address step or sudo mode (`POST /users/@me/email-change/bounced/*`).

use rune::{El, TextEdit, col, row, text};
use serde_json::{Value, json};

use super::expr::Fetched;
use super::{Intent, ShellAccess, ShellState};
use crate::i18n::{t, tf};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Input, Modal};

/// The request in flight: `request-new`, then `verify-new`; `resend-new` reuses neither.
const CHANGE: &str = "bounced-email";
const RESEND: &str = "bounced-email-resend";
/// `RESEND_COOLDOWN_MS`.
const RESEND_COOLDOWN_MS: u64 = 30_000;

/// The modal's fields; `ticket` set means the code step.
#[derive(Default)]
pub struct Ui {
    pub(crate) ticket: Option<String>,
    pub(crate) email: TextEdit,
    pub(crate) code: TextEdit,
    resend_at: u64,
    busy: bool,
    sending: bool,
    /// (field, message).
    error: Option<(&'static str, String)>,
}

/// `openBouncedEmailChange`.
pub fn open(sh: &mut ShellState) {
    if sh.account_gate.bounced.is_some() {
        return;
    }
    let mut u = Ui::default();
    u.email.focused = true;
    sh.account_gate.bounced = Some(u);
}

fn message(e: &str) -> String {
    let e = e.split_once(": ").map_or(e, |(_, m)| m);
    e.rsplit_once(" (").map_or(e, |(m, _)| m).to_string()
}

fn resend_at(v: &Value, now: u64) -> u64 {
    v["resend_available_at"].as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map_or(now + RESEND_COOLDOWN_MS, |d| d.timestamp_millis() as u64)
}

fn fetch(sh: &mut ShellState, key: &str, path: &str, body: Value) {
    sh.intents.push(Intent::Fetch { key: key.into(), method: reqwest::Method::POST, path: path.into(), body: Some(body) });
}

/// The primary button (`Send code`, then `Confirm`).
fn submit(sh: &mut ShellState) {
    let Some(u) = sh.account_gate.bounced.as_mut() else { return };
    if u.busy {
        return;
    }
    u.error = None;
    match &u.ticket {
        None => {
            let email = u.email.text.trim().to_string();
            if email.is_empty() {
                return;
            }
            u.busy = true;
            fetch(sh, CHANGE, "/users/@me/email-change/bounced/request-new", json!({ "new_email": email }));
        }
        Some(ticket) => {
            let code: String = u.code.text.split_whitespace().collect();
            if code.is_empty() {
                return;
            }
            let ticket = ticket.clone();
            u.busy = true;
            fetch(sh, CHANGE, "/users/@me/email-change/bounced/verify-new", json!({ "ticket": ticket, "code": code }));
        }
    }
}

fn resend(sh: &mut ShellState) {
    let now = sh.now_ms;
    let Some(u) = sh.account_gate.bounced.as_mut() else { return };
    if u.busy || u.sending || now < u.resend_at {
        return;
    }
    let Some(ticket) = u.ticket.clone() else { return };
    u.sending = true;
    fetch(sh, RESEND, "/users/@me/email-change/bounced/resend-new", json!({ "ticket": ticket }));
}

/// Answers to the two requests.
pub fn poll(sh: &mut ShellState) -> bool {
    let mut changed = false;
    for key in [CHANGE, RESEND] {
        let r = match sh.expr.fetched.remove(key) {
            Some(Fetched::Loading) | None => continue,
            Some(r) => r,
        };
        changed = true;
        let now = sh.now_ms;
        let Some(u) = sh.account_gate.bounced.as_mut() else { continue };
        if key == RESEND {
            u.sending = false;
        } else {
            u.busy = false;
        }
        match (key, r) {
            (RESEND, Fetched::Done(_)) => u.resend_at = now + RESEND_COOLDOWN_MS,
            (RESEND, Fetched::Failed(e)) => u.error = Some(("code", message(&e))),
            (_, Fetched::Failed(e)) => {
                let field = if u.ticket.is_some() { "code" } else { "email" };
                u.error = Some((field, message(&e)));
            }
            (_, Fetched::Done(_)) if u.ticket.is_some() => {
                // `verifyBouncedEmailChangeNew` returns the updated user; the gateway's
                // USER_UPDATE paints it and the toast confirms.
                sh.account_gate.bounced = None;
                crate::desktop::toast(&mut sh.desktop, t("Email changed"), true);
            }
            (_, Fetched::Done(v)) => {
                u.ticket = v["ticket"].as_str().map(str::to_string);
                u.resend_at = resend_at(&v, now);
                u.code = TextEdit::default();
                u.code.focused = true;
            }
        }
    }
    changed
}

/// The modal (`BouncedEmailChangeModal`).
pub fn modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let u = st.account_gate.bounced.as_ref()?;
    let close = move |s: &mut S| acc(s).account_gate.bounced = None;
    let desc = |s: &str| text(s.to_string()).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap();
    let err = |f: &str| u.error.as_ref().filter(|(k, _)| *k == f).map(|(_, m)| m.as_str());
    let go = move |s: &mut S| submit(acc(s));
    let body = match &u.ticket {
        None => col()
            .w_full()
            .gap(16.0)
            .child(desc(t("Emails to your current address are not being delivered. Enter a new email and we'll send a code there.")))
            .child(
                Input::new("bounced-email", &u.email, move |s: &mut S| -> &mut TextEdit {
                    &mut acc(s).account_gate.bounced.get_or_insert_with(Ui::default).email
                })
                .label(t("New email"))
                .placeholder("you@example.com")
                .error(err("email"))
                .el(p),
            ),
        Some(_) => {
            let left = u.resend_at.saturating_sub(st.now_ms).div_ceil(1000);
            let label = if left > 0 { tf("Resend ({secondsRemaining}s)", &[("secondsRemaining", &left.to_string())]) } else { t("Resend").to_string() };
            col()
                .w_full()
                .gap(16.0)
                .child(desc(t("Enter the code we emailed to your new address.")))
                .child(
                    Input::new("bounced-email-code", &u.code, move |s: &mut S| -> &mut TextEdit {
                        &mut acc(s).account_gate.bounced.get_or_insert_with(Ui::default).code
                    })
                    .label(t("Verification code"))
                    .placeholder("XXXX-XXXX")
                    .error(err("code"))
                    .el(p),
                )
                .child(
                    row().w_full().child(
                        Button::new(label)
                            .variant(ButtonVariant::Secondary)
                            .size(crate::ui::ButtonSize::Small)
                            .disabled(left > 0 || u.sending || u.busy)
                            .key("bounced-resend")
                            .on_press(move |s: &mut S| resend(acc(s)))
                            .el(p),
                    ),
                )
        }
    };
    let footer = match &u.ticket {
        None => row()
            .gap(8.0)
            .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
            .child(Button::new(t("Send code")).submitting(u.busy).key("bounced-send").on_press(go).el(p)),
        Some(_) => row()
            .gap(8.0)
            .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
            .child(Button::new(t("Confirm")).submitting(u.busy).key("bounced-confirm").on_press(go).el(p)),
    };
    Some(Modal::new(t("Add a working email"), close).body(body).footer(footer).el(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_advance_to_the_code_step_then_close() {
        let mut sh = ShellState::default();
        open(&mut sh);
        sh.account_gate.bounced.as_mut().unwrap().email = TextEdit::new("new@example.com");
        submit(&mut sh);
        assert!(matches!(sh.intents.last(), Some(Intent::Fetch { path, body: Some(b), .. }) if path == "/users/@me/email-change/bounced/request-new" && b["new_email"] == "new@example.com"));
        sh.expr.fetched.insert(CHANGE.into(), Fetched::Done(json!({"ticket": "t1", "resend_available_at": "2026-01-01T00:00:30Z"})));
        poll(&mut sh);
        let u = sh.account_gate.bounced.as_ref().unwrap();
        assert_eq!(u.ticket.as_deref(), Some("t1"));
        assert!(u.code.focused && u.resend_at > 0);
        sh.account_gate.bounced.as_mut().unwrap().code = TextEdit::new("1234 5678");
        submit(&mut sh);
        assert!(matches!(sh.intents.last(), Some(Intent::Fetch { path, body: Some(b), .. }) if path == "/users/@me/email-change/bounced/verify-new" && b["code"] == "12345678" && b["ticket"] == "t1"));
        sh.expr.fetched.insert(CHANGE.into(), Fetched::Done(json!({})));
        poll(&mut sh);
        assert!(sh.account_gate.bounced.is_none(), "verified");
    }

    #[test]
    fn resend_waits_out_the_cooldown() {
        let mut sh = ShellState::default();
        open(&mut sh);
        let u = sh.account_gate.bounced.as_mut().unwrap();
        (u.ticket, u.resend_at) = (Some("t1".into()), 10_000);
        sh.now_ms = 5_000;
        resend(&mut sh);
        assert!(sh.intents.is_empty(), "still cooling down");
        sh.now_ms = 10_000;
        resend(&mut sh);
        assert!(matches!(sh.intents.last(), Some(Intent::Fetch { path, .. }) if path == "/users/@me/email-change/bounced/resend-new"));
    }
}
