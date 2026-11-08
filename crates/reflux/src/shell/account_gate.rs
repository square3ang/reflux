//! Account tiers (`AccountTierRules.ts`, `AccountGates.ts`), their presentation
//! (`AccountGatePresentation.tsx`), the claim account flow (`ClaimAccountModal.tsx`: the
//! username variant with its recovery kit, the account email flow, the legacy
//! `EmailClaimAccountModal` and `offerAuthenticatorCalmly`) and the "My text didn't go through"
//! help (`MessagingPausedHelpModal.tsx`).

use reflux_model::User;
use reflux_store::Store;
use rune::{El, Icon, TextEdit, col, row, text};
use serde_json::{Value, json};

use super::expr::Fetched;
use super::{Intent, ShellAccess, ShellState};
use crate::i18n::{t, tf};
use crate::icons;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Input, Modal};

/// The instance's account rules (discovery `features`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cfg {
    /// `account_identity == "username"`.
    pub username: bool,
    pub emails_enabled: bool,
    /// `email_delivery_state == "on"` (derived like `RuntimeConfig.emailDeliveryState`).
    pub delivery_on: bool,
    /// `interaction_tier == "enforced"` (else legacy).
    pub enforced: bool,
    /// `verification_basis == "local_credential"`.
    pub local_credential: bool,
    pub self_hosted: bool,
}

impl Cfg {
    pub fn of(f: &reflux_model::InstanceFeatures) -> Self {
        let username = f.account_identity == "username";
        Self {
            username,
            emails_enabled: f.emails_enabled,
            delivery_on: f.email_delivery_state.as_deref().map_or(!username && f.emails_enabled, |s| s == "on"),
            enforced: f.interaction_tier.as_deref() == Some("enforced"),
            local_credential: f.verification_basis.as_deref() == Some("local_credential"),
            self_hosted: f.self_hosted,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Unclaimed,
    Unverified,
    Verified,
}

/// `AccountVerificationNeeds`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    AddEmail,
    ConfirmEmail,
    SetPassword,
}

/// The `TierGatedActions` Reflux gates on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Message,
    GuildVerification,
}

/// `User.accountTier`: the server's tier, else derived from the email / password state.
pub fn tier(me: &User, cfg: &Cfg) -> Tier {
    match me.account_tier.as_deref() {
        Some("unclaimed") => return Tier::Unclaimed,
        Some("unverified") => return Tier::Unverified,
        Some("verified") => return Tier::Verified,
        _ => {}
    }
    if me.claimed == Some(false) {
        return Tier::Unclaimed;
    }
    // `resolveAccountTier`.
    let derived = if me.base.bot || me.base.system {
        Tier::Verified
    } else if cfg.username || (cfg.local_credential && me.email.is_some()) {
        if me.has_password == Some(true) || !me.authenticator_types.is_empty() { Tier::Verified } else { Tier::Unclaimed }
    } else if me.email.is_none() {
        Tier::Unclaimed
    } else if me.verified {
        Tier::Verified
    } else {
        Tier::Unverified
    };
    if me.claimed == Some(true) && derived == Tier::Unclaimed { Tier::Verified } else { derived }
}

/// `resolveVerificationNeed`.
fn need_of(tier: Tier, cfg: &Cfg, has_email: bool) -> Option<Need> {
    match tier {
        Tier::Verified => None,
        Tier::Unverified => Some(Need::ConfirmEmail),
        Tier::Unclaimed if cfg.username || has_email => Some(Need::SetPassword),
        Tier::Unclaimed => Some(Need::AddEmail),
    }
}

/// `User.verificationNeed`: the server's, unless a developer override reshaped the tier.
pub fn need(me: &User, cfg: &Cfg) -> Option<Need> {
    let tier = tier(me, cfg);
    let server = match me.verification_need.as_deref() {
        Some("add_email") => Some(Need::AddEmail),
        Some("confirm_email") => Some(Need::ConfirmEmail),
        Some("password") => Some(Need::SetPassword),
        _ => None,
    };
    if me.verification_need.is_some() && tier != Tier::Verified {
        return server;
    }
    need_of(tier, cfg, me.email.is_some())
}

/// `canPerform`: the need that blocks `action`, if any (`evaluateTierGate`).
pub fn denied(me: Option<&User>, action: Action, cfg: &Cfg) -> Option<Need> {
    let tier = me.map_or(Tier::Unclaimed, |m| tier(m, cfg));
    let blocked = match (tier, action) {
        (Tier::Verified, _) => false,
        (_, Action::GuildVerification) => true,
        (Tier::Unclaimed, Action::Message) => true,
        (Tier::Unverified, Action::Message) => cfg.enforced,
    };
    if !blocked {
        return None;
    }
    need_of(tier, cfg, me.is_some_and(|m| m.email.is_some()))
}

/// `needsEmailConfirmation`.
pub fn needs_email_confirmation(me: &User, cfg: &Cfg) -> bool {
    cfg.emails_enabled && need(me, cfg) == Some(Need::ConfirmEmail)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    AddEmail,
    AddEmailNoDelivery,
    ConfirmEmail,
    SetPassword,
}

fn variant(need: Need, cfg: &Cfg) -> Variant {
    match need {
        Need::AddEmail if cfg.delivery_on => Variant::AddEmail,
        Need::AddEmail => Variant::AddEmailNoDelivery,
        Need::ConfirmEmail => Variant::ConfirmEmail,
        Need::SetPassword => Variant::SetPassword,
    }
}

/// `getAccountGatePresentation`: (icon, title, reason, action label).
pub fn presentation(need: Need, action: Action, cfg: &Cfg) -> (&'static Icon, &'static str, &'static str, &'static str) {
    let v = variant(need, cfg);
    let reason = match (v, action) {
        (Variant::AddEmail, Action::Message) => "Add an email to send messages.",
        (Variant::AddEmail, Action::GuildVerification) => "Add an email to talk in this community.",
        (Variant::AddEmailNoDelivery, Action::Message) => "Add an email and a password to send messages.",
        (Variant::AddEmailNoDelivery, Action::GuildVerification) => "Add an email and a password to talk in this community.",
        (Variant::ConfirmEmail, Action::Message) => "Confirm your email to send messages.",
        (Variant::ConfirmEmail, Action::GuildVerification) => "Confirm your email to talk in this community.",
        (Variant::SetPassword, Action::Message) => "Set a password to send messages.",
        (Variant::SetPassword, Action::GuildVerification) => "Set a password to talk in this community.",
    };
    let (title, label) = match v {
        Variant::AddEmail => ("Add an email", "Add email"),
        Variant::AddEmailNoDelivery => ("Add an email and a password", "Add email and password"),
        Variant::ConfirmEmail => ("Confirm your email", "Confirm email"),
        Variant::SetPassword => ("Set a password", "Set password"),
    };
    let icon = if v == Variant::SetPassword { &icons::USER_CIRCLE_FILL } else { &icons::ENVELOPE_SIMPLE_FILL };
    (icon, title, reason, label)
}

/// `getUnclaimedAccountPresentation(NAGBAR)`: (message, action label).
pub fn unclaimed_nagbar(need: Option<Need>) -> (&'static str, &'static str) {
    if need == Some(Need::SetPassword) {
        ("This account is unclaimed. Set a password to sign in on other devices.", "Claim account")
    } else {
        ("This account is unclaimed. Add an email to sign in on other devices.", "Add email")
    }
}

// ---- Claim account ------------------------------------------------------------------------------

const CLAIM: &str = "claim-account";
const CLAIM_SEND: &str = "claim-account-send";

/// The legacy flow's request in flight under `CLAIM` (`EmailClaimAccountModal`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Step {
    #[default]
    None,
    /// `POST /users/@me/email-change/start`: the ticket and original proof.
    Start,
    /// `request-new`: the code goes to the new address.
    Request,
    /// `verify-new`: the code for an email token.
    Verify,
    /// `PATCH /users/@me` with the token and the password.
    Apply,
}

#[derive(Default)]
pub struct Claim {
    /// `UsernameClaimAccountModal` (username sign-in instances).
    pub username_mode: bool,
    /// `EmailClaimAccountModal`: the server reports no account tier (`reportsAccountTier`).
    pub legacy: bool,
    pub ticket: Option<String>,
    pub proof: Option<String>,
    pub email_token: Option<String>,
    pub step: Step,
    /// The code step of the email flow.
    pub verify: bool,
    /// `reason`: the gate that opened it.
    pub reason: Option<String>,
    /// The account already has this email (it needs a password).
    pub existing_email: Option<String>,
    pub asks_password: bool,
    pub username: TextEdit,
    pub email: TextEdit,
    pub password: TextEdit,
    pub confirm: TextEdit,
    pub code: TextEdit,
    /// (field, message).
    pub error: Option<(&'static str, String)>,
    pub busy: bool,
    /// A code is being sent.
    pub sending: bool,
    /// When "Send a new code" is allowed again (unix ms), and "We sent a new code." shows.
    pub resend_at: u64,
    pub resent: bool,
}

/// `openClaimAccountModal`.
pub fn open_claim(store: &Store, sh: &mut ShellState, reason: Option<String>) {
    if sh.account_gate.claim.is_some() {
        return;
    }
    let cfg = sh.tiers.clone();
    let me = super::devtools::me(store, sh).map(|m| m.into_owned());
    let existing_email = me.as_ref().filter(|m| need(m, &cfg) == Some(Need::SetPassword)).and_then(|m| m.email.clone());
    let start_with_code = me.as_ref().is_some_and(|m| tier(m, &cfg) == Tier::Unverified);
    if !cfg.username && me.as_ref().is_some_and(|m| m.account_tier.is_none() && m.claimed.is_none()) {
        let mut c = Claim { legacy: true, reason, ..Default::default() };
        c.email.focused = true;
        sh.account_gate.claim = Some(c);
        return;
    }
    let mut c = Claim {
        username_mode: cfg.username,
        verify: start_with_code && !cfg.username,
        reason,
        asks_password: existing_email.is_some() || cfg.local_credential,
        // The code step confirms the account's own address.
        email: TextEdit::new(existing_email.as_deref().or(me.as_ref().filter(|_| start_with_code).and_then(|m| m.email.as_deref())).unwrap_or_default()),
        username: TextEdit::new(me.as_ref().map(|m| m.base.username.as_str()).unwrap_or_default()),
        existing_email,
        ..Default::default()
    };
    c.username.focused = cfg.username;
    c.email.focused = !cfg.username && c.existing_email.is_none() && !c.verify;
    c.password.focused = !cfg.username && c.existing_email.is_some();
    c.code.focused = c.verify;
    if c.verify {
        send_code(sh);
        c.sending = true;
    }
    sh.account_gate.claim = Some(c);
}

fn send_code(sh: &mut ShellState) {
    sh.intents.push(Intent::Fetch { key: CLAIM_SEND.into(), method: reqwest::Method::POST, path: "/users/@me/email-verification/send".into(), body: Some(json!({})) });
}

/// `FormUtils.extractErrorMessage`: the API message without the status and code.
fn message(e: &str) -> String {
    let e = e.split_once(": ").map_or(e, |(_, m)| m);
    e.rsplit_once(" (").map_or(e, |(m, _)| m).to_string()
}

fn resend_at(v: &Value, now: u64) -> u64 {
    v["resend_available_at"].as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map_or(now + 30_000, |d| d.timestamp_millis() as u64)
}

fn finish(sh: &mut ShellState, toast: &str) {
    sh.account_gate.claim = None;
    crate::desktop::toast(&mut sh.desktop, t(toast), true);
}

fn fetch(sh: &mut ShellState, key: &str, method: reqwest::Method, path: &str, body: Value) {
    sh.intents.push(Intent::Fetch { key: key.into(), method, path: path.into(), body: Some(body) });
}

/// The legacy flow's next request after `step` answered `v`.
fn legacy_next(sh: &mut ShellState, step: Step, v: &Value) {
    let now = sh.now_ms;
    let Some(c) = sh.account_gate.claim.as_mut() else { return };
    let s = |k: &str| v[k].as_str().map(str::to_string);
    match step {
        Step::Start => {
            (c.ticket, c.proof) = (s("ticket"), s("original_proof"));
            if v["resend_available_at"].is_string() {
                c.resend_at = resend_at(v, now);
            }
            let body = json!({ "ticket": c.ticket, "new_email": c.email.text.trim(), "original_proof": c.proof, "new_password": c.password.text });
            (c.step, c.busy) = (Step::Request, true);
            fetch(sh, CLAIM, reqwest::Method::POST, "/users/@me/email-change/request-new", body);
        }
        Step::Request => {
            c.resend_at = if v["resend_available_at"].is_string() { resend_at(v, now) } else { 0 };
            (c.verify, c.code, c.error) = (true, TextEdit::default(), None);
            c.code.focused = true;
            crate::desktop::toast(&mut sh.desktop, t("Verification code sent"), true);
        }
        Step::Verify => {
            c.email_token = s("email_token");
            let body = json!({ "email_token": c.email_token, "new_password": c.password.text });
            (c.step, c.busy) = (Step::Apply, true);
            fetch(sh, CLAIM, reqwest::Method::PATCH, "/users/@me", body);
        }
        Step::Apply => finish(sh, "Account claimed successfully"),
        Step::None => {}
    }
}

/// `offerAuthenticatorCalmly`: once per account on this device, unless it has a passkey.
fn offer_authenticator(store: &Store, sh: &mut ShellState) {
    let Some(me) = store.me.as_ref() else { return };
    if sh.prefs.authenticator_offered.contains(&me.base.id) || me.authenticator_types.contains(&reflux_model::AuthenticatorType::WEBAUTHN) {
        return;
    }
    let id = me.base.id;
    crate::shell::expr::image_search::save_prefs(sh, |p| {
        p.authenticator_offered.insert(id);
    });
    sh.account_gate.offer = true;
}

/// Answers to the claim requests.
pub fn poll(store: &Store, sh: &mut ShellState) -> bool {
    let mut changed = false;
    if let Some(reason) = sh.account_gate.open.take() {
        open_claim(store, sh, reason);
        changed = true;
    }
    for key in [CLAIM, CLAIM_SEND] {
        let r = match sh.expr.fetched.remove(key) {
            Some(Fetched::Loading) | None => continue,
            Some(r) => r,
        };
        changed = true;
        let now = sh.now_ms;
        let Some(c) = sh.account_gate.claim.as_mut() else { continue };
        if key == CLAIM_SEND {
            c.sending = false;
        } else {
            c.busy = false;
        }
        match (key, r) {
            (CLAIM_SEND, Fetched::Done(_)) if c.legacy => {
                c.resend_at = now + 30_000;
                crate::desktop::toast(&mut sh.desktop, t("Code resent"), true);
            }
            (CLAIM_SEND, Fetched::Done(v)) => c.resend_at = resend_at(&v, now),
            (_, Fetched::Failed(e)) => {
                let field = if c.verify { "code" } else if c.username_mode { "username" } else { "email" };
                c.error = Some((field, message(&e)));
                c.resent = false;
                c.step = Step::None;
            }
            (_, Fetched::Done(v)) if c.legacy => {
                let step = std::mem::take(&mut c.step);
                legacy_next(sh, step, &v);
            }
            (_, Fetched::Done(_)) if c.username_mode => {
                // `createRecoveryKitWithPasswordAndOpen` when a password was set.
                let password = std::mem::take(&mut c.password.text);
                finish(sh, "Account claimed successfully");
                if !password.is_empty() {
                    sh.account_gate.recovery_kit = true;
                    sh.form.error = None;
                    sh.api(reqwest::Method::POST, "/users/@me/recovery-kit", Some(json!({ "password": password })), crate::guild::Then::Keep("m.kit".into()));
                }
            }
            (_, Fetched::Done(_)) if c.verify => {
                finish(sh, "Email confirmed");
                offer_authenticator(store, sh);
            }
            (_, Fetched::Done(v)) => {
                if v["next_step"].as_str().unwrap_or("none") == "none" {
                    let toast = if c.existing_email.is_none() { "Email added" } else { "Password set" };
                    finish(sh, toast);
                    offer_authenticator(store, sh);
                } else {
                    (c.verify, c.resend_at, c.error) = (true, resend_at(&v, now), None);
                    c.code = TextEdit::default();
                    c.code.focused = true;
                }
            }
            _ => {}
        }
    }
    changed
}

fn submit(sh: &mut ShellState) {
    let Some(c) = sh.account_gate.claim.as_mut() else { return };
    if c.busy {
        return;
    }
    c.error = None;
    if c.legacy {
        let (step, path, method, body) = if !c.verify {
            if c.email.text.trim().is_empty() || c.password.text.is_empty() {
                return;
            }
            match (&c.ticket, &c.proof) {
                (Some(ticket), Some(proof)) => (Step::Request, "/users/@me/email-change/request-new", reqwest::Method::POST, json!({ "ticket": ticket, "new_email": c.email.text.trim(), "original_proof": proof, "new_password": c.password.text })),
                _ => (Step::Start, "/users/@me/email-change/start", reqwest::Method::POST, json!({})),
            }
        } else if let Some(token) = &c.email_token {
            (Step::Apply, "/users/@me", reqwest::Method::PATCH, json!({ "email_token": token, "new_password": c.password.text }))
        } else {
            if c.code.text.trim().is_empty() || c.password.text.is_empty() {
                return;
            }
            (Step::Verify, "/users/@me/email-change/verify-new", reqwest::Method::POST, json!({ "ticket": c.ticket, "code": c.code.text.trim(), "original_proof": c.proof }))
        };
        (c.step, c.busy) = (step, true);
        fetch(sh, CLAIM, method, path, body);
        return;
    }
    let (path, method, body) = if c.username_mode {
        if c.password.text != c.confirm.text {
            c.error = Some(("confirm", t("Passwords do not match").into()));
            return;
        }
        let username = c.username.text.trim().to_string();
        let body = if c.password.text.is_empty() { json!({ "username": username }) } else { json!({ "username": username, "new_password": c.password.text }) };
        ("/users/@me", reqwest::Method::PATCH, body)
    } else if c.verify {
        let code: String = c.code.text.split_whitespace().collect();
        // `isCompleteEmailCode`.
        if code.chars().count() != 6 {
            return;
        }
        ("/users/@me/email-verification/confirm", reqwest::Method::POST, json!({ "code": code }))
    } else {
        let email = c.existing_email.clone().unwrap_or_else(|| c.email.text.trim().to_string());
        let body = if c.asks_password && !c.password.text.is_empty() { json!({ "email": email, "password": c.password.text }) } else { json!({ "email": email }) };
        ("/users/@me/claim", reqwest::Method::POST, body)
    };
    c.busy = true;
    sh.intents.push(Intent::Fetch { key: CLAIM.into(), method, path: path.into(), body: Some(body) });
}

fn claim_mut(sh: &mut ShellState) -> &mut Claim {
    sh.account_gate.claim.get_or_insert_with(Default::default)
}

/// The claim modal (`ClaimAccountModal`).
pub fn claim_modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let c = st.account_gate.claim.as_ref()?;
    let close = move |s: &mut S| acc(s).account_gate.claim = None;
    let desc = |s: String| text(s).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap();
    let err = |f: &str| c.error.as_ref().filter(|(k, _)| *k == f).map(|(_, m)| m.as_str());
    let go = move |s: &mut S| submit(acc(s));
    let mut body = col().w_full().gap(16.0);
    let password = |key: &'static str| Input::new(key, &c.password, move |s: &mut S| -> &mut TextEdit { &mut claim_mut(acc(s)).password }).label(t("Password")).secure().placeholder(&"•".repeat(32)).error(err("password")).el(p);
    let (title, footer) = if c.legacy {
        // `EmailClaimAccountModal`: collect, then verify.
        let cancel = Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p);
        let footer = if c.verify {
            let left = c.resend_at.saturating_sub(st.now_ms).div_ceil(1000);
            let label = if left > 0 { tf("Resend ({resendSecondsRemaining}s)", &[("resendSecondsRemaining", &left.to_string())]) } else { t("Resend").to_string() };
            let resend = Button::new(label).disabled(left > 0 || c.sending || c.busy).key("claim-resend").on_press(move |s: &mut S| {
                let sh = acc(s);
                let Some(c) = sh.account_gate.claim.as_mut() else { return };
                let Some(ticket) = c.ticket.clone() else { return };
                (c.sending, c.error) = (true, None);
                fetch(sh, CLAIM_SEND, reqwest::Method::POST, "/users/@me/email-change/resend-new", json!({ "ticket": ticket }));
            });
            body = body
                .child(desc(t("Enter the code we sent to your email to confirm it. Your password will be set once the code is confirmed.").into()))
                .child(Input::new("claim-code", &c.code, move |s: &mut S| -> &mut TextEdit { &mut claim_mut(acc(s)).code }).label(t("Verification code")).placeholder("XXXX-XXXX").error(err("code")).el(p))
                .child(password("claim-password-2"));
            row().gap(8.0).child(cancel).child(resend.el(p)).child(Button::new(t("Claim account")).submitting(c.busy).key("claim-submit").on_press(go).el(p))
        } else {
            body = body
                .child(desc(t("Claim your account by adding an email and password. We will send a verification code to confirm your email before finishing.").into()))
                .child(Input::new("claim-email", &c.email, move |s: &mut S| -> &mut TextEdit { &mut claim_mut(acc(s)).email }).label(t("Email")).placeholder("you@example.com").error(err("email")).el(p))
                .child(password("claim-password"));
            row().gap(8.0).child(cancel).child(Button::new(t("Send code")).submitting(c.busy).key("claim-submit").on_press(go).el(p))
        };
        (t("Claim your account"), footer)
    } else if c.username_mode {
        body = body
            .children(c.reason.clone().map(desc))
            .child(desc(t("Claim your account by choosing a username and password. You sign in with them, so pick ones you will remember.").into()))
            .child(Input::new("claim-username", &c.username, move |s: &mut S| -> &mut TextEdit { &mut claim_mut(acc(s)).username }).label(t("Username")).error(err("username")).el(p))
            .child(Input::new("claim-password", &c.password, move |s: &mut S| -> &mut TextEdit { &mut claim_mut(acc(s)).password }).label(t("Password")).secure().error(err("password")).el(p))
            .child(Input::new("claim-confirm", &c.confirm, move |s: &mut S| -> &mut TextEdit { &mut claim_mut(acc(s)).confirm }).label(t("Confirm password")).secure().error(err("confirm")).el(p));
        let footer = row()
            .gap(8.0)
            .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
            .child(Button::new(t("Claim account")).submitting(c.busy).key("claim-submit").on_press(go).el(p));
        (t("Claim your account"), footer)
    } else if c.verify {
        let to = c.existing_email.clone().unwrap_or_else(|| c.email.text.trim().to_string());
        let left = c.resend_at.saturating_sub(st.now_ms).div_ceil(1000);
        let resend_label = if left > 0 { tf("Send a new code in {seconds}s", &[("seconds", &left.to_string())]) } else { t("Send a new code").to_string() };
        let resend = Button::new(resend_label).variant(ButtonVariant::Secondary).disabled(left > 0 || c.sending).key("claim-resend").on_press(move |s: &mut S| {
            let sh = acc(s);
            if let Some(c) = sh.account_gate.claim.as_mut() {
                (c.sending, c.resent, c.error) = (true, true, None);
            }
            send_code(sh);
        });
        body = body
            .children(c.reason.clone().map(desc))
            .child(desc(tf("Enter the code we sent to {email}.", &[("email", &to)])))
            .child(Input::new("claim-code", &c.code, move |s: &mut S| -> &mut TextEdit { &mut claim_mut(acc(s)).code }).placeholder("000000").error(err("code")).el(p));
        if c.resent && c.error.is_none() {
            body = body.child(text(t("We sent a new code.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).key("resent"));
        }
        body = body.child(row().w_full().child(resend.el(p)).key("resend"));
        let complete = c.code.text.split_whitespace().collect::<String>().chars().count() == 6;
        let footer = row()
            .gap(8.0)
            .child(Button::new(t("Confirm later")).variant(ButtonVariant::Secondary).on_press(close).el(p))
            .child(Button::new(t("Verify")).submitting(c.busy).disabled(!complete).key("claim-verify").on_press(go).el(p));
        (t("Confirm your email"), footer)
    } else {
        let set_password = c.existing_email.is_some();
        let lead = match (set_password, st.tiers.emails_enabled) {
            (true, _) => "Set a password to sign in on other devices with this email.",
            (false, true) => "Add an email to sign in on other devices. We'll send you a code to confirm it.",
            (false, false) => "Add an email and a password to sign in on other devices.",
        };
        let button = match (set_password, st.tiers.emails_enabled) {
            (true, _) => "Set password",
            (false, true) => "Send code",
            (false, false) => "Add email",
        };
        body = body.child(desc(c.reason.clone().unwrap_or_else(|| t(lead).into()))).child(
            Input::new("claim-email", &c.email, move |s: &mut S| -> &mut TextEdit {
                let c = claim_mut(acc(s));
                // `readOnly` when the account already has its email.
                if c.existing_email.is_some() {
                    c.email = TextEdit::new(c.existing_email.as_deref().unwrap_or_default());
                }
                &mut c.email
            })
            .label(t("Email"))
            .placeholder("you@example.com")
            .error(err("email"))
            .el(p),
        );
        if c.asks_password {
            body = body.child(Input::new("claim-password", &c.password, move |s: &mut S| -> &mut TextEdit { &mut claim_mut(acc(s)).password }).label(t("Password")).secure().error(err("password")).el(p));
        }
        let footer = row()
            .gap(8.0)
            .child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close).el(p))
            .child(Button::new(t(button)).submitting(c.busy).key("claim-submit").on_press(go).el(p));
        (if set_password { t("Set a password") } else { t("Add an email") }, footer)
    };
    Some(Modal::new(title, close).body(body).footer(footer).el(p))
}

/// The kit made after a username claim (`createRecoveryKitWithPasswordAndOpen`): over the app,
/// or a toast when it can't be made.
pub fn recovery_kit<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    (st.account_gate.recovery_kit && st.settings.is_none()).then(|| crate::settings::recovery_kit::kit(store, st, p, acc))
}

/// Drops the kit with Fluxer's toast when creating it failed.
pub fn poll_kit(sh: &mut ShellState) -> bool {
    if !sh.account_gate.recovery_kit || sh.form.data.contains_key("m.kit") || sh.form.error.is_none() {
        return false;
    }
    sh.account_gate.recovery_kit = false;
    sh.form.error = None;
    crate::desktop::toast(&mut sh.desktop, t("Couldn't create your recovery kit. You can create one later in your account settings."), false);
    true
}

/// `AuthenticatorOfferModal`: Add passkey runs `startPasskeyRegistration` from the Account tab.
pub fn offer_modal<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if !st.account_gate.offer {
        return None;
    }
    let close = move |s: &mut S| acc(s).account_gate.offer = false;
    let add = move |s: &mut S| {
        let sh = acc(s);
        sh.account_gate.offer = false;
        sh.open_settings(Some("account_security"));
        sh.open_account_modal(crate::settings::AccountModal::NamePasskey);
        sh.api(reqwest::Method::POST, "/users/@me/mfa/webauthn/credentials/registration-options", Some(json!({})), crate::guild::Then::Keep("m.passkey_options".into()));
    };
    let body = text(t("Add a passkey to sign in with your fingerprint, face or screen lock. You can also do this later in settings.")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap();
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Not now")).variant(ButtonVariant::Secondary).key("offer-not-now").on_press(close).el(p))
        .child(Button::new(t("Add passkey")).key("offer-add").on_press(add).el(p));
    Some(Modal::new(t("Sign in faster next time"), close).body(col().w_full().child(body)).footer(footer).el(p))
}

// ---- Messaging paused ---------------------------------------------------------------------------

pub const SUPPORT_EMAIL: &str = "support@fluxer.com";

/// `buildMessagingPausedHelpEmail`: the body and the `mailto:` link.
pub fn help_email(me: &User, country: &str) -> (String, String) {
    let subject = t("My text didn't go through").to_string();
    let created = chrono::DateTime::from_timestamp_millis(me.base.id.timestamp_ms() as i64).unwrap_or_default().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let tag = if me.base.discriminator.is_empty() || me.base.discriminator == "0" { me.base.username.clone() } else { format!("{}#{}", me.base.username, me.base.discriminator) };
    let body = [
        subject.clone(),
        String::new(),
        tf("Account ID: {accountId}", &[("accountId", &me.base.id.to_string())]),
        tf("Username: {username}", &[("username", &tag)]),
        tf("Account created: {createdAt}", &[("createdAt", &created)]),
        tf("Country: {country}", &[("country", country.trim())]),
    ]
    .join("\n");
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>().replace('+', "%20");
    let mailto = format!("mailto:{SUPPORT_EMAIL}?subject={}&body={}", enc(&subject), enc(&body));
    (body, mailto)
}

/// `MessagingPausedHelpModal`.
pub fn help_modal<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let country = st.account_gate.help.as_ref()?;
    let me = super::devtools::me(store, st)?;
    let (body_text, mailto) = help_email(&me, &country.text);
    let close = move |s: &mut S| acc(s).account_gate.help = None;
    let body = col()
        .w_full()
        .gap(16.0)
        .child(text(tf("Email {supportEmail} and a person will check your account. You can also reply to the email we sent you.", &[("supportEmail", SUPPORT_EMAIL)])).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap())
        .child(Input::new("help-country", country, move |s: &mut S| -> &mut TextEdit { acc(s).account_gate.help.get_or_insert_with(TextEdit::default) }).label(t("Country you're in")).el(p));
    let copy = format!("{SUPPORT_EMAIL}\n\n{body_text}");
    let footer = row()
        .gap(8.0)
        .child(Button::new(t("Copy details")).variant(ButtonVariant::Secondary).key("help-copy").on_press_tx(move |_: &mut S, tx: &mut rune::Tx| tx.copy(copy.clone())).el(p))
        .child(Button::new(t("Email support")).key("help-mail").on_press(move |_: &mut S| crate::settings::privacy::open_url(&mailto)).el(p));
    Some(Modal::new(t("My text didn't go through"), close).body(body).footer(footer).el(p))
}

/// The claim and help dialogs' state.
#[derive(Default)]
pub struct Ui {
    pub claim: Option<Claim>,
    /// The help dialog's country field, while it's open.
    pub help: Option<TextEdit>,
    /// A button asked for the claim modal with this reason (opened where the store is at hand).
    pub open: Option<Option<String>>,
    /// The mocked barrier, and when it was picked (its countdown's start).
    pub mock_key: String,
    pub mock_since: u64,
    /// The unclaimed account nagbar was dismissed this session.
    pub unclaimed_dismissed: bool,
    /// The voice snapshot found at startup (`None` until the account is ready), and whether
    /// this session has been connected to voice.
    pub voice_startup: Option<Option<crate::desktop::top_bars::VoiceSnapshot>>,
    pub was_connected: bool,
    /// A nagbar's confirmation modal.
    pub bar_confirm: Option<crate::desktop::top_bars::Confirm>,
    /// The recovery kit made right after a username claim, shown over the app.
    pub recovery_kit: bool,
    /// `AuthenticatorOfferModal` is up.
    pub offer: bool,
    /// `BouncedEmailChangeModal`'s fields, while it's open.
    pub bounced: Option<crate::shell::bounced::Ui>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(tier: Option<&str>, email: Option<&str>, verified: bool) -> User {
        User { account_tier: tier.map(str::to_owned), email: email.map(str::to_owned), verified, ..Default::default() }
    }

    #[test]
    fn tiers_and_needs() {
        let cfg = Cfg { emails_enabled: true, delivery_on: true, ..Default::default() };
        assert_eq!(tier(&user(None, None, false), &cfg), Tier::Unclaimed);
        assert_eq!(tier(&user(None, Some("a@b"), false), &cfg), Tier::Unverified);
        assert_eq!(tier(&user(None, Some("a@b"), true), &cfg), Tier::Verified);
        assert_eq!(tier(&user(Some("unclaimed"), Some("a@b"), true), &cfg), Tier::Unclaimed, "the server's tier wins");
        assert_eq!(need(&user(None, None, false), &cfg), Some(Need::AddEmail));
        assert_eq!(need(&user(Some("unclaimed"), Some("a@b"), false), &cfg), Some(Need::SetPassword), "unclaimed with an email needs a password");
        let username = Cfg { username: true, ..cfg.clone() };
        assert_eq!(need(&user(Some("unclaimed"), None, false), &username), Some(Need::SetPassword));
        assert_eq!(need(&user(Some("verified"), None, false), &cfg), None);
    }

    #[test]
    fn gates_follow_the_interaction_tier() {
        let legacy = Cfg { emails_enabled: true, delivery_on: true, ..Default::default() };
        let enforced = Cfg { enforced: true, ..legacy.clone() };
        let unverified = user(Some("unverified"), Some("a@b"), false);
        assert_eq!(denied(Some(&unverified), Action::Message, &legacy), None, "legacy lets unverified accounts talk");
        assert_eq!(denied(Some(&unverified), Action::Message, &enforced), Some(Need::ConfirmEmail));
        assert_eq!(denied(Some(&unverified), Action::GuildVerification, &legacy), Some(Need::ConfirmEmail));
        assert_eq!(denied(Some(&user(Some("unclaimed"), None, false)), Action::Message, &legacy), Some(Need::AddEmail));
        assert_eq!(denied(None, Action::Message, &legacy), Some(Need::AddEmail));
        assert_eq!(denied(Some(&user(Some("verified"), None, false)), Action::GuildVerification, &enforced), None);
        let (_, _, reason, label) = presentation(Need::AddEmail, Action::GuildVerification, &Cfg::default());
        assert_eq!((reason, label), ("Add an email and a password to talk in this community.", "Add email and password"), "no delivery");
    }

    #[test]
    fn help_email_lists_the_account() {
        let me = User { base: reflux_model::PartialUser { id: reflux_model::Snowflake(1 << 22), username: "ana".into(), ..Default::default() }, ..Default::default() };
        let (body, mailto) = help_email(&me, " Korea ");
        assert!(body.starts_with("My text didn't go through\n\nAccount ID: 4194304\nUsername: ana\nAccount created: "), "{body}");
        assert!(body.ends_with("Country: Korea"));
        assert!(mailto.starts_with("mailto:support@fluxer.com?subject=My%20text%20didn%27t%20go%20through&body="), "{mailto}");
    }

    #[test]
    fn claim_answers_move_the_flow() {
        let mut sh = ShellState::default();
        sh.account_gate.claim = Some(Claim { email: TextEdit::new("a@b.c"), ..Default::default() });
        submit(&mut sh);
        assert!(matches!(sh.intents.last(), Some(Intent::Fetch { path, .. }) if path == "/users/@me/claim"));
        sh.expr.fetched.insert(CLAIM.into(), Fetched::Done(json!({"next_step": "confirm_email", "resend_available_at": "2026-01-01T00:00:30Z"})));
        poll(&Store::default(), &mut sh);
        let c = sh.account_gate.claim.as_ref().unwrap();
        assert!(c.verify && c.resend_at > 0, "the code step");
        sh.account_gate.claim.as_mut().unwrap().code = TextEdit::new("123456");
        submit(&mut sh);
        assert!(matches!(sh.intents.last(), Some(Intent::Fetch { path, body: Some(b), .. }) if path == "/users/@me/email-verification/confirm" && b["code"] == "123456"));
        sh.expr.fetched.insert(CLAIM.into(), Fetched::Failed("400 Bad Request: Invalid code (INVALID_CODE)".into()));
        poll(&Store::default(), &mut sh);
        assert_eq!(sh.account_gate.claim.as_ref().unwrap().error, Some(("code", "Invalid code".into())));
        submit(&mut sh);
        sh.expr.fetched.insert(CLAIM.into(), Fetched::Done(json!({})));
        poll(&Store::default(), &mut sh);
        assert!(sh.account_gate.claim.is_none(), "confirmed");
    }
}
