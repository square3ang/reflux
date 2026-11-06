//! What survives a restart: the chosen instance, the session token and the installation ID, as
//! JSON in the platform config dir (`$REFLUX_CONFIG_DIR` overrides it).

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Official instance, as `OFFICIAL_INSTANCE_DISPLAY_HOST` in `packages/instance_bootstrap`.
pub const DEFAULT_INSTANCE: &str = "fluxer.com";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    /// What the user typed in the instance picker (normalized host or URL).
    pub instance: String,
    // shortcut: plain file with 0600, move the token to the OS keyring (keyring crate) later.
    pub token: Option<String>,
    /// 32 lowercase hex chars, like `ClientInstallationId.ts`.
    pub installation_id: String,
    /// Chosen microphone / speaker names (`None` = system default).
    pub voice_input: Option<String>,
    pub voice_output: Option<String>,
    /// Interface language picked before sign-in (`None` = the system's); signed in, the
    /// account's `locale` setting wins.
    pub locale: Option<String>,
    pub desktop: crate::desktop::Prefs,
    /// The token was imported from the Fluxer desktop app: logging out only forgets it here,
    /// since revoking it would sign that app out too.
    pub imported: bool,
}

fn path() -> Option<PathBuf> {
    let dir = std::env::var_os("REFLUX_CONFIG_DIR").map(PathBuf::from).or_else(|| Some(dirs::config_dir()?.join("reflux")))?;
    Some(dir.join("session.json"))
}

/// Reads the saved session, filling in the default instance and a new installation ID.
pub fn load() -> Saved {
    let mut s: Saved = path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if s.instance.is_empty() {
        s.instance = DEFAULT_INSTANCE.into();
    }
    if s.installation_id.len() != 32 || !s.installation_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        s.installation_id = format!("{:016x}{:016x}", fastrand::u64(..), fastrand::u64(..));
        save(&s);
    }
    s
}

/// Writes atomically (temp file + rename), owner-only on Unix. Failures are logged, not fatal.
pub fn save(s: &Saved) {
    let Some(path) = path() else { return };
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension("json.tmp");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        let mut f = opts.open(&tmp)?;
        f.write_all(&serde_json::to_vec_pretty(s)?)?;
        f.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if let Err(e) = result {
        eprintln!("reflux: saving {}: {e}", path.display());
    }
}

/// The official desktop app's store (`<Electron userData>/desktop-app-store.sqlite3`).
// shortcut: userData guessed as `<config dir>/fluxer` (Linux ~/.config, Windows %APPDATA%, macOS
// ~/Library/Application Support); verified on Linux only, check on Windows/macOS when tested there.
pub fn fluxer_desktop_db() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("fluxer").join("desktop-app-store.sqlite3"))
}

/// The session of the official desktop app's active account (else its most recently used one),
/// opened read-only. Keeps this install's other saved fields; marks it `imported`.
pub fn import_fluxer_desktop(db: &std::path::Path) -> Result<Saved, String> {
    use rusqlite::{Connection, OpenFlags, OptionalExtension};
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("Couldn't open {}: {e}", db.display()))?;
    let global = |key: &str| -> rusqlite::Result<Option<String>> {
        conn.query_row("SELECT value FROM scoped_storage WHERE store = 'app' AND scope = 'global' AND entry_key = ?1", [key], |r| r.get(0))
            .optional()
    };
    let err = |e: rusqlite::Error| format!("Couldn't read the Fluxer desktop app's data: {e}");
    let active = global("fluxer:auth:active-account-key").map_err(err)?.unwrap_or_default();
    let record: Option<String> = conn
        .query_row(
            "SELECT record_json FROM accounts ORDER BY storage_key = ?1 DESC, last_active DESC LIMIT 1",
            [active.trim_matches('"')],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    let record: serde_json::Value =
        serde_json::from_str(&record.ok_or("The Fluxer desktop app isn't signed in.")?).map_err(|e| e.to_string())?;
    let token = record["token"].as_str().filter(|t| !t.is_empty()).ok_or("The Fluxer desktop app's session has no token.")?;
    let i = &record["instance"];
    let web = [&i["webAppEndpoint"], &i["apiEndpoint"]].into_iter().find_map(|v| v.as_str()).ok_or("The session has no instance.")?;
    let host = url::Url::parse(web).ok().and_then(|u| Some(format!("{}{}", u.host_str()?, u.port().map(|p| format!(":{p}")).unwrap_or_default())));
    let mut saved = load();
    saved.instance = host.ok_or("The session's instance URL is invalid.")?;
    saved.token = Some(token.to_owned());
    saved.imported = true;
    // The same installation as the desktop app, so the server sees no new device.
    if let Some(id) = global("ClientInstallationId").map_err(err)?.map(|v| v.trim_matches('"').to_owned())
        && id.len() == 32
        && id.bytes().all(|b| b.is_ascii_hexdigit())
    {
        saved.installation_id = id.to_ascii_lowercase();
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both tests share `REFLUX_CONFIG_DIR`.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn imports_the_desktop_apps_active_account() {
        let _env = ENV.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("reflux-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: serialized by ENV.
        unsafe { std::env::set_var("REFLUX_CONFIG_DIR", &dir) };
        let db = dir.join("store.sqlite3");
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch(
            r#"CREATE TABLE accounts (storage_key TEXT PRIMARY KEY NOT NULL, user_id TEXT NOT NULL, last_active REAL NOT NULL, record_json TEXT NOT NULL);
            CREATE TABLE scoped_storage (store TEXT NOT NULL, scope TEXT NOT NULL, entry_key TEXT NOT NULL, value TEXT NOT NULL, updated_at REAL NOT NULL, PRIMARY KEY(store, scope, entry_key));
            INSERT INTO accounts VALUES ('https://a.example/api::1', '1', 1, '{"token":"tok-a","instance":{"apiEndpoint":"https://a.example/api","webAppEndpoint":"https://a.example"}}');
            INSERT INTO accounts VALUES ('https://b.example:8443/api::2', '2', 5, '{"token":"tok-b","instance":{"apiEndpoint":"https://b.example:8443/api"}}');
            INSERT INTO scoped_storage VALUES ('app', 'global', 'ClientInstallationId', '0123456789ABCDEF0123456789abcdef', 0);"#,
        )
        .unwrap();
        let s = import_fluxer_desktop(&db).unwrap();
        assert_eq!((s.instance.as_str(), s.token.as_deref()), ("b.example:8443", Some("tok-b")), "most recent without an active key");
        assert_eq!(s.installation_id, "0123456789abcdef0123456789abcdef");
        c.execute("INSERT INTO scoped_storage VALUES ('app', 'global', 'fluxer:auth:active-account-key', 'https://a.example/api::1', 0)", []).unwrap();
        let s = import_fluxer_desktop(&db).unwrap();
        assert_eq!((s.instance.as_str(), s.token.as_deref()), ("a.example", Some("tok-a")), "the active account wins");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn round_trip_and_permissions() {
        let _env = ENV.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("reflux-session-{}", std::process::id()));
        // SAFETY: serialized by ENV.
        unsafe { std::env::set_var("REFLUX_CONFIG_DIR", &dir) };
        let first = load();
        assert_eq!(first.instance, DEFAULT_INSTANCE);
        assert_eq!(first.installation_id.len(), 32);
        save(&Saved { token: Some("t".into()), ..first.clone() });
        let again = load();
        assert_eq!((again.token.as_deref(), &again.installation_id), (Some("t"), &first.installation_id));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("session.json")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
