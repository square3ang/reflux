//! The main window: its title with the unread badge (`useFluxerDocumentTitle.ts`), the
//! remembered size (`Window.ts` `window-state.json`) and the app zoom
//! (`Accessibility.zoomLevel`, Ctrl+= / Ctrl+- / Ctrl+0).
// shortcut: only the size is remembered. Rune reports no window position or maximized state
// (and Wayland can't place windows), so those aren't restored.
// shortcut: no minimize-to-tray; Rune sends no minimize event to hide on.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use reflux_model::ChannelType;
use reflux_store::Store;
use rune::{El, col, el};

use crate::State;
use crate::shell::ShellState;

pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 2.0;

static ZOOM: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0

pub fn zoom() -> f32 {
    f32::from_bits(ZOOM.load(Ordering::Relaxed))
}

pub fn set_zoom(z: f32) {
    ZOOM.store(clamp_zoom(z).to_bits(), Ordering::Relaxed);
}

/// `clampZoomLevel`: whole percents in 50..=200.
pub fn clamp_zoom(z: f32) -> f32 {
    if !z.is_finite() {
        return 1.0;
    }
    (z * 100.0).round().clamp(ZOOM_MIN * 100.0, ZOOM_MAX * 100.0) / 100.0
}

/// `nextZoomLevel`: the next multiple of 10% up or down.
pub fn step_zoom(z: f32, up: bool) -> f32 {
    let pct = (clamp_zoom(z) * 100.0).round() as i32;
    let next = if up { ((pct + 1) as f32 / 10.0).ceil() as i32 * 10 } else { ((pct - 1) as f32 / 10.0).floor() as i32 * 10 };
    clamp_zoom(next as f32 / 100.0)
}

/// The root: records the window size, and draws the app scaled by the zoom. The content lays
/// out at the window size divided by the zoom, then `scale` draws it back to the window's size
/// (text re-rasterizes crisp at the scaled size).
pub fn root(content: El, size: [f32; 2]) -> El {
    let z = zoom();
    let root = el().w_full().h_full().on_layout(|s: &mut State, r| s.desktop.size = [r[2], r[3]]);
    if (z - 1.0).abs() < 0.005 || size[0] <= 0.0 {
        return root.child(content);
    }
    root.child(col().absolute(0.0, 0.0).w(size[0] / z).h(size[1] / z).scale(z).origin(0.0, 0.0).child(content))
}

/// A window point (pointer, context menu) in the zoomed content's points.
pub fn unzoom(at: [f32; 2]) -> [f32; 2] {
    let z = zoom();
    [at[0] / z, at[1] / z]
}

/// The initial window size: the remembered one in a windowed run.
pub fn initial_size(p: &super::Prefs, default: (f32, f32)) -> (f32, f32) {
    match p.window.filter(|_| p.remember_window && !rune::is_headless()) {
        Some([w, h]) if w >= 420.0 && h >= 400.0 => (w, h),
        _ => default,
    }
}

/// Saves a changed size once it has held still for a second (a drag resizes every frame).
pub fn tick(s: &mut State, tx: &mut rune::Tx) {
    let p = &s.app.saved.desktop;
    if !p.remember_window || rune::is_headless() || s.desktop.size[0] <= 0.0 || p.window == Some(s.desktop.size) {
        s.desktop.size_changed = None;
        return;
    }
    let since = *s.desktop.size_changed.get_or_insert_with(Instant::now);
    if since.elapsed() >= Duration::from_secs(1) {
        save_size(s);
    } else {
        tx.wake_after(Duration::from_secs(1));
    }
}

pub fn save_size(s: &mut State) {
    let size = s.desktop.size;
    let p = &mut s.app.saved.desktop;
    if p.remember_window && size[0] > 0.0 && p.window != Some(size) && !rune::is_headless() {
        p.window = Some(size);
        crate::session::save(&s.app.saved);
    }
    s.desktop.size_changed = None;
}

/// `useFluxerDocumentTitle`: "(3) Fluxer | #general | Guild", "• Fluxer" while only unread.
pub fn title(store: &Store, sh: &ShellState, mentions: u32, unread: bool) -> String {
    let mut parts = vec!["Fluxer".to_string()];
    if let Some(c) = sh.channel.and_then(|c| store.channels.get(&c)) {
        match c.kind {
            ChannelType::DM | ChannelType::GROUP_DM => parts.push(crate::shell::channel_title(store, c)),
            _ => {
                parts.push(format!("#{}", c.name.clone().unwrap_or_default()));
                parts.extend(c.guild_id.and_then(|g| store.guilds.get(&g)?.info.as_ref()).map(|g| g.name.clone()));
            }
        }
    } else if sh.guild.is_none() && sh.channel.is_none() {
        parts.push(crate::i18n::t("My friends").to_string());
    }
    let base = parts.join(" | ");
    if mentions > 0 {
        format!("({mentions}) {base}")
    } else if unread {
        format!("• {base}")
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_by_ten_percent_within_bounds() {
        assert_eq!(step_zoom(1.0, true), 1.1);
        assert_eq!(step_zoom(1.05, true), 1.1);
        assert_eq!(step_zoom(1.05, false), 1.0);
        assert_eq!(step_zoom(0.5, false), 0.5);
        assert_eq!(step_zoom(2.0, true), 2.0);
        assert_eq!(clamp_zoom(f32::NAN), 1.0);
    }

    #[test]
    fn title_badges() {
        let store = Store::default();
        let sh = ShellState::default();
        assert_eq!(title(&store, &sh, 3, true), "(3) Fluxer | My friends");
        assert_eq!(title(&store, &sh, 0, true), "• Fluxer | My friends");
    }
}
