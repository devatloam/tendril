//! Everything you can search for: installed apps, and Tendril's own commands.

use crate::system::Power;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Icon {
    #[cfg_attr(windows, allow(dead_code))]
    None,
    /// One of Tendril's own glyphs (icons.rs), drawn on a tinted square.
    Builtin(&'static str),
    #[cfg_attr(windows, allow(dead_code))]
    File(PathBuf),
    /// A shell item Windows draws the icon for, by its parsing name.
    #[cfg_attr(not(windows), allow(dead_code))]
    Shell(String),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Application,
    Command,
    Calculator,
    Fallback,
    File,
    Link,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Application => "Application",
            Kind::Command => "Command",
            Kind::Link => "Quicklink",
            Kind::Calculator | Kind::Fallback | Kind::File => "",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Run {
    App(crate::apps::Launch),
    Power(Power),
    Url(String),
    Copy(String),
    Theme(&'static str),
    Settings,
    Reload,
    About,
    /// The Appearance view.
    Appearance,
    /// One setting in the Appearance view, by its config key.
    Setting(&'static str),
    /// The row that records the shortcut that opens Tendril.
    Shortcut,
    /// The first-run walkthrough, again.
    Onboarding,
    CheckUpdates,
    /// A file or folder: folders are gone into, files opened.
    Path(std::path::PathBuf, bool),
    /// Puts this in the search field, for a quicklink that wants text after its keyword.
    Prefill(String),
    InstallUpdate,
    /// Switches to preview versions and installs the newest.
    InstallPreview,
    RestartToUpdate,
    Quit,
}

#[derive(Clone, Debug)]
pub struct Item {
    /// Stable across runs; usage.toml is keyed by it.
    pub id: String,
    pub title: String,
    pub subtitle: String,
    /// Other words it should be found by, separated by spaces.
    pub keywords: String,
    pub kind: Kind,
    pub icon: Icon,
    /// Tint for a builtin glyph (see Theme.tint in ui/theme.slint); 0 for an app's own icon.
    pub tint: i32,
    pub run: Run,
    /// Where it is on disk, for "Show in folder" and "Copy path".
    pub path: Option<PathBuf>,
}

impl Item {
    pub fn command(id: &str, title: &str, keywords: &str, icon: &'static str, tint: i32, run: Run) -> Item {
        Item {
            id: format!("cmd:{id}"),
            title: title.into(),
            subtitle: String::new(),
            keywords: keywords.into(),
            kind: Kind::Command,
            icon: Icon::Builtin(icon),
            tint,
            run,
            path: None,
        }
    }

    /// What Enter does, in the bar along the bottom.
    pub fn verb(&self) -> &'static str {
        match self.run {
            Run::App(_) => "Open Application",
            Run::Copy(_) => "Copy Answer",
            Run::Url(_) if self.kind == Kind::Fallback => "Open in Browser",
            Run::Url(_) => "Open",
            Run::Setting(_) => "Change",
            Run::Shortcut => "Press New Keys",
            Run::InstallUpdate => "Update",
            Run::InstallPreview => "Install Preview",
            Run::Path(_, true) => "Open Folder",
            Run::Path(_, false) => "Open File",
            Run::Prefill(_) => "Type Text",
            Run::RestartToUpdate => "Restart",
            Run::Appearance => "Open",
            _ => "Run Command",
        }
    }
}

pub fn commands() -> Vec<Item> {
    let mut items = vec![
        Item::command("lock", "Lock Screen", "lock away", "lock", 7, Run::Power(Power::Lock)),
        Item::command("sleep", "Sleep", "suspend", "moon", 7, Run::Power(Power::Sleep)),
        Item::command("logout", "Log Out", "sign out logoff", "logout", 7, Run::Power(Power::LogOut)),
        Item::command("restart", "Restart", "reboot", "restart", 7, Run::Power(Power::Restart)),
        Item::command("shutdown", "Shut Down", "power off poweroff turn off", "power", 4, Run::Power(Power::ShutDown)),
        Item::command("settings", "Tendril Settings", "preferences config hotkey", "settings", 7, Run::Settings),
        Item::command("reload", "Reload Apps", "refresh rescan", "refresh", 7, Run::Reload),
        Item::command("appearance", "Appearance", "settings look theme blur glow compact details grid keys", "palette", 7, Run::Appearance),
        Item::command("updates", "Check for Updates", "update upgrade new version", "download", 7, Run::CheckUpdates),
        Item::command("setup", "Set Up Tendril", "onboarding welcome walkthrough shortcut hotkey", "wave", 7, Run::Onboarding),
        Item::command("about", "About Tendril", "version info", "tendril", 7, Run::About),
        Item::command("quit", "Quit Tendril", "exit close", "quit", 7, Run::Quit),
    ];
    for p in crate::palettes::PALETTES {
        let mut item = Item::command(
            &format!("theme:{}", p.key),
            &format!("{} Theme", p.name),
            "theme palette colours colors appearance",
            "swatch",
            7,
            Run::Theme(p.key),
        );
        item.subtitle = p.note.into();
        items.push(item);
    }
    items
}

/// Your quicklinks as things to find by name.
pub fn quicklinks(links: &[crate::config::Quicklink]) -> Vec<Item> {
    links
        .iter()
        .map(|link| {
            let keyword = keyword_of(link);
            let run = if link.takes_text() { Run::Prefill(format!("{keyword} ")) } else { link_target(&link.url) };
            Item {
                id: format!("link:{}", link.name),
                title: link.name.clone(),
                subtitle: if link.takes_text() { format!("{keyword} …") } else { link.url.clone() },
                keywords: format!("{keyword} {}", link.url),
                kind: Kind::Link,
                icon: Icon::Builtin("link"),
                tint: 7,
                run,
                path: None,
            }
        })
        .collect()
}

/// A quicklink's keyword, or the first word of its name when it has none.
pub fn keyword_of(link: &crate::config::Quicklink) -> String {
    let k = link.keyword.trim();
    if k.is_empty() { link.name.split_whitespace().next().unwrap_or("").to_lowercase() } else { k.to_lowercase() }
}

/// Where a filled-in link goes: a folder or file if it's a path, the browser otherwise.
pub fn link_target(url: &str) -> Run {
    if crate::paths::is_path(url) {
        let path = crate::paths::expand(url);
        let dir = path.is_dir();
        Run::Path(path, dir)
    } else {
        Run::Url(url.to_string())
    }
}
