//! `Accessibility.showNeko` / `keepNekoStill`: oneko's cat chases the pointer. The sheet is
//! Fluxer's (`assets/media/images/neko.png`); only the idle and four look frames are drawn.
//! shortcut: walk/scratch/sleep animations and fling physics are left out; add them with a
//! per-frame tick (the shell has no animation clock for overlays).

use rune::{El, col, el};

use super::{BODY_KEY, ShellAccess, ShellState};
use crate::theme::Palette;

#[derive(Clone, Debug, PartialEq)]
pub struct Neko {
    /// Top-left in window points; `None` until the first frame (bottom-right).
    pub at: Option<(f32, f32)>,
    /// Last pointer point, where the cat heads.
    pub target: (f32, f32),
    /// 0 idle, 1 north, 2 south, 3 east, 4 west.
    pub facing: u8,
    /// Where a drag started (window points).
    pub drag: Option<(f32, f32)>,
}

impl Default for Neko {
    fn default() -> Self {
        Self { at: None, target: (0.0, 0.0), facing: 0, drag: None }
    }
}

/// One step: `speed` points toward the pointer at most.
fn advance(neko: &mut Neko, x: f32, y: f32, speed: f32) {
    let (cx, cy) = neko.at.unwrap_or((x, y));
    let (dx, dy) = (x - cx, y - cy);
    let dist = dx.hypot(dy);
    let step = dist.min(speed);
    let (nx, ny) = if dist > 0.5 { (cx + dx / dist * step, cy + dy / dist * step) } else { (cx, cy) };
    neko.at = Some((nx, ny));
    neko.target = (x, y);
    neko.facing = if dx.abs() > dy.abs() { if dx < 0.0 { 4 } else { 3 } } else if dy < 0.0 { 1 } else { 2 };
}

/// Moves the cat a step toward the pointer (`Accessibility.showNeko`).
pub fn track(st: &mut ShellState, x: f32, y: f32) {
    if st.neko.drag.is_some() || !crate::look::with(|l| l.show_neko && !l.keep_neko_still) {
        return;
    }
    advance(&mut st.neko, x, y, 12.0);
}

/// The cat overlay, above everything (its own element takes no pointer input).
pub fn view<S: 'static>(st: &ShellState, _p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if !crate::look::with(|l| l.show_neko) {
        return None;
    }
    let [w, h] = rune::memo::layout_size(BODY_KEY).unwrap_or([1280.0, 800.0]);
    let keep = crate::look::with(|l| l.keep_neko_still);
    let (x, y) = st.neko.at.unwrap_or((w - 64.0, h - 96.0));
    let (x, y) = (x.clamp(0.0, (w - 32.0).max(0.0)), y.clamp(0.0, (h - 32.0).max(0.0)));
    let cell = if keep { (3, 3) } else { [(3, 3), (8, 2), (8, 3), (8, 1), (8, 0)][st.neko.facing.min(4) as usize] };
    let glyph = match st.media.neko(cell) {
        crate::media::Load::Ready(px) => el().size(32.0, 32.0).pixels_fit(&px, rune::SurfaceFit::Fit),
        _ => el().size(32.0, 32.0),
    };
    Some(
        col().absolute(x, y).size(32.0, 32.0).cursor(rune::Cursor::Pointer).label("Neko").child(glyph).on_drag(move |s: &mut S, d: &mut rune::Drag, _| {
            let sh = acc(s);
            match d.phase {
                rune::Phase::Down => sh.neko.drag = Some((x, y)),
                rune::Phase::Move => {
                    if let Some((x0, y0)) = sh.neko.drag {
                        sh.neko.at = Some(((x0 + d.dx).clamp(0.0, (w - 32.0).max(0.0)), (y0 + d.dy).clamp(0.0, (h - 32.0).max(0.0))));
                    }
                }
                rune::Phase::Up => sh.neko.drag = None,
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_toward_the_pointer_and_faces_it() {
        let mut n = Neko { at: Some((100.0, 100.0)), ..Default::default() };
        advance(&mut n, 300.0, 100.0, 12.0);
        assert_eq!(n.at, Some((112.0, 100.0)));
        assert_eq!(n.facing, 3);
        advance(&mut n, 112.0, 160.0, 12.0);
        assert_eq!(n.facing, 2);
        for _ in 0..8 {
            advance(&mut n, 112.0, 160.0, 12.0);
        }
        assert_eq!(n.at, Some((112.0, 160.0)), "stops on the pointer");
    }
}
