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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_permissions() {
        let dir = std::env::temp_dir().join(format!("reflux-session-{}", std::process::id()));
        // SAFETY: the only test touching this variable.
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
