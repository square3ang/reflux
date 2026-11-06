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

#[cfg(test)]
mod tests {
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
