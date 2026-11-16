//! Finding files by name. The folders in [files] are walked on a background thread now and then
//! (on starting, on opening after a while, when the rules change, or by "Index Files"), and what's
//! found is kept in memory, paths only. There's no limit on how many: past the first few thousand
//! the walk paces itself and runs at background priority, so a big folder takes longer rather
//! than taking over the computer.
//!
//! Typing matches names loosely ("rprt" finds report.pdf) on a thread of its own, so a big index
//! never slows typing: the list gets its Files when they're found, a few milliseconds later, and
//! a search that's been typed past is given up.

use crate::catalog::Item;
use crate::config::Files;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32Str};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime};

/// Below this many letters, a search would match most of the disk.
const SHORTEST: usize = 2;
pub const MAX_SHOWN: usize = 8;
/// The walk runs flat out for this many, then paces itself.
const UNPACED: usize = 20_000;

#[derive(Clone)]
struct Entry {
    path: Box<str>,
    /// Where the name starts in `path`.
    name_at: u32,
    dir: bool,
}

impl Entry {
    fn new(path: &str, dir: bool) -> Option<Entry> {
        let name_at = Path::new(path).file_name()?.len();
        Some(Entry { path: path.into(), name_at: (path.len() - name_at) as u32, dir })
    }

    fn name(&self) -> &str {
        &self.path[self.name_at as usize..]
    }
}

#[derive(Default, Clone)]
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
    let mut paced = false;
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
        // A path that isn't valid Unicode can't be matched as text; it's left out.
        let Some(e) = entry.path().to_str().and_then(|p| Entry::new(p, dir)) else { continue };
        entries.push(e);
        FOUND.store(entries.len(), Ordering::Relaxed);
        if files.max_files > 0 && entries.len() >= files.max_files {
            full = true;
            break;
        }
        // Past the first few thousand: background priority, and a short rest every so often,
        // so a huge folder is walked slowly rather than keeping the disk and a core busy.
        if progress.is_some() && entries.len() > UNPACED {
            if !paced {
                paced = true;
                background_priority();
            }
            if entries.len().is_multiple_of(1000) {
                std::thread::sleep(Duration::from_millis(3));
            }
        }
    }
    entries.shrink_to_fit();
    (Index { entries }, full, problems)
}

/// Lowers this thread's priority, disk included on Windows, for the rest of the walk. The thread
/// ends with the walk, so nothing needs setting back.
fn background_priority() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
    #[cfg(target_os = "linux")]
    unsafe {
        // On Linux a thread has its own niceness.
        libc::setpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t, 10);
    }
}

/// Starts a walk on its own thread, unless one is under way. `progress` is called on the UI
/// thread now and then while it runs, `done` once with the new index.
/// `cached`, when given, gets the index kept from last time first, so file search works while
/// the folders are walked again.
pub fn start(files: Files, cached: Option<fn(Index)>, progress: fn(), done: fn(Index)) {
    {
        let mut state = STATE.lock().unwrap();
        if matches!(*state, State::Walking) {
            return;
        }
        *state = State::Walking;
    }
    FOUND.store(0, Ordering::Relaxed);
    let spawned = std::thread::Builder::new().name("files".into()).spawn(move || {
        if let Some(cached) = cached
            && let Some(index) = load(&cache_path())
        {
            let _ = slint::invoke_from_event_loop(move || cached(index));
        }
        let started = Instant::now();
        let (index, full, problems) = walk(&files, Some(progress));
        let count = index.entries.len();
        save(&index, &cache_path());
        *STATE.lock().unwrap() = State::Done { count, at: SystemTime::now(), took: started.elapsed(), full, problems };
        let _ = slint::invoke_from_event_loop(move || done(index));
    });
    if spawned.is_err() {
        *STATE.lock().unwrap() = State::Never;
    }
}

/// The index as kept between runs: a path a line, "d" or "f" and a tab before it.
pub fn cache_path() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("tendril").join("files.txt")
}

fn save(index: &Index, path: &Path) {
    let mut text = String::with_capacity(index.entries.len() * 80);
    for e in &index.entries {
        text.push(if e.dir { 'd' } else { 'f' });
        text.push('\t');
        text.push_str(&e.path);
        text.push('\n');
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, text);
}

fn load(path: &Path) -> Option<Index> {
    let text = std::fs::read_to_string(path).ok()?;
    let entries: Vec<Entry> = text
        .lines()
        .filter_map(|line| {
            let (kind, path) = line.split_once('\t')?;
            Entry::new(path, kind == "d")
        })
        .collect();
    (!entries.is_empty()).then_some(Index { entries })
}

/// Deletes the kept index; the next walk starts it again.
pub fn forget() -> std::io::Result<()> {
    if !walking() {
        *STATE.lock().unwrap() = State::Never;
    }
    match std::fs::remove_file(cache_path()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

impl Index {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Forgets a file or folder (and what's in it) that Tendril moved to the bin.
    pub fn remove(&mut self, path: &Path) {
        self.entries.retain(|e| !Path::new(&*e.path).starts_with(path));
    }

    /// The best matches for what's typed. Names match loosely, each word anywhere in the name
    /// with its letters in order ("rprt" finds report.pdf), as long as they hang together; then
    /// the exact name, names starting with it and names containing it come first, then shorter
    /// paths. The best `limit`, and how many matched in all; None if `stale` says a newer search
    /// came along first.
    fn find(&self, query: &str, limit: usize, matcher: &mut Matcher, stale: impl Fn() -> bool) -> Option<(Vec<Item>, usize)> {
        let query = query.trim().to_lowercase();
        let letters = query.chars().filter(|c| !c.is_whitespace()).count();
        if letters < SHORTEST {
            return Some((Vec::new(), 0));
        }
        let pattern = Pattern::new(&query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
        // Letters scattered across a name score low; below this, it isn't a match worth showing.
        let least = 12 * letters as u32;
        let mut buf = Vec::new();
        // The best few so far, best first: (score, path length, entry).
        let mut best: Vec<(u32, usize, &Entry)> = Vec::with_capacity(limit + 1);
        let mut total = 0;
        for (i, e) in self.entries.iter().enumerate() {
            if i % 4096 == 0 && stale() {
                return None;
            }
            let Some(score) = pattern.score(Utf32Str::new(e.name(), &mut buf), matcher).filter(|&s| s >= least) else { continue };
            total += 1;
            let name = e.name().to_lowercase();
            let bonus = if name == query {
                300
            } else if name.starts_with(&query) {
                200
            } else if name.contains(&query) {
                100
            } else {
                0
            };
            let candidate = (score + bonus, e.path.len(), e);
            let better = |a: &(u32, usize, &Entry), b: &(u32, usize, &Entry)| a.0 > b.0 || (a.0 == b.0 && a.1 < b.1);
            if best.len() == limit && !better(&candidate, &best[limit - 1]) {
                continue;
            }
            let at = best.iter().position(|b| better(&candidate, b)).unwrap_or(best.len());
            best.insert(at, candidate);
            best.truncate(limit);
        }
        let items = best.into_iter().map(|(_, _, e)| crate::paths::item(e.dir, e.name().to_string(), PathBuf::from(&*e.path))).collect();
        Some((items, total))
    }
}

/// Which search is the newest: a search sees it's been typed past when this moves on.
static ASKED: AtomicU64 = AtomicU64::new(0);

/// A search for the file search thread: what's typed, and the index to look in.
struct Ask {
    number: u64,
    query: String,
    limit: usize,
    index: Arc<Index>,
}

/// What a search found: the best `limit` for `query`, and how many matched in all.
pub struct Found {
    pub query: String,
    pub limit: usize,
    pub items: Vec<Item>,
    pub total: usize,
}

/// Looks for files on a thread of its own and hands back what it found, so typing never waits
/// on a big index. The thread sleeps until there's something to look for.
pub struct Finder {
    tx: Option<mpsc::Sender<Ask>>,
}

impl Finder {
    pub fn new(found: fn(Found)) -> Finder {
        let (tx, rx) = mpsc::channel::<Ask>();
        let started = std::thread::Builder::new().name("file-search".into()).spawn(move || {
            let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT.match_paths());
            while let Ok(mut ask) = rx.recv() {
                // Only the newest of whatever's waiting matters.
                while let Ok(newer) = rx.try_recv() {
                    ask = newer;
                }
                let number = ask.number;
                if let Some((items, total)) =
                    ask.index.find(&ask.query, ask.limit, &mut matcher, || ASKED.load(Ordering::Relaxed) != number)
                {
                    let result = Found { query: ask.query, limit: ask.limit, items, total };
                    let _ = slint::invoke_from_event_loop(move || found(result));
                }
            }
        });
        Finder { tx: started.ok().map(|_| tx) }
    }

    /// Looks for the best `limit` matches for `query` in `index`, giving up any search still running.
    pub fn ask(&self, query: &str, limit: usize, index: Arc<Index>) {
        let number = ASKED.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(tx) = &self.tx {
            let _ = tx.send(Ask { number, query: query.to_string(), limit: limit.max(1), index });
        }
    }
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

    fn search(index: &Index, query: &str) -> Vec<String> {
        let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT.match_paths());
        index.find(query, MAX_SHOWN, &mut matcher, || false).unwrap().0.into_iter().map(|i| i.title).collect()
    }

    fn names(files: &Files) -> Vec<String> {
        let (index, _, problems) = walk(files, None);
        assert!(problems.is_empty(), "{problems:?}");
        let mut names: Vec<String> = index.entries.iter().map(|e| e.name().to_lowercase()).collect();
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
        assert_eq!(search(&index, "report"), ["report.pdf", "report-draft.docx"]);
        assert_eq!(search(&index, "draft rep"), ["report-draft.docx"], "every word, in any order");
        assert!(search(&index, "r").is_empty(), "one letter matches too much");
        // Loosely: letters in order, close enough together.
        assert_eq!(search(&index, "rprt").first().map(String::as_str), Some("report.pdf"));
        assert_eq!(search(&index, "mainrs"), ["main.rs"]);
        assert!(search(&index, "zqx").is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn kept_between_runs() {
        let dir = tree("cache");
        let files = Files { folders: vec![dir.to_string_lossy().into_owned()], ..Files::default() };
        let (index, _, _) = walk(&files, None);
        let path = dir.join("kept.txt");
        save(&index, &path);
        let back = load(&path).unwrap();
        assert_eq!(back.len(), index.len());
        assert_eq!(search(&back, "report").len(), 2);
        assert!(back.entries.iter().any(|e| e.dir && e.name() == "notes"));
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
