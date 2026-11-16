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

/// Things the system does that Tendril can ask for, beside power.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sys {
    EmptyBin,
    DarkMode,
    Mute,
    VolumeUp,
    VolumeDown,
    ShowDesktop,
    Downloads,
}

impl Sys {
    pub const ALL: [Sys; 7] = [Sys::EmptyBin, Sys::DarkMode, Sys::Mute, Sys::VolumeUp, Sys::VolumeDown, Sys::ShowDesktop, Sys::Downloads];

    /// Whether this system has a way to do it.
    pub fn available(self) -> bool {
        match self {
            // Windows has a shell command for it; elsewhere it depends on the window manager.
            Sys::ShowDesktop => cfg!(windows),
            _ => true,
        }
    }

    /// (id, title, words it's found by, icon).
    pub fn describe(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Sys::EmptyBin if cfg!(windows) => ("empty-bin", "Empty Recycle Bin", "trash delete clean bin", "trash"),
            Sys::EmptyBin => ("empty-bin", "Empty Trash", "recycle bin delete clean", "trash"),
            Sys::DarkMode => ("dark-mode", "Toggle Dark Mode", "light mode theme night appearance colors", "contrast"),
            Sys::Mute => ("mute", "Mute or Unmute", "sound volume audio silence", "volume"),
            Sys::VolumeUp => ("volume-up", "Volume Up", "sound louder audio", "volume"),
            Sys::VolumeDown => ("volume-down", "Volume Down", "sound quieter softer audio", "volume"),
            Sys::ShowDesktop => ("show-desktop", "Show Desktop", "hide windows minimize all", "window"),
            Sys::Downloads => ("downloads", "Open Downloads", "downloads folder files", "download"),
        }
    }

    /// The question asked first, for what can't be undone.
    pub fn confirmation(self) -> Option<(&'static str, &'static str, &'static str)> {
        match self {
            Sys::EmptyBin if cfg!(windows) => Some(("Empty the Recycle Bin?", "What's in it is deleted for good.", "Empty")),
            Sys::EmptyBin => Some(("Empty the Trash?", "What's in it is deleted for good.", "Empty")),
            _ => None,
        }
    }

    /// Volume steps keep Tendril open, to press Enter again.
    pub fn stays_open(self) -> bool {
        matches!(self, Sys::VolumeUp | Sys::VolumeDown | Sys::Mute)
    }
}

/// Does it. What's said afterwards, if anything.
pub fn system(what: Sys) -> io::Result<&'static str> {
    if what == Sys::Downloads {
        let dir = dirs::download_dir().ok_or_else(|| io::Error::other("there's no Downloads folder"))?;
        open(&dir.to_string_lossy())?;
        return Ok("");
    }
    imp::system(what)
}

#[cfg(windows)]
mod imp {
    use super::Sys;
    use std::io;

    pub fn system(what: Sys) -> io::Result<&'static str> {
        match what {
            Sys::EmptyBin => {
                use windows::Win32::UI::Shell::{SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW};
                // An empty bin is "nothing to do", not a failure.
                let _ = unsafe {
                    SHEmptyRecycleBinW(None, windows::core::PCWSTR::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND)
                };
                Ok("Recycle Bin emptied")
            }
            Sys::DarkMode => dark_mode(),
            Sys::Mute => keys(windows::Win32::UI::Input::KeyboardAndMouse::VK_VOLUME_MUTE, 1).map(|_| ""),
            // Each press is two steps of Windows' 100.
            Sys::VolumeUp => keys(windows::Win32::UI::Input::KeyboardAndMouse::VK_VOLUME_UP, 5).map(|_| ""),
            Sys::VolumeDown => keys(windows::Win32::UI::Input::KeyboardAndMouse::VK_VOLUME_DOWN, 5).map(|_| ""),
            Sys::ShowDesktop => super::spawn("explorer.exe", &["shell:::{3080F90D-D7AD-11D9-BD98-0000947B0257}"]).map(|_| ""),
            Sys::Downloads => Ok(""),
        }
    }

    /// The volume keys, as if pressed: Windows changes the volume and shows its own slider.
    fn keys(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY, times: usize) -> io::Result<()> {
        use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput};
        let key = |up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() }, ..Default::default() },
            },
        };
        let inputs: Vec<INPUT> = (0..times).flat_map(|_| [key(false), key(true)]).collect();
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize == inputs.len() { Ok(()) } else { Err(io::Error::other("Windows didn't take the key presses")) }
    }

    /// Flips apps and Windows itself between light and dark, as Settings → Colors does, and tells
    /// open apps so they follow.
    fn dark_mode() -> io::Result<&'static str> {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD, RegGetValueW, RegSetKeyValueW};
        use windows::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE};
        use windows::core::w;
        let key = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        let mut light = 1u32;
        let mut size = 4u32;
        unsafe {
            let _ = RegGetValueW(
                HKEY_CURRENT_USER,
                key,
                w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut light as *mut u32 as *mut _),
                Some(&mut size),
            );
        }
        let next: u32 = if light == 0 { 1 } else { 0 };
        for name in [w!("AppsUseLightTheme"), w!("SystemUsesLightTheme")] {
            let set = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key, name, REG_DWORD.0, Some(&next as *const u32 as *const _), 4) };
            if set.is_err() {
                return Err(io::Error::other("Windows didn't take the change"));
            }
        }
        unsafe {
            let _ = SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(w!("ImmersiveColorSet").as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                200,
                None,
            );
        }
        Ok(if next == 0 { "Dark mode on" } else { "Light mode on" })
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Sys;
    use std::io;
    use std::process::{Command, Stdio};

    fn script(s: &str) -> io::Result<()> {
        let status = Command::new("osascript").args(["-e", s]).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
        if status.success() { Ok(()) } else { Err(io::Error::other("macOS said no")) }
    }

    pub fn system(what: Sys) -> io::Result<&'static str> {
        match what {
            Sys::EmptyBin => script("tell application \"Finder\" to empty trash").map(|_| "Trash emptied"),
            Sys::DarkMode => {
                script("tell application \"System Events\" to tell appearance preferences to set dark mode to not dark mode").map(|_| "")
            }
            Sys::Mute => script("set volume output muted not (output muted of (get volume settings))").map(|_| ""),
            Sys::VolumeUp => script("set volume output volume ((output volume of (get volume settings)) + 10)").map(|_| "Volume up"),
            Sys::VolumeDown => script("set volume output volume ((output volume of (get volume settings)) - 10)").map(|_| "Volume down"),
            Sys::ShowDesktop | Sys::Downloads => Ok(""),
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::Sys;
    use std::io;
    use std::process::{Command, Stdio};

    fn run(program: &str, args: &[&str]) -> io::Result<()> {
        let status = Command::new(program).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
        if status.success() { Ok(()) } else { Err(io::Error::other(format!("{program} said no"))) }
    }

    /// PipeWire's wpctl, else PulseAudio's pactl.
    fn volume(wpctl: &[&str], pactl: &[&str]) -> io::Result<()> {
        run("wpctl", wpctl).or_else(|_| run("pactl", pactl))
    }

    pub fn system(what: Sys) -> io::Result<&'static str> {
        match what {
            Sys::EmptyBin => run("gio", &["trash", "--empty"]).map(|_| "Trash emptied"),
            Sys::DarkMode => {
                // GNOME's setting, which other desktops and apps read too.
                let out = Command::new("gsettings").args(["get", "org.gnome.desktop.interface", "color-scheme"]).output()?;
                let dark = String::from_utf8_lossy(&out.stdout).contains("dark");
                let next = if dark { "default" } else { "prefer-dark" };
                run("gsettings", &["set", "org.gnome.desktop.interface", "color-scheme", next])?;
                Ok(if dark { "Light mode on" } else { "Dark mode on" })
            }
            Sys::Mute => {
                volume(&["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"], &["set-sink-mute", "@DEFAULT_SINK@", "toggle"]).map(|_| "")
            }
            Sys::VolumeUp => {
                volume(&["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", "10%+"], &["set-sink-volume", "@DEFAULT_SINK@", "+10%"])
                    .map(|_| "Volume up")
            }
            Sys::VolumeDown => volume(&["set-volume", "@DEFAULT_AUDIO_SINK@", "10%-"], &["set-sink-volume", "@DEFAULT_SINK@", "-10%"])
                .map(|_| "Volume down"),
            Sys::ShowDesktop | Sys::Downloads => Ok(""),
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

/// Windows' "Open with" dialog for a file. Elsewhere there's no system dialog to ask for.
pub fn can_open_with() -> bool {
    cfg!(windows)
}

pub fn open_with(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    return spawn("rundll32.exe", &["shell32.dll,OpenAs_RunDLL", &path.to_string_lossy()]);
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(io::Error::other("there's no Open With dialog here"))
    }
}

/// Asks for a folder with the system's own picker, and waits for the answer: None if cancelled
/// or there's no picker. Run it off the UI thread. `owner` is the window it belongs to, on Windows.
pub fn pick_folder(owner: isize) -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        };
        use windows::Win32::UI::Shell::{FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
            let options = dialog.GetOptions().ok()?;
            dialog.SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
            let owner = (owner != 0).then_some(HWND(owner as *mut _));
            dialog.Show(owner).ok()?;
            let name = dialog.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let path = name.to_string().ok();
            CoTaskMemFree(Some(name.0 as *const _));
            path.map(Into::into)
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = owner;
        let out = Command::new("osascript").args(["-e", "POSIX path of (choose folder)"]).output().ok()?;
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && !path.is_empty()).then(|| path.into())
    }
    #[cfg(target_os = "linux")]
    {
        let _ = owner;
        // GNOME's picker, else KDE's.
        let home = dirs::home_dir().unwrap_or_default();
        let tries: [(&str, Vec<&std::ffi::OsStr>); 2] = [
            ("zenity", vec!["--file-selection".as_ref(), "--directory".as_ref()]),
            ("kdialog", vec!["--getexistingdirectory".as_ref(), home.as_os_str()]),
        ];
        for (program, args) in tries {
            if let Ok(out) = Command::new(program).args(args).stderr(Stdio::null()).output() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                return (out.status.success() && !path.is_empty()).then(|| path.into());
            }
        }
        None
    }
}

/// What the bin is called here.
pub fn trash_name() -> &'static str {
    if cfg!(windows) { "Recycle Bin" } else { "Trash" }
}

/// Moves a file or folder to the Recycle Bin or Trash, where it can be got back from.
pub fn trash(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::UI::Shell::{FO_DELETE, FOF_ALLOWUNDO, FOF_NO_UI, SHFILEOPSTRUCTW, SHFileOperationW};
        use windows::core::PCWSTR;
        // The path list ends with two zeros.
        let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
        from.extend([0, 0]);
        let mut op = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: PCWSTR(from.as_ptr()),
            fFlags: (FOF_ALLOWUNDO | FOF_NO_UI).0 as u16,
            ..Default::default()
        };
        let result = unsafe { SHFileOperationW(&mut op) };
        if result != 0 || op.fAnyOperationsAborted.as_bool() {
            return Err(io::Error::other(format!("Windows said no (code {result})")));
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!("tell application \"Finder\" to delete POSIX file \"{}\"", path.to_string_lossy().replace('"', "\\\""));
        let status = Command::new("osascript").args(["-e", &script]).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
        if status.success() { Ok(()) } else { Err(io::Error::other("Finder said no")) }
    }
    #[cfg(target_os = "linux")]
    {
        let status = Command::new("gio").arg("trash").arg(path).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
        if status.success() { Ok(()) } else { Err(io::Error::other("gio trash said no")) }
    }
}

thread_local! {
    // On X11 and Wayland what's copied lives in the app that copied it, so the clipboard is kept
    // for as long as Tendril runs. It's made the first time something is copied, not at start.
    static CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> = const { std::cell::RefCell::new(None) };
}

/// Puts a file itself on the clipboard, to paste into a folder or a chat.
pub fn copy_file(path: &Path) -> Result<(), String> {
    CLIPBOARD.with_borrow_mut(|slot| {
        if slot.is_none() {
            *slot = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
        }
        let clipboard = slot.as_mut().expect("made above");
        clipboard.set().file_list(&[path]).map_err(|e| e.to_string())
    })
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
