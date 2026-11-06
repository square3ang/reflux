//! `fluxer_app/src/features/ui/button/Button.module.css`.

use rune::{Color, El, Icon, LoopSpec, icon, row, text};

use super::{mix, over, rgba};
use crate::theme::{Palette, font};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    #[default]
    Primary,
    Secondary,
    Danger,
    Inverted,
    InvertedOutline,
    Ghost,
    Recording,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonSize {
    #[default]
    Medium,
    Small,
    Compact,
    SuperCompact,
}

impl ButtonSize {
    /// (height, min-width, horizontal padding, font size, line height, radius, icon size)
    fn metrics(self) -> (f32, f32, f32, f32, f32, f32, f32) {
        match self {
            ButtonSize::Medium => (36.0, 72.0, 12.0, 13.0, 18.0, 8.0, 16.0),
            ButtonSize::Small => (34.0, 56.0, 10.0, 13.0, 18.0, 8.0, 16.0),
            ButtonSize::Compact => (32.0, 52.0, 10.0, 13.0, 18.0, 8.0, 16.0),
            ButtonSize::SuperCompact => (26.0, 0.0, 7.0, 12.0, 16.0, 6.0, 14.0),
        }
    }
}

/// `<Button>`: build with the builder methods, then `.el(palette)`.
pub struct Button<S: 'static> {
    label: String,
    variant: ButtonVariant,
    size: ButtonSize,
    icon: Option<&'static Icon>,
    icon_gap: f32,
    fit_container: bool,
    square: bool,
    disabled: bool,
    submitting: bool,
    key: Option<String>,
    on_press: Option<Box<dyn Fn(&mut S)>>,
    on_press_tx: Option<Box<dyn Fn(&mut S, &mut rune::Tx)>>,
    icon_right: bool,
}

impl<S: 'static> Button<S> {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            variant: ButtonVariant::Primary,
            size: ButtonSize::Medium,
            icon: None,
            icon_gap: 6.0,
            fit_container: false,
            square: false,
            disabled: false,
            submitting: false,
            key: None,
            on_press: None,
            on_press_tx: None,
            icon_right: false,
        }
    }
    pub fn variant(mut self, v: ButtonVariant) -> Self { self.variant = v; self }
    pub fn size(mut self, s: ButtonSize) -> Self { self.size = s; self }
    /// Left icon (`.iconWrapper` gap 6).
    pub fn icon(mut self, i: &'static Icon) -> Self { self.icon = Some(i); self }
    /// Gap between icon and label; links styled as buttons (`.methodChoiceLink`) use 8.
    pub fn icon_gap(mut self, g: f32) -> Self { self.icon_gap = g; self }
    pub fn fit_container(mut self) -> Self { self.fit_container = true; self }
    pub fn square(mut self) -> Self { self.square = true; self }
    pub fn disabled(mut self, d: bool) -> Self { self.disabled = d; self }
    pub fn submitting(mut self, s: bool) -> Self { self.submitting = s; self }
    pub fn key(mut self, k: impl Into<String>) -> Self { self.key = Some(k.into()); self }
    pub fn on_press(mut self, f: impl Fn(&mut S) + 'static) -> Self { self.on_press = Some(Box::new(f)); self }
    /// `on_press` that needs the `Tx` (open a URL, copy).
    pub fn on_press_tx(mut self, f: impl Fn(&mut S, &mut rune::Tx) + 'static) -> Self { self.on_press_tx = Some(Box::new(f)); self }
    /// `rightIcon`: the icon after the label.
    pub fn icon_right(mut self, i: &'static Icon) -> Self { self.icon = Some(i); self.icon_right = true; self }

    pub fn el(self, p: &Palette) -> El {
        let (h, min_w, pad, fs, lh, radius, icon_size) = self.size.metrics();
        let white = Color::WHITE;
        // (fill, border, text, hover fill, press fill); CSS paints the fill under the border.
        let (fill, border, fg, hover, press) = match self.variant {
            ButtonVariant::Primary => (
                p.brand_primary,
                mix(p.brand_primary, white, 82.0),
                p.brand_primary_fill,
                p.brand_secondary,
                p.brand_primary_pressed,
            ),
            ButtonVariant::Secondary => {
                let hover = mix(p.form_surface_background, p.text_primary, 94.0);
                (p.form_surface_background, p.background_modifier_accent, p.button_secondary_text, hover, hover)
            }
            ButtonVariant::Danger => (
                mix(p.button_danger_fill, p.background_primary, 92.0),
                mix(p.button_danger_fill, Color::TRANSPARENT, 70.0),
                p.button_danger_text,
                mix(p.button_danger_active_fill, p.background_primary, 92.0),
                mix(p.button_danger_active_fill, Color::BLACK, 78.0),
            ),
            ButtonVariant::Inverted => (
                p.button_inverted_fill,
                Color::TRANSPARENT,
                p.button_inverted_text,
                Color::TRANSPARENT,
                Color::TRANSPARENT,
            ),
            ButtonVariant::InvertedOutline => (
                Color::TRANSPARENT,
                mix(white, Color::TRANSPARENT, 78.0),
                white,
                rgba(255, 255, 255, 0.1),
                rgba(255, 255, 255, 0.05),
            ),
            ButtonVariant::Ghost => (
                Color::TRANSPARENT,
                Color::TRANSPARENT,
                p.text_primary,
                mix(p.text_primary, Color::TRANSPARENT, 12.0),
                mix(p.text_primary, Color::TRANSPARENT, 6.0),
            ),
            ButtonVariant::Recording => {
                let c = p.accent_success;
                (c, mix(c, white, 80.0), p.button_primary_text, c, c)
            }
        };
        let live = !self.disabled && !self.submitting;

        let mut content = row().center().gap(self.icon_gap).opacity(if self.submitting { 0.0 } else { 1.0 });
        if let Some(i) = self.icon.filter(|_| !self.icon_right) {
            content = content.child(icon(i, icon_size).color(fg));
        }
        if !self.label.is_empty() {
            content = content.child(text(self.label.clone()).font(font(fs, 600.0).lh(lh)).color(fg));
        }
        if let Some(i) = self.icon.filter(|_| self.icon_right) {
            content = content.child(icon(i, icon_size).color(fg));
        }

        // Rune's border takes no layout space, so the 1px CSS border is added to the padding.
        let mut b = row()
            .h(h)
            .min_w(if self.square { h } else { min_w })
            .px(if self.square { 0.0 } else { pad + 1.0 })
            .center()
            .radius(radius)
            .fill(fill)
            .border(1.0, over(border, fill))
            .child(content);
        if self.square {
            b = b.w(h);
        }
        if self.fit_container {
            b = b.w_full();
        }
        if self.submitting {
            b = b.child(spinner(fg).pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)));
        }
        if self.disabled {
            b = b.opacity(0.5);
        }
        b = b.disabled(self.disabled).role("button").label(self.label.clone());
        if let Some(k) = self.key {
            b = b.key(k);
        }
        if live {
            // shortcut: hover/press replace the fill only; CSS also shifts the border colour, which
            // Rune's overlay (drawn under the border) can't do. Upgrade with a hover-driven border if visible.
            b = b.hover(hover).press(press).cursor(rune::Cursor::Pointer);
            if let Some(f) = self.on_press {
                b = b.on_press(f);
            }
            if let Some(f) = self.on_press_tx {
                b = b.on_press_tx(move |s: &mut S, tx: &mut rune::Tx| f(s, tx));
            }
        }
        b
    }
}

/// `.spinner`: three 5px dots pulsing 1→0.3→1 over 1.4s, staggered 0.2s.
pub fn spinner(color: Color) -> El {
    row().center().gap(2.0).children((0..3).map(|i| {
        rune::el()
            .size(5.0, 5.0)
            .radius(2.5)
            .fill(color)
            .looping(LoopSpec::opacity(1.4, 0.2 * i as f64, 0.3, 1.0))
    }))
}
