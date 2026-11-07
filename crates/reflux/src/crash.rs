//! The crash screen (`AppErrorBoundary` + `ErrorFallback`): a panic in the frame's poll or view
//! (caught with `catch_unwind`) or Developer tools' "Trigger React crash" (`RuntimeCrash`) replaces
//! the app with it. A panic anywhere else (a Rune handler, a background task) ends the process;
//! the panic hook saved its report, so the screen shows on the next launch.

use std::sync::Mutex;

use rune::{El, col, row, text};

use crate::i18n::t;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant};

/// The last panic's report, from the hook.
static LAST: Mutex<Option<String>> = Mutex::new(None);
/// The report the crash screen shows (`RuntimeCrash.fatalError`).
static SHOWN: Mutex<Option<String>> = Mutex::new(None);

fn file() -> Option<std::path::PathBuf> {
    Some(crate::session::dir()?.join("crash.txt"))
}

/// Records each panic (message, location, backtrace) and saves it for the next launch; then a
/// saved report from the last run shows.
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let report = format!("{info}\n\n{}", std::backtrace::Backtrace::force_capture());
        if let Some(f) = file() {
            let _ = std::fs::write(f, &report);
        }
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(report);
        default(info);
    }));
    if let Some(f) = file()
        && let Ok(r) = std::fs::read_to_string(&f)
    {
        let _ = std::fs::remove_file(f);
        show(r);
    }
}

/// `RuntimeCrash.triggerFatalCrash`: the first report stays.
pub fn show(report: String) {
    SHOWN.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(report);
}

pub fn shown() -> Option<String> {
    SHOWN.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Runs `f`; a panic becomes the crash screen (its report no longer waits for the next launch).
pub fn guard<T>(f: impl FnOnce() -> T) -> Option<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => Some(v),
        Err(_) => {
            if let Some(f) = file() {
                let _ = std::fs::remove_file(f);
            }
            let report = LAST.lock().unwrap_or_else(|e| e.into_inner()).take();
            show(report.unwrap_or_else(|| t("Unknown error").into()));
            None
        }
    }
}

/// `resetClientState(RESET_APP_DATA)`: signs every account out and drops the saved session,
/// keeping the instance (Reflux keeps no drafts on disk).
fn reset_app_data() {
    let mut s = crate::session::load();
    for a in s.current.iter().chain(&s.accounts) {
        crate::app::cache::forget(a);
    }
    while !s.accounts.is_empty() {
        s.remove_account(0);
    }
    crate::session::save(&crate::session::Saved { instance: s.instance, installation_id: s.installation_id, ..Default::default() });
}

/// `ErrorFallback`.
pub fn view<S: 'static>(report: &str, p: &Palette) -> El {
    // shortcut: no `ensureLatestAssets` check (the desktop updater bar handles updates), so the screen starts at "Reload app"; port it if Reflux gets hot asset updates.
    let report = report.to_string();
    let reload = Button::new(t("Reload app")).key("crash-reload").on_press(|_: &mut S| crate::desktop::updater::restart()).el(p);
    let copy = Button::new(t("Copy stack trace")).variant(ButtonVariant::Secondary).key("crash-copy").on_press_tx(move |_: &mut S, tx| tx.copy(report.clone())).el(p);
    let reset = Button::new(t("Reset app data"))
        .variant(ButtonVariant::Danger)
        .key("crash-reset")
        .on_press(|_: &mut S| {
            reset_app_data();
            crate::desktop::updater::restart();
        })
        .el(p);
    col()
        .grow(1.0)
        .fill(p.bg_primary)
        .items_center()
        .justify_center()
        .gap(32.0)
        .px(16.0).py(16.0)
        .child(crate::login::logo(96.0))
        .child(
            col()
                .items_center()
                .gap(12.0)
                .child(text(t("The app crashed")).font(font(30.0, 600.0).lh(36.0)).color(p.text_primary))
                .child(text(t("Something went wrong and the app crashed. Try reloading or resetting the app.")).font(font(16.0, 400.0)).color(p.text_primary_muted).max_w(336.0).align(rune::Align::Center).wrap()),
        )
        .child(row().gap(12.0).child(reload).child(copy).child(reset))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_caught_panic_shows_and_the_first_report_stays() {
        assert!(super::guard(|| 1).is_some());
        super::show("first".into());
        super::show("second".into());
        assert_eq!(super::shown().as_deref(), Some("first"));
    }
}
