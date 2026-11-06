//! Runs [`crate::settings::Op`]: account switching, profile fetches and the sudo prompt's
//! passkey and email-code requests.

use reflux_model::WebAuthnAuthenticationOptions;
use reqwest::Method;
use serde_json::{Value, json};

use super::{Route, discover, reset, session, spawn, update};
use crate::State;
use crate::guild::describe;
use crate::settings::Op;
use crate::settings::sudo::{self, Passkey};

pub(super) fn run(s: &mut State, op: Op) {
    match op {
        Op::SwitchAccount(i) => {
            if s.app.saved.switch_to(i) {
                session::save(&s.app.saved);
                reset(s);
                s.app.route = Route::Connecting;
                discover(s); // the instance may differ; connects once discovered
            }
        }
        Op::AddAccount => {
            s.app.saved.stash_current();
            session::save(&s.app.saved);
            reset(s);
            s.app.route = Route::Auth;
            s.login = crate::login::Login { instance: rune::TextEdit::new(s.app.saved.instance.clone()), ..Default::default() };
            discover(s);
        }
        Op::RemoveAccount(i) => {
            s.app.saved.remove_account(i);
            session::save(&s.app.saved);
        }
        Op::FetchProfile { user, guild } => {
            let Some(inst) = s.app.instance.clone() else { return };
            let mut path = format!("/users/{user}/profile?with_mutual_guilds=true&with_mutual_friends=true");
            if let Some(g) = guild {
                path.push_str(&format!("&guild_id={g}"));
            }
            // Marks it as loading so it's asked once.
            s.app.shell.account.profiles.insert((user, guild), Ok(Value::Null));
            spawn(async move {
                let r = inst.rest.get::<Value>(&path).await.map_err(|e| describe(&e));
                update(move |s| {
                    s.app.shell.account.profiles.insert((user, guild), r);
                });
            });
        }
        Op::SudoEmail => {
            let Some(inst) = s.app.instance.clone() else { return };
            spawn(async move {
                let r = inst.rest.request::<Value, Value>(Method::POST, "/users/@me/sudo/email-code", Some(&json!({}))).await;
                update(move |s| {
                    let Some(x) = s.app.shell.account.sudo.as_mut() else { return };
                    match r {
                        Ok(v) => x.email_ticket = v["ticket"].as_str().map(str::to_owned),
                        Err(e) => x.fail(describe(&e)),
                    }
                });
            });
        }
        Op::SudoPasskey => {
            let Some(inst) = s.app.instance.clone() else { return };
            spawn(async move {
                let fail = |e: String| update(move |s| fail(s, e));
                let opts: WebAuthnAuthenticationOptions =
                    match inst.rest.request(Method::POST, "/users/@me/sudo/webauthn/authentication-options", Some(&json!({}))).await {
                        Ok(o) => o,
                        Err(e) => return fail(describe(&e)),
                    };
                let origin = crate::passkey::origin_for(&inst.discovery, &opts.rp_id);
                let challenge = opts.challenge.clone();
                let r = tokio::task::spawn_blocking(move || {
                    crate::passkey::get(&opts, &origin, |st| update(move |s| status(s, st)), |c| update(move |s| set(s, |x| x.cancel = Some(c))))
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
                match r {
                    Ok(response) => update(move |s| {
                        sudo::retry(&mut s.app.shell, json!({ "mfa_method": "webauthn", "webauthn_response": response, "webauthn_challenge": challenge }))
                    }),
                    Err(e) => fail(e),
                }
            });
        }
        Op::AddPasskey => {
            // shortcut: no WebAuthn registration (makeCredential) client yet; passkeys are added
            // in the web app and then listed, renamed and removed here.
            s.app.shell.form.error = Some(crate::i18n::t("Adding a passkey isn't supported in Reflux yet. Add it from the Fluxer web app.").into());
        }
    }
}

fn set(s: &mut State, f: impl FnOnce(&mut sudo::Sudo)) {
    if let Some(x) = s.app.shell.account.sudo.as_mut() {
        f(x);
    }
}

fn fail(s: &mut State, e: String) {
    set(s, |x| x.fail(e));
}

fn status(s: &mut State, st: crate::passkey::Status) {
    set(s, |x| {
        x.passkey = match st {
            crate::passkey::Status::Pin { reply, wrong, .. } => {
                x.pin = rune::TextEdit::default();
                x.pin.focused = true;
                x.pin_reply = Some(reply);
                Passkey::Pin { wrong }
            }
            _ => Passkey::Waiting,
        }
    });
}

/// Records who the session is (on READY) for the account switcher.
pub(super) fn remember_me(s: &mut State) {
    let Some(me) = s.app.store.me.as_ref().map(|u| &u.base) else { return };
    let a = session::Account {
        instance: s.app.saved.instance.clone(),
        user_id: me.id.to_string(),
        username: me.username.clone(),
        display_name: me.global_name.clone().unwrap_or_else(|| me.username.clone()),
        avatar: me.avatar.clone(),
        token: None,
        imported: false,
    };
    if s.app.saved.current.as_ref() != Some(&a) {
        s.app.saved.current = Some(a);
        session::save(&s.app.saved);
    }
}

/// Copies the saved accounts into the shell for the switcher.
pub(super) fn mirror(s: &mut State) {
    let a = &mut s.app.shell.account;
    if a.current != s.app.saved.current {
        a.current = s.app.saved.current.clone();
    }
    if a.accounts != s.app.saved.accounts {
        a.accounts = s.app.saved.accounts.clone();
    }
    a.me = s.app.store.my_id();
}
