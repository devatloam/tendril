//! An icon in the Windows notification area, as Flow Launcher and PowerToys have: click it to
//! open or close Tendril, right-click it for Appearance and Quit. It's the way in when another
//! app holds your shortcut. Its own thread sleeps in a message loop until Windows has something
//! for it. Not on Linux or macOS, where a tray takes far more machinery for less.

#[cfg(windows)]
pub fn start() {
    imp::start();
}

#[cfg(not(windows))]
pub fn start() {}

#[cfg(windows)]
mod imp {
    use crate::app::with_app;
    use crate::window;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW};
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, HICON,
        HWND_MESSAGE, IDI_APPLICATION, LoadIconW, MF_SEPARATOR, MF_STRING, MSG, RegisterClassW, SetForegroundWindow, TPM_RETURNCMD,
        TPM_RIGHTBUTTON, TrackPopupMenu, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
    };
    use windows::core::{PCWSTR, w};

    const CALLBACK: u32 = WM_APP + 1;
    const OPEN: usize = 1;
    const APPEARANCE: usize = 2;
    const QUIT: usize = 3;
    const SETTINGS: usize = 4;

    pub fn start() {
        let _ = std::thread::Builder::new().name("tray".into()).stack_size(256 * 1024).spawn(|| unsafe {
            let class = WNDCLASSW { lpfnWndProc: Some(window_proc), lpszClassName: w!("TendrilTray"), ..Default::default() };
            RegisterClassW(&class);
            let Ok(window) = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("TendrilTray"),
                PCWSTR::null(),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            ) else {
                return;
            };
            let mut data = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: window,
                uID: 1,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: CALLBACK,
                hIcon: icon(),
                ..Default::default()
            };
            let tip: Vec<u16> = "Tendril".encode_utf16().collect();
            data.szTip[..tip.len()].copy_from_slice(&tip);
            let added = Shell_NotifyIconW(NIM_ADD, &data).as_bool();
            eprintln!("tendril: notification area icon {}", if added { "added" } else { "refused" });
            if !added {
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
        });
    }

    /// Tendril's own icon, from the .exe (build.rs puts it there as resource 1).
    unsafe fn icon() -> HICON {
        unsafe {
            let module = GetModuleHandleW(None).unwrap_or_default();
            LoadIconW(Some(module.into()), PCWSTR(std::ptr::without_provenance(1)))
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))
                .unwrap_or_default()
        }
    }

    unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if message == CALLBACK {
            match lparam.0 as u32 {
                WM_LBUTTONUP => on_ui(OPEN),
                WM_RBUTTONUP => {
                    let chosen = unsafe { menu(window) };
                    if chosen != 0 {
                        on_ui(chosen);
                    }
                }
                _ => {}
            }
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    unsafe fn menu(window: HWND) -> usize {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return 0 };
            let _ = AppendMenuW(menu, MF_STRING, OPEN, w!("Open Tendril"));
            let _ = AppendMenuW(menu, MF_STRING, SETTINGS, w!("Settings"));
            let _ = AppendMenuW(menu, MF_STRING, APPEARANCE, w!("Appearance"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, QUIT, w!("Quit Tendril"));
            let mut at = POINT::default();
            let _ = GetCursorPos(&mut at);
            // Without this the menu doesn't close when you click elsewhere.
            let _ = SetForegroundWindow(window);
            let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.x, at.y, None, window, None);
            let _ = DestroyMenu(menu);
            chosen.0 as usize
        }
    }

    fn on_ui(command: usize) {
        let _ = slint::invoke_from_event_loop(move || {
            with_app(|app| {
                let Some(ui) = app.ui.upgrade() else { return };
                match command {
                    OPEN => window::toggle(&ui),
                    APPEARANCE => {
                        window::show(&ui);
                        app.open_appearance();
                    }
                    SETTINGS => crate::settings::open(0),
                    QUIT => {
                        let _ = slint::quit_event_loop();
                    }
                    _ => {}
                }
            })
        });
    }
}
