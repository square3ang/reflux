//! `SudoVerificationModal` (`auth/components/modals/SudoVerificationModal.tsx`): a request that
//! answered `SUDO_MODE_REQUIRED` waits here while the person proves it's them with a password,
//! an authenticator or backup code, a passkey or an emailed code, then goes out again with the
//! proof merged into its body (`users/mfa.mdx`, sudo verification object). The server's
//! `X-Fluxer-Sudo-Mode-JWT` then covers the next five minutes (`reflux_net::Client`).

use std::sync::mpsc::Sender;

use rune::{El, TextEdit, col, text};
use serde_json::{Value, json};

use super::Op;
use crate::guild::Call;
use crate::i18n::t;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Input, Modal, mix};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Passkey {
    #[default]
    Idle,
    Waiting,
    /// The key wants its PIN (`wrong`: the last one was wrong).
    Pin { wrong: bool },
}

#[derive(Default)]
pub struct Sudo {
    pub call: Call,
    pub totp: bool,
    pub backup: bool,
    pub webauthn: bool,
    pub password: bool,
    pub email: bool,
    pub code: TextEdit,
    pub secret: TextEdit,
    pub email_code: TextEdit,
    pub email_ticket: Option<String>,
    pub busy: bool,
    pub error: Option<String>,
    pub passkey: Passkey,
    pub pin: TextEdit,
    pub pin_reply: Option<Sender<authenticator::Pin>>,
    pub cancel: Option<crate::passkey::Cancel>,
}

impl Sudo {
    pub fn fail(&mut self, e: String) {
        (self.busy, self.passkey, self.error) = (false, Passkey::Idle, Some(e));
    }
}

impl Default for Call {
    fn default() -> Self {
        Call { method: reqwest::Method::POST, path: String::new(), body: None, then: crate::guild::Then::Nothing }
    }
}

/// Opens the prompt for `call` from the `SUDO_MODE_REQUIRED` error's fields.
pub fn require(sh: &mut ShellState, call: Call, err: &Value) {
    let m = &err["methods"];
    let on = |k: &str| m[k].as_bool().unwrap_or(false);
    let has_mfa = err["has_mfa"].as_bool().unwrap_or(on("totp") || on("webauthn"));
    let mut code = TextEdit::default();
    code.focused = true;
    sh.account.sudo = Some(Sudo {
        call,
        totp: on("totp"),
        backup: on("backup_codes"),
        webauthn: on("webauthn"),
        // Without the newer fields, an account with no second factor proves it with its password.
        password: m.get("password").map_or(!has_mfa, |v| v.as_bool() == Some(true)),
        email: on("email_code"),
        code,
        ..Default::default()
    });
}

/// Sends the waiting call again with `proof` merged into its JSON body.
pub fn retry(sh: &mut ShellState, proof: Value) {
    let Some(x) = sh.account.sudo.as_mut() else { return };
    (x.busy, x.error) = (true, None);
    let mut call = x.call.clone();
    let mut body = match call.body.take() {
        Some(Value::Object(o)) => o,
        _ => Default::default(),
    };
    if let Value::Object(p) = proof {
        body.extend(p);
    }
    call.body = Some(Value::Object(body));
    sh.intents.push(Intent::Api(call));
}

/// The proof the filled-in fields make, if any.
pub fn proof(x: &Sudo) -> Option<Value> {
    let code: String = x.code.text.split_whitespace().collect();
    if (x.totp || x.backup) && !code.is_empty() {
        return Some(json!({ "mfa_method": "totp", "mfa_code": code }));
    }
    if let Some(ticket) = &x.email_ticket
        && !x.email_code.text.trim().is_empty()
    {
        return Some(json!({ "mfa_method": "email_code", "email_code_ticket": ticket, "email_code": x.email_code.text.trim() }));
    }
    (x.password && !x.secret.text.is_empty()).then(|| json!({ "password": x.secret.text }))
}

fn close<S: 'static>(acc: ShellAccess<S>) -> impl Fn(&mut S) + Clone + 'static {
    move |s: &mut S| {
        let sh = acc(s);
        if let Some(c) = sh.account.sudo.take().and_then(|x| x.cancel) {
            c.cancel();
        }
        sh.form.busy = false;
    }
}

pub fn view<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let x = st.account.sudo.as_ref()?;
    let desc = |s: &str| text(s.to_string()).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap();
    let err_color = mix(p.status_danger, p.text_primary, 78.0);
    let mut body = col().w_full().gap(16.0).child(desc(t("For your security, please confirm it's you to continue.")));
    let none = !(x.totp || x.backup || x.webauthn || x.password || x.email);
    if none {
        body = body
            .child(text(t("No verification methods available")).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary))
            .child(desc(t("Your account requires multi-factor authentication, but no supported methods are set up on this device. Sign in on a device with your authenticator app or security key, or contact the administrators of this instance.")));
    }
    let show_code = x.totp || x.backup;
    if show_code {
        let backup_only = !x.totp;
        let (label, ph) = if backup_only { (t("Backup code"), t("Backup code")) } else { (t("Authenticator code"), t("6-digit code")) };
        body = body.child(
            Input::new("sudo-code", &x.code, move |s: &mut S| -> &mut TextEdit { &mut acc(s).account.sudo.get_or_insert_with(Default::default).code })
                .label(label)
                .placeholder(ph)
                .el(p),
        );
    }
    if x.webauthn {
        let waiting = x.passkey != Passkey::Idle;
        let b = Button::new(if waiting { t("Waiting for passkey…") } else { t("Continue with passkey") })
            .variant(if show_code || x.password { ButtonVariant::Secondary } else { ButtonVariant::Primary })
            .fit_container()
            .submitting(waiting)
            .key("sudo-passkey")
            .on_press(move |s: &mut S| {
                let sh = acc(s);
                if let Some(x) = sh.account.sudo.as_mut() {
                    (x.passkey, x.error) = (Passkey::Waiting, None);
                }
                sh.intents.push(Intent::Account(Op::SudoPasskey));
            });
        body = body.child(b.el(p));
        if let Passkey::Pin { wrong } = x.passkey {
            let mut i = Input::new("sudo-pin", &x.pin, move |s: &mut S| -> &mut TextEdit { &mut acc(s).account.sudo.get_or_insert_with(Default::default).pin })
                .label(t("Security key PIN"))
                .secure();
            if wrong {
                i = i.error(Some(t("Wrong PIN. Try again.")));
            }
            body = body.child(i.el(p)).child(
                Button::new(t("Continue"))
                    .key("sudo-pin-submit")
                    .on_press(move |s: &mut S| {
                        if let Some(x) = acc(s).account.sudo.as_mut()
                            && let Some(reply) = x.pin_reply.take()
                        {
                            let _ = reply.send(authenticator::Pin::new(&std::mem::take(&mut x.pin).text));
                            x.passkey = Passkey::Waiting;
                        }
                    })
                    .el(p),
            );
        }
    }
    if x.email {
        body = match &x.email_ticket {
            None => body.child(
                Button::new(t("Email me a code"))
                    .variant(if x.password || x.webauthn { ButtonVariant::Secondary } else { ButtonVariant::Primary })
                    .fit_container()
                    .key("sudo-email")
                    .on_press(move |s: &mut S| acc(s).intents.push(Intent::Account(Op::SudoEmail)))
                    .el(p),
            ),
            Some(_) => body.child(desc(t("Enter the code we sent to your email address."))).child(
                Input::new("sudo-email-code", &x.email_code, move |s: &mut S| -> &mut TextEdit {
                    &mut acc(s).account.sudo.get_or_insert_with(Default::default).email_code
                })
                .label(t("Verification code"))
                .el(p),
            ),
        };
    }
    if x.password {
        body = body.child(
            Input::new("sudo-password", &x.secret, move |s: &mut S| -> &mut TextEdit { &mut acc(s).account.sudo.get_or_insert_with(Default::default).secret })
                .label(t("Password"))
                .secure()
                .el(p),
        );
    }
    if let Some(e) = &x.error {
        body = body.child(text(e.clone()).font(font(14.0, 400.0).lh(20.0)).color(err_color).wrap());
    }
    let can_submit = !none && (show_code || x.password || x.email_ticket.is_some());
    let mut footer = rune::row().gap(8.0).child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close(acc)).el(p));
    if can_submit {
        footer = footer.child(
            Button::new(t("Verify"))
                .submitting(x.busy)
                .key("sudo-verify")
                .on_press(move |s: &mut S| {
                    let sh = acc(s);
                    match sh.account.sudo.as_ref().map(proof) {
                        Some(Some(v)) => retry(sh, v),
                        Some(None) => {
                            if let Some(x) = sh.account.sudo.as_mut() {
                                x.error = Some(t("This field is required.").into());
                            }
                        }
                        None => {}
                    }
                })
                .el(p),
        );
    }
    Some(Modal::new(t("Verify it's you"), close(acc)).body(body).footer(footer).el(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_picks_the_filled_method_and_retry_merges_it() {
        let mut sh = ShellState::default();
        let call = Call { method: reqwest::Method::POST, path: "/users/@me/disable".into(), body: Some(json!({"x": 1})), then: crate::guild::Then::SignOut };
        require(&mut sh, call, &json!({"has_mfa": true, "methods": {"totp": true, "webauthn": false, "backup_codes": true}}));
        let x = sh.account.sudo.as_mut().unwrap();
        assert!(x.totp && !x.password, "an account with a second factor can't use its password");
        assert_eq!(proof(x), None);
        x.code = TextEdit::new("123 456");
        assert_eq!(proof(x), Some(json!({"mfa_method": "totp", "mfa_code": "123456"})));
        let v = proof(x).unwrap();
        retry(&mut sh, v);
        let Some(Intent::Api(c)) = sh.intents.pop() else { panic!("retried") };
        assert_eq!(c.body, Some(json!({"x": 1, "mfa_method": "totp", "mfa_code": "123456"})));
        assert!(sh.account.sudo.as_ref().unwrap().busy);

        require(&mut sh, Call::default(), &json!({"has_mfa": false, "methods": {"totp": false, "webauthn": false, "backup_codes": false}}));
        let x = sh.account.sudo.as_mut().unwrap();
        assert!(x.password, "older servers: no second factor means password");
        x.secret = TextEdit::new("hunter22");
        assert_eq!(proof(x), Some(json!({"password": "hunter22"})));
    }
}
