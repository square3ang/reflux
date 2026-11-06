mod theme;

use rune::{AppSpec, Cx, El, col, text};

#[derive(Default)]
struct State;

fn view(_: &State, _: &mut Cx<State>) -> El {
    let p = theme::palette(theme::Kind::Dark);
    col().w_full().h_full().center().fill(p.background_primary)
        .child(text("Welcome back").font(theme::font(20.0, 600.0)).color(p.text_primary))
}

fn main() {
    rune::run(AppSpec {
        fonts: theme::FONTS,
        families: theme::FAMILIES,
        ..AppSpec::new("Fluxer", (1280.0, 800.0), State::default, view)
    });
}
