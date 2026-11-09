//! In-app context and popout menus (`fluxer_app/src/features/ui/action_menu/ContextMenu.module.css`,
//! `MenuItemRadio.module.css`). Rune's native popup menu is macOS-only, so menus draw in the
//! overlay layer: arrows move a shared pointer/keyboard highlight, Return picks, Escape or a
//! press outside closes.

use std::rc::Rc;

use rune::{Color, El, Icon, Key, Tx, col, el, row, text};
use super::icon;

use super::overlay::{Placement, PopupAccess, Popups, anchor, anchor_at_point, popout_shadow};
use super::{mix, over};
use crate::icons;
use crate::theme::{Palette, font};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Check {
    #[default]
    None,
    Checkbox(bool),
    Radio(bool),
}

pub struct MenuItem<S: 'static> {
    id: String,
    label: String,
    icon: Option<&'static Icon>,
    /// A 16px icon before the label (`MenuItemRadio` `icon`, `.leadingActionLabel`).
    lead: Option<&'static Icon>,
    hint: Option<String>,
    /// `.modeDescription`: an 11/14 text-tertiary line under the label.
    desc: Option<String>,
    shortcut: Option<String>,
    danger: bool,
    disabled: bool,
    check: Check,
    keep_open: bool,
    on_select: Option<Rc<dyn Fn(&mut S, &mut Tx)>>,
}

impl<S: 'static> MenuItem<S> {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            lead: None,
            hint: None,
            desc: None,
            shortcut: None,
            danger: false,
            disabled: false,
            check: Check::None,
            keep_open: false,
            on_select: None,
        }
    }
    pub fn icon(mut self, i: &'static Icon) -> Self { self.icon = Some(i); self }
    pub fn hint(mut self, h: impl Into<String>) -> Self { self.hint = Some(h.into()); self }
    pub fn lead(mut self, i: &'static Icon) -> Self { self.lead = Some(i); self }
    pub fn desc(mut self, d: impl Into<String>) -> Self { self.desc = Some(d.into()); self }
    pub fn shortcut(mut self, k: impl Into<String>) -> Self { self.shortcut = Some(k.into()); self }
    pub fn danger(mut self) -> Self { self.danger = true; self }
    pub fn disabled(mut self, d: bool) -> Self { self.disabled = d; self }
    pub fn checkbox(mut self, on: bool) -> Self { self.check = Check::Checkbox(on); self }
    pub fn radio(mut self, on: bool) -> Self { self.check = Check::Radio(on); self }
    /// `closeOnSelect={false}`: the menu stays open after a pick.
    pub fn keep_open(mut self) -> Self { self.keep_open = true; self }
    /// Runs after the menu closes.
    pub fn on_select(mut self, f: impl Fn(&mut S) + 'static) -> Self { self.on_select = Some(Rc::new(move |s, _| f(s))); self }
    /// [`Self::on_select`] with the press's `Tx` (clipboard writes, Shift-click).
    pub fn on_select_tx(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self { self.on_select = Some(Rc::new(f)); self }
}

pub enum MenuEntry<S: 'static> {
    Item(MenuItem<S>),
    Separator,
    /// `.groupLabel`: a small uppercase heading.
    Group(String),
    /// (id, label, entries). Nested ones open beside their parent (`Popups::sub` holds the
    /// open path, `a/b`).
    Submenu(String, String, Vec<MenuEntry<S>>),
    /// A [`Self::Submenu`] with a hint line under its label, or disabled.
    SubmenuHint { id: String, label: String, hint: Option<String>, disabled: bool, entries: Vec<MenuEntry<S>> },
    /// Any element, e.g. a labelled slider (`MenuSliderType`).
    El(Box<El>),
}

pub struct Menu<S: 'static> {
    id: String,
    entries: Vec<MenuEntry<S>>,
    placement: Placement,
    min_w: f32,
}

impl<S: 'static> Menu<S> {
    /// Opens over the element keyed `id` (a popout menu), or at [`Popups::at`] (a context menu,
    /// see [`Popups::open_at`]).
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), entries: Vec::new(), placement: Placement::BottomStart, min_w: 208.0 }
    }
    pub fn placement(mut self, p: Placement) -> Self { self.placement = p; self }
    pub fn min_w(mut self, w: f32) -> Self { self.min_w = w; self }
    pub fn item(mut self, i: MenuItem<S>) -> Self { self.entries.push(MenuEntry::Item(i)); self }
    pub fn separator(mut self) -> Self { self.entries.push(MenuEntry::Separator); self }
    pub fn entry(mut self, e: MenuEntry<S>) -> Self { self.entries.push(e); self }
    pub fn group(mut self, label: impl Into<String>) -> Self { self.entries.push(MenuEntry::Group(label.into())); self }
    pub fn submenu(mut self, id: impl Into<String>, label: impl Into<String>, entries: Vec<MenuEntry<S>>) -> Self {
        self.entries.push(MenuEntry::Submenu(id.into(), label.into(), entries));
        self
    }

    /// Pushes the menu into `layer` while [`Popups`] says it is open.
    pub fn el(self, popups: &Popups, access: impl PopupAccess<S>, p: &Palette, layer: &mut Vec<El>) {
        if !popups.is_open(&self.id) {
            return;
        }
        let m = surface(&self.id, self.entries, popups, access, p, None).key(format!("{}-menu", self.id)).min_w(self.min_w).modal(true, move |s: &mut S| access(s).close());
        let mut late = Vec::new();
        let m = match popups.at {
            Some(at) => anchor_at_point(&mut late, m, &self.id, at),
            None => anchor(m, &self.id, self.placement, 4.0),
        };
        let _ = layer;
        late.push(m);
        late.into_iter().for_each(super::overlay::late);
    }
}

/// `parent`: the submenu path this surface belongs to (`None` = the menu itself).
fn surface<S: 'static>(menu: &str, entries: Vec<MenuEntry<S>>, popups: &Popups, access: impl PopupAccess<S>, p: &Palette, parent: Option<&str>) -> El {
    let bg = p.form_surface_background;
    let mut c = col()
        .min_w(208.0)
        .max_w(344.0)
        .padding(7.0)
        .fill(bg)
        .border(1.0, over(p.background_modifier_accent, bg))
        .radius(8.0)
        .box_shadow(popout_shadow(p, 0.24, 0.14))
        .focus_group()
        .role("menu");
    // `.separator:first-child, :last-child, + .separator { display: none }`.
    let mut prev_sep = true;
    let n = entries.len();
    for (i, e) in entries.into_iter().enumerate() {
        let is_sep = matches!(e, MenuEntry::Separator);
        if is_sep && (prev_sep || i + 1 == n) {
            continue;
        }
        prev_sep = is_sep;
        c = c.child(match e {
            MenuEntry::Separator => el()
                .h(1.0)
                .mx(2.0)
                .my(4.0)
                .self_stretch()
                .fill(Color { a: p.background_modifier_accent.a * 0.55, ..p.background_modifier_accent }),
            MenuEntry::Group(l) => text(l.to_uppercase())
                .font(font(11.0, 600.0).lh(16.0).tracking(0.22))
                .color(p.text_tertiary)
                .pt(4.0)
                .px(8.0)
                .pb(2.0)
                .mt(2.0),
            MenuEntry::Item(it) => item(menu, it, access, p, parent),
            MenuEntry::El(e) => *e,
            MenuEntry::Submenu(id, label, entries) => submenu(menu, popups, access, p, parent, id, label, None, false, entries),
            MenuEntry::SubmenuHint { id, label, hint, disabled, entries } => submenu(menu, popups, access, p, parent, id, label, hint, disabled, entries),
        });
    }
    if parent.is_none() {
        // Escape and Left close one submenu level first (the popup is modal, so Rune would
        // otherwise dismiss the whole menu).
        c = c.on_key(move |s: &mut S, k: &rune::KeyEvent, _| {
            if !matches!(k.key, Key::Escape | Key::Left) || k.mods != rune::Mods::default() {
                return false;
            }
            close_sub(access(s))
        });
    }
    c
}

/// Closes the deepest open submenu (`a/b` → `a`, `a` → none); false when none is open.
fn close_sub(p: &mut Popups) -> bool {
    match p.sub.take() {
        Some(sub) => {
            p.sub = sub.rsplit_once('/').map(|(outer, _)| outer.to_string());
            true
        }
        None => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn submenu<S: 'static>(menu: &str, popups: &Popups, access: impl PopupAccess<S>, p: &Palette, parent: Option<&str>, id: String, label: String, hint: Option<String>, disabled: bool, entries: Vec<MenuEntry<S>>) -> El {
    let id = match parent {
        Some(par) => format!("{par}/{id}"),
        None => id,
    };
    let key = format!("{menu}-sub-{id}");
    let open = inside(&popups.sub, &id);
    let caret = row().size(18.0, 18.0).center().child(icon(&icons::CARET_RIGHT_BOLD, 14.0).color(p.text_tertiary));
    // In a column, `label_text`'s row grow/shrink would squash the label to no height (it hung
    // 9px below the row's centre); here it only fills the width.
    let mut words = col().grow(1.0).shrink().min_w(0.0).child(text(label.as_str()).font(font(13.0, 500.0).lh(18.0)).color(p.text_primary).truncate().w_full());
    let min_w = hint.as_deref().map(|h| hint_width(h) + 2.0 * 9.0 + 8.0 + 18.0);
    if let Some(h) = hint {
        words = words.child(hint_text(&h, p));
    }
    let mut r = row_shell(p, false).key(key.clone()).child(words).child(caret).role("menuitem").label(label).expanded(open);
    if let Some(w) = min_w {
        r = r.min_w(w);
    }
    if disabled {
        return r.opacity(0.5).disabled(true).cursor(rune::Cursor::NotAllowed);
    }
    // Over a deeper submenu (a descendant), keep it open.
    let (sid, pid) = (id.clone(), id.clone());
    r = r
        .on_hover(move |s: &mut S| {
            let sub = &mut access(s).sub;
            if !inside(sub, &sid) {
                *sub = Some(sid.clone());
            }
        })
        .on_press(move |s: &mut S| access(s).sub = Some(pid.clone()));
    if open {
        // shortcut: rune can't tell us which way the parent flipped, so a nested submenu opens to the
        // left (it flips right when there's no room). Only the developer tools menu nests, and it sits at the right edge.
        let side = if parent.is_some() { Placement::LeftStart } else { Placement::RightStart };
        r = r.fill(p.surface_interactive_hover_bg).child(anchor(
            surface(menu, entries, popups, access, p, Some(&id)),
            &key,
            side,
            8.0,
        ));
    }
    r
}

/// A labelled slider row in a menu (`MenuItemSlider`): label, the formatted value, a mini slider.
#[allow(clippy::too_many_arguments)]
pub fn menu_slider<S: 'static>(key: String, label: &str, value: f32, min: f32, max: f32, shown: String, p: &Palette, on: impl Fn(&mut S, f32) + 'static) -> MenuEntry<S> {
    MenuEntry::El(Box::new(
        col()
            .w_full()
            .px(8.0)
            .py(6.0)
            .gap(4.0)
            .child(
                row()
                    .w_full()
                    .child(text(label.to_string()).font(font(14.0, 500.0).lh(18.0)).color(p.text_primary).grow(1.0))
                    .child(text(shown).font(font(12.0, 500.0).lh(18.0)).color(p.text_tertiary)),
            )
            .child(super::Slider::new(key, value, min, max).mini().label(label).on_change(on).el(p)),
    ))
}

/// `sub` is `id` or a submenu nested in it (`id/…`).
fn inside(sub: &Option<String>, id: &str) -> bool {
    sub.as_deref().is_some_and(|s| s == id || s.strip_prefix(id).is_some_and(|r| r.starts_with('/')))
}

/// `.item`: 32 tall, padding 5×8 inside a transparent 1px border, radius 6.
fn row_shell(p: &Palette, danger: bool) -> El {
    let hover = if danger { mix(p.status_danger, p.surface_interactive_hover_bg, 6.0) } else { p.surface_interactive_hover_bg };
    row()
        .w_full()
        .min_h(32.0)
        .py(6.0)
        .px(9.0)
        .gap(8.0)
        .center_y()
        .radius(6.0)
        .focusable(true)
        .focus_highlight(hover)
        .cursor(rune::Cursor::Pointer)
}

/// A hint line under an item's label. It sizes the menu like CSS max-content (one line up to
/// 320 wide, then wrapping): a wrapping text alone adds no width, and the popup would be measured
/// a line short.
fn hint_text(h: &str, p: &Palette) -> El {
    text(h.to_string()).font(font(12.0, 400.0).lh(16.0)).color(p.text_tertiary_muted).mt(1.0).max_w(320.0).wrap()
}

fn hint_width(h: &str) -> f32 {
    rune::text::measure(h, &[], font(12.0, 400.0).lh(16.0), None).0.min(320.0)
}

fn label_text(l: &str, c: Color) -> El {
    text(l).font(font(13.0, 500.0).lh(18.0)).color(c).truncate().grow(1.0).shrink()
}

fn item<S: 'static>(menu: &str, it: MenuItem<S>, access: impl PopupAccess<S>, p: &Palette, parent: Option<&str>) -> El {
    let fg = if it.disabled {
        p.interactive_muted
    } else if it.danger {
        mix(p.status_danger, p.text_primary, 78.0)
    } else {
        p.text_primary
    };
    let mut words = col().grow(1.0).shrink().min_w(0.0);
    let mut line = row().w_full().gap(16.0).center_y();
    line = match it.lead {
        Some(i) => line.child(row().grow(1.0).shrink().min_w(0.0).gap(8.0).center_y().child(icon(i, 16.0).color(fg)).child(label_text(&it.label, fg))),
        None => line.child(label_text(&it.label, fg)),
    };
    // "Show context menu keybinds" (Accessibility → Keyboard).
    if let Some(k) = it.shortcut.as_ref().filter(|_| crate::look::with(|l| l.menu_shortcuts)) {
        let kc = if it.danger { fg } else { p.text_primary_muted };
        line = line.child(text(k.clone()).font(font(12.0, 400.0).mono()).color(kc));
    }
    words = words.child(line);
    if let Some(d) = &it.desc {
        words = words.gap(2.0).child(text(d.clone()).font(font(11.0, 400.0).lh(14.0)).color(p.text_tertiary).truncate());
    }
    if let Some(h) = &it.hint {
        words = words.child(hint_text(h, p));
    }
    let mut r = row_shell(p, it.danger).key(format!("{menu}-{}", it.id)).child(words).role("menuitem").label(it.label.clone());
    if let Some(h) = &it.hint {
        r = r.description(h.clone());
    }
    if let Some(k) = it.shortcut.as_ref().filter(|_| crate::look::with(|l| l.menu_shortcuts)) {
        r = r.shortcut_hint(k.clone());
    }
    if let Some(d) = &it.desc {
        // Nowrap: the description widens the menu (CSS max-content).
        r = r.min_w(rune::text::measure(d, &[], font(11.0, 400.0).lh(14.0), None).0 + 2.0 * 9.0 + 8.0 + 18.0 + 1.0);
    }
    if let Some(h) = &it.hint {
        // Padding, the gap and the trailing icon around the hint.
        r = r.min_w(hint_width(h) + 2.0 * 9.0 + 8.0 + 18.0);
    }
    let trailing = match it.check {
        Check::None => it.icon.map(|i| icon(i, 18.0).color(fg)),
        Check::Checkbox(on) => {
            let mut b = row().size(16.0, 16.0).center().radius(4.0);
            b = if on {
                b.fill(p.brand_primary)
                    .border(1.0, p.brand_primary_foreground)
                    .child(icon(&icons::CHECK_BOLD, 12.0).color(p.brand_primary_fill))
            } else {
                b.fill(p.form_surface_background).border(1.0, over(p.background_modifier_accent, p.form_surface_background))
            };
            r = r.role("menuitemcheckbox").checked(on);
            Some(b)
        }
        Check::Radio(on) => {
            let mut b = row().size(16.0, 16.0).center().radius(8.0);
            b = if on {
                b.fill(p.brand_primary).border(1.0, p.brand_primary_foreground).child(el().size(6.0, 6.0).radius(3.0).fill(p.brand_primary_fill))
            } else {
                b.fill(p.form_surface_background).border(1.0, over(p.background_modifier_accent, p.form_surface_background))
            };
            r = r.role("menuitemradio").checked(on);
            Some(b)
        }
    };
    if let Some(t) = trailing {
        r = r.child(row().size(18.0, 18.0).center().child(t));
    }
    // Hovering an item closes the submenus open below its own level.
    let level = parent.map(str::to_owned);
    r = r.on_hover(move |s: &mut S| {
        let sub = &mut access(s).sub;
        if sub.as_deref().is_some_and(|x| Some(x) != level.as_deref()) {
            *sub = level.clone();
        }
    });
    if it.disabled {
        return r.opacity(0.5).disabled(true).cursor(rune::Cursor::NotAllowed);
    }
    let (f, keep) = (it.on_select, it.keep_open);
    r.on_press_tx(move |s: &mut S, tx| {
        if !keep {
            access(s).close();
        }
        if let Some(f) = &f {
            f(s, tx);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_closes_one_submenu_level_at_a_time() {
        let mut p = Popups { sub: Some("a/b".into()), ..Default::default() };
        assert!(close_sub(&mut p));
        assert_eq!(p.sub.as_deref(), Some("a"));
        assert!(close_sub(&mut p));
        assert_eq!(p.sub, None);
        assert!(!close_sub(&mut p));
    }
}
