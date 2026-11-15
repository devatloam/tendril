//! Update checks: the one thing Tendril sends anywhere. It asks GitHub for the releases of
//! devatloam/tendril, and with updates on "auto" downloads the build for this system, checks it
//! against the SHA-256 the release lists, and puts it in place of the running one. The new
//! version starts the next time Tendril does.
//!
//! There are two kinds of release. Versions like 0.2.0 come from a tag (`bun run bump`).
//! Previews like 0.2.1-nightly.20261115.42 are built by CI from every change to main and
//! published as prereleases; anything after the `-` names the kind. Preview versions sort before
//! the release they lead up to, so someone on previews moves on to that release when it's out.
//!
//! Everything here blocks; app.rs runs it on a thread of its own.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::path::PathBuf;
use std::time::Duration;

/// The newest releases first, previews included. Previews are pruned to the last few, so 30
/// always reaches back to the newest proper release.
const RELEASES: &str = "https://api.github.com/repos/devatloam/tendril/releases?per_page=30";

/// This build's version. CI sets TENDRIL_VERSION for previews; otherwise it's Cargo.toml's.
pub const VERSION: &str = match option_env!("TENDRIL_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
/// Bigger than any build should ever be; stops a bad download filling the disk.
const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    /// Built by CI from main rather than tagged: the version has something after a `-`.
    pub preview: bool,
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
        .user_agent(format!("Tendril/{VERSION} (update check)"))
        .build()
        .into()
}

/// What GitHub has: the newest release, and the newest preview with a build for this system.
#[derive(Debug, Default)]
pub struct Found {
    pub release: Option<Release>,
    pub preview: Option<Release>,
}

pub fn check() -> Result<Found, String> {
    let response = agent().get(RELEASES).header("Accept", "application/vnd.github+json").call();
    let body = match response {
        Err(ureq::Error::StatusCode(404)) => return Ok(Found::default()),
        Err(e) => return Err(e.to_string()),
        Ok(mut r) => r.body_mut().read_to_string().map_err(|e| e.to_string())?,
    };
    parse(&body)
}

/// Releases as GitHub's API lists them, reduced to the newest of each kind.
fn parse(json: &str) -> Result<Found, String> {
    let releases: Vec<GitHubRelease> = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let wanted = asset_name();
    let mut found = Found::default();
    for r in releases.into_iter().filter(|r| !r.draft) {
        let version = r.tag_name.trim_start_matches('v').to_string();
        let preview = r.prerelease || version.contains('-');
        let asset = r.assets.iter().find(|a| Some(a.name.as_str()) == wanted).cloned();
        // A preview without a build for this system is no use; a release without one still
        // gets mentioned, with a link to its page.
        if preview && asset.is_none() {
            continue;
        }
        let sums = r.assets.iter().find(|a| a.name == "SHA256SUMS").map(|a| a.browser_download_url.clone());
        let release = Release { version, preview, page: r.html_url, asset, sums };
        let slot = if preview { &mut found.preview } else { &mut found.release };
        if slot.as_ref().is_none_or(|s| newer(&release.version, &s.version)) {
            *slot = Some(release);
        }
    }
    Ok(found)
}

/// What kind of preview a version is: "nightly" for 0.2.1-nightly.20261115.42.
pub fn kind(version: &str) -> Option<&str> {
    let (_, pre) = version.split_once('-')?;
    pre.split('.').next().filter(|k| !k.is_empty())
}

/// Whether `theirs` is a later version than `ours`: 0.10.0 is after 0.9.3, and a preview of
/// 0.2.1 is after 0.2.0 but before 0.2.1 itself.
pub fn newer(theirs: &str, ours: &str) -> bool {
    compare(theirs, ours) == Ordering::Greater
}

/// Semantic versioning's order, build metadata (after a `+`) ignored.
fn compare(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> (Vec<u64>, Option<String>) {
        let v = v.trim().trim_start_matches('v');
        let v = v.split('+').next().unwrap_or("");
        let (core, pre) = match v.split_once('-') {
            Some((core, pre)) => (core, Some(pre.to_string())),
            None => (v, None),
        };
        let mut numbers: Vec<u64> = core.split('.').map(|p| p.parse().unwrap_or(0)).collect();
        numbers.resize(3, 0);
        (numbers, pre)
    };
    let (core_a, pre_a) = parts(a);
    let (core_b, pre_b) = parts(b);
    core_a.cmp(&core_b).then_with(|| match (pre_a, pre_b) {
        (None, None) => Ordering::Equal,
        // 0.2.0 is after any 0.2.0-something.
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            let (mut x, mut y) = (x.split('.'), y.split('.'));
            loop {
                match (x.next(), y.next()) {
                    (None, None) => break Ordering::Equal,
                    (None, Some(_)) => break Ordering::Less,
                    (Some(_), None) => break Ordering::Greater,
                    (Some(p), Some(q)) => {
                        let order = match (p.parse::<u64>(), q.parse::<u64>()) {
                            (Ok(m), Ok(n)) => m.cmp(&n),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => p.cmp(q),
                        };
                        if order != Ordering::Equal {
                            break order;
                        }
                    }
                }
            }
        }
    })
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
        // Previews of the next version come after this one and before that version itself.
        assert!(newer("0.1.1-nightly.20261115.42", "0.1.0"));
        assert!(newer("0.1.1", "0.1.1-nightly.20261115.42"));
        assert!(newer("0.1.1-nightly.20261116.3", "0.1.1-nightly.20261115.42"));
        assert!(newer("0.1.1-nightly.20261115.42", "0.1.1-nightly.20261115.9"));
        assert!(newer("0.2.0-beta.2", "0.2.0-alpha.9"));
        assert!(newer("0.2.0-alpha.1", "0.2.0-alpha"));
        assert_eq!(kind("0.1.1-nightly.20261115.42"), Some("nightly"));
        assert_eq!(kind("0.1.1"), None);
    }

    /// Reaches GitHub, so it's left out of `cargo test`; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn asks_github() {
        let found = check();
        assert!(found.is_ok(), "{found:?}");
    }

    #[test]
    fn reads_releases() {
        let assets = |extra: &str| {
            format!(
                r#"[{{"name":"tendril-linux-x86_64","browser_download_url":"https://example.invalid/l","digest":"sha256:ab12"}},
                {{"name":"tendril-windows-x86_64.exe","browser_download_url":"https://example.invalid/w","digest":null}},
                {{"name":"tendril-macos-aarch64","browser_download_url":"https://example.invalid/m"}},
                {{"name":"tendril-macos-x86_64","browser_download_url":"https://example.invalid/x"}}{extra}]"#
            )
        };
        let release = |tag: &str, pre: bool, draft: bool, assets: &str| {
            format!(
                r#"{{"tag_name":"{tag}","html_url":"https://example.invalid/{tag}","draft":{draft},"prerelease":{pre},"assets":{assets}}}"#
            )
        };
        let json = format!(
            "[{},{},{},{},{}]",
            release("v0.2.1-nightly.20261115.7", true, false, &assets("")),
            release("v0.3.0", false, true, &assets("")),
            release("v0.2.1-nightly.20261114.6", true, false, &assets("")),
            release("v0.2.1-nightly.20261116.8", true, false, "[]"),
            release("v0.2.0", false, false, &assets(r#",{"name":"SHA256SUMS","browser_download_url":"https://example.invalid/sums"}"#)),
        );
        let found = parse(&json).unwrap();
        let stable = found.release.unwrap();
        assert_eq!(stable.version, "0.2.0", "drafts are skipped");
        assert!(!stable.preview && stable.sums.is_some());
        assert_eq!(stable.asset.as_ref().map(|a| a.name.as_str()), asset_name());
        if asset_name().is_some() {
            let preview = found.preview.unwrap();
            assert_eq!(preview.version, "0.2.1-nightly.20261115.7", "the newest preview with a build for this system");
            assert!(preview.preview);
        }
        assert!(parse("[]").unwrap().release.is_none());
    }

    #[test]
    fn hashes() {
        assert_eq!(hex(&Sha256::digest(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
