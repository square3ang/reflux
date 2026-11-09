mod app;
mod audio_share;
mod compose;
mod crash;
mod desktop;
mod file_picker;
mod guild;
mod camera;
mod gst;
#[cfg(windows)]
mod mf;
mod handoff;
mod i18n;
mod keybinds;
mod look;
mod icons;
mod login;
mod media;
mod nav;
mod passkey;
mod popout;
mod quick_css;
mod quick_css_syntax;
mod restrict;
mod session;
mod settings;
mod shell;
mod synced;
mod social;
mod theme;
#[allow(dead_code, unused_imports)] // the kit exposes variants the screens don't use yet
mod ui;
mod voice_input;

use rune::{AppSpec, Cx, El};

#[derive(Default)]
struct State {
    login: login::Login,
    app: app::App,
    desktop: desktop::Desktop,
    /// Dev-only component gallery (`--gallery`).
    gallery: Option<ui::gallery::Gallery>,
    /// Dev-only shell over a fixture store (`--shell-demo`).
    shell_demo: Option<shell::demo::Demo>,
}

fn init() -> State {
    // Headless snapshots stay English unless asked (`REFLUX_LOCALE=ko`); `app::boot` picks the
    // saved or system language in a real run.
    if let Ok(l) = std::env::var("REFLUX_LOCALE") {
        i18n::set_locale(&l);
    }
    // Headless zoom checks (`REFLUX_ZOOM=1.5`); a real run takes the saved zoom in `app::boot`.
    if let Some(z) = std::env::var("REFLUX_ZOOM").ok().and_then(|z| z.parse().ok()) {
        desktop::window::set_zoom(z);
    }
    let gallery = rune::extra_args().iter().any(|a| a == "--gallery").then(ui::gallery::Gallery::screen);
    let shell_demo = rune::extra_args().iter().any(|a| a == "--shell-demo").then(shell::demo::Demo::new);
    State { gallery, shell_demo, ..State::default() }
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    // Popups left by a view that bailed (a crash) don't belong to this frame.
    ui::reset_late();
    let scale = cx.env().scale;
    s.app.shell.media.set_scale(scale);
    if let Some(d) = &s.shell_demo {
        d.shell.media.set_scale(scale);
    }
    let sh = s.shell_demo.as_ref().map_or(&s.app.shell, |d| &d.shell);
    let (a, b) = sh.composer.range();
    compose::CARET.set(if sh.composer.focused && a != b { cx.caret("composer") } else { None });
    quick_css_syntax::CARET.set(if sh.form.text.get("quick-css").is_some_and(|e| e.focused) { cx.caret("quick-css") } else { None });
    let content = crash::shown().is_none().then(|| {
        crash::guard(|| match (&s.gallery, &s.shell_demo) {
            (Some(g), _) => ui::gallery::view(g, cx),
            (_, Some(d)) => shell::demo::view(d),
            _ => app::view(s, cx),
        })
    });
    let content = match content.flatten() {
        Some(c) => c,
        None => {
            let look = &s.app.saved.desktop.look;
            let p = theme::custom::effective(look.theme(&s.app.store.settings.theme), cx.system_dark(), look.saturation, &look.theme_css());
            crash::view(&crash::shown().unwrap_or_default(), s.desktop.update.as_ref(), &p)
        }
    };
    desktop::window::root(content, s.desktop.size)
}

fn main() {
    // `--import-fluxer-session`: copy the official desktop app's session into ours and exit.
    if std::env::args().any(|a| a == "--import-fluxer-session") {
        let db = session::fluxer_desktop_db().expect("config dir");
        match session::import_fluxer_desktop(&db) {
            Ok(s) => {
                session::save(&s);
                println!("Imported the Fluxer desktop session for {}.", s.instance);
            }
            Err(e) => {
                eprintln!("reflux: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if !rune::is_headless() {
        crash::install();
    }
    // The remembered size, and a launch at login with "Start minimized" stays in the tray.
    let prefs = if rune::is_headless() { desktop::Prefs::default() } else { session::load().desktop };
    let app = rune::App::new(AppSpec {
        fonts: theme::app_fonts(),
        families: theme::FAMILIES,
        scenes: ui::gallery::SCENES,
        poll: Some(|s: &mut State, _, tx| {
            ui::misc::retarget_wheels(tx);
            if crash::shown().is_some() {
                crash::poll(s);
                return false;
            }
            crash::guard(|| match &mut s.shell_demo {
                Some(d) => {
                    let older = d.serve_older(tx);
                    let resized = d.shell.desktop.width != s.desktop.size[0];
                    d.shell.desktop.width = s.desktop.size[0];
                    shell::poll_shell(&d.store, &mut d.shell, tx) || resized || older
                }
                None => app::poll(s, tx),
            })
            .unwrap_or(true)
        }),
        on_key: Some(keybinds::on_key),
        setup: Some(|s, waker| {
            ui::gallery::setup(s, waker.clone());
            s.app.shell.default_avatars = shell::load_default_avatars(waker.clone());
            let live = app::live::scene().is_some();
            s.app.shell.media.setup(waker.clone(), live.then_some(app::live::fetch_image as media::Fixture));
            shell::demo::setup(s, waker);
            app::live::wait(s);
        }),
        ..AppSpec::new("Fluxer", desktop::window::initial_size(&prefs, (1280.0, 800.0)), init, view)
    })
    .keep_running()
    .modifier_keys()
    .on_close(desktop::on_close)
    .on_focus(|s, focused, _| {
        s.desktop.focused = focused;
        s.app.shell.desktop.unfocused = !focused;
    })
    .on_window(desktop::window::on_window);
    let mut app = desktop::menu::install(app);
    quick_css::register(&mut app);
    if desktop::autostart::start_hidden(&prefs) && !rune::is_headless() {
        app = app.windowless();
    }
    // Headless snapshots stay offline and show the logged-out screens, unless `--live`.
    if app::live::scene().is_some() {
        reflux_net::force_readonly();
        app::live::capture();
        app::start(app.handle());
    } else if !rune::is_headless() && !rune::extra_args().iter().any(|a| a == "--gallery" || a == "--shell-demo") {
        desktop::start(&app.handle());
        app::start(app.handle());
    }
    app.run();
}
