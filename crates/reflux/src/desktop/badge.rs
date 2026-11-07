//! The app badge (`AppBadgeCount.ts` + the desktop's `applyUnreadBadge`): a count of mentions and
//! pending friend requests, or a dot for unread messages. Linux: Unity LauncherEntry; Windows:
//! the taskbar overlay icon (`WindowsBadge.ts`, Fluxer's `badge-N.ico`); macOS: the Dock label.

/// `UNREAD_INDICATOR`.
pub const DOT: i32 = -1;

/// `computeAppBadge`: the count when there is one, else the dot for unread messages when
/// "Enable unread message badge" is on.
pub fn compute(mentions: u32, friend_requests: u32, unread: bool, unread_badge: bool) -> i32 {
    let total = mentions.saturating_add(friend_requests).min(i32::MAX as u32) as i32;
    if total > 0 {
        total
    } else if unread && unread_badge {
        DOT
    } else {
        0
    }
}

/// Unity LauncherEntry: the count on the dock icon (KDE, Plank, GNOME's Dash to Dock) and its
/// urgency, as Electron's `app.setBadgeCount` does on Linux (no dot).
#[cfg(target_os = "linux")]
pub fn set(badge: i32, urgent: bool) {
    if rune::is_headless() {
        return;
    }
    let count = badge.max(0);
    std::thread::spawn(move || {
        use std::collections::HashMap;
        use zbus::zvariant::Value;
        let r = (|| -> zbus::Result<()> {
            let conn = zbus::blocking::Connection::session()?;
            let props: HashMap<&str, Value> =
                HashMap::from([("count", Value::from(count as i64)), ("count-visible", Value::from(count > 0)), ("urgent", Value::from(urgent))]);
            let uri = format!("application://{}.desktop", super::notify::DESKTOP_ID);
            conn.emit_signal(None::<&str>, "/com/canonical/unity/launcherentry/reflux", "com.canonical.Unity.LauncherEntry", "Update", &(uri, props))
        })();
        if let Err(e) = r {
            eprintln!("reflux: dock badge: {e}");
        }
    });
}

/// `app.dock.setBadge`: the count, "•" for the dot, nothing for 0. Runs on the main thread
/// (the UI tick).
#[cfg(target_os = "macos")]
pub fn set(badge: i32, _urgent: bool) {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{MainThreadMarker, NSString};
    let Some(mtm) = MainThreadMarker::new() else { return };
    let label = match badge {
        DOT => Some(NSString::from_str("•")),
        n if n > 0 => Some(NSString::from_str(&n.to_string())),
        _ => None,
    };
    // SAFETY: AppKit calls on the main thread (checked by `mtm`).
    unsafe { NSApplication::sharedApplication(mtm).dockTile().setBadgeLabel(label.as_deref()) };
}

#[cfg(windows)]
pub use win::{on_window, set};

#[cfg(windows)]
mod win {
    use std::cell::{Cell, RefCell};

    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconFromResourceEx, DestroyIcon, HICON, LR_DEFAULTCOLOR};
    use windows::core::HSTRING;

    use crate::i18n::{t, tf};

    /// `badge-1.ico` … `badge-10.ico` (10 = "10+"), `badge-11.ico` = the dot.
    const ICONS: [&[u8]; 11] = {
        macro_rules! ico {
            ($($n:literal),*) => { [$(include_bytes!(concat!("../../../../assets/media/badges/badge-", $n, ".ico"))),*] };
        }
        ico!("1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11")
    };

    thread_local! {
        static HWND_: Cell<isize> = const { Cell::new(0) };
        static LAST: Cell<i32> = const { Cell::new(0) };
        static TASKBAR: RefCell<Option<ITaskbarList3>> = const { RefCell::new(None) };
    }

    /// `App::on_window`: remember the main window and re-apply the badge to a new one
    /// (`refreshWindowsBadgeOverlay`).
    pub fn on_window(w: &rune::WindowHandle) {
        use rune::raw_window_handle::RawWindowHandle;
        let hwnd = w.with_raw_handle(|h| match h.as_raw() {
            RawWindowHandle::Win32(h) => h.hwnd.get(),
            _ => 0,
        });
        HWND_.set(hwnd.unwrap_or(0));
        set(LAST.get(), false);
    }

    /// `readIconPngFrames` + `pickBadgeFrameSize`: the smallest PNG frame at least 16 px × scale.
    fn frame(ico: &[u8], target: u32) -> Option<(&[u8], i32)> {
        let u16_at = |i: usize| ico.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        let u32_at = |i: usize| ico.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        if u16_at(2)? != 1 {
            return None;
        }
        let mut frames: Vec<(u32, &[u8])> = (0..u16_at(4)? as usize)
            .filter_map(|i| {
                let e = 6 + i * 16;
                let size = match *ico.get(e)? {
                    0 => 256,
                    s => s as u32,
                };
                let (len, off) = (u32_at(e + 8)? as usize, u32_at(e + 12)? as usize);
                let png = ico.get(off..off.checked_add(len)?)?;
                png.starts_with(b"\x89PNG").then_some((size, png))
            })
            .collect();
        frames.sort_by_key(|f| f.0);
        let &(size, png) = frames.iter().find(|f| f.0 >= target).or(frames.last())?;
        Some((png, size as i32))
    }

    /// `ITaskbarList3::SetOverlayIcon` with the badge's icon and `describeUnreadBadge`. On the UI thread.
    pub fn set(badge: i32, _urgent: bool) {
        LAST.set(badge);
        let hwnd = HWND(HWND_.get() as *mut _);
        if hwnd.0.is_null() {
            return;
        }
        let index = match badge {
            super::DOT => Some(10),
            n if n > 0 => Some(n.min(10) as usize - 1),
            _ => None,
        };
        let description = match badge {
            super::DOT => t("Unread messages").to_string(),
            n if n > 0 => tf("Unread messages: {count}", &[("count", &n.to_string())]),
            _ => "No Notifications".into(),
        };
        TASKBAR.with_borrow_mut(|tb| {
            if tb.is_none() {
                // SAFETY: COM setup on this (the UI) thread; an already-initialised apartment is fine.
                *tb = unsafe {
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                    CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok().filter(|t| t.HrInit().is_ok())
                };
            }
            let Some(tb) = tb else { return };
            // SAFETY: a live window handle from `on_window`; the taskbar copies the icon, so it's
            // destroyed right after.
            unsafe {
                let target = 16 * GetDpiForWindow(hwnd).max(96) / 96;
                let icon = index.and_then(|i| frame(ICONS[i], target)).and_then(|(png, size)| CreateIconFromResourceEx(png, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR).ok());
                if let Err(e) = tb.SetOverlayIcon(hwnd, icon.unwrap_or(HICON::default()), &HSTRING::from(description)) {
                    eprintln!("reflux: taskbar badge: {e}");
                }
                if let Some(icon) = icon {
                    let _ = DestroyIcon(icon);
                }
            }
        });
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn set(_badge: i32, _urgent: bool) {}

/// The window's native handle only matters to the Windows overlay.
#[cfg(not(windows))]
pub fn on_window(_: &rune::WindowHandle) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_beat_the_unread_dot() {
        assert_eq!(compute(2, 1, true, true), 3);
        assert_eq!(compute(0, 0, true, true), DOT);
        assert_eq!(compute(0, 0, true, false), 0, "unread badge off hides only the dot");
        assert_eq!(compute(4, 0, false, false), 4);
        assert_eq!(compute(0, 0, false, true), 0);
    }
}
