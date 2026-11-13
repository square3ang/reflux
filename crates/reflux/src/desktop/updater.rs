//! Update check (`Updater.ts`, `AppImageUpdate.ts`): at start and every six hours, the release
//! feed's newest version against ours. A newer one shows the update bar; "Update now" downloads
//! this platform's package, checks its SHA-256 against the feed's (Fluxer refuses an update
//! without a published checksum), and a restart puts it in place:
//! - Linux AppImage: swapped over the running file at once; the restart runs it.
//! - Windows (installed by our NSIS setup): the restart runs the new setup silently, which waits
//!   for this process to exit, installs over it and starts the app again.
//! - macOS (`Reflux.app`): the restart, once this process has exited, copies the app out of the
//!   new disk image over the bundle and opens it.
//!
//! Anything else (a plain binary, Flatpak) gets the release page.
//!
//! The feed: `REFLUX_UPDATE_FEED` (at build time, as the release workflow sets it, or run time),
//! a GitHub "latest release" API URL (`https://api.github.com/repos/<owner>/<repo>/releases/latest`):
//! `tag_name` is the version, `html_url` the page, and the package asset (preferring one naming
//! this architecture) with its `digest` (`sha256:<hex>`, which GitHub publishes for every asset).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::State;

const EVERY: Duration = Duration::from_secs(6 * 3600);

pub(crate) fn feed() -> Option<String> {
    std::env::var("REFLUX_UPDATE_FEED").ok().or(option_env!("REFLUX_UPDATE_FEED").map(String::from)).filter(|u| u.starts_with("https://"))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Available,
    Downloading,
    /// Downloaded (and, for an AppImage, swapped in); a restart finishes it.
    Installed,
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Available {
    pub version: String,
    /// The release page.
    pub page: String,
    /// This platform's package and its SHA-256 (hex), when this run can update itself and the
    /// feed publishes a checksum for it.
    pub package: Option<(String, String)>,
    pub stage: Stage,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// `sha256:<hex>`.
    #[serde(default)]
    digest: Option<String>,
}

/// How this run is installed, when it can replace itself.
enum Install {
    /// The AppImage file (`$APPIMAGE`).
    AppImage(PathBuf),
    /// A Windows install made by our setup (its `Uninstall.exe` beside the executable).
    Setup,
    /// The macOS `.app` bundle the executable is in (read by the Unix relaunch).
    #[cfg_attr(windows, allow(dead_code))]
    Bundle(PathBuf),
}

impl Install {
    fn current() -> Option<Install> {
        if cfg!(target_os = "linux") {
            return std::env::var_os("APPIMAGE").map(|p| Install::AppImage(p.into()));
        }
        let exe = std::env::current_exe().ok()?;
        if cfg!(windows) {
            return exe.parent()?.join("Uninstall.exe").is_file().then_some(Install::Setup);
        }
        if cfg!(target_os = "macos") {
            let bundle = exe.ancestors().nth(3)?;
            return bundle.extension().is_some_and(|e| e == "app").then(|| Install::Bundle(bundle.to_path_buf()));
        }
        None
    }

    /// The release asset's name ends with this.
    fn suffix(&self) -> &'static str {
        match self {
            Install::AppImage(_) => ".AppImage",
            Install::Setup => "-setup.exe",
            Install::Bundle(_) => ".dmg",
        }
    }
}

/// The package for `suffix`, preferring one naming this architecture.
fn pick<'a>(assets: &'a [Asset], suffix: &str) -> Option<&'a Asset> {
    let of = |a: &&Asset| a.name.ends_with(suffix);
    assets.iter().filter(of).find(|a| a.name.contains(std::env::consts::ARCH)).or_else(|| assets.iter().find(of))
}

/// The hex SHA-256 in a GitHub asset digest.
fn sha256_of(digest: Option<&str>) -> Option<String> {
    let hex = digest?.strip_prefix("sha256:")?.to_ascii_lowercase();
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
}

/// `1.2.10` > `1.2.9`; a leading `v` and anything after `-` / `+` are ignored.
pub fn newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.trim_start_matches('v').split(['-', '+']).next().unwrap_or("").split('.').map(|n| n.parse().unwrap_or(0)).collect() };
    parse(candidate) > parse(current)
}

pub fn tick(s: &mut State) {
    if rune::is_headless() || s.app.shell.prefs.no_auto_update || s.desktop.update_checked.is_some_and(|t| t.elapsed() < EVERY) {
        return;
    }
    s.desktop.update_checked = Some(Instant::now());
    let Some(url) = feed() else { return };
    crate::app::spawn(async move {
        match check(&url).await {
            Ok(Some(a)) => crate::app::update(move |s| {
                if s.desktop.update.as_ref().is_none_or(|u| u.version != a.version) {
                    s.desktop.update = Some(a);
                }
            }),
            Ok(None) => {}
            Err(e) => eprintln!("reflux: update check: {e}"),
        }
    });
}

pub(crate) async fn check(url: &str) -> reqwest::Result<Option<Available>> {
    let r: Release = reflux_net::http_client().get(url).send().await?.error_for_status()?.json().await?;
    if !newer(&r.tag_name, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let package = Install::current().and_then(|i| pick(&r.assets, i.suffix())).and_then(|a| Some((a.browser_download_url.clone(), sha256_of(a.digest.as_deref())?)));
    Ok(Some(Available { version: r.tag_name.trim_start_matches('v').to_string(), page: r.html_url, package, stage: Stage::Available }))
}

fn set_stage(stage: Stage) {
    crate::app::update(move |s| {
        if let Some(u) = &mut s.desktop.update {
            u.stage = stage;
        }
    });
}

/// The downloaded setup or disk image the restart installs (Windows, macOS).
static PENDING: Mutex<Option<PathBuf>> = Mutex::new(None);

/// "Update now": download and check the package, then put it where the restart finds it (an
/// AppImage goes straight over the running file, atomic on one filesystem).
pub fn install(s: &mut State) {
    let Some(u) = s.desktop.update.as_mut().filter(|u| u.stage != Stage::Downloading) else { return };
    let (Some((url, sha256)), Some(how)) = (u.package.clone(), Install::current()) else { return };
    u.stage = Stage::Downloading;
    crate::app::spawn(async move {
        let (target, magic): (PathBuf, &[u8]) = match &how {
            Install::AppImage(p) => (p.clone(), b"\x7fELF"),
            Install::Setup => (std::env::temp_dir().join("reflux-setup.exe"), b"MZ"),
            Install::Bundle(_) => (std::env::temp_dir().join("reflux-update.dmg"), b""),
        };
        let tmp = target.with_extension("download");
        let r = async {
            let bytes = reflux_net::http_client().get(&url).send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?.bytes().await.map_err(|e| e.to_string())?;
            // The package, not an error page.
            if !bytes.starts_with(magic) {
                return Err("not the update package".to_string());
            }
            let got: String = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(&bytes)).iter().map(|b| format!("{b:02x}")).collect();
            if got != sha256 {
                return Err(format!("Update checksum mismatch: expected {sha256}, downloaded {got}"));
            }
            tokio::fs::write(&tmp, &bytes).await.map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).await.map_err(|e| e.to_string())?;
            }
            tokio::fs::rename(&tmp, &target).await.map_err(|e| e.to_string())
        }
        .await;
        if let Err(e) = &r {
            eprintln!("reflux: update download: {e}");
            let _ = tokio::fs::remove_file(&tmp).await;
        } else if !matches!(how, Install::AppImage(_)) {
            *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(target);
        }
        set_stage(if r.is_ok() { Stage::Installed } else { Stage::Failed });
    });
}

/// Starts the app again (installing a downloaded update first) once this process has quit and
/// freed the single-instance lock, then quits.
pub fn restart() {
    let pending = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Err(e) = relaunch(pending) {
        eprintln!("reflux: restart: {e}");
    }
    crate::app::quit();
}

#[cfg(unix)]
fn relaunch(dmg: Option<PathBuf>) -> std::io::Result<()> {
    // $0 our pid, $1 what to start, $2 the disk image to copy `Reflux.app` out of over $1.
    const SCRIPT: &str = r#"while kill -0 "$0" 2>/dev/null; do sleep 0.2; done
if [ -n "$2" ]; then
  m=$(mktemp -d)
  if hdiutil attach -nobrowse -readonly -mountpoint "$m" "$2" >/dev/null; then
    rm -rf "$1.old"
    if mv "$1" "$1.old"; then
      if ditto "$m/Reflux.app" "$1"; then rm -rf "$1.old"; else rm -rf "$1"; mv "$1.old" "$1"; fi
    fi
    hdiutil detach "$m" >/dev/null
  fi
  rm -f "$2"
fi
case "$1" in *.app) exec open "$1";; *) exec "$1";; esac"#;
    let what = match Install::current() {
        Some(Install::AppImage(p) | Install::Bundle(p)) => p,
        _ => std::env::current_exe()?,
    };
    let dmg = dmg.map(|d| d.into_os_string()).unwrap_or_default();
    std::process::Command::new("sh").arg("-c").arg(SCRIPT).arg(std::process::id().to_string()).arg(what).arg(dmg).spawn().map(drop)
}

#[cfg(windows)]
fn relaunch(setup: Option<PathBuf>) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    match setup {
        // The setup waits for our executable to be free, and starts the app when silent.
        Some(s) => std::process::Command::new(s).arg("/S").spawn().map(drop),
        None => {
            let exe = std::env::current_exe()?;
            std::process::Command::new("cmd").raw_arg(format!("/C ping -n 2 127.0.0.1 >nul & start \"\" \"{}\"", exe.display())).creation_flags(NO_WINDOW).spawn().map(drop)
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn relaunch(_: Option<PathBuf>) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions_compare_numerically() {
        assert!(super::newer("v0.2.0", "0.1.0"));
        assert!(super::newer("1.2.10", "1.2.9"));
        assert!(!super::newer("v0.1.0", "0.1.0"));
        assert!(!super::newer("0.1.0-beta", "0.1.0"));
        assert!(super::newer("1.0", "0.9.9"));
    }

    #[test]
    fn asset_digests() {
        let hex = "AB".repeat(32);
        assert_eq!(super::sha256_of(Some(&format!("sha256:{hex}"))), Some(hex.to_ascii_lowercase()));
        assert_eq!(super::sha256_of(Some("sha512:00")), None);
        assert_eq!(super::sha256_of(Some("sha256:xyz")), None);
        assert_eq!(super::sha256_of(None), None, "no checksum, no self-update");
    }

    #[test]
    fn picks_the_platform_package() {
        let a = |n: &str| super::Asset { name: n.into(), browser_download_url: String::new(), digest: None };
        let assets = [a("Reflux-0.2.0-aarch64.dmg"), a("Reflux-0.2.0-x86_64-setup.exe"), a("Reflux-0.2.0-aarch64.AppImage"), a("Reflux-0.2.0-x86_64.AppImage")];
        let arch = std::env::consts::ARCH;
        assert_eq!(super::pick(&assets, ".AppImage").unwrap().name, format!("Reflux-0.2.0-{arch}.AppImage"));
        assert_eq!(super::pick(&assets, "-setup.exe").unwrap().name, "Reflux-0.2.0-x86_64-setup.exe");
        assert!(super::pick(&assets, ".msi").is_none());
    }
}
