//! Apps on Linux: the .desktop files in the XDG data folders, and their icons from the icon theme.

use crate::catalog::{Icon, Item, Kind, Run};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Launch {
    argv: Vec<String>,
    terminal: bool,
    dir: Option<PathBuf>,
}

fn data_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut dirs = vec![std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"))];
    let system = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty()).unwrap_or("/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').map(PathBuf::from));
    // Where Flatpak and Snap put their apps, in case the session didn't add them.
    for extra in [home.join(".local/share/flatpak/exports/share"), "/var/lib/flatpak/exports/share".into(), "/var/lib/snapd/desktop".into()]
    {
        if !dirs.contains(&extra) {
            dirs.push(extra);
        }
    }
    dirs
}

pub fn scan() -> Vec<Item> {
    let lang = Lang::current();
    let desktops: Vec<String> =
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').filter(|s| !s.is_empty()).map(str::to_lowercase).collect();
    let mut icons = IconFinder::new();
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for dir in data_dirs() {
        let root = dir.join("applications");
        let mut files = Vec::new();
        collect(&root, &root, &mut files, 0);
        for (id, path) in files {
            // The first folder with a given id wins, so your own copy in ~/.local hides the system's.
            if !seen.insert(id.clone()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let entry = parse(&text);
            if let Some(item) = item(&id, &path, &entry, &lang, &desktops, &mut icons) {
                items.push(item);
            }
        }
    }
    items
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>, depth: u8) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < 3 {
            collect(root, &path, out, depth + 1);
        } else if path.extension().is_some_and(|e| e == "desktop") {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('/', "-");
            out.push((rel, path));
        }
    }
}

/// The keys of the [Desktop Entry] group. Other groups (actions) are skipped.
fn parse(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    map
}

struct Lang {
    keys: Vec<String>,
}

impl Lang {
    fn current() -> Lang {
        let raw =
            ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).unwrap_or_default();
        let base = raw.split(['.', '@']).next().unwrap_or("").to_string();
        let mut keys = Vec::new();
        if !base.is_empty() && base != "C" && base != "POSIX" {
            keys.push(base.clone());
            if let Some((lang, _)) = base.split_once('_') {
                keys.push(lang.to_string());
            }
        }
        Lang { keys }
    }

    fn get<'a>(&self, entry: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
        self.keys
            .iter()
            .find_map(|l| entry.get(&format!("{key}[{l}]")))
            .or_else(|| entry.get(key))
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }
}

fn item(id: &str, path: &Path, entry: &HashMap<String, String>, lang: &Lang, desktops: &[String], icons: &mut IconFinder) -> Option<Item> {
    let flag = |k: &str| entry.get(k).is_some_and(|v| v == "true");
    if entry.get("Type").map(String::as_str) != Some("Application") || flag("NoDisplay") || flag("Hidden") {
        return None;
    }
    let listed = |k: &str| entry.get(k).map(|v| v.split(';').any(|d| desktops.contains(&d.to_lowercase())));
    if listed("OnlyShowIn") == Some(false) || listed("NotShowIn") == Some(true) {
        return None;
    }
    if let Some(try_exec) = entry.get("TryExec").filter(|s| !s.is_empty()) {
        which(try_exec)?;
    }
    let name = lang.get(entry, "Name")?.to_string();
    let icon_name = entry.get("Icon").cloned().unwrap_or_default();
    let argv = exec_argv(entry.get("Exec")?, &icon_name, &name, path);
    if argv.is_empty() {
        return None;
    }
    let generic = lang.get(entry, "GenericName").unwrap_or("").to_string();
    let keywords = [lang.get(entry, "Keywords").unwrap_or(""), lang.get(entry, "Comment").unwrap_or("")].join(" ").replace(';', " ");
    Some(Item {
        id: format!("app:{}", id.trim_end_matches(".desktop")),
        subtitle: if generic.eq_ignore_ascii_case(&name) { String::new() } else { generic },
        title: name,
        keywords,
        kind: Kind::Application,
        icon: icons.find(&icon_name).map_or(Icon::None, Icon::File),
        tint: 0,
        run: Run::App(Launch { argv, terminal: flag("Terminal"), dir: entry.get("Path").filter(|p| !p.is_empty()).map(PathBuf::from) }),
        path: Some(path.to_path_buf()),
    })
}

/// Splits an Exec line into arguments, as the desktop entry spec says, dropping the
/// %f/%u style codes since Tendril never passes files.
fn exec_argv(exec: &str, icon: &str, name: &str, path: &Path) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if started || !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => current.push(c),
        }
    }
    if started || !current.is_empty() {
        args.push(current);
    }
    let mut out = Vec::new();
    for arg in args {
        match arg.as_str() {
            "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
            "%i" if !icon.is_empty() => out.extend(["--icon".to_string(), icon.to_string()]),
            "%i" => {}
            _ => {
                let arg = arg.replace("%c", name).replace("%k", &path.to_string_lossy());
                let mut cleaned = String::with_capacity(arg.len());
                let mut chars = arg.chars();
                while let Some(c) = chars.next() {
                    if c == '%' {
                        if let Some(n) = chars.next()
                            && n == '%'
                        {
                            cleaned.push('%');
                        }
                    } else {
                        cleaned.push(c);
                    }
                }
                out.push(cleaned);
            }
        }
    }
    out
}

fn which(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        return Path::new(program).exists().then(|| program.into());
    }
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(program)).find(|p| p.is_file()))
}

/// A terminal to run Terminal=true apps in, and how to hand it the command.
fn terminal() -> Option<Vec<String>> {
    if let Some(t) = std::env::var("TERMINAL").ok().filter(|t| which(t).is_some()) {
        return Some(vec![t, "-e".into()]);
    }
    let known: &[(&str, &[&str])] = &[
        ("x-terminal-emulator", &["-e"]),
        ("gnome-terminal", &["--"]),
        ("ptyxis", &["--"]),
        ("konsole", &["-e"]),
        ("kitty", &[]),
        ("alacritty", &["-e"]),
        ("foot", &[]),
        ("wezterm", &["start", "--"]),
        ("xfce4-terminal", &["-x"]),
        ("xterm", &["-e"]),
    ];
    known.iter().find(|(name, _)| which(name).is_some()).map(|(name, args)| {
        let mut v = vec![name.to_string()];
        v.extend(args.iter().map(|a| a.to_string()));
        v
    })
}

/// What launching it runs, for the details pane.
pub fn describe(launch: &Launch) -> Option<String> {
    let runs = launch.argv.join(" ");
    Some(if launch.terminal { format!("{runs} (in a terminal)") } else { runs })
}

pub fn launch(launch: &Launch) -> io::Result<()> {
    let mut argv = launch.argv.clone();
    if launch.terminal {
        let mut wrapped = terminal().ok_or_else(|| io::Error::other("no terminal found; set $TERMINAL"))?;
        wrapped.append(&mut argv);
        argv = wrapped;
    }
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    command.env_remove("SLINT_DESTROY_WINDOW_ON_HIDE");
    if let Some(dir) = &launch.dir {
        command.current_dir(dir);
    }
    crate::system::detach(&mut command);
    crate::system::reap(command.spawn()?);
    Ok(())
}

/// Finds an icon by name in the icon themes, preferring sizes close to how big it's drawn.
struct IconFinder {
    roots: Vec<PathBuf>,
    themes: Vec<String>,
}

const SIZES: &[&str] = &["48x48", "64x64", "scalable", "96x96", "128x128", "256x256", "32x32", "512x512", "24x24"];
const FLAT_SIZES: &[&str] = &["48", "64", "scalable", "96", "128", "256", "32"];

impl IconFinder {
    fn new() -> IconFinder {
        let home = dirs::home_dir().unwrap_or_default();
        let mut roots = vec![home.join(".icons")];
        roots.extend(data_dirs().into_iter().map(|d| d.join("icons")));
        roots.retain(|r| r.is_dir());
        let mut finder = IconFinder { roots, themes: Vec::new() };
        if let Some(theme) = current_theme() {
            finder.add_theme(&theme, 0);
        }
        for fallback in ["hicolor", "Adwaita", "breeze", "Papirus"] {
            finder.add_theme(fallback, 0);
        }
        finder
    }

    /// A theme and the themes it inherits from, in order.
    fn add_theme(&mut self, name: &str, depth: u8) {
        if depth > 4 || self.themes.iter().any(|t| t == name) {
            return;
        }
        self.themes.push(name.to_string());
        let index = self.roots.iter().map(|r| r.join(name).join("index.theme")).find(|p| p.is_file());
        let Some(text) = index.and_then(|p| std::fs::read_to_string(p).ok()) else { return };
        if let Some(parents) = text.lines().find_map(|l| l.strip_prefix("Inherits=")) {
            for parent in parents.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                self.add_theme(parent, depth + 1);
            }
        }
    }

    fn find(&mut self, name: &str) -> Option<PathBuf> {
        self.find_in(name, "apps")
    }

    /// `context` is the theme's folder for that kind of icon: apps, places, mimetypes.
    fn find_in(&mut self, name: &str, context: &str) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }
        let as_path = Path::new(name);
        if as_path.is_absolute() {
            return as_path.is_file().then(|| as_path.to_path_buf());
        }
        for theme in &self.themes {
            for root in &self.roots {
                let base = root.join(theme);
                if !base.is_dir() {
                    continue;
                }
                for size in SIZES {
                    if let Some(p) = probe(&base.join(size).join(context), name) {
                        return Some(p);
                    }
                }
                for size in FLAT_SIZES {
                    if let Some(p) = probe(&base.join(context).join(size), name) {
                        return Some(p);
                    }
                }
            }
        }
        data_dirs().iter().find_map(|d| probe(&d.join("pixmaps"), name)).or_else(|| probe(Path::new("/usr/share/pixmaps"), name))
    }
}

/// The icon theme, and what's been looked up in it.
type Theme = (IconFinder, HashMap<&'static str, Option<PathBuf>>);

thread_local! {
    static THEME: std::cell::RefCell<Option<Theme>> = const { std::cell::RefCell::new(None) };
}

/// The icon theme's icon for a folder or kind of file ("folder", "text-x-generic"), looked up
/// once and remembered.
pub fn theme_icon(name: &'static str) -> Option<PathBuf> {
    THEME.with_borrow_mut(|slot| {
        let (finder, found) = slot.get_or_insert_with(|| (IconFinder::new(), HashMap::new()));
        found
            .entry(name)
            .or_insert_with(|| {
                let context = if name.starts_with("folder") || name.starts_with("user-") { "places" } else { "mimetypes" };
                finder.find_in(name, context)
            })
            .clone()
    })
}

fn probe(dir: &Path, name: &str) -> Option<PathBuf> {
    ["svg", "png"].iter().map(|ext| dir.join(format!("{name}.{ext}"))).find(|p| p.is_file())
}

/// The icon theme from GTK's settings, which most desktops keep up to date.
fn current_theme() -> Option<String> {
    let config = dirs::config_dir()?;
    for file in ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"] {
        if let Ok(text) = std::fs::read_to_string(config.join(file))
            && let Some(v) = text.lines().find_map(|l| l.trim().strip_prefix("gtk-icon-theme-name"))
        {
            let v = v.trim_start_matches([' ', '=']).trim().trim_matches('"');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    let kde = std::fs::read_to_string(config.join("kdeglobals")).ok()?;
    let mut in_icons = false;
    for line in kde.lines() {
        if line.starts_with('[') {
            in_icons = line == "[Icons]";
        } else if in_icons && let Some(v) = line.strip_prefix("Theme=") {
            return Some(v.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_lines() {
        let p = Path::new("/x.desktop");
        assert_eq!(exec_argv("firefox %u", "", "Firefox", p), ["firefox"]);
        assert_eq!(exec_argv(r#""/opt/My App/run" --flag %F"#, "", "", p), ["/opt/My App/run", "--flag"]);
        assert_eq!(exec_argv("app --icon-name=%i x", "app", "", p), ["app", "--icon-name=", "x"]);
        assert_eq!(exec_argv("app %i", "app", "", p), ["app", "--icon", "app"]);
        assert_eq!(exec_argv(r#"sh -c "echo \"hi\"""#, "", "", p), ["sh", "-c", "echo \"hi\""]);
        assert_eq!(exec_argv("app 100%%", "", "", p), ["app", "100%"]);
    }

    #[test]
    fn entries() {
        let text =
            "[Desktop Entry]\nName=Files\nName[de]=Dateien\nExec=nautilus %U\nType=Application\n[Desktop Action new]\nName=New Window\n";
        let entry = parse(text);
        assert_eq!(entry.get("Name").map(String::as_str), Some("Files"));
        let lang = Lang { keys: vec!["de_DE".into(), "de".into()] };
        assert_eq!(lang.get(&entry, "Name"), Some("Dateien"));
    }
}
