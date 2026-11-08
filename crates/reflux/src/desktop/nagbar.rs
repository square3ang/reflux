//! The app's nagbars above the layout (`app_layout/nagbars/{ConnectionNagbar,
//! StreamerModeNagbar}.tsx`, the updater's "Desktop update available"), toasts
//! (`ui/toast/Toast.module.css`) and the composer's spelling menu.

use std::time::Duration;

use rune::{Color, El, Shadow, icon, row, text};

use super::updater::Stage;
use crate::i18n::{t, tf};
use crate::icons;
use crate::shell::{Intent, ShellAccess, ShellState};
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant, Menu, MenuItem, popup_access, rgb};

/// `MINIMUM_TIMEOUT` is 1.5 s; Fluxer's toasts mostly ask for 3 s.
pub const TOAST_TIME: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub struct Toast {
    pub text: String,
    pub success: bool,
    pub at: std::time::Instant,
}

/// `Nagbar` + `NagbarContent`: 36 tall, centred 14/600 message, super-compact inverted buttons
/// and a close button when dismissible.
type Handler<S> = Box<dyn Fn(&mut S)>;

pub(super) fn bar<S: 'static>(key: &str, bg: Color, message: String, actions: Vec<El>, dismiss: Option<Handler<S>>) -> El {
    let mut r = row()
        .key(key.to_string())
        .w_full()
        .min_h(36.0)
        .py(4.0)
        .px(8.0)
        .gap(12.0)
        .center()
        .fill(bg)
        .child(text(message).font(font(14.0, 600.0).lh(20.0)).color(Color::WHITE).wrap().align(rune::Align::Center))
        .children(actions)
        .role("status");
    if let Some(f) = dismiss {
        let close = row()
            .key(format!("{key}-dismiss"))
            .pin(None, Some(6.0), Some(8.0), None)
            .size(24.0, 24.0)
            .center()
            .radius(4.0)
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(t("Dismiss"))
            .child(icon(&icons::X_BOLD, 16.0).color(Color::WHITE))
            .on_press(f);
        r = r.child(close);
    }
    r
}

pub(super) fn button<S: 'static>(key: &str, label: &str, p: &Palette, f: impl Fn(&mut S) + 'static) -> El {
    Button::new(label.to_string()).variant(ButtonVariant::Inverted).size(ButtonSize::SuperCompact).key(key.to_string()).on_press(f).el(p)
}

fn prefs<S: 'static>(acc: ShellAccess<S>, f: impl Fn(&mut super::Prefs) + 'static) -> impl Fn(&mut S) + 'static {
    move |s: &mut S| {
        let sh = acc(s);
        let mut p = sh.prefs.clone();
        f(&mut p);
        sh.intents.push(Intent::Prefs(Box::new(p)));
    }
}

/// The bars, top to bottom.
pub fn bars<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>) -> Vec<super::top_bars::Bar> {
    use super::top_bars::Bar;
    let v = &st.desktop;
    let mut out = Vec::new();
    // `forceConnectionNotice` (Developer tools' "Service incident nagbar").
    if crate::shell::devtools::nagbar(st, "forceIncident").unwrap_or(v.offline) {
        out.push(Bar::new(-100.0, false, bar::<S>("nagbar-connection", rgb(0x4e, 0x50, 0x58), t("Connection lost. Reconnecting…").into(), Vec::new(), None)));
    }
    if let Some(u) = &v.update {
        let product = [("productName", "Fluxer")];
        let page = u.page.clone();
        let open = move |s: &mut S| acc(s).intents.push(Intent::Desktop(super::Request::OpenUrl(page.clone())));
        let (message, actions) = match (u.stage, u.appimage.is_some()) {
            (Stage::Installed, _) => (tf("{productName} closes, installs the update and opens again.", &product), vec![button("nagbar-update-restart", t("Restart now"), p, move |s: &mut S| acc(s).intents.push(Intent::Desktop(super::Request::Restart)))]),
            (Stage::Failed, _) => (t("Couldn't download the desktop update").into(), vec![button("nagbar-update-open", t("Open desktop downloads"), p, open)]),
            (Stage::Downloading, _) => (
                t("Desktop update available").into(),
                vec![Button::<S>::new(t("Update now")).variant(ButtonVariant::Inverted).size(ButtonSize::SuperCompact).submitting(true).key("nagbar-update").el(p)],
            ),
            (Stage::Available, true) => (t("Desktop update available").into(), vec![button("nagbar-update", t("Update now"), p, move |s: &mut S| acc(s).intents.push(Intent::Desktop(super::Request::Update)))]),
            (Stage::Available, false) => (t("Desktop update available").into(), vec![button("nagbar-update-open", t("Download installer"), p, open)]),
        };
        let version = u.version.clone();
        let dismiss = prefs(acc, move |p| p.skipped_update = Some(version.clone()));
        // The desktop update bar is Reflux's own: it stays outside Fluxer's pick, like its
        // build-environment bar.
        out.push(Bar { pinned: true, ..Bar::new(-1000.0, true, bar("nagbar-update", p.brand_primary, format!("{message} ({})", u.version), actions, Some(Box::new(dismiss)))) });
    }
    // `SoftwareEncoderNagbar` (`NagbarToneKind.ENCODER`, priority -2).
    if let Some(codec) = st.voice.software_encoder {
        let dismiss = move |s: &mut S| acc(s).voice.software_encoder = None;
        let forever = move |s: &mut S| {
            let sh = acc(s);
            sh.voice.software_encoder = None;
            let mut p = sh.prefs.clone();
            p.dismissed_nagbars.push("software-encoder".into());
            sh.intents.push(Intent::Prefs(Box::new(p)));
        };
        let msg = tf(
            "Screen share is using software encoding ({codec}). Quality may be reduced and CPU usage increased. Your GPU may not support hardware encoding for this codec.",
            &[("codec", codec)],
        );
        out.push(Bar::new(-2.0, true, bar(
            "nagbar-software-encoder",
            rgb(0xb4, 0x53, 0x09),
            msg,
            vec![button("nagbar-encoder-dismiss", t("Dismiss"), p, dismiss), button("nagbar-encoder-forever", t("Don't show again"), p, forever)],
            Some(Box::new(dismiss)),
        )));
    }
    if v.streamer && !st.prefs.streamer.nagbar_dismissed {
        let off = prefs(acc, |p| {
            (p.streamer.manual, p.streamer.auto, p.streamer.nagbar_dismissed) = (false, false, false);
        });
        let dismiss = prefs(acc, |p| p.streamer.nagbar_dismissed = true);
        out.push(Bar::new(-2.5, true, bar(
            "nagbar-streamer",
            rgb(0x58, 0x65, 0xf2),
            t("Streaming privacy is on. Names are shortened.").into(),
            vec![button("nagbar-streamer-off", t("Turn off"), p, off)],
            Some(Box::new(dismiss)),
        )));
    }
    out
}

/// Toasts (top centre, pill, icon + 16/600 text) and the spelling menu, into the overlay layer.
pub fn layer<S: 'static>(st: &ShellState, p: &Palette, acc: ShellAccess<S>, layer: &mut Vec<El>) {
    if let Some(t) = st.desktop.toasts.last() {
        let (ic, color) = if t.success { (&icons::CHECK_BOLD, p.status_online) } else { (&icons::X_BOLD, p.status_danger) };
        let pill = row()
            .key("toast")
            .gap(12.0)
            .px(20.0)
            .py(12.0)
            .center_y()
            .radius(9999.0)
            .fill(p.background_primary)
            .border(1.0, p.background_modifier_accent)
            .box_shadow(Shadow { x: 0.0, y: 10.0, blur: 15.0, spread: -3.0, color: crate::ui::rgba(0, 0, 0, 0.1), inset: false })
            .cursor(rune::Cursor::Pointer)
            .role("status")
            .child(icon(ic, 20.0).color(color))
            .child(text(t.text.clone()).font(font(16.0, 600.0).lh(22.0)).color(p.text_primary))
            .on_press(move |s: &mut S| acc(s).desktop.toasts.clear());
        layer.push(row().pin(Some(0.0), Some(24.0), Some(0.0), None).justify_center().items_start().child(pill));
    }
    if !st.popups.is_open("spell-menu") {
        return;
    }
    let mut m = Menu::new("spell-menu");
    let spell = st.desktop.spell_menu.clone();
    if let Some((range, word)) = spell.clone() {
    let suggestions = super::spell::suggest(&word);
    if suggestions.is_empty() {
        m = m.item(MenuItem::new("spell-none", t("No spelling suggestions")).disabled(true));
    }
    for (i, sug) in suggestions.into_iter().enumerate() {
        let r = range.clone();
        m = m.item(MenuItem::new(format!("spell-{i}"), sug.clone()).on_select(move |s: &mut S| {
            let sh = acc(s);
            // The text may have changed since the menu opened.
            let d = crate::compose::menu_draft(sh);
            if d.text.get(r.clone()).is_some() {
                d.set_selection(r.start, r.end);
                d.insert(&sug);
            }
            sh.desktop.spell_menu = None;
            sh.popups.close();
        }));
    }
    let add = word.clone();
    m = m.separator().item(MenuItem::new("spell-add", t("Add to dictionary")).on_select(move |s: &mut S| {
        let sh = acc(s);
        let mut p = sh.prefs.clone();
        if !p.words.contains(&add) {
            p.words.push(add.clone());
        }
        sh.intents.push(Intent::Prefs(Box::new(p)));
        super::toast(&mut sh.desktop, tf("Added \"{word}\" to dictionary", &[("word", &add)]), true);
        sh.desktop.spell_menu = None;
        sh.popups.close();
    }));
    }
    let m = crate::compose::edit_items(st, m, spell.is_some(), acc);
    m.el(&st.popups, popup_access(move |s: &mut S| &mut acc(s).popups), p, layer);
}

/// The composer's right click (`TextareaContextMenu`): the spelling items for the misspelling
/// at (or before) the caret, then the edit groups. `pane`: the thread panel's composer.
pub fn open_spell_menu(sh: &mut ShellState, at: [f32; 2], pane: bool) -> bool {
    sh.desktop.menu_pane = pane;
    let d = if pane { &sh.threads.composer } else { &sh.composer };
    let (caret, _) = d.range();
    sh.desktop.spell_menu = if sh.prefs.spellcheck { super::spell::pick(&d.text, caret) } else { None };
    sh.popups.open_at("spell-menu", at);
    true
}
