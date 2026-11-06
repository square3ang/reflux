mod icons;
mod login;
mod theme;
#[allow(dead_code, unused_imports)] // the kit exposes variants the screens don't use yet
mod ui;

use rune::{AppSpec, Cx, El};

#[derive(Default)]
struct State {
    login: login::Login,
    /// Dev-only component gallery (`--gallery`).
    gallery: Option<ui::gallery::Gallery>,
}

fn init() -> State {
    let gallery = rune::extra_args().iter().any(|a| a == "--gallery").then(ui::gallery::Gallery::default);
    State { gallery, ..State::default() }
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    match &s.gallery {
        Some(g) => ui::gallery::view(g, cx),
        None => login::view(s, cx),
    }
}

fn main() {
    rune::run(AppSpec {
        fonts: theme::FONTS,
        families: theme::FAMILIES,
        scenes: ui::gallery::SCENES,
        setup: Some(ui::gallery::setup),
        ..AppSpec::new("Fluxer", (1280.0, 800.0), init, view)
    });
}
