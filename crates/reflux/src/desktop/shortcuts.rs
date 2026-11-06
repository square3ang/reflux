//! System-wide shortcuts (`GlobalShortcutsEngine.ts`): mute, deafen and push-to-talk while the
//! window isn't focused. Linux binds them through the xdg-desktop-portal GlobalShortcuts portal
//! (the user picks the keys in the desktop's settings, as with `LinuxGlobalShortcutsPortal.ts`);
//! Windows registers Ctrl+Shift+M / Ctrl+Shift+D with `RegisterHotKey`.
//!
//! For voice: [`ptt_held`] is the push-to-talk key's state, and every press and release also
//! wakes the UI thread, so a frame's tick sees it change.
// shortcut: without the portal (X11 sessions, older compositors) Linux has no global shortcuts;
// an evdev / XGrabKey fallback isn't written. macOS has none yet (needs a CGEventTap and the
// Accessibility permission). Windows has no push-to-talk: RegisterHotKey reports no release,
// and there's no key recorder to pick one for a low-level hook.

use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleMute,
    ToggleDeafen,
    PushToTalk,
}

/// `GLOBAL_SHORTCUT_ACTIONS` ids and `GLOBAL_SHORTCUT_DEFAULT_DESCRIPTIONS`.
pub const ACTIONS: &[(Action, &str, &str)] = &[
    (Action::ToggleMute, "voice_toggle_mute", "Toggle mute"),
    (Action::ToggleDeafen, "voice_toggle_deafen", "Toggle deafen"),
    (Action::PushToTalk, "voice_push_to_talk", "Push-to-talk"),
];

static ENABLED: AtomicBool = AtomicBool::new(true);
static STARTED: AtomicBool = AtomicBool::new(false);
static PTT: AtomicBool = AtomicBool::new(false);

/// The push-to-talk shortcut is held down.
pub fn ptt_held() -> bool {
    PTT.load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if !on {
        PTT.store(false, Ordering::Relaxed);
    }
}

/// A shortcut went down (`pressed`) or up; runs on any thread.
pub fn fire(action: Action, pressed: bool) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    match action {
        Action::PushToTalk => {
            PTT.store(pressed, Ordering::Relaxed);
            crate::app::update(|_| {});
        }
        Action::ToggleMute if pressed => crate::app::update(|s| s.app.shell.toggle_mute()),
        Action::ToggleDeafen if pressed => crate::app::update(|s| s.app.shell.toggle_deaf()),
        _ => {}
    }
}

/// Starts listening once, in a windowed run.
pub fn start(enabled: bool) {
    set_enabled(enabled);
    if rune::is_headless() || STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    platform::start();
}

#[cfg(target_os = "linux")]
mod platform {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    use super::{ACTIONS, fire};

    pub fn start() {
        crate::app::spawn(async {
            if let Err(e) = portal().await {
                eprintln!("reflux: global shortcuts portal unavailable: {e}");
            }
        });
    }

    async fn portal() -> ashpd::Result<()> {
        let gs = GlobalShortcuts::new().await?;
        let session = gs.create_session(Default::default()).await?;
        let shortcuts: Vec<NewShortcut> = ACTIONS.iter().map(|&(_, id, desc)| NewShortcut::new(id, desc)).collect();
        gs.bind_shortcuts(&session, &shortcuts, None, Default::default()).await?.response()?;
        let down = gs.receive_activated().await?.map(|a| (a.shortcut_id().to_string(), true));
        let up = gs.receive_deactivated().await?.map(|a| (a.shortcut_id().to_string(), false));
        let mut events = futures_util::stream::select(down, up);
        while let Some((id, pressed)) = events.next().await {
            if let Some(&(action, ..)) = ACTIONS.iter().find(|(_, i, _)| *i == id) {
                fire(action, pressed);
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

    use super::{Action, fire};

    const KEYS: [(Action, u32); 2] = [(Action::ToggleMute, b'M' as u32), (Action::ToggleDeafen, b'D' as u32)];

    pub fn start() {
        std::thread::spawn(|| {
            // Thread hotkeys (no window): WM_HOTKEY arrives in this thread's queue.
            for (i, (_, vk)) in KEYS.iter().enumerate() {
                // SAFETY: plain Win32 calls with a null window handle.
                if unsafe { RegisterHotKey(std::ptr::null_mut(), i as i32 + 1, MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT, *vk) } == 0 {
                    eprintln!("reflux: global shortcut {i} is taken by another app");
                }
            }
            // SAFETY: MSG is plain data; GetMessageW fills it.
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
                if msg.message == WM_HOTKEY
                    && let Some((action, _)) = KEYS.get((msg.wParam as usize).wrapping_sub(1))
                {
                    fire(*action, true);
                }
            }
        });
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    pub fn start() {}
}

#[cfg(test)]
mod tests {
    #[test]
    fn ptt_follows_press_and_release_and_disable() {
        super::fire(super::Action::PushToTalk, true);
        assert!(super::ptt_held());
        super::set_enabled(false);
        assert!(!super::ptt_held());
        super::fire(super::Action::PushToTalk, true);
        assert!(!super::ptt_held(), "ignored while off");
        super::set_enabled(true);
    }
}
