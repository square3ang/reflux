//! User settings: Fluxer's full-window settings modal (`features/user/components/modals/
//! UserSettingsModal.tsx`, `app/components/dialogs/components/DesktopSettingsView.tsx`, styles in
//! `dialogs/shared/SettingsModalLayout.module.css`): a nav sidebar of tabs grouped by category with
//! Sign out at the bottom, and a content pane with a title bar and close button. Esc closes it.
//!
//! Views only queue [`Intent`]s; [`patch`] applies a settings change optimistically to the store
//! and sends `PATCH /users/@me/settings`.

mod account;
mod desktop_tabs;
mod look_tabs;
pub(crate) mod dev;
pub(crate) mod privacy;
mod premium;
pub use premium::{gift_accept, open_gift};
pub(crate) use privacy::open_url;
pub(crate) mod profile;
mod advanced;
pub(crate) mod badges;
pub(crate) mod recovery_kit;
mod recovery_pdf;
pub(crate) mod crop;
mod timezone;
mod timezones;
pub(crate) mod search;
pub(crate) mod search_engines;
pub mod theme_folder;
mod theme_library;
mod theme_menu;
mod theme_meta;
mod search_index;
pub mod sudo;
pub(crate) mod tabs;
mod voice_tabs;

pub use account::{AccountModal, view_shell_modals};

use reflux_model::Snowflake;
use reflux_store::Store;
use rune::{Color, El, ScrollSpec, Shadow, TextEdit, col, el, icon, row, text, text_input};
use serde_json::Value;

use crate::i18n::t;
use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant, Modal, is_light, mix, over, rgba, scroller};

/// UI state while the settings modal is open.
pub struct SettingsUi {
    pub tab: &'static str,
    pub search: TextEdit,
    pub reveal_email: bool,
    pub confirm_logout: bool,
    /// A dialog over the settings (email or password change, TOTP setup, …).
    pub modal: Option<AccountModal>,
    /// The Profile tab edits this community's profile (`ProfileTypeSelector`).
    pub profile_guild: Option<Snowflake>,
    /// The query the search results were last set up for (`debouncedSearchQuery`), and the
    /// results folded since.
    pub searched: String,
    /// The typed query waiting out the 300 ms debounce, and since when.
    pub typed: Option<(String, std::time::Instant)>,
    pub collapsed: Vec<&'static str>,
}

impl SettingsUi {
    /// Opens on `tab` (a [`TABS`] id), or on the first tab like Fluxer.
    pub fn new(tab: Option<&str>) -> Self {
        let tab = TABS.iter().find(|t| Some(t.id) == tab).map_or("my_profile", |t| t.id);
        Self { tab, search: TextEdit::new(""), reveal_email: false, confirm_logout: false, modal: None, profile_guild: None, searched: String::new(), typed: None, collapsed: Vec::new() }
    }
}

/// Account state that outlives the settings modal: the sudo prompt, fetched profiles, the
/// user-area modals and the account switcher's list.
#[derive(Default)]
pub struct AccountUi {
    pub sudo: Option<sudo::Sudo>,
    /// `GET /users/{id}/profile` answers by (user, guild); `Err` = couldn't load.
    pub profiles: std::collections::HashMap<(Snowflake, Option<Snowflake>), Result<Value, String>>,
    /// `UserProfileModal`: (user, guild, tab).
    pub profile_modal: Option<(Snowflake, Option<Snowflake>, &'static str)>,
    pub modal: Option<ShellModal>,
    /// Mirrors of the saved session for the switcher: the active account and the others.
    pub current: Option<crate::session::Account>,
    pub accounts: Vec<crate::session::Account>,
    /// The signed-in user's ID.
    pub me: Snowflake,
    /// Adding a passkey: the key asked for its PIN (reply, the last one was wrong).
    pub passkey_pin: Option<(std::sync::mpsc::Sender<authenticator::Pin>, bool)>,
}

/// A modal the user area or a menu opens outside settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellModal {
    /// `CustomStatusModal`.
    CustomStatus,
    /// `AccountSwitcherModal`.
    Accounts,
    /// `ChangeNicknameModal` for a community.
    Nickname(Snowflake),
}

/// Account requests that aren't a plain API call.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    SwitchAccount(usize),
    AddAccount,
    RemoveAccount(usize),
    /// `GET /users/{user}/profile` into [`AccountUi::profiles`].
    FetchProfile { user: Snowflake, guild: Option<Snowflake> },
    SudoPasskey,
    SudoEmail,
    /// Register a security key as a passkey.
    AddPasskey,
}

#[allow(clippy::result_large_err)]
/// A list a tab loaded into `form.data[key]`, or what to show instead (loading, the error).
pub(crate) fn loaded<T: serde::de::DeserializeOwned>(st: &ShellState, key: &str, p: &Palette) -> Result<T, El> {
    let note = |s: String| text(s).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap();
    match st.form.data.get(key) {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| note(format!("Couldn't read the list: {e}"))),
        None => Err(note(st.form.error.clone().unwrap_or_else(|| t("Loading…").into()))),
    }
}

#[allow(clippy::too_many_arguments)]
/// A settings form field (`form.text[key]`) that shows `initial` until edited.
pub(crate) fn input<S: 'static>(st: &ShellState, acc: ShellAccess<S>, key: &'static str, initial: &str, label: &str, placeholder: &str, secure: bool, p: &Palette) -> El {
    let edit = st.form.text.get(key).cloned().unwrap_or_else(|| TextEdit::new(initial));
    let init = initial.to_string();
    let el_key = format!("form-{key}");
    let mut i = crate::ui::Input::new(&el_key, &edit, move |s: &mut S| -> &mut TextEdit {
        acc(s).form.text.entry(key.to_string()).or_insert_with(|| TextEdit::new(init.clone()))
    })
    .placeholder(placeholder);
    if !label.is_empty() {
        i = i.label(label);
    }
    if secure {
        i = i.secure();
    }
    i.el(p)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cat {
    Account,
    Billing,
    App,
    Developer,
}

impl Cat {
    fn label(self) -> &'static str {
        t(match self {
            Cat::Account => t("Your account"),
            Cat::Billing => t("Billing"),
            Cat::App => t("App settings"),
            Cat::Developer => t("Developer"),
        })
    }
}

struct TabDef {
    id: &'static str,
    cat: Cat,
    label: &'static str,
    icon: &'static rune::Icon,
}

const fn tab(id: &'static str, cat: Cat, label: &'static str, icon: &'static rune::Icon) -> TabDef {
    TabDef { id, cat, label, icon }
}

/// `ALL_TABS_DESCRIPTORS` (`settings_utils/SettingsConstants.tsx`) as a Linux desktop build shows
/// them. Subscriptions, Payments and Operator pass only appear once the account has one, so they're
/// left out.
const TABS: &[TabDef] = &[
    tab("my_profile", Cat::Account, "Profile", &icons::USER),
    tab("account_security", Cat::Account, "Account", &icons::SHIELD),
    tab("privacy_safety", Cat::Account, "Privacy", &icons::EYE_SLASH),
    tab("linked_accounts", Cat::Account, "Connections", &icons::USER_LIST),
    tab("authorized_apps", Cat::Account, "Authorized apps", &icons::ROBOT),
    tab("blocked_users", Cat::Account, "Blocked users", &icons::PROHIBIT),
    tab("devices", Cat::Account, "Devices", &icons::DEVICES),
    tab("plutonium", Cat::Billing, "Plutonium", &icons::ATOM),
    tab("gift_inventory", Cat::Billing, "Gifts", &icons::GIFT),
    tab("appearance", Cat::App, "Appearance", &icons::PAINT_BRUSH),
    tab("themes", Cat::App, "Themes", &icons::SWATCHES),
    tab("notifications", Cat::App, "Notifications", &icons::BELL),
    tab("chat_settings", Cat::App, "Chat", &icons::CHAT_CIRCLE),
    tab("voice_video", Cat::App, "Voice & video", &icons::MICROPHONE),
    tab("accessibility", Cat::App, "Accessibility", &icons::PERSON_SIMPLE_CIRCLE),
    tab("language", Cat::App, "Language", &icons::TRANSLATE_BOLD),
    tab("keybinds", Cat::App, "Keybinds", &icons::KEYBOARD),
    tab("desktop_settings", Cat::App, DESKTOP_APP, &icons::DESKTOP),
    tab("advanced_settings", Cat::App, "Advanced", &icons::GEAR),
    tab("applications", Cat::Developer, "Applications", &icons::CODE_BOLD),
    tab("embed_debugger", Cat::Developer, "Embed debugger", &icons::CODE_BOLD),
    tab("component_gallery", Cat::Developer, "Design system", &icons::PALETTE),
];

const DESKTOP_APP: &str = match std::env::consts::OS.as_bytes() {
    b"windows" => "Windows app",
    b"macos" => "macOS app",
    b"linux" => "Linux app",
    _ => "Desktop app",
};

/// Profile is the card above the nav; Authorized apps, Blocked users and Devices live inside the
/// Account tab (`PRIMARY_SETTINGS_NAV_HIDDEN_TAB_TYPES`).
const NAV_HIDDEN: &[&str] = &["my_profile", "authorized_apps", "blocked_users", "devices"];

fn visible_id(id: &str, store: &Store, premium: bool) -> bool {
    TABS.iter().find(|t| t.id == id).is_some_and(|t| visible(t, store, premium))
}

fn visible(t: &TabDef, store: &Store, premium: bool) -> bool {
    match t.id {
        "embed_debugger" | "component_gallery" => store.settings.developer_mode,
        _ => t.cat != Cat::Billing || premium,
    }
}

fn label_of(id: &str) -> &'static str {
    t(TABS.iter().find(|tab| tab.id == id).map_or(t("User settings"), |tab| tab.label))
}

// ---- Settings changes ----------------------------------------------------------------------

/// Merges a partial settings object into `settings` (what the server will do); false when it
/// doesn't fit the type, leaving `settings` unchanged.
pub fn apply<T: serde::Serialize + serde::de::DeserializeOwned>(settings: &mut T, patch: &Value) -> bool {
    let (Ok(Value::Object(mut all)), Value::Object(p)) = (serde_json::to_value(&*settings), patch) else { return false };
    all.extend(p.clone());
    match serde_json::from_value(Value::Object(all)) {
        Ok(s) => {
            *settings = s;
            true
        }
        Err(_) => false,
    }
}

/// The keys of `patch` as `old` had them: applied after a failed request, it undoes that patch
/// alone, leaving later optimistic changes to other keys in place.
pub fn revert_of<T: serde::Serialize>(old: &T, patch: &Value) -> Value {
    let (Ok(Value::Object(all)), Value::Object(p)) = (serde_json::to_value(old), patch) else { return Value::Null };
    Value::Object(p.keys().filter_map(|k| Some((k.clone(), all.get(k)?.clone()))).collect())
}

/// Applies `patch` to the store now and sends it; a failed request puts its keys back.
pub fn patch(s: &mut crate::State, patch: Value) {
    let revert = revert_of(&s.app.store.settings, &patch);
    if !apply(&mut s.app.store.settings, &patch) {
        return eprintln!("reflux: settings patch doesn't fit: {patch}");
    }
    let Some(inst) = s.app.instance.clone() else { return };
    crate::app::spawn(async move {
        let r = inst.rest.update_settings(&patch).await;
        crate::app::update(move |s| match r {
            Ok(new) => s.app.store.settings = new,
            Err(e) => {
                eprintln!("reflux: settings update: {e}");
                apply(&mut s.app.store.settings, &revert);
            }
        });
    });
}

// ---- View ----------------------------------------------------------------------------------

/// Viewport-dependent sizes of the settings modal (the CSS uses vw/vh clamps and media queries).
struct Metrics {
    pad: [f32; 2],
    card_w: f32,
    sidebar_w: f32,
    gutter: f32,
    inner_max: f32,
}

fn metrics(w: f32, h: f32) -> Metrics {
    let clamp = |lo: f32, v: f32, hi: f32| v.max(lo).min(hi);
    // `.layerFullscreen` padding and `.root.fullscreen` width, per its width media queries.
    let (pad_y, pad_x) = if w >= 2560.0 {
        (clamp(28.0, 0.04 * h, 56.0), clamp(96.0, 0.12 * w, 320.0))
    } else if w >= 1920.0 {
        (clamp(24.0, 0.035 * h, 48.0), clamp(64.0, 0.10 * w, 200.0))
    } else if w >= 1440.0 {
        (clamp(20.0, 0.03 * h, 40.0), clamp(48.0, 0.08 * w, 128.0))
    } else if w >= 1024.0 {
        (clamp(16.0, 0.025 * h, 32.0), clamp(32.0, 0.06 * w, 96.0))
    } else if w >= 768.0 {
        (clamp(12.0, 0.02 * h, 24.0), clamp(24.0, 0.04 * w, 64.0))
    } else {
        (clamp(8.0, 0.015 * h, 16.0), clamp(12.0, 0.02 * w, 24.0))
    };
    let pad = [pad_y, pad_x];
    let card_w = (w - 2.0 * pad[1] - 2.0).min(clamp(960.0, 0.96 * w, 1400.0) - 2.0);
    let sidebar_w = clamp(252.0, 0.24 * w, 320.0);
    let content_w = card_w - 2.0 - sidebar_w - 1.0;
    // `--settings-edge-gutter` and `.desktopScrollInner` max-width per breakpoint.
    let (gutter, inner_max) = if w >= 1920.0 {
        (clamp(40.0, 0.07 * w, 80.0), clamp(900.0, 0.75 * content_w, 1120.0))
    } else if w >= 1440.0 {
        (clamp(32.0, 0.06 * w, 64.0), clamp(820.0, 0.80 * content_w, 1040.0))
    } else if w >= 1024.0 {
        (clamp(24.0, 0.05 * w, 48.0), clamp(740.0, 0.85 * content_w, 960.0))
    } else if w >= 768.0 {
        (clamp(20.0, 0.04 * w, 32.0), clamp(640.0, 0.90 * content_w, 800.0))
    } else {
        (clamp(16.0, 0.03 * w, 24.0), clamp(640.0, 0.90 * content_w, 800.0))
    };
    Metrics { pad, card_w, sidebar_w, gutter, inner_max }
}

const LAYER_KEY: &str = "settings-layer";

/// The modal layer (backdrop + card, and the sign-out confirmation over it).
pub fn view<S: 'static>(store: &Store, st: &ShellState, ui: &SettingsUi, p: &Palette, premium: bool, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    let mut popups = Vec::new();
    let close = move |s: &mut S| acc(s).settings = None;
    let q = ui.searched.as_str();
    let hits = search::search(q, |id| visible_id(id, store, premium));
    let (head, nav) = sidebar(store, st, ui, p, premium, &hits, acc);
    let (title, body) = if q.is_empty() {
        (label_of(ui.tab), tabs::body(store, st, ui, ui.tab, p, acc, &mut popups))
    } else {
        (t("Search results"), search::results(store, st, ui, &hits, p, acc, &mut popups))
    };
    let mut modal = frame(t("User settings"), title, head, nav, body, close, p);
    if ui.confirm_logout {
        let cancel = move |s: &mut S| set(acc, s, |u| u.confirm_logout = false);
        modal = modal.child(
            Modal::new(t("Sign out?"), cancel)
                .body(text(t("You can sign back in at any time.")).font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap())
                .footer(
                    row()
                        .gap(8.0)
                        .child(Button::new(t("Stay signed in")).variant(ButtonVariant::Secondary).on_press(cancel).el(p))
                        .child(Button::new(t("Sign out")).key("settings-sign-out").on_press(move |s: &mut S| acc(s).intents.push(Intent::Logout)).el(p)),
                )
                .el(p),
        );
    }
    layer.push(modal);
    layer.extend(popups);
    if st.nav.timezone {
        layer.push(timezone::modal(store, st, p, acc));
    }
    if let Some(c) = &st.nav.crop {
        layer.push(crop::modal(st, c, p, acc));
    }
    if let Some(m) = &ui.modal {
        let mut pops = Vec::new();
        if *m == AccountModal::EncoderControls {
            layer.push(advanced::encoder_modal(st, p, acc, &mut pops));
        } else if let AccountModal::SwitchGroup(id) = m {
            layer.push(advanced::switch_group_modal(st, id, p, acc));
        } else if search_engines::modal(store, st, m, p, acc, &mut pops) || theme_menu::modal(st, m, p, acc, &mut pops) {
        } else {
            layer.extend(account::modal(store, st, m, p, acc));
        }
        layer.extend(pops);
    }
}

/// The full-window settings modal user, community and channel settings share: the backdrop and
/// card, a sidebar (`head` over the scrolling `nav`), and a content pane titled `title` around
/// `body`. Escape and the close button call `close`.
pub(crate) fn frame<S: 'static>(label: &str, title: &str, head: Vec<El>, nav: El, body: El, close: impl Fn(&mut S) + Clone + 'static, p: &Palette) -> El {
    // The layer covers the window; its size from the last frame stands in for the viewport.
    let [w, h] = rune::memo::layout_size(LAYER_KEY).unwrap_or([1280.0, 800.0]);
    let m = metrics(w, h);
    let c = colors(p);
    let radius = (0.01 * w).clamp(8.0, 12.0);
    let viewport = col()
        .key("settings-nav")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .child(col().w_full().pr(8.0).child(nav));
    let sidebar = col()
        .w(m.sidebar_w)
        .h_full()
        .fill(c.surface)
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .pt(16.0)
                .px(12.0)
                .gap(4.0)
                .children(head)
                .child(col().w_full().grow(1.0).min_h(0.0).mr(-8.0).child(scroller(viewport, false, p))),
        )
        .child(el().pin(None, Some(0.0), Some(0.0), Some(0.0)).w(1.0).fill(over(c.border, c.surface)));
    let card = row()
        .w(m.card_w)
        .h_full()
        .radius(radius)
        .fill(p.background_secondary)
        .border(1.0, over(p.background_header_secondary, p.background_secondary))
        .box_shadow(Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 1.0, color: rgba(47, 49, 55, 0.08), inset: false })
        .box_shadow(Shadow { x: 0.0, y: 8.0, blur: 24.0, spread: -4.0, color: rgba(0, 0, 0, 0.25), inset: false })
        .box_shadow(Shadow { x: 0.0, y: 20.0, blur: 48.0, spread: -8.0, color: rgba(0, 0, 0, 0.2), inset: false })
        .clip_rounded(radius)
        .role("dialog")
        .label(label.to_string())
        // The dialog is the initial Tab stop: opening it announces the title, and Enter does not
        // press a sidebar tab before the person has looked at it.
        .focusable(true)
        .no_focus_ring()
        .child(sidebar)
        .child(content(title, body, p, &m, close.clone()))
        .modal(true, close);
    el().key(LAYER_KEY)
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(Color::BLACK).opacity(0.85))
        .child(
            row()
                .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                .py(m.pad[0] + 1.0)
                .px(m.pad[1])
                .justify_center()
                .child(card),
        )
}

fn set<S>(acc: ShellAccess<S>, s: &mut S, f: impl FnOnce(&mut SettingsUi)) {
    if let Some(u) = acc(s).settings.as_mut() {
        f(u);
    }
}

fn select<S: 'static>(acc: ShellAccess<S>, id: &'static str) -> impl Fn(&mut S) + 'static {
    move |s: &mut S| open_tab(acc(s), id)
}

impl ShellState {
    /// Opens user settings on `tab` (see [`SettingsUi::new`]).
    pub fn open_settings(&mut self, tab: Option<&str>) {
        let u = SettingsUi::new(tab);
        let id = u.tab;
        self.settings = Some(u);
        open_tab(self, id);
    }

    /// Closes the settings dialog and the user-area modals, dropping their `m.` form fields.
    pub fn close_account_modals(&mut self) {
        if let Some(u) = self.settings.as_mut() {
            u.modal = None;
        }
        self.account.modal = None;
        self.account_gate.recovery_kit = false;
        // Dropping a pending PIN reply ends that key request.
        self.account.passkey_pin = None;
        self.form.text.retain(|k, _| !k.starts_with("m."));
        self.form.data.retain(|k, _| !k.starts_with("m."));
        self.form.error = None;
    }

    /// Opens a settings dialog with fresh `m.` fields.
    pub fn open_account_modal(&mut self, m: AccountModal) {
        self.close_account_modals();
        if let Some(u) = self.settings.as_mut() {
            u.modal = Some(m);
        }
    }

    /// The Profile tab on a community's profile ("Edit community profile").
    pub fn open_community_profile(&mut self, guild: Snowflake) {
        self.popups.close();
        self.settings = Some(SettingsUi { profile_guild: Some(guild), ..SettingsUi::new(Some("my_profile")) });
        open_tab(self, "my_profile");
    }
}

/// Shows tab `id` with a fresh form, loading what it lists.
pub(crate) fn open_tab(sh: &mut ShellState, id: &'static str) {
    let Some(u) = sh.settings.as_mut() else { return };
    (u.tab, u.modal) = (id, None);
    sh.form = Default::default();
    load_tab(sh, id);
}

/// Loads what tab `id` lists into the form.
pub(crate) fn load_tab(sh: &mut ShellState, id: &str) {
    match id {
        "account_security" => {
            sh.load("passkeys", "/users/@me/mfa/webauthn/credentials");
            recovery_kit::load(sh);
        }
        "devices" => sh.load("sessions", "/auth/sessions"),
        "authorized_apps" => sh.load("apps", "/oauth2/@me/authorizations"),
        "linked_accounts" => sh.load("connections", "/users/@me/connections"),
        "privacy_safety" => sh.load("harvest", "/users/@me/harvest/latest"),
        "my_profile" => profile::load(sh),
        "applications" => dev::load(sh),
        "gift_inventory" => match crate::shell::devtools::gifts(sh) {
            Some(mock) => drop(sh.form.data.insert("gifts".into(), mock)),
            None => sh.load("gifts", "/users/@me/gifts"),
        },
        _ => {}
    }
}

/// `--settings-*` colours of `.container`.
pub(crate) struct Colors {
    pub surface: Color,
    pub border: Color,
    pub hover: Color,
    pub selected: Color,
    pub danger: Color,
    pub danger_hover: Color,
}

pub(crate) fn colors(p: &Palette) -> Colors {
    Colors {
        surface: p.form_surface_background,
        border: p.background_modifier_accent,
        hover: p.surface_interactive_hover_bg,
        selected: p.surface_interactive_selected_bg,
        danger: mix(p.status_danger, p.text_primary, 78.0),
        danger_hover: mix(p.status_danger, p.surface_interactive_hover_bg, 6.0),
    }
}

/// The search field and profile card, and the nav.
fn sidebar<S: 'static>(store: &Store, st: &ShellState, ui: &SettingsUi, p: &Palette, premium: bool, hits: &[search::Hit], acc: ShellAccess<S>) -> (Vec<El>, El) {
    let c = colors(p);
    // `SettingsSearch`: 34 tall, a 16 bold magnifier 12 in, text 13/18.
    let search = row()
        .w_full()
        .h(34.0)
        .pl(36.0)
        .pr(16.0)
        .mb(10.0)
        .center_y()
        .radius(8.0)
        .fill(c.surface)
        .border(1.0, over(if ui.search.focused { mix(c.border, p.text_tertiary, 72.0) } else { c.border }, c.surface))
        .cursor(rune::Cursor::Text)
        .child(icon(&icons::MAGNIFYING_GLASS_BOLD, 16.0).color(p.text_tertiary).pin(Some(12.0), Some(8.0), None, None))
        .child(
            text_input(&ui.search, font(13.0, 400.0).lh(18.0))
                .single_line()
                .key("settings-search")
                .label(t("Search settings"))
                .grow(1.0)
                .shrink()
                .color(p.text_primary)
                .placeholder(t("Search settings…"), p.text_tertiary)
                .caret(p.text_primary, p.background_modifier_selected)
                .on_edit(move |s: &mut S| -> &mut TextEdit { &mut acc(s).settings.get_or_insert_with(|| SettingsUi::new(None)).search }),
        );

    // `.userProfile`: the profile card that opens the Profile tab.
    let me = store.me.as_ref();
    let name = me.map_or(String::new(), |u| u.base.global_name.clone().unwrap_or_else(|| u.base.username.clone()));
    let profile_sel = ui.tab == "my_profile";
    let hint = if profile_sel { p.text_secondary } else { p.text_tertiary };
    let mut profile = row()
        .w_full()
        .min_h(44.0)
        .px(9.0)
        .py(7.0)
        .mb(8.0)
        .gap(10.0)
        .center_y()
        .radius(8.0)
        .cursor(rune::Cursor::Pointer)
        .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
        .label(t("Edit your profile"))
        .hover_group()
        .on_press(select(acc, "my_profile"))
        .child(crate::shell::avatar(store, st, store.my_id(), 40.0, Some(crate::shell::presence(store, store.my_id())), if profile_sel { c.selected } else { c.surface }, p))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .mr(8.0)
                .child(text(name).font(font(15.0, 600.0).lh(20.0)).color(p.text_primary).truncate())
                .child(
                    row()
                        .gap(4.0)
                        .center_y()
                        .child(icon(&icons::PENCIL_BOLD, 12.0).color(hint).group_color(p.text_secondary))
                        .child(text(t("Edit your profile")).font(font(12.0, 500.0).lh(16.0)).color(hint).group_color(p.text_secondary).truncate()),
                ),
        );
    profile = if profile_sel { profile.fill(c.selected) } else { profile.fill(c.surface).hover(c.hover) };

    // Nav: categories of items, then Sign out, then the client info footer.
    let q = ui.searched.to_lowercase();
    let shown: Vec<&TabDef> = TABS
        .iter()
        .filter(|t| !NAV_HIDDEN.contains(&t.id) && visible(t, store, premium))
        .filter(|tab| q.is_empty() || hits.iter().any(|h| h.tab == tab.id))
        .collect();
    let mut nav = col().w_full().gap(3.0).focus_group().role("tablist");
    if shown.is_empty() {
        nav = nav.child(
            text(t("No settings found")).font(font(14.0, 400.0).lh(20.0)).color(p.text_tertiary).align(rune::Align::Center).w_full().py(16.0).px(8.0),
        );
    }
    for cat in [Cat::Account, Cat::Billing, Cat::App, Cat::Developer] {
        let items: Vec<&&TabDef> = shown.iter().filter(|t| t.cat == cat).collect();
        if items.is_empty() {
            continue;
        }
        let items = items.into_iter().map(|tab| nav_item(tab.icon, t(tab.label), ui.tab == tab.id, false, p, &c).on_press(select(acc, tab.id)));
        nav = nav.child(nav_group(Some(cat.label()), items, p));
    }
    // What's new, Submit feedback and Donate open the official site's pages in the browser;
    // self-hosted instances have none (`Routes.patchNotes` / `feedback` / `donate`).
    if q.is_empty()
        && let Some(base) = st.marketing_base.as_deref()
    {
        let base = base.trim_end_matches('/');
        let links = [
            (&icons::SPARKLE, "What's new", format!("{base}/patch-notes")),
            (&icons::CHAT_CIRCLE_DOTS, "Submit feedback", "https://feedback.fluxer.com".to_string()),
            (&icons::HAND_HEART, "Donate", format!("{base}/donate")),
        ];
        for (i, label, url) in links {
            nav = nav.child(nav_item(i, t(label), false, false, p, &c).label(format!("{} ({})", t(label), t("Opens in browser"))).on_press(move |_: &mut S| open_url(&url)));
        }
    }
    if q.is_empty() {
        nav = nav.child(nav_item(&icons::SIGN_OUT, t("Sign out"), false, true, p, &c).on_press(move |s: &mut S| set(acc, s, |u| u.confirm_logout = true)));
    }
    let footer = col()
        .w_full()
        .pt(16.0)
        .pb(28.0)
        .pl(12.0)
        .gap(2.0)
        .child(text(format!("Reflux {}", env!("CARGO_PKG_VERSION"))).font(font(11.0, 400.0).lh(14.0)).color(p.text_primary_muted))
        .child(text(format!("{} ({})", os_name(), std::env::consts::ARCH)).font(font(11.0, 400.0).lh(14.0)).color(p.text_primary_muted));
    (vec![search, profile], col().w_full().child(nav).child(footer))
}

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "linux" => "Linux",
        "macos" => "macOS",
        "windows" => "Windows",
        other => other,
    }
}

/// A nav category: its uppercase 11/600 title (if any) over its items.
pub(crate) fn nav_group(title: Option<&str>, items: impl IntoIterator<Item = El>, p: &Palette) -> El {
    let mut group = col().w_full().gap(3.0).mb(8.0);
    if let Some(title) = title {
        group = group.child(
            text(t(title))
                .font(font(11.0, 600.0).lh(16.0).tracking(0.22))
                .text_case(rune::TextCase::Uppercase)
                .color(p.text_tertiary)
                .pt(4.0)
                .px(10.0)
                .pb(3.0)
                .mb(1.0),
        );
    }
    group.children(items)
}

/// `.sidebarItem`: 34 tall, padding 5 10, a 20 icon, 13/500 label.
pub(crate) fn nav_item(i: &'static rune::Icon, label: &str, selected: bool, danger: bool, p: &Palette, c: &Colors) -> El {
    let label = t(label);
    let (fg, icon_fg, hover_fg) = if danger {
        (c.danger, c.danger, c.danger)
    } else if selected {
        (p.text_primary, p.text_primary, p.text_primary)
    } else {
        (p.text_primary_muted, p.text_tertiary, p.text_primary)
    };
    let item = row()
        .w_full()
        .min_h(34.0)
        .px(11.0)
        .py(6.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .cursor(rune::Cursor::Pointer)
        .role("tab").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
        .checked(selected)
        .label(label.to_string())
        .hover_group()
        .child(icon(i, 20.0).color(icon_fg).group_color(hover_fg))
        .child(text(label.to_string()).font(font(13.0, 500.0).lh(18.0)).color(fg).group_color(hover_fg).truncate().grow(1.0).shrink());
    match (selected, danger) {
        (true, false) => item.fill(c.selected),
        (_, true) => item.hover(c.danger_hover),
        _ => item.hover(c.hover),
    }
}

fn content<S: 'static>(title: &str, body: El, p: &Palette, m: &Metrics, close: impl Fn(&mut S) + 'static) -> El {
    let c = colors(p);
    let bg = if is_light(p) { p.background_primary } else { p.background_secondary };
    // `SettingsModalHeader`: 60 tall, padding 0 16, title 18/600, a 34 close button.
    let close = row()
        .size(34.0, 34.0)
        .center()
        .radius(8.0)
        .hover(c.hover)
        .hover_group()
        .cursor(rune::Cursor::Pointer)
        .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
        .label(t("Close"))
        .on_press(close)
        .child(icon(&icons::X_BOLD, 16.0).color(p.text_tertiary).group_color(p.text_primary));
    let header = row()
        .w_full()
        .h(61.0)
        .px(16.0)
        .pb(1.0)
        .center_y()
        .justify_between()
        .child(text(title.to_string()).font(font(18.0, 600.0).lh(24.0)).color(p.text_primary).truncate().grow(1.0).shrink().mr(16.0))
        .child(close)
        .child(el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(over(c.border, bg)));
    let inner = col().w_full().max_w(m.inner_max).px(m.gutter).child(el().h(20.0)).child(body).child(el().h(32.0));
    let viewport = col()
        .key("settings-content")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .child(row().w_full().justify_center().child(inner));
    col().grow(1.0).min_w(0.0).h_full().fill(bg).child(header).child(scroller(viewport, true, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_merges_known_fields_and_rejects_bad_types() {
        let mut s = reflux_model::UserSettings { theme: "dark".into(), ..Default::default() };
        assert!(apply(&mut s, &serde_json::json!({"theme": "coal", "developer_mode": true})));
        assert_eq!((s.theme.as_str(), s.developer_mode), ("coal", true));
        assert!(!apply(&mut s, &serde_json::json!({"developer_mode": "yes"})));
        assert!(s.developer_mode, "a rejected patch leaves the settings alone");
    }

    #[test]
    fn a_failed_patch_reverts_only_its_keys() {
        let mut s = reflux_model::UserSettings { theme: "dark".into(), ..Default::default() };
        let p = serde_json::json!({"theme": "coal"});
        let revert = revert_of(&s, &p);
        apply(&mut s, &p);
        apply(&mut s, &serde_json::json!({"developer_mode": true}));
        apply(&mut s, &revert);
        assert_eq!((s.theme.as_str(), s.developer_mode), ("dark", true));
    }

    #[test]
    fn metrics_at_1280x800() {
        let m = metrics(1280.0, 800.0);
        assert!((m.pad[0] - 20.0).abs() < 0.01 && (m.pad[1] - 76.8).abs() < 0.01, "{:?}", m.pad);
        assert!((m.card_w - 1124.4).abs() < 0.01 && (m.sidebar_w - 307.2).abs() < 0.01, "{}", m.card_w);
        assert_eq!(m.gutter, 48.0);
    }

    #[test]
    fn opens_on_known_tab_or_profile() {
        assert_eq!(SettingsUi::new(Some("themes")).tab, "themes");
        assert_eq!(SettingsUi::new(Some("nope")).tab, "my_profile");
    }
}
