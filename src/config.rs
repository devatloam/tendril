//! config.toml: what you can change, read again each time the window opens. The Appearance
//! view in Tendril writes the same file, one line at a time, leaving your comments alone.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub hotkey: String,
    pub theme: String,
    pub dark_theme: String,
    pub light_theme: String,
    pub hide_on_blur: bool,
    /// Set once the first-run walkthrough is done or skipped.
    pub onboarded: bool,
    /// "auto" installs new versions, "notify" offers them, "off" never asks GitHub.
    pub updates: String,
    /// Preview versions, built nightly from main before a release: "ask" offers the newest one
    /// beside updates, "always" follows them, "never" stays on releases.
    pub previews: String,
    /// Keep what you copy, for Clipboard History.
    pub clipboard_history: bool,
    /// Fetch quicklinks' sites' own icons.
    pub site_icons: bool,
    pub appearance: Appearance,
    pub search: Vec<Engine>,
    /// Your own links, [[quicklink]] in the file.
    pub quicklink: Vec<Quicklink>,
}

#[derive(Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Appearance {
    /// See through the window, with what's behind it blurred.
    pub blur: bool,
    /// What kind of see-through, where the system offers more than one: on Windows "acrylic",
    /// "mica", "mica-alt" or "blur"; on macOS "hud", "popover", "sidebar", "menu" or "under-window".
    pub blur_style: String,
    /// How solid the window is while blurred, from 0.4 to 1.
    pub opacity: f32,
    /// "off", "still" or "drift": colour glowing behind the list.
    pub glow: String,
    /// The line under the search field in colour.
    pub gradient_divider: bool,
    /// "regular" or "compact".
    pub size: String,
    /// A pane beside the list with the selected item's details.
    pub details: bool,
    /// "grid" or "list": how what you use most shows before you type.
    pub favourites: String,
    /// Ctrl 1–9 on the first nine rows, shown where the labels were.
    pub quick_keys: bool,
    /// "Results", "Commands" and the like above each group.
    pub headings: bool,
    /// "flat", or "modern": gradients and sheens, drawn with the GPU.
    pub style: String,
    /// The selected row: "white" or "blue".
    pub selection: String,
    /// "subtle" or "bold" squares behind the icons.
    pub icons: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            // On by default only where it's free: X11 draws see-through on the CPU.
            blur: !crate::window::needs_gpu_for_blur(),
            blur_style: default_blur_style().into(),
            opacity: 0.78,
            glow: "still".into(),
            gradient_divider: true,
            size: "regular".into(),
            details: false,
            favourites: "grid".into(),
            quick_keys: false,
            headings: true,
            style: "flat".into(),
            selection: "white".into(),
            icons: "subtle".into(),
        }
    }
}

pub fn default_blur_style() -> &'static str {
    if cfg!(target_os = "macos") { "hud" } else { "acrylic" }
}

impl Appearance {
    pub fn compact(&self) -> bool {
        self.size.eq_ignore_ascii_case("compact")
    }

    pub fn grid(&self) -> bool {
        self.favourites.eq_ignore_ascii_case("grid")
    }

    pub fn opacity(&self) -> f32 {
        self.opacity.clamp(0.4, 1.0)
    }

    /// 0 off, 1 still, 2 drifting.
    pub fn glow_mode(&self) -> i32 {
        match self.glow.to_lowercase().as_str() {
            "off" | "none" => 0,
            "drift" | "drifting" | "moving" => 2,
            _ => 1,
        }
    }
}

/// A link of your own: found by name, or opened with what you type after its keyword.
#[derive(Deserialize, Clone)]
pub struct Quicklink {
    pub name: String,
    #[serde(default)]
    pub keyword: String,
    /// `{query}` is what's typed after the keyword, made safe for a URL; `{raw}` is it as typed.
    /// A path or folder works as well as a web address.
    pub url: String,
}

impl Quicklink {
    pub fn takes_text(&self) -> bool {
        self.url.contains("{query}") || self.url.contains("{raw}")
    }
}

#[derive(Deserialize, Clone)]
pub struct Engine {
    pub name: String,
    pub url: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey: "Alt+Space".into(),
            theme: "system".into(),
            dark_theme: "ember".into(),
            light_theme: "paper".into(),
            hide_on_blur: true,
            onboarded: false,
            updates: "auto".into(),
            previews: "ask".into(),
            clipboard_history: true,
            site_icons: true,
            appearance: Appearance::default(),
            search: vec![
                Engine { name: "Google".into(), url: "https://www.google.com/search?q={query}".into() },
                Engine { name: "DuckDuckGo".into(), url: "https://duckduckgo.com/?q={query}".into() },
            ],
            quicklink: vec![Quicklink { name: "GitHub repository".into(), keyword: "gh".into(), url: "https://github.com/{raw}".into() }],
        }
    }
}

const TEMPLATE: &str = r#"# Tendril's settings. Saved changes apply the next time the window opens.
# Most of these can also be changed from Tendril itself: search for "Appearance".

# The keys that open Tendril from anywhere, like "Alt+Space", "Ctrl+Shift+Space" or "Super+KeyF".
# On Wayland, bind `tendril toggle` in your compositor instead: global keys are an X11, Windows and macOS thing.
hotkey = "Alt+Space"

# "system" follows your light or dark setting, with the two palettes below.
# Or name one palette to always use it: ember, midnight, moss, dusk, black, paper, sepia, mist.
theme = "system"
dark_theme = "ember"
light_theme = "paper"

# Close when you click somewhere else.
hide_on_blur = true

# Set once you've been through the first-run walkthrough ("Set Up Tendril" runs it again).
onboarded = false

# New versions: "auto" downloads and installs them (used from the next start), "notify" only
# offers them, "off" never asks. Checking is the one thing Tendril sends anywhere: a request to
# GitHub for the latest release, every six hours while it runs.
updates = "auto"

# Preview versions are built every night from the newest changes, before they're in a release, so they get new things
# first and can be rough. "ask" offers the newest one beside updates, "always" installs them as
# they come (and the next release once it's newer), "never" sticks to releases.
previews = "ask"

# Keep text you copy (up to 200 copies) for Clipboard History. It's kept on this computer only, in
# clipboard.json next to usage.toml. Copies a password manager marks as secret are skipped where the
# system shows the mark (Windows, and KDE on X11). Turning this off deletes what's kept.
clipboard_history = true

# Show your quicklinks with their sites' own icons. Each site's icon is fetched once, from that site
# only, and kept in the cache folder.
site_icons = true

[appearance]
# See through the window, with what's behind it blurred. On Linux under X11 this costs nothing
# extra and is on to start with; on Windows, macOS and Wayland it switches Tendril to drawing with
# the GPU, which keeps tens of megabytes more while Tendril runs, so it starts off there. Blur itself comes from the system: Windows'
# acrylic, macOS' vibrancy, KDE, or your compositor's blur setting (Hyprland, picom, …).
blur = true
# The kind of see-through, where the system has more than one. Windows: "acrylic" (blurs what's
# behind), "mica" (tinted by your wallpaper), "mica-alt" (a stronger tint) or "blur" (Windows 10).
# macOS: "hud", "popover", "sidebar", "menu" or "under-window". Linux leaves it to the compositor.
blur_style = "acrylic"
# How solid the window is while blurred, from 0.4 to 1.
opacity = 0.78
# Colour glowing behind the list: "off", "still", or "drift" (moves slowly; redraws while open).
glow = "still"
# The line under the search field in Blight's clay, blue and green.
gradient_divider = true
# "flat" draws plain fills, on the CPU. "modern" adds gradients and sheens, and draws with the GPU
# to keep their corners round, which keeps tens of megabytes more while Tendril runs.
style = "flat"
# The selected row: a "white" wash, or a soft "blue" one.
selection = "white"
# The squares behind icons: "subtle", or "bold" (with a blue selection, the selected one turns blue).
icons = "subtle"
# "regular" (750 × 474) or "compact" (narrower, shorter rows, no bar along the bottom).
size = "regular"
# A pane beside the list with the selected item's details.
details = false
# How what you use most shows before you type: "grid" or "list".
favourites = "grid"
# Ctrl 1–9 open the first nine rows, shown where the labels were.
quick_keys = false
# "Results", "Commands" and the like above each group.
headings = true

# Offered under your results as "Search <name> for …". {query} is what you typed.
[[search]]
name = "Google"
url = "https://www.google.com/search?q={query}"

[[search]]
name = "DuckDuckGo"
url = "https://duckduckgo.com/?q={query}"

# Your own links. Type the keyword, a space and some text: {query} is that text made safe for a
# web address, {raw} is it as typed. A link with neither is a bookmark you find by its name.
# Folders and files work too: url = "~/Projects".
[[quicklink]]
name = "GitHub repository"
keyword = "gh"
url = "https://github.com/{raw}"
"#;

/// The commented file, with this system's own default blur style.
fn template() -> String {
    TEMPLATE
        .replace("blur_style = \"acrylic\"", &format!("blur_style = \"{}\"", default_blur_style()))
        .replace("blur = true", &format!("blur = {}", Appearance::default().blur))
}

pub fn path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("tendril").join("config.toml")
}

/// Reads config.toml, writing the commented default the first time. A file that doesn't parse
/// falls back to the defaults and says why on stderr rather than refusing to open.
pub fn load() -> Config {
    let path = path();
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("tendril: {} has a mistake, using the defaults: {e}", path.display());
            Config::default()
        }),
        Err(_) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, template());
            Config::default()
        }
    }
}

/// A value as TOML writes it.
#[derive(Clone)]
pub enum Value<'a> {
    Text(&'a str),
    Bool(bool),
    Number(f32),
}

impl Value<'_> {
    fn literal(&self) -> String {
        match self {
            Value::Text(t) => format!("\"{}\"", t.replace('\\', "\\\\").replace('"', "\\\"")),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => format!("{:.2}", n).trim_end_matches('0').trim_end_matches('.').to_string(),
        }
    }
}

/// Sets one key in config.toml, at the top level (`section` None) or in a [section],
/// changing only that line, or adding it if it isn't there.
pub fn set(section: Option<&str>, key: &str, value: Value) -> std::io::Result<()> {
    let path = path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| template());
    std::fs::write(&path, with(&text, section, key, &value.literal()))
}

fn with(text: &str, section: Option<&str>, key: &str, literal: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let line = format!("{key} = {literal}");
    let is_key = |l: &str| l.trim_start().strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='));
    let is_header = |l: &str| l.trim_start().starts_with('[');
    // Where the section's lines run: from after its header (or the top) to the next header.
    let start = match section {
        None => Some(0),
        Some(name) => lines.iter().position(|l| l.trim() == format!("[{name}]")).map(|i| i + 1),
    };
    match start {
        Some(start) => {
            let end = lines[start..].iter().position(|l| is_header(l)).map_or(lines.len(), |i| start + i);
            if let Some(i) = (start..end).find(|&i| is_key(&lines[i])) {
                lines[i] = line;
            } else {
                // After the section's last setting, so it doesn't land below a blank line or a
                // comment meant for the next section.
                let at = (start..end).rev().find(|&i| !lines[i].trim().is_empty() && !lines[i].trim_start().starts_with('#'));
                lines.insert(at.map_or(start, |i| i + 1), line);
            }
        }
        None => {
            lines.push(String::new());
            lines.push(format!("[{}]", section.unwrap_or_default()));
            lines.push(line);
        }
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_parses_to_the_defaults() {
        // This system's version of the file, as written on first run.
        let config: Config = toml::from_str(&template()).unwrap();
        let default = Config::default();
        assert_eq!(config.hotkey, default.hotkey);
        assert_eq!(config.search.len(), default.search.len());
        assert_eq!(config.theme, default.theme);
        assert!(config.appearance == default.appearance);
    }

    #[test]
    fn sets_one_line() {
        let text = with(TEMPLATE, Some("appearance"), "glow", "\"drift\"");
        let config: Config = toml::from_str(&text).unwrap();
        assert_eq!(config.appearance.glow, "drift");
        assert_eq!(text.lines().count(), TEMPLATE.lines().count());
        assert!(text.contains("# Colour glowing behind the list"));

        let text = with(&text, None, "theme", "\"moss\"");
        assert_eq!(toml::from_str::<Config>(&text).unwrap().theme, "moss");

        // Missing from an older file: added to the section, or the section added.
        let old = "theme = \"ember\"\n\n[appearance]\nblur = false\n\n[[search]]\nname = \"A\"\nurl = \"b\"\n";
        let text = with(old, Some("appearance"), "details", "true");
        let config: Config = toml::from_str(&text).unwrap();
        assert!(config.appearance.details && !config.appearance.blur);
        let text = with("theme = \"ember\"\n", Some("appearance"), "blur", "false");
        assert!(!toml::from_str::<Config>(&text).unwrap().appearance.blur);
    }

    #[test]
    fn numbers_and_text() {
        assert_eq!(Value::Number(0.7).literal(), "0.7");
        assert_eq!(Value::Number(1.0).literal(), "1");
        assert_eq!(Value::Text("a\"b").literal(), "\"a\\\"b\"");
    }
}
