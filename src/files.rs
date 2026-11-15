//! Finding files by name. The folders in [files] are walked on a background thread now and then
//! (on starting, on opening after a while, when the rules change, or by "Index Files"), and what's
//! found is kept in memory, names and paths only. Typing matches against that list; nothing
//! reads the disk while you type.

use crate::catalog::Item;
use crate::config::Files;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// Below this many letters, a search would match most of the disk.
const SHORTEST: usize = 2;
pub const MAX_SHOWN: usize = 8;

struct Entry {
    /// The name, lowercase, for matching.
    name: Box<str>,
    path: PathBuf,
    dir: bool,
}

#[derive(Default)]
pub struct Index {
    entries: Vec<Entry>,
}

/// Where the index stands, said in the Files rows.
enum State {
    Off,
    Never,
    Walking,
    Done { count: usize, at: SystemTime, took: Duration, full: bool, problems: Vec<String> },
}

static STATE: Mutex<State> = Mutex::new(State::Never);
/// Found so far by the walk under way, read for "Indexing… 3,210 so far".
static FOUND: AtomicUsize = AtomicUsize::new(0);

pub fn walking() -> bool {
    matches!(*STATE.lock().unwrap(), State::Walking)
}

pub fn found_so_far() -> usize {
    FOUND.load(Ordering::Relaxed)
}

pub fn set_off() {
    *STATE.lock().unwrap() = State::Off;
}

/// One line on where the index stands: "12,034 files · 3 minutes ago".
pub fn status() -> String {
    match &*STATE.lock().unwrap() {
        State::Off => "Off: files aren't searched".into(),
        State::Never => "Not looked through yet".into(),
        State::Walking => format!("Indexing… {} so far", thousands(found_so_far())),
        State::Done { count, at, full, problems, .. } => {
            let mut s = format!("{} files and folders · indexed {}", thousands(*count), crate::app::ago(since(*at)).to_lowercase());
            if *full {
                s.push_str(" · stopped at the limit");
            }
            if !problems.is_empty() {
                s.push_str(" · a rule has a mistake");
            }
            s
        }
    }
}

/// The details pane's facts about the index: (label, value).
pub fn facts(files: &Files) -> Vec<(&'static str, String)> {
    let mut facts = vec![("Folders", roots(files).iter().map(|p| crate::paths::shorten_home(p)).collect::<Vec<_>>().join(", "))];
    if let State::Done { count, at, took, full, problems } = &*STATE.lock().unwrap() {
        facts.push(("Found", thousands(*count)));
        facts.push(("Last indexed", crate::app::ago(since(*at))));
        facts.push(("Took", format!("{:.1} s", took.as_secs_f32())));
        if *full {
            facts.push(("Limit", format!("Stopped at max_files ({})", thousands(files.max_files))));
        }
        for p in problems {
            facts.push(("Mistake", p.clone()));
        }
    }
    let rules = [
        (!files.include.is_empty()).then(|| format!("only {}", files.include.join(", "))),
        (!files.exclude.is_empty()).then(|| format!("not {}", files.exclude.join(", "))),
        (!files.exclude_regex.is_empty()).then(|| format!("not matching {}", files.exclude_regex.join(", "))),
    ];
    let rules: Vec<String> = rules.into_iter().flatten().collect();
    if !rules.is_empty() {
        facts.push(("Rules", rules.join("; ")));
    }
    facts
}

fn since(at: SystemTime) -> u64 {
    SystemTime::now().duration_since(at).map_or(0, |d| d.as_secs())
}

pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The folders to look through: yours, or the usual ones.
pub fn roots(files: &Files) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = if files.folders.is_empty() {
        [dirs::desktop_dir(), dirs::document_dir(), dirs::download_dir(), dirs::picture_dir(), dirs::audio_dir(), dirs::video_dir()]
            .into_iter()
            .flatten()
            .collect()
    } else {
        files.folders.iter().map(|f| crate::paths::expand(f)).collect()
    };
    roots.retain(|r| r.is_dir());
    roots.sort();
    // A folder inside another one listed would be walked twice.
    let mut kept: Vec<PathBuf> = Vec::new();
    for root in roots {
        if !kept.iter().any(|k| root.starts_with(k)) {
            kept.push(root);
        }
    }
    kept
}

/// What's left out and what's kept, built once per walk.
struct Rules {
    /// Per folder walked: its exclude and include patterns, rooted there.
    per_root: Vec<(PathBuf, Gitignore, Gitignore)>,
    /// Whole folders left out by path ("~/Downloads/old").
    prefixes: Vec<PathBuf>,
    regexes: Vec<Regex>,
    only: bool,
}

impl Rules {
    fn new(files: &Files, roots: &[PathBuf], problems: &mut Vec<String>) -> Rules {
        let by_path = |p: &str| p.starts_with('~') || Path::new(p).is_absolute();
        let mut per_root = Vec::new();
        for root in roots {
            // The same mistake turns up for every folder; said for the first.
            let first = per_root.is_empty();
            let mut exclude = GitignoreBuilder::new(root);
            let mut include = GitignoreBuilder::new(root);
            for pattern in files.exclude.iter().filter(|p| !by_path(p)) {
                if let Err(e) = exclude.add_line(None, pattern)
                    && first
                {
                    problems.push(format!("exclude \"{pattern}\": {e}"));
                }
            }
            for pattern in &files.include {
                if let Err(e) = include.add_line(None, pattern)
                    && first
                {
                    problems.push(format!("include \"{pattern}\": {e}"));
                }
            }
            let exclude = exclude.build().unwrap_or_else(|_| Gitignore::empty());
            let include = include.build().unwrap_or_else(|_| Gitignore::empty());
            per_root.push((root.clone(), exclude, include));
        }
        let prefixes = files.exclude.iter().filter(|p| by_path(p)).map(|p| crate::paths::expand(p)).collect();
        let regexes = files
            .exclude_regex
            .iter()
            .filter_map(|r| Regex::new(r).map_err(|e| problems.push(format!("exclude_regex \"{r}\": {e}"))).ok())
            .collect();
        Rules { per_root, prefixes, regexes, only: !files.include.is_empty() }
    }

    fn root(&self, path: &Path) -> Option<&(PathBuf, Gitignore, Gitignore)> {
        self.per_root.iter().find(|(root, ..)| path.starts_with(root))
    }

    fn left_out(&self, path: &Path, dir: bool) -> bool {
        if self.prefixes.iter().any(|p| path.starts_with(p)) {
            return true;
        }
        if let Some((_, exclude, _)) = self.root(path)
            && exclude.matched(path, dir).is_ignore()
        {
            return true;
        }
        if !self.regexes.is_empty() {
            let shown = path.to_string_lossy().replace('\\', "/");
            if self.regexes.iter().any(|r| r.is_match(&shown)) {
                return true;
            }
        }
        false
    }

    /// Whether a file is one `include` asks for (every file, when it's empty).
    fn wanted(&self, path: &Path) -> bool {
        !self.only || self.root(path).is_some_and(|(_, _, include)| include.matched(path, false).is_ignore())
    }
}

/// Walks the folders. Run on its own thread; `FOUND` counts as it goes, and `progress` is sent
/// to the UI thread a few times a second.
fn walk(files: &Files, progress: Option<fn()>) -> (Index, bool, Vec<String>) {
    let roots = roots(files);
    let mut problems = Vec::new();
    let Some(first) = roots.first() else {
        problems.push("none of the folders exist".into());
        return (Index::default(), false, problems);
    };
    let rules = std::sync::Arc::new(Rules::new(files, &roots, &mut problems));
    let mut builder = ignore::WalkBuilder::new(first);
    for root in &roots[1..] {
        builder.add(root);
    }
    let r = rules.clone();
    builder
        .hidden(!files.hidden)
        .git_ignore(files.gitignore)
        .git_exclude(files.gitignore)
        .git_global(files.gitignore)
        .ignore(files.gitignore)
        .parents(files.gitignore)
        // A .gitignore counts in any folder, not only inside a git repository.
        .require_git(false)
        .follow_links(false)
        .max_depth(Some(files.max_depth.max(1)))
        .filter_entry(move |e| !r.left_out(e.path(), e.file_type().is_some_and(|t| t.is_dir())));
    let mut entries = Vec::new();
    let mut full = false;
    let mut told = Instant::now();
    for entry in builder.build().flatten() {
        if let Some(progress) = progress
            && told.elapsed() > Duration::from_millis(300)
        {
            told = Instant::now();
            let _ = slint::invoke_from_event_loop(progress);
        }
        if entry.depth() == 0 {
            continue;
        }
        let dir = entry.file_type().is_some_and(|t| t.is_dir());
        if !dir && !rules.wanted(entry.path()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase().into_boxed_str();
        entries.push(Entry { name, path: entry.into_path(), dir });
        FOUND.store(entries.len(), Ordering::Relaxed);
        if entries.len() >= files.max_files {
            full = true;
            break;
        }
    }
    entries.shrink_to_fit();
    (Index { entries }, full, problems)
}

/// Starts a walk on its own thread, unless one is under way. `progress` is called on the UI
/// thread now and then while it runs, `done` once with the new index.
pub fn start(files: Files, progress: fn(), done: fn(Index)) {
    {
        let mut state = STATE.lock().unwrap();
        if matches!(*state, State::Walking) {
            return;
        }
        *state = State::Walking;
    }
    FOUND.store(0, Ordering::Relaxed);
    let spawned = std::thread::Builder::new().name("files".into()).spawn(move || {
        let started = Instant::now();
        let (index, full, problems) = walk(&files, Some(progress));
        let count = index.entries.len();
        *STATE.lock().unwrap() = State::Done { count, at: SystemTime::now(), took: started.elapsed(), full, problems };
        let _ = slint::invoke_from_event_loop(move || done(index));
    });
    if spawned.is_err() {
        *STATE.lock().unwrap() = State::Never;
    }
}

impl Index {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The best matches for what's typed: every word somewhere in the name, names that start
    /// with it first, then shorter paths.
    pub fn find(&self, query: &str) -> Vec<Item> {
        let query = query.trim().to_lowercase();
        if query.chars().filter(|c| !c.is_whitespace()).count() < SHORTEST {
            return Vec::new();
        }
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut found: Vec<(u8, usize, &Entry)> = self
            .entries
            .iter()
            .filter(|e| words.iter().all(|w| e.name.contains(w)))
            .map(|e| {
                let rank = if *e.name == *query {
                    0
                } else if e.name.starts_with(&query) {
                    1
                } else if e.name.contains(&query) {
                    2
                } else {
                    3
                };
                (rank, e.path.as_os_str().len(), e)
            })
            .collect();
        let by = |a: &(u8, usize, &Entry), b: &(u8, usize, &Entry)| (a.0, a.1).cmp(&(b.0, b.1));
        if found.len() > MAX_SHOWN {
            found.select_nth_unstable_by(MAX_SHOWN, by);
            found.truncate(MAX_SHOWN);
        }
        found.sort_by(by);
        found.into_iter().map(|(_, _, e)| crate::paths::item(e.dir, display_name(&e.path), e.path.clone())).collect()
    }
}

fn display_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tendril-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["notes", "app/node_modules/left-pad", "app/build", "app/src", ".secret"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for f in [
            "notes/report.pdf",
            "notes/report-draft.docx",
            "app/node_modules/left-pad/index.js",
            "app/build/out.bin",
            "app/src/main.rs",
            "app/debug.log",
            ".secret/key.txt",
        ] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        std::fs::write(dir.join("app/.gitignore"), "*.log\n").unwrap();
        dir
    }

    fn names(files: &Files) -> Vec<String> {
        let (index, _, problems) = walk(files, None);
        assert!(problems.is_empty(), "{problems:?}");
        let mut names: Vec<String> = index.entries.iter().map(|e| e.name.to_string()).collect();
        names.sort();
        names
    }

    #[test]
    fn follows_the_rules() {
        let dir = tree("rules");
        let base = Files { folders: vec![dir.to_string_lossy().into_owned()], ..Files::default() };
        let all = names(&base);
        assert!(all.contains(&"report.pdf".into()) && all.contains(&"main.rs".into()));
        assert!(!all.contains(&"index.js".into()), "node_modules is left out by default: {all:?}");
        assert!(!all.contains(&"debug.log".into()), ".gitignore is followed");
        assert!(!all.contains(&"key.txt".into()) && !all.contains(&".secret".into()), "hidden stays hidden");

        let loose = Files { gitignore: false, hidden: true, exclude: vec![], ..base.clone() };
        let all = names(&loose);
        assert!(all.contains(&"debug.log".into()) && all.contains(&"key.txt".into()) && all.contains(&"index.js".into()));

        let only = Files { include: vec!["*.pdf".into()], ..base.clone() };
        let all = names(&only);
        assert!(all.contains(&"report.pdf".into()) && !all.contains(&"report-draft.docx".into()));
        assert!(all.contains(&"notes".into()), "folders are still listed");

        let regex = Files { exclude_regex: vec!["/app/build(/|$)".into()], ..base.clone() };
        assert!(!names(&regex).contains(&"out.bin".into()));

        let by_path = Files { exclude: vec![format!("{}/notes", dir.display())], ..base };
        assert!(!names(&by_path).contains(&"report.pdf".into()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn finds_best_first() {
        let dir = tree("find");
        let files = Files { folders: vec![dir.to_string_lossy().into_owned()], ..Files::default() };
        let (index, _, _) = walk(&files, None);
        let titles: Vec<String> = index.find("report").into_iter().map(|i| i.title).collect();
        assert_eq!(titles, ["report.pdf", "report-draft.docx"]);
        assert_eq!(index.find("draft rep").len(), 1, "every word, in any order");
        assert!(index.find("r").is_empty(), "one letter matches too much");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn mistakes_are_said() {
        let files = Files {
            folders: vec![std::env::temp_dir().to_string_lossy().into_owned()],
            exclude_regex: vec!["(".into()],
            max_depth: 1,
            ..Files::default()
        };
        let (_, _, problems) = walk(&files, None);
        assert!(problems.iter().any(|p| p.contains("exclude_regex")));
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
