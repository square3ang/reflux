//! System-wide idle time (`powerMonitor.getSystemIdleTime`, used by `ui/state/Idle.ts`): on Linux
//! the Wayland `ext-idle-notify-v1` protocol (KDE, wlroots, …), else GNOME's Mutter IdleMonitor or
//! org.freedesktop.ScreenSaver over D-Bus; `GetLastInputInfo` on Windows,
//! `CGEventSourceSecondsSinceLastEventType` on macOS. A thread asks every 30 s
//! (`IDLE_CHECK_INTERVAL_MS`) and [`last_activity`] folds the answer in.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// `IDLE_CHECK_INTERVAL_MS` (a quarter of the 10-minute idle time, at most 30 s).
pub const CHECK: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
struct Seen {
    activity: Option<Instant>,
    nonzero: Option<Instant>,
    /// Wayland says there's input right now.
    active: bool,
}

static SEEN: Mutex<Seen> = Mutex::new(Seen { activity: None, nonzero: None, active: false });

/// Starts the polling thread (once).
pub fn start() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new().name("reflux-idle".into()).spawn(|| {
            #[cfg(target_os = "linux")]
            if wayland::run().is_ok() {
                return;
            }
            loop {
                if let Some(idle) = query() {
                    record(Instant::now(), idle, crate::app::idle::IDLE);
                }
                std::thread::sleep(CHECK);
            }
        });
    });
}

/// `updateIdleStateFromSystem`: a non-zero idle time puts the last activity `idle` ago; a zero one
/// counts only if a non-zero one was seen within `window` (some backends always answer 0).
fn record(now: Instant, idle: Duration, window: Duration) {
    let mut s = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if !idle.is_zero() {
        s.nonzero = Some(now);
        let at = now.checked_sub(idle).unwrap_or(now);
        s.activity = Some(s.activity.map_or(at, |a| a.max(at)));
    } else if s.nonzero.is_some_and(|n| now - n < window) {
        s.activity = Some(now);
    }
}

/// The latest input the OS reported, if it can tell.
pub fn last_activity() -> Option<Instant> {
    let s = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if s.active { Some(Instant::now()) } else { s.activity }
}

/// `ext-idle-notify-v1` reports "no input for [`AFTER`]" and "input again" instead of an idle
/// time: active means activity now; idled means the last activity was [`AFTER`] before.
#[cfg(target_os = "linux")]
mod wayland {
    use std::time::{Duration, Instant};

    use wayland_client::globals::{GlobalListContents, registry_queue_init};
    use wayland_client::protocol::{wl_registry, wl_seat};
    use wayland_client::{Connection, Dispatch, QueueHandle};
    use wayland_protocols::ext::idle_notify::v1::client::{ext_idle_notification_v1 as note, ext_idle_notifier_v1 as notifier};

    const AFTER: Duration = Duration::from_secs(10);

    struct S;

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for S {
        fn event(_: &mut S, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<S>) {}
    }
    impl Dispatch<wl_seat::WlSeat, ()> for S {
        fn event(_: &mut S, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<S>) {}
    }
    impl Dispatch<notifier::ExtIdleNotifierV1, ()> for S {
        fn event(_: &mut S, _: &notifier::ExtIdleNotifierV1, _: notifier::Event, _: &(), _: &Connection, _: &QueueHandle<S>) {}
    }
    impl Dispatch<note::ExtIdleNotificationV1, ()> for S {
        fn event(_: &mut S, _: &note::ExtIdleNotificationV1, e: note::Event, _: &(), _: &Connection, _: &QueueHandle<S>) {
            let mut seen = super::SEEN.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            match e {
                note::Event::Idled => (seen.active, seen.activity) = (false, Some(now.checked_sub(AFTER).unwrap_or(now))),
                note::Event::Resumed => (seen.active, seen.activity) = (true, Some(now)),
                _ => {}
            }
        }
    }

    /// Serves notifications until the connection drops; `Err` when the protocol isn't there.
    pub fn run() -> Result<(), ()> {
        let conn = Connection::connect_to_env().map_err(drop)?;
        let (globals, mut queue) = registry_queue_init::<S>(&conn).map_err(drop)?;
        let qh = queue.handle();
        let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ()).map_err(drop)?;
        let n: notifier::ExtIdleNotifierV1 = globals.bind(&qh, 1..=1, ()).map_err(drop)?;
        n.get_idle_notification(AFTER.as_millis() as u32, &seat, &qh, ());
        super::SEEN.lock().unwrap_or_else(|e| e.into_inner()).active = true;
        while queue.blocking_dispatch(&mut S).is_ok() {}
        super::SEEN.lock().unwrap_or_else(|e| e.into_inner()).active = false;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn query() -> Option<Duration> {
    let bus = zbus::blocking::Connection::session().ok()?;
    let mutter = bus
        .call_method(Some("org.gnome.Mutter.IdleMonitor"), "/org/gnome/Mutter/IdleMonitor/Core", Some("org.gnome.Mutter.IdleMonitor"), "GetIdletime", &())
        .ok()
        .and_then(|r| r.body().deserialize::<u64>().ok())
        .map(Duration::from_millis);
    mutter.or_else(|| {
        let r = bus.call_method(Some("org.freedesktop.ScreenSaver"), "/org/freedesktop/ScreenSaver", Some("org.freedesktop.ScreenSaver"), "GetSessionIdleTime", &()).ok()?;
        r.body().deserialize::<u32>().ok().map(|s| Duration::from_secs(s.into()))
    })
}

#[cfg(windows)]
fn query() -> Option<Duration> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: `info` is a valid, sized LASTINPUTINFO.
    (unsafe { GetLastInputInfo(&mut info) } != 0).then(|| Duration::from_millis(unsafe { GetTickCount() }.wrapping_sub(info.dwTime).into()))
}

#[cfg(target_os = "macos")]
fn query() -> Option<Duration> {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, kind: u32) -> f64;
    }
    // kCGEventSourceStateCombinedSessionState, kCGAnyInputEventType.
    let s = unsafe { CGEventSourceSecondsSinceLastEventType(0, u32::MAX) };
    (s.is_finite() && s >= 0.0).then(|| Duration::from_secs_f64(s))
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn query() -> Option<Duration> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_counts_only_after_a_recent_nonzero() {
        let w = Duration::from_secs(600);
        let t0 = Instant::now();
        record(t0, Duration::ZERO, w);
        assert!(last_activity().is_none(), "a backend that always says 0 tells nothing");
        record(t0, Duration::from_secs(5), w);
        assert_eq!(last_activity(), Some(t0 - Duration::from_secs(5)));
        let t1 = t0 + Duration::from_secs(30);
        record(t1, Duration::ZERO, w);
        assert_eq!(last_activity(), Some(t1));
    }
}
