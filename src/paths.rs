//! Typing a path lists that folder: `~/Doc` shows what in your home folder starts with "Doc".
//! The folder is read as you type and nothing is kept, so it costs nothing the rest of the time.

use crate::catalog::{Icon, Item, Kind, Run};
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

const MAX_ENTRIES: usize = 40;

/// Whether what's typed is a path rather than a search: it starts at the top of the disk, at
/// your home folder, or (on Windows) at a drive.
pub fn is_path(query: &str) -> bool {
    let q = query.trim_start();
    q.starts_with('/') || q == "~" || q.starts_with("~/") || (cfg!(windows) && (q.starts_with('\\') || q.starts_with("~\\") || is_drive(q)))
}

fn is_drive(q: &str) -> bool {
    let b = q.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// `~` becomes your home folder.
pub fn expand(query: &str) -> PathBuf {
    let q = query.trim();
    match q.strip_prefix('~') {
        Some(rest) => dirs::home_dir().unwrap_or_default().join(rest.trim_start_matches(['/', '\\'])),
        None => PathBuf::from(q),
    }
}

/// The folder to list and the start of the name typed so far: `~/Doc` is (home, "Doc").
fn split(query: &str) -> (PathBuf, String) {
    let full = expand(query);
    let ends_in_separator = query.ends_with('/') || query.ends_with('\\') || query.trim() == "~";
    if ends_in_separator || (cfg!(windows) && query.len() == 2 && is_drive(query)) {
        return (full, String::new());
    }
    let name = full.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    (full.parent().map(Path::to_path_buf).unwrap_or(full), name)
}

/// The folder's entries matching the typed name: folders first, then by name. Hidden files only
/// when what's typed starts with a dot.
pub fn entries(query: &str) -> Vec<Item> {
    let (dir, typed) = split(query);
    let typed_lower = typed.to_lowercase();
    let Ok(read) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut found: Vec<(bool, String, PathBuf)> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && !typed.starts_with('.') {
                return None;
            }
            let lower = name.to_lowercase();
            if !typed_lower.is_empty() && !lower.contains(&typed_lower) {
                return None;
            }
            let is_dir = e.file_type().map(|t| t.is_dir() || (t.is_symlink() && e.path().is_dir())).unwrap_or(false);
            Some((is_dir, name, e.path()))
        })
        .collect();
    // Names that start with what's typed before ones that only contain it.
    found.sort_by(|a, b| {
        let starts = |n: &str| !n.to_lowercase().starts_with(&typed_lower);
        (starts(&a.1), !a.0, a.1.to_lowercase()).cmp(&(starts(&b.1), !b.0, b.1.to_lowercase()))
    });
    found.into_iter().take(MAX_ENTRIES).map(|(is_dir, name, path)| item(is_dir, name, path)).collect()
}

fn item(is_dir: bool, name: String, path: PathBuf) -> Item {
    let shown = shorten_home(path.parent().unwrap_or(&path));
    Item {
        id: format!("path:{}", path.display()),
        title: if is_dir { format!("{name}{MAIN_SEPARATOR}") } else { name },
        subtitle: shown,
        keywords: String::new(),
        kind: Kind::File,
        icon: Icon::Builtin(if is_dir { "folder" } else { "file" }),
        tint: if is_dir { 2 } else { 7 },
        run: Run::Path(path.clone(), is_dir),
        path: Some(path),
    }
}

/// `/home/you/Documents` as `~/Documents`.
pub fn shorten_home(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(&home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{MAIN_SEPARATOR}{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// What the search field becomes when you go into a folder.
pub fn into(path: &Path) -> String {
    let shown = shorten_home(path);
    if shown.ends_with(MAIN_SEPARATOR) { shown } else { format!("{shown}{MAIN_SEPARATOR}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_a_path() {
        assert!(is_path("/usr"));
        assert!(is_path("~/Doc"));
        assert!(is_path("~"));
        assert!(!is_path("firefox"));
        assert!(!is_path("~bob"));
        assert!(!is_path("1/2"));
    }

    #[test]
    fn lists_a_folder() {
        let dir = std::env::temp_dir().join(format!("tendril-paths-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Documents")).unwrap();
        std::fs::write(dir.join("doc.txt"), "").unwrap();
        std::fs::write(dir.join(".hidden"), "").unwrap();
        std::fs::write(dir.join("notes.md"), "").unwrap();
        let q = format!("{}/doc", dir.display());
        let titles: Vec<String> = entries(&q).into_iter().map(|i| i.title).collect();
        assert_eq!(titles, [format!("Documents{MAIN_SEPARATOR}"), "doc.txt".to_string()], "folders first");
        let all = entries(&format!("{}/", dir.display()));
        assert_eq!(all.len(), 3, "hidden files stay hidden");
        assert_eq!(entries(&format!("{}/.h", dir.display())).len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
