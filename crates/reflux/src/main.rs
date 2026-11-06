mod app;
mod icons;
mod login;
mod session;
mod settings;
mod shell;
mod theme;
#[allow(dead_code, unused_imports)] // the kit exposes variants the screens don't use yet
mod ui;

use rune::{AppSpec, Cx, El};

#[derive(Default)]
struct State {
    login: login::Login,
    app: app::App,
    /// Dev-only component gallery (`--gallery`).
    gallery: Option<ui::gallery::Gallery>,
    /// Dev-only shell over a fixture store (`--shell-demo`).
    shell_demo: Option<shell::demo::Demo>,
}

fn init() -> State {
    let gallery = rune::extra_args().iter().any(|a| a == "--gallery").then(ui::gallery::Gallery::default);
    let shell_demo = rune::extra_args().iter().any(|a| a == "--shell-demo").then(shell::demo::Demo::new);
    State { gallery, shell_demo, ..State::default() }
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    match (&s.gallery, &s.shell_demo) {
        (Some(g), _) => ui::gallery::view(g, cx),
        (_, Some(d)) => shell::demo::view(d),
        _ => app::view(s, cx),
    }
}

fn main() {
    let app = rune::App::new(AppSpec {
        fonts: theme::FONTS,
        families: theme::FAMILIES,
        scenes: ui::gallery::SCENES,
        poll: Some(|s, _, _| app::poll(s)),
        setup: Some(|s, waker| {
            ui::gallery::setup(s, waker.clone());
            s.app.shell.default_avatars = shell::load_default_avatars(waker.clone());
            shell::demo::setup(s, waker);
        }),
        ..AppSpec::new("Fluxer", (1280.0, 800.0), init, view)
    });
    // Headless snapshots stay offline and show the logged-out screens.
    if !rune::is_headless() && !rune::extra_args().iter().any(|a| a == "--gallery" || a == "--shell-demo") {
        app::start(app.handle());
    }
    app.run();
}
