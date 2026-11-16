//! Installed apps, found the way each system lists them, and opened the way it opens them.
//! Each module offers `Launch`, `scan()` and `launch()`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

use crate::catalog::{Icon, Item, Kind, Run};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// An app as kept between runs (apps.json in the cache folder), so the list is there the moment
/// Tendril starts while a fresh scan runs behind it, as Flow Launcher does it. Scanning takes a
/// second or two on Windows.
#[derive(Serialize, Deserialize)]
struct Kept {
    id: String,
    title: String,
    subtitle: String,
    keywords: String,
    icon: KeptIcon,
    launch: Launch,
    path: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
enum KeptIcon {
    None,
    File(PathBuf),
    Shell(String),
}

pub fn cache_path() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("tendril").join("apps.json")
}

/// The apps found last time, or nothing.
pub fn cached() -> Vec<Item> {
    let Ok(text) = std::fs::read_to_string(cache_path()) else { return Vec::new() };
    let kept: Vec<Kept> = serde_json::from_str(&text).unwrap_or_default();
    kept.into_iter()
        .map(|k| Item {
            id: k.id,
            title: k.title,
            subtitle: k.subtitle,
            keywords: k.keywords,
            kind: Kind::Application,
            icon: match k.icon {
                KeptIcon::None => Icon::None,
                KeptIcon::File(p) => Icon::File(p),
                KeptIcon::Shell(s) => Icon::Shell(s),
            },
            tint: 0,
            run: Run::App(k.launch),
            path: k.path,
        })
        .collect()
}

/// Keeps what a scan found, for next time.
pub fn remember(apps: &[Item]) {
    let kept: Vec<Kept> = apps
        .iter()
        .filter_map(|i| {
            let Run::App(launch) = &i.run else { return None };
            Some(Kept {
                id: i.id.clone(),
                title: i.title.clone(),
                subtitle: i.subtitle.clone(),
                keywords: i.keywords.clone(),
                icon: match &i.icon {
                    Icon::File(p) => KeptIcon::File(p.clone()),
                    Icon::Shell(s) => KeptIcon::Shell(s.clone()),
                    _ => KeptIcon::None,
                },
                launch: launch.clone(),
                path: i.path.clone(),
            })
        })
        .collect();
    let path = cache_path();
    if let (Some(dir), Ok(json)) = (path.parent(), serde_json::to_string(&kept)) {
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(path, json);
    }
}
