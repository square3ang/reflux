//! Launch at login (`Autostart.ts`): an XDG autostart entry on Linux, the HKCU `Run` key on
//! Windows, a LaunchAgent on macOS. The launch carries [`ARG`], so "Start minimized" applies to
//! launches at login only. Inside Flatpak the entry is the host's, so it goes through the XDG
//! Background portal (`setFlatpakAutostart`) and the answer is remembered in a state file.

use std::io;
use std::path::PathBuf;

/// `AUTOSTART_LAUNCH_ARG`.
pub const ARG: &str = "--autostart";

/// This process was started at login.
pub fn launched_at_login() -> bool {
    std::env::args().any(|a| a == ARG)
}

/// Start hidden in the tray: a launch at login with "Start minimized" and the tray icon on.
pub fn start_hidden(p: &super::Prefs) -> bool {
    launched_at_login() && p.start_minimized && p.tray
}

/// What to launch: the AppImage when running from one (its mount path changes every run).
fn exe() -> io::Result<PathBuf> {
    match std::env::var_os("APPIMAGE") {
        Some(a) => Ok(a.into()),
        None => std::env::current_exe(),
    }
}

#[cfg(target_os = "linux")]
fn entry_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("autostart").join(format!("{}.desktop", super::notify::DESKTOP_ID)))
}

/// `quoteDesktopExecArg` then `escapeDesktopEntry`: the Desktop Entry spec's quoting for `Exec`.
#[cfg(any(target_os = "linux", test))]
fn desktop_file(exe: &str) -> String {
    let mut quoted = String::from('"');
    for c in exe.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    let esc = |s: &str| s.replace('\\', "\\\\");
    [
        "[Desktop Entry]",
        "Type=Application",
        "Name=Fluxer",
        "Comment=Fluxer",
        &format!("Exec={} {ARG}", esc(&quoted)),
        &format!("TryExec={}", esc(exe)),
        "Terminal=false",
        "Categories=Network;InstantMessaging;",
        "X-GNOME-Autostart-enabled=true",
        "StartupNotify=false",
        "",
    ]
    .join("\n")
}

/// `isFlatpakRuntime`.
#[cfg(target_os = "linux")]
fn flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

/// `flatpak-autostart-v1.json`: what the portal last granted.
#[cfg(target_os = "linux")]
fn flatpak_state() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("reflux/flatpak-autostart-v1.json"))
}

/// Asks the Background portal; the request (and its permission dialog) runs on the runtime, so
/// a refusal is only logged, as the switch has already moved.
#[cfg(target_os = "linux")]
fn set_flatpak(enabled: bool) {
    crate::app::spawn(async move {
        use ashpd::desktop::background::Background;
        // `FLUXER_FLATPAK_COMMAND`: the command inside the sandbox.
        let command = std::env::var("REFLUX_FLATPAK_COMMAND").ok().filter(|c| !c.trim().is_empty()).unwrap_or_else(|| "reflux".into());
        let mut req = Background::request().auto_start(enabled);
        if enabled {
            req = req.reason(crate::i18n::tf("Start {appName} automatically when you sign in.", &[("appName", "Fluxer")]).as_str()).command([command.as_str(), ARG]);
        }
        let granted = match req.send().await.and_then(|r| r.response()) {
            Ok(r) => r.auto_start(),
            Err(e) => return eprintln!("reflux: Background portal: {e}"),
        };
        if granted != enabled {
            eprintln!("reflux: Background portal did not {} autostart", if enabled { "enable" } else { "disable" });
        }
        if let Some(p) = flatpak_state() {
            let _ = std::fs::create_dir_all(p.parent().expect("config dir"));
            let _ = std::fs::write(p, serde_json::json!({ "enabled": granted }).to_string());
        }
    });
}

#[cfg(target_os = "linux")]
pub fn set(enabled: bool) -> io::Result<()> {
    if flatpak() {
        set_flatpak(enabled);
        return Ok(());
    }
    let path = entry_path().ok_or_else(|| io::Error::other("no config dir"))?;
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(path.parent().expect("autostart dir"))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, desktop_file(&exe()?.to_string_lossy()))?;
    std::fs::rename(tmp, path)
}

#[cfg(target_os = "linux")]
pub fn is_enabled() -> bool {
    if flatpak() {
        let read = flatpak_state().and_then(|p| std::fs::read_to_string(p).ok());
        return read.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).is_some_and(|v| v["enabled"] == true);
    }
    entry_path().and_then(|p| std::fs::read_to_string(p).ok()).is_some_and(|s| !s.contains("X-GNOME-Autostart-enabled=false") && !s.contains("Hidden=true"))
}

#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
fn reg(args: &[&str]) -> io::Result<bool> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let st = std::process::Command::new("reg").args(args).creation_flags(CREATE_NO_WINDOW).output()?;
    Ok(st.status.success())
}

#[cfg(windows)]
pub fn set(enabled: bool) -> io::Result<()> {
    if enabled {
        let cmd = format!("\"{}\" {ARG}", exe()?.display());
        reg(&["add", RUN_KEY, "/v", "Fluxer", "/t", "REG_SZ", "/d", &cmd, "/f"])?.then_some(()).ok_or_else(|| io::Error::other("reg add failed"))
    } else {
        reg(&["delete", RUN_KEY, "/v", "Fluxer", "/f"]).map(|_| ())
    }
}

#[cfg(windows)]
pub fn is_enabled() -> bool {
    reg(&["query", RUN_KEY, "/v", "Fluxer"]).unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn agent_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join("Library/LaunchAgents/app.fluxer.reflux.plist"))
}

#[cfg(target_os = "macos")]
pub fn set(enabled: bool) -> io::Result<()> {
    let path = agent_path().ok_or_else(|| io::Error::other("no home dir"))?;
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let exe = exe()?.display().to_string().replace('&', "&amp;").replace('<', "&lt;");
    std::fs::create_dir_all(path.parent().expect("LaunchAgents"))?;
    std::fs::write(
        path,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>app.fluxer.reflux</string>
<key>ProgramArguments</key><array><string>{exe}</string><string>{ARG}</string></array>
<key>RunAtLoad</key><true/>
</dict></plist>
"#
        ),
    )
}

#[cfg(target_os = "macos")]
pub fn is_enabled() -> bool {
    agent_path().is_some_and(|p| p.exists())
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub fn set(_: bool) -> io::Result<()> {
    Err(io::Error::other("unsupported platform"))
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
pub fn is_enabled() -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn exec_line_is_quoted_and_escaped() {
        let f = super::desktop_file("/opt/My $App/re\"flux");
        assert!(f.contains("Exec=\"/opt/My \\\\$App/re\\\\\"flux\" --autostart\n"), "{f}");
        assert!(f.contains("TryExec=/opt/My $App/re\"flux\n"));
    }
}
