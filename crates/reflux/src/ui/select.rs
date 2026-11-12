//! `<Select>`: Fluxer's read-only `FormCombobox` (`components/form/FormCombobox.module.css`),
//! default and compact sizes.

use crate::ui::ChromiumWheel;
use std::f32::consts::PI;
use std::rc::Rc;

use rune::{El, col, icon, row, text};

use super::overlay::{Placement, PopupAccess, Popups, anchor, popout_shadow};
use super::over;
use crate::i18n::t;
use crate::icons;
use crate::theme::{Palette, font};

pub struct Select<'a, S: 'static> {
    id: &'a str,
    /// (value, label)
    options: &'a [(&'a str, &'a str)],
    value: Option<&'a str>,
    placeholder: &'a str,
    label: Option<&'a str>,
    width: f32,
    compact: bool,
    disabled: bool,
    on_choose: Option<Rc<dyn Fn(&mut S, &str)>>,
}

impl<'a, S: 'static> Select<'a, S> {
    pub fn new(id: &'a str, options: &'a [(&'a str, &'a str)], value: Option<&'a str>) -> Self {
        Self { id, options, value, placeholder: "", label: None, width: 240.0, compact: false, disabled: false, on_choose: None }
    }
    pub fn placeholder(mut self, p: &'a str) -> Self { self.placeholder = p; self }
    pub fn label(mut self, l: &'a str) -> Self { self.label = Some(l); self }
    /// Trigger width; the list opens at least as wide.
    pub fn width(mut self, w: f32) -> Self { self.width = w; self }
    pub fn compact(mut self) -> Self { self.compact = true; self }
    pub fn disabled(mut self, d: bool) -> Self { self.disabled = d; self }
    pub fn on_choose(mut self, f: impl Fn(&mut S, &str) + 'static) -> Self { self.on_choose = Some(Rc::new(f)); self }

    pub fn el(self, popups: &Popups, access: impl PopupAccess<S>, p: &Palette, _layer: &mut Vec<El>) -> El {
        let open = popups.is_open(self.id);
        let bg = p.form_surface_background;
        let border = if open { p.background_modifier_accent_focus } else { p.background_modifier_accent };
        let (min_h, pl, style) =
            if self.compact { (34.0, 10.0, font(13.0, 400.0).lh(18.0)) } else { (44.0, 16.0, font(14.0, 400.0).lh(20.0)) };
        let current = self.options.iter().find(|(v, _)| Some(*v) == self.value).map(|(_, l)| t(l));
        let shown = match current {
            Some(l) => text(l).color(p.text_primary),
            None => text(self.placeholder).color(p.text_tertiary),
        };
        // `.triggerButton`: 28px, radius 6, a bold 16px caret that turns over while open.
        let caret = row()
            .size(28.0, 28.0)
            .center()
            .radius(6.0)
            .child(icon(&icons::CARET_DOWN_BOLD, 16.0).color(p.text_tertiary).rotate(if open { PI } else { 0.0 }));
        let id = self.id.to_string();
        let mut trigger = row()
            .key(self.id)
            .w(self.width)
            .min_h(min_h)
            .pl(pl + 1.0)
            .pr(7.0)
            .gap(4.0)
            .center_y()
            .radius(8.0)
            .fill(bg)
            .border(1.0, over(border, bg))
            .child(shown.font(style).truncate().grow(1.0).shrink())
            .child(caret)
            .role("combobox")
            .expanded(open)
            .label(self.label.unwrap_or(self.placeholder))
            .focus_ring(4.0, 0.0, p.focus_primary)
            .focusable(!self.disabled);
        if self.disabled {
            trigger = trigger.opacity(0.5).disabled(true).cursor(rune::Cursor::NotAllowed);
        } else {
            let id2 = id.clone();
            trigger = trigger
                .cursor(rune::Cursor::Pointer)
                .on_press(move |s: &mut S| access(s).toggle(&id))
                // Web selects open on Enter as well as Space.
                .on_key(move |s: &mut S, k, _| {
                    if super::enters(k) {
                        access(s).toggle(&id2);
                        true
                    } else {
                        false
                    }
                });
        }

        if open {
            let (item_h, ipy, ipx) = if self.compact { (30.0, 5.0, 8.0) } else { (32.0, 8.0, 10.0) };
            let rows = self.options.iter().map(|&(value, label)| {
                let label = t(label);
                let on = self.value == Some(value);
                let mut r = row()
                    .key(format!("{}-{value}", self.id))
                    .w_full()
                    .min_h(item_h)
                    .py(ipy)
                    .px(ipx)
                    .gap(8.0)
                    .center_y()
                    .radius(6.0)
                    .child(text(label).font(font(14.0, 400.0).lh(20.0)).color(if on { p.brand_primary_fill } else { p.text_primary }).truncate().grow(1.0).shrink())
                    .role("option")
                    .label(label)
                    .checked(on)
                    .focusable(true)
                    .focus_selected(on)
                    .cursor(rune::Cursor::Pointer);
                r = if on {
                    r.fill(p.brand_primary).child(icon(&icons::CHECK_BOLD, 16.0).color(p.brand_primary_fill))
                } else {
                    r.focus_highlight(p.background_modifier_hover)
                };
                if let Some(f) = &self.on_choose {
                    let (f, v) = (f.clone(), value.to_string());
                    r = r.on_press(move |s: &mut S| {
                        access(s).close();
                        f(s, &v);
                    });
                }
                r
            });
            // `.list`: padding 4 4 0, gap 1, and an ::after that leaves 4 at the bottom.
            let popup = col()
                .key(format!("{}-popup", self.id))
                .min_w(self.width)
                .padding(1.0)
                .fill(bg)
                .border(1.0, over(p.background_modifier_accent, bg))
                .radius(8.0)
                .box_shadow(popout_shadow(p, 0.24, 0.24))
                .child(super::scroller(
                    // react-select's `maxMenuHeight` (300) caps long lists like the birth year.
                    col()
                        .key(format!("{}-list", self.id))
                        .w_full()
                        .max_h(300.0)
                        .scroll(rune::ScrollSpec { stick_bottom: false }).chromium_wheel::<S>(format!("{}-list", self.id))
                        .child(col().w_full().pt(4.0).px(4.0).pb(4.0).gap(1.0).children(rows)),
                    false,
                    p,
                ))
                .focus_group()
                .role("listbox")
                .label(self.label.unwrap_or(self.placeholder))
                .modal(true, move |s: &mut S| access(s).close())
                // react-select's menu has no enter/exit animation.
                .transition(rune::Transition::none());
            super::overlay::late(anchor(popup, self.id, Placement::BottomStart, 4.0));
        }

        match self.label {
            Some(l) => col().gap(4.0).child(super::mark::marked(l).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary)).child(trigger),
            None => trigger,
        }
    }
}
