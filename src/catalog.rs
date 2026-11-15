//! Everything you can search for: installed apps, and Flint's own commands.

use crate::system::Power;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Icon {
    #[cfg_attr(windows, allow(dead_code))]
    None,
    /// One of Flint's own glyphs (icons.rs), drawn on a tinted square.
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
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Application => "Application",
            Kind::Command => "Command",
            Kind::Calculator | Kind::Fallback => "",
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
            _ => "Run Command",
        }
    }
}

pub fn commands() -> Vec<Item> {
    let mut items = vec![
        Item::command("lock", "Lock Screen", "lock away", "lock", 2, Run::Power(Power::Lock)),
        Item::command("sleep", "Sleep", "suspend", "moon", 6, Run::Power(Power::Sleep)),
        Item::command("logout", "Log Out", "sign out logoff", "logout", 5, Run::Power(Power::LogOut)),
        Item::command("restart", "Restart", "reboot", "restart", 5, Run::Power(Power::Restart)),
        Item::command("shutdown", "Shut Down", "power off poweroff turn off", "power", 4, Run::Power(Power::ShutDown)),
        Item::command("settings", "Flint Settings", "preferences config hotkey", "settings", 7, Run::Settings),
        Item::command("reload", "Reload Apps", "refresh rescan", "refresh", 3, Run::Reload),
        Item::command("about", "About Flint", "version info", "bolt", 1, Run::About),
        Item::command("quit", "Quit Flint", "exit close", "quit", 7, Run::Quit),
    ];
    for p in crate::palettes::PALETTES {
        let mut item = Item::command(
            &format!("theme:{}", p.key),
            &format!("{} Theme", p.name),
            "theme palette colours colors appearance",
            "palette",
            1,
            Run::Theme(p.key),
        );
        item.subtitle = p.note.into();
        items.push(item);
    }
    items
}
