//! Small pieces: `<MentionBadge>`, `<NotificationDot>`, `<Spinner>` and `<Scroller>`
//! (`components/{MentionBadge,NotificationDot,Spinner}.module.css`, `scroller/ScrollerTrack.module.css`).

use rune::{Color, El, LoopSpec, Shadow, el, row, text};
use rune_kit::{ScrollBar, ScrollBarStyle};

use super::rgba;
use crate::theme::{Palette, dim, font};

/// `formatMentionCount`: 1–99 as is, then `99+`, then compact (`1K`, `12K`, `3M`).
pub fn mention_count(n: u64) -> String {
    match n {
        0..=99 => n.to_string(),
        100..=999 => "99+".into(),
        1_000..=999_999 => format!("{}K", (n as f64 / 1e3).round() as u64),
        1_000_000..=999_999_999 => format!("{}M", (n as f64 / 1e6).round() as u64),
        _ => format!("{}B", (n as f64 / 1e9).round() as u64),
    }
}

/// `<MentionBadge>`: the red count pill (medium 24 tall, small 20). Nothing for 0.
pub fn mention_badge(count: u64, small: bool, p: &Palette) -> Option<El> {
    if count == 0 {
        return None;
    }
    let (h, py, style) = if small {
        (dim::MENTION_BADGE_SMALL_SIZE, 4.0, font(11.0, 600.0))
    } else {
        (dim::MENTION_BADGE_MEDIUM_SIZE, 6.0, font(12.0, 600.0).lh(16.0))
    };
    Some(
        row()
            .h(h)
            .min_w(dim::MENTION_BADGE_SMALL_SIZE)
            .py(py)
            .px(6.0)
            .center()
            .radius(6.0)
            .fill(p.status_danger)
            .box_shadow(Shadow { x: 0.0, y: 4.0, blur: 6.0, spread: -1.0, color: rgba(0, 0, 0, 0.1), inset: false })
            .box_shadow(Shadow { x: 0.0, y: 2.0, blur: 4.0, spread: -2.0, color: rgba(0, 0, 0, 0.1), inset: false })
            .child(text(mention_count(count)).font(style).color(Color::WHITE))
            .inert(),
    )
}

/// `<NotificationDot>`: an 8px `status-danger` dot.
pub fn notification_dot(p: &Palette) -> El {
    el().size(8.0, 8.0).radius(4.0).fill(p.status_danger).inert()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpinnerSize {
    Small,
    #[default]
    Medium,
    Large,
}

/// `<Spinner>`: three `text-secondary` dots pulsing over 1.4s, staggered .2s.
pub fn spinner(size: SpinnerSize, p: &Palette) -> El {
    let d = match size {
        SpinnerSize::Small => 6.0,
        SpinnerSize::Medium => 8.0,
        SpinnerSize::Large => 10.0,
    };
    // shortcut: the keyframes also scale each dot to .8; Rune's loop here is opacity only.
    row().center().gap(2.0).children((0..3).map(move |i| {
        el().size(d, d).radius(4.0).fill(p.text_secondary).looping(LoopSpec::opacity(1.4, 0.2 * i as f64, 0.3, 1.0))
    }))
}

/// `<Scroller scrollbar="thin" | "regular">` on a vertical [`El::scroll`] viewport: a
/// `scrollbar-thumb-bg` pill (4 wide thin, 8 regular), `-hover` under the pointer.
pub fn scroller(viewport: El, regular: bool, p: &Palette) -> El {
    // shortcut: rune-kit's bar keeps its own 2pt edge gap and 24pt minimum thumb; Fluxer's regular
    // bar sits 4 in with a 40 minimum. Upgrade if a regular scroller looks off.
    let w = if regular { 8.0 } else { 4.0 };
    viewport.scroll_bar_styled(
        ScrollBarStyle::neutral(true).thumb(p.scrollbar_thumb_bg).thumb_hover(p.scrollbar_thumb_bg_hover).width(w).hover_width(w),
    )
}

/// Points Rune scrolls per wheel line (`wheel_delta`'s `LineDelta`).
const RUNE_LINE: f32 = 40.0;
/// Points Chromium scrolls per wheel detent: 120 on Wayland, X11 and Windows since
/// `ozone/wayland: Fix high resolution scroll handling` (M140; Fluxer's Electron 44 is M146).
const WHEEL_STEP: f32 = 120.0;

/// A wheel glide from `.0` to `.1`, set at `.2`.
type Glide = (f32, f32, std::time::Instant);

/// How long after a tick the next one still adds to its glide (the glide is ~0.15 s).
const GLIDE_FOR: std::time::Duration = std::time::Duration::from_millis(400);

/// Where the next tick starts from: the running glide's target while the offset is still on its
/// way there, else the offset. A target the offset left (older messages loading above re-anchor
/// it, the list following its end) would send the next tick back to it, thousands of points off.
fn wheel_base(at: f32, glide: Option<Glide>, now: std::time::Instant) -> f32 {
    match glide {
        Some((from, to, t)) if now.duration_since(t) < GLIDE_FOR && at >= from.min(to) - 2.0 && at <= from.max(to) + 2.0 => to,
        _ => at,
    }
}

thread_local! {
    /// Each [`smooth_wheel`] viewport's last scroll position and its glide: (from, to, when).
    static WHEEL: std::cell::RefCell<std::collections::HashMap<String, (rune::ScrollPos, Option<Glide>)>> = Default::default();
}

/// Chromium's smooth wheel scrolling on a vertical scroll viewport: a notched wheel's line
/// steps go Chromium's [`WHEEL_STEP`] and glide instead of jumping, each adding to where the glide was heading. Trackpads and
/// free-spinning wheels (`precise`) and modified wheels scroll as before. `key` names a marker
/// (`{key}-wheel`, absolute and empty) the glide finds the viewport by, so it must be unique
/// among what's on screen; the viewport's own key is left alone.
pub fn smooth_wheel<S: 'static>(viewport: El, key: &str) -> El {
    smooth_wheel_with::<S>(viewport, key, |_| {})
}

/// [`smooth_wheel`] for a viewport that also follows its offset with `f` (`on_scroll_offset`).
pub fn smooth_wheel_with<S: 'static>(viewport: El, key: &str, f: impl Fn(rune::ScrollPos) + 'static) -> El {
    let mark = format!("{key}-wheel");
    let (k1, k2) = (mark.clone(), mark.clone());
    viewport
        .child(el().absolute(0.0, 0.0).size(0.0, 0.0).inert().key(mark))
        .on_scroll_offset(move |pos| {
            WHEEL.with_borrow_mut(|m| m.entry(k1.clone()).or_insert((pos, None)).0 = pos);
            f(pos);
        })
        .on_wheel(move |_: &mut S, e: rune::WheelEvent, tx: &mut rune::Tx| {
            let m = e.mods;
            if e.precise || e.dx != 0.0 || e.dy == 0.0 || m.cmd || m.ctrl || m.alt || m.shift {
                return false;
            }
            let Some(to) = WHEEL.with_borrow_mut(|w| {
                let (pos, aim) = w.get_mut(&k2)?;
                let at = pos.offset[1];
                let now = std::time::Instant::now();
                let to = (wheel_base(at, *aim, now) - e.dy * WHEEL_STEP / RUNE_LINE).clamp(0.0, pos.max[1].max(0.0));
                *aim = Some((at, to, now));
                Some((pos.offset[0], to))
            }) else {
                return false;
            };
            tx.scroll_to_with(k2.clone(), to.0, to.1, rune::Spring::new(0.15, 0.0));
            true
        })
}

/// [`smooth_wheel`] chained after `.scroll(…)`.
pub trait SmoothWheel {
    fn smooth_wheel<S: 'static>(self, key: impl AsRef<str>) -> El;
}

impl SmoothWheel for El {
    fn smooth_wheel<S: 'static>(self, key: impl AsRef<str>) -> El {
        smooth_wheel::<S>(self, key.as_ref())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_tick_adds_to_a_running_glide_only() {
        use super::wheel_base;
        use std::time::{Duration, Instant};
        let t = Instant::now();
        assert_eq!(wheel_base(50.0, Some((0.0, 120.0, t)), t), 120.0, "on its way: adds to the target");
        assert_eq!(wheel_base(3050.0, Some((0.0, 120.0, t)), t), 3050.0, "re-anchored: from where it is");
        assert_eq!(wheel_base(50.0, Some((0.0, 120.0, t)), t + Duration::from_secs(1)), 50.0, "an old glide");
        assert_eq!(wheel_base(50.0, None, t), 50.0);
    }

    use super::*;

    #[test]
    fn mention_count_formats_like_fluxer() {
        assert_eq!(mention_count(7), "7");
        assert_eq!(mention_count(99), "99");
        assert_eq!(mention_count(100), "99+");
        assert_eq!(mention_count(1_499), "1K");
        assert_eq!(mention_count(12_600), "13K");
    }
}
