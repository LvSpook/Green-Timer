//! Update checking and installer download.
//!
//! Only HTTPS GitHub URLs are accepted, redirects are bounded, downloads are
//! size-capped and written atomically. Networking runs on a background thread
//! so the overlay never stalls.

use serde::Deserialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

pub const GITHUB_REPO: &str = "LvSpook/Green-Timer";
const USER_AGENT: &str = "GreenTimer-Updater";
const MAX_DOWNLOAD_BYTES: u64 = 200 * 1024 * 1024;
const MAX_REDIRECTS: u32 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    pub latest: String,
    pub url: String,
    /// File name of the installer asset, used to keep the download's extension.
    pub name: String,
    /// True when the release is a work-in-progress pre-release.
    pub prerelease: bool,
}

/// Turn `v1.2.3` / `1.2` into a comparable tuple, ignoring suffixes.
pub fn parse_version(tag: &str) -> Vec<u64> {
    tag.split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u64>().ok())
        .take(4)
        .collect()
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = parse_version;
    let mut a = parse(candidate);
    let mut b = parse(current);
    // Pad so "1.2" and "1.2.0" compare equal rather than shorter-is-smaller.
    while a.len() < b.len() {
        a.push(0);
    }
    while b.len() < a.len() {
        b.push(0);
    }
    a > b
}

#[derive(Deserialize)]
struct Release {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    browser_download_url: String,
}

fn require_https(url: &str) -> Result<(), String> {
    if url.to_ascii_lowercase().starts_with("https://") {
        Ok(())
    } else {
        Err(format!("refusing non-HTTPS URL: {url}"))
    }
}

/// Fetch a URL, following redirects manually so every hop can be validated.
fn get(url: &str, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut current = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        require_https(&current)?;
        let response = ureq::builder()
            .user_agent(USER_AGENT)
            .redirects(0)
            .timeout(timeout)
            .build()
            .get(&current)
            .call();

        let response = match response {
            Ok(response) => response,
            Err(ureq::Error::Status(status, response)) if (300..400).contains(&status) => {
                let location = response
                    .header("Location")
                    .ok_or_else(|| "redirect without Location header".to_string())?;
                current = location.to_string();
                continue;
            }
            Err(err) => return Err(err.to_string()),
        };

        let mut body = Vec::new();
        response
            .into_reader()
            .take(MAX_DOWNLOAD_BYTES)
            .read_to_end(&mut body)
            .map_err(|err| err.to_string())?;
        return Ok(body);
    }
    Err("too many redirects".to_string())
}

/// Pick the best installer from a release's assets.
///
/// Order matters: an MSI is preferred, then an Inno Setup `Setup` executable,
/// then any other `.exe` that is not the bare portable binary. The old code
/// took the *first* `.exe`, which handed the updater the standalone
/// `GreenTimer.exe` instead of the installer.
fn select_installer(assets: Vec<Asset>) -> Option<Asset> {
    let usable: Vec<Asset> = assets
        .into_iter()
        .filter(|asset| !asset.browser_download_url.is_empty())
        .collect();

    let score = |name: &str| -> u8 {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".msi") {
            3
        } else if lower.ends_with(".exe") && lower.contains("setup") {
            2
        } else if lower.ends_with(".exe") && lower != "greentimer.exe" {
            1
        } else {
            0
        }
    };

    usable
        .into_iter()
        .max_by_key(|asset| score(&asset.name))
        .filter(|asset| score(&asset.name) > 0)
}

/// Fetch the newest release that is allowed by `include_prereleases`.
///
/// `/releases/latest` deliberately excludes pre-releases, so it is only used
/// for stable checks. Otherwise the release list is scanned and the highest
/// version wins.
fn fetch_release(include_prereleases: bool) -> Result<Release, String> {
    if !include_prereleases {
        let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
        let body = get(&url, Duration::from_secs(8))?;
        return serde_json::from_slice(&body).map_err(|err| err.to_string());
    }

    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases?per_page=20");
    let body = get(&url, Duration::from_secs(8))?;
    let releases: Vec<Release> =
        serde_json::from_slice(&body).map_err(|err| err.to_string())?;

    releases
        .into_iter()
        .filter(|release| !release.draft && !release.tag_name.is_empty())
        .max_by(|a, b| {
            let (mut x, mut y) = (parse_version(&a.tag_name), parse_version(&b.tag_name));
            let len = x.len().max(y.len());
            x.resize(len, 0);
            y.resize(len, 0);
            x.cmp(&y)
        })
        .ok_or_else(|| "no releases found".to_string())
}

/// Return the new release when one exists.
///
/// `include_prereleases` opts into work-in-progress builds, which GitHub hides
/// from `/releases/latest` entirely.
pub fn check_for_update(current_version: &str, include_prereleases: bool) -> Option<UpdateInfo> {
    if !GITHUB_REPO.contains('/') {
        return None;
    }
    let release = match fetch_release(include_prereleases) {
        Ok(release) => release,
        Err(err) => {
            log::info!("Update check skipped: {err}");
            return None;
        }
    };
    if release.draft
        || release.tag_name.is_empty()
        || !is_newer(&release.tag_name, current_version)
    {
        return None;
    }
    select_installer(release.assets).map(|asset| UpdateInfo {
        latest: release.tag_name,
        url: asset.browser_download_url,
        name: asset.name,
        prerelease: release.prerelease,
    })
}

/// Download an installer to a temp file and return its path.
pub fn download_installer(info: &UpdateInfo) -> Result<PathBuf, String> {
    let body = get(&info.url, Duration::from_secs(30))?;
    if body.is_empty() {
        return Err("download was empty".to_string());
    }
    // Keep the real extension: Windows needs `.msi` to hand the file to
    // Windows Installer rather than trying to run it as a program.
    let extension = asset_extension(&info.name);
    let path = std::env::temp_dir().join(format!(
        "GreenTimer-Update-{}{extension}",
        std::process::id()
    ));
    let mut file = std::fs::File::create(&path).map_err(|err| err.to_string())?;
    file.write_all(&body).map_err(|err| err.to_string())?;
    file.sync_all().map_err(|err| err.to_string())?;
    Ok(path)
}

/// `.msi` / `.exe`, defaulting to `.exe` for anything unexpected.
fn asset_extension(name: &str) -> &'static str {
    if name.to_ascii_lowercase().ends_with(".msi") {
        ".msi"
    } else {
        ".exe"
    }
}

/// Kick off a background update check; the result arrives on the channel.
pub fn spawn_check(current_version: String, include_prereleases: bool) -> Receiver<Option<UpdateInfo>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(check_for_update(&current_version, include_prereleases));
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing_ignores_prefixes() {
        assert_eq!(parse_version("v1.2.3"), vec![1, 2, 3]);
        assert_eq!(parse_version("1.2"), vec![1, 2]);
        assert_eq!(parse_version("release-2.0.0-beta"), vec![2, 0, 0]);
        assert!(parse_version("nonsense").is_empty());
    }

    #[test]
    fn newer_comparison() {
        assert!(is_newer("v1.3.0", "v1.2.0"));
        assert!(is_newer("1.2.1", "1.2"));
        assert!(!is_newer("v1.2.0", "v1.2.0"));
        assert!(!is_newer("1.2", "1.2.0"));
        assert!(!is_newer("v1.1.9", "v1.2.0"));
    }

    #[test]
    fn non_https_is_rejected() {
        assert!(require_https("http://example.com/x").is_err());
        assert!(require_https("ftp://example.com/x").is_err());
        assert!(require_https("https://example.com/x").is_ok());
    }

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.to_string(),
            browser_download_url: format!("https://example.invalid/{name}"),
        }
    }

    #[test]
    fn prefers_msi_then_setup_exe() {
        let picked = select_installer(vec![
            asset("GreenTimer.exe"),
            asset("GreenTimer_Setup_v1.3.1.exe"),
            asset("GreenTimer_v1.3.1.msi"),
        ])
        .expect("an installer should be chosen");
        assert_eq!(picked.name, "GreenTimer_v1.3.1.msi");
    }

    #[test]
    fn falls_back_to_setup_exe_without_msi() {
        let picked = select_installer(vec![
            asset("GreenTimer.exe"),
            asset("GreenTimer_Setup_v1.3.1.exe"),
        ])
        .expect("a setup exe should be chosen");
        assert_eq!(picked.name, "GreenTimer_Setup_v1.3.1.exe");
    }

    #[test]
    fn never_returns_the_bare_portable_binary() {
        assert!(select_installer(vec![asset("GreenTimer.exe")]).is_none());
        assert!(select_installer(vec![]).is_none());
    }

    #[test]
    fn skips_assets_without_a_url() {
        let mut broken = asset("GreenTimer_v1.3.1.msi");
        broken.browser_download_url.clear();
        let picked = select_installer(vec![broken, asset("GreenTimer_Setup.exe")])
            .expect("the usable setup exe should win");
        assert_eq!(picked.name, "GreenTimer_Setup.exe");
    }

    #[test]
    fn download_keeps_msi_extension() {
        assert_eq!(asset_extension("GreenTimer_v1.3.1.msi"), ".msi");
        assert_eq!(asset_extension("GreenTimer_v1.3.1.MSI"), ".msi");
        assert_eq!(asset_extension("GreenTimer_Setup.exe"), ".exe");
        assert_eq!(asset_extension("weird"), ".exe");
    }

    #[test]
    fn prerelease_tags_compare_by_number() {
        // `-wip` suffixes must not make v1.3.1 look older than v1.3.0.
        assert!(is_newer("v1.3.1-wip", "v1.3.0"));
        assert!(!is_newer("v1.3.0-wip", "1.3.0"));
    }
}
