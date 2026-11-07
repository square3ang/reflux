//! `fluxer_app/src/features/ui/components/form/FormInput.module.css` (single-line `<Input>`).

use std::rc::Rc;

use rune::{El, Key, TextEdit, col, icon, row, text, text_input};

use super::{mix, over};
use crate::icons;
use crate::theme::{Palette, font};
use crate::i18n::t;

pub struct Input<'a, S: 'static> {
    edit: &'a TextEdit,
    access: Rc<dyn for<'b> Fn(&'b mut S) -> &'b mut TextEdit>,
    key: &'a str,
    label: Option<&'a str>,
    placeholder: &'a str,
    error: Option<&'a str>,
    /// Password field: whether it is revealed, and the toggle.
    password: Option<(bool, fn(&mut S))>,
    /// Masked without the reveal button.
    secure: bool,
    /// `<Textarea>`: wraps and takes Enter, at least this tall.
    multiline: Option<f32>,
    /// `minRows` / `maxRows`: grows with its text, then scrolls inside.
    rows: Option<(usize, usize)>,
    /// `leftElement` / `rightElement`: buttons inside the box beside the text.
    left: Option<El>,
    right: Option<El>,
}

impl<'a, S: 'static> Input<'a, S> {
    pub fn new(key: &'a str, edit: &'a TextEdit, access: impl for<'b> Fn(&'b mut S) -> &'b mut TextEdit + 'static) -> Self {
        Self { edit, access: Rc::new(access), key, label: None, placeholder: "", error: None, password: None, secure: false, multiline: None, rows: None, left: None, right: None }
    }
    pub fn label(mut self, l: &'a str) -> Self { self.label = Some(l); self }
    pub fn placeholder(mut self, p: &'a str) -> Self { self.placeholder = p; self }
    pub fn error(mut self, e: Option<&'a str>) -> Self { self.error = e; self }
    pub fn password(mut self, revealed: bool, toggle: fn(&mut S)) -> Self { self.password = Some((revealed, toggle)); self }
    pub fn secure(mut self) -> Self { self.secure = true; self }
    pub fn multiline(mut self, min_h: f32) -> Self { self.multiline = Some(min_h); self }
    pub fn rows(mut self, min: usize, max: usize) -> Self { self.multiline = Some(0.0); self.rows = Some((min, max)); self }
    pub fn left(mut self, e: El) -> Self { self.left = Some(e); self }
    pub fn right(mut self, e: El) -> Self { self.right = Some(e); self }

    pub fn el(self, p: &Palette) -> El {
        let style = font(14.0, 400.0).lh(20.0);
        let mut field = text_input(self.edit, style);
        field = match self.multiline {
            Some(_) if self.rows.is_some() => field,
            Some(h) => field.min_h(h - 22.0),
            None => field.single_line(),
        };
        let mut field = field
            .key(self.key)
            .grow(1.0)
            .shrink()
            .color(p.text_primary)
            .placeholder(self.placeholder, p.text_tertiary)
            .caret(p.text_primary, p.background_modifier_selected)
            .on_edit({
                let access = self.access.clone();
                move |s: &mut S| -> &mut TextEdit { access(s) }
            });
        if self.multiline.is_some() {
            // A textarea's Enter types a newline, as on the web (Rune's default gives Return to
            // submit / the default button). Shift and Option Return still insert one in the editor.
            let access = self.access.clone();
            field = field.on_input_key(move |s: &mut S, k, _| {
                if k.key == Key::Enter && !k.mods.cmd && !k.mods.ctrl && !k.mods.alt && !k.mods.shift {
                    access(s).insert("\n");
                    true
                } else {
                    false
                }
            });
        }
        if let Some(l) = self.label {
            field = field.label(l);
        } else if !self.placeholder.is_empty() {
            field = field.label(self.placeholder);
        }
        if let Some(e) = self.error {
            field = field.invalid(true).description(e.to_string());
        }
        if self.secure {
            field = field.secure();
        }
        let border = if self.error.is_some() {
            p.status_danger
        } else if self.edit.focused {
            p.background_modifier_accent_focus
        } else {
            p.background_modifier_accent
        };
        // padding 10px 16px inside a 1px border; min-height 44.
        let mut boxed = row()
            .w_full()
            .min_h(44.0)
            .pl(if self.left.is_some() { 5.0 } else { 17.0 })
            .pr(if self.password.is_some() { 49.0 } else if self.right.is_some() { 5.0 } else { 17.0 })
            .py(if self.multiline.is_some() { 11.0 } else { 0.0 })
            .gap(if self.left.is_some() || self.right.is_some() { 4.0 } else { 0.0 })
            .center_y()
            .radius(8.0)
            .fill(p.form_surface_background)
            .border(1.0, over(border, p.form_surface_background))
            .cursor(rune::Cursor::Text);
        if let Some((revealed, toggle)) = self.password {
            field = field.secure().revealed(revealed);
            let eye = if revealed { &icons::EYE_SLASH_FILL } else { &icons::EYE_FILL };
            boxed = boxed.child(field).child(
                rune::el()
                    .pin(None, Some(7.0), Some(12.0), None)
                    .padding(6.0)
                    .radius(4.0)
                    .hover(p.background_modifier_hover)
                    .cursor(rune::Cursor::Pointer)
                    .role("button")
                    .label(if revealed { t("Hide password") } else { t("Show password") })
                    .focusable(true)
                    .focus_ring(4.0, 0.0, p.focus_primary)
                    .on_press(move |s: &mut S| toggle(s))
                    .on_key(move |s: &mut S, k, _| {
                        if super::enters(k) {
                            toggle(s);
                            true
                        } else {
                            false
                        }
                    })
                    .child(icon(eye, 18.0).color(p.text_tertiary)),
            );
        } else {
            let field = match self.rows {
                Some((min, max)) => rune::auto_grow(field.w_full(), min, max).grow(1.0),
                None => field,
            };
            boxed = boxed.children(self.left).child(field).children(self.right);
        }

        let mut group = col().w_full().gap(6.0).child(boxed);
        if let Some(e) = self.error {
            group = group.child(
                text(e).font(font(14.0, 400.0).lh(20.0)).color(mix(p.status_danger, p.text_primary, 78.0)).wrap().role("alert"),
            );
        }
        let mut fieldset = col().w_full().gap(4.0);
        if let Some(l) = self.label {
            fieldset = fieldset.child(super::mark::marked(l).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary));
        }
        fieldset.child(group)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Cx, Env, Key, Mods, Runtime, TextEdit, col};

    fn single(edit: &TextEdit, _: &mut Cx<TextEdit>) -> El {
        let p = crate::theme::palette(crate::theme::Kind::Dark);
        col().w_full().child(Input::new("field", edit, |s: &mut TextEdit| s).label("Name").el(p))
    }

    fn multiline(edit: &TextEdit, _: &mut Cx<TextEdit>) -> El {
        let p = crate::theme::palette(crate::theme::Kind::Dark);
        col().w_full().child(Input::new("field", edit, |s: &mut TextEdit| s).label("Name").multiline(60.0).el(p))
    }

    fn press(rt: &mut Runtime<TextEdit>, k: Key, ctrl: bool) {
        rt.key(rune::KeyEvent { key: k, mods: Mods { ctrl, ..Default::default() } });
        rt.frame(0.0);
    }

    fn caret(edit: &TextEdit) -> usize {
        edit.range().1
    }

    #[test]
    fn text_field_keeps_home_end_word_movement_and_select_all() {
        let mut edit = TextEdit::new("hello world");
        edit.set_caret(edit.text.len());
        let mut rt = Runtime::with_text(edit, single, (300.0, 80.0), Env::new(true, 1.0), rune::text::TextSystem::new(crate::theme::FONTS, crate::theme::FAMILIES));
        rt.frame(0.0);
        press(&mut rt, Key::Tab, false);
        let focused = rt.elements().into_iter().find(|e| e.focused).unwrap();
        assert_eq!((focused.key.as_deref(), focused.role), (Some("field"), "textbox"));
        press(&mut rt, Key::Home, false);
        assert_eq!(caret(&rt.state), 0);
        press(&mut rt, Key::End, false);
        assert_eq!(caret(&rt.state), 11);
        press(&mut rt, Key::Left, true);
        assert_eq!(caret(&rt.state), 6, "Ctrl+Left moves a word");
        press(&mut rt, Key::Command('a'), false);
        assert_eq!(rt.state.range(), (0, 11), "Ctrl+A selects all");
        press(&mut rt, Key::Backspace, false);
        assert_eq!(rt.state.text, "", "Backspace deletes the selection");
    }

    #[test]
    fn multiline_enter_types_a_newline() {
        let mut edit = TextEdit::new("one");
        edit.set_caret(3);
        let mut rt = Runtime::with_text(edit, multiline, (300.0, 120.0), Env::new(true, 1.0), rune::text::TextSystem::new(crate::theme::FONTS, crate::theme::FAMILIES));
        rt.frame(0.0);
        press(&mut rt, Key::Tab, false);
        rt.elements().into_iter().find(|e| e.focused).expect("the field takes Tab");
        press(&mut rt, Key::Enter, false);
        press(&mut rt, Key::Text("two".into()), false);
        assert_eq!(rt.state.text, "one\ntwo");
        assert_eq!(caret(&rt.state), 7);
    }
}
