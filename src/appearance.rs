//! The Appearance view: every look setting as a row you change with Enter or ← →, saved to
//! config.toml as you go.

use crate::config::{self, Config, Value};
use crate::palettes::PALETTES;

/// Where a setting is kept.
#[derive(Clone, Copy)]
enum Kept {
    /// A key in config.toml, at the top or in a [section].
    Config(Option<&'static str>),
    /// The system's own list of what opens at login (autostart.rs).
    Login,
}

pub struct Setting {
    pub key: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    pub icon: &'static str,
    pub tint: i32,
    kept: Kept,
    pub choices: Vec<(String, Value<'static>)>,
    pub current: usize,
}

impl Setting {
    pub fn label(&self) -> &str {
        &self.choices[self.current].0
    }

    /// Moves to the next (1) or previous (-1) choice, wrapping, and saves it.
    /// The value one step along, without saving it.
    pub fn next(&self, delta: i32) -> &Value<'static> {
        let n = self.choices.len() as i32;
        &self.choices[(self.current as i32 + delta).rem_euclid(n) as usize].1
    }

    pub fn step(&self, delta: i32) -> std::io::Result<String> {
        let n = self.choices.len() as i32;
        let next = (self.current as i32 + delta).rem_euclid(n) as usize;
        let (label, value) = &self.choices[next];
        match (self.kept, value) {
            (Kept::Config(section), value) => config::set(section, self.key, value.clone())?,
            (Kept::Login, Value::Bool(on)) => crate::autostart::set(*on)?,
            (Kept::Login, _) => {}
        }
        Ok(label.clone())
    }
}

fn on_off(on: bool) -> (Vec<(String, Value<'static>)>, usize) {
    (vec![("On".into(), Value::Bool(true)), ("Off".into(), Value::Bool(false))], if on { 0 } else { 1 })
}

fn texts(options: &[(&'static str, &'static str)], current: &str) -> (Vec<(String, Value<'static>)>, usize) {
    let at = options.iter().position(|(v, _)| v.eq_ignore_ascii_case(current)).unwrap_or(0);
    (options.iter().map(|(v, label)| (label.to_string(), Value::Text(v))).collect(), at)
}

const OPACITIES: [f32; 6] = [0.6, 0.7, 0.78, 0.85, 0.92, 1.0];

pub fn settings(config: &Config) -> Vec<Setting> {
    let a = &config.appearance;
    let mut list = Vec::new();
    let mut add = |key, title, about, icon, tint, section, (choices, current): (Vec<(String, Value<'static>)>, usize)| {
        list.push(Setting { key, title, about, icon, tint, kept: Kept::Config(section), choices, current });
    };

    let mut themes = vec![("system", "System")];
    themes.extend(PALETTES.iter().map(|p| (p.key, p.name)));
    add("theme", "Theme", "Follows light and dark, or one palette", "swatch", 7, None, texts(&themes, &config.theme));
    add("blur", "Blur", blur_about(), "blur", 7, Some("appearance"), on_off(a.blur));
    if let Some(styles) = blur_styles() {
        add("blur_style", "Blur type", styles.1, "window", 7, Some("appearance"), texts(styles.0, &a.blur_style));
    }
    let opacity =
        OPACITIES.iter().enumerate().min_by(|x, y| (x.1 - a.opacity).abs().total_cmp(&(y.1 - a.opacity).abs())).map_or(2, |(i, _)| i);
    add(
        "opacity",
        "Opacity",
        "How solid it is while blurred",
        "opacity",
        7,
        Some("appearance"),
        (OPACITIES.iter().map(|o| (format!("{}%", (o * 100.0).round()), Value::Number(*o))).collect(), opacity),
    );
    add(
        "glow",
        "Glow",
        "Colour behind the list",
        "sparkle",
        7,
        Some("appearance"),
        texts(&[("still", "Still"), ("drift", "Drifting"), ("off", "Off")], &a.glow),
    );
    add(
        "style",
        "Style",
        "Modern adds gradients; uses the GPU and more memory",
        "sparkle",
        7,
        Some("appearance"),
        texts(&[("flat", "Flat"), ("modern", "Modern")], &a.style),
    );
    add(
        "selection",
        "Selection",
        "The selected row's colour",
        "pointer",
        7,
        Some("appearance"),
        texts(&[("white", "White"), ("blue", "Blue")], &a.selection),
    );
    add(
        "icons",
        "Icons",
        "The squares behind them",
        "app",
        7,
        Some("appearance"),
        texts(&[("subtle", "Subtle"), ("bold", "Bold")], &a.icons),
    );
    add("gradient_divider", "Gradient line", "Under the search field", "gradient", 7, Some("appearance"), on_off(a.gradient_divider));
    add(
        "size",
        "Size",
        "Compact is narrower, with no bottom bar",
        "resize",
        7,
        Some("appearance"),
        texts(&[("regular", "Regular"), ("compact", "Compact")], &a.size),
    );
    add("details", "Details pane", "Beside the list, for what's selected", "columns", 7, Some("appearance"), on_off(a.details));
    add(
        "favourites",
        "Favourites",
        "What you use most, before you type",
        "star",
        7,
        Some("appearance"),
        texts(&[("grid", "Grid"), ("list", "List")], &a.favourites),
    );
    add("quick_keys", "Quick keys", "Ctrl 1 to 9 open the first nine", "number", 7, Some("appearance"), on_off(a.quick_keys));
    add("headings", "Headings", "Above each group in the list", "heading", 7, Some("appearance"), on_off(a.headings));
    add("hide_on_blur", "Close on click away", "When you click another window", "pointer", 7, None, on_off(config.hide_on_blur));
    add(
        "updates",
        "Updates",
        "New versions from GitHub",
        "download",
        7,
        None,
        texts(&[("auto", "Automatic"), ("notify", "Tell me"), ("off", "Off")], &config.updates),
    );
    add(
        "clipboard_history",
        "Clipboard history",
        "Keeps what you copy, on this computer only",
        "clipboard",
        7,
        None,
        on_off(config.clipboard_history),
    );
    add("site_icons", "Website icons", "Quicklinks show their site's icon, fetched once", "globe", 7, None, on_off(config.site_icons));
    add(
        "previews",
        "Preview versions",
        "Built every night from the newest changes",
        "flask",
        7,
        None,
        texts(&[("ask", "Ask"), ("always", "Always"), ("never", "Never")], &config.previews),
    );
    let (choices, current) = on_off(crate::autostart::enabled());
    list.push(Setting {
        key: "login",
        title: "Open at login",
        about: "Starts hidden, ready for your shortcut",
        icon: "login",
        tint: 7,
        kept: Kept::Login,
        choices,
        current,
    });
    list
}

/// What blur costs here, said where it's turned on.
fn blur_about() -> &'static str {
    if crate::window::needs_gpu_for_blur() {
        "See through, softened; uses the GPU and more memory"
    } else {
        "See through the window, softened"
    }
}

/// The kinds of see-through this system offers, and what to say about them. Linux has none to
/// pick: the compositor blurs, by its own settings.
fn blur_styles() -> Option<(&'static [(&'static str, &'static str)], &'static str)> {
    if cfg!(windows) {
        Some((
            &[("acrylic", "Acrylic"), ("mica", "Mica"), ("mica-alt", "Mica Alt"), ("blur", "Blur")],
            "Acrylic blurs what's behind; Mica tints by your wallpaper",
        ))
    } else if cfg!(target_os = "macos") {
        Some((
            &[("hud", "HUD"), ("popover", "Popover"), ("sidebar", "Sidebar"), ("menu", "Menu"), ("under-window", "Under window")],
            "Which of macOS's materials it's made of",
        ))
    } else {
        None
    }
}

/// The settings shown at each step of the first-run walkthrough, by key.
pub const LOOK: &[&str] = &["theme", "style", "selection", "icons", "blur", "blur_style", "opacity", "glow"];
pub const LAYOUT: &[&str] = &["size", "favourites", "details", "quick_keys", "headings"];

#[cfg(test)]
mod tests {
    #[test]
    fn every_setting_starts_on_its_value() {
        let config = crate::config::Config::default();
        let list = super::settings(&config);
        let label = |k: &str| list.iter().find(|s| s.key == k).unwrap().label().to_string();
        assert_eq!(label("theme"), "System");
        assert_eq!(label("blur"), if crate::window::needs_gpu_for_blur() { "Off" } else { "On" });
        assert_eq!(label("opacity"), "78%");
        assert_eq!(label("glow"), "Still");
        assert_eq!(label("favourites"), "Grid");
        assert_eq!(label("size"), "Regular");
    }
}
