//! The crash screen (`AppErrorBoundary` + `ErrorFallback`): a panic in the frame's poll or view
//! (caught with `catch_unwind`) or Developer tools' "Trigger React crash" (`RuntimeCrash`) replaces
//! the app with it. A panic anywhere else (a Rune handler, a background task) ends the process;
//! the panic hook saved its report, so the screen shows on the next launch.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use rune::{El, col, row, text};

use crate::i18n::t;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonVariant};

/// The last panic's report, from the hook.
static LAST: Mutex<Option<String>> = Mutex::new(None);
/// The report the crash screen shows (`RuntimeCrash.fatalError`).
static SHOWN: Mutex<Option<String>> = Mutex::new(None);
/// Whether the crash screen's first poll ran (`cleanupRuntimeStateOnCrash`, the update check).
static STARTED: AtomicBool = AtomicBool::new(false);
/// `checkingForUpdates` / `updateAvailable`: `None` while checking.
static UPDATE: Mutex<Option<bool>> = Mutex::new(None);

fn update_found() -> Option<bool> {
    *UPDATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn set_update_found(found: bool) {
    *UPDATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(found);
}

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

/// The crash screen's poll: once, leave voice and close the gateway (`cleanupRuntimeStateOnCrash`;
/// Reload starts them again) and check for updates (`ensureLatestAssets({force: true})`); then
/// restarts once "Update app" installed the update.
pub fn poll(s: &mut crate::State) {
    if !STARTED.swap(true, Ordering::Relaxed) {
        if s.shell_demo.is_none() {
            guard(|| crate::app::stop_for_crash(s));
        }
        match crate::desktop::updater::feed().filter(|_| !rune::is_headless()) {
            // No release feed: nothing to find, as when Fluxer's check finds no update.
            None => set_update_found(false),
            Some(url) => crate::app::spawn(async move {
                let found = match crate::desktop::updater::check(&url).await {
                    Ok(a) => {
                        let found = a.is_some();
                        crate::app::update(move |s| s.desktop.update = a.or(s.desktop.update.take()));
                        found
                    }
                    Err(e) => {
                        eprintln!("reflux: update check: {e}");
                        false
                    }
                };
                set_update_found(found);
                crate::app::update(|_| {});
            }),
        }
    }
    if s.desktop.update.as_ref().is_some_and(|u| u.stage == crate::desktop::updater::Stage::Installed) {
        s.desktop.update = None;
        crate::desktop::updater::restart();
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

/// `ErrorFallback`: "Reload app" and "Reset app data" wait for the update check; a found update
/// turns the first button into "Update app" (installs the update, else opens the release page).
pub fn view(report: &str, update: Option<&crate::desktop::updater::Available>, p: &Palette) -> El {
    use crate::desktop::updater::Stage;
    let report = report.to_string();
    let checking = update_found().is_none();
    let found = update_found() == Some(true) && update.is_some();
    let first = if checking || found { t("Update app") } else { t("Reload app") };
    let reload = Button::new(first)
        .key("crash-reload")
        .disabled(checking)
        .submitting(update.is_some_and(|u| u.stage == Stage::Downloading))
        .on_press_tx(move |s: &mut crate::State, tx| match s.desktop.update.as_ref().filter(|_| found) {
            Some(u) if u.package.is_some() && u.stage != Stage::Installed => crate::desktop::updater::install(s),
            Some(u) => {
                if let Err(e) = tx.open_url(&u.page) {
                    eprintln!("reflux: open {}: {e:?}", u.page);
                }
            }
            None => crate::desktop::updater::restart(),
        })
        .el(p);
    let copy = Button::new(t("Copy stack trace")).variant(ButtonVariant::Secondary).key("crash-copy").on_press_tx(move |_: &mut crate::State, tx| tx.copy(report.clone())).el(p);
    let reset = Button::new(t("Reset app data"))
        .variant(ButtonVariant::Danger)
        .key("crash-reset")
        .disabled(checking)
        .on_press(|_: &mut crate::State| {
            reset_app_data();
            crate::desktop::updater::restart();
        })
        .el(p);
    let description = if checking {
        t("The app has crashed. Checking for updates that might fix this issue…")
    } else if found {
        t("Something went wrong and the app crashed. An update is available that may fix this issue.")
    } else {
        t("Something went wrong and the app crashed. Try reloading or resetting the app.")
    };
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
                .child(text(description).font(font(16.0, 400.0)).color(p.text_primary_muted).max_w(336.0).align(rune::Align::Center).wrap()),
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
