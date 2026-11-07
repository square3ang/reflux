//! `fluxer://` links (`fluxer_desktop/src/main/DeepLinks.ts`) and single-instance forwarding: a
//! second launch hands its link and XDG activation token to the running app over a Unix socket
//! and exits, like Electron's `requestSingleInstanceLock` + `second-instance`.

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

#[cfg(not(unix))]
// shortcut: no named-pipe forwarding on Windows yet; a second launch opens a second window.
pub fn forward(_link: Option<&str>) -> bool {
    false
}

/// Listens for later launches; `on_launch(token, link)` runs on the listener thread.
#[cfg(unix)]
pub fn listen(on_launch: impl Fn(Option<String>, Option<String>) + Send + 'static) {
    use std::io::{BufRead, Read};
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
                let mut line = String::new();
                // A bounded read: anyone who can reach the socket is the same user, but keep it sane.
                if std::io::BufReader::new(conn).take(4096).read_line(&mut line).is_err() {
                    continue;
                }
                let (token, link) = line.trim_end().split_once('\t').unwrap_or(("", ""));
                let some = |s: &str| (!s.is_empty()).then(|| s.to_string());
                on_launch(some(token), some(link));
            }
        })
        .ok();
}

#[cfg(not(unix))]
pub fn listen(_on_launch: impl Fn(Option<String>, Option<String>) + Send + 'static) {}

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
