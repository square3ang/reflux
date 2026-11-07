//! `fluxer_app/src/features/ui/components/form/FormInput.module.css` (single-line `<Input>`).

use rune::{El, TextEdit, col, icon, row, text, text_input};

use super::{mix, over};
use crate::icons;
use crate::theme::{Palette, font};
use crate::i18n::t;

pub struct Input<'a, S: 'static> {
    edit: &'a TextEdit,
    access: Box<dyn for<'b> Fn(&'b mut S) -> &'b mut TextEdit>,
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
        Self { edit, access: Box::new(access), key, label: None, placeholder: "", error: None, password: None, secure: false, multiline: None, rows: None, left: None, right: None }
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
                let access = self.access;
                move |s: &mut S| -> &mut TextEdit { access(s) }
            });
        if let Some(l) = self.label {
            field = field.label(l);
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
                    .on_press(move |s: &mut S| toggle(s))
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
