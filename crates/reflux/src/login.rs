//! The login screen: `AuthLayout` card with `AuthBrandPanel` and the login method / credential
//! steps (`fluxer_app/src/features/auth`). Numbers come from `assets/reference/login-layout.json`.

use std::f32::consts::PI;

use rune::{Align, Color, Cx, El, Icon, Shadow, TextEdit, col, el, icon, row, text};

use crate::State;
use crate::icons;
use crate::theme::{self, Palette, font};
use crate::ui::{Button, ButtonVariant, Input, mix, over, rgb, rgba};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Step {
    #[default]
    Method,
    Credentials,
}

#[derive(Default)]
pub struct Login {
    pub step: Step,
    pub email: TextEdit,
    pub password: TextEdit,
    pub show_password: bool,
    pub submitting: bool,
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
    col()
        .key("login-root")
        .w_full()
        .h_full()
        .center()
        .fill(rgb(8, 6, 22))
        .linear_gradient(145.0 * PI / 180.0, [(0.0, rgb(5, 4, 13)), (0.46, rgb(8, 6, 22)), (1.0, rgb(13, 10, 28))])
        .children(stars(size))
        .child(card(s, p))
}

fn card(s: &State, p: &Palette) -> El {
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
            col().grow(1.0).padding(40.0).justify_center().child(match s.login.step {
                Step::Method => method_step(s, p),
                Step::Credentials => credential_step(s, p),
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
        .child(title("Welcome back", p))
        .child(
            col()
                .w_full()
                .gap(10.0)
                .child(
                    Button::new("Sign in with email")
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
                    Button::<State>::new("Sign in with a passkey")
                        .variant(ButtonVariant::Secondary)
                        .icon(&icons::KEY)
                        .fit_container()
                        .disabled(disabled)
                        .key("login-passkey")
                        .el(p),
                )
                .child(
                    Button::<State>::new("Register an account")
                        .variant(ButtonVariant::Secondary)
                        .icon(&icons::USER_PLUS)
                        .icon_gap(8.0)
                        .fit_container()
                        .key("login-register")
                        .el(p)
                        .role("link"),
                ),
        )
}

fn credential_step(s: &State, p: &Palette) -> El {
    let l = &s.login;
    let footer_rule = mix(p.background_modifier_accent, Color::TRANSPARENT, 72.0);
    col()
        .w_full()
        .child(title("Welcome back", p))
        .child(
            col()
                .w_full()
                .gap(18.0)
                .child(Input::new("login-email-input", &l.email, |s: &mut State| &mut s.login.email).label("Email").el(p))
                .child(
                    Input::new("login-password-input", &l.password, |s: &mut State| &mut s.login.password)
                        .label("Password")
                        .password(l.show_password, |s: &mut State| s.login.show_password = !s.login.show_password)
                        .el(p),
                )
                .child(
                    col().gap(4.0).child(
                        text("Forgot your password?")
                            .font(font(14.0, 400.0))
                            .color(p.text_tertiary)
                            .cursor(rune::Cursor::Pointer)
                            .role("link"),
                    ),
                )
                .child(
                    // `.footer`: margin-top 2, padding-top 14, 1px top rule.
                    col().w_full().mt(2.0).gap(13.0).child(el().w_full().h(1.0).fill(footer_rule)).child(
                        row()
                            .w_full()
                            .justify_end()
                            .gap(12.0)
                            .child(
                                Button::new("Back")
                                    .variant(ButtonVariant::Secondary)
                                    .disabled(l.submitting)
                                    .key("login-back")
                                    .on_press(|s: &mut State| s.login.step = Step::Method)
                                    .el(p)
                                    .min_w(112.0),
                            )
                            .child(
                                Button::new("Sign in")
                                    .submitting(l.submitting)
                                    .key("login-submit")
                                    // shortcut: no network yet, so submitting only shows the spinner.
                                    .on_press(|s: &mut State| s.login.submitting = true)
                                    .el(p)
                                    .min_w(112.0),
                            ),
                    ),
                ),
        )
}
