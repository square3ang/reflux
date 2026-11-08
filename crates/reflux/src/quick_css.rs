//! The Quick CSS editor in its own window (`QuickCssEditorHost` placed in `'window'`, opened
//! with "Open in a new window"): the CSS, Save, and "Move back into Fluxer". Saving goes to
//! the main window's look preferences like the in-tab editor's Save.
//! Syntax colours and variable completions come from [`crate::quick_css_syntax`].

use std::cell::RefCell;

use rune::{AppHandle, AppSpec, Cx, El, TextEdit, WindowHandle, WindowOptions, WindowSpec, col, row, text, text_input};

use crate::State;
use crate::i18n::t;
use crate::theme::{Palette, font};
use crate::ui::{Button, ButtonSize, ButtonVariant};

#[derive(Default)]
pub struct Editor {
    edit: TextEdit,
    /// A library theme's ID and name when editing that theme (`openQuickCssEditor(themeId)`),
    /// and whether it follows a file (View CSS: no Save).
    theme: Option<(String, String, bool)>,
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

/// Opens (or brings forward) the editor on the quick CSS `css`, drawn in `p`.
pub fn open(css: &str, p: &Palette) {
    show(css, None, p);
}

/// Opens the editor on a library theme's CSS (read-only for a linked file).
pub fn open_theme(th: &crate::look::LibraryTheme, read_only: bool, p: &Palette) {
    show(&th.css, Some((th.id.clone(), th.name.clone(), read_only)), p);
}

fn show(css: &str, theme: Option<(String, String, bool)>, p: &Palette) {
    let (css, p) = (css.to_string(), *p);
    WINDOW.with(|w| {
        let Some((app, win)) = &*w.borrow() else { return };
        app.update(move |e, tx| {
            tx.set_title(theme.as_ref().map_or_else(|| t("Quick CSS").to_string(), |th| th.1.clone()));
            (e.edit, e.palette, e.theme) = (TextEdit::new(css), Some(p), theme);
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

/// Hands the text to the main window, which saves it as the in-tab Save does (or into the
/// library theme, `updateThemeDetails` with the CSS).
fn save(css: String, theme: Option<String>) {
    crate::app::update(move |s| {
        let sh = &mut s.app.shell;
        let mut prefs = sh.prefs.clone();
        match theme {
            Some(id) => {
                let Some(th) = prefs.look.themes.iter_mut().find(|x| x.id == id) else { return };
                (th.css, th.updated_at) = (css, crate::app::now_ms() as i64);
                crate::desktop::toast(&mut sh.desktop, t("Theme saved."), true);
            }
            None => {
                prefs.look.css = css;
                sh.form.text.remove("quick-css");
            }
        }
        sh.intents.push(crate::shell::Intent::Prefs(Box::new(prefs)));
    });
}

fn view(e: &Editor, cx: &mut Cx<Editor>) -> El {
    let p = e.palette.as_ref().unwrap_or_else(|| crate::theme::palette(crate::theme::Kind::Dark));
    let line = || rune::el().w_full().h(1.0).fill(p.background_modifier_accent);
    let header = row()
        .w_full()
        .h(48.0)
        .px(16.0)
        .gap(8.0)
        .center_y()
        .child(text(e.theme.as_ref().map_or_else(|| t("Quick CSS").to_string(), |th| th.1.clone())).font(font(16.0, 600.0).lh(20.0)).color(p.text_primary).truncate().grow(1.0).shrink())
        .child(
            Button::new(t("Move back into Fluxer"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Small)
                .icon(&crate::icons::ARROW_SQUARE_IN_FILL)
                .on_press(|_: &mut Editor| hide())
                .el(p),
        )
        .children((!e.theme.as_ref().is_some_and(|th| th.2)).then(|| {
            Button::new(t("Save"))
                .size(ButtonSize::Small)
                .key("quick-css-window-save")
                .on_press(|e: &mut Editor| save(e.edit.text.to_string(), e.theme.as_ref().map(|th| th.0.clone())))
                .el(p)
        }));
    let editor = text_input(&e.edit, font(13.0, 400.0).mono().lh(18.0))
        .key("quick-css-window")
        .w_full()
        .h_full()
        .color(p.text_primary)
        .caret(p.text_primary, p.background_modifier_selected)
        .placeholder(":root { --text-link: #00a8fc; }", p.text_tertiary)
        .decorations(crate::quick_css_syntax::spans(&e.edit.text, p))
        .on_input_key(|e: &mut Editor, k, _| !e.theme.as_ref().is_some_and(|th| th.2) && crate::quick_css_syntax::key(&mut e.edit, 1, k));
    let read_only = e.theme.as_ref().is_some_and(|th| th.2);
    let completions = crate::quick_css_syntax::list(&e.edit, 1).filter(|_| !read_only).zip(cx.caret("quick-css-window")).map(|(l, c)| {
        let at = rune::el().key("quick-css-window-caret").absolute(c[0], c[1] + c[3]).size(1.0, 1.0);
        let list = crate::quick_css_syntax::popup(&l, 1, p, |e: &mut Editor, f: &mut dyn FnMut(&mut TextEdit)| f(&mut e.edit)).anchored("quick-css-window-caret", rune::Side::Bottom, rune::Align::Start, 2.0);
        [at, list]
    });
    let editor = if e.theme.as_ref().is_some_and(|th| th.2) { editor } else { editor.on_edit(|e: &mut Editor| -> &mut TextEdit { &mut e.edit }) };
    let (applied, ignored) = crate::theme::custom::overrides(&e.edit.text);
    let note = crate::i18n::tf(
        "Reflux applies only theme variable overrides inside :root {…} ({applied} applied, {ignored} ignored).",
        &[("applied", &applied.len().to_string()), ("ignored", &ignored.to_string())],
    );
    let page = col()
        .w_full()
        .h_full()
        .fill(p.background_primary)
        .child(header)
        .child(line())
        .child(col().w_full().grow(1.0).min_h(0.0).padding(16.0).child(editor))
        .child(line())
        .child(row().w_full().px(16.0).py(8.0).child(text(note).font(font(12.0, 400.0).lh(16.0)).color(if ignored > 0 { p.status_warning } else { p.text_tertiary }).wrap()));
    rune::el().w_full().h_full().child(page).children(completions.into_iter().flatten())
}
