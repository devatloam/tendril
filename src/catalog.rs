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
    /// Something you copied.
    Clip,
    /// An open window.
    Window,
    /// Text of yours to paste.
    Snippet,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Application => "Application",
            Kind::Command => "Command",
            Kind::Link => "Quicklink",
            Kind::Window => "Window",
            Kind::Snippet => "Snippet",
            Kind::Calculator | Kind::Fallback | Kind::File | Kind::Clip => "",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Run {
    App(crate::apps::Launch),
    Power(Power),
    /// Emptying the bin, dark mode, the volume and the like.
    System(crate::system::Sys),
    Url(String),
    Copy(String),
    Theme(&'static str),
    /// The Settings window.
    Settings,
    /// config.toml in your editor.
    SettingsFile,
    Reload,
    /// Looks through the folders for file search again.
    IndexFiles,
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
    /// The Clipboard History view.
    Clipboard,
    /// The Switch Windows view.
    Windows,
    /// A copy in clipboard history, by its id.
    Clip(u64),
    /// A snippet's text, before {date} and the like are filled in.
    Snippet(String),
    /// Keeps this word as the alias being set (Set Alias); empty takes the alias away.
    SaveAlias(String),
    /// An open window, by the system's id for it.
    Window(u64),
    ClearClipboard,
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
            Run::Appearance | Run::Clipboard | Run::Windows | Run::System(crate::system::Sys::Downloads) => "Open",
            Run::Clip(_) | Run::Snippet(_) if crate::clipboard::can_paste() => "Paste",
            Run::Clip(_) | Run::Snippet(_) => "Copy",
            Run::Window(_) => "Switch to Window",
            Run::SaveAlias(ref w) if w.is_empty() => "Remove Alias",
            Run::SaveAlias(_) => "Save Alias",
            _ => "Run Command",
        }
    }
}

pub fn commands() -> Vec<Item> {
    // Before you type, in this order: what you came for first, power commands last, so the first
    // row under your hand is never Shut Down.
    let mut items = vec![
        Item::command("clipboard", "Clipboard History", "clipboard copied paste history clip", "clipboard", 7, Run::Clipboard),
        Item::command("clear-clipboard", "Clear Clipboard History", "clipboard forget delete copied", "eraser", 7, Run::ClearClipboard),
        Item::command("appearance", "Appearance", "settings look theme blur glow compact details grid keys", "palette", 7, Run::Appearance),
        Item::command("settings", "Settings", "preferences options config hotkey shortcut tendril", "settings", 7, Run::Settings),
        Item::command("settings-file", "Open Settings File", "config toml edit preferences", "file", 7, Run::SettingsFile),
        Item::command("updates", "Check for Updates", "update upgrade new version", "download", 7, Run::CheckUpdates),
        Item::command("reload", "Reload Apps", "refresh rescan", "refresh", 7, Run::Reload),
        Item::command("index-files", "Index Files", "files search reindex rescan folders documents status", "folder", 7, Run::IndexFiles),
        Item::command("setup", "Set Up Tendril", "onboarding welcome walkthrough shortcut hotkey", "wave", 7, Run::Onboarding),
        Item::command("about", "About Tendril", "version info", "tendril", 7, Run::About),
    ];
    // The system's own switches, before the power commands.
    for s in crate::system::Sys::ALL.into_iter().filter(|s| s.available()) {
        let (id, title, keywords, icon) = s.describe();
        // Red for the one that deletes.
        let tint = if s == crate::system::Sys::EmptyBin { 4 } else { 7 };
        items.push(Item::command(id, title, keywords, icon, tint, Run::System(s)));
    }
    items.extend([
        Item::command("lock", "Lock Screen", "lock away", "lock", 7, Run::Power(Power::Lock)),
        Item::command("sleep", "Sleep", "suspend", "moon", 7, Run::Power(Power::Sleep)),
        Item::command("logout", "Log Out", "sign out logoff", "logout", 7, Run::Power(Power::LogOut)),
        Item::command("restart", "Restart", "reboot", "restart", 7, Run::Power(Power::Restart)),
        Item::command("shutdown", "Shut Down", "power off poweroff turn off", "power", 4, Run::Power(Power::ShutDown)),
        Item::command("quit", "Quit Tendril", "exit close", "quit", 7, Run::Quit),
    ]);
    #[cfg(windows)]
    items.extend(windows_settings());
    if crate::switcher::available() {
        items.insert(1, Item::command("windows", "Switch Windows", "window switch focus alt tab open windows", "windows", 7, Run::Windows));
    }
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

/// Pages of the Windows Settings app, found by name like any command ("bluetooth", "display"),
/// with the Settings app's own icon. Not listed before you type.
#[cfg(windows)]
fn windows_settings() -> Vec<Item> {
    const PAGES: &[(&str, &str, &str)] = &[
        ("Display", "display", "screen resolution scale brightness monitor night light hdr"),
        ("Sound", "sound", "audio volume speaker microphone output input"),
        ("Notifications", "notifications", "focus do not disturb alerts"),
        ("Power & Battery", "powersleep", "battery sleep energy saver power plan"),
        ("Storage", "storagesense", "disk space cleanup drive"),
        ("Multitasking", "multitasking", "snap windows virtual desktops alt tab"),
        ("Clipboard Settings", "clipboard", "clipboard history sync"),
        ("About This PC", "about", "system info specs device name rename pc version"),
        ("Bluetooth & Devices", "bluetooth", "bluetooth devices pair headphones mouse keyboard"),
        ("Printers & Scanners", "printers", "printer scanner print"),
        ("Mouse", "mousetouchpad", "mouse pointer speed scroll buttons"),
        ("Touchpad", "devices-touchpad", "touchpad trackpad gestures"),
        ("Network & Internet", "network", "internet ethernet network status"),
        ("Wi-Fi", "network-wifi", "wifi wireless network"),
        ("VPN", "network-vpn", "vpn proxy"),
        ("Personalization", "personalization", "theme wallpaper colors"),
        ("Background", "personalization-background", "wallpaper desktop background"),
        ("Colors", "colors", "dark mode light mode accent color"),
        ("Taskbar", "taskbar", "taskbar start menu tray"),
        ("Installed Apps", "appsfeatures", "uninstall apps programs remove"),
        ("Default Apps", "defaultapps", "default browser default programs file associations"),
        ("Startup Apps", "startupapps", "startup login apps autostart"),
        ("Accounts", "yourinfo", "account user profile picture"),
        ("Sign-in Options", "signinoptions", "password pin windows hello fingerprint"),
        ("Date & Time", "dateandtime", "clock time zone date"),
        ("Language & Region", "regionlanguage", "language region keyboard layout input"),
        ("Typing", "typing", "autocorrect spelling text suggestions"),
        ("Gaming", "gaming-gamebar", "game bar game mode xbox"),
        ("Accessibility", "easeofaccess", "accessibility narrator magnifier contrast"),
        ("Privacy & Security", "privacy", "privacy security permissions"),
        ("Windows Security", "windowsdefender", "antivirus defender firewall virus"),
        ("Windows Update", "windowsupdate", "update windows updates patch"),
    ];
    const SETTINGS_APP: &str = "shell:AppsFolder\\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel";
    PAGES
        .iter()
        .map(|(title, page, keywords)| Item {
            id: format!("cmd:ms-settings:{page}"),
            title: (*title).into(),
            subtitle: "Windows Settings".into(),
            keywords: format!("settings {keywords}"),
            kind: Kind::Command,
            icon: Icon::Shell(SETTINGS_APP.into()),
            tint: 0,
            run: Run::Url(format!("ms-settings:{page}")),
            path: None,
        })
        .collect()
}
