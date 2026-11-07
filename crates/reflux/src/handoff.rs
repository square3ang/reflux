//! Browser sign-in (`auth/flow/browser_handoff`): the instance hands out a short code, the
//! person signs in at `{webapp}/login?handoff=1&code=…` in their browser (where single sign-on
//! also runs: `sso=1` starts it), and the app polls `/auth/handoff/{code}/status` until the
//! browser approves it and the session's token comes back.

use reflux_model::{AuthResponse, AuthToken};
use reqwest::Method;
use rune::{El, col, row, text};
use serde_json::{Value, json};

use crate::State;
use crate::app::{spawn, update};
use crate::i18n::t;
use crate::login::Step;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant};

/// `BROWSER_LOGIN_HANDOFF_POLL_INTERVAL_MS`.
const POLL_MS: u64 = 2000;
/// `useCountdown`'s refresh.
const TICK_MS: u64 = 500;

/// Seconds left on the code (`None`: no expiry given).
fn remaining(h: &Handoff, now: u64) -> Option<u64> {
    h.expires_ms.map(|e| e.saturating_sub(now).div_ceil(1000))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Handoff {
    pub code: String,
    pub url: String,
    pub sso: bool,
    pub error: Option<String>,
    /// `expires_at` (Unix ms), for `HandoffCodeDisplay`'s countdown.
    pub expires_ms: Option<u64>,
    /// "Copy code" was pressed at (ms): "Copied" for 2s.
    pub copied_ms: Option<u64>,
}

/// `buildBrowserLoginHandoffURL`.
pub fn url(webapp: &str, api: &str, code: &str, sso: bool) -> String {
    let origin = api.split('/').take(3).collect::<Vec<_>>().join("/");
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    let mut u = format!("{}/login?handoff=1&code={}&api={}", webapp.trim_end_matches('/'), enc(code), enc(&origin));
    if sso {
        u.push_str("&sso=1");
    }
    u
}

/// `formatDesktopHandoffCode`: 12 characters in groups of 6 (`XXXXXX-XXXXXX`).
pub fn display(code: &str) -> String {
    let c: Vec<char> = code.chars().filter(|c| c.is_ascii_alphanumeric()).flat_map(char::to_uppercase).take(12).collect();
    c.chunks(6).map(|g| g.iter().collect::<String>()).collect::<Vec<_>>().join("-")
}

/// Starts a browser sign-in (`sso`: straight into single sign-on).
pub fn start(s: &mut State, sso: bool) {
    let Some(inst) = s.app.instance.clone() else {
        s.login.errors.form = Some("No instance selected.".into());
        return;
    };
    s.login.errors = Default::default();
    s.login.attempt += 1;
    let attempt = s.login.attempt;
    s.login.submitting = true;
    spawn(async move {
        let r = inst.rest.request::<Value, Value>(Method::POST, "/auth/handoff/initiate", Some(&json!({}))).await;
        let (code, secret, expires_ms) = match r {
            Ok(v) => (v["code"].as_str().unwrap_or_default().to_string(), v["poll_secret"].as_str().map(str::to_owned), v["expires_at"].as_str().and_then(reflux_store::parse_iso_ms)),
            Err(e) => {
                let msg = crate::guild::describe(&e);
                return update(move |s| {
                    s.login.submitting = false;
                    s.login.errors.form = Some(msg);
                });
            }
        };
        let link = url(&inst.discovery.endpoints.webapp, &inst.discovery.endpoints.api_client, &code, sso);
        crate::settings::privacy::open_url(&link);
        let h = Handoff { code: code.clone(), url: link, sso, error: None, expires_ms, copied_ms: None };
        update(move |s| {
            s.login.submitting = false;
            s.login.step = Step::Handoff(h);
        });
        // Polls while this attempt's screen is up, redrawing the countdown each half second.
        for tick in 1u64.. {
            tokio::time::sleep(std::time::Duration::from_millis(TICK_MS)).await;
            let (tx, rx) = tokio::sync::oneshot::channel();
            update(move |s| {
                let _ = tx.send(s.login.attempt == attempt && matches!(&s.login.step, Step::Handoff(h) if h.error.is_none()));
            });
            if !rx.await.unwrap_or(false) {
                return;
            }
            if tick % (POLL_MS / TICK_MS) != 0 {
                continue;
            }
            let path = format!("/auth/handoff/{code}/status");
            let r = match &secret {
                Some(p) => inst.rest.request::<Value, Value>(Method::POST, &path, Some(&json!({ "poll_secret": p }))).await,
                None => inst.rest.get::<Value>(&path).await,
            };
            let v = match r {
                Ok(v) => v,
                Err(e) => return fail(attempt, crate::guild::describe(&e)),
            };
            match v["status"].as_str() {
                Some("completed") => {
                    let token = serde_json::from_value::<AuthToken>(v).map_err(|e| e.to_string());
                    return update(move |s| {
                        if s.login.attempt != attempt {
                            return;
                        }
                        match token {
                            Ok(tk) => {
                                s.login.step = Step::Method;
                                crate::app::on_auth(s, Ok(AuthResponse::Token(tk)));
                            }
                            Err(e) => set_error(s, e),
                        }
                    });
                }
                Some("expired") => return fail(attempt, t("This code has expired. Start again for a new one.").into()),
                Some("denied") => return fail(attempt, t("The sign-in was denied in the browser.").into()),
                _ => {}
            }
        }
    });
}

fn set_error(s: &mut State, e: String) {
    if let Step::Handoff(h) = &mut s.login.step {
        h.error = Some(e);
    }
}

fn fail(attempt: u64, e: String) {
    update(move |s| {
        if s.login.attempt == attempt {
            set_error(s, e);
        }
    });
}

/// The waiting screen (`HandoffCodeDisplay`): the code to match in the browser, its countdown,
/// "Copy code", and a way back.
pub fn view(h: &Handoff, p: &Palette) -> El {
    let heading = if h.sso { t("Single sign-on") } else { t("Sign in with your browser") };
    let now = crate::app::now_ms();
    let left = remaining(h, now);
    let sso = h.sso;
    let regenerate = |label: &str| Button::<State>::new(label).key("handoff-restart").fit_container().on_press(move |s: &mut State| start(s, sso)).el(p);
    let body = if left == Some(0) && h.error.is_none() {
        col()
            .w_full()
            .gap(18.0)
            .child(text(t("Code expired")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary))
            .child(crate::login::description(t("This handoff code has expired. Generate a new one to continue."), p))
            .child(regenerate(t("Generate new code")))
    } else {
        let code = h.code.clone();
        let copied = h.copied_ms.is_some_and(|at| now < at + 2000);
        let mut body = col()
            .w_full()
            .gap(18.0)
            .child(crate::login::description(t("Finish signing in in the browser window that opened. Make sure it shows this code:"), p))
            .child(
                col()
                    .w_full()
                    .items_center()
                    .gap(8.0)
                    .py(12.0)
                    .radius(8.0)
                    .fill(p.background_tertiary)
                    .child(text(display(&h.code)).font(font(28.0, 700.0).lh(34.0).mono().tracking(2.0)).color(p.text_primary).selectable(true))
                    .children(left.map(|n| text(crate::i18n::tf("Expires in {remaining}s", &[("remaining", &n.to_string())])).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary)))
                    .child(
                        Button::<State>::new(if copied { t("Copied") } else { t("Copy code") })
                            .variant(ButtonVariant::Secondary)
                            .key("handoff-copy")
                            .on_press_tx(move |s: &mut State, tx: &mut rune::Tx| {
                                tx.copy(code.clone());
                                if let Step::Handoff(h) = &mut s.login.step {
                                    h.copied_ms = Some(crate::app::now_ms());
                                }
                                tx.wake_after(std::time::Duration::from_millis(2000));
                            })
                            .el(p),
                    ),
            );
        body = match &h.error {
            Some(e) => body.child(text(e.clone()).font(font(14.0, 400.0).lh(20.0)).color(p.status_danger).wrap()),
            None => body.child(row().w_full().justify_center().gap(8.0).center_y().child(crate::ui::spinner(p.text_tertiary)).child(text(t("Waiting for your browser…")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted))),
        };
        let url = h.url.clone();
        body.child(if h.error.is_some() {
            regenerate(t("Start again"))
        } else {
            Button::<State>::new(t("Open browser again")).key("handoff-open").variant(ButtonVariant::Secondary).fit_container().on_press(move |_: &mut State| crate::settings::privacy::open_url(&url)).el(p)
        })
    };
    col().w_full().child(crate::login::title(heading, p)).child(body.child(crate::login::footer(p, false, |s| s.login.step = Step::Method, None)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn handoff_urls_and_codes_match_fluxer() {
        let u = super::url("https://web.fluxer.app/", "https://api.fluxer.app/v1", "ab12cd34", true);
        assert_eq!(u, "https://web.fluxer.app/login?handoff=1&code=ab12cd34&api=https%3A%2F%2Fapi.fluxer.app&sso=1");
        assert_eq!(super::display("abcdef234567"), "ABCDEF-234567");
        assert_eq!(super::display("ABCDEF-234567"), "ABCDEF-234567");
        let h = super::Handoff { code: String::new(), url: String::new(), sso: false, error: None, expires_ms: Some(10_000), copied_ms: None };
        assert_eq!(super::remaining(&h, 8_500), Some(2));
        assert_eq!(super::remaining(&h, 12_000), Some(0));
    }
}
