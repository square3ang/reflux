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
        // `FormSwitch`: the checked thumb carries a brand check, the unchecked a grey X.
        let (fill, glyph, size, ink) =
            if on { (p.brand_primary_fill, &icons::CHECK_BOLD, 12.0, p.brand_primary) } else { (Color::WHITE, &icons::X_BOLD, 10.0, rgb(107, 114, 128)) };
        // `.switchThumb`: only `transform` transitions (150ms, cubic-bezier(.4,0,.2,1)), so it
        // slides out of `ui::root`'s instant changes while its colour and glyph swap at once,
        // and it shows in place when mounted (a tab opening).
        let thumb = el()
            .absolute(2.0, 2.0)
            .offset(if on { 20.0 } else { 0.0 }, 0.0)
            .offset_motion(rune::Curve::tween(0.15, rune::Easing::cubic(0.4, 0.0, 0.2, 1.0)))
            .animated_changes()
            .transition(rune::Transition::none())
            .motion(rune::Spring::NONE)
            .size(20.0, 20.0)
            .radius(10.0)
            .fill(fill)
            .box_shadow(sh(0.0, -1.0, 2.0, 0.0, rgba(0, 0, 0, 0.08), true))
            .box_shadow(sh(0.0, 1.0, 0.0, 0.0, rgba(0, 0, 0, 0.15), false))
            .box_shadow(sh(0.0, 2.0, 4.0, -1.0, rgba(0, 0, 0, 0.2), false))
            .child(row().w_full().h_full().center().instant_changes().child(icon(glyph, size).color(ink)));
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
        if let Some(f) = act.clone() {
            // Buttons answer Enter on the web (Radix's switch is one).
            sw = sw.on_key(move |s: &mut S, k, _| {
                if super::enters(k) {
                    f(s);
                    true
                } else {
                    false
                }
            });
        }
        if self.label.is_none() && self.description.is_none() {
            return sw;
        }
        let mut labels = col().grow(1.0).shrink().gap(4.0);
        if let Some(l) = self.label {
            labels = labels.child(super::mark::marked(l).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary));
        }
        if let Some(d) = self.description {
            labels = labels.child(super::mark::marked(d).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary_muted).wrap().pr(16.0));
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
                super::mark::marked(l).font(font(14.0, 400.0).lh(20.0)).color(p.text_primary).pl(8.0).mt((n - 20.0) / 2.0 - 1.0).wrap().shrink(),
            );
        }
        if self.disabled {
            return w.opacity(0.6).disabled(true);
        }
        w = w.cursor(rune::Cursor::Pointer);
        if let Some(f) = &self.on_toggle {
            let (press, enter) = (f.clone(), f.clone());
            w = w.on_press(move |s: &mut S| press(s)).on_key(move |s: &mut S, k, _| {
                if super::enters(k) {
                    enter(s);
                    true
                } else {
                    false
                }
            });
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
                .child(super::mark::marked(label).font(font(15.0, 500.0).lh(21.0)).color(p.text_primary));
            if let Some(d) = desc {
                words = words.child(super::mark::marked(d).font(font(14.0, 400.0).lh(19.6)).color(p.text_secondary).wrap());
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
                let (press, enter, v) = (f.clone(), f.clone(), value.to_string());
                let v2 = v.clone();
                opt = opt.cursor(rune::Cursor::Pointer).on_press(move |s: &mut S| press(s, &v)).on_key(move |s: &mut S, k, _| {
                    if super::enters(k) {
                        enter(s, &v2);
                        true
                    } else {
                        false
                    }
                });
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
    label: Option<String>,
    on_change: Option<Rc<dyn Fn(&mut S, f32)>>,
    /// `markers` with `onMarkerRender`'s labels, and `factoryDefaultValue` (its mark is green).
    marks: Vec<(f32, String)>,
    factory: Option<f32>,
    /// `onValueRender`: the value in a tooltip over the thumb.
    tip: Option<String>,
}

impl<S: 'static> Slider<S> {
    pub fn new(key: impl Into<String>, value: f32, min: f32, max: f32) -> Self {
        Self { value, min, max, step: 1.0, mini: false, disabled: false, key: key.into(), label: None, on_change: None, marks: Vec::new(), factory: None, tip: None }
    }
    /// Marks above the track (`markerPosition="above"`), the `factory` one green.
    pub fn marks(mut self, marks: Vec<(f32, String)>, factory: f32) -> Self { (self.marks, self.factory) = (marks, Some(factory)); self }
    pub fn tip(mut self, t: impl Into<String>) -> Self { self.tip = Some(t.into()); self }
    pub fn step(mut self, s: f32) -> Self { self.step = s; self }
    pub fn mini(mut self) -> Self { self.mini = true; self }
    pub fn disabled(mut self, d: bool) -> Self { self.disabled = d; self }
    /// The accessible name; the key (an id) when unset.
    pub fn label(mut self, l: impl Into<String>) -> Self { self.label = Some(l.into()); self }
    pub fn on_change(mut self, f: impl Fn(&mut S, f32) + 'static) -> Self { self.on_change = Some(Rc::new(f)); self }

    pub fn el(self, p: &Palette) -> El {
        let (h, thumb, track) = if self.mini { (20.0, 12.0, 4.0) } else { (26.0, 16.0, 6.0) };
        let span = (self.max - self.min).max(f32::EPSILON);
        let frac = ((self.value - self.min) / span).clamp(0.0, 1.0);
        let marks = (!self.marks.is_empty()).then(|| self.mark_lane(thumb, span, p));
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
        let grab = el()
            .size(thumb, thumb)
            .radius(thumb / 2.0)
            .fill(p.background_primary)
            .gradient(rgba(255, 255, 255, 0.96), rgba(245, 246, 248, 0.92))
            .border(2.0, mix(p.background_primary, p.brand_primary, 80.0))
            .box_shadow(sh(0.0, 1.0, 2.0, 0.0, rgba(0, 0, 0, 0.22), false))
            .box_shadow(sh(0.0, 6.0, 14.0, 0.0, rgba(0, 0, 0, 0.18), false))
            // `.sliderControl:hover .grabber`: the border turns brand 62%.
            .child(el().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).radius(thumb / 2.0).border(2.0, mix(p.brand_primary, p.background_primary, 62.0)).group_opacity(0.0, if self.disabled { 0.0 } else { 1.0 }).inert());
        // `SliderTooltipPortal`: Fluxer shows it while the thumb is hovered or dragged.
        // shortcut: it shows while the whole control is hovered (Rune's hover group), which also
        // covers a drag that stays on the track; a per-thumb hover needs its own state.
        let grab = match &self.tip {
            Some(tip) if !self.disabled => grab.child(
                row()
                    .pin(Some(-60.0), None, Some(-60.0), Some(thumb + 8.0))
                    .justify_center()
                    .inert()
                    .group_opacity(0.0, 1.0)
                    .child(super::overlay::tooltip_surface(tip, super::overlay::Placement::Top, super::overlay::TooltipKind::Normal, false, p)),
            ),
            _ => grab,
        };
        // Free space split frac : 1 - frac puts the thumb's left edge at frac of the track.
        let thumb_row = row()
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .center_y()
            .child(el().grow(frac))
            .child(grab)
            .child(el().grow(1.0 - frac));
        let label = self.label.clone().unwrap_or_else(|| self.key.clone());
        let mut ctl = row()
            .key(self.key)
            .w_full()
            .h(h)
            .px(thumb / 2.0)
            .center_y()
            .child(rail)
            .child(thumb_row)
            .role("slider")
            .label(label)
            .focus_ring(4.0, 0.0, p.focus_primary)
            .range_value(self.value as f64, self.min as f64, self.max as f64)
            .range_step(self.step as f64)
            .hover_group();
        // `.slider.hasMarks.marksAbove`: the marks take a 24px lane over the control.
        let with_marks = |ctl: El| match marks {
            Some(lane) => col().w_full().pt(24.0).child(lane).child(ctl),
            None => ctl,
        };
        if self.disabled {
            return with_marks(ctl.opacity(0.55).disabled(true));
        }
        if let Some(f) = self.on_change {
            let (min, max, step) = (self.min, self.max, self.step);
            let snap = move |v: f32| (((v - min) / step).round() * step + min).clamp(min, max);
            let value = self.value;
            let (g, end) = (f.clone(), f.clone());
            ctl = ctl
                .cursor(rune::Cursor::Pointer)
                .on_drag(move |s: &mut S, d, _| {
                    let t = ((d.x - thumb / 2.0) / (d.w - thumb).max(1.0)).clamp(0.0, 1.0);
                    f(s, snap(min + t * (max - min)));
                })
                .on_step(move |s: &mut S, delta, _| g(s, snap(value + delta as f32 * step)))
                // Home / End take the slider to its ends, as AppKit's do.
                .on_step_limit(move |s: &mut S, max_end, _| end(s, if max_end { max } else { min }));
        }
        with_marks(ctl)
    }

    /// `.marks`: each mark's value over a 2x8 dash, centred on its value (the ends flush with
    /// the track's ends), inset by half a thumb like the track.
    fn mark_lane(&self, thumb: f32, span: f32, p: &Palette) -> El {
        const W: f32 = 48.0;
        let last = self.marks.len() - 1;
        let lane = row().pin(Some(thumb / 2.0), Some(0.0), Some(thumb / 2.0), None).h(24.0).inert();
        lane.children(self.marks.iter().enumerate().map(|(i, (v, label))| {
            let f = ((v - self.min) / span).clamp(0.0, 1.0);
            let on = self.factory == Some(*v);
            let (fg, dash) = if on { (p.accent_success, p.accent_success) } else { (p.text_secondary, mix(p.interactive_muted, rgb(0x72, 0x76, 0x7d), 78.0)) };
            let mark = col().w(W).gap(3.0).child(text(label.clone()).font(font(11.0, 600.0).lh(14.0)).color(fg)).child(el().size(2.0, 8.0).radius(1.0).fill(dash));
            let (mark, x) = match i {
                0 => (mark.items_start(), 0.0),
                i if i == last => (mark.items_end(), -W),
                _ => (mark.items_center(), -W / 2.0),
            };
            row().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).child(el().w_frac(f)).child(el().w(0.0).h_full().child(mark.pin(Some(x), Some(0.0), None, None)))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Cx, Env, Key, KeyEvent, Mods, Runtime, col};

    fn slider(v: &f32, _: &mut Cx<f32>) -> El {
        col().w_full().h_full().child(
            Slider::new("volume", *v, 0.0, 100.0)
                .label("Volume")
                .on_change(|s: &mut f32, x| *s = x)
                .el(crate::theme::palette(crate::theme::Kind::Dark)),
        )
    }

    fn key<S>(rt: &mut Runtime<S>, k: Key) {
        rt.key(KeyEvent { key: k, mods: Mods::default() });
        rt.frame(0.0);
    }

    fn switch(on: &bool, _: &mut Cx<bool>) -> El {
        col().w_full().h_full().child(
            Switch::new(*on)
                .label("Mute")
                .on_toggle(|s: &mut bool| *s = !*s)
                .el(crate::theme::palette(crate::theme::Kind::Dark)),
        )
    }

    #[test]
    fn switch_toggles_with_space_and_enter() {
        let mut rt = Runtime::with_text(false, switch, (300.0, 80.0), Env::new(true, 1.0), rune::text::TextSystem::new(crate::theme::FONTS, crate::theme::FAMILIES));
        rt.frame(0.0);
        key(&mut rt, Key::Tab);
        let focused = rt.elements().into_iter().find(|e| e.focused).unwrap();
        assert_eq!((focused.role, focused.label.as_deref()), ("switch", Some("Mute")));
        key(&mut rt, Key::Text(" ".into()));
        assert!(rt.state, "Space toggles a switch");
        key(&mut rt, Key::Enter);
        assert!(!rt.state, "Enter toggles a switch too");
    }

    #[test]
    fn slider_takes_focus_steps_and_jumps_to_its_ends() {
        let mut rt = Runtime::with_text(50.0, slider, (200.0, 80.0), Env::new(true, 1.0), rune::text::TextSystem::new(crate::theme::FONTS, crate::theme::FAMILIES));
        rt.frame(0.0);
        key(&mut rt, Key::Tab);
        let focused = rt.elements().into_iter().find(|e| e.focused).expect("the slider takes Tab");
        assert_eq!(focused.key.as_deref(), Some("volume"));
        assert_eq!((focused.role, focused.label.as_deref()), ("slider", Some("Volume")));
        key(&mut rt, Key::Right);
        assert_eq!(rt.state, 51.0, "Right steps by one");
        key(&mut rt, Key::End);
        assert_eq!(rt.state, 100.0, "End takes the maximum");
        key(&mut rt, Key::Home);
        assert_eq!(rt.state, 0.0, "Home takes the minimum");
    }
}
