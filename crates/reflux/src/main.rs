mod app;
mod compose;
mod desktop;
mod guild;
mod gst;
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
    let scale = cx.env().scale;
    s.app.shell.media.set_scale(scale);
    if let Some(d) = &s.shell_demo {
        d.shell.media.set_scale(scale);
    }
    let content = match (&s.gallery, &s.shell_demo) {
        (Some(g), _) => ui::gallery::view(g, cx),
        (_, Some(d)) => shell::demo::view(d),
        _ => app::view(s, cx),
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
    // The remembered size, and a launch at login with "Start minimized" stays in the tray.
    let prefs = if rune::is_headless() { desktop::Prefs::default() } else { session::load().desktop };
    let app = rune::App::new(AppSpec {
        fonts: theme::FONTS,
        families: theme::FAMILIES,
        scenes: ui::gallery::SCENES,
        poll: Some(|s, _, tx| match &mut s.shell_demo {
            Some(d) => shell::poll_shell(&d.store, &mut d.shell, tx),
            None => app::poll(s, tx),
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
    .on_focus(|s, focused, _| s.desktop.focused = focused);
    let mut app = desktop::menu::install(app);
    popout::register(&mut app);
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
