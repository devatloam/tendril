//! Switching windows: every open window, most recently used first. Listed when the view opens
//! and not kept, so it costs nothing otherwise. X11 and Windows; macOS only lets an app see
//! other apps' windows with the Screen Recording permission, and Wayland doesn't at all.

#[derive(Clone, Debug)]
pub struct Win {
    pub id: u64,
    pub title: String,
    /// The app it belongs to, as the system names it: WM_CLASS on X11, the .exe's name on Windows.
    pub app: String,
    /// The program behind it, where known (Windows), for its icon.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub program: Option<std::path::PathBuf>,
}

pub fn available() -> bool {
    imp::available()
}

pub fn list() -> Vec<Win> {
    imp::list().unwrap_or_default()
}

pub fn focus(id: u64) -> Result<(), String> {
    imp::focus(id)
}

pub fn close(id: u64) -> Result<(), String> {
    imp::close(id)
}

#[cfg(target_os = "linux")]
mod imp {
    use super::Win;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window};
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    type Error = Box<dyn std::error::Error>;

    pub fn available() -> bool {
        std::env::var_os("DISPLAY").is_some() && std::env::var("XDG_SESSION_TYPE").map_or(true, |t| t != "wayland")
    }

    fn atom(conn: &RustConnection, name: &str) -> Result<u32, Error> {
        Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
    }

    fn words(conn: &RustConnection, window: Window, property: u32, kind: impl Into<u32>) -> Result<Vec<u32>, Error> {
        let reply = conn.get_property(false, window, property, kind, 0, 4096)?.reply()?;
        Ok(reply.value32().map(Iterator::collect).unwrap_or_default())
    }

    fn text(conn: &RustConnection, window: Window, property: u32, kind: impl Into<u32>) -> Result<String, Error> {
        let reply = conn.get_property(false, window, property, kind, 0, 1024)?.reply()?;
        Ok(String::from_utf8_lossy(&reply.value).into_owned())
    }

    pub fn list() -> Result<Vec<Win>, Error> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let utf8 = atom(&conn, "UTF8_STRING")?;
        let (name, kind, state) = (atom(&conn, "_NET_WM_NAME")?, atom(&conn, "_NET_WM_WINDOW_TYPE")?, atom(&conn, "_NET_WM_STATE")?);
        let normal = atom(&conn, "_NET_WM_WINDOW_TYPE_NORMAL")?;
        let skip = atom(&conn, "_NET_WM_STATE_SKIP_TASKBAR")?;
        // Bottom to top, so reversed it's the most recently used first.
        let mut windows = words(&conn, root, atom(&conn, "_NET_CLIENT_LIST_STACKING")?, AtomEnum::WINDOW)?;
        if windows.is_empty() {
            windows = words(&conn, root, atom(&conn, "_NET_CLIENT_LIST")?, AtomEnum::WINDOW)?;
        }
        let mut found = Vec::new();
        for &window in windows.iter().rev() {
            let types = words(&conn, window, kind, AtomEnum::ATOM).unwrap_or_default();
            if !types.is_empty() && !types.contains(&normal) {
                continue;
            }
            if words(&conn, window, state, AtomEnum::ATOM).unwrap_or_default().contains(&skip) {
                continue;
            }
            let class = text(&conn, window, AtomEnum::WM_CLASS.into(), AtomEnum::STRING).unwrap_or_default();
            // "instance\0Class\0": the class is the app's proper name.
            let app = class.split('\0').rfind(|s| !s.is_empty()).unwrap_or("").to_string();
            if app.eq_ignore_ascii_case("tendril") {
                continue;
            }
            let mut title = text(&conn, window, name, utf8).unwrap_or_default();
            if title.is_empty() {
                title = text(&conn, window, AtomEnum::WM_NAME.into(), AtomEnum::STRING).unwrap_or_default();
            }
            if title.is_empty() && app.is_empty() {
                continue;
            }
            found.push(Win { id: u64::from(window), title, app, program: None });
        }
        Ok(found)
    }

    /// Asks the window manager, which decides; source 2 says a pager-like tool asked for it.
    fn message(id: u64, kind: &str, data: [u32; 5]) -> Result<(), String> {
        let run = || -> Result<(), Error> {
            let (conn, screen) = x11rb::connect(None)?;
            let root = conn.setup().roots[screen].root;
            let event = ClientMessageEvent::new(32, u32::try_from(id)?, atom(&conn, kind)?, data);
            conn.send_event(false, root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event)?;
            conn.sync()?;
            Ok(())
        };
        run().map_err(|e| e.to_string())
    }

    pub fn focus(id: u64) -> Result<(), String> {
        message(id, "_NET_ACTIVE_WINDOW", [2, x11rb::CURRENT_TIME, 0, 0, 0])
    }

    pub fn close(id: u64) -> Result<(), String> {
        message(id, "_NET_CLOSE_WINDOW", [x11rb::CURRENT_TIME, 2, 0, 0, 0])
    }
}

#[cfg(windows)]
mod imp {
    use super::Win;
    use std::path::PathBuf;
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsIconic, IsWindowVisible, PostMessageW, SW_RESTORE, SetForegroundWindow, ShowWindow, WM_CLOSE, WS_EX_TOOLWINDOW,
    };
    use windows::core::{BOOL, PWSTR};

    pub fn available() -> bool {
        true
    }

    pub fn list() -> Result<Vec<Win>, String> {
        let mut handles: Vec<HWND> = Vec::new();
        unsafe extern "system" fn each(window: HWND, found: LPARAM) -> BOOL {
            unsafe { (*(found.0 as *mut Vec<HWND>)).push(window) };
            BOOL(1)
        }
        // Top of the stack first, which is the most recently used.
        unsafe { EnumWindows(Some(each), LPARAM(&mut handles as *mut Vec<HWND> as isize)) }.map_err(|e| e.to_string())?;
        let ours = std::process::id();
        Ok(handles.into_iter().filter_map(|w| unsafe { describe(w, ours) }).collect())
    }

    /// A window as the taskbar would show it, or None for the many it wouldn't.
    unsafe fn describe(window: HWND, ours: u32) -> Option<Win> {
        unsafe {
            if !IsWindowVisible(window).as_bool() || GetWindow(window, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
                return None;
            }
            if GetWindowLongPtrW(window, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
                return None;
            }
            // Windows on other virtual desktops, and some suspended apps, are "cloaked".
            let mut cloaked = 0u32;
            let size = std::mem::size_of::<u32>() as u32;
            if DwmGetWindowAttribute(window, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, size).is_ok() && cloaked != 0 {
                return None;
            }
            let length = GetWindowTextLengthW(window);
            if length == 0 {
                return None;
            }
            let mut buffer = vec![0u16; length as usize + 1];
            let copied = GetWindowTextW(window, &mut buffer);
            let title = String::from_utf16_lossy(&buffer[..copied as usize]);
            let mut pid = 0u32;
            GetWindowThreadProcessId(window, Some(&mut pid));
            if pid == ours {
                return None;
            }
            let program = program(pid);
            let app = program.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            Some(Win { id: window.0 as usize as u64, title, app, program })
        }
    }

    unsafe fn program(pid: u32) -> Option<PathBuf> {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buffer = [0u16; 1024];
            let mut length = buffer.len() as u32;
            let named = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut length);
            let _ = CloseHandle(process);
            named.ok()?;
            Some(PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize])))
        }
    }

    fn handle(id: u64) -> HWND {
        HWND(id as usize as *mut _)
    }

    pub fn focus(id: u64) -> Result<(), String> {
        let window = handle(id);
        unsafe {
            if IsIconic(window).as_bool() {
                let _ = ShowWindow(window, SW_RESTORE);
            }
            // Allowed because Tendril is in front when you choose.
            if SetForegroundWindow(window).as_bool() { Ok(()) } else { Err("Windows didn't bring it forward".into()) }
        }
    }

    pub fn close(id: u64) -> Result<(), String> {
        unsafe { PostMessageW(Some(handle(id)), WM_CLOSE, WPARAM(0), LPARAM(0)) }.map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Win;

    pub fn available() -> bool {
        false
    }

    pub fn list() -> Result<Vec<Win>, String> {
        Ok(Vec::new())
    }

    pub fn focus(_id: u64) -> Result<(), String> {
        Err("switching windows isn't on macOS yet".into())
    }

    pub fn close(_id: u64) -> Result<(), String> {
        Err("switching windows isn't on macOS yet".into())
    }
}
