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

#[derive(Clone, Debug, PartialEq)]
pub struct Handoff {
    pub code: String,
    pub url: String,
    pub sso: bool,
    pub error: Option<String>,
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

/// `XXXX-XXXX` for reading aloud, as `displayCode` shows it.
pub fn display(code: &str) -> String {
    let c: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase();
    if c.len() == 8 { format!("{}-{}", &c[..4], &c[4..]) } else { c }
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
        let (code, secret) = match r {
            Ok(v) => (v["code"].as_str().unwrap_or_default().to_string(), v["poll_secret"].as_str().map(str::to_owned)),
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
        let h = Handoff { code: code.clone(), url: link, sso, error: None };
        update(move |s| {
            s.login.submitting = false;
            s.login.step = Step::Handoff(h);
        });
        // Polls while this attempt's screen is up.
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
            let (tx, rx) = tokio::sync::oneshot::channel();
            update(move |s| {
                let _ = tx.send(s.login.attempt == attempt && matches!(s.login.step, Step::Handoff(_)));
            });
            if !rx.await.unwrap_or(false) {
                return;
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

/// The waiting screen: the code to match in the browser, and a way back.
pub fn view(h: &Handoff, p: &Palette) -> El {
    let heading = if h.sso { t("Single sign-on") } else { t("Sign in with your browser") };
    let mut body = col()
        .w_full()
        .gap(18.0)
        .child(crate::login::description(t("Finish signing in in the browser window that opened. Make sure it shows this code:"), p))
        .child(
            row()
                .w_full()
                .justify_center()
                .py(12.0)
                .radius(8.0)
                .fill(p.background_tertiary)
                .child(text(display(&h.code)).font(font(28.0, 700.0).lh(34.0).mono().tracking(2.0)).color(p.text_primary).selectable(true)),
        );
    body = match &h.error {
        Some(e) => body.child(text(e.clone()).font(font(14.0, 400.0).lh(20.0)).color(p.status_danger).wrap()),
        None => body.child(row().w_full().justify_center().gap(8.0).center_y().child(crate::ui::spinner(p.text_tertiary)).child(text(t("Waiting for your browser…")).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted))),
    };
    let url = h.url.clone();
    let sso = h.sso;
    let again = if h.error.is_some() {
        Button::<State>::new(t("Start again")).key("handoff-restart").fit_container().on_press(move |s: &mut State| start(s, sso)).el(p)
    } else {
        Button::<State>::new(t("Open browser again")).key("handoff-open").variant(ButtonVariant::Secondary).fit_container().on_press(move |_: &mut State| crate::settings::privacy::open_url(&url)).el(p)
    };
    col()
        .w_full()
        .child(crate::login::title(heading, p))
        .child(body.child(again).child(crate::login::footer(p, false, |s| s.login.step = Step::Method, None)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn handoff_urls_and_codes_match_fluxer() {
        let u = super::url("https://web.fluxer.app/", "https://api.fluxer.app/v1", "ab12cd34", true);
        assert_eq!(u, "https://web.fluxer.app/login?handoff=1&code=ab12cd34&api=https%3A%2F%2Fapi.fluxer.app&sso=1");
        assert_eq!(super::display("ab12cd34"), "AB12-CD34");
    }
}
