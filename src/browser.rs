//! Which browser links open in, so web rows can wear its icon. Links themselves open through the
//! system (system::open), which already picks this browser; this only finds out which it is.

use crate::catalog::{Icon, Item, Kind};

/// The default browser's icon, from the apps Tendril found, or None if it can't tell.
pub fn icon(catalog: &[Item]) -> Option<Icon> {
    imp::icon(catalog)
}

#[cfg_attr(windows, allow(dead_code))]
fn app_with_id<'a>(catalog: &'a [Item], id: &str) -> Option<&'a Item> {
    catalog.iter().find(|i| i.kind == Kind::Application && i.id.eq_ignore_ascii_case(id))
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::path::PathBuf;

    /// mimeapps.list, wherever the spec says to look, most personal first.
    fn lists() -> Vec<PathBuf> {
        let mut out = Vec::new();
        let config = dirs::config_dir();
        if let Some(c) = &config {
            out.push(c.join("mimeapps.list"));
        }
        let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".into());
        out.extend(dirs.split(':').filter(|d| !d.is_empty()).map(|d| PathBuf::from(d).join("mimeapps.list")));
        if let Some(d) = dirs::data_dir() {
            out.push(d.join("applications/mimeapps.list"));
        }
        let data = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        for d in data.split(':').filter(|d| !d.is_empty()) {
            out.push(PathBuf::from(d).join("applications/mimeapps.list"));
            out.push(PathBuf::from(d).join("applications/defaults.list"));
        }
        out
    }

    /// The .desktop file named for https (or http) links in a mimeapps.list.
    pub(super) fn handler(text: &str) -> Option<String> {
        let mut defaults = false;
        let mut http = None;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                defaults = line == "[Default Applications]";
                continue;
            }
            let Some((key, value)) = line.split_once('=').filter(|_| defaults) else { continue };
            let first = value.split(';').map(str::trim).find(|v| !v.is_empty()).map(str::to_string);
            match key.trim() {
                "x-scheme-handler/https" => return first,
                "x-scheme-handler/http" => http = http.or(first),
                _ => {}
            }
        }
        http
    }

    pub fn icon(catalog: &[Item]) -> Option<Icon> {
        let desktop = lists().iter().filter_map(|p| std::fs::read_to_string(p).ok()).find_map(|t| handler(&t))?;
        let app = app_with_id(catalog, &format!("app:{}", desktop.trim_end_matches(".desktop")))?;
        Some(app.icon.clone())
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Win32::System::Registry::{HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::{HSTRING, PCWSTR};

    fn read(root: HKEY, key: &str, value: Option<&str>) -> Option<String> {
        let key = HSTRING::from(key);
        let value = value.map(HSTRING::from);
        let name = value.as_ref().map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr()));
        let mut buffer = vec![0u16; 1024];
        let mut size = (buffer.len() * 2) as u32;
        unsafe { RegGetValueW(root, &key, name, RRF_RT_REG_SZ, None, Some(buffer.as_mut_ptr().cast()), Some(&mut size)) }.ok().ok()?;
        let length = (size as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buffer[..length.min(buffer.len())]))
    }

    /// The program in a command line like `"C:\…\chrome.exe" --single-argument %1`.
    pub(super) fn program(command: &str) -> Option<String> {
        let command = command.trim();
        let exe = match command.strip_prefix('"') {
            Some(rest) => rest.split('"').next()?,
            None => command.split(".exe").next().map(|p| p.trim())?,
        };
        let exe = if exe.to_lowercase().ends_with(".exe") { exe.to_string() } else { format!("{exe}.exe") };
        Some(exe)
    }

    pub fn icon(catalog: &[Item]) -> Option<Icon> {
        let choice = r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice";
        let prog_id = read(HKEY_CURRENT_USER, choice, Some("ProgId"))?;
        let command = read(HKEY_CLASSES_ROOT, &format!(r"{prog_id}\shell\open\command"), None)?;
        let exe = program(&command)?;
        // The Start menu's entry when it has one, so it looks the same as in the list.
        let listed = catalog
            .iter()
            .find(|i| i.kind == Kind::Application && i.path.as_ref().is_some_and(|p| p.to_string_lossy().eq_ignore_ascii_case(&exe)));
        Some(listed.map_or_else(|| Icon::Shell(exe.clone()), |i| i.icon.clone()))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;

    pub fn icon(catalog: &[Item]) -> Option<Icon> {
        let prefs = dirs::home_dir()?.join("Library/Preferences/com.apple.LaunchServices/com.apple.launchservices.secure.plist");
        let bundle = plist::Value::from_file(prefs)
            .ok()
            .and_then(|v| {
                v.as_dictionary()?.get("LSHandlers")?.as_array()?.iter().find_map(|h| {
                    let h = h.as_dictionary()?;
                    (h.get("LSHandlerURLScheme")?.as_string()? == "https")
                        .then(|| h.get("LSHandlerRoleAll")?.as_string().map(str::to_string))?
                })
            })
            // Nothing chosen means Safari.
            .unwrap_or_else(|| "com.apple.Safari".into());
        Some(app_with_id(catalog, &format!("app:{bundle}"))?.icon.clone())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    #[test]
    fn reads_mimeapps() {
        let text = "[Added Associations]\nx-scheme-handler/https=chromium.desktop;\n\n[Default Applications]\ntext/html=a.desktop\nx-scheme-handler/http=b.desktop;\nx-scheme-handler/https=firefox.desktop;other.desktop;\n";
        assert_eq!(super::imp::handler(text).as_deref(), Some("firefox.desktop"));
        assert_eq!(super::imp::handler("[Default Applications]\nx-scheme-handler/http=b.desktop\n").as_deref(), Some("b.desktop"));
        assert_eq!(super::imp::handler("[Added Associations]\nx-scheme-handler/https=c.desktop\n"), None);
    }
}
