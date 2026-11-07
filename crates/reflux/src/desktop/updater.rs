//! Update check (`Updater.ts`, `AppImageUpdate.ts`): at start and every six hours, the release
//! feed's newest version against ours. A newer one shows the update bar; from an AppImage it
//! downloads next to the running file, checks its SHA-256 against the feed's (Fluxer refuses an
//! AppImage update without a published checksum), and swaps it in, then offers a restart.
//! Elsewhere (Flatpak, Windows, macOS, a plain binary) the bar opens the release page: Fluxer
//! updates those through Flathub or Velopack, which Reflux doesn't ship with.
//!
//! The feed: `REFLUX_UPDATE_FEED` (at build time or run time), a GitHub "latest release" API URL
//! (`https://api.github.com/repos/<owner>/<repo>/releases/latest`): `tag_name` is the version,
//! `html_url` the page, and the `.AppImage` asset (preferring one naming this architecture)
//! with its `digest` (`sha256:<hex>`, which GitHub publishes for every asset).
// shortcut: Reflux has no release feed yet, so without `REFLUX_UPDATE_FEED` the updater stays
// off; set it once releases are published.

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
    /// Installed over the AppImage; a restart runs it.
    Installed,
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Available {
    pub version: String,
    /// The release page.
    pub page: String,
    /// The AppImage asset and its SHA-256 (hex), when this run is an AppImage and the feed
    /// publishes a checksum for it.
    pub appimage: Option<(String, String)>,
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
    let arch = std::env::consts::ARCH;
    let appimage = std::env::var_os("APPIMAGE")
        .and_then(|_| r.assets.iter().find(|a| a.name.ends_with(".AppImage") && a.name.contains(arch)).or_else(|| r.assets.iter().find(|a| a.name.ends_with(".AppImage"))))
        .and_then(|a| Some((a.browser_download_url.clone(), sha256_of(a.digest.as_deref())?)));
    Ok(Some(Available { version: r.tag_name.trim_start_matches('v').to_string(), page: r.html_url, appimage, stage: Stage::Available }))
}

fn set_stage(stage: Stage) {
    crate::app::update(move |s| {
        if let Some(u) = &mut s.desktop.update {
            u.stage = stage;
        }
    });
}

/// "Update now" in an AppImage: download beside it and rename over it (atomic on one filesystem).
pub fn install(s: &mut State) {
    let Some(u) = s.desktop.update.as_mut().filter(|u| u.stage != Stage::Downloading) else { return };
    let (Some((url, sha256)), Some(target)) = (u.appimage.clone(), std::env::var_os("APPIMAGE")) else { return };
    u.stage = Stage::Downloading;
    crate::app::spawn(async move {
        let target = std::path::PathBuf::from(target);
        let tmp = target.with_extension("download");
        let r = async {
            let bytes = reflux_net::http_client().get(&url).send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?.bytes().await.map_err(|e| e.to_string())?;
            // An ELF executable, not an error page.
            if !bytes.starts_with(b"\x7fELF") {
                return Err("not an AppImage".to_string());
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
        }
        set_stage(if r.is_ok() { Stage::Installed } else { Stage::Failed });
    });
}

/// Relaunches the (updated) AppImage once this process has quit and freed the single-instance
/// socket, then quits.
pub fn restart() {
    let exe = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from).or_else(|| std::env::current_exe().ok());
    #[cfg(unix)]
    if let Some(exe) = exe {
        let r = std::process::Command::new("sh").arg("-c").arg("sleep 1; exec \"$0\"").arg(exe).spawn();
        if let Err(e) = r {
            return eprintln!("reflux: restart: {e}");
        }
    }
    #[cfg(not(unix))]
    let _ = exe;
    crate::app::quit();
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
}
