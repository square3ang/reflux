//! The login screen: `AuthLayout` card with `AuthBrandPanel` and the login method / credential
//! steps (`fluxer_app/src/features/auth`). Numbers come from `assets/reference/login-layout.json`.

use std::f32::consts::PI;

use rune::{Align, Color, Cx, El, Icon, Shadow, TextEdit, col, el, icon, row, text};

use crate::State;
use crate::i18n::t;
use crate::icons;
use crate::theme::{self, Palette, font};
use crate::ui::{Button, ButtonVariant, Input, Popups, Select, mix, over, rgb, rgba};

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Step {
    #[default]
    Method,
    Credentials,
    /// Instance picker: a host or URL to discover.
    Instance,
    Mfa(Mfa),
    /// "Check your email" while the new-IP sign-in waits for the link.
    IpAuth(IpAuth),
    Register,
    /// Request a reset link; `sent` once the instance accepted it.
    Forgot { sent: bool },
    /// Waiting on a security key, for a password-less sign-in or as the second factor.
    Passkey(Passkey),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Passkey {
    /// The MFA step this came from; `None` for the password-less sign-in.
    pub mfa: Option<Mfa>,
    pub prompt: Prompt,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prompt {
    Touch,
    SelectDevice,
    Pin { wrong: bool, attempts: Option<u8> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mfa {
    pub ticket: String,
    pub totp: bool,
    pub backup_codes: bool,
    pub webauthn: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IpAuth {
    pub ticket: String,
    pub email: String,
    /// The poll task stops once this step (the only strong holder) is gone.
    pub alive: std::sync::Arc<()>,
}

#[derive(Default)]
pub struct Login {
    pub step: Step,
    pub email: TextEdit,
    pub password: TextEdit,
    pub show_password: bool,
    pub submitting: bool,
    /// Submitting for a while: the captcha is being solved.
    pub slow: bool,
    /// Counts submissions, so a late "slow" timer can't mark a newer one.
    pub attempt: u64,
    pub instance: TextEdit,
    pub discovering: bool,
    /// TOTP or backup code.
    pub code: TextEdit,
    pub errors: Errors,
    pub register: Register,
    /// Security key PIN, and where to send it while the key waits.
    pub pin: TextEdit,
    pub pin_reply: Option<std::sync::mpsc::Sender<authenticator::Pin>>,
    pub passkey_cancel: Option<crate::passkey::Cancel>,
    /// The date of birth selects.
    pub popups: Popups,
}

/// Register form fields besides `Login::email` / `password`, which it shares.
#[derive(Default)]
pub struct Register {
    pub global_name: TextEdit,
    pub username: TextEdit,
    pub confirm: TextEdit,
    pub show_confirm: bool,
    /// Date of birth parts as the selects' values ("1".."12", "1".."31", "1990").
    pub month: Option<String>,
    pub day: Option<String>,
    pub year: Option<String>,
    /// The account waits for an admin's approval.
    pub pending: bool,
}

/// Messages under the inputs, and one for the whole form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Errors {
    pub email: Option<String>,
    pub password: Option<String>,
    pub code: Option<String>,
    pub instance: Option<String>,
    pub global_name: Option<String>,
    pub username: Option<String>,
    pub confirm: Option<String>,
    pub date_of_birth: Option<String>,
    pub form: Option<String>,
}

impl Errors {
    /// Field errors go under their input (by `errors[].path`), the rest to the form.
    pub fn from_error(e: &reflux_net::Error) -> Self {
        let mut out = Self::default();
        let reflux_net::Error::Api { error, .. } = e else {
            out.form = Some(format!("Something went wrong: {e}"));
            return out;
        };
        for v in &error.errors {
            let slot = match v.path.as_str() {
                "login" | "email" => &mut out.email,
                "password" => &mut out.password,
                "code" | "ticket" => &mut out.code,
                "global_name" => &mut out.global_name,
                "username" => &mut out.username,
                "date_of_birth" => &mut out.date_of_birth,
                _ => &mut out.form,
            };
            slot.get_or_insert_with(|| v.message.clone());
        }
        if error.errors.is_empty() {
            out.form = Some(match error.retry_after {
                Some(secs) if error.code == "RATE_LIMITED" => format!("{} Try again in {}s.", error.message, secs.ceil()),
                _ => error.message.clone(),
            });
        }
        out
    }
}

/// The white waves of `fluxer-logo-color.svg`; the blue disc is drawn as a circle under them.
static LOGO_WAVES: Icon = Icon { name: "fluxer-logo-waves", view: 512.0, body: include_str!("logo_wave.svg") };
/// `fluxer-wordmark.svg` (1374×366) in a square view box: drawn 120.1 wide, its top 32px hold it.
static WORDMARK: Icon =
    Icon { name: "fluxer-wordmark", view: 1374.0, body: include_str!("../../../assets/media/images/fluxer-wordmark.svg") };

/// `SpaceBackground` `::before` stars: (vw, vh, spread, rgb, alpha) on a 1px dot.
const SMALL_STARS: [(f32, f32, f32, [u8; 3], f32); 28] = [
    (4.0, 12.0, 0.0, [255, 255, 255], 0.36), (6.0, 8.0, 1.0, [238, 232, 255], 0.42), (9.0, 68.0, 0.0, [213, 202, 255], 0.34),
    (11.0, 88.0, 0.0, [255, 255, 255], 0.3), (14.0, 28.0, 0.0, [226, 218, 255], 0.38), (18.0, 58.0, 1.0, [255, 255, 255], 0.28),
    (21.0, 15.0, 0.0, [255, 255, 255], 0.36), (24.0, 34.0, 0.0, [211, 199, 255], 0.28), (26.0, 53.0, 0.0, [255, 255, 255], 0.3),
    (29.0, 74.0, 1.0, [237, 231, 255], 0.34), (33.0, 7.0, 0.0, [255, 255, 255], 0.34), (35.0, 38.0, 0.0, [226, 218, 255], 0.38),
    (39.0, 66.0, 0.0, [255, 255, 255], 0.28), (42.0, 9.0, 1.0, [255, 255, 255], 0.36), (46.0, 48.0, 0.0, [211, 199, 255], 0.28),
    (48.0, 82.0, 0.0, [255, 255, 255], 0.3), (53.0, 24.0, 0.0, [226, 218, 255], 0.34), (58.0, 92.0, 0.0, [211, 199, 255], 0.3),
    (61.0, 62.0, 1.0, [255, 255, 255], 0.34), (64.0, 31.0, 0.0, [211, 199, 255], 0.26), (68.0, 18.0, 0.0, [255, 255, 255], 0.32),
    (71.0, 86.0, 0.0, [255, 255, 255], 0.28), (73.0, 47.0, 0.0, [226, 218, 255], 0.34), (81.0, 11.0, 1.0, [255, 255, 255], 0.38),
    (87.0, 78.0, 0.0, [255, 255, 255], 0.34), (91.0, 64.0, 0.0, [255, 255, 255], 0.34), (94.0, 35.0, 0.0, [226, 218, 255], 0.3),
    (97.0, 92.0, 1.0, [255, 255, 255], 0.28),
];
/// `::after` stars, on a 2px dot.
const BIG_STARS: [(f32, f32, f32, [u8; 3], f32); 15] = [
    (7.0, 36.0, 0.0, [226, 218, 255], 0.3), (9.0, 42.0, 1.0, [226, 218, 255], 0.34), (16.0, 80.0, 0.0, [255, 255, 255], 0.24),
    (19.0, 66.0, 0.0, [255, 255, 255], 0.28), (27.0, 24.0, 1.0, [213, 202, 255], 0.26), (32.0, 20.0, 0.0, [226, 218, 255], 0.34),
    (37.0, 90.0, 0.0, [255, 255, 255], 0.24), (45.0, 70.0, 1.0, [255, 255, 255], 0.26), (51.0, 14.0, 0.0, [237, 231, 255], 0.28),
    (57.0, 33.0, 0.0, [226, 218, 255], 0.32), (66.0, 52.0, 1.0, [213, 202, 255], 0.24), (72.0, 86.0, 0.0, [255, 255, 255], 0.3),
    (84.0, 25.0, 0.0, [226, 218, 255], 0.32), (89.0, 46.0, 1.0, [237, 231, 255], 0.26), (96.0, 56.0, 0.0, [255, 255, 255], 0.26),
];

fn stars(size: [f32; 2]) -> impl Iterator<Item = El> {
    let dots = |list: &'static [(f32, f32, f32, [u8; 3], f32)], dot: f32, glow: f32| {
        list.iter().map(move |&(vw, vh, spread, [r, g, b], a)| {
            let d = dot + 2.0 * spread;
            el().absolute(size[0] * vw / 100.0 - spread, size[1] * vh / 100.0 - spread)
                .size(d, d)
                .radius(d / 2.0)
                .fill(rgba(r, g, b, a))
                // filter: drop-shadow(0 0 glow rgb(210 199 255 / …))
                .box_shadow(Shadow { x: 0.0, y: 0.0, blur: glow, spread: 0.0, color: rgba(210, 199, 255, 0.2), inset: false })
        })
    };
    dots(&SMALL_STARS, 1.0, 2.88).chain(dots(&BIG_STARS, 2.0, 4.8))
}

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let p = theme::palette(theme::Kind::Dark);
    let size = cx.layout_size("login-root").unwrap_or([0.0, 0.0]);
    let mut layer = Vec::new();
    // The register form is taller than small windows: the page scrolls, the card stays centred.
    let page = col()
        .key("login-scroll")
        .w_full()
        .h_full()
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .child(col().w_full().min_h(size[1]).py(24.0).center().child(card(s, p, &mut layer)));
    let root = col()
        .key("login-root")
        .w_full()
        .h_full()
        .fill(rgb(8, 6, 22))
        .linear_gradient(145.0 * PI / 180.0, [(0.0, rgb(5, 4, 13)), (0.46, rgb(8, 6, 22)), (1.0, rgb(13, 10, 28))])
        .children(stars(size))
        .child(crate::ui::scroller(page, false, p));
    rune_kit::root(root, layer)
}

fn card(s: &State, p: &Palette, layer: &mut Vec<El>) -> El {
    let bg = p.background_secondary;
    // Rune's border takes no layout space: pad by the CSS border's 1px.
    row()
        .w(800.0)
        .min_h(448.0)
        .padding(1.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(rgba(255, 255, 255, 0.08), bg))
        .box_shadow(Shadow { x: 0.0, y: 20.0, blur: 64.0, spread: -24.0, color: rgba(0, 0, 0, 0.7), inset: false })
        .child(brand(p))
        .child(
            col().grow(1.0).padding(40.0).justify_center().child(match &s.login.step {
                Step::Method => method_step(s, p),
                Step::Credentials => credential_step(s, p),
                Step::Instance => instance_step(s, p),
                Step::Mfa(m) => mfa_step(s, m, p),
                Step::IpAuth(ip) => ip_step(ip, p),
                Step::Register => register_step(s, p, layer),
                Step::Forgot { sent } => forgot_step(s, *sent, p),
                Step::Passkey(k) => passkey_step(s, k, p),
            }),
        )
}

fn brand(p: &Palette) -> El {
    let logo = el()
        .size(128.0, 128.0)
        .radius(64.0)
        .fill(rgb(0x46, 0x41, 0xD9))
        .child(icon(&LOGO_WAVES, 128.0).color(Color::WHITE).absolute(0.0, 0.0));
    let wordmark = el().size(120.1, 32.0).child(icon(&WORDMARK, 120.1).color(p.text_primary).absolute(0.0, 0.0));
    row().w(288.0).child(
        col().grow(1.0).py(40.0).px(24.0).center().child(col().gap(24.0).items_center().child(logo).child(wordmark)),
    )
    .child(el().w(1.0).self_stretch().fill(over(p.background_modifier_accent, p.background_secondary)))
    .a11y_hidden()
}

fn title(t: &str, p: &Palette) -> El {
    // `.title`: 20px/600, letter-spacing .025em, margin-bottom 20.
    text(t).w_full().align(Align::Center).font(font(20.0, 600.0).lh(30.0).tracking(0.5)).color(p.text_primary).mb(20.0)
}

fn method_step(s: &State, p: &Palette) -> El {
    let disabled = s.login.submitting;
    col()
        .w_full()
        .child(title(t("Welcome back"), p))
        .child(
            col()
                .w_full()
                .gap(10.0)
                .child(
                    Button::new(t("Sign in with email"))
                        .icon(&icons::ENVELOPE_SIMPLE)
                        .fit_container()
                        .disabled(disabled)
                        .key("login-email")
                        .on_press(|s: &mut State| {
                            s.login.step = Step::Credentials;
                            s.login.email.focused = true;
                        })
                        .el(p),
                )
                .child(
                    Button::<State>::new(t("Sign in with a passkey"))
                        .variant(ButtonVariant::Secondary)
                        .icon(&icons::KEY)
                        .fit_container()
                        .disabled(disabled)
                        .key("login-passkey")
                        .on_press(|s: &mut State| crate::app::start_passkey(s, None))
                        .el(p),
                )
                .child(
                    Button::<State>::new(t("Register an account"))
                        .variant(ButtonVariant::Secondary)
                        .icon(&icons::USER_PLUS)
                        .icon_gap(8.0)
                        .fit_container()
                        .key("login-register")
                        .on_press(to_register)
                        .el(p)
                        .role("link"),
                )
                // Reflux-only: reuse the official desktop app's session on this machine.
                .children(crate::session::fluxer_desktop_db().filter(|db| db.exists()).map(|_| {
                    Button::<State>::new("Import session from Fluxer desktop")
                        .variant(ButtonVariant::Secondary)
                        .icon(&icons::DOWNLOAD_SIMPLE)
                        .fit_container()
                        .disabled(disabled)
                        .key("login-import")
                        .on_press(crate::app::import_session)
                        .el(p)
                })),
        )
        .children(form_note(&s.login, p).map(|n| n.mt(12.0).align(Align::Center)))
        .child(instance_line(s, p))
}

/// "Signing in to {host} · Change", in place of the app's instance picker row.
fn instance_line(s: &State, p: &Palette) -> El {
    let host = if s.app.saved.instance.is_empty() { crate::session::DEFAULT_INSTANCE } else { &s.app.saved.instance };
    let status = if s.login.discovering { crate::i18n::tf("Connecting to {domain}…", &[("domain", host)]) } else { format!("Signing in to {host}") };
    row()
        .w_full()
        .mt(16.0)
        .justify_center()
        .gap(6.0)
        .child(text(status).font(font(13.0, 400.0)).color(p.text_tertiary))
        .child(link(t("Change"), "login-change-instance", p, |s: &mut State| {
            s.login.errors = Default::default();
            s.login.instance.focused = true;
            s.login.step = Step::Instance;
        }))
}

fn link(label: &str, key: &str, p: &Palette, f: fn(&mut State)) -> El {
    text(label)
        .key(key)
        .font(font(13.0, 500.0))
        .color(p.text_link)
        .cursor(rune::Cursor::Pointer)
        .role("link")
        .on_press(move |s: &mut State| f(s))
}

/// Form-level error (`.error` text) or the captcha note, above the footer.
fn form_note(l: &crate::login::Login, p: &Palette) -> Option<El> {
    let (msg, color) = match (&l.errors.form, l.slow) {
        (Some(e), _) => (e.clone(), mix(p.status_danger, p.text_primary, 78.0)),
        (None, true) => ("Verifying you're human…".to_owned(), p.text_tertiary),
        (None, false) => return None,
    };
    Some(text(msg).w_full().font(font(14.0, 400.0).lh(20.0)).color(color).wrap())
}

/// The 1px rule and right-aligned Back / primary buttons of every step.
#[allow(clippy::type_complexity)]
fn footer(p: &Palette, submitting: bool, back: fn(&mut State), primary: Option<(&str, &str, fn(&mut State))>) -> El {
    let footer_rule = mix(p.background_modifier_accent, Color::TRANSPARENT, 72.0);
    let mut buttons = row().w_full().justify_end().gap(12.0).child(
        Button::new(t("Back"))
            .variant(ButtonVariant::Secondary)
            .disabled(submitting)
            .key("login-back")
            .on_press(move |s: &mut State| back(s))
            .el(p)
            .min_w(112.0),
    );
    if let Some((label, key, f)) = primary {
        buttons = buttons.child(
            Button::new(label).submitting(submitting).key(key).on_press(move |s: &mut State| f(s)).el(p).min_w(112.0).default_button(),
        );
    }
    // `.footer`: margin-top 2, padding-top 14, 1px top rule.
    col().w_full().mt(2.0).gap(13.0).child(el().w_full().h(1.0).fill(footer_rule)).child(buttons)
}

fn to_method(s: &mut State) {
    s.login.errors = Default::default();
    s.login.step = Step::Method;
}

fn instance_step(s: &State, p: &Palette) -> El {
    let l = &s.login;
    col()
        .w_full()
        .child(title("Choose an instance", p))
        .child(
            col()
                .w_full()
                .gap(18.0)
                .child(
                    Input::new("login-instance-input", &l.instance, |s: &mut State| &mut s.login.instance)
                        .label(t("Instance"))
                        .placeholder(crate::session::DEFAULT_INSTANCE)
                        .error(l.errors.instance.as_deref())
                        .el(p),
                )
                .child(footer(p, l.discovering, to_method, Some((t("Continue"), "login-instance-continue", crate::app::choose_instance)))),
        )
}

fn mfa_step(s: &State, m: &crate::login::Mfa, p: &Palette) -> El {
    let l = &s.login;
    let codes = m.totp || m.backup_codes;
    let mut form = col().w_full().gap(18.0);
    if codes {
        let label = if m.totp { t("Authenticator code") } else { t("Backup code") };
        let hint = if m.totp {
            "Enter the 6-digit code from your authenticator app or a backup code."
        } else {
            t("Enter one of your backup codes if you can't use your passkey.")
        };
        form = form.child(description(hint, p)).child(
            Input::new("login-mfa-code", &l.code, |s: &mut State| &mut s.login.code)
                .label(label)
                .placeholder(if m.totp { t("6-digit code") } else { t("Backup code") })
                .error(l.errors.code.as_deref())
                .el(p),
        );
    }
    if m.webauthn {
        let mfa = m.clone();
        let label = if codes { t("Try security key / passkey instead") } else { t("Security key / passkey") };
        form = form.child(
            Button::new(label)
                .variant(if codes { ButtonVariant::Secondary } else { ButtonVariant::Primary })
                .icon(&icons::KEY)
                .fit_container()
                .disabled(l.submitting)
                .key("login-mfa-passkey")
                .on_press(move |s: &mut State| crate::app::start_passkey(s, Some(mfa.clone())))
                .el(p),
        );
    }
    let primary = codes.then_some((t("Verify"), "login-mfa-submit", crate::app::submit_mfa as fn(&mut State)));
    col()
        .w_full()
        .child(title(t("Two-factor authentication"), p))
        .child(form.children(form_note(l, p)).child(footer(p, l.submitting, |s| to(s, Step::Credentials), primary)))
}

/// `.description`: 14/20 centred, tertiary.
fn description(t: &str, p: &Palette) -> El {
    text(t).w_full().align(Align::Center).font(font(14.0, 400.0).lh(20.0)).color(p.text_tertiary).wrap()
}

fn to(s: &mut State, step: Step) {
    s.login.errors = Default::default();
    s.login.step = step;
}

fn to_register(s: &mut State) {
    s.login.register.pending = false;
    to(s, Step::Register);
    s.login.email.focused = true;
}

/// "Already have an account? Sign in" (`AuthBottomLink`).
fn bottom_link(prompt: &str, label: &str, key: &str, p: &Palette, f: fn(&mut State)) -> El {
    row()
        .w_full()
        .mt(16.0)
        .gap(4.0)
        .child(text(prompt).font(font(14.0, 400.0).lh(20.0)).color(p.text_tertiary))
        .child(link(label, key, p, f).font(font(14.0, 400.0).lh(20.0)))
}

/// `.registrationNotice`: a bordered box for form errors and registration states.
fn notice(msg: &str, p: &Palette) -> El {
    let bg = p.background_secondary;
    col()
        .w_full()
        .py(12.0)
        .px(17.0)
        .radius(8.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .child(text(msg).w_full().align(Align::Center).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary).wrap())
        .role("alert")
}

const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
/// `MAXIMUM_PLAUSIBLE_AGE` in `@fluxer/constants/RegionalMinimumAge`.
const MAX_AGE: i32 = 120;

/// `isPlausibleDateOfBirth`: a real date, before today, at most 120 years ago.
pub fn plausible_birth_date(y: i32, m: u32, d: u32, today: chrono::NaiveDate) -> bool {
    use chrono::Datelike;
    chrono::NaiveDate::from_ymd_opt(y, m, d).is_some_and(|date| date < today && y >= today.year() - MAX_AGE)
}

fn days_in_month(y: i32, m: u32) -> u32 {
    (28..=31).rev().find(|&d| chrono::NaiveDate::from_ymd_opt(y, m, d).is_some()).unwrap_or(31)
}

/// Drops a day the new month/year doesn't have, like `DateOfBirthField`.
fn clamp_day(r: &mut Register) {
    let (Some(y), Some(m), Some(d)) = (&r.year, &r.month, &r.day) else { return };
    if let (Ok(y), Ok(m), Ok(d)) = (y.parse(), m.parse(), d.parse::<u32>())
        && d > days_in_month(y, m)
    {
        r.day = None;
    }
}

/// `DateOfBirthField`: month / day / year selects (flex 2 : 1.5 : 1.5, gap 8) under a label.
fn date_of_birth(l: &crate::login::Login, width: f32, p: &Palette, layer: &mut Vec<El>) -> El {
    use chrono::Datelike;
    let r = &l.register;
    let today = chrono::Local::now().date_naive();
    let years: Vec<String> = (today.year() - MAX_AGE..=today.year()).rev().map(|y| y.to_string()).collect();
    let days_n = match (r.year.as_deref().and_then(|y| y.parse().ok()), r.month.as_deref().and_then(|m| m.parse().ok())) {
        (Some(y), Some(m)) => days_in_month(y, m),
        _ => 31,
    };
    let days: Vec<String> = (1..=days_n).map(|d| d.to_string()).collect();
    let months: Vec<String> = (1..=12).map(|m| m.to_string()).collect();
    let month_opts: Vec<(&str, &str)> = months.iter().zip(MONTHS).map(|(v, l)| (v.as_str(), l)).collect();
    let day_opts: Vec<(&str, &str)> = days.iter().map(|d| (d.as_str(), d.as_str())).collect();
    let year_opts: Vec<(&str, &str)> = years.iter().map(|y| (y.as_str(), y.as_str())).collect();
    let unit = (width - 16.0) / 5.0;
    let access = crate::ui::popup_access(|s: &mut State| &mut s.login.popups);
    let row_ = row()
        .w_full()
        .gap(8.0)
        .child(
            Select::new("register-dob-month", &month_opts, r.month.as_deref())
                .placeholder(t("Month"))
                .width(unit * 2.0)
                .on_choose(|s: &mut State, v| {
                    s.login.register.month = Some(v.to_owned());
                    clamp_day(&mut s.login.register);
                })
                .el(&l.popups, access, p, layer),
        )
        .child(
            Select::new("register-dob-day", &day_opts, r.day.as_deref())
                .placeholder(t("Day"))
                .width(unit * 1.5)
                .on_choose(|s: &mut State, v| s.login.register.day = Some(v.to_owned()))
                .el(&l.popups, access, p, layer),
        )
        .child(
            Select::new("register-dob-year", &year_opts, r.year.as_deref())
                .placeholder(t("Year"))
                .width(unit * 1.5)
                .on_choose(|s: &mut State, v| {
                    s.login.register.year = Some(v.to_owned());
                    clamp_day(&mut s.login.register);
                })
                .el(&l.popups, access, p, layer),
        );
    let implausible = match (&r.year, &r.month, &r.day) {
        (Some(y), Some(m), Some(d)) => {
            !matches!((y.parse(), m.parse(), d.parse()), (Ok(y), Ok(m), Ok(d)) if plausible_birth_date(y, m, d, today))
        }
        _ => false,
    };
    let error = l.errors.date_of_birth.clone().or_else(|| implausible.then(|| t("Enter your real date of birth.").to_owned()));
    col()
        .w_full()
        .gap(8.0)
        .child(text(t("Date of birth")).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary))
        .child(
            col()
                .w_full()
                .gap(6.0)
                .child(row_)
                .children(error.map(|e| text(e).font(font(14.0, 400.0).lh(20.0)).color(p.status_danger).wrap())),
        )
}

/// What the instance asks for at sign-up.
pub struct SignUp {
    pub collect_date_of_birth: bool,
    pub terms: Option<String>,
    pub privacy: Option<String>,
    /// `registration.mode == "closed"`.
    pub closed: bool,
}

pub fn sign_up(s: &State) -> SignUp {
    let Some(inst) = &s.app.instance else {
        return SignUp { collect_date_of_birth: true, terms: None, privacy: None, closed: false };
    };
    let d = &inst.discovery;
    let legal = &d.app_public.legal;
    // Official instances fall back to the marketing site's pages; self-hosted ones show none.
    let fallback = |page: &str| (!d.features.self_hosted).then(|| format!("{}/{page}", d.endpoints.marketing.trim_end_matches('/')));
    SignUp {
        collect_date_of_birth: d.app_public.registration.collect_date_of_birth,
        terms: legal.terms_url.clone().or_else(|| fallback("terms")),
        privacy: legal.privacy_url.clone().or_else(|| fallback("privacy")),
        closed: d.registration.mode == "closed",
    }
}

/// `RegistrationLegalNotice`: underlined terms / privacy links in 14/20 primary text.
fn legal_notice(su: &SignUp, p: &Palette) -> Option<El> {
    use rune::text::Span;
    let parts: Vec<(&str, Option<&str>)> = match (&su.terms, &su.privacy) {
        (Some(t), Some(pr)) => vec![
            ("By creating an account, you agree to the ", None),
            ("terms of service", Some(t.as_str())),
            (". The ", None),
            ("privacy policy", Some(pr.as_str())),
            (" explains how we handle your data.", None),
        ],
        (Some(t), None) => vec![("By creating an account, you agree to the ", None), ("terms of service", Some(t.as_str())), (".", None)],
        (None, Some(pr)) => vec![("The ", None), ("privacy policy", Some(pr.as_str())), (" explains how we handle your data.", None)],
        (None, None) => return None,
    };
    let (mut body, mut spans, mut links) = (String::new(), Vec::new(), Vec::new());
    for (t, url) in parts {
        let start = body.len();
        body.push_str(t);
        if let Some(url) = url {
            spans.push(Span::new(start, body.len()).color(p.text_link).underline(p.text_link));
            links.push(rune::Link::new(start..body.len(), url));
        }
    }
    let mut e = rune::rich(body, spans).w_full().font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).wrap();
    for l in links {
        e = e.link(l);
    }
    Some(e.on_link(|_: &mut State, url: &str, tx: &mut rune::Tx| {
        let _ = tx.open_url(url);
    }))
}

/// Fields still empty: the submit button stays disabled (`SubmitTooltip`).
pub fn register_missing(s: &State) -> bool {
    let (l, r) = (&s.login, &s.login.register);
    l.email.text.trim().is_empty()
        || l.password.text.is_empty()
        || r.confirm.text.is_empty()
        || (sign_up(s).collect_date_of_birth && (r.month.is_none() || r.day.is_none() || r.year.is_none()))
}

/// `USERNAME_PATTERN` / `MAX_USERNAME_LENGTH`.
pub fn username_problem(name: &str) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        None
    } else if name.chars().count() > 32 {
        Some("Username must be 32 characters or fewer")
    } else if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some(t("Only letters, numbers, and underscores"))
    } else {
        None
    }
}

fn register_step(s: &State, p: &Palette, layer: &mut Vec<El>) -> El {
    let l = &s.login;
    let r = &l.register;
    let su = sign_up(s);
    let heading = title(t("Create an account"), p).mb(24.0);
    let to_login: fn(&mut State) = |s| to(s, Step::Method);
    let body = if r.pending {
        col().w_full().child(notice(t("Your account request is pending approval. You can sign in after an admin approves it."), p))
    } else if su.closed {
        col().w_full().child(notice(t("Registration is currently closed. Use a registration link from an admin to create an account."), p))
    } else {
        let username_error = l.errors.username.as_deref().or(username_problem(&r.username.text));
        let username = Input::new("register-username", &r.username, |s: &mut State| &mut s.login.register.username)
            .label(t("Username (optional)"))
            .placeholder(t("Leave blank for a random username"))
            .el(p);
        let username = col().w_full().child(username).children(
            username_error.map(|e| text(e).mt(4.0).font(font(12.0, 400.0).lh(16.0)).color(mix(p.status_danger, p.text_primary, 78.0))),
        );
        let mut form = col()
            .w_full()
            .gap(20.0)
            .children(l.errors.form.as_deref().map(|e| notice(e, p)))
            .child(Input::new("register-email", &l.email, |s: &mut State| &mut s.login.email).label(t("Email")).error(l.errors.email.as_deref()).el(p))
            .child(
                Input::new("register-global-name", &r.global_name, |s: &mut State| &mut s.login.register.global_name)
                    .label(t("Display name (optional)"))
                    .placeholder(t("What should people call you?"))
                    .error(l.errors.global_name.as_deref())
                    .el(p),
            )
            .child(username)
            .child(
                Input::new("register-password", &l.password, |s: &mut State| &mut s.login.password)
                    .label(t("Password"))
                    .password(l.show_password, |s: &mut State| s.login.show_password = !s.login.show_password)
                    .error(l.errors.password.as_deref())
                    .el(p),
            )
            .child(
                Input::new("register-confirm", &r.confirm, |s: &mut State| &mut s.login.register.confirm)
                    .label(t("Confirm password"))
                    .password(r.show_confirm, |s: &mut State| s.login.register.show_confirm = !s.login.register.show_confirm)
                    .error(l.errors.confirm.as_deref())
                    .el(p),
            );
        if su.collect_date_of_birth {
            form = form.child(date_of_birth(l, 430.0, p, layer));
        }
        let slow = l.slow.then(|| text("Verifying you're human…").w_full().font(font(14.0, 400.0).lh(20.0)).color(p.text_tertiary));
        form.children(legal_notice(&su, p)).children(slow).child(
            Button::new(t("Create account"))
                .fit_container()
                .submitting(l.submitting)
                .disabled(register_missing(s) || username_problem(&r.username.text).is_some())
                .key("register-submit")
                .on_press(crate::app::submit_register)
                .el(p)
                .default_button(),
        )
    };
    col()
        .w_full()
        .child(heading)
        .child(body)
        .child(bottom_link(t("Already have an account?"), t("Sign in"), "register-to-login", p, to_login))
}

fn forgot_step(s: &State, sent: bool, p: &Palette) -> El {
    let l = &s.login;
    let to_login: fn(&mut State) = |s| to(s, Step::Credentials);
    // `ForgotPasswordPage.module.css`: title 20/28 mb 8, description mb 32, form gap 24, footer.
    let heading = title(if sent { t("Check your email") } else { t("Forgot your password?") }, p).mb(8.0);
    if sent {
        return col()
            .w_full()
            .child(heading)
            .child(description(t("If an account uses that address, we've sent it password reset instructions. Check your inbox for the reset link."), p).mb(32.0).role("status"))
            .child(row().w_full().mt(24.0).justify_center().child(link(t("Return to sign-in"), "forgot-return", p, to_login).font(font(14.0, 400.0).lh(20.0))));
    }
    let footer = col()
        .w_full()
        .mt(24.0)
        .gap(12.0)
        .items_center()
        .child(
            text(t("Back to sign-in"))
                .key("forgot-back")
                .font(font(14.0, 400.0).lh(20.0))
                .color(p.text_tertiary)
                .cursor(rune::Cursor::Pointer)
                .role("link")
                .on_press(move |s: &mut State| to_login(s)),
        )
        .child(row().gap(4.0).child(text(t("Don't have an account?")).font(font(14.0, 400.0).lh(20.0)).color(p.text_tertiary)).child(link(t("Register"), "forgot-register", p, to_register).font(font(14.0, 400.0).lh(20.0))));
    col()
        .w_full()
        .child(heading)
        .child(description(t("Enter your email address and we'll send you a link to reset your password."), p).mb(32.0))
        .child(
            col()
                .w_full()
                .gap(24.0)
                .child(Input::new("forgot-email", &l.email, |s: &mut State| &mut s.login.email).label(t("Email")).error(l.errors.email.as_deref()).el(p))
                .child(
                    Button::new(t("Send reset link"))
                        .fit_container()
                        .submitting(l.submitting)
                        .key("forgot-submit")
                        .on_press(crate::app::submit_forgot)
                        .el(p)
                        .default_button(),
                ),
        )
        .children(form_note(l, p).map(|n| n.mt(12.0)))
        .child(footer)
}

fn passkey_step(s: &State, k: &Passkey, p: &Palette) -> El {
    let l = &s.login;
    let heading = title(if k.mfa.is_some() { t("Two-factor authentication") } else { t("Sign in with a passkey") }, p);
    let mut body = col().w_full().gap(18.0);
    let mut primary = None;
    match k.prompt {
        Prompt::Touch | Prompt::SelectDevice => {
            let msg = if k.prompt == Prompt::Touch {
                "Insert your security key and touch it when it blinks."
            } else {
                "Touch the security key you want to use."
            };
            body = body.child(description(msg, p)).child(row().w_full().justify_center().child(crate::ui::spinner(p.text_tertiary)));
        }
        Prompt::Pin { wrong, attempts } => {
            let error = wrong.then(|| match attempts {
                Some(n) => format!("Wrong PIN. {n} attempts left."),
                None => "Wrong PIN. Try again.".to_owned(),
            });
            body = body.child(description("Enter your security key's PIN.", p)).child(
                Input::new("passkey-pin", &l.pin, |s: &mut State| &mut s.login.pin)
                    .label(t("Security key PIN"))
                    .password(l.show_password, |s: &mut State| s.login.show_password = !s.login.show_password)
                    .error(error.as_deref())
                    .el(p),
            );
            primary = Some((t("Continue"), "passkey-pin-submit", crate::app::submit_pin as fn(&mut State)));
        }
    }
    col()
        .w_full()
        .child(heading)
        .child(body.children(form_note(l, p)).child(footer(p, false, crate::app::cancel_passkey, primary)))
}

fn ip_step(ip: &crate::login::IpAuth, p: &Palette) -> El {
    let to = if ip.email.is_empty() { "your email".to_owned() } else { ip.email.clone() };
    col()
        .w_full()
        .gap(18.0)
        .child(title(t("Check your email"), p).mb(0.0))
        .child(
            text(format!("We sent a link to {to} to approve signing in from this new location. This page continues once you open it."))
                .w_full()
                .align(Align::Center)
                .font(font(14.0, 400.0).lh(20.0))
                .color(p.text_secondary)
                .wrap(),
        )
        .child(row().w_full().justify_center().child(crate::ui::spinner(p.text_tertiary)))
        .child(footer(p, false, |s| s.login.step = Step::Credentials, None))
}

fn credential_step(s: &State, p: &Palette) -> El {
    let l = &s.login;
    col()
        .w_full()
        .child(title(t("Welcome back"), p))
        .child(
            col()
                .w_full()
                .gap(18.0)
                .child(
                    Input::new("login-email-input", &l.email, |s: &mut State| &mut s.login.email)
                        .label(t("Email"))
                        .error(l.errors.email.as_deref())
                        .el(p),
                )
                .child(
                    Input::new("login-password-input", &l.password, |s: &mut State| &mut s.login.password)
                        .label(t("Password"))
                        .password(l.show_password, |s: &mut State| s.login.show_password = !s.login.show_password)
                        .error(l.errors.password.as_deref())
                        .el(p),
                )
                .child(
                    col().gap(4.0).child(
                        text(t("Forgot your password?"))
                            .key("login-forgot")
                            .font(font(14.0, 400.0))
                            .color(p.text_tertiary)
                            .cursor(rune::Cursor::Pointer)
                            .role("link")
                            .on_press(|s: &mut State| to(s, Step::Forgot { sent: false })),
                    ),
                )
                .children(form_note(l, p))
                .child(footer(p, l.submitting, to_method, Some((t("Sign in"), "login-submit", crate::app::submit_login)))),
        )
}

/// Headless `--scene` states of the logged-out screens, over an offline fluxer.com-like instance.
pub mod scenes {
    use super::*;

    fn instance(s: &mut State) {
        let mut d = reflux_model::InstanceDiscovery::default();
        d.endpoints.marketing = "https://fluxer.com".into();
        d.endpoints.webapp = "https://fluxer.com".into();
        d.app_public.registration.collect_date_of_birth = true;
        d.registration.mode = "open".into();
        let rest = reflux_net::Client::new(reflux_net::http_client(), "https://fluxer.com/api");
        s.app.instance = Some(std::sync::Arc::new(crate::app::Instance { discovery: d, rest }));
    }

    pub fn register(s: &mut State) {
        instance(s);
        s.login.step = Step::Register;
    }

    pub fn register_filled(s: &mut State) {
        register(s);
        let l = &mut s.login;
        l.email = TextEdit::new("someone@example.com".to_owned());
        l.password = TextEdit::new("hunter22hunter22".to_owned());
        l.register.confirm = TextEdit::new("hunter22hunter2".to_owned());
        l.register.username = TextEdit::new("not valid!".to_owned());
        (l.register.month, l.register.day, l.register.year) = (Some("2".into()), Some("29".into()), Some("2001".into()));
        l.errors.email = Some("Email is already in use.".into());
        l.errors.confirm = Some(t("The passwords don't match.").into());
        l.errors.form = Some("You're going too fast. Try again in 5s.".into());
    }

    pub fn register_dob(s: &mut State) {
        register(s);
        s.login.register.month = Some("3".into());
        s.login.popups.open = Some("register-dob-year".into());
    }

    pub fn register_pending(s: &mut State) {
        register(s);
        s.login.register.pending = true;
    }

    pub fn forgot(s: &mut State) {
        instance(s);
        s.login.step = Step::Forgot { sent: false };
    }

    pub fn forgot_sent(s: &mut State) {
        instance(s);
        s.login.step = Step::Forgot { sent: true };
    }

    fn mfa() -> Mfa {
        Mfa { ticket: "t".into(), totp: true, backup_codes: true, webauthn: true }
    }

    pub fn mfa_webauthn(s: &mut State) {
        s.login.step = Step::Mfa(mfa());
    }

    pub fn passkey_failed(s: &mut State) {
        s.login.errors.form = Some("The security key request failed: no security key was found.".into());
    }

    pub fn passkey(s: &mut State) {
        s.login.step = Step::Passkey(Passkey { mfa: None, prompt: Prompt::Touch });
    }

    pub fn passkey_pin(s: &mut State) {
        s.login.step = Step::Passkey(Passkey { mfa: Some(mfa()), prompt: Prompt::Pin { wrong: true, attempts: Some(7) } });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn birth_dates() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 7).unwrap();
        assert!(plausible_birth_date(2000, 2, 29, today));
        assert!(!plausible_birth_date(2001, 2, 29, today), "not a date");
        assert!(!plausible_birth_date(2026, 11, 7, today), "today");
        assert!(!plausible_birth_date(1905, 1, 1, today), "over 120");
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2023, 12), 31);
    }

    #[test]
    fn usernames() {
        assert_eq!(username_problem(""), None);
        assert_eq!(username_problem("ok_name1"), None);
        assert!(username_problem("no spaces").is_some());
        assert!(username_problem(&"a".repeat(33)).is_some());
    }
}
