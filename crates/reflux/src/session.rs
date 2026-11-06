//! What survives a restart: the chosen instance, the session token and the installation ID, as
//! JSON in the platform config dir (`$REFLUX_CONFIG_DIR` overrides it). Tokens go to the OS
//! keyring when it has one (like the official app's `DesktopAppStorage`), else stay in the 0600
//! file; a token found in the file is moved into the keyring on the next save.

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
    /// In the file only when the keyring is unavailable (see [`save`]).
    #[serde(skip_serializing_if = "Option::is_none")]
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
    /// Who the active session is (filled on READY), for the account switcher.
    pub current: Option<Account>,
    /// The other signed-in accounts (`AccountSwitcherModal`), most recent first.
    pub accounts: Vec<Account>,
}

/// A signed-in account the switcher can go back to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Account {
    pub instance: String,
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub avatar: Option<String>,
    /// In the file only when the keyring is unavailable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    pub imported: bool,
}

impl Account {
    fn slot(&self) -> String {
        format!("account:{}:{}", self.instance, self.user_id)
    }
}

impl Saved {
    /// Moves the active session (if any) to the top of [`Self::accounts`] and signs it out here.
    pub fn stash_current(&mut self) {
        let current = self.current.take();
        let Some(token) = self.token.take() else { return };
        let a = Account { instance: self.instance.clone(), token: Some(token), imported: std::mem::take(&mut self.imported), ..current.unwrap_or_default() };
        self.accounts.retain(|x| x.slot() != a.slot());
        self.accounts.insert(0, a);
    }

    /// Makes stored account `i` the active session, stashing the current one.
    pub fn switch_to(&mut self, i: usize) -> bool {
        if i >= self.accounts.len() {
            return false;
        }
        let a = self.accounts.remove(i);
        self.stash_current();
        (self.instance, self.token, self.imported) = (a.instance.clone(), a.token.clone(), a.imported);
        self.current = Some(Account { token: None, ..a });
        true
    }

    /// Forgets stored account `i` (its keyring entry too).
    pub fn remove_account(&mut self, i: usize) {
        if i < self.accounts.len() {
            let a = self.accounts.remove(i);
            secret_set(&a.slot(), None);
        }
    }
}

// ---- Keyring --------------------------------------------------------------------------------

/// Values last written to (or read from) the keyring by slot, so unchanged saves skip D-Bus.
static SECRETS: std::sync::Mutex<Option<std::collections::HashMap<String, Option<String>>>> = std::sync::Mutex::new(None);

fn keyring_on() -> bool {
    !cfg!(test) && std::env::var_os("REFLUX_NO_KEYRING").is_none()
}

/// One keyring entry per config dir and slot, so separate profiles don't share tokens.
fn entry(slot: &str) -> Option<keyring::Entry> {
    let dir = path()?.parent()?.display().to_string();
    keyring::Entry::new("reflux", &format!("{dir}#{slot}")).ok()
}

/// Stores (or with `None` deletes) a secret; false when the keyring can't be used.
fn secret_set(slot: &str, value: Option<&str>) -> bool {
    if !keyring_on() {
        return false;
    }
    let mut cache = SECRETS.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_default();
    if cache.get(slot).is_some_and(|v| v.as_deref() == value) {
        return true;
    }
    let Some(e) = entry(slot) else { return false };
    let r = match value {
        Some(v) => e.set_password(v),
        None => match e.delete_credential() {
            Err(keyring::Error::NoEntry) => Ok(()),
            r => r,
        },
    };
    match r {
        Ok(()) => {
            cache.insert(slot.to_owned(), value.map(str::to_owned));
            true
        }
        Err(err) => {
            eprintln!("reflux: keyring unavailable, keeping the token in the session file: {err}");
            false
        }
    }
}

fn secret_get(slot: &str) -> Option<String> {
    if !keyring_on() {
        return None;
    }
    let v = entry(slot)?.get_password().ok();
    SECRETS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().insert(slot.to_owned(), v.clone());
    v
}

/// reflux's config directory (`$REFLUX_CONFIG_DIR`, else `~/.config/reflux`).
pub(crate) fn dir() -> Option<PathBuf> {
    std::env::var_os("REFLUX_CONFIG_DIR").map(PathBuf::from).or_else(|| Some(dirs::config_dir()?.join("reflux")))
}

fn path() -> Option<PathBuf> {
    Some(dir()?.join("session.json"))
}

/// Reads the saved session, filling in the default instance and a new installation ID.
pub fn load() -> Saved {
    let mut s: Saved = path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    // A token still in the file wins (older versions, or no keyring then); `save` moves it.
    if s.token.is_none() {
        s.token = secret_get("session");
    }
    for a in &mut s.accounts {
        if a.token.is_none() {
            a.token = secret_get(&a.slot());
        }
    }
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
    // Tokens the keyring took stay out of the file.
    let mut s = s.clone();
    if secret_set("session", s.token.as_deref()) {
        s.token = None;
    }
    for a in &mut s.accounts {
        if a.token.is_some() && secret_set(&a.slot(), a.token.as_deref()) {
            a.token = None;
        }
    }
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension("json.tmp");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        let mut f = opts.open(&tmp)?;
        f.write_all(&serde_json::to_vec_pretty(&s)?)?;
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
    fn switching_accounts_swaps_the_active_session() {
        let me = |id: &str| Account { user_id: id.into(), username: format!("u{id}"), ..Default::default() };
        let mut s = Saved { instance: "a.example".into(), token: Some("ta".into()), current: Some(me("1")), ..Default::default() };
        s.stash_current();
        assert_eq!((s.token.as_deref(), s.current.is_none()), (None, true), "add account: signed out here");
        assert_eq!((s.accounts[0].instance.as_str(), s.accounts[0].token.as_deref()), ("a.example", Some("ta")));
        // Signed in to a second account on another instance.
        (s.instance, s.token, s.current) = ("b.example".into(), Some("tb".into()), Some(me("2")));
        assert!(s.switch_to(0));
        assert_eq!((s.instance.as_str(), s.token.as_deref(), s.current.as_ref().unwrap().user_id.as_str()), ("a.example", Some("ta"), "1"));
        assert_eq!(s.accounts.len(), 1);
        assert_eq!((s.accounts[0].instance.as_str(), s.accounts[0].token.as_deref()), ("b.example", Some("tb")));
        assert!(s.current.as_ref().unwrap().token.is_none(), "the active token lives in `token` only");
        assert!(!s.switch_to(5));
        s.remove_account(0);
        assert!(s.accounts.is_empty());
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
