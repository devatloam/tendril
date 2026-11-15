//! Asking the operating system to do things: power, opening links and folders.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Power {
    Lock,
    Sleep,
    LogOut,
    Restart,
    ShutDown,
}

impl Power {
    /// The question asked first, for the ones that close your apps.
    pub fn confirmation(self) -> Option<(&'static str, &'static str, &'static str)> {
        match self {
            Power::LogOut => Some(("Log out?", "Open apps will be closed.", "Log Out")),
            Power::Restart => Some(("Restart?", "Open apps will be closed.", "Restart")),
            Power::ShutDown => Some(("Shut down?", "Open apps will be closed.", "Shut Down")),
            Power::Lock | Power::Sleep => None,
        }
    }
}

/// Starts a program without waiting for it, and without a console window on Windows.
pub fn spawn(program: &str, args: &[&str]) -> io::Result<()> {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    detach(&mut command);
    reap(command.spawn()?);
    Ok(())
}

/// Lets a launched app outlive Tendril and its keyboard shortcuts.
pub fn detach(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }
}

/// Waits for a child on a small thread so it doesn't linger as a zombie once it exits.
pub fn reap(mut child: std::process::Child) {
    #[cfg(unix)]
    let _ = std::thread::Builder::new().stack_size(32 * 1024).spawn(move || {
        let _ = child.wait();
    });
    #[cfg(not(unix))]
    let _ = &mut child;
}

pub fn power(action: Power) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
        match action {
            Power::Lock => spawn("loginctl", &["lock-session"]),
            Power::Sleep => spawn("systemctl", &["suspend"]),
            Power::LogOut if !session.is_empty() => spawn("loginctl", &["terminate-session", &session]),
            Power::LogOut => spawn("loginctl", &["terminate-user", &whoami()]),
            Power::Restart => spawn("systemctl", &["reboot"]),
            Power::ShutDown => spawn("systemctl", &["poweroff"]),
        }
    }
    #[cfg(windows)]
    {
        match action {
            Power::Lock => spawn("rundll32.exe", &["user32.dll,LockWorkStation"]),
            Power::Sleep => crate::apps::sleep(),
            Power::LogOut => spawn("shutdown.exe", &["/l"]),
            Power::Restart => spawn("shutdown.exe", &["/r", "/t", "0"]),
            Power::ShutDown => spawn("shutdown.exe", &["/s", "/t", "0"]),
        }
    }
    #[cfg(target_os = "macos")]
    {
        let script = |s: &str| spawn("osascript", &["-e", s]);
        match action {
            Power::Lock => spawn("pmset", &["displaysleepnow"]),
            Power::Sleep => spawn("pmset", &["sleepnow"]),
            Power::LogOut => script("tell application \"System Events\" to log out"),
            Power::Restart => script("tell application \"System Events\" to restart"),
            Power::ShutDown => script("tell application \"System Events\" to shut down"),
        }
    }
}

#[cfg(target_os = "linux")]
fn whoami() -> String {
    std::env::var("USER").unwrap_or_default()
}

/// Opens a link or file with whatever the system uses for it.
pub fn open(target: &str) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    return spawn("xdg-open", &[target]);
    #[cfg(target_os = "macos")]
    return spawn("open", &[target]);
    #[cfg(windows)]
    return crate::apps::shell_open(target);
}

/// Shows a file in the file manager, selected where the platform can.
pub fn reveal(path: &Path) -> io::Result<()> {
    let text = path.to_string_lossy();
    #[cfg(target_os = "macos")]
    return spawn("open", &["-R", &text]);
    #[cfg(windows)]
    return spawn("explorer.exe", &[&format!("/select,{text}")]);
    #[cfg(target_os = "linux")]
    {
        // The FileManager1 interface selects the file in Nautilus, Dolphin, Nemo and others.
        let uri = format!("file://{text}");
        let asked = Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:{uri}"),
                "string:",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        match asked {
            Ok(s) if s.success() => Ok(()),
            _ => open(&path.parent().unwrap_or(path).to_string_lossy()),
        }
    }
}

thread_local! {
    // On X11 and Wayland what's copied lives in the app that copied it, so the clipboard is kept
    // for as long as Tendril runs. It's made the first time something is copied, not at start.
    static CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> = const { std::cell::RefCell::new(None) };
}

pub fn copy(text: &str) -> Result<(), String> {
    CLIPBOARD.with_borrow_mut(|slot| {
        if slot.is_none() {
            *slot = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
        }
        let clipboard = slot.as_mut().expect("made above");
        clipboard.set_text(text.to_string()).map_err(|e| e.to_string())
    })
}
