//! The main window: its title with the unread badge (`useFluxerDocumentTitle.ts`), the
//! remembered size (`Window.ts` `window-state.json`) and the app zoom
//! (`Accessibility.zoomLevel`, Ctrl+= / Ctrl+- / Ctrl+0).
// shortcut: only the size is remembered. Rune reports no window position or maximized state
// (and Wayland can't place windows), so those aren't restored.
// shortcut: no minimize-to-tray; Rune sends no minimize event to hide on.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use reflux_model::ChannelType;
use reflux_store::Store;
use rune::{El, el};

use crate::State;
use crate::shell::ShellState;

pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 2.0;

static ZOOM: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0
/// The main window's native id (X11 XID / HWND), as screen capture lists it.
static OWN: AtomicU64 = AtomicU64::new(0);

/// `App::on_window`: remember the native window (`getMediaSourceId`) and give it the badge.
pub fn on_window(w: &rune::WindowHandle) {
    use rune::raw_window_handle::RawWindowHandle;
    let id = w.with_raw_handle(|h| match h.as_raw() {
        // `c_ulong`: u64 here, u32 on 32-bit targets.
        #[allow(clippy::unnecessary_cast)]
        RawWindowHandle::Xlib(x) => x.window as u64,
        RawWindowHandle::Xcb(x) => x.window.get() as u64,
        RawWindowHandle::Win32(x) => x.hwnd.get() as u64,
        _ => 0,
    });
    OWN.store(id.unwrap_or(0), Ordering::Relaxed);
    MAIN.set(Some(w.clone()));
    super::badge::on_window(w);
    #[cfg(target_os = "linux")]
    super::wl_drop::on_window(w);
}

thread_local! {
    static MAIN: std::cell::RefCell<Option<rune::WindowHandle>> = const { std::cell::RefCell::new(None) };
    /// The main window was last seen full screen while `watch`ed.
    static WAS_FULL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The main window left full screen since the last call, read from its placement while `watch`
/// (off, it forgets, so a full screen still being entered doesn't read as left).
// shortcut: Wayland reports no placement, so leaving full screen from the window manager isn't
// noticed there; the next exit enters it again.
pub fn left_full_screen(watch: bool) -> bool {
    let now = watch && MAIN.with_borrow(|w| w.as_ref().and_then(|w| w.placement()).is_some_and(|p| p.full_screen));
    WAS_FULL.replace(now) && !now
}

/// A capture source id is one of our windows (`isOwnWindow`).
pub fn is_own(id: u64) -> bool {
    id != 0 && id == OWN.load(Ordering::Relaxed)
}

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

/// `--native-titlebar-height` with Fluxer's own chrome (`useNativeTitleBar`), in CSS pixels.
pub const TITLEBAR: f32 = 32.0;

/// `.fluxer-native-titlebar` (`NativeTitlebarShell`) where Rune replaced the native titlebar
/// (Windows, macOS; `App::titlebar`): a 32px band of `--background-secondary` that drags the
/// window, with the wordmark at its start (after the traffic lights on macOS, where it sits at
/// the end). Rune draws the Windows caption buttons over its end. None elsewhere and in full
/// screen, where `guildsLayoutContainer` drops the padding.
// shortcut: the caption buttons are Rune's (46pt, Windows' own glyphs) rather than Fluxer's
// 48px Phosphor ones.
pub fn titlebar(cx: &mut rune::Cx<State>, palette: impl FnOnce() -> crate::theme::Palette) -> Option<El> {
    let insets = cx.chrome_insets();
    if insets.height <= 0.0 || (insets.leading <= 0.0 && insets.trailing <= 0.0) {
        return None;
    }
    let p = &palette();
    // Fluxer's titlebar is fixed px (outside its rem zoom): window points here, divided back out
    // of the zoomed layout. The insets already come in the view's points.
    let z = zoom();
    cx.titlebar_height(TITLEBAR);
    cx.caption_color(p.text_secondary);
    // `.fluxer-native-titlebar__wordmark`: 13px tall, `--text-primary-muted` at 80%.
    let (h, w) = (13.0 / z, 13.0 * 1374.0 / 366.0 / z);
    let mark = el().size(w, h).opacity(0.8).child(rune::icon(&crate::login::WORDMARK, w).color(p.text_primary_muted).absolute(0.0, 0.0)).label("Fluxer");
    let band = rune::row().key("titlebar").w_full().h(TITLEBAR / z).fill(p.background_secondary).center_y().window_drag().unmirrored();
    Some(if insets.leading > 0.0 {
        band.pl(insets.leading).pr(12.0 / z).child(el().grow(1.0)).child(mark)
    } else {
        band.pl(12.0 / z).pr(insets.trailing).child(mark)
    })
}

/// The root: zooms the app as Electron's `setZoomFactor` does (`Cx::zoom`), and records the
/// window size in window points (the layout is in zoomed ones).
pub fn root(cx: &mut rune::Cx<State>, content: El) -> El {
    cx.zoom(zoom());
    let dir = if crate::i18n::rtl() { rune::Direction::Rtl } else { rune::Direction::Ltr };
    el().w_full().h_full().dir(dir).on_layout(|s: &mut State, r| s.desktop.size = [r[2] * zoom(), r[3] * zoom()]).child(content)
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
