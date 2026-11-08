//! `DateTimePickerField` (`fluxer_app/src/features/ui/components/form/DateTimePickerField`): a
//! read-only field with a calendar button whose popover is react-day-picker's single-month
//! calendar (Sunday first, outside days shown; Fluxer passes no `locale`, so the captions stay
//! English) plus a "Time" row; the shown value is `Intl`'s medium date and short time.
//!
//! The month on show lives in the popup id (`{id}@YYYY-MM`), so the picker needs no state of its
//! own; the time row edits a caller-owned `TextEdit` (`HH:MM`) that [`with_time`] reads back.

use std::rc::Rc;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use rune::{El, Span, TextEdit, col, el, icon, row, text, text_input};

use super::overlay::{Placement, PopupAccess, Popups, anchor, popout_shadow};
use super::{mix, over};
use crate::i18n::t;
use crate::icons;
use crate::theme::{Palette, font};

/// `--rdp-day-size`.
const DAY: f32 = 36.0;

pub struct DatePicker<'a, S: 'static> {
    id: &'a str,
    label: Option<&'a str>,
    description: Option<&'a str>,
    value: Option<NaiveDate>,
    min: Option<NaiveDate>,
    max: Option<NaiveDate>,
    time: Option<(&'a TextEdit, Rc<dyn for<'b> Fn(&'b mut S) -> &'b mut TextEdit>)>,
    on_pick: Option<Rc<dyn Fn(&mut S, NaiveDate)>>,
}

/// `date` at the `HH:MM` typed in the time row (midnight when it doesn't parse).
pub fn with_time(date: NaiveDate, time: &str) -> NaiveDateTime {
    date.and_time(NaiveTime::parse_from_str(time.trim(), "%H:%M").unwrap_or_default())
}

/// The month the popover of `id` shows, while it's open.
fn shown(popups: &Popups, id: &str) -> Option<NaiveDate> {
    let ym = popups.open.as_deref()?.strip_prefix(id)?.strip_prefix('@')?;
    NaiveDate::parse_from_str(&format!("{ym}-01"), "%Y-%m-%d").ok()
}

fn add_months(d: NaiveDate, n: i32) -> NaiveDate {
    let m = d.year() * 12 + d.month0() as i32 + n;
    NaiveDate::from_ymd_opt(m.div_euclid(12), m.rem_euclid(12) as u32 + 1, 1).unwrap_or(d)
}

/// The weeks (Sunday first) covering `month`'s days.
fn weeks(month: NaiveDate) -> Vec<[NaiveDate; 7]> {
    let start = month - Duration::days(month.weekday().num_days_from_sunday() as i64);
    let next = add_months(month, 1);
    (0..6).map(|w| std::array::from_fn(|d| start + Duration::days(w * 7 + d as i64))).take_while(|wk: &[NaiveDate; 7]| wk[0] < next).collect()
}

impl<'a, S: 'static> DatePicker<'a, S> {
    pub fn new(id: &'a str, value: Option<NaiveDate>) -> Self {
        Self { id, label: None, description: None, value, min: None, max: None, time: None, on_pick: None }
    }
    pub fn label(mut self, l: &'a str) -> Self { self.label = Some(l); self }
    pub fn description(mut self, d: &'a str) -> Self { self.description = Some(d); self }
    pub fn min(mut self, d: Option<NaiveDate>) -> Self { self.min = d; self }
    pub fn max(mut self, d: Option<NaiveDate>) -> Self { self.max = d; self }
    /// Shows the "Time" row, editing `edit`.
    pub fn time(mut self, edit: &'a TextEdit, access: impl for<'b> Fn(&'b mut S) -> &'b mut TextEdit + 'static) -> Self {
        self.time = Some((edit, Rc::new(access)));
        self
    }
    pub fn on_pick(mut self, f: impl Fn(&mut S, NaiveDate) + 'static) -> Self { self.on_pick = Some(Rc::new(f)); self }

    pub fn el(self, popups: &Popups, access: impl PopupAccess<S>, p: &Palette, _layer: &mut Vec<El>) -> El {
        let month = shown(popups, self.id);
        let bg = p.form_surface_background;
        let border = if month.is_some() { p.background_modifier_accent_focus } else { p.background_modifier_accent };
        let shown_text = self.value.map(|d| {
            let at = self.time.as_ref().map_or(d.and_time(NaiveTime::MIN), |(e, _)| with_time(d, &e.text));
            crate::desktop::time::medium_date_time(&at.and_utc(), crate::i18n::locale())
        });
        let today = chrono::Local::now().date_naive();
        let (id, start) = (self.id.to_string(), self.value.unwrap_or(today).with_day(1).unwrap_or(today));
        let open_at = move |s: &mut S, m: NaiveDate| {
            let pops = access(s);
            pops.close();
            pops.open = Some(format!("{id}@{}", m.format("%Y-%m")));
        };
        let toggle = {
            let (id, open_at) = (self.id.to_string(), open_at.clone());
            Rc::new(move |s: &mut S| {
                if shown(access(s), &id).is_some() { access(s).close() } else { open_at(s, start) }
            })
        };
        // `.inputWrapper`: 44 tall, radius 8, the read-only text, a divider and the 48px button.
        // The whole field is the button: Tab reaches it, Space / Return opens the calendar.
        let mut field = row()
            .key(self.id)
            .w_full()
            .h(44.0)
            .radius(8.0)
            .fill(bg)
            .border(1.0, over(border, bg))
            .cursor(rune::Cursor::Pointer)
            .role("button")
            .label(self.label.unwrap_or(t("Select a date and time")))
            .focusable(true)
            .focus_ring(4.0, 0.0, p.focus_primary)
            .on_press({
                let toggle = toggle.clone();
                move |s: &mut S| toggle(s)
            })
            // Web fields open on Enter as well as Space.
            .on_key(move |s: &mut S, k, _| {
                if crate::ui::enters(k) {
                    toggle(s);
                    true
                } else {
                    false
                }
            })
            .child(
                el().grow(1.0).min_w(0.0).px(16.0).py(10.0).child(match &shown_text {
                    Some(v) => text(v.clone()).color(p.text_primary),
                    None => text(t("Select a date and time")).color(p.text_primary_muted),
                }.font(font(14.0, 400.0).lh(20.0)).truncate()),
            )
            .child(el().w(1.0).h_full().fill(p.background_modifier_accent))
            .child(
                row()
                    .w(48.0)
                    .h_full()
                    .center()
                    .hover_group()
                    .child(icon(&icons::CALENDAR_BLANK_FILL, 18.0).color(p.text_tertiary).group_color(p.text_primary))
                    .inert(),
            );
        if let Some(d) = self.description {
            field = field.description(d);
        }
        let mut c = col().w_full().gap(4.0);
        if let Some(l) = self.label {
            c = c.child(text(l).font(font(14.0, 500.0).lh(20.0)).color(p.text_primary));
        }
        c = c.child(col().w_full().gap(6.0).child(field));
        if let Some(d) = self.description {
            c = c.child(text(d).font(font(12.0, 400.0).lh(16.0)).color(p.text_primary_muted).wrap());
        }
        let Some(month) = month else { return c };

        // The popover: caption with prev/next, weekday row, the day grid, then the time row.
        let (first, last) = (self.min.map(|d| d.with_day(1).unwrap_or(d)), self.max.map(|d| d.with_day(1).unwrap_or(d)));
        let nav = |ic: &'static rune::Icon, key: &str, label: &str, to: NaiveDate, off: bool| {
            let mut b = row()
                .key(format!("{}-{key}", self.id))
                .size(28.0, 28.0)
                .center()
                .radius(6.0)
                .role("button")
                .label(label.to_string())
                .focusable(!off)
                .focus_ring(4.0, 0.0, p.focus_primary);
            b = b.child(icon(ic, 16.0).color(p.text_tertiary).group_color(p.text_primary)).hover_group();
            if off {
                return b.opacity(0.3).disabled(true);
            }
            let open_at = open_at.clone();
            let enter = open_at.clone();
            b.hover(p.background_modifier_hover)
                .cursor(rune::Cursor::Pointer)
                .on_press(move |s: &mut S| open_at(s, to))
                .on_key(move |s: &mut S, k, _| {
                    if crate::ui::enters(k) {
                        enter(s, to);
                        true
                    } else {
                        false
                    }
                })
        };
        let caption = row()
            .w_full()
            .h(DAY)
            .px(4.0)
            .justify_between()
            .center_y()
            .child(nav(&icons::CARET_LEFT_FILL, "prev", t("Previous month"), add_months(month, -1), first.is_some_and(|f| month <= f)))
            .child(text(month.format("%B %Y").to_string()).font(font(14.0, 600.0).lh(20.0)).color(p.text_primary))
            .child(nav(&icons::CARET_RIGHT_FILL, "next", t("Next month"), add_months(month, 1), last.is_some_and(|l| month >= l)));
        let weekdays = row().role("row").children(["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"].map(|d| row().size(DAY, 28.0).center().child(text(t(d)).font(font(12.0, 500.0).lh(16.0)).color(p.text_primary_muted))));
        let mut grid = col().child(weekdays).focus_group().role("grid").label(t("Select a date and time"));
        for (w, wk) in weeks(month).into_iter().enumerate() {
            grid = grid.child(row().role("row").children(wk.iter().enumerate().map(|(col, &day)| {
                let d = day;
                let label = d.day().to_string();
                let n = label.len();
                let (sel, outside) = (self.value == Some(d), d.month() != month.month());
                let initial = sel || (self.value.is_none() && d == today && !outside);
                let off = self.min.is_some_and(|m| d < m) || self.max.is_some_and(|m| d > m);
                let fg = if sel { p.brand_primary_fill } else if outside { mix(p.text_primary_muted, rune::Color { a: 0.0, ..p.text_primary_muted }, 40.0) } else { p.text_primary };
                let weight = if sel { 600.0 } else if d == today { 700.0 } else { 400.0 };
                let mut num = text(label).font(font(13.0, weight).lh(18.0)).color(fg);
                if d == today && !sel {
                    num = num.spans(vec![Span::new(0, n).underline(p.brand_primary_foreground)]);
                }
                let mut b = row()
                    .key(format!("{}-{d}", self.id))
                    .size(DAY - 4.0, DAY - 4.0)
                    .center()
                    .radius(6.0)
                    .role("button")
                    .label(crate::desktop::time::medium_date(d, crate::i18n::locale()))
                    .focusable(!off)
                    .focus_selected(initial)
                    // The calendar opens on the chosen day, not on the month arrows.
                    .tab_index(if initial { 1 } else { 0 })
                    .child(num);
                if sel {
                    b = b.fill(p.brand_primary);
                }
                if off {
                    b = b.opacity(0.25).cursor(rune::Cursor::NotAllowed).disabled(true);
                } else if let Some(f) = &self.on_pick {
                    let f = f.clone();
                    b = b.cursor(rune::Cursor::Pointer).focus_highlight(p.background_modifier_hover).on_press(move |s: &mut S| f(s, d));
                    if !sel {
                        b = b.hover(p.background_modifier_hover);
                    }
                }
                row().size(DAY, DAY).center().role("gridcell").position_in_set(w * 7 + col + 1, 42).child(b)
            })));
        }
        let mut pop = col().padding(12.0).fill(p.background_primary).border(1.0, over(p.background_modifier_accent, p.background_primary)).radius(8.0).box_shadow(popout_shadow(p, 0.24, 0.24)).gap(4.0).child(caption).child(grid);
        if let Some((edit, acc)) = self.time {
            let input = text_input(edit, font(13.0, 400.0).lh(18.0))
                .single_line()
                .key(format!("{}-time", self.id))
                .label(t("Time"))
                .grow(1.0)
                .color(p.text_primary)
                .placeholder("00:00", p.text_tertiary)
                .caret(p.text_primary, p.background_modifier_selected)
                .on_edit(move |s: &mut S| -> &mut TextEdit { acc(s) });
            let surface = p.background_secondary;
            pop = pop.child(el().w_full().h(1.0).mt(8.0).fill(p.background_modifier_accent)).child(
                row()
                    .w_full()
                    .pt(10.0)
                    .gap(8.0)
                    .center_y()
                    .child(text(t("Time")).font(font(13.0, 500.0).lh(18.0)).color(p.text_primary_muted))
                    .child(row().grow(1.0).px(8.0).py(5.0).radius(6.0).fill(surface).border(1.0, over(p.background_modifier_accent, surface)).child(input)),
            );
        }
        super::overlay::late(anchor(pop.key(format!("{}-picker", self.id)).role("dialog").label(t("Date picker")).modal(true, move |s: &mut S| access(s).close()), self.id, Placement::BottomStart, 8.0));
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::Key;

    #[derive(Default)]
    struct State {
        pops: Popups,
        picked: Option<NaiveDate>,
    }

    fn view(s: &State, _: &mut rune::Cx<State>) -> El {
        let p = crate::theme::palette(crate::theme::Kind::Dark);
        let mut layer = Vec::new();
        let pops = s.pops.clone();
        let field = DatePicker::new("dp", s.picked).label("Date").on_pick(|s: &mut State, d| {
            s.picked = Some(d);
            s.pops.close();
        }).el(&pops, |s: &mut State| &mut s.pops, p, &mut layer);
        crate::ui::root(rune::col().w_full().h_full().child(field), layer)
    }

    fn press(rt: &mut rune::Runtime<State>, k: Key) {
        rt.key(rune::KeyEvent { key: k, mods: rune::Mods::default() });
        rt.frame(0.0);
    }

    #[test]
    fn calendar_is_keyboard_reachable() {
        let mut rt = rune::Runtime::with_text(State::default(), view, (500.0, 500.0), rune::Env::new(true, 1.0), rune::text::TextSystem::new(crate::theme::FONTS, crate::theme::FAMILIES));
        rt.frame(0.0);
        press(&mut rt, Key::Tab);
        let field = rt.elements().into_iter().find(|e| e.focused).expect("the field takes Tab");
        assert_eq!((field.key.as_deref(), field.role), (Some("dp"), "button"));
        press(&mut rt, Key::Text(" ".into()));
        assert!(rt.state.pops.open.as_deref().is_some_and(|o| o.starts_with("dp@")));
        let day = rt.elements().into_iter().find(|e| e.focused).expect("a day takes focus");
        assert!(day.key.as_deref().is_some_and(|k| k.starts_with("dp-20")), "{:?}", day.key);
        press(&mut rt, Key::Right);
        let next = rt.elements().into_iter().find(|e| e.focused).expect("arrow moves the day");
        assert_ne!(next.key, day.key);
        press(&mut rt, Key::Enter);
        assert!(rt.state.picked.is_some(), "Return picks the focused day");
        assert!(rt.state.pops.open.is_none(), "picking closes the calendar");
    }

    #[test]
    fn months_and_weeks() {
        let nov = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
        assert_eq!(add_months(nov, 2), NaiveDate::from_ymd_opt(2027, 1, 1).unwrap());
        assert_eq!(add_months(nov, -11), NaiveDate::from_ymd_opt(2025, 12, 1).unwrap());
        let w = weeks(nov);
        // Nov 1 2026 is a Sunday; 30 days → 5 weeks.
        assert_eq!((w.len(), w[0][0], w[4][6].day()), (5, nov, 5));
        assert_eq!(with_time(nov, "13:45").format("%H:%M").to_string(), "13:45");
        let pops = Popups { open: Some("d@2026-11".into()), ..Default::default() };
        assert_eq!(shown(&pops, "d"), Some(nov));
        assert_eq!(shown(&pops, "x"), None);
    }
}
