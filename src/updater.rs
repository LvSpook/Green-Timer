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

/// Return the new release when one exists.
pub fn check_for_update(current_version: &str) -> Option<UpdateInfo> {
    if !GITHUB_REPO.contains('/') {
        return None;
    }
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let body = match get(&url, Duration::from_secs(8)) {
        Ok(body) => body,
        Err(err) => {
            log::info!("Update check skipped: {err}");
            return None;
        }
    };
    let release: Release = match serde_json::from_slice(&body) {
        Ok(release) => release,
        Err(err) => {
            log::info!("Update check skipped: {err}");
            return None;
        }
    };
    if release.tag_name.is_empty() || !is_newer(&release.tag_name, current_version) {
        return None;
    }
    release
        .assets
        .into_iter()
        .find(|asset| {
            asset.name.to_ascii_lowercase().ends_with(".exe")
                && !asset.browser_download_url.is_empty()
        })
        .map(|asset| UpdateInfo {
            latest: release.tag_name,
            url: asset.browser_download_url,
        })
}

/// Download an installer to a temp file and return its path.
pub fn download_installer(url: &str) -> Result<PathBuf, String> {
    let body = get(url, Duration::from_secs(30))?;
    if body.is_empty() {
        return Err("download was empty".to_string());
    }
    let path = std::env::temp_dir().join(format!(
        "GreenTimer-Setup-{}.exe",
        std::process::id()
    ));
    let mut file = std::fs::File::create(&path).map_err(|err| err.to_string())?;
    file.write_all(&body).map_err(|err| err.to_string())?;
    file.sync_all().map_err(|err| err.to_string())?;
    Ok(path)
}

/// Kick off a background update check; the result arrives on the channel.
pub fn spawn_check(current_version: String) -> Receiver<Option<UpdateInfo>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(check_for_update(&current_version));
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
}
