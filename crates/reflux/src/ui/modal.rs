//! `<Modal>` (`fluxer_app/src/features/app/components/dialogs/Modal.module.css`): the dimmed
//! backdrop, the card, and its header / content / footer.

use std::rc::Rc;

use rune::{Color, El, Shadow, col, el, icon, row, text};

use super::{over, rgba};
use crate::icons;
use crate::theme::{Palette, font};
use crate::i18n::t;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModalSize {
    /// 440 wide.
    #[default]
    Small,
    /// 600 × min 400.
    Medium,
    /// 800 × min 400.
    Large,
}

pub struct Modal<S: 'static> {
    title: String,
    size: ModalSize,
    body: Option<El>,
    footer: Option<El>,
    on_close: Rc<dyn Fn(&mut S)>,
}

impl<S: 'static> Modal<S> {
    pub fn new(title: impl Into<String>, on_close: impl Fn(&mut S) + 'static) -> Self {
        Self { title: title.into(), size: ModalSize::Small, body: None, footer: None, on_close: Rc::new(on_close) }
    }
    pub fn size(mut self, s: ModalSize) -> Self { self.size = s; self }
    pub fn body(mut self, b: El) -> Self { self.body = Some(b); self }
    /// Right-aligned buttons, gap 8.
    pub fn footer(mut self, f: El) -> Self { self.footer = Some(f); self }

    /// The whole layer (backdrop + centred card); push it last into the screen's overlay layer.
    /// Escape, the close button and a press on the backdrop call `on_close`.
    pub fn el(self, p: &Palette) -> El {
        let bg = p.background_secondary;
        let (w, min_h) = match self.size {
            ModalSize::Small => (440.0, if self.footer.is_some() { 200.0 } else { 0.0 }),
            ModalSize::Medium => (600.0, 400.0),
            ModalSize::Large => (800.0, 400.0),
        };
        let close = self.on_close.clone();
        let x = row()
            .size(32.0, 32.0)
            .center()
            .radius(4.0)
            .hover_group()
            .hover(p.background_modifier_hover)
            .cursor(rune::Cursor::Pointer)
            .role("button").focusable(true).focus_ring(4.0, 0.0, p.focus_primary)
            .label(t("Close"))
            .on_press(move |s: &mut S| close(s))
            .child(icon(&icons::X_BOLD, 24.0).color(p.text_primary_muted).group_color(p.text_primary).group_opacity(0.5, 1.0));
        let header = row()
            .w_full()
            .padding(16.0)
            .justify_between()
            .center_y()
            .child(text(self.title.clone()).font(font(18.0, 600.0).lh(20.0)).color(p.text_primary))
            .child(x);
        let mut card = col()
            .w(w)
            .min_h(min_h)
            .padding(1.0)
            .radius(8.0)
            .fill(bg)
            .border(1.0, over(p.background_header_secondary, bg))
            .box_shadow(Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 1.0, color: rgba(47, 49, 55, 0.08), inset: false })
            .box_shadow(Shadow { x: 0.0, y: 8.0, blur: 24.0, spread: -4.0, color: rgba(0, 0, 0, 0.25), inset: false })
            .box_shadow(Shadow { x: 0.0, y: 20.0, blur: 48.0, spread: -8.0, color: rgba(0, 0, 0, 0.2), inset: false })
            .role("dialog")
            .label(self.title)
            .child(header)
            .child(col().w_full().grow(1.0).px(16.0).pb(16.0).children(self.body));
        if let Some(f) = self.footer {
            card = card.child(row().w_full().padding(16.0).gap(8.0).justify_end().center_y().child(f));
        }
        let close = self.on_close;
        el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(Color::BLACK).opacity(0.85))
            .child(
                col()
                    .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
                    .py(24.0)
                    .px(16.0)
                    .center()
                    .child(card.modal(true, move |s: &mut S| close(s))),
            )
    }
}
