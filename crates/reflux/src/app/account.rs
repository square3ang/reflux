//! Runs [`crate::settings::Op`]: account switching, profile fetches and the sudo prompt's
//! passkey and email-code requests.

use reflux_model::WebAuthnAuthenticationOptions;
use reqwest::Method;
use serde_json::{Value, json};

use super::{Route, accounts, discover, reset_view, session, spawn, update};
use crate::State;
use crate::guild::describe;
use crate::settings::Op;
use crate::settings::sudo::{self, Passkey};

pub(super) fn run(s: &mut State, op: Op) {
    match op {
        Op::SwitchAccount(i) => switch(s, i),
        Op::SwitchAccountTo { index, guild, channel } => {
            if index < s.app.saved.accounts.len() {
                s.app.pending_channel = Some((guild, channel));
                switch(s, index);
            }
        }
        Op::AddAccount => {
            accounts::demote(s);
            s.app.saved.stash_current();
            session::save(&s.app.saved);
            reset_view(s);
            s.app.pending_channel = None;
            s.app.route = Route::Auth;
            s.login = crate::login::Login { instance: rune::TextEdit::new(s.app.saved.instance.clone()), ..Default::default() };
            discover(s);
        }
        Op::RemoveAccount(i) => {
            if let Some(a) = s.app.saved.accounts.get(i) {
                accounts::stop(s, &a.slot());
            }
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
            let slow = s.app.saved.desktop.dev.slow_profile_load;
            spawn(async move {
                let _ = crate::shell::devtools::gate(slow, None).await;
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
        // `createPasskeyRegistration`: `makeCredential` on the key, then
        // `POST /users/@me/mfa/webauthn/credentials`.
        Op::AddPasskey => {
            let Some(inst) = s.app.instance.clone() else { return };
            let sh = &mut s.app.shell;
            let Some(opts) = sh.form.data.get("m.passkey_options").cloned() else { return };
            let text = |k: &str| sh.form.text.get(k).map(|t| t.text.trim().to_string()).unwrap_or_default();
            let name = text("m.name");
            let name = if name.is_empty() { "Passkey".to_string() } else { name };
            (sh.form.busy, sh.form.error) = (true, None);
            spawn(async move {
                let origin = crate::passkey::origin_for(&inst.discovery, opts["rp"]["id"].as_str().unwrap_or_default());
                let challenge = opts["challenge"].clone();
                let r = tokio::task::spawn_blocking(move || {
                    crate::passkey::create(
                        &opts,
                        &origin,
                        // The PIN is asked only when the key wants one.
                        move |st| {
                            if let crate::passkey::Status::Pin { reply, wrong, .. } = st {
                                update(move |s| {
                                    let sh = &mut s.app.shell;
                                    sh.account.passkey_pin = Some((reply, wrong));
                                    let mut e = rune::TextEdit::default();
                                    e.focused = true;
                                    sh.form.text.insert("m.pin".into(), e);
                                });
                            }
                        },
                        |_| {},
                    )
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
                let r = match r {
                    Ok(response) => {
                        let body = json!({ "response": response, "challenge": challenge, "name": name });
                        inst.rest.request::<Value, Value>(Method::POST, "/users/@me/mfa/webauthn/credentials", Some(&body)).await.map(drop).map_err(|e| describe(&e))
                    }
                    Err(e) => Err(e),
                };
                update(move |s| {
                    let sh = &mut s.app.shell;
                    sh.form.busy = false;
                    sh.account.passkey_pin = None;
                    match r {
                        Ok(()) => {
                            sh.close_account_modals();
                            sh.load("passkeys", "/users/@me/mfa/webauthn/credentials");
                        }
                        Err(e) => sh.form.error = Some(e),
                    }
                });
            });
        }
    }
}

/// Switches to `saved.accounts[i]`: the foreground becomes a background engine and the target
/// takes over its live gateway when it has one, else it reconnects.
fn switch(s: &mut State, i: usize) {
    if i >= s.app.saved.accounts.len() {
        return;
    }
    accounts::demote(s);
    if !s.app.saved.switch_to(i) {
        return;
    }
    session::save(&s.app.saved);
    reset_view(s);
    if accounts::promote(s) {
        return;
    }
    s.app.route = Route::Connecting;
    super::cache::restore(s);
    discover(s); // the instance may differ; connects once discovered
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

/// Copies the saved accounts and the background engines' activity into the shell for the
/// switcher and the inbox.
pub(super) fn mirror(s: &mut State) {
    let a = &mut s.app.shell.account;
    if a.current != s.app.saved.current {
        a.current = s.app.saved.current.clone();
    }
    if a.accounts != s.app.saved.accounts {
        a.accounts = s.app.saved.accounts.clone();
    }
    let activity = s.app.accounts.activities();
    if a.activity != activity {
        a.activity = activity;
    }
    a.me = s.app.store.my_id();
}
