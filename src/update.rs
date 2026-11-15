//! Update checks: the one thing Tendril sends anywhere. It asks GitHub for the latest release
//! of devatloam/tendril, and with updates on "auto" downloads the build for this system,
//! checks it against the SHA-256 the release lists, and puts it in place of the running one.
//! The new version starts the next time Tendril does.
//!
//! Everything here blocks; app.rs runs it on a thread of its own.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;

const LATEST: &str = "https://api.github.com/repos/devatloam/tendril/releases/latest";
/// Bigger than any build should ever be; stops a bad download filling the disk.
const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub page: String,
    asset: Option<Asset>,
    sums: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// "sha256:…", which GitHub works out for every file attached to a release.
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// The release file built for this system, as the release workflow names it.
pub fn asset_name() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("tendril-linux-x86_64"),
        ("windows", "x86_64") => Some("tendril-windows-x86_64.exe"),
        ("macos", "aarch64") => Some("tendril-macos-aarch64"),
        ("macos", "x86_64") => Some("tendril-macos-x86_64"),
        _ => None,
    }
}

fn agent() -> ureq::Agent {
    // The system's own certificates, so it works wherever the system's browser does (behind
    // a company proxy, say), rather than a list built into Tendril.
    #[cfg(target_os = "linux")]
    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    #[cfg(not(target_os = "linux"))]
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent(concat!("Tendril/", env!("CARGO_PKG_VERSION"), " (update check)"))
        .build()
        .into()
}

/// The newest release, or None if there's none yet.
pub fn latest() -> Result<Option<Release>, String> {
    let response = agent().get(LATEST).header("Accept", "application/vnd.github+json").call();
    let body = match response {
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(mut r) => r.body_mut().read_to_string().map_err(|e| e.to_string())?,
    };
    parse(&body)
}

/// A release as GitHub's API describes it, reduced to what the updater needs.
fn parse(json: &str) -> Result<Option<Release>, String> {
    let release: GitHubRelease = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let wanted = asset_name();
    let asset = release.assets.iter().find(|a| Some(a.name.as_str()) == wanted).cloned();
    let sums = release.assets.iter().find(|a| a.name == "SHA256SUMS").map(|a| a.browser_download_url.clone());
    Ok(Some(Release { version: release.tag_name.trim_start_matches('v').to_string(), page: release.html_url, asset, sums }))
}

/// Whether `theirs` is a later version than `ours`: 0.10.0 is after 0.9.3. Anything after a
/// `-` (0.2.0-beta) is ignored.
pub fn newer(theirs: &str, ours: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v').split('-').next().unwrap_or("").split('.').map(|p| p.parse().unwrap_or(0)).collect()
    };
    parts(theirs) > parts(ours)
}

/// Why Tendril can't put a new version in place itself here, if it can't.
pub fn cannot_install(release: &Release) -> Option<String> {
    if cfg!(debug_assertions) {
        return Some("this is a development build; update it with git and cargo".into());
    }
    let exe = std::env::current_exe().ok()?;
    if exe.components().any(|c| c.as_os_str() == "target") {
        return Some("this one was built from source; update it with git and cargo".into());
    }
    if release.asset.is_none() {
        return Some("that release has no build for this system".into());
    }
    None
}

/// Downloads the release's build for this system, checks it, and puts it in place of the
/// running executable.
pub fn install(release: &Release) -> Result<(), String> {
    if let Some(why) = cannot_install(release) {
        return Err(why);
    }
    let asset = release.asset.as_ref().ok_or("no build for this system")?;
    let expected = match asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
        Some(hex) => hex.to_lowercase(),
        None => sum_from_list(release, &asset.name)?,
    };
    let bytes = download(&asset.browser_download_url)?;
    let actual = hex(&Sha256::digest(&bytes));
    if actual != expected {
        return Err("the download didn't match its checksum, so it wasn't used".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let staged = staged_path(&exe);
    std::fs::write(&staged, &bytes).map_err(|e| format!("can't write next to {} ({e})", exe.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    let replaced = self_replace::self_replace(&staged).map_err(|e| format!("can't replace {} ({e})", exe.display()));
    let _ = std::fs::remove_file(&staged);
    replaced
}

/// The checksum from the release's SHA256SUMS file, for releases whose files carry no digest.
fn sum_from_list(release: &Release, name: &str) -> Result<String, String> {
    let url = release.sums.as_ref().ok_or("the release has no checksums, so it wasn't installed")?;
    let list = String::from_utf8(download(url)?).map_err(|e| e.to_string())?;
    list.lines()
        .filter_map(|l| l.split_once(char::is_whitespace))
        .find(|(_, file)| file.trim().trim_start_matches('*') == name)
        .map(|(sum, _)| sum.to_lowercase())
        .ok_or_else(|| "the release's checksums don't list this build".into())
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let mut response = agent().get(url).call().map_err(|e| e.to_string())?;
    response.body_mut().with_config().limit(MAX_DOWNLOAD).read_to_vec().map_err(|e| e.to_string())
}

fn staged_path(exe: &std::path::Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    exe.with_file_name(name)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("0.10.0", "0.9.3"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0-beta", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
    }

    /// Reaches GitHub, so it's left out of `cargo test`; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn asks_github() {
        let latest = latest();
        assert!(latest.is_ok(), "{latest:?}");
    }

    #[test]
    fn reads_a_release() {
        let json = r#"{"tag_name":"v0.2.0","html_url":"https://github.com/devatloam/tendril/releases/tag/v0.2.0","draft":false,
            "prerelease":false,"assets":[
              {"name":"tendril-linux-x86_64","browser_download_url":"https://example.invalid/l","digest":"sha256:ab12"},
              {"name":"tendril-windows-x86_64.exe","browser_download_url":"https://example.invalid/w","digest":null},
              {"name":"tendril-macos-aarch64","browser_download_url":"https://example.invalid/m"},
              {"name":"tendril-macos-x86_64","browser_download_url":"https://example.invalid/x"},
              {"name":"SHA256SUMS","browser_download_url":"https://example.invalid/sums"}]}"#;
        let release = parse(json).unwrap().unwrap();
        assert_eq!(release.version, "0.2.0");
        assert!(release.sums.is_some());
        assert_eq!(release.asset.as_ref().map(|a| a.name.as_str()), asset_name());
        let draft = json.replace(r#""draft":false"#, r#""draft":true"#);
        assert!(parse(&draft).unwrap().is_none());
    }

    #[test]
    fn hashes() {
        assert_eq!(hex(&Sha256::digest(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
