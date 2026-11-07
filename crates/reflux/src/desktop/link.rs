//! `fluxer://` links (`fluxer_desktop/src/main/DeepLinks.ts`) and single-instance forwarding: a
//! second launch hands its link and XDG activation token to the running app over a Unix socket
//! (a per-user named pipe on Windows) and exits, like Electron's `requestSingleInstanceLock` +
//! `second-instance`.

use reflux_model::Snowflake;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    Invite(String),
    /// `guild` `None` = `@me` (a DM).
    Channel { guild: Option<Snowflake>, channel: Snowflake },
    /// `fluxer://oauth2/authorize?…`: its query (`crate::nav::oauth`).
    OAuth2(Vec<(String, String)>),
    Gift(String),
    /// An account email's page (reset password, verify email, recover account, authorize IP,
    /// revert email change), with its `#token=` fragment. Fluxer's desktop app doesn't route
    /// these in-app, so they open on the instance's web app.
    Web(String),
}

/// The web app's account email pages (`Routes.RESET_PASSWORD` …).
const WEB_PAGES: &[&str] = &["reset", "verify", "recover", "authorize-ip", "wasntme"];

/// `fluxer://invite/CODE`, `fluxer://channels/@me/C`, `fluxer://channels/G/C[/M]`. Like Fluxer,
/// links that try to name an instance (a query) are refused; links resolve against the
/// signed-in account only.
pub fn parse(url: &str) -> Option<Link> {
    let rest = url.trim().strip_prefix("fluxer://").or_else(|| url.trim().strip_prefix("fluxer:"))?;
    if let Some(params) = crate::nav::oauth::parse(rest) {
        return Some(Link::OAuth2(params));
    }
    if rest.contains(['?', '"', '\'', '<', '>', '\\']) {
        return None;
    }
    let (path, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let path = path.trim_matches('/');
    if WEB_PAGES.contains(&path) {
        return Some(Link::Web(if fragment.is_empty() { format!("/{path}") } else { format!("/{path}#{fragment}") }));
    }
    let rest = path;
    let parts: Vec<&str> = rest.split('/').collect();
    let id = |s: &str| s.parse::<u64>().ok().map(Snowflake);
    match parts.as_slice() {
        ["invite", code] => crate::guild::parse_invite(code).map(Link::Invite),
        ["gift", code] if !code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') => Some(Link::Gift(code.to_string())),
        ["channels", "@me", c, ..] => Some(Link::Channel { guild: None, channel: id(c)? }),
        ["channels", g, c, ..] => Some(Link::Channel { guild: Some(id(g)?), channel: id(c)? }),
        _ => None,
    }
}

/// The `fluxer:` URL among the launch arguments (`--url -- <link>` or a bare one).
pub fn from_args() -> Option<String> {
    std::env::args().skip(1).find(|a| a.to_ascii_lowercase().starts_with("fluxer:"))
}

#[cfg(unix)]
fn socket() -> std::path::PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    // The uid keeps users apart where the fallback dir is shared.
    let uid = std::fs::metadata(&dir).map(|m| std::os::unix::fs::MetadataExt::uid(&m)).unwrap_or(0);
    dir.join(format!("reflux-{uid}.sock"))
}

/// Hands `link` (and our activation token) to a running instance; true when one took it and
/// this process should exit.
#[cfg(unix)]
pub fn forward(link: Option<&str>) -> bool {
    use std::io::Write;
    let Ok(mut s) = std::os::unix::net::UnixStream::connect(socket()) else { return false };
    let token = std::env::var("XDG_ACTIVATION_TOKEN").or_else(|_| std::env::var("DESKTOP_STARTUP_ID")).unwrap_or_default();
    writeln!(s, "{}\t{}", token.replace(['\t', '\n'], ""), link.unwrap_or_default().replace(['\t', '\n'], "")).is_ok()
}

/// The per-user pipe name (`requestSingleInstanceLock`'s, keyed on the account).
#[cfg(windows)]
fn pipe() -> String {
    let user: String = std::env::var("USERNAME").unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    format!(r"\\.\pipe\reflux-{user}")
}

/// Hands `link` to a running instance over its named pipe; true when one took it. The running
/// instance may take the foreground (`AllowSetForegroundWindow`), as Electron's lock allows.
#[cfg(windows)]
pub fn forward(link: Option<&str>) -> bool {
    use std::io::Write;
    const ERROR_PIPE_BUSY: i32 = 231;
    for _ in 0..20 {
        match std::fs::OpenOptions::new().write(true).open(pipe()) {
            Ok(mut f) => {
                // SAFETY: a plain Win32 call.
                unsafe { windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(windows_sys::Win32::UI::WindowsAndMessaging::ASFW_ANY) };
                return writeln!(f, "\t{}", link.unwrap_or_default().replace(['\t', '\n'], "")).is_ok();
            }
            // Between two pipe instances: the listener is about to make the next one.
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => return false,
        }
    }
    false
}

#[cfg(not(any(unix, windows)))]
pub fn forward(_link: Option<&str>) -> bool {
    false
}

/// One launch message: `token\tlink\n`, bounded.
fn read_launch(conn: impl std::io::Read, on_launch: &impl Fn(Option<String>, Option<String>)) {
    use std::io::{BufRead, Read};
    let mut line = String::new();
    // A bounded read: anyone who can reach the socket is the same user, but keep it sane.
    if std::io::BufReader::new(conn).take(4096).read_line(&mut line).is_err() {
        return;
    }
    let (token, link) = line.trim_end().split_once('\t').unwrap_or(("", ""));
    let some = |s: &str| (!s.is_empty()).then(|| s.to_string());
    on_launch(some(token), some(link));
}

/// Listens for later launches; `on_launch(token, link)` runs on the listener thread.
#[cfg(unix)]
pub fn listen(on_launch: impl Fn(Option<String>, Option<String>) + Send + 'static) {
    let path = socket();
    let _ = std::fs::remove_file(&path); // a stale socket from a crashed run (`forward` found no one)
    let listener = match std::os::unix::net::UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => return eprintln!("reflux: single-instance socket {}: {e}", path.display()),
    };
    // Owner-only, for the shared temp dir fallback.
    let _ = std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600));
    std::thread::Builder::new()
        .name("reflux-instance".into())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                read_launch(conn, &on_launch);
            }
        })
        .ok();
}

/// Serves the pipe: the first instance owns it (`FILE_FLAG_FIRST_PIPE_INSTANCE`), and the next
/// instance is created before a connected one is read, so a launch never finds no pipe.
#[cfg(windows)]
pub fn listen(on_launch: impl Fn(Option<String>, Option<String>) + Send + 'static) {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND};
    use windows_sys::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT};
    let name: Vec<u16> = pipe().encode_utf16().chain([0]).collect();
    // SAFETY: a NUL-terminated name; default security (only this user may write).
    let create = move |first: bool| unsafe {
        let flags = PIPE_ACCESS_INBOUND | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
        let h = CreateNamedPipeW(name.as_ptr(), flags, PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, PIPE_UNLIMITED_INSTANCES, 0, 4096, 0, std::ptr::null());
        // As an address: raw handles aren't `Send`.
        (h != INVALID_HANDLE_VALUE).then_some(h as usize)
    };
    let Some(mut h) = create(true) else {
        return eprintln!("reflux: single-instance pipe {}: {}", pipe(), std::io::Error::last_os_error());
    };
    std::thread::Builder::new()
        .name("reflux-instance".into())
        .spawn(move || {
            loop {
                // SAFETY: `h` is our pipe instance; a client that connected first is fine too.
                let ok = unsafe { ConnectNamedPipe(h as _, std::ptr::null_mut()) != 0 || GetLastError() == ERROR_PIPE_CONNECTED };
                let next = create(false);
                // SAFETY: we own `h`; the File closes it.
                let conn = unsafe { std::fs::File::from_raw_handle(h as _) };
                if ok {
                    read_launch(conn, &on_launch);
                }
                match next {
                    Some(n) => h = n,
                    None => return eprintln!("reflux: single-instance pipe: {}", std::io::Error::last_os_error()),
                }
            }
        })
        .ok();
}

#[cfg(not(any(unix, windows)))]
pub fn listen(_on_launch: impl Fn(Option<String>, Option<String>) + Send + 'static) {}

/// `app.setAsDefaultProtocolClient('fluxer')` on Windows: `HKCU\Software\Classes\fluxer` runs
/// this executable with the link (macOS takes it from the bundle's `CFBundleURLTypes`, Linux
/// from `packaging/reflux.desktop`). Packaged (release) builds only, like Fluxer.
#[cfg(windows)]
pub fn register_protocol() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    if cfg!(debug_assertions) {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let key = r"HKCU\Software\Classes\fluxer";
    let command = format!("\"{}\" \"%1\"", exe.display());
    for args in [
        vec!["add", key, "/ve", "/d", "URL:fluxer", "/f"],
        vec!["add", key, "/v", "URL Protocol", "/d", "", "/f"],
        vec!["add", &format!(r"{key}\shell\open\command"), "/ve", "/d", &command, "/f"],
    ] {
        let _ = std::process::Command::new("reg").args(args).creation_flags(CREATE_NO_WINDOW).output();
    }
}

#[cfg(not(windows))]
pub fn register_protocol() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fluxer_links() {
        assert_eq!(parse("fluxer://invite/AbCd12"), Some(Link::Invite("AbCd12".into())));
        assert_eq!(parse("fluxer://channels/@me/42"), Some(Link::Channel { guild: None, channel: Snowflake(42) }));
        assert_eq!(parse("fluxer://channels/1/2/3"), Some(Link::Channel { guild: Some(Snowflake(1)), channel: Snowflake(2) }));
        assert_eq!(parse("fluxer://gift/G1ft-x"), Some(Link::Gift("G1ft-x".into())));
        assert_eq!(parse("fluxer://reset#token=abc"), Some(Link::Web("/reset#token=abc".into())));
        assert_eq!(parse("fluxer://verify"), Some(Link::Web("/verify".into())));
        assert_eq!(parse("fluxer://recover/x"), None);
        assert_eq!(parse("fluxer://channels/x/2"), None);
        assert_eq!(parse("fluxer://invite/abc?instance=evil.example"), None, "instance designation refused");
        assert_eq!(parse("https://fluxer.gg/abc"), None);
    }
}
