//! App runtime: a Tokio runtime for the network, a bridge from its tasks to UI state through
//! Rune's `AppHandle::update`, and the auth / gateway / message flows that UI handlers start.

use std::future::Future;
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reflux_model::{
    AuthResponse, CloseCode, IdentifyProperties, InstanceDiscovery, LoginRequest,
    RegisterRequest, SessionFlags, Snowflake, TotpLoginRequest, WebAuthnLoginRequest,
};
use reflux_net::{Client, Command, ConnectionState, Gateway, GatewayConfig, GatewayEvent, MessagePage};
use reflux_store::Store;
use rune::{AppHandle, Cx, El, TextEdit, col, text};
use tokio::sync::mpsc::UnboundedSender;

use crate::login::{IpAuth, Mfa, Prompt, Step};
use crate::theme::{self, font};
use crate::ui::{Button, ButtonVariant, spinner};
use crate::{State, session};

mod actions;
mod chat;
mod guild;
pub mod live;
mod voice;

const PAGE: u8 = 50;
const IP_POLL: Duration = Duration::from_millis(2000);
/// When the sign-in request takes this long, the ALTCHA proof of work is probably running.
const SLOW: Duration = Duration::from_secs(2);

struct Bridge {
    rt: tokio::runtime::Runtime,
    ui: AppHandle<State>,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

/// Starts the network runtime (its worker threads are the background threads) and boots the
/// session: load the saved instance/token, discover, and connect if signed in.
pub fn start(ui: AppHandle<State>) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("reflux-net")
        .enable_all()
        .build()
        .expect("tokio runtime");
    if BRIDGE.set(Bridge { rt, ui }).is_ok() {
        update(boot);
    }
}

/// Runs `fut` on the network runtime. Does nothing before [`start`] (headless snapshots).
pub fn spawn(fut: impl Future<Output = ()> + Send + 'static) {
    if let Some(b) = BRIDGE.get() {
        b.rt.spawn(fut);
    }
}

/// Changes UI state from a task; runs on the UI thread before the next frame.
pub fn update(f: impl FnOnce(&mut State) + Send + 'static) {
    let Err(f) = live::send(Box::new(f)) else { return };
    if let Some(b) = BRIDGE.get() {
        b.ui.update(move |s, _| f(s));
    }
}

fn window() -> Option<&'static AppHandle<State>> {
    BRIDGE.get().map(|b| &b.ui)
}

/// Shows and focuses the window (a notification click, a second launch).
pub fn show_window() {
    if let Some(w) = window() {
        w.open();
    }
}

/// [`show_window`] with an XDG activation token when the caller has one (Wayland focus).
pub fn show_window_with(token: Option<String>) {
    match (window(), token) {
        (Some(w), Some(t)) => w.activate(t),
        _ => show_window(),
    }
}

pub fn hide_window() {
    if let Some(w) = window() {
        w.hide();
    }
}

pub fn window_visible() -> bool {
    window().is_some_and(|w| w.is_visible())
}

/// A tokenless focus request: Wayland compositors mark the window urgent.
pub fn request_attention() {
    if let Some(w) = window() {
        w.focus();
    }
}

pub fn quit() {
    if let Some(w) = window() {
        w.quit();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Route {
    #[default]
    Auth,
    /// Token known, waiting for discovery and READY.
    Connecting,
    Main,
}

pub struct Instance {
    pub discovery: InstanceDiscovery,
    pub rest: Client,
}

#[derive(Default)]
pub struct App {
    pub route: Route,
    pub saved: session::Saved,
    /// Discovered instance; `None` while discovering or after a failure.
    pub instance: Option<Arc<Instance>>,
    pub store: Store,
    pub connection: Option<ConnectionState>,
    pub selected_channel: Option<Snowflake>,
    pub shell: crate::shell::ShellState,
    /// Why the saved session couldn't connect (shown on the connecting screen).
    pub error: Option<String>,
    gateway: Option<UnboundedSender<Command>>,
    /// Bumped per gateway connection, so events of a closed one are dropped.
    generation: u64,
    acks: actions::PendingAcks,
    /// The member list subscription last sent: (guild, channel, ranges).
    member_sub: Option<(Snowflake, Snowflake, Vec<[u32; 2]>)>,
    voice: voice::VoiceRt,
}

/// What views (the shell) ask for; `act` runs it.
#[derive(Clone, Debug)]
pub enum Action {
    /// Select a channel and fetch its newest messages if none are loaded.
    SelectChannel(Snowflake),
    /// `reply`: (message, ping its author); `attachments`: the tray's files.
    SendMessage { channel: Snowflake, content: String, reply: Option<(Snowflake, bool)>, attachments: Vec<crate::shell::upload::Upload> },
    /// Fetch the page before the oldest loaded message.
    LoadOlder(Snowflake),
    Logout,
}

pub fn act(s: &mut State, a: Action) {
    match a {
        Action::SelectChannel(ch) => {
            s.app.selected_channel = Some(ch);
            if !s.app.store.messages.contains_key(&ch) {
                fetch_messages(s, ch);
            }
        }
        Action::SendMessage { channel, content, reply, attachments } => chat::send(s, channel, content, reply, attachments),
        Action::LoadOlder(ch) => fetch_older(s, ch),
        Action::Logout => {
            if let Some(inst) = s.app.instance.clone().filter(|_| !s.app.saved.imported) {
                spawn(async move {
                    if let Err(e) = inst.rest.logout().await {
                        eprintln!("reflux: logout: {e}");
                    }
                });
            }
            sign_out(s, None);
        }
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

// ---- Instance ------------------------------------------------------------------------------

fn boot(s: &mut State) {
    s.app.saved = session::load();
    crate::i18n::set_locale(s.app.saved.locale.as_deref().unwrap_or_else(|| crate::i18n::system()));
    restore_voice_devices(s);
    s.login.instance = TextEdit::new(s.app.saved.instance.clone());
    if s.app.saved.token.is_some() {
        s.app.route = Route::Connecting;
    }
    discover(s);
}

/// Instance picker "Continue".
pub fn choose_instance(s: &mut State) {
    let Some(host) = reflux_net::discovery::normalize(&s.login.instance.text) else {
        s.login.errors.instance = Some("Enter a valid instance address, like fluxer.com.".into());
        return;
    };
    s.app.saved.instance = host.trim_start_matches("https://").to_owned();
    session::save(&s.app.saved);
    discover(s);
}

pub fn discover(s: &mut State) {
    let (host, id) = (s.app.saved.instance.clone(), s.app.saved.installation_id.clone());
    s.app.instance = None;
    s.app.error = None;
    s.login.discovering = true;
    s.login.errors = Default::default();
    spawn(async move {
        let http = reflux_net::http_client();
        let r = reflux_net::discover(&http, &host).await;
        update(move |s| {
            if s.app.saved.instance != host {
                return; // another instance was picked meanwhile
            }
            s.login.discovering = false;
            match r {
                Ok(d) => {
                    let rest = Client::from_discovery(http, &d).with_installation_id(id);
                    rest.set_token(s.app.saved.token.clone());
                    s.app.instance = Some(Arc::new(Instance { discovery: d, rest }));
                    if s.login.step == Step::Instance {
                        s.login.step = Step::Method;
                    }
                    if s.app.route == Route::Connecting {
                        connect(s);
                    }
                }
                Err(e) => {
                    let msg = format!("Couldn't reach {host}: {e}");
                    if s.app.route == Route::Connecting {
                        s.app.error = Some(msg);
                    } else {
                        s.login.step = Step::Instance;
                        s.login.errors.instance = Some(msg);
                    }
                }
            }
        });
    });
}

// ---- Login ---------------------------------------------------------------------------------

pub fn submit_login(s: &mut State) {
    let l = &mut s.login;
    if l.submitting {
        return;
    }
    l.errors = Default::default();
    let email = l.email.text.trim().to_owned();
    if email.is_empty() {
        l.errors.email = Some("This field is required.".into());
    }
    if l.password.text.is_empty() {
        l.errors.password = Some("This field is required.".into());
    }
    if l.errors.email.is_some() || l.errors.password.is_some() {
        return;
    }
    let req = LoginRequest { login: Some(email), password: l.password.text.clone(), ..Default::default() };
    request(s, move |inst| async move { inst.rest.login(&req).await });
}

pub fn submit_mfa(s: &mut State) {
    let Step::Mfa(m) = &s.login.step else { return };
    let code: String = s.login.code.text.chars().filter(|c| !c.is_whitespace()).collect();
    if code.is_empty() {
        s.login.errors.code = Some("This field is required.".into());
        return;
    }
    let req = TotpLoginRequest { code, ticket: m.ticket.clone() };
    request(s, move |inst| async move { inst.rest.login_totp(&req).await.map(AuthResponse::Token) });
}

/// Runs an auth request with the submitting/slow flags and hands its result to `on_auth`.
fn request<F, Fut>(s: &mut State, f: F)
where
    F: FnOnce(Arc<Instance>) -> Fut + Send + 'static,
    Fut: Future<Output = reflux_net::Result<AuthResponse>> + Send,
{
    request_with(s, f, on_auth);
}

fn request_with<T, F, Fut>(s: &mut State, f: F, done: fn(&mut State, reflux_net::Result<T>))
where
    T: Send + 'static,
    F: FnOnce(Arc<Instance>) -> Fut + Send + 'static,
    Fut: Future<Output = reflux_net::Result<T>> + Send,
{
    let Some(inst) = s.app.instance.clone() else {
        s.login.errors.form = Some(if s.login.discovering { "Still connecting to the instance…" } else { "No instance selected." }.into());
        return;
    };
    s.login.errors = Default::default();
    s.login.submitting = true;
    s.login.slow = false;
    s.login.attempt += 1;
    let attempt = s.login.attempt;
    spawn(async move {
        tokio::time::sleep(SLOW).await;
        update(move |s| s.login.slow = s.login.submitting && s.login.attempt == attempt);
    });
    spawn(async move {
        let r = f(inst).await;
        update(move |s| {
            s.login.submitting = false;
            s.login.slow = false;
            done(s, r);
        });
    });
}

fn on_auth(s: &mut State, r: reflux_net::Result<AuthResponse>) {
    match r {
        Ok(AuthResponse::Token(t)) => signed_in(s, t.token),
        Ok(AuthResponse::Mfa(m)) => {
            s.login.code = TextEdit::default();
            s.login.code.focused = m.totp || m.backup_codes;
            s.login.step = Step::Mfa(Mfa { ticket: m.ticket, totp: m.totp, backup_codes: m.backup_codes, webauthn: m.webauthn });
        }
        Ok(AuthResponse::PendingApproval(_)) if s.login.step == Step::Register => s.login.register.pending = true,
        Ok(AuthResponse::PendingApproval(_)) => {
            s.login.errors.form = Some("Your account is waiting for approval by the instance's staff.".into());
        }
        Err(reflux_net::Error::Api { error, .. }) if error.code == "IP_AUTHORIZATION_REQUIRED" && error.ticket.is_some() => {
            let email = error.extra.get("email").and_then(|v| v.as_str()).unwrap_or_default().to_owned();
            let ip = IpAuth { ticket: error.ticket.unwrap_or_default(), email, alive: Arc::new(()) };
            poll_ip_authorization(s, ip.ticket.clone(), Arc::downgrade(&ip.alive));
            s.login.step = Step::IpAuth(ip);
        }
        Err(e) => s.login.errors = crate::login::Errors::from_error(&e),
    }
}

pub fn submit_register(s: &mut State) {
    if s.login.submitting || crate::login::register_missing(s) {
        return;
    }
    s.login.errors = Default::default();
    if s.login.password.text != s.login.register.confirm.text {
        s.login.errors.confirm = Some(crate::i18n::t("The passwords don't match.").into());
        return;
    }
    let r = &s.login.register;
    let opt = |t: &str| Some(t.trim().to_owned()).filter(|t| !t.is_empty());
    let date_of_birth = match (&r.year, &r.month, &r.day) {
        (Some(y), Some(m), Some(d)) if crate::login::sign_up(s).collect_date_of_birth => Some(format!("{y}-{m:0>2}-{d:0>2}")),
        _ => None,
    };
    let req = RegisterRequest {
        email: opt(&s.login.email.text),
        username: opt(&r.username.text),
        global_name: opt(&r.global_name.text),
        password: Some(s.login.password.text.clone()),
        date_of_birth,
        consent: true,
        ..Default::default()
    };
    request(s, move |inst| async move { inst.rest.register(&req).await });
}

pub fn submit_forgot(s: &mut State) {
    let email = s.login.email.text.trim().to_owned();
    if s.login.submitting {
        return;
    }
    if email.is_empty() {
        s.login.errors.email = Some("This field is required.".into());
        return;
    }
    request_with(s, move |inst| async move { inst.rest.forgot_password(&email).await }, |s, r| match r {
        Ok(()) => s.login.step = Step::Forgot { sent: true },
        Err(_) => s.login.errors.email = Some(crate::i18n::t("Failed to send reset link. Try again.").into()),
    });
}

/// Signs in with a security key: password-less, or as the second factor of `mfa`.
pub fn start_passkey(s: &mut State, mfa: Option<Mfa>) {
    let Some(inst) = s.app.instance.clone() else {
        s.login.errors.form = Some(if s.login.discovering { "Still connecting to the instance…" } else { "No instance selected." }.into());
        return;
    };
    s.login.errors = Default::default();
    s.login.attempt += 1;
    let attempt = s.login.attempt;
    let ticket = mfa.as_ref().map(|m| m.ticket.clone());
    s.login.step = Step::Passkey(crate::login::Passkey { mfa, prompt: Prompt::Touch });
    // Applies `f` only while this attempt's passkey screen is still up.
    let live = move |f: Box<dyn FnOnce(&mut State) + Send>| {
        update(move |s| {
            if s.login.attempt == attempt && matches!(s.login.step, Step::Passkey(_)) {
                f(s)
            }
        })
    };
    spawn(async move {
        let opts = match &ticket {
            Some(t) => inst.rest.login_webauthn_options(t).await,
            None => inst.rest.passkey_options().await,
        };
        let opts = match opts {
            Ok(o) => o,
            Err(e) => return live(Box::new(move |s| passkey_failed(s, crate::login::Errors::from_error(&e)))),
        };
        let origin = crate::passkey::origin_for(&inst.discovery, &opts.rp_id);
        let challenge = opts.challenge.clone();
        let assertion = tokio::task::spawn_blocking(move || {
            crate::passkey::get(
                &opts,
                &origin,
                move |st| live(Box::new(move |s| passkey_status(s, st))),
                move |c| live(Box::new(move |s| s.login.passkey_cancel = Some(c))),
            )
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
        let response = match assertion {
            Ok(r) => r,
            Err(msg) => {
                let errors = crate::login::Errors { form: Some(msg), ..Default::default() };
                return live(Box::new(move |s| passkey_failed(s, errors)));
            }
        };
        let req = WebAuthnLoginRequest { response, challenge, ticket: ticket.clone() };
        let r = match ticket {
            Some(_) => inst.rest.login_webauthn(&req).await,
            None => inst.rest.login_passkey(&req).await,
        };
        live(Box::new(move |s| {
            passkey_back(s);
            on_auth(s, r.map(AuthResponse::Token));
        }));
    });
}

fn passkey_status(s: &mut State, st: crate::passkey::Status) {
    let Step::Passkey(k) = &mut s.login.step else { return };
    k.prompt = match st {
        crate::passkey::Status::Touch => Prompt::Touch,
        crate::passkey::Status::SelectDevice => Prompt::SelectDevice,
        crate::passkey::Status::Pin { reply, wrong, attempts } => {
            s.login.pin = TextEdit::default();
            s.login.pin.focused = true;
            s.login.pin_reply = Some(reply);
            Prompt::Pin { wrong, attempts }
        }
    };
}

pub fn submit_pin(s: &mut State) {
    let pin = std::mem::take(&mut s.login.pin);
    if let (Some(reply), Step::Passkey(k)) = (s.login.pin_reply.take(), &mut s.login.step) {
        let _ = reply.send(authenticator::Pin::new(&pin.text));
        k.prompt = Prompt::Touch;
    }
}

/// Leaves the passkey screen for where it came from (MFA or the method list).
fn passkey_back(s: &mut State) {
    if let Some(c) = s.login.passkey_cancel.take() {
        c.cancel();
    }
    s.login.pin_reply = None;
    s.login.pin = TextEdit::default();
    if let Step::Passkey(k) = std::mem::take(&mut s.login.step) {
        s.login.step = k.mfa.map_or(Step::Method, Step::Mfa);
    }
}

fn passkey_failed(s: &mut State, errors: crate::login::Errors) {
    passkey_back(s);
    s.login.errors = errors;
}

/// Back on the passkey screen: cancel the key request and ignore its late result.
pub fn cancel_passkey(s: &mut State) {
    s.login.attempt += 1;
    passkey_back(s);
    s.login.errors = Default::default();
}

/// Polls every 2 s while the IP authorization screen (holding `alive`) is shown.
fn poll_ip_authorization(s: &State, ticket: String, alive: Weak<()>) {
    let Some(inst) = s.app.instance.clone() else { return };
    spawn(async move {
        loop {
            tokio::time::sleep(IP_POLL).await;
            if alive.strong_count() == 0 {
                return;
            }
            match inst.rest.poll_ip_authorization(&ticket).await {
                Ok(p) if p.completed => {
                    let Some(token) = p.token else { return };
                    return update(move |s| {
                        if matches!(s.login.step, Step::IpAuth(_)) {
                            signed_in(s, token);
                        }
                    });
                }
                Ok(_) => {}
                Err(e @ reflux_net::Error::Api { .. }) => {
                    return update(move |s| {
                        if matches!(s.login.step, Step::IpAuth(_)) {
                            s.login.step = Step::Credentials;
                            s.login.errors = crate::login::Errors::from_error(&e);
                        }
                    });
                }
                Err(e) => eprintln!("reflux: IP authorization poll: {e}"), // network blip: keep polling
            }
        }
    });
}

fn signed_in(s: &mut State, token: String) {
    s.login = crate::login::Login { instance: std::mem::take(&mut s.login.instance), ..Default::default() };
    s.app.saved.token = Some(token.clone());
    s.app.saved.imported = false;
    session::save(&s.app.saved);
    if let Some(inst) = &s.app.instance {
        inst.rest.set_token(Some(token));
    }
    s.app.route = Route::Connecting;
    connect(s);
}

/// "Import session from Fluxer desktop": take over the official app's session and connect.
pub fn import_session(s: &mut State) {
    let r = session::fluxer_desktop_db().ok_or_else(|| "No config directory.".to_owned()).and_then(|db| session::import_fluxer_desktop(&db));
    match r {
        Ok(saved) => {
            session::save(&saved);
            s.login = crate::login::Login { instance: TextEdit::new(saved.instance.clone()), ..Default::default() };
            s.app.saved = saved;
            s.app.route = Route::Connecting;
            discover(s); // the instance and installation ID may have changed
        }
        Err(e) => s.login.errors.form = Some(e),
    }
}

fn restore_voice_devices(s: &mut State) {
    let v = &mut s.app.shell.voice;
    (v.input, v.output) = (s.app.saved.voice_input.clone(), s.app.saved.voice_output.clone());
}

/// Forgets the session locally; `message` is shown on the login screen.
fn sign_out(s: &mut State, message: Option<String>) {
    voice::leave(s, true);
    if let Some(gw) = s.app.gateway.take() {
        let _ = gw.send(Command::Close);
    }
    s.app.generation += 1;
    s.app.store = Store::default();
    s.app.connection = None;
    s.app.selected_channel = None;
    let avatars = std::mem::take(&mut s.app.shell.default_avatars);
    let media = s.app.shell.media.clone();
    media.set_base("");
    s.app.shell = crate::shell::ShellState { default_avatars: avatars, media, ..Default::default() };
    restore_voice_devices(s);
    s.app.error = None;
    s.app.route = Route::Auth;
    s.app.saved.token = None;
    session::save(&s.app.saved);
    s.login = crate::login::Login { instance: std::mem::take(&mut s.login.instance), ..Default::default() };
    s.login.errors.form = message;
    if s.app.instance.is_none() && !s.login.discovering {
        discover(s);
    }
}

// ---- Gateway -------------------------------------------------------------------------------

fn connect(s: &mut State) {
    let (Some(inst), Some(token)) = (s.app.instance.clone(), s.app.saved.token.clone()) else { return };
    if let Some(gw) = s.app.gateway.take() {
        let _ = gw.send(Command::Close);
    }
    s.app.generation += 1;
    s.app.error = None;
    let generation = s.app.generation;
    let config = GatewayConfig {
        url: inst.discovery.endpoints.gateway.clone(),
        token,
        properties: IdentifyProperties {
            os: os_name().into(),
            browser: "Reflux".into(),
            device: std::env::consts::ARCH.into(),
            ..Default::default()
        },
        flags: SessionFlags::CHANNEL_THREADS,
        presence: None,
        compress: true,
    };
    spawn(async move {
        let mut gw = match Gateway::connect(config) {
            Ok(gw) => gw,
            Err(e) => return update(move |s| s.app.error = Some(format!("Gateway: {e}"))),
        };
        let commands = gw.commands.clone();
        update(move |s| {
            if s.app.generation == generation {
                s.app.gateway = Some(commands);
            }
        });
        while let Some(ev) = gw.events.recv().await {
            update(move |s| {
                if s.app.generation == generation {
                    on_gateway(s, ev);
                }
            });
        }
    });
}

fn on_gateway(s: &mut State, ev: GatewayEvent) {
    match ev {
        GatewayEvent::State(c) => {
            if c == ConnectionState::Disconnected(Some(CloseCode::AUTHENTICATION_FAILED)) {
                if let Some(inst) = &s.app.instance {
                    inst.rest.set_token(None);
                }
                return sign_out(s, Some("Your session has expired. Please sign in again.".into()));
            }
            if let ConnectionState::Disconnected(code) = &c {
                s.app.error = Some(format!("Disconnected from the gateway ({code:?})."));
            }
            s.app.connection = Some(c);
        }
        GatewayEvent::Dispatch(d) => {
            s.app.store.apply(&d, now_ms());
            match &d {
                reflux_model::DispatchEvent::VoiceServerUpdate(v) => voice::on_server_update(s, v),
                reflux_model::DispatchEvent::VoiceStateUpdate(v) => voice::on_state(s, v),
                _ => {}
            }
            if let reflux_model::DispatchEvent::Ready(_) = &d {
                s.app.member_sub = None; // a new session has no subscriptions
                s.app.route = Route::Main;
                if !s.app.store.settings.locale.is_empty() {
                    crate::i18n::set_locale(&s.app.store.settings.locale);
                }
                crate::desktop::ready(s);
            }
            crate::desktop::on_dispatch(s, &d);
        }
        GatewayEvent::Error(e) => voice::on_error(s, &e),
        GatewayEvent::Malformed { t, error, .. } => eprintln!("reflux: malformed {t} skipped: {error}"),
    }
}

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "linux" => "Linux",
        "macos" => "Mac OS X",
        "windows" => "Windows",
        other => other,
    }
}

// ---- Messages ------------------------------------------------------------------------------

fn fetch_messages(s: &State, channel: Snowflake) {
    let Some(inst) = s.app.instance.clone() else { return };
    let generation = s.app.generation;
    spawn(async move {
        let r = inst.rest.messages(channel, MessagePage::Latest, PAGE).await;
        update(move |s| match r {
            Ok(page) if s.app.generation == generation => s.app.store.set_latest_messages(channel, page, PAGE as usize),
            Ok(_) => {}
            Err(e) => eprintln!("reflux: messages of {channel}: {e}"),
        });
    });
}

fn fetch_older(s: &State, channel: Snowflake) {
    let Some(w) = s.app.store.messages.get(&channel) else { return };
    let (Some(oldest), Some(inst)) = (w.messages.first().map(|m| m.id), s.app.instance.clone()) else { return };
    if !w.has_more_before {
        return;
    }
    let generation = s.app.generation;
    spawn(async move {
        let r = inst.rest.messages(channel, MessagePage::Before(oldest), PAGE).await;
        update(move |s| match r {
            Ok(page) if s.app.generation == generation => s.app.store.prepend_messages(channel, page, PAGE as usize),
            Ok(_) => {}
            Err(e) => eprintln!("reflux: older messages of {channel}: {e}"),
        });
    });
}

/// Runs what the shell queued since the last frame; true when anything ran.
pub fn poll(s: &mut State, tx: &mut rune::Tx) -> bool {
    s.app.shell.now_ms = now_ms();
    let f = s.app.instance.as_ref().map(|i| &i.discovery.features);
    s.app.shell.premium = f.is_some_and(|f| !f.self_hosted || f.premium_enabled);
    let community = s.app.instance.as_ref().map(|i| &i.discovery.community);
    s.app.shell.personal_notes = community.is_none_or(|c| c.personal_notes_enabled != Some(false));
    // The Voice & video tab lists devices once it opens.
    let sh = &mut s.app.shell;
    if sh.settings.as_ref().is_some_and(|u| u.tab == "voice_video") && sh.voice.devices.is_none() && !s.app.voice.devices_loading {
        s.app.voice.devices_loading = true;
        sh.intents.push(crate::shell::Intent::LoadDevices);
    }
    let intents = std::mem::take(&mut s.app.shell.intents);
    let ran = !intents.is_empty();
    use crate::shell::Intent;
    // A jump loads around its message itself; the latest page would race it.
    let jumps: Vec<Snowflake> = intents.iter().filter_map(|i| if let Intent::JumpTo { channel, .. } = i { Some(*channel) } else { None }).collect();
    for i in intents {
        match i {
            Intent::SelectGuild(_) => {}
            Intent::SelectChannel(c) if jumps.contains(&c) => s.app.selected_channel = Some(c),
            Intent::SelectChannel(c) => act(s, Action::SelectChannel(c)),
            Intent::SendMessage { channel, content, reply, attachments } => act(s, Action::SendMessage { channel, content, reply, attachments }),
            i @ (Intent::LoadNewer { .. } | Intent::JumpTo { .. } | Intent::JumpToPresent(_) | Intent::LoadPins { .. } | Intent::Search(_)) => chat::run(s, i),
            Intent::LoadOlder { channel } => act(s, Action::LoadOlder(channel)),
            Intent::PatchSettings(v) => crate::settings::patch(s, v),
            Intent::Api(c) => guild::run(s, c),
            Intent::PatchGuildSettings { guild: g, patch } => guild::patch_settings(s, g, patch),
            Intent::Logout => act(s, Action::Logout),
            Intent::Prefs(p) => {
                s.app.saved.desktop = p;
                session::save(&s.app.saved);
            }
            Intent::SetLocale(code) => {
                crate::i18n::set_locale(code);
                s.app.saved.locale = Some(code.to_string());
                session::save(&s.app.saved);
                if s.app.route == Route::Main {
                    crate::settings::patch(s, serde_json::json!({ "locale": code }));
                }
            }
            i @ (Intent::JoinVoice { .. }
            | Intent::LeaveVoice
            | Intent::VoiceSelf
            | Intent::ToggleScreen
            | Intent::Watch(_)
            | Intent::LoadDevices
            | Intent::VoiceDevices) => voice::run(s, i),
            other => actions::run(s, other),
        }
    }
    actions::subscribe_member_list(s);
    s.app.shell.prefs = s.app.saved.desktop.clone();
    crate::desktop::tick(s);
    live::on_poll(s, tx);
    let shell = crate::shell::poll_shell(&s.app.store, &mut s.app.shell, tx);
    actions::tick(s) || ran || shell
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ` (UTC), the API's timestamp format.
fn iso8601(ms: u64) -> String {
    let (days, rem) = ((ms / 86_400_000) as i64, ms % 86_400_000);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (h, mi, sec, milli) = (rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{sec:02}.{milli:03}Z")
}

// ---- Views ---------------------------------------------------------------------------------

/// Auth, connecting screen, or the shell.
pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let p = theme::palette(theme::Kind::from_setting(&s.app.store.settings.theme));
    let body = match s.app.route {
        Route::Auth => return crate::login::view(s, cx),
        Route::Connecting => {
            let mut c = col().gap(16.0).items_center();
            match &s.app.error {
                None => c = c.child(spinner(p.text_primary)).child(text(crate::i18n::t("Connecting…")).font(font(16.0, 500.0)).color(p.text_primary)),
                Some(e) => {
                    c = c
                        .child(text(e.clone()).font(font(14.0, 400.0)).color(p.text_primary).wrap().max_w(480.0))
                        .child(Button::new(crate::i18n::t("Retry")).key("connect-retry").on_press(|s: &mut State| discover(s)).el(p))
                }
            }
            c.child(logout_button(p))
        }
        Route::Main => return crate::shell::view(&s.app.store, &s.app.shell, p, |s: &mut State| &mut s.app.shell),
    };
    col().w_full().h_full().center().fill(p.background_primary).child(body)
}

fn logout_button(p: &theme::Palette) -> El {
    Button::new(crate::i18n::t("Log out"))
        .variant(ButtonVariant::Secondary)
        .key("logout")
        .on_press(|s: &mut State| act(s, Action::Logout))
        .el(p)
}

#[cfg(test)]
mod tests {
    #[test]
    fn iso8601() {
        assert_eq!(super::iso8601(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(super::iso8601(1_420_070_400_000), "2015-01-01T00:00:00.000Z");
        assert_eq!(super::iso8601(1_709_210_096_789), "2024-02-29T12:34:56.789Z");
    }
}

