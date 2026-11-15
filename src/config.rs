//! config.toml: what you can change, read again each time the window opens.

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
    pub search: Vec<Engine>,
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
            search: vec![
                Engine { name: "Google".into(), url: "https://www.google.com/search?q={query}".into() },
                Engine { name: "DuckDuckGo".into(), url: "https://duckduckgo.com/?q={query}".into() },
            ],
        }
    }
}

const TEMPLATE: &str = r#"# Flint's settings. Saved changes apply the next time the window opens.

# The keys that open Flint from anywhere, like "Alt+Space", "Ctrl+Shift+Space" or "Super+KeyF".
# On Wayland, bind `flint toggle` in your compositor instead: global keys are an X11, Windows and macOS thing.
hotkey = "Alt+Space"

# "system" follows your light or dark setting, with the two palettes below.
# Or name one palette to always use it: ember, midnight, moss, dusk, black, paper, sepia, mist.
theme = "system"
dark_theme = "ember"
light_theme = "paper"

# Close when you click somewhere else.
hide_on_blur = true

# Offered under your results as "Search <name> for …". {query} is what you typed.
[[search]]
name = "Google"
url = "https://www.google.com/search?q={query}"

[[search]]
name = "DuckDuckGo"
url = "https://duckduckgo.com/?q={query}"
"#;

pub fn path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("flint").join("config.toml")
}

/// Reads config.toml, writing the commented default the first time. A file that doesn't parse
/// falls back to the defaults and says why on stderr rather than refusing to open.
pub fn load() -> Config {
    let path = path();
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("flint: {} has a mistake, using the defaults: {e}", path.display());
            Config::default()
        }),
        Err(_) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, TEMPLATE);
            Config::default()
        }
    }
}

/// Sets `theme = "…"` in the file, leaving everything else as you wrote it.
pub fn save_theme(key: &str) -> std::io::Result<()> {
    let path = path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| TEMPLATE.to_string());
    let line = format!("theme = \"{key}\"");
    let mut found = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            if !found && l.trim_start().starts_with("theme") && l.contains('=') {
                found = true;
                line.clone()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        out.insert(0, line);
    }
    std::fs::write(&path, out.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn template_parses_to_the_defaults() {
        let config: super::Config = toml::from_str(super::TEMPLATE).unwrap();
        let default = super::Config::default();
        assert_eq!(config.hotkey, default.hotkey);
        assert_eq!(config.search.len(), default.search.len());
        assert_eq!(config.theme, default.theme);
    }
}
