//! The Quick CSS editor in its own window (`QuickCssEditorHost` placed in `'window'`, opened
//! with "Open in a new window"): the CSS, Save, and "Move back into Fluxer". Saving goes to
//! the main window's look preferences like the in-tab editor's Save.
// shortcut: no CSS completions or syntax colours (Fluxer's CodeMirror workspace); a plain mono
// text area, as the in-tab editor is.

use std::cell::RefCell;

use rune::{AppHandle, AppSpec, Cx, El, TextEdit, WindowHandle, WindowOptions, WindowSpec, col, row, text, text_input};

use crate::State;
use crate::i18n::t;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant};

#[derive(Default)]
pub struct Editor {
    edit: TextEdit,
    /// The main window's palette when it opened.
    palette: Option<Palette>,
}

thread_local! {
    static WINDOW: RefCell<Option<(AppHandle<Editor>, WindowHandle)>> = const { RefCell::new(None) };
}

/// `EDITOR_WINDOW_WIDTH` × `EDITOR_WINDOW_HEIGHT`.
const SIZE: (f32, f32) = (1100.0, 760.0);

/// Adds the (hidden) editor window to the app.
pub fn register(app: &mut rune::App<State>) {
    let spec = AppSpec { fonts: crate::theme::FONTS, families: crate::theme::FAMILIES, min_size: Some((420.0, 300.0)), ..AppSpec::new("Quick CSS", SIZE, Editor::default, view) };
    let options = WindowOptions { visible: false, ..Default::default() };
    let handles = app.window(WindowSpec::new("quick-css", spec).options(options));
    WINDOW.with(|w| *w.borrow_mut() = Some(handles));
}

/// Opens (or brings forward) the editor on `css`, drawn in `p`.
pub fn open(css: &str, p: &Palette) {
    let (css, p) = (css.to_string(), *p);
    WINDOW.with(|w| {
        let Some((app, win)) = &*w.borrow() else { return };
        app.update(move |e, tx| {
            tx.set_title(t("Quick CSS"));
            (e.edit, e.palette) = (TextEdit::new(css), Some(p));
            e.edit.focused = true;
        });
        win.open();
        win.focus();
    });
}

fn hide() {
    WINDOW.with(|w| {
        if let Some((_, win)) = &*w.borrow() {
            win.hide();
        }
    });
}

/// Hands the text to the main window, which saves it as the in-tab Save does.
fn save(css: String) {
    crate::app::update(move |s| {
        let sh = &mut s.app.shell;
        let mut prefs = sh.prefs.clone();
        prefs.look.css = css;
        sh.form.text.remove("quick-css");
        sh.intents.push(crate::shell::Intent::Prefs(Box::new(prefs)));
    });
}

fn view(e: &Editor, _cx: &mut Cx<Editor>) -> El {
    let p = e.palette.as_ref().unwrap_or_else(|| crate::theme::palette(crate::theme::Kind::Dark));
    let line = || rune::el().w_full().h(1.0).fill(p.background_modifier_accent);
    let header = row()
        .w_full()
        .h(48.0)
        .px(16.0)
        .gap(8.0)
        .center_y()
        .child(text(t("Quick CSS")).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).grow(1.0))
        .child(
            Button::new(t("Move back into Fluxer"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Small)
                .icon(&crate::icons::ARROW_SQUARE_IN)
                .on_press(|_: &mut Editor| hide())
                .el(p),
        )
        .child(
            Button::new(t("Save"))
                .size(ButtonSize::Small)
                .key("quick-css-window-save")
                .on_press(|e: &mut Editor| save(e.edit.text.to_string()))
                .el(p),
        );
    let editor = text_input(&e.edit, font(13.0, 400.0).mono().lh(18.0))
        .key("quick-css-window")
        .w_full()
        .h_full()
        .color(p.text_primary)
        .caret(p.text_primary, p.background_modifier_selected)
        .placeholder(":root { --text-link: #00a8fc; }", p.text_tertiary)
        .on_edit(|e: &mut Editor| -> &mut TextEdit { &mut e.edit });
    let (applied, ignored) = crate::theme::custom::overrides(&e.edit.text);
    let note = crate::i18n::tf(
        "Reflux applies only theme variable overrides inside :root {…} ({applied} applied, {ignored} ignored).",
        &[("applied", &applied.len().to_string()), ("ignored", &ignored.to_string())],
    );
    col()
        .w_full()
        .h_full()
        .fill(p.background_primary)
        .child(header)
        .child(line())
        .child(col().w_full().grow(1.0).min_h(0.0).padding(16.0).child(editor))
        .child(line())
        .child(row().w_full().px(16.0).py(8.0).child(text(note).font(font(12.0, 400.0).lh(16.0)).color(if ignored > 0 { p.status_warning } else { p.text_tertiary }).wrap()))
}
