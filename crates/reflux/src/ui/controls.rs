//! Form controls: `<Switch>` (`components/form/FormSwitch.module.css`), `<Checkbox>`
//! (`checkbox/Checkbox.module.css`), `<RadioGroup>` (`radio_group/RadioGroup.module.css`) and
//! `<Slider>` (`components/Slider.module.css`).

use std::f32::consts::PI;
use std::rc::Rc;

use rune::{Color, El, Shadow, col, el, icon, row, text};

use super::{is_light, mix, rgb, rgba};
use crate::i18n::t;
use crate::icons;
use crate::theme::{Palette, font};

fn sh(x: f32, y: f32, blur: f32, spread: f32, color: Color, inset: bool) -> Shadow {
    Shadow { x, y, blur, spread, color, inset }
}

/// The unchecked checkbox/radio border: `color-mix(--border-color 70%, #fff)` (light: text-secondary with black).
fn control_border(p: &Palette) -> Color {
    if is_light(p) { mix(p.text_secondary, Color::BLACK, 70.0) } else { mix(p.border_color, Color::WHITE, 70.0) }
}

type Handler<S> = Option<Rc<dyn Fn(&mut S)>>;

pub struct Switch<S: 'static> {
    on: bool,
    label: Option<String>,
    description: Option<String>,
    disabled: bool,
    on_toggle: Handler<S>,
}

impl<S: 'static> Switch<S> {
    pub fn new(on: bool) -> Self {
        Self { on, label: None, description: None, disabled: false, on_toggle: None }
    }
    pub fn label(mut self, l: impl Into<String>) -> Self { self.label = Some(l.into()); self }
    pub fn description(mut self, d: impl Into<String>) -> Self { self.description = Some(d.into()); self }
    pub fn disabled(mut self, d: bool) -> Self { self.disabled = d; self }
    pub fn on_toggle(mut self, f: impl Fn(&mut S) + 'static) -> Self { self.on_toggle = Some(Rc::new(f)); self }

    pub fn el(self, p: &Palette) -> El {
        let on = self.on;
        let (track, inset, hi) = if on {
            (p.brand_primary, rgba(0, 0, 0, 0.1), rgba(255, 255, 255, 0.15))
        } else {
            (rgb(107, 114, 128), rgba(0, 0, 0, 0.15), rgba(255, 255, 255, 0.1))
        };
        let thumb = el()
            .absolute(if on { 22.0 } else { 2.0 }, 2.0)
            .size(20.0, 20.0)
            .radius(10.0)
            .fill(if on { p.brand_primary_fill } else { Color::WHITE })
            .box_shadow(sh(0.0, -1.0, 2.0, 0.0, rgba(0, 0, 0, 0.08), true))
            .box_shadow(sh(0.0, 1.0, 0.0, 0.0, rgba(0, 0, 0, 0.15), false))
            .box_shadow(sh(0.0, 2.0, 4.0, -1.0, rgba(0, 0, 0, 0.2), false));
        let mut sw = el()
            .size(44.0, 24.0)
            .radius(12.0)
            .fill(track)
            .box_shadow(sh(0.0, 2.0, 4.0, 0.0, inset, true))
            .box_shadow(sh(0.0, 1.0, 0.0, 0.0, hi, false))
            .child(thumb)
            .role("switch")
            .checked(on)
            .focusable(!self.disabled);
        if let Some(l) = &self.label {
            sw = sw.label(l.clone());
        }
        let act = (!self.disabled).then_some(self.on_toggle).flatten();
        if self.disabled {
            sw = sw.opacity(0.5).disabled(true);
        } else {
            sw = sw.cursor(rune::Cursor::Pointer);
            if let Some(f) = &act {
                let f = f.clone();
                sw = sw.on_press(move |s: &mut S| f(s));
            }
        }
        if self.label.is_none() && self.description.is_none() {
            return sw;
        }
        let mut labels = col().grow(1.0).shrink().gap(4.0);
        if let Some(l) = self.label {
            labels = labels.child(text(l).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary));
        }
        if let Some(d) = self.description {
            labels = labels.child(text(d).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap().pr(16.0));
        }
        if let Some(f) = act {
            labels = labels.cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| f(s));
        }
        row().w_full().min_h(44.0).py(4.0).gap(16.0).center_y().justify_between().child(labels).child(sw)
    }
}

pub struct Checkbox<S: 'static> {
    checked: bool,
    size: f32,
    round: bool,
    label: Option<String>,
    disabled: bool,
    on_toggle: Handler<S>,
}

impl<S: 'static> Checkbox<S> {
    pub fn new(checked: bool) -> Self {
        Self { checked, size: 24.0, round: false, label: None, disabled: false, on_toggle: None }
    }
    /// `size="small"` is 18.
    pub fn size(mut self, s: f32) -> Self { self.size = s; self }
    pub fn round(mut self) -> Self { self.round = true; self }
    pub fn label(mut self, l: impl Into<String>) -> Self { self.label = Some(l.into()); self }
    pub fn disabled(mut self, d: bool) -> Self { self.disabled = d; self }
    pub fn on_toggle(mut self, f: impl Fn(&mut S) + 'static) -> Self { self.on_toggle = Some(Rc::new(f)); self }

    pub fn el(self, p: &Palette) -> El {
        let n = self.size;
        let mut b = row().size(n, n).center().radius(if self.round { n / 2.0 } else { 3.0 });
        b = if self.checked {
            b.fill(p.brand_primary)
                .border(1.0, p.brand_primary_foreground)
                .child(icon(&icons::CHECK_BOLD, (n * 0.75).floor()).color(p.brand_primary_fill))
        } else {
            b.border(1.0, control_border(p))
        };
        let mut w = row().min_h(n).items_start().child(b).role("checkbox").checked(self.checked).focusable(!self.disabled);
        if let Some(l) = self.label {
            w = w.label(l.clone()).child(
                text(l).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).pl(8.0).mt((n - 20.0) / 2.0 - 1.0).wrap().shrink(),
            );
        }
        if self.disabled {
            return w.opacity(0.6).disabled(true);
        }
        w = w.cursor(rune::Cursor::Pointer);
        if let Some(f) = self.on_toggle {
            w = w.on_press(move |s: &mut S| f(s));
        }
        w
    }
}

/// One `<RadioGroup>` option: (value, label, description).
pub type RadioOption<'a> = (&'a str, &'a str, Option<&'a str>);

pub struct RadioGroup<'a, S: 'static> {
    options: &'a [RadioOption<'a>],
    value: Option<&'a str>,
    disabled: bool,
    on_change: Option<Rc<dyn Fn(&mut S, &str)>>,
}

impl<'a, S: 'static> RadioGroup<'a, S> {
    pub fn new(options: &'a [RadioOption<'a>], value: Option<&'a str>) -> Self {
        Self { options, value, disabled: false, on_change: None }
    }
    pub fn disabled(mut self, d: bool) -> Self { self.disabled = d; self }
    pub fn on_change(mut self, f: impl Fn(&mut S, &str) + 'static) -> Self { self.on_change = Some(Rc::new(f)); self }

    pub fn el(self, p: &Palette) -> El {
        col().gap(6.0).focus_group().role("radiogroup").children(self.options.iter().map(|&(value, label, desc)| {
            // Option tables are often `const`; their text is translated here.
            let (label, desc) = (t(label), desc.map(t));
            let on = self.value == Some(value);
            let dot = radio_dot(on, p).mt(2.0);
            let mut words = col()
                .gap(4.0)
                .shrink()
                .child(text(label).font(font(15.0, 500.0).lh(21.0)).color(p.text_primary));
            if let Some(d) = desc {
                words = words.child(text(d).font(font(14.0, 400.0).lh(19.6)).color(p.text_secondary).wrap());
            }
            let mut opt = row()
                .w_full()
                .py(4.0)
                .gap(8.0)
                .items_start()
                .child(dot)
                .child(words)
                .role("radio")
                .label(label)
                .checked(on)
                .focusable(!self.disabled);
            if self.disabled {
                opt = opt.opacity(0.55).disabled(true);
            } else if let Some(f) = &self.on_change {
                let (f, v) = (f.clone(), value.to_string());
                opt = opt.cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| f(s, &v));
            }
            opt
        }))
    }
}

/// The radio circle: a 40-unit SVG circle (r 20, stroke 2) drawn at 18px, with an r 8 dot.
pub fn radio_dot(on: bool, p: &Palette) -> El {
    let border = control_border(p);
    let dot = row()
        .size(18.0, 18.0)
        .center()
        .radius(9.0)
        .fill(if on { p.brand_primary } else { mix(p.background_primary, border, 55.0) })
        .border(0.9, if on { p.brand_primary_foreground } else { border })
        .shadow(1.0, if on { 3.0 } else { 2.0 }, rgba(0, 0, 0, if on { 0.2 } else { 0.15 }));
    if on { dot.child(el().size(7.2, 7.2).radius(3.6).fill(p.brand_primary_fill)) } else { dot }
}

pub struct Slider<S: 'static> {
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    mini: bool,
    disabled: bool,
    key: String,
    on_change: Option<Rc<dyn Fn(&mut S, f32)>>,
}

impl<S: 'static> Slider<S> {
    pub fn new(key: impl Into<String>, value: f32, min: f32, max: f32) -> Self {
        Self { value, min, max, step: 1.0, mini: false, disabled: false, key: key.into(), on_change: None }
    }
    pub fn step(mut self, s: f32) -> Self { self.step = s; self }
    pub fn mini(mut self) -> Self { self.mini = true; self }
    pub fn disabled(mut self, d: bool) -> Self { self.disabled = d; self }
    pub fn on_change(mut self, f: impl Fn(&mut S, f32) + 'static) -> Self { self.on_change = Some(Rc::new(f)); self }

    pub fn el(self, p: &Palette) -> El {
        let (h, thumb, track) = if self.mini { (20.0, 12.0, 4.0) } else { (26.0, 16.0, 6.0) };
        let span = (self.max - self.min).max(f32::EPSILON);
        let frac = ((self.value - self.min) / span).clamp(0.0, 1.0);
        let light = is_light(p);
        let end = if light { mix(p.brand_primary_foreground, rgb(0x11, 0x11, 0x11), 72.0) } else { mix(p.brand_primary_light, Color::WHITE, 84.0) };
        let mut bar = el()
            .h_full()
            .w_frac(frac)
            .radius(track / 2.0)
            .linear_gradient(PI / 2.0, [(0.0, p.brand_primary_foreground), (1.0, end)]);
        if !light {
            bar = bar.box_shadow(sh(0.0, 0.0, 0.0, 1.0, rgba(255, 255, 255, 0.12), true));
        }
        let rail = el()
            .w_full()
            .h(track)
            .radius(track / 2.0)
            .fill(mix(p.interactive_muted, p.background_tertiary, 82.0))
            .box_shadow(sh(0.0, 0.0, 0.0, 1.0, mix(p.background_primary, Color::TRANSPARENT, 20.0), true))
            .child(bar);
        // shortcut: the grabber's hover border (brand 62%) is skipped; Rune's hover paints an overlay, not a border.
        let grab = el()
            .size(thumb, thumb)
            .radius(thumb / 2.0)
            .fill(p.background_primary)
            .gradient(rgba(255, 255, 255, 0.96), rgba(245, 246, 248, 0.92))
            .border(2.0, mix(p.background_primary, p.brand_primary, 80.0))
            .box_shadow(sh(0.0, 1.0, 2.0, 0.0, rgba(0, 0, 0, 0.22), false))
            .box_shadow(sh(0.0, 6.0, 14.0, 0.0, rgba(0, 0, 0, 0.18), false));
        // Free space split frac : 1 - frac puts the thumb's left edge at frac of the track.
        let thumb_row = row()
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .center_y()
            .child(el().grow(frac))
            .child(grab)
            .child(el().grow(1.0 - frac));
        let mut ctl = row()
            .key(self.key)
            .w_full()
            .h(h)
            .px(thumb / 2.0)
            .center_y()
            .child(rail)
            .child(thumb_row)
            .range_value(self.value as f64, self.min as f64, self.max as f64)
            .range_step(self.step as f64);
        if self.disabled {
            return ctl.opacity(0.55).disabled(true).role("slider");
        }
        if let Some(f) = self.on_change {
            let (min, max, step) = (self.min, self.max, self.step);
            let snap = move |v: f32| (((v - min) / step).round() * step + min).clamp(min, max);
            let value = self.value;
            let g = f.clone();
            ctl = ctl
                .cursor(rune::Cursor::Pointer)
                .on_drag(move |s: &mut S, d, _| {
                    let t = ((d.x - thumb / 2.0) / (d.w - thumb).max(1.0)).clamp(0.0, 1.0);
                    f(s, snap(min + t * (max - min)));
                })
                .on_step(move |s: &mut S, delta, _| g(s, snap(value + delta as f32 * step)));
        }
        ctl.role("slider")
    }
}
