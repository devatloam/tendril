//! Open at login: Flint starts hidden when you log in, each system's own way.
//!   Linux:   a .desktop file in ~/.config/autostart
//!   Windows: a value under HKCU\Software\Microsoft\Windows\CurrentVersion\Run
//!   macOS:   a launch agent in ~/Library/LaunchAgents

use std::io;

fn command() -> io::Result<String> {
    Ok(std::env::current_exe()?.to_string_lossy().into_owned())
}

#[cfg(target_os = "linux")]
fn file() -> Option<std::path::PathBuf> {
    Some(dirs::config_dir()?.join("autostart").join("flint.desktop"))
}

#[cfg(target_os = "linux")]
pub fn enabled() -> bool {
    file().is_some_and(|f| f.is_file())
}

#[cfg(target_os = "linux")]
pub fn set(on: bool) -> io::Result<()> {
    let file = file().ok_or_else(|| io::Error::other("no config folder"))?;
    if !on {
        return std::fs::remove_file(&file).or_else(|e| if e.kind() == io::ErrorKind::NotFound { Ok(()) } else { Err(e) });
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let exe = command()?.replace('"', "\\\"");
    std::fs::write(
        file,
        format!(
            "[Desktop Entry]\nType=Application\nName=Flint\nComment=A keyboard launcher\nExec=\"{exe}\" --hidden\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n"
        ),
    )
}

#[cfg(windows)]
mod registry {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW};
    use windows::core::w;

    const RUN: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    const NAME: windows::core::PCWSTR = w!("Flint");

    pub fn enabled() -> bool {
        unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN, NAME, RRF_RT_REG_SZ, None, None, None).is_ok() }
    }

    pub fn set(on: bool) -> std::io::Result<()> {
        let result = unsafe {
            if on {
                let value: Vec<u16> = format!("\"{}\" --hidden", super::command()?).encode_utf16().chain([0]).collect();
                RegSetKeyValueW(HKEY_CURRENT_USER, RUN, NAME, REG_SZ.0, Some(value.as_ptr().cast()), (value.len() * 2) as u32)
            } else {
                let result = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, NAME);
                // Already gone is fine.
                if result.0 == 2 { windows::Win32::Foundation::ERROR_SUCCESS } else { result }
            }
        };
        result.ok().map_err(|e| std::io::Error::other(e.to_string()))
    }
}

#[cfg(windows)]
pub use registry::{enabled, set};

#[cfg(target_os = "macos")]
fn file() -> Option<std::path::PathBuf> {
    Some(dirs::home_dir()?.join("Library/LaunchAgents/dev.flint.launcher.plist"))
}

#[cfg(target_os = "macos")]
pub fn enabled() -> bool {
    file().is_some_and(|f| f.is_file())
}

#[cfg(target_os = "macos")]
pub fn set(on: bool) -> io::Result<()> {
    let file = file().ok_or_else(|| io::Error::other("no home folder"))?;
    if !on {
        return std::fs::remove_file(&file).or_else(|e| if e.kind() == io::ErrorKind::NotFound { Ok(()) } else { Err(e) });
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let exe = command()?.replace('&', "&amp;").replace('<', "&lt;");
    std::fs::write(
        file,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.flint.launcher</string>
  <key>ProgramArguments</key><array><string>{exe}</string><string>--hidden</string></array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#
        ),
    )
}
