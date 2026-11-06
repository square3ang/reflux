//! Streaming privacy (`streamer_mode/state/StreamerMode.ts`): masks names and private details,
//! invite links, sounds and notifications while on, which it is when turned on by hand or when
//! OBS / XSplit runs (`StreamerModeProcessDetection.ts`, polled every 15 s).

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::State;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub manual: bool,
    pub auto: bool,
    pub hide_personal: bool,
    pub hide_invites: bool,
    pub disable_sounds: bool,
    pub disable_notifications: bool,
    pub nagbar_dismissed: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { manual: false, auto: true, hide_personal: true, hide_invites: true, disable_sounds: true, disable_notifications: true, nagbar_dismissed: false }
    }
}

pub fn enabled(p: &Prefs, detected: bool) -> bool {
    p.manual || (p.auto && detected)
}

pub fn mute_sounds(p: &Prefs, detected: bool) -> bool {
    enabled(p, detected) && p.disable_sounds
}

pub fn pause_notifications(p: &Prefs, detected: bool) -> bool {
    enabled(p, detected) && p.disable_notifications
}

const POLL: Duration = Duration::from_secs(15);

pub fn tick(s: &mut State, tx: &mut rune::Tx) {
    let p = &s.app.saved.desktop.streamer;
    // `formatNameForStreamerMode` (`truncateStreamerModeName`) through `Store::display_name`.
    s.app.store.mask_names = enabled(p, s.desktop.capture_app) && p.hide_personal;
    if !s.app.saved.desktop.streamer.auto || rune::is_headless() {
        s.desktop.capture_app = false;
        return;
    }
    if s.desktop.streamer_polled.is_some_and(|t| t.elapsed() < POLL) {
        return;
    }
    s.desktop.streamer_polled = Some(Instant::now());
    tx.wake_after(POLL);
    crate::app::spawn(async {
        let found = tokio::task::spawn_blocking(capture_app_running).await.unwrap_or(false);
        crate::app::update(move |s| s.desktop.capture_app = found);
    });
}

const POSIX_NAMES: &[&str] = &["obs", "obs-studio", "xsplit"];
#[cfg_attr(not(windows), allow(dead_code))]
const WINDOWS_NAMES: &[&str] = &["obs.exe", "obs32.exe", "obs64.exe", "obs-studio.exe", "xsplit.exe", "xsplit.core.exe", "xsplit.broadcaster.exe"];

/// A capture app's process is running.
#[cfg(target_os = "linux")]
fn capture_app_running() -> bool {
    let Ok(dir) = std::fs::read_dir("/proc") else { return false };
    dir.flatten().any(|e| std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| POSIX_NAMES.contains(&c.trim())))
}

#[cfg(target_os = "macos")]
fn capture_app_running() -> bool {
    let Ok(out) = std::process::Command::new("ps").args(["-axco", "comm="]).output() else { return false };
    String::from_utf8_lossy(&out.stdout).lines().any(|l| POSIX_NAMES.contains(&l.trim().to_lowercase().as_str()))
}

#[cfg(windows)]
fn capture_app_running() -> bool {
    use std::os::windows::process::CommandExt;
    let Ok(out) = std::process::Command::new("tasklist").args(["/fo", "csv", "/nh"]).creation_flags(0x0800_0000).output() else { return false };
    String::from_utf8_lossy(&out.stdout).lines().any(|l| WINDOWS_NAMES.contains(&l.split(',').next().unwrap_or("").trim_matches('"').to_lowercase().as_str()))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn capture_app_running() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_when_manual_or_detected_with_auto() {
        let p = Prefs::default();
        assert!(!enabled(&p, false));
        assert!(enabled(&p, true));
        assert!(!enabled(&Prefs { auto: false, ..p.clone() }, true));
        assert!(mute_sounds(&Prefs { manual: true, ..p }, false));
        let _ = capture_app_running();
    }
}
