use std::{
    fs::{self, File},
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::models::now_utc;
use crate::paths;

const MANIFEST_ASSET_NAME: &str = "latch-release-manifest.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKind {
    Chrome,
    Raycast,
}

impl ClientKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Raycast => "raycast",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Chrome => "Chrome",
            Self::Raycast => "Raycast",
        }
    }

    fn expected_asset_name(self) -> &'static str {
        match self {
            Self::Chrome => "latch-chrome-mv3.zip",
            Self::Raycast => "latch-raycast.zip",
        }
    }

    fn marker_file(self) -> &'static str {
        match self {
            Self::Chrome => "manifest.json",
            Self::Raycast => "package.json",
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub repo: String,
    pub version: Option<String>,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct InstalledClient {
    pub kind: ClientKind,
    pub version: String,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseManifest {
    version: Option<String>,
    assets: Vec<ManifestAsset>,
}

#[derive(Debug, Deserialize)]
struct ManifestAsset {
    kind: Option<String>,
    name: String,
    sha256: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ClientsState {
    chrome: Option<ClientStateEntry>,
    raycast: Option<ClientStateEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ClientStateEntry {
    version: String,
    path: PathBuf,
    installed_at: DateTime<Utc>,
    source_repo: String,
    asset: String,
}

pub async fn install(kind: ClientKind, options: InstallOptions) -> Result<InstalledClient> {
    paths::ensure_runtime_dirs()?;

    let release = fetch_release(&options.repo, options.version.as_deref()).await?;
    let manifest_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == MANIFEST_ASSET_NAME)
        .ok_or_else(|| {
            anyhow!(
                "Release {} does not include {}",
                release.tag_name,
                MANIFEST_ASSET_NAME
            )
        })?;
    let manifest_bytes = download_bytes(&manifest_asset.browser_download_url).await?;
    let manifest = parse_manifest(&manifest_bytes)?;
    cache_release_manifest(&options.repo, &release.tag_name, &manifest_bytes)?;

    let manifest_entry = find_manifest_asset(kind, &manifest).ok_or_else(|| {
        anyhow!(
            "Release manifest does not include a {} client asset",
            kind.display_name()
        )
    })?;
    let github_asset = release
        .assets
        .iter()
        .find(|asset| asset.name == manifest_entry.name)
        .ok_or_else(|| {
            anyhow!(
                "Release {} does not include asset {}",
                release.tag_name,
                manifest_entry.name
            )
        })?;

    let version = manifest
        .version
        .as_deref()
        .unwrap_or(release.tag_name.as_str())
        .trim_start_matches('v')
        .to_owned();
    let archive_path = download_asset(
        &release.tag_name,
        github_asset,
        &manifest_entry.sha256,
        options.force,
    )
    .await?;
    let client_root = extract_client_archive(kind, &version, &archive_path, options.force)?;
    update_current_link(kind, &client_root)?;
    update_clients_state(
        kind,
        &version,
        &client_root,
        &options.repo,
        &manifest_entry.name,
    )?;

    Ok(InstalledClient {
        kind,
        version,
        path: installed_path(kind).unwrap_or(client_root),
    })
}

pub fn installed_path(kind: ClientKind) -> Option<PathBuf> {
    let current = paths::client_current_link(kind.id());
    if !path_exists(&current) {
        return None;
    }

    let direct_marker = current.join(kind.marker_file());
    if path_exists(&direct_marker) {
        return Some(current);
    }

    let entries = fs::read_dir(&current).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path_exists(&path.join(kind.marker_file())) {
            return Some(path);
        }
    }

    Some(current)
}

pub fn uninstall(kind: ClientKind) -> Result<()> {
    let client_dir = paths::client_dir(kind.id());
    if path_exists(&client_dir) {
        fs::remove_dir_all(&client_dir)
            .with_context(|| format!("Failed to remove {}", client_dir.display()))?;
    }

    let mut state = read_clients_state()?;
    set_state_entry(&mut state, kind, None);
    write_clients_state(&state)?;

    Ok(())
}

pub fn open(kind: ClientKind) -> Result<()> {
    let path = installed_path(kind).ok_or_else(|| {
        anyhow!(
            "{} client is not installed. Run `latch {} install` first.",
            kind.display_name(),
            kind.id()
        )
    })?;
    open_path(&path)
}

async fn fetch_release(repo: &str, version: Option<&str>) -> Result<GitHubRelease> {
    let url = match version {
        Some(version) => format!("https://api.github.com/repos/{repo}/releases/tags/{version}"),
        None => format!("https://api.github.com/repos/{repo}/releases/latest"),
    };

    http_client()
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json::<GitHubRelease>()
        .await
        .context("Failed to parse GitHub release response")
}

async fn download_asset(
    release_tag: &str,
    asset: &GitHubAsset,
    expected_sha256: &str,
    force: bool,
) -> Result<PathBuf> {
    let target_dir = paths::downloads_dir().join(release_tag);
    fs::create_dir_all(&target_dir)
        .with_context(|| format!("Failed to create {}", target_dir.display()))?;
    let target_path = target_dir.join(&asset.name);

    if path_exists(&target_path) && !force {
        let bytes = fs::read(&target_path)
            .with_context(|| format!("Failed to read {}", target_path.display()))?;
        verify_sha256(&bytes, expected_sha256)
            .with_context(|| format!("Cached asset {} failed checksum", target_path.display()))?;
        return Ok(target_path);
    }

    let bytes = download_bytes(&asset.browser_download_url).await?;
    verify_sha256(&bytes, expected_sha256)
        .with_context(|| format!("Downloaded asset {} failed checksum", asset.name))?;
    fs::write(&target_path, bytes)
        .with_context(|| format!("Failed to write {}", target_path.display()))?;

    Ok(target_path)
}

async fn download_bytes(url: &str) -> Result<Vec<u8>> {
    let response = http_client().get(url).send().await?.error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

fn http_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static("latch-cli"),
    );
    headers.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
    );

    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if let Ok(value) =
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token.trim()))
        {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
    }

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("valid reqwest client")
}

fn parse_manifest(bytes: &[u8]) -> Result<ReleaseManifest> {
    serde_json::from_slice(bytes).context("Failed to parse release manifest")
}

fn cache_release_manifest(repo: &str, release_tag: &str, bytes: &[u8]) -> Result<()> {
    let repo_name = repo.replace('/', "-");
    let path = paths::release_cache_dir().join(format!("{repo_name}-{release_tag}.json"));
    fs::write(&path, bytes).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

fn find_manifest_asset<'a>(
    kind: ClientKind,
    manifest: &'a ReleaseManifest,
) -> Option<&'a ManifestAsset> {
    manifest
        .assets
        .iter()
        .find(|asset| asset.kind.as_deref() == Some(kind.id()))
        .or_else(|| {
            manifest
                .assets
                .iter()
                .find(|asset| asset.name == kind.expected_asset_name())
        })
}

fn verify_sha256(bytes: &[u8], expected_sha256: &str) -> Result<()> {
    let digest = Sha256::digest(bytes);
    let actual = format!("{digest:x}");
    let expected = expected_sha256.trim().to_lowercase();
    if actual != expected {
        bail!("sha256 mismatch: expected {expected}, got {actual}");
    }

    Ok(())
}

fn extract_client_archive(
    kind: ClientKind,
    version: &str,
    archive_path: &Path,
    force: bool,
) -> Result<PathBuf> {
    let version_dir = paths::client_versions_dir(kind.id()).join(version);
    if path_exists(&version_dir) {
        if !force {
            return Ok(version_dir);
        }
        fs::remove_dir_all(&version_dir)
            .with_context(|| format!("Failed to remove {}", version_dir.display()))?;
    }

    fs::create_dir_all(&version_dir)
        .with_context(|| format!("Failed to create {}", version_dir.display()))?;
    let bytes = fs::read(archive_path)
        .with_context(|| format!("Failed to read {}", archive_path.display()))?;
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).context("Failed to read zip archive")?;

    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let Some(enclosed_name) = file.enclosed_name().map(PathBuf::from) else {
            continue;
        };
        let outpath = version_dir.join(enclosed_name);

        if file.is_dir() {
            fs::create_dir_all(&outpath)
                .with_context(|| format!("Failed to create {}", outpath.display()))?;
            continue;
        }

        if let Some(parent) = outpath.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }

        let mut outfile = File::create(&outpath)
            .with_context(|| format!("Failed to create {}", outpath.display()))?;
        std::io::copy(&mut file, &mut outfile)
            .with_context(|| format!("Failed to extract {}", outpath.display()))?;
    }

    warn_if_marker_missing(kind, &version_dir);
    Ok(version_dir)
}

fn update_current_link(kind: ClientKind, version_dir: &Path) -> Result<()> {
    let link = paths::client_current_link(kind.id());
    if path_exists(&link) {
        if link.is_dir() && fs::read_link(&link).is_err() {
            fs::remove_dir_all(&link)
                .with_context(|| format!("Failed to remove {}", link.display()))?;
        } else {
            fs::remove_file(&link)
                .with_context(|| format!("Failed to remove {}", link.display()))?;
        }
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(version_dir, &link).with_context(|| {
            format!(
                "Failed to create symlink {} -> {}",
                link.display(),
                version_dir.display()
            )
        })?;
    }

    #[cfg(not(unix))]
    {
        fs::create_dir_all(&link)
            .with_context(|| format!("Failed to create {}", link.display()))?;
    }

    Ok(())
}

fn update_clients_state(
    kind: ClientKind,
    version: &str,
    path: &Path,
    source_repo: &str,
    asset: &str,
) -> Result<()> {
    let mut state = read_clients_state()?;
    set_state_entry(
        &mut state,
        kind,
        Some(ClientStateEntry {
            version: version.to_owned(),
            path: path.to_path_buf(),
            installed_at: now_utc(),
            source_repo: source_repo.to_owned(),
            asset: asset.to_owned(),
        }),
    );
    write_clients_state(&state)
}

fn read_clients_state() -> Result<ClientsState> {
    let path = paths::clients_state_file();
    if !path_exists(&path) {
        return Ok(ClientsState::default());
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))
}

fn write_clients_state(state: &ClientsState) -> Result<()> {
    paths::ensure_runtime_dirs()?;
    let raw = serde_json::to_string_pretty(state)?;
    let path = paths::clients_state_file();
    fs::write(&path, raw).with_context(|| format!("Failed to write {}", path.display()))
}

fn set_state_entry(state: &mut ClientsState, kind: ClientKind, entry: Option<ClientStateEntry>) {
    match kind {
        ClientKind::Chrome => state.chrome = entry,
        ClientKind::Raycast => state.raycast = entry,
    }
}

fn warn_if_marker_missing(kind: ClientKind, version_dir: &Path) {
    if installed_marker_exists(kind, version_dir) {
        return;
    }

    eprintln!(
        "Warning: {} package does not contain {} at the root or one level down. Check the release package structure.",
        kind.display_name(),
        kind.marker_file()
    );
}

fn installed_marker_exists(kind: ClientKind, version_dir: &Path) -> bool {
    if path_exists(&version_dir.join(kind.marker_file())) {
        return true;
    }

    fs::read_dir(version_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let path = entry.path();
            path.is_dir() && path_exists(&path.join(kind.marker_file()))
        })
}

fn open_path(path: &Path) -> Result<()> {
    if cfg!(target_os = "macos") {
        Command::new("open")
            .arg(path)
            .status()
            .context("Failed to run open")?;
        return Ok(());
    }

    println!("{}", path.display());
    Ok(())
}

fn path_exists(path: &Path) -> bool {
    path.try_exists().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{ClientKind, ReleaseManifest, find_manifest_asset};

    #[test]
    fn finds_manifest_asset_by_kind_or_name() {
        let manifest = ReleaseManifest {
            version: Some("0.1.0".to_owned()),
            assets: vec![
                super::ManifestAsset {
                    kind: None,
                    name: "latch-chrome-mv3.zip".to_owned(),
                    sha256: "abc".to_owned(),
                },
                super::ManifestAsset {
                    kind: Some("raycast".to_owned()),
                    name: "raycast.zip".to_owned(),
                    sha256: "def".to_owned(),
                },
            ],
        };

        assert_eq!(
            find_manifest_asset(ClientKind::Chrome, &manifest)
                .unwrap()
                .name,
            "latch-chrome-mv3.zip"
        );
        assert_eq!(
            find_manifest_asset(ClientKind::Raycast, &manifest)
                .unwrap()
                .name,
            "raycast.zip"
        );
    }
}
