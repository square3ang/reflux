//! User settings: Fluxer's full-window settings modal (`features/user/components/modals/
//! UserSettingsModal.tsx`, `app/components/dialogs/components/DesktopSettingsView.tsx`, styles in
//! `dialogs/shared/SettingsModalLayout.module.css`): a nav sidebar of tabs grouped by category with
//! Sign out at the bottom, and a content pane with a title bar and close button. Esc closes it.
//!
//! Views only queue [`Intent`]s; [`patch`] applies a settings change optimistically to the store
//! and sends `PATCH /users/@me/settings`.

pub(crate) mod tabs;

use reflux_store::Store;
use rune::{Color, El, ScrollSpec, Shadow, TextEdit, col, el, icon, row, text, text_input};
use serde_json::Value;

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
}

impl SettingsUi {
    /// Opens on `tab` (a [`TABS`] id), or on the first tab like Fluxer.
    pub fn new(tab: Option<&str>) -> Self {
        let tab = TABS.iter().find(|t| Some(t.id) == tab).map_or("my_profile", |t| t.id);
        Self { tab, search: TextEdit::new(""), reveal_email: false, confirm_logout: false }
    }
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
        match self {
            Cat::Account => "Your account",
            Cat::Billing => "Billing",
            Cat::App => "App settings",
            Cat::Developer => "Developer",
        }
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

fn visible(t: &TabDef, store: &Store, premium: bool) -> bool {
    match t.id {
        "embed_debugger" | "component_gallery" => store.settings.developer_mode,
        _ => t.cat != Cat::Billing || premium,
    }
}

fn label_of(id: &str) -> &'static str {
    TABS.iter().find(|t| t.id == id).map_or("User settings", |t| t.label)
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

/// Applies `patch` to the store now and sends it; a failed request restores the old settings.
pub fn patch(s: &mut crate::State, patch: Value) {
    let old = s.app.store.settings.clone();
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
                // shortcut: restores the snapshot from before this patch, dropping any later
                // optimistic change too; queue patches if that ever matters.
                s.app.store.settings = old;
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
    // `.layerFullscreen` padding and `.root.fullscreen` width.
    let pad = [clamp(8.0, 0.015 * h, 16.0), clamp(12.0, 0.02 * w, 24.0)];
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
    let (head, nav) = sidebar(store, st, ui, p, premium, acc);
    let body = tabs::body(store, st, ui, p, acc, &mut popups);
    let mut modal = frame("User settings", label_of(ui.tab), head, nav, body, close, p);
    if ui.confirm_logout {
        let cancel = move |s: &mut S| set(acc, s, |u| u.confirm_logout = false);
        modal = modal.child(
            Modal::new("Sign out?", cancel)
                .body(text("You can sign back in at any time.").font(font(14.0, 400.0).lh(19.6)).color(p.text_primary_muted).wrap())
                .footer(
                    row()
                        .gap(8.0)
                        .child(Button::new("Stay signed in").variant(ButtonVariant::Secondary).on_press(cancel).el(p))
                        .child(Button::new("Sign out").key("settings-sign-out").on_press(move |s: &mut S| acc(s).intents.push(Intent::Logout)).el(p)),
                )
                .el(p),
        );
    }
    layer.push(modal);
    layer.extend(popups);
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
    move |s: &mut S| set(acc, s, |u| u.tab = id)
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
fn sidebar<S: 'static>(store: &Store, st: &ShellState, ui: &SettingsUi, p: &Palette, premium: bool, acc: ShellAccess<S>) -> (Vec<El>, El) {
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
                .label("Search settings")
                .grow(1.0)
                .shrink()
                .color(p.text_primary)
                .placeholder("Search settings…", p.text_tertiary)
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
        .role("button")
        .label("Edit your profile")
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
                        .child(text("Edit your profile").font(font(12.0, 500.0).lh(16.0)).color(hint).group_color(p.text_secondary).truncate()),
                ),
        );
    profile = if profile_sel { profile.fill(c.selected) } else { profile.fill(c.surface).hover(c.hover) };

    // Nav: categories of items, then Sign out, then the client info footer.
    let q = ui.search.text.trim().to_lowercase();
    let shown: Vec<&TabDef> = TABS
        .iter()
        .filter(|t| !NAV_HIDDEN.contains(&t.id) && visible(t, store, premium))
        .filter(|t| q.is_empty() || t.label.to_lowercase().contains(&q))
        .collect();
    let mut nav = col().w_full().gap(3.0);
    if shown.is_empty() {
        nav = nav.child(
            text("No settings found").font(font(14.0, 400.0).lh(20.0)).color(p.text_tertiary).align(rune::Align::Center).w_full().py(16.0).px(8.0),
        );
    }
    for cat in [Cat::Account, Cat::Billing, Cat::App, Cat::Developer] {
        let items: Vec<&&TabDef> = shown.iter().filter(|t| t.cat == cat).collect();
        if items.is_empty() {
            continue;
        }
        let items = items.into_iter().map(|t| nav_item(t.icon, t.label, ui.tab == t.id, false, p, &c).on_press(select(acc, t.id)));
        nav = nav.child(nav_group(Some(cat.label()), items, p));
    }
    // shortcut: Fluxer also lists What's new, Submit feedback and Donate here; they open the
    // instance's web pages, and Reflux can't open URLs yet.
    if q.is_empty() {
        nav = nav.child(nav_item(&icons::SIGN_OUT, "Sign out", false, true, p, &c).on_press(move |s: &mut S| set(acc, s, |u| u.confirm_logout = true)));
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
    if let Some(t) = title {
        group = group.child(
            text(t.to_string())
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
        .role("tab")
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
        .role("button")
        .label("Close")
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
    fn metrics_at_1280x800() {
        let m = metrics(1280.0, 800.0);
        assert_eq!(m.pad, [12.0, 24.0]);
        assert!((m.card_w - 1226.8).abs() < 0.01 && (m.sidebar_w - 307.2).abs() < 0.01, "{}", m.card_w);
        assert_eq!(m.gutter, 48.0);
    }

    #[test]
    fn opens_on_known_tab_or_profile() {
        assert_eq!(SettingsUi::new(Some("themes")).tab, "themes");
        assert_eq!(SettingsUi::new(Some("nope")).tab, "my_profile");
    }
}
