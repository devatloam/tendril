//! What Tendril keeps on this computer, where, and how big it is: the list on Settings' Privacy &
//! Data tab. Deleting goes through App::delete_data, which also forgets what's in memory.

use std::path::{Path, PathBuf};

pub struct Kept {
    pub key: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    pub path: PathBuf,
    /// The button that deletes it, if it can be deleted from here.
    pub action: Option<&'static str>,
}

/// Everything kept, in the order it's listed. Ranking (usage.toml) has its own section.
pub fn kept() -> Vec<Kept> {
    let mut list = vec![
        Kept {
            key: "settings",
            title: "Settings",
            about: "Everything you set in Tendril, as text you can edit. Kept until you delete it yourself.",
            path: crate::config::path(),
            action: None,
        },
        Kept {
            key: "clipboard",
            title: "Clipboard history",
            about: "Up to 200 things you copied, for Clipboard History. Copies marked secret by password managers are skipped.",
            path: crate::clipboard::path(),
            action: Some("Clear"),
        },
        Kept {
            key: "files",
            title: "File index",
            about: "Names and paths of the files file search found, so it works as soon as Tendril starts. Never what's in them.",
            path: crate::files::cache_path(),
            action: Some("Delete"),
        },
        Kept {
            key: "apps",
            title: "App list",
            about: "The apps found last time, so the list is there the moment Tendril starts. Found again on every start.",
            path: crate::apps::cache_path(),
            action: Some("Delete"),
        },
        Kept {
            key: "icons",
            title: "Website icons",
            about: "Your quicklinks' sites' icons. Fetched again the next time they're listed, unless Website icons is off.",
            path: crate::sites::dir(),
            action: Some("Delete"),
        },
    ];
    if cfg!(windows) {
        list.push(Kept {
            key: "log",
            title: "Log",
            about: "What happened while Tendril ran, for when something goes wrong. Started fresh each time.",
            path: dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("tendril").join("tendril.log"),
            action: None,
        });
    }
    list
}

/// Bytes in a file, or in everything in a folder.
pub fn size(path: &Path) -> u64 {
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => std::fs::read_dir(path).into_iter().flatten().flatten().map(|e| size(&e.path())).sum(),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

/// "1.2 MB".
pub fn human(bytes: u64) -> String {
    match bytes {
        0 => "Nothing kept".into(),
        b if b < 1024 => format!("{b} bytes"),
        b if b < 1024 * 1024 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_well() {
        assert_eq!(human(0), "Nothing kept");
        assert_eq!(human(500), "500 bytes");
        assert_eq!(human(2048), "2 KB");
        assert_eq!(human(3 * 1024 * 1024 + 200_000), "3.2 MB");
        assert!(kept().iter().all(|k| !k.path.as_os_str().is_empty()));
    }
}
