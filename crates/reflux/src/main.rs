mod icons;
mod login;
mod theme;
#[allow(dead_code, unused_imports)] // the kit exposes variants the screens don't use yet
mod ui;

use rune::{AppSpec, Cx, El};

#[derive(Default)]
struct State {
    login: login::Login,
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    login::view(s, cx)
}

fn main() {
    rune::run(AppSpec {
        fonts: theme::FONTS,
        families: theme::FAMILIES,
        ..AppSpec::new("Fluxer", (1280.0, 800.0), State::default, view)
    });
}
