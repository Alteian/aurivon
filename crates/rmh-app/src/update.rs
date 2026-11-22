//! Self-update from GitHub releases, versioned with semver.
//!
//! Releases are cut by `scripts/release.sh`; CI (`.github/workflows/release.yml`)
//! attaches `aurivon-<version>-<arch>-linux.tar.gz` and a matching `.sha256`.
//! Public releases work without auth. An optional GitHub token (for rate limits)
//! is taken from `AURIVON_GITHUB_TOKEN` / `RMH_GITHUB_TOKEN` / `GH_TOKEN` /
//! `GITHUB_TOKEN`, `~/.config/aurivon/github_token`, or `gh auth token`.
//! Tokens are never logged or stored by the app.
//!
//! Installed layout (see `scripts/install.sh`):
//! ```text
//! ~/.local/share/aurivon/app/versions/<version>/aurivon
//! ~/.local/share/aurivon/app/current -> versions/<version>
//! ~/.local/bin/aurivon            (launcher wrapper)
//! ```
//! An update unpacks the new version next to the old one, checks it runs, and
//! swaps the `current` symlink atomically, so a failed update never breaks the app.

use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;
use sha2::{Digest, Sha256};

use crate::service::{MediaService, ServiceResult};

/// GitHub repository the releases are published to. This is the only place to
/// change when the repository is renamed (scripts/release.sh reads it from here).
pub const REPO: &str = "Alteian/aurivon";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A release newer than the running version
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub version: Version,
    pub tag: String,
    pub name: String,
    /// Release notes (markdown, from CHANGELOG.md)
    pub notes: String,
    pub html_url: String,
    pub published_at: String,
    pub prerelease: bool,
    asset_name: String,
    asset_url: String,
    sha_url: Option<String>,
}

#[derive(Debug, Clone)]
pub enum UpdateCheck {
    /// Nothing newer; carries the latest published version if any
    UpToDate { latest: Option<String> },
    Available(ReleaseInfo),
}

pub fn current_version() -> Version {
    Version::parse(CURRENT_VERSION).expect("crate version is semver")
}

/// `~/.local/share/aurivon/app` (the library database lives one level up)
pub fn install_root() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("aurivon").join("app"))
}

/// The install root when this process runs from an installed copy
/// (`<root>/versions/<v>/aurivon`), `None` for dev builds
pub fn installed_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let root = install_root()?.canonicalize().ok()?;
    let versions = exe.parent()?.parent()?;
    (versions.file_name()? == "versions" && versions.parent()? == root).then_some(root)
}

/// The launcher wrapper (`~/.local/bin/aurivon`) or the installed binary
pub fn launcher_path() -> Option<PathBuf> {
    let wrapper = dirs::home_dir()?.join(".local/bin/aurivon");
    if wrapper.exists() {
        return Some(wrapper);
    }
    installed_root().map(|r| r.join("current").join("aurivon"))
}

fn arch() -> &'static str {
    std::env::consts::ARCH
}

pub fn asset_name(version: &Version) -> String {
    format!("aurivon-{version}-{}-linux.tar.gz", arch())
}

/// Optional GitHub token (rate limits / private forks); never logged
fn github_token() -> Option<String> {
    if let Some(t) = crate::config::env_alias("AURIVON_GITHUB_TOKEN", "RMH_GITHUB_TOKEN") {
        return Some(t);
    }
    for var in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(t) = std::env::var(var) {
            if !t.trim().is_empty() {
                return Some(t.trim().to_string());
            }
        }
    }
    if let Some(path) = dirs::config_dir().map(|d| d.join("aurivon").join("github_token")) {
        if let Ok(t) = std::fs::read_to_string(path) {
            if !t.trim().is_empty() {
                return Some(t.trim().to_string());
            }
        }
    }
    // `gh` may not be on the launcher's PATH
    let mut candidates = vec![PathBuf::from("gh"), PathBuf::from("/usr/bin/gh"), PathBuf::from("/usr/local/bin/gh")];
    if let Some(h) = dirs::home_dir() {
        candidates.push(h.join(".local/bin/gh"));
    }
    for gh in candidates {
        if let Ok(out) = Command::new(&gh).args(["auth", "token"]).output() {
            let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.status.success() && !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("Aurivon/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(300))
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()?)
}

fn authed(req: reqwest::RequestBuilder, token: &Option<String>) -> reqwest::RequestBuilder {
    let req = req.header("X-GitHub-Api-Version", "2022-11-28");
    match token {
        Some(t) => req.bearer_auth(t),
        None => req,
    }
}

#[derive(serde::Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(serde::Deserialize)]
struct GhAsset {
    name: String,
    /// API asset URL (needs `Accept: application/octet-stream`; may count against rate limit)
    url: String,
    /// Direct CDN link for public releases — preferred for downloads (no API auth)
    #[serde(default)]
    browser_download_url: Option<String>,
}

fn asset_download_url(a: &GhAsset) -> String {
    a.browser_download_url
        .as_deref()
        .filter(|u| !u.is_empty())
        .unwrap_or(&a.url)
        .to_string()
}

/// Pick the newest usable release (pure, for tests)
fn pick_latest(releases: Vec<GhRelease>, include_pre: bool) -> Option<ReleaseInfo> {
    releases
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let version = Version::parse(r.tag_name.trim_start_matches('v')).ok()?;
            let pre = r.prerelease || !version.pre.is_empty();
            if pre && !include_pre {
                return None;
            }
            let asset_name = asset_name(&version);
            let asset = r.assets.iter().find(|a| a.name == asset_name)?;
            let sha_url = r
                .assets
                .iter()
                .find(|a| a.name == format!("{asset_name}.sha256"))
                .map(asset_download_url);
            Some(ReleaseInfo {
                name: r.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| r.tag_name.clone()),
                notes: r.body.clone().unwrap_or_default(),
                html_url: r.html_url.clone(),
                published_at: r.published_at.clone().unwrap_or_default(),
                prerelease: pre,
                asset_url: asset_download_url(asset),
                asset_name,
                sha_url,
                tag: r.tag_name,
                version,
            })
        })
        .max_by(|a, b| a.version.cmp(&b.version))
}

/// Notes of every release newer than `current`, newest first, so skipping
/// versions doesn't hide what changed in between
fn combined_notes(releases: &[GhRelease], current: &Version, include_pre: bool) -> String {
    let mut newer: Vec<(Version, &str)> = releases
        .iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let v = Version::parse(r.tag_name.trim_start_matches('v')).ok()?;
            let pre = r.prerelease || !v.pre.is_empty();
            (v > *current && (include_pre || !pre)).then(|| (v, r.body.as_deref().unwrap_or("")))
        })
        .collect();
    newer.sort_by(|a, b| b.0.cmp(&a.0));
    if newer.len() == 1 {
        return newer[0].1.trim().to_string();
    }
    newer.iter().map(|(v, body)| format!("## {v}\n{}", body.trim())).collect::<Vec<_>>().join("\n\n")
}

/// Ask GitHub for the newest release and compare it with the running version
pub async fn check(include_pre: bool) -> anyhow::Result<UpdateCheck> {
    let token = github_token();
    let resp = authed(client()?.get(format!("https://api.github.com/repos/{REPO}/releases?per_page=30")), &token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    match resp.status().as_u16() {
        200 => {}
        401 | 403 | 404 if token.is_none() => anyhow::bail!(
            "Could not list GitHub releases (HTTP {}). If rate-limited, sign in with `gh auth login` or put a token in ~/.config/aurivon/github_token",
            resp.status().as_u16()
        ),
        401 => anyhow::bail!("GitHub rejected the token (401): run `gh auth login` again"),
        s => anyhow::bail!("GitHub releases request failed: HTTP {s}"),
    }
    let releases: Vec<GhRelease> = resp.json().await?;
    let current = current_version();
    let notes = combined_notes(&releases, &current, include_pre);
    let latest = pick_latest(releases, include_pre);
    Ok(match latest {
        Some(mut r) if r.version > current => {
            r.notes = notes;
            UpdateCheck::Available(r)
        }
        Some(r) => UpdateCheck::UpToDate { latest: Some(r.version.to_string()) },
        None => UpdateCheck::UpToDate { latest: None },
    })
}

async fn download(url: &str, token: &Option<String>) -> anyhow::Result<Vec<u8>> {
    // Public `browser_download_url` hits GitHub's CDN — no token, no API quota.
    // API asset URLs need Accept: application/octet-stream; reqwest drops
    // Authorization when the redirect leaves api.github.com.
    let cdn = url.contains("://github.com/") && url.contains("/releases/download/");
    let req = client()?.get(url).header("Accept", "application/octet-stream");
    let resp = if cdn { req } else { authed(req, token) }.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("download failed: HTTP {}", resp.status());
    }
    Ok(resp.bytes().await?.to_vec())
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Download, verify (sha256) and install `rel`, then switch `current` to it.
/// Returns the path to start the new version with.
pub async fn install(rel: &ReleaseInfo) -> anyhow::Result<PathBuf> {
    let root = installed_root().ok_or_else(|| {
        anyhow::anyhow!("This copy is not installed (it runs from a build folder). Install it with `make install` first.")
    })?;
    let token = github_token();
    let sha_url = rel.sha_url.as_deref().ok_or_else(|| anyhow::anyhow!("release {} has no checksum file", rel.tag))?;
    let sha_file = String::from_utf8(download(sha_url, &token).await?)?;
    let expected = sha_file.split_whitespace().next().unwrap_or_default().to_lowercase();
    if expected.len() != 64 {
        anyhow::bail!("checksum file for {} is malformed", rel.tag);
    }
    let data = download(&rel.asset_url, &token).await?;
    let actual = sha256_hex(&data);
    if actual != expected {
        anyhow::bail!("checksum mismatch for {} (expected {expected}, got {actual}); update aborted", rel.asset_name);
    }
    tracing::info!("Downloaded {} ({} bytes), sha256 verified", rel.asset_name, data.len());
    let version = rel.version.to_string();
    let root2 = root.clone();
    tokio::task::spawn_blocking(move || install_archive(&root2, &version, &data)).await??;
    Ok(root.join("current").join("aurivon"))
}

/// Unpack a verified tarball as `<root>/versions/<version>` and activate it
pub fn install_archive(root: &Path, version: &str, tarball: &[u8]) -> anyhow::Result<()> {
    let versions = root.join("versions");
    std::fs::create_dir_all(&versions)?;
    let staging = versions.join(format!(".staging-{version}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let result = (|| -> anyhow::Result<()> {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
        // `unpack` refuses entries that would escape the target directory
        archive.unpack(&staging)?;
        let dir = staging.join(format!("aurivon-{version}"));
        let exe = dir.join("aurivon");
        if !exe.is_file() {
            anyhow::bail!("archive does not contain aurivon-{version}/aurivon");
        }
        // the new binary must run and report the expected version
        let out = Command::new(&exe).arg("--version").output()?;
        let reported = String::from_utf8_lossy(&out.stdout);
        if !out.status.success() || !reported.contains(version) {
            anyhow::bail!("new binary failed its self-check (reported {:?})", reported.trim());
        }
        let target = versions.join(version);
        let running = std::env::current_exe().ok().and_then(|e| e.canonicalize().ok());
        if target.exists() {
            if running.as_deref().is_some_and(|r| r.starts_with(&target)) {
                anyhow::bail!("version {version} is the one running");
            }
            std::fs::remove_dir_all(&target)?;
        }
        std::fs::rename(&dir, &target)?;
        activate(root, version)?;
        // refresh launcher entry / icons shipped with the new version (best effort)
        let script = target.join("install.sh");
        if script.is_file() {
            let _ = Command::new("sh").arg(&script).arg("--integrate").output();
        }
        prune(root, version, running.as_deref());
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Atomically point `<root>/current` at `versions/<version>`
pub fn activate(root: &Path, version: &str) -> anyhow::Result<()> {
    let tmp = root.join(format!(".current-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    std::os::unix::fs::symlink(Path::new("versions").join(version), &tmp)?;
    std::fs::rename(&tmp, root.join("current"))?;
    Ok(())
}

/// Keep the new version and the one running (for rollback); remove older ones
fn prune(root: &Path, keep: &str, running: Option<&Path>) {
    let Ok(dir) = std::fs::read_dir(root.join("versions")) else { return };
    for e in dir.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name == keep || name.starts_with('.') || running.is_some_and(|r| r.starts_with(&path)) {
            continue;
        }
        let _ = std::fs::remove_dir_all(&path);
    }
}

/// Start the (new) installed version detached; the caller should then quit
pub fn spawn_restart() -> anyhow::Result<()> {
    let exe = launcher_path().ok_or_else(|| anyhow::anyhow!("no installed launcher found"))?;
    Command::new(exe).spawn()?;
    Ok(())
}

/// Blocking helpers for the command line (`--check-update`, `--update`)
pub mod cli {
    use super::*;

    fn rt() -> anyhow::Result<tokio::runtime::Runtime> {
        Ok(tokio::runtime::Builder::new_current_thread().enable_all().build()?)
    }

    pub fn check(include_pre: bool) -> anyhow::Result<UpdateCheck> {
        rt()?.block_on(super::check(include_pre))
    }

    pub fn install(rel: &ReleaseInfo) -> anyhow::Result<PathBuf> {
        rt()?.block_on(super::install(rel))
    }
}

impl MediaService {
    pub fn check_update(&self) -> impl std::future::Future<Output = ServiceResult<UpdateCheck>> + 'static {
        let pre = self.config().update_prereleases;
        self.run(async move { check(pre).await.map_err(|e| format!("{e:#}")) })
    }

    pub fn install_update(&self, rel: ReleaseInfo) -> impl std::future::Future<Output = ServiceResult<PathBuf>> + 'static {
        self.run(async move { install(&rel).await.map_err(|e| format!("{e:#}")) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str, pre: bool, with_asset: bool) -> GhRelease {
        let v = Version::parse(tag.trim_start_matches('v')).unwrap();
        let assets = if with_asset {
            vec![
                GhAsset {
                    name: asset_name(&v),
                    url: format!("u/{tag}"),
                    browser_download_url: Some(format!("https://github.com/x/y/releases/download/{tag}/a.tar.gz")),
                },
                GhAsset {
                    name: format!("{}.sha256", asset_name(&v)),
                    url: format!("s/{tag}"),
                    browser_download_url: None,
                },
            ]
        } else {
            vec![]
        };
        GhRelease {
            tag_name: tag.into(),
            name: None,
            body: Some("notes".into()),
            html_url: String::new(),
            published_at: None,
            draft: false,
            prerelease: pre,
            assets,
        }
    }

    #[test]
    fn picks_highest_semver_and_skips_prereleases() {
        let list = || vec![rel("v0.2.0", false, true), rel("v0.10.0", false, true), rel("v0.9.5", false, true), rel("v0.11.0-beta.1", true, true), rel("v0.12.0", false, false)];
        let r = pick_latest(list(), false).unwrap();
        assert_eq!(r.version.to_string(), "0.10.0"); // semver, not string order; no asset -> skipped
        assert_eq!(r.sha_url.as_deref(), Some("s/v0.10.0")); // no CDN url → API url
        assert!(r.asset_url.contains("/releases/download/")); // prefers browser_download_url
        let r = pick_latest(list(), true).unwrap();
        assert_eq!(r.version.to_string(), "0.11.0-beta.1");
    }

    #[test]
    fn install_archive_swaps_current() {
        let root = tempfile::tempdir().unwrap();
        // a fake release whose "binary" is a shell script printing its version
        let mut buf = vec![];
        {
            let enc = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
            let mut tar = tar::Builder::new(enc);
            let script = b"#!/bin/sh\necho aurivon 9.9.9\n";
            let mut h = tar::Header::new_gnu();
            h.set_size(script.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            tar.append_data(&mut h, "aurivon-9.9.9/aurivon", &script[..]).unwrap();
            tar.into_inner().unwrap().finish().unwrap();
        }
        install_archive(root.path(), "9.9.9", &buf).unwrap();
        let link = std::fs::read_link(root.path().join("current")).unwrap();
        assert_eq!(link, Path::new("versions/9.9.9"));
        assert!(root.path().join("current/aurivon").is_file());
        // a binary reporting the wrong version is rejected and current is untouched
        assert!(install_archive(root.path(), "9.9.10", &buf).is_err());
        assert_eq!(std::fs::read_link(root.path().join("current")).unwrap(), Path::new("versions/9.9.9"));
    }

    #[test]
    fn notes_cover_every_skipped_release() {
        let list = vec![rel("v0.2.0", false, true), rel("v0.2.1", false, true), rel("v0.3.0", false, true), rel("v0.4.0-beta.1", true, true)];
        let n = combined_notes(&list, &Version::parse("0.2.0").unwrap(), false);
        assert!(n.starts_with("## 0.3.0") && n.contains("## 0.2.1") && !n.contains("0.4.0") && !n.contains("## 0.2.0"));
    }

    #[test]
    fn sha256_matches_known_value() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
