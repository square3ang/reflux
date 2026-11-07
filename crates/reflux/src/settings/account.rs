//! The Account tab (`tabs/account_security_tab/*`): sign-in details, security (authenticator
//! app, backup codes, passkeys, apps and devices), the danger zone and blocked users, and the
//! dialogs they open (`EmailChangeModal`, `PasswordChangeModal`, `FluxerTagChangeModal`,
//! `MfaTotpEnableModal`, `BackupCodesViewModal`, `AccountDisableModal`, `AccountDeleteModal`, …).
//!
//! Every request is an [`Intent::Api`] call; steps keep their answers in `form.data["m.*"]`, and a
//! call that needs sudo mode opens [`super::sudo`] first.

use reflux_model::{AuthenticatorType, RelationshipType, Snowflake};
use reflux_store::Store;
use reqwest::Method;
use rune::{El, col, el, row};
use crate::ui::mark::marked as text;
use serde_json::{Value, json};

use super::tabs::{not_available, section};
use super::{Op, ShellModal, input};
use crate::guild::Then;
use crate::i18n::t;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Modal, Switch, mix};

/// A dialog over the settings.
#[derive(Clone, Debug, PartialEq)]
pub enum AccountModal {
    Email,
    Password,
    Username,
    /// The Base32 secret this setup generated.
    TotpEnable(String),
    TotpDisable,
    BackupCodes,
    Disable,
    Delete,
    RenamePasskey(String),
    DeletePasskey(String),
    /// `PasskeyNameModal`: names the passkey, then runs the key ceremony.
    NamePasskey,
    /// Sign out these sessions (`id_hash`es).
    SignOutDevices(Vec<String>),
    DeleteMessages,
    AddConnection,
    RemoveConnection { kind: String, id: String },
    Deauthorize(String),
    RecoveryKitReplace,
    RecoveryKit,
    /// Advanced's `ScreenShareEncoderControlsModal` (drawn by [`super::advanced::encoder_modal`]).
    EncoderControls,
    /// Advanced's reverse image search settings, a custom engine to add (`None`) or edit, and
    /// one to remove ([`super::search_engines`]).
    ReverseImageSearch,
    RisEngine(Option<String>),
    RisRemove(String),
    /// A library theme's `ThemeDetailsModal` and delete confirmation, and a watched folder's
    /// "Stop watching" confirmation ([`super::theme_menu`]).
    ThemeDetails(String),
    ThemeDelete(String),
    ThemeStopWatching(String),
    /// Advanced's `SwitchGroupSettingsModal` for this item ([`super::advanced::switch_group_modal`]).
    SwitchGroup(&'static str),
}

pub(super) fn txt(s: impl Into<String>, size: f32, weight: f32, c: rune::Color) -> El {
    let s = s.into();
    crate::ui::mark::text(text(s.clone()).font(font(size, weight).lh(size * 1.4)).color(c).wrap(), &s)
}

/// `AccountSecurityCard`: a bordered card with an optional header (title, description, action)
/// over rows divided by 1px rules.
pub(super) fn card(head: Option<(&str, Option<&str>, Option<El>)>, rows: Vec<El>, p: &Palette) -> El {
    let mut c = col().w_full().radius(12.0).border(1.0, p.background_modifier_accent);
    let has_head = head.is_some();
    if let Some((title, desc, action)) = head {
        let mut words = col().grow(1.0).min_w(0.0).gap(4.0).child(crate::ui::mark::text(text(title.to_string()).font(font(15.0, 600.0).lh(19.5)).color(p.text_primary), title));
        if let Some(d) = desc {
            words = words.child(txt(d, 13.0, 400.0, p.text_tertiary));
        }
        c = c.child(row().w_full().padding(16.0).gap(16.0).center_y().child(words).children(action));
    }
    for (i, r) in rows.into_iter().enumerate() {
        let rule = i > 0 || has_head;
        c = c.child(if rule { r.child(el().pin(Some(0.0), Some(0.0), Some(0.0), None).h(1.0).fill(p.background_modifier_accent)) } else { r });
    }
    c
}

/// `AccountSecurityRow`: label 14/500 and a 13px description, actions on the right.
pub(super) fn card_row(label: &str, desc: Option<El>, actions: Vec<El>, p: &Palette) -> El {
    let mut words = col().grow(1.0).min_w(0.0).gap(2.0).child(crate::ui::mark::text(text(label.to_string()).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary).wrap(), label));
    if let Some(d) = desc {
        words = words.child(d);
    }
    row().w_full().px(16.0).py(12.0).gap(16.0).center_y().child(words).child(row().gap(8.0).children(actions))
}

pub(super) fn muted(s: impl Into<String>, p: &Palette) -> El {
    txt(s, 13.0, 400.0, p.text_primary_muted)
}

pub(super) fn small_button<S: 'static>(label: &str, secondary: bool, key: &str, f: impl Fn(&mut S) + 'static, p: &Palette) -> El {
    Button::new(label.to_string())
        .variant(if secondary { ButtonVariant::Secondary } else { ButtonVariant::Primary })
        .size(ButtonSize::Small)
        .key(key.to_string())
        .on_press(f)
        .el(p)
}

pub(super) fn open<S: 'static>(acc: ShellAccess<S>, m: AccountModal) -> impl Fn(&mut S) + 'static {
    move |s: &mut S| acc(s).open_account_modal(m.clone())
}

/// A date's `YYYY-MM-DD`.
pub(super) fn day(ts: &str) -> String {
    ts.get(..10).unwrap_or(ts).to_string()
}

/// `maskEmail`: every character before the `@` becomes `*`.
fn mask_email(email: &str) -> String {
    match email.split_once('@') {
        Some((user, domain)) => format!("{}@{domain}", "*".repeat(user.chars().count())),
        None => "*".repeat(email.chars().count()),
    }
}

// ---- The tab ---------------------------------------------------------------------------------

pub(super) fn tab<S: 'static>(store: &Store, st: &ShellState, ui: &super::SettingsUi, p: &Palette, acc: ShellAccess<S>) -> El {
    let Some(me) = store.me.as_ref() else { return not_available(p) };
    let set = move |s: &mut S| {
        if let Some(u) = acc(s).settings.as_mut() {
            u.reveal_email = !u.reveal_email;
        }
    };
    // Sign-in details.
    let email_desc = match &me.email {
        Some(e) => row()
            .gap(8.0)
            .center_y()
            .child(muted(if ui.reveal_email { e.clone() } else { mask_email(e) }, p))
            .child(txt(if ui.reveal_email { t("Hide") } else { t("Reveal") }, 13.0, 400.0, p.text_link).cursor(rune::Cursor::Pointer).role("button").on_press(set)),
        None => row().child(txt(t("No email address set"), 13.0, 400.0, p.alert_warning_color)),
    };
    let has_password = me.has_password != Some(false);
    let password = match (&me.password_last_changed_at, has_password) {
        (_, false) => t("No password set").to_string(),
        (Some(ts), _) => format!("{} {}", t("Last changed:"), day(&ts.to_string())),
        (None, _) => t("Last changed: never").to_string(),
    };
    let email_label = if me.email.is_some() { t("Change email") } else { t("Add email") };
    let sign_in = card(
        None,
        vec![
            card_row(t("Username"), Some(muted(crate::shell::friends::tag(store, me.base.id), p)), vec![small_button(t("Change username"), true, "acct-username", open(acc, AccountModal::Username), p)], p),
            // `usesUsernameSignIn`: accounts have no email address.
            card_row(t("Email address"), Some(email_desc), vec![small_button(email_label, true, "acct-email", open(acc, AccountModal::Email), p)], p),
            card_row(
                t("Password"),
                Some(muted(password, p)),
                vec![small_button(if has_password { t("Change password") } else { t("Set password") }, true, "acct-password", open(acc, AccountModal::Password), p)],
                p,
            ),
        ]
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !(st.username_sign_in && *i == 1))
        .map(|(_, r)| r)
        .chain(super::recovery_kit::shown(store, st).then(|| super::recovery_kit::row_el(st, p, acc)))
        .collect(),
        p,
    );
    let sign_in = match super::recovery_kit::shown(store, st).then(|| super::recovery_kit::reminder(st, p, acc)).flatten() {
        Some(alert) => col().w_full().child(alert).child(sign_in),
        None => sign_in,
    };

    // Security.
    let totp = me.authenticator_types.contains(&AuthenticatorType::TOTP);
    let passkey_mfa = me.authenticator_types.contains(&AuthenticatorType::WEBAUTHN);
    let unverified = !me.verified && me.email.is_some();
    let mut two_fa = Vec::new();
    if unverified {
        two_fa.push(card_row(t("Confirm your email to turn on two-factor authentication or add a passkey."), None, vec![], p));
    }
    let totp_desc = if totp { t("Codes from your authenticator app are your second step when you sign in.") } else { t("Use an authenticator app to generate codes for two-factor authentication") };
    let totp_button = if totp {
        small_button(t("Turn off"), true, "acct-totp-off", open(acc, AccountModal::TotpDisable), p)
    } else {
        small_button(t("Set up"), false, "acct-totp-on", move |s: &mut S| acc(s).open_account_modal(AccountModal::TotpEnable(totp_secret())), p)
    };
    two_fa.push(card_row(t("Authenticator app"), Some(muted(totp_desc, p)), vec![totp_button], p));
    if totp || passkey_mfa {
        let view = move |s: &mut S| {
            let sh = acc(s);
            sh.open_account_modal(AccountModal::BackupCodes);
            sh.api(Method::POST, "/users/@me/mfa/backup-codes", Some(json!({ "regenerate": false })), Then::Keep("m.codes".into()));
        };
        two_fa.push(card_row(t("Backup codes"), Some(muted(t("View and manage your backup codes for account recovery"), p)), vec![small_button(t("View codes"), true, "acct-codes", view, p)], p));
    }
    let two_fa = card(Some((t("Two-factor authentication"), Some(t("Ask for a second step after your password when you sign in.")), None)), two_fa, p);

    let passkeys: Vec<Value> = st.form.data.get("passkeys").and_then(Value::as_array).cloned().unwrap_or_default();
    let add = Button::new(t("Add passkey"))
        .variant(if passkeys.is_empty() { ButtonVariant::Primary } else { ButtonVariant::Secondary })
        .size(ButtonSize::Small)
        .disabled(unverified)
        .key("acct-passkey-add")
        // `startPasskeyRegistration`: the options (sudo first, if the server wants it), then
        // `PasskeyNameModal`.
        .on_press(move |s: &mut S| {
            let sh = acc(s);
            sh.open_account_modal(AccountModal::NamePasskey);
            sh.api(Method::POST, "/users/@me/mfa/webauthn/credentials/registration-options", Some(json!({})), Then::Keep("m.passkey_options".into()));
        })
        .el(p);
    let mut rows = Vec::new();
    if passkeys.is_empty() && !unverified {
        rows.push(row().w_full().px(16.0).py(12.0).child(muted(t("No passkeys yet. Add one to sign in with your fingerprint, face or a security key."), p)));
    }
    for k in &passkeys {
        let id = k["id"].as_str().unwrap_or_default().to_string();
        let name = k["name"].as_str().unwrap_or_default();
        let created = day(k["created_at"].as_str().unwrap_or_default());
        let desc = match k["last_used_at"].as_str() {
            Some(u) => crate::i18n::tf("Added {createdDate}, last used {lastUsedDate}", &[("createdDate", &created.to_string()), ("lastUsedDate", &(day(u)).to_string())]),
            None => crate::i18n::tf("Added {createdDate}, never used", &[("createdDate", &created.to_string())]),
        };
        rows.push(card_row(
            name,
            Some(muted(desc, p)),
            vec![
                small_button(t("Rename"), true, &format!("passkey-rename-{id}"), open(acc, AccountModal::RenamePasskey(id.clone())), p),
                Button::new(t("Delete")).variant(ButtonVariant::Ghost).size(ButtonSize::Small).on_press(open(acc, AccountModal::DeletePasskey(id))).el(p),
            ],
            p,
        ));
    }
    if !passkeys.is_empty() {
        let toggle = move |s: &mut S| acc(s).api(Method::PUT, "/users/@me/mfa/webauthn/two-factor", Some(json!({ "enabled": !passkey_mfa })), Then::Nothing);
        rows.push(
            row().w_full().px(16.0).py(12.0).child(
                Switch::new(passkey_mfa)
                    .label(t("Require a passkey as your second factor"))
                    .description(t("Ask for a passkey after your password when you sign in"))
                    .disabled(unverified && !passkey_mfa)
                    .on_toggle(toggle)
                    .el(p),
            ),
        );
    }
    let passkeys = card(Some((t("Passkeys"), Some(t("Use passkeys to sign in without a password")), Some(add))), rows, p);
    let goto = move |id: &'static str| move |s: &mut S| super::open_tab(acc(s), id);
    let apps = card(
        Some((t("Apps and devices"), Some(t("Manage apps and devices with access to your account")), None)),
        vec![
            card_row(t("Authorized apps"), Some(muted(t("Review apps that can access your account."), p)), vec![small_button(t("Manage"), true, "acct-apps", goto("authorized_apps"), p)], p),
            card_row(t("Devices"), Some(muted(t("Review signed-in devices and sign out of sessions you don't recognize."), p)), vec![small_button(t("Manage"), true, "acct-devices", goto("devices"), p)], p),
        ],
        p,
    );
    let mut security = col().w_full().gap(16.0).child(two_fa).child(passkeys).child(apps);
    if let Some(e) = &st.form.error
        && ui.modal.is_none()
    {
        security = security.child(txt(e.clone(), 14.0, 400.0, mix(p.status_danger, p.text_primary, 78.0)));
    }

    // Danger zone.
    let danger = card(
        Some((t("Disable or delete your account"), None, None)),
        vec![
            card_row(t("Disable account"), Some(muted(t("Temporarily disable your account. You can reactivate it later by signing back in."), p)), vec![small_button(t("Disable"), true, "acct-disable", open(acc, AccountModal::Disable), p)], p),
            card_row(
                t("Delete account"),
                Some(muted(t("Permanently delete your account and all associated data. This action cannot be undone."), p)),
                vec![Button::new(t("Delete")).variant(ButtonVariant::Danger).size(ButtonSize::Small).key("acct-delete").on_press(open(acc, AccountModal::Delete)).el(p)],
                p,
            ),
        ],
        p,
    );

    col()
        .w_full()
        .child(section(t("Sign-in details"), sign_in, true, p))
        .child(section(t("Security"), security, false, p))
        .child(section(t("Danger zone"), danger, false, p))
        .child(section(t("Blocked users"), blocked(store, st, p, acc), false, p))
}

/// `BlockedUsersTab`: each blocked user with Unblock.
pub(super) fn blocked<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> El {
    let mut ids: Vec<Snowflake> = store.relationships.values().filter(|r| r.kind == RelationshipType::BLOCKED).map(|r| r.id).collect();
    ids.sort_by_key(|u| store.display_name(None, *u).to_lowercase());
    if ids.is_empty() {
        return col()
            .w_full()
            .py(24.0)
            .gap(4.0)
            .items_center()
            .child(txt(t("No blocked users"), 16.0, 600.0, p.text_primary))
            .child(muted(t("You haven't blocked anyone yet."), p));
    }
    let rows = ids
        .into_iter()
        .map(|u| {
            let name = store.display_name(None, u);
            row()
                .w_full()
                .px(16.0)
                .py(10.0)
                .gap(12.0)
                .center_y()
                .child(crate::shell::avatar(store, st, u, 32.0, None, p.background_secondary, p))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .child(text(name).font(font(14.0, 500.0).lh(18.0)).color(p.text_primary).truncate())
                        .child(text(crate::shell::friends::tag(store, u)).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary).truncate()),
                )
                .child(small_button(t("Unblock"), true, &format!("unblock-{u}"), move |s: &mut S| acc(s).intents.push(Intent::RemoveRelationship(u)), p))
        })
        .collect();
    card(None, rows, p)
}

// ---- TOTP ------------------------------------------------------------------------------------

/// A new 160-bit Base32 TOTP secret (`users/mfa.mdx`: the client generates it).
pub fn totp_secret() -> String {
    let bytes: Vec<u8> = (0..20).map(|_| fastrand::u8(..)).collect();
    base32(&bytes)
}

/// RFC 4648 Base32 without padding.
pub fn base32(bytes: &[u8]) -> String {
    const A: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0);
    for &b in bytes {
        buf = buf << 8 | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(A[(buf >> bits & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(A[(buf << (5 - bits) & 31) as usize] as char);
    }
    out
}

/// The `otpauth://` URI authenticator apps scan.
pub fn otpauth_uri(secret: &str, account: &str, issuer: &str) -> String {
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>().replace('+', "%20");
    format!("otpauth://totp/{}:{}?secret={secret}&issuer={}", enc(issuer), enc(account), enc(issuer))
}

/// The QR code of `data` as a PNG (4-module quiet zone, `scale` px per module).
pub fn qr_png(data: &str, scale: u32) -> Result<Vec<u8>, String> {
    let code = qrcode::QrCode::new(data.as_bytes()).map_err(|e| e.to_string())?;
    let n = code.width() as u32;
    let colors = code.to_colors();
    let size = (n + 8) * scale;
    let img = image::GrayImage::from_fn(size, size, |x, y| {
        let (mx, my) = ((x / scale) as i64 - 4, (y / scale) as i64 - 4);
        let dark = mx >= 0 && my >= 0 && (mx as u32) < n && (my as u32) < n && colors[(my as u32 * n + mx as u32) as usize] == qrcode::Color::Dark;
        image::Luma([if dark { 0 } else { 255 }])
    });
    let mut png = Vec::new();
    image::DynamicImage::ImageLuma8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(png)
}

// ---- Dialogs ---------------------------------------------------------------------------------

fn close<S: 'static>(acc: ShellAccess<S>) -> impl Fn(&mut S) + Clone + 'static {
    move |s: &mut S| acc(s).close_account_modals()
}

fn data<'a>(st: &'a ShellState, key: &str) -> Option<&'a Value> {
    st.form.data.get(key)
}

fn val(st: &ShellState, key: &str) -> String {
    st.form.text.get(key).map(|t| t.text.trim().to_string()).unwrap_or_default()
}

fn desc(s: &str, p: &Palette) -> El {
    txt(s, 14.0, 400.0, p.text_primary_muted)
}

/// A dialog's primary button: (label, danger, action).
pub(super) type Primary<'a, S> = Option<(&'a str, bool, Box<dyn Fn(&mut S)>)>;

/// A dialog: the body, then the error, over Cancel and the primary action (if any).
fn dialog<S: 'static>(st: &ShellState, title: &str, body: Vec<El>, primary: Primary<'_, S>, p: &Palette, acc: ShellAccess<S>) -> El {
    let mut b = col().w_full().gap(16.0).children(body);
    if let Some(e) = &st.form.error {
        b = b.child(txt(e.clone(), 14.0, 400.0, mix(p.status_danger, p.text_primary, 78.0)));
    }
    let mut footer = row().gap(8.0).child(Button::new(t("Cancel")).variant(ButtonVariant::Secondary).on_press(close(acc)).el(p));
    if let Some((label, danger, f)) = primary {
        footer = footer.child(
            Button::new(label.to_string())
                .variant(if danger { ButtonVariant::Danger } else { ButtonVariant::Primary })
                .submitting(st.form.busy)
                .key("modal-primary")
                .on_press(move |s: &mut S| f(s))
                .el(p),
        );
    }
    Modal::new(title.to_string(), close(acc)).body(b).footer(footer).el(p)
}

fn post<S: 'static>(acc: ShellAccess<S>, path: &'static str, body: impl Fn(&ShellState) -> Value + 'static, then: Then) -> Box<dyn Fn(&mut S)> {
    Box::new(move |s: &mut S| {
        let sh = acc(s);
        let b = body(sh);
        sh.api(Method::POST, path, Some(b), then.clone());
    })
}

pub(super) fn modal<S: 'static>(store: &Store, st: &ShellState, m: &AccountModal, p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    let me = store.me.as_ref()?;
    let field = |key: &'static str, label: &str, ph: &str, secure: bool| input(st, acc, key, "", label, ph, secure, p);
    Some(match m {
        AccountModal::Email => {
            let start = data(st, "m.start");
            let ticket = start.and_then(|v| v["ticket"].as_str()).unwrap_or_default().to_string();
            let proof = data(st, "m.orig").or(start).and_then(|v| v["original_proof"].as_str()).map(str::to_string);
            let tk = ticket.clone();
            if start.is_none() {
                dialog(st, t("Change your email"), vec![desc(t("We'll send a verification code to your current email address first."), p)], Some((t("Start"), false, post(acc, "/users/@me/email-change/start", |_| json!({}), Then::Keep("m.start".into())))), p, acc)
            } else if let Some(token) = data(st, "m.token").and_then(|v| v["email_token"].as_str()).map(str::to_string) {
                let apply = post(acc, "/users/@me/email-change/apply", move |_| json!({ "email_token": token }), Then::Close);
                dialog(st, t("Change your email"), vec![desc(t("Your new address is verified. Apply the change to finish."), p)], Some((t("Change email"), false, apply)), p, acc)
            } else if let (Some(proof), Some(new)) = (proof.clone(), data(st, "m.new")) {
                let to = new["new_email"].as_str().unwrap_or_default();
                let verify = post(acc, "/users/@me/email-change/verify-new", move |sh| json!({ "ticket": tk, "code": val(sh, "m.code2"), "original_proof": proof }), Then::Keep("m.token".into()));
                dialog(st, t("Change your email"), vec![desc(&crate::i18n::tf("Enter the code we sent to {email}.", &[("email", to)]), p), field("m.code2", t("Verification code"), "XXXX-XXXX", false)], Some((t("Verify"), false, verify)), p, acc)
            } else if let Some(proof) = proof {
                let req = post(acc, "/users/@me/email-change/request-new", move |sh| json!({ "ticket": tk, "new_email": val(sh, "m.email"), "original_proof": proof }), Then::Keep("m.new".into()));
                dialog(st, t("Change your email"), vec![field("m.email", t("New email address"), "", false)], Some((t("Send code"), false, req)), p, acc)
            } else {
                let to = start.and_then(|v| v["original_email"].as_str()).unwrap_or_default();
                let verify = post(acc, "/users/@me/email-change/verify-original", move |sh| json!({ "ticket": tk, "code": val(sh, "m.code") }), Then::Keep("m.orig".into()));
                dialog(st, t("Change your email"), vec![desc(&crate::i18n::tf("Enter the code we sent to {email}.", &[("email", to)]), p), field("m.code", t("Verification code"), "XXXX-XXXX", false)], Some((t("Verify"), false, verify)), p, acc)
            }
        }
        AccountModal::Password => {
            let setting = me.has_password == Some(false);
            let title = if setting { t("Set a password") } else { t("Update your password") };
            let ticket = data(st, "m.ticket").and_then(|v| v["ticket"].as_str()).unwrap_or_default().to_string();
            match data(st, "m.proof").and_then(|v| v["verification_proof"].as_str()).map(str::to_string) {
                _ if ticket.is_empty() => {
                    let intro = if setting { t("We'll send a verification code to your email before you set a password.") } else { t("We'll send a verification code to your email before you can change your password.") };
                    dialog(st, title, vec![desc(intro, p)], Some((t("Start"), false, post(acc, "/users/@me/password-change/start", |_| json!({}), Then::Keep("m.ticket".into())))), p, acc)
                }
                None => {
                    let verify = post(acc, "/users/@me/password-change/verify", move |sh| json!({ "ticket": ticket, "code": val(sh, "m.code") }), Then::Keep("m.proof".into()));
                    dialog(st, title, vec![desc(t("Enter the code sent to your email address."), p), field("m.code", t("Verification code"), "XXXX-XXXX", false)], Some((t("Verify"), false, verify)), p, acc)
                }
                Some(proof) => {
                    let save = Box::new(move |s: &mut S| {
                        let sh = acc(s);
                        let (new, confirm) = (val(sh, "m.new"), val(sh, "m.confirm"));
                        if new != confirm {
                            sh.form.error = Some(t("Passwords do not match").into());
                            return;
                        }
                        let body = json!({ "ticket": ticket, "verification_proof": proof, "new_password": new });
                        sh.api(Method::POST, "/users/@me/password-change/complete", Some(body), Then::Token);
                    });
                    dialog(st, title, vec![desc(t("Choose a new password."), p), field("m.new", t("New password"), "", true), field("m.confirm", t("Confirm new password"), "", true)], Some((t("Save"), false, save)), p, acc)
                }
            }
        }
        AccountModal::Username => {
            let save = Box::new(move |s: &mut S| {
                let sh = acc(s);
                let mut body = json!({ "username": val(sh, "m.username") });
                let d = val(sh, "m.disc");
                if !d.is_empty() {
                    body["discriminator"] = json!(d);
                }
                sh.api(Method::PATCH, "/users/@me", Some(body), Then::Close);
            });
            let username = input(st, acc, "m.username", &me.base.username, t("Username"), "", false, p);
            let disc = input(st, acc, "m.disc", &me.base.discriminator, t("Tag"), "0000", false, p);
            dialog(st, t("Change your username"), vec![username, disc, desc(t("Changing your tag needs Plutonium unless you change your username too."), p)], Some((t("Save"), false, save)), p, acc)
        }
        AccountModal::TotpEnable(secret) => match data(st, "m.codes") {
            Some(v) => backup_codes(st, v, false, p, acc),
            None => {
                let uri = otpauth_uri(secret, &me.base.username, "Fluxer");
                let png_uri = uri.clone();
                let qr = match st.media.local(&format!("qr:{uri}"), move || qr_png(&png_uri, 4)) {
                    crate::media::Load::Ready(px) => el().size(176.0, 176.0).radius(8.0).pixels_fit(&px, rune::SurfaceFit::Fill),
                    _ => el().size(176.0, 176.0).radius(8.0).fill(p.background_secondary_alt),
                };
                let grouped: Vec<String> = secret.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).into_owned()).collect();
                let sec = secret.clone();
                let enable = post(acc, "/users/@me/mfa/totp/enable", move |sh| json!({ "secret": sec, "code": val(sh, "m.code") }), Then::Keep("m.codes".into()));
                let body = vec![
                    desc(t("Scan the QR code with your authenticator app, or enter the key by hand, then type the 6-digit code it shows."), p),
                    row().w_full().gap(16.0).center_y().child(qr).child(
                        col().grow(1.0).min_w(0.0).gap(4.0).child(txt(t("Key"), 12.0, 600.0, p.text_tertiary)).child(text(grouped.join(" ")).font(font(14.0, 500.0).lh(20.0).mono()).color(p.text_primary).wrap().selectable(true)),
                    ),
                    field("m.code", t("Authenticator code"), t("6-digit code"), false),
                ];
                dialog(st, t("Set up authenticator app"), body, Some((t("Turn on"), false, enable)), p, acc)
            }
        },
        AccountModal::TotpDisable => {
            let off = post(acc, "/users/@me/mfa/totp/disable", |sh| json!({ "code": val(sh, "m.code") }), Then::Close);
            dialog(st, t("Turn off authenticator app"), vec![desc(t("Enter a code from your authenticator app or a backup code to turn it off."), p), field("m.code", t("Authenticator code"), t("6-digit code"), false)], Some((t("Turn off"), true, off)), p, acc)
        }
        AccountModal::BackupCodes => match data(st, "m.codes") {
            Some(v) => backup_codes(st, v, true, p, acc),
            None => dialog(st, t("Backup codes"), vec![desc(t("Loading…"), p)], None, p, acc),
        },
        AccountModal::Disable => {
            let go = post(acc, "/users/@me/disable", |_| json!({}), Then::SignOut);
            dialog(st, t("Disable account"), vec![desc(t("Disabling your account signs you out everywhere. You can reactivate it later by signing back in."), p)], Some((t("Disable account"), true, go)), p, acc)
        }
        AccountModal::Delete => {
            let go = post(acc, "/users/@me/delete", |_| json!({}), Then::SignOut);
            dialog(
                st,
                t("Delete account"),
                vec![desc(t("Your account will be deleted in 14 days. Signing back in before then cancels the deletion. Communities you own must be transferred or deleted first."), p)],
                Some((t("Delete account"), true, go)),
                p,
                acc,
            )
        }
        AccountModal::RenamePasskey(id) => {
            let name = data(st, "passkeys").and_then(Value::as_array).and_then(|a| a.iter().find(|k| k["id"].as_str() == Some(id))).and_then(|k| k["name"].as_str()).unwrap_or_default().to_string();
            let id = id.clone();
            let save = Box::new(move |s: &mut S| {
                let sh = acc(s);
                let body = json!({ "name": val(sh, "m.name") });
                sh.api(Method::PATCH, format!("/users/@me/mfa/webauthn/credentials/{id}"), Some(body), Then::Saved { clear: "m.".into(), reload: Some(("passkeys".into(), "/users/@me/mfa/webauthn/credentials".into())) });
            });
            dialog(st, t("Rename passkey"), vec![input(st, acc, "m.name", &name, t("Name"), "", false, p)], Some((t("Save"), false, save)), p, acc)
        }
        AccountModal::NamePasskey => {
            let ready = data(st, "m.passkey_options").is_some();
            let save = Box::new(move |s: &mut S| {
                let sh = acc(s);
                if data(sh, "m.passkey_options").is_some() && !sh.form.busy {
                    sh.intents.push(Intent::Account(Op::AddPasskey));
                }
            });
            let mut body = vec![input(st, acc, "m.name", "", t("Passkey name"), t("e.g., YubiKey, iPhone, work computer"), false, p)];
            if let Some((_, wrong)) = &st.account.passkey_pin {
                let send = move |s: &mut S| {
                    let sh = acc(s);
                    let pin = sh.form.text.remove("m.pin").map(|e| e.text.clone()).unwrap_or_default();
                    if let Some((reply, _)) = sh.account.passkey_pin.take() {
                        let _ = reply.send(authenticator::Pin::new(&pin));
                    }
                };
                let edit = st.form.text.get("m.pin").cloned().unwrap_or_default();
                let mut pin = crate::ui::Input::new("form-m.pin", &edit, move |s: &mut S| acc(s).form.text.entry("m.pin".into()).or_default()).label(t("Security key PIN")).secure();
                if *wrong {
                    pin = pin.error(Some(t("Wrong PIN. Try again.")));
                }
                body.push(pin.el(p));
                body.push(Button::new(t("Continue")).key("passkey-pin-submit").on_press(send).el(p));
            } else if st.form.busy && ready {
                body.push(desc(t("Touch your security key to continue."), p));
            }
            dialog(st, t("Name passkey"), body, Some((t("Save"), false, save)), p, acc)
        }
        AccountModal::DeletePasskey(id) => {
            let id = id.clone();
            let go = Box::new(move |s: &mut S| {
                acc(s).api(Method::DELETE, format!("/users/@me/mfa/webauthn/credentials/{id}"), None, Then::Saved { clear: "m.".into(), reload: Some(("passkeys".into(), "/users/@me/mfa/webauthn/credentials".into())) })
            });
            dialog(st, t("Delete passkey"), vec![desc(t("Are you sure you want to delete this passkey?"), p)], Some((t("Delete passkey"), true, go)), p, acc)
        }
        AccountModal::EncoderControls
        | AccountModal::ReverseImageSearch
        | AccountModal::RisEngine(_)
        | AccountModal::RisRemove(_)
        | AccountModal::ThemeDetails(_)
        | AccountModal::ThemeDelete(_)
        | AccountModal::ThemeStopWatching(_) => return None,
        AccountModal::RecoveryKitReplace | AccountModal::RecoveryKit => return super::recovery_kit::modal(store, st, m, p, acc),
        other => return super::privacy::modal(store, st, other, p, acc),
    })
}

/// `BackupCodesModal`: the ten codes in two columns, used ones struck through, and Regenerate.
fn backup_codes<S: 'static>(st: &ShellState, v: &Value, regenerate: bool, p: &Palette, acc: ShellAccess<S>) -> El {
    let codes = v["backup_codes"].as_array().cloned().unwrap_or_default();
    let grid = row().w_full().flex_wrap().gaps(8.0, 8.0).children(codes.iter().map(|c| {
        let used = c["consumed"].as_bool().unwrap_or(false);
        let code = c["code"].as_str().unwrap_or_default().to_string();
        let n = code.len();
        let mut s = text(code).font(font(14.0, 500.0).lh(20.0).mono()).color(if used { p.text_tertiary } else { p.text_primary });
        if used {
            s = s.strike(0, n, None);
        }
        row().w(180.0).py(8.0).px(12.0).radius(6.0).fill(p.background_tertiary).child(s.selectable(true))
    }));
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(desc(t("Keep these codes somewhere safe. Each one works once if you lose access to your authenticator app or passkeys."), p))
        .child(grid);
    if let Some(e) = &st.form.error {
        body = body.child(txt(e.clone(), 14.0, 400.0, mix(p.status_danger, p.text_primary, 78.0)));
    }
    let mut footer = row().gap(8.0);
    if regenerate {
        footer = footer.child(
            Button::new(t("Regenerate codes"))
                .variant(ButtonVariant::Secondary)
                .submitting(st.form.busy)
                .on_press(move |s: &mut S| acc(s).api(Method::POST, "/users/@me/mfa/backup-codes", Some(json!({ "regenerate": true })), Then::Keep("m.codes".into())))
                .el(p),
        );
    }
    footer = footer.child(Button::new(t("Done")).key("codes-done").on_press(close(acc)).el(p));
    Modal::new(t("Backup codes"), close(acc)).body(body).footer(footer).el(p)
}

// ---- Shell-level modals ------------------------------------------------------------------------

/// The user-area and profile modals that live outside settings.
pub fn view_shell_modals<S: 'static>(store: &Store, st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    match st.account.modal {
        Some(ShellModal::CustomStatus) => crate::shell::status::custom_status_modal(store, st, p, acc, layer),
        Some(ShellModal::Accounts) => crate::shell::status::accounts_modal(store, st, p, acc, layer),
        Some(ShellModal::Nickname(g)) => crate::shell::status::nickname_modal(store, st, g, p, acc, layer),
        None => {}
    }
    if let Some((user, guild, tab)) = st.account.profile_modal {
        layer.push(crate::shell::profile::modal(store, st, user, guild, tab, p, acc));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_and_otpauth() {
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32(b"f"), "MY");
        assert_eq!(totp_secret().len(), 32);
        assert_eq!(otpauth_uri("ABC", "ada lovelace", "Fluxer"), "otpauth://totp/Fluxer:ada%20lovelace?secret=ABC&issuer=Fluxer");
        let png = qr_png("otpauth://totp/Fluxer:x?secret=ABC", 2).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn mask_email_hides_the_local_part() {
        assert_eq!(mask_email("ada@fluxer.app"), "***@fluxer.app");
        assert_eq!(mask_email("x"), "*");
    }
}
