//! Opening, placing and closing the window. Closing destroys it (see main.rs), so between uses
//! Tendril holds no window and no pixels, only the list it searches.

use crate::Launcher;
use crate::app::with_app;
use slint::ComponentHandle;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use winit::dpi::PhysicalPosition;
use winit::window::{WindowAttributes, WindowLevel};

thread_local! {
    /// Where the window went last time, so the next one is made there instead of jumping.
    static PLACE: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
    /// True while a window that isn't the launcher is being made (Settings).
    static PLAIN: Cell<bool> = const { Cell::new(false) };
}

/// Makes a window that behaves like any other app's: in the taskbar, not kept on top, a normal
/// window to tiling window managers. Slint asks `attributes` for every window it makes, and
/// the window's title isn't set yet when it does, so this is how the two are told apart.
pub fn plain<T>(make: impl FnOnce() -> T) -> T {
    PLAIN.set(true);
    let made = make();
    PLAIN.set(false);
    made
}

/// How the Settings window is framed on this system, as (frameless, corner Slint draws, room
/// left at the top for the system's buttons).
///   Windows: Tendril's own frame; Windows 11 rounds it and gives it a shadow.
///   macOS:   the system's frame with the title bar see-through, buttons inset, as Raycast's.
///   Linux:   Tendril's own rounded frame where the window can be see-through, the window
///            manager's frame where it can't (square corners would look worse than either).
pub fn settings_frame() -> (bool, f32, f32) {
    if cfg!(windows) {
        (true, 0.0, 0.0)
    } else if cfg!(target_os = "macos") {
        (false, 0.0, 28.0)
    } else if can_see_through() {
        (true, 12.0, 0.0)
    } else {
        (false, 0.0, 0.0)
    }
}

fn plain_attributes(attrs: WindowAttributes) -> WindowAttributes {
    let attrs = attrs.with_resizable(false);
    #[cfg(target_os = "linux")]
    let attrs = {
        use winit::platform::wayland::WindowAttributesExtWayland;
        use winit::platform::x11::WindowAttributesExtX11;
        let attrs = WindowAttributesExtX11::with_name(attrs, "tendril", "Tendril");
        WindowAttributesExtWayland::with_name(attrs, "tendril", "Tendril")
    };
    #[cfg(windows)]
    let attrs = {
        use winit::platform::windows::WindowAttributesExtWindows;
        // Without its frame a window has no shadow on Windows unless asked.
        attrs.with_undecorated_shadow(true)
    };
    #[cfg(target_os = "macos")]
    let attrs = {
        use winit::platform::macos::WindowAttributesExtMacOS;
        attrs.with_titlebar_transparent(true).with_title_hidden(true).with_fullsize_content_view(true)
    };
    attrs
}

/// Asks Windows 11 to round a window's corners (Windows 10 keeps them square).
#[allow(unused_variables)]
pub fn round(w: &winit::window::Window) {
    #[cfg(windows)]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = w.window_handle()
            && let RawWindowHandle::Win32(h) = handle.as_raw()
        {
            crate::apps::round_corners(h.hwnd.get());
        }
    }
}

/// Whether the renderer in use can draw a see-through window, and whether it's the GPU one.
/// Set once in main.rs, when the renderer is picked.
static SEE_THROUGH: AtomicBool = AtomicBool::new(false);
static GPU: AtomicBool = AtomicBool::new(false);

pub fn set_renderer(gpu: bool, see_through: bool) {
    GPU.store(gpu, Ordering::Relaxed);
    SEE_THROUGH.store(see_through, Ordering::Relaxed);
}

pub fn can_see_through() -> bool {
    SEE_THROUGH.load(Ordering::Relaxed)
}

/// The CPU renderer draws see-through windows only under X11; everywhere else that takes the GPU.
pub fn needs_gpu_for_blur() -> bool {
    cfg!(any(windows, target_os = "macos")) || (cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some())
}

/// Whether these settings want the GPU renderer: blur where the CPU can't see through, or the
/// Modern style, whose gradients only the GPU rounds.
pub fn wants_gpu(a: &crate::config::Appearance) -> bool {
    (a.blur && needs_gpu_for_blur()) || a.style == "modern"
}

pub fn is_gpu() -> bool {
    GPU.load(Ordering::Relaxed)
}

/// Whether these settings need Tendril to start again with the other renderer.
pub fn needs_restart(a: &crate::config::Appearance) -> bool {
    wants_gpu(a) != is_gpu()
}

/// A blur this soon after opening is the window manager settling, not you clicking away.
const SETTLE: Duration = Duration::from_millis(300);

pub fn attributes(attrs: WindowAttributes) -> WindowAttributes {
    if PLAIN.get() {
        return plain_attributes(attrs);
    }
    let attrs = attrs.with_window_level(WindowLevel::AlwaysOnTop).with_resizable(false);
    #[cfg(target_os = "linux")]
    let attrs = {
        use winit::platform::wayland::WindowAttributesExtWayland;
        use winit::platform::x11::{WindowAttributesExtX11, WindowType};
        // Tiling window managers float dialogs; the class lets people write rules for it.
        let attrs = WindowAttributesExtX11::with_name(attrs, "tendril", "Tendril");
        let attrs = WindowAttributesExtWayland::with_name(attrs, "tendril", "Tendril");
        attrs.with_x11_window_type(vec![WindowType::Dialog])
    };
    #[cfg(windows)]
    let attrs = {
        use winit::platform::windows::WindowAttributesExtWindows;
        attrs.with_skip_taskbar(true)
    };
    #[cfg(target_os = "macos")]
    let attrs = {
        use winit::platform::macos::WindowAttributesExtMacOS;
        // A real frame with nothing showing, for macOS's own rounded corners and shadow.
        attrs
            .with_titlebar_transparent(true)
            .with_title_hidden(true)
            .with_fullsize_content_view(true)
            .with_titlebar_buttons_hidden(true)
            .with_movable_by_window_background(false)
    };
    attrs
}

/// Hides the window when it loses focus, if config.toml says to.
pub fn watch(ui: &Launcher) {
    ui.window().on_winit_window_event(|_, event| {
        if let winit::event::WindowEvent::Focused(focused) = event {
            eprintln!("tendril: window focused: {focused}");
            if !focused {
                slint::Timer::single_shot(Duration::ZERO, clicked_away);
            }
        }
        EventResult::Propagate
    });
    #[cfg(windows)]
    foreground::watch();
    ui.window().on_close_requested(|| {
        with_app(|app| {
            app.closing();
            if let Some(ui) = app.ui.upgrade() {
                ui.invoke_blur();
            }
        });
        slint::CloseRequestResponse::HideWindow
    });
}

/// Another window took over: close, if that's the setting and Tendril has been open long enough
/// for this not to be the window manager settling.
fn clicked_away() {
    with_app(|app| {
        let settled = app.shown_at.get().is_some_and(|t| t.elapsed() > SETTLE);
        let Some(ui) = app.ui.upgrade() else { return };
        let (setting, visible) = (app.config.borrow().hide_on_blur, ui.window().is_visible());
        if setting && settled && visible {
            hide(&ui);
        } else if visible {
            eprintln!("tendril: another window came forward; staying (close on click away: {setting}, open long enough: {settled})");
        }
    });
}

/// Windows doesn't always let Tendril's window become the active one when it opens, and a window
/// that never was active is never told it stopped being. So Windows is also asked to say when
/// any other app's window comes to the front. It tells this thread, through the message loop
/// it's already running: nothing polls.
#[cfg(windows)]
mod foreground {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
    use windows::Win32::UI::WindowsAndMessaging::{EVENT_SYSTEM_FOREGROUND, GetWindowThreadProcessId, WINEVENT_OUTOFCONTEXT};

    pub fn watch() {
        unsafe {
            SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(changed), 0, 0, WINEVENT_OUTOFCONTEXT);
        }
    }

    unsafe extern "system" fn changed(_: HWINEVENTHOOK, _: u32, window: HWND, _: i32, _: i32, _: u32, _: u32) {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        if pid != 0 && pid != std::process::id() {
            eprintln!("tendril: another app came to the front");
            slint::Timer::single_shot(std::time::Duration::ZERO, super::clicked_away);
        }
    }
}

pub fn toggle(ui: &Launcher) {
    if ui.window().is_visible() { hide(ui) } else { show(ui) }
}

pub fn show(ui: &Launcher) {
    if !ui.window().is_visible() {
        with_app(|app| {
            app.opening();
            app.shown_at.set(Some(Instant::now()));
        });
        if let Some((x, y)) = PLACE.get() {
            ui.window().set_position(slint::PhysicalPosition::new(x, y));
        }
        if let Err(e) = ui.show() {
            eprintln!("tendril: couldn't open the window: {e}");
            return;
        }
    }
    place(ui);
    ui.invoke_focus_search();
}

pub fn hide(ui: &Launcher) {
    if ui.window().is_visible() {
        with_app(|app| app.closing());
        ui.invoke_blur();
        let _ = ui.hide();
    }
}

/// Centred across the screen and a third of the way down it, then given the keyboard. The first
/// time, the window only exists once the event loop runs, so this waits for it.
fn place(ui: &Launcher) {
    let weak = ui.as_weak();
    let _ = slint::spawn_local(async move {
        let Some(ui) = weak.upgrade() else { return };
        if let Ok(w) = ui.window().winit_window().await {
            let mut style = String::new();
            with_app(|app| style = app.config.borrow().appearance.blur_style.clone());
            put(&w, ui.get_see_through().then_some(style.as_str()), ui.global::<crate::Theme>().get_dark());
        }
    });
}

/// The screen the mouse pointer is on: where you're working, as Raycast and Spotlight open.
fn monitor_under_pointer(w: &winit::window::Window) -> Option<winit::monitor::MonitorHandle> {
    let (x, y) = pointer()?;
    w.available_monitors().find(|m| {
        let (at, size) = (m.position(), m.size());
        x >= at.x && x < at.x + size.width as i32 && y >= at.y && y < at.y + size.height as i32
    })
}

/// The pointer's position on the whole desktop, in physical pixels.
#[cfg(windows)]
fn pointer() -> Option<(i32, i32)> {
    let mut at = windows::Win32::Foundation::POINT::default();
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut at) }.ok()?;
    Some((at.x, at.y))
}

#[cfg(target_os = "linux")]
fn pointer() -> Option<(i32, i32)> {
    use x11rb::protocol::xproto::ConnectionExt as _;
    // Wayland has no DISPLAY to ask, and doesn't say where the pointer is.
    std::env::var_os("DISPLAY")?;
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = x11rb::connection::Connection::setup(&conn).roots[screen].root;
    let reply = conn.query_pointer(root).ok()?.reply().ok()?;
    Some((i32::from(reply.root_x), i32::from(reply.root_y)))
}

/// macOS: winit's own idea of the window's screen stands in.
#[cfg(target_os = "macos")]
fn pointer() -> Option<(i32, i32)> {
    None
}

/// `blur` is the blur style when the window is see-through.
fn put(w: &winit::window::Window, blur: Option<&str>, dark: bool) {
    if let Some(style) = blur {
        blur_behind(w, style, dark);
    }
    {
        if let Some(monitor) = monitor_under_pointer(w).or_else(|| w.primary_monitor()).or_else(|| w.current_monitor()) {
            let (at, screen, size) = (monitor.position(), monitor.size(), w.outer_size());
            let x = at.x + (screen.width as i32 - size.width as i32) / 2;
            let y = at.y + (screen.height as i32 - size.height as i32) / 3;
            if PLACE.get() != Some((x, y)) {
                w.set_outer_position(PhysicalPosition::new(x, y));
                PLACE.set(Some((x, y)));
            }
        }
        w.focus_window();
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{
                GWL_STYLE, GetWindowLongPtrW, SetForegroundWindow, SetWindowLongPtrW, WS_SYSMENU,
            };
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = w.window_handle()
                && let RawWindowHandle::Win32(h) = handle.as_raw()
            {
                crate::apps::round_corners(h.hwnd.get());
                let hwnd = HWND(h.hwnd.get() as *mut _);
                unsafe {
                    // No window menu: otherwise Alt+Space opens it instead of reaching Tendril,
                    // and that's the shortcut people most often want.
                    let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                    SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(WS_SYSMENU.0 as isize));
                    // The hotkey (or the Tendril that was started again) gave us the right to come
                    // to the front; winit's focus doesn't always use it.
                    let _ = SetForegroundWindow(hwnd);
                }
            }
        }
    }
}

/// Asks the system to blur what's behind the window. The window itself is see-through already
/// (it's drawn with a translucent panel); this is the softening, in the style picked:
///   Windows: "acrylic" blurs what's behind; "mica" and "mica-alt" (Windows 11) tint by the
///            wallpaper instead, and stay the same whatever's behind; "blur" is Windows 10's.
///   macOS:   a material: "hud", "popover", "sidebar", "menu" or "under-window".
///   Linux:   KDE is asked; picom, Hyprland and others blur see-through windows by their own settings.
#[allow(unused_variables)]
fn blur_behind(w: &winit::window::Window, style: &str, dark: bool) {
    #[cfg(windows)]
    {
        use window_vibrancy::{apply_acrylic, apply_blur, apply_mica, apply_tabbed};
        let applied = match style {
            "mica" => apply_mica(w, Some(dark)),
            "mica-alt" | "tabbed" => apply_tabbed(w, Some(dark)),
            "blur" => apply_blur(w, None),
            _ => apply_acrylic(w, None),
        };
        // Older Windows lacks the newer kinds: fall back to what it has.
        if applied.is_err() && apply_acrylic(w, None).is_err() {
            let _ = apply_blur(w, None);
        }
    }
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{NSVisualEffectMaterial as M, NSVisualEffectState, apply_vibrancy};
        let material = match style {
            "popover" => M::Popover,
            "sidebar" => M::Sidebar,
            "menu" => M::Menu,
            "under-window" => M::UnderWindowBackground,
            _ => M::HudWindow,
        };
        let _ = apply_vibrancy(w, material, Some(NSVisualEffectState::Active), Some(12.0));
    }
    #[cfg(target_os = "linux")]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let xid = match w.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Xlib(h)) => h.window as u32,
            Ok(RawWindowHandle::Xcb(h)) => h.window.get(),
            _ => return,
        };
        let _ = kde_blur(xid);
    }
}

#[cfg(target_os = "linux")]
fn kde_blur(xid: u32) -> Result<(), Box<dyn std::error::Error>> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
    use x11rb::wrapper::ConnectionExt as _;
    let (conn, _) = x11rb::connect(None)?;
    let atom = conn.intern_atom(false, b"_KDE_NET_WM_BLUR_BEHIND_REGION")?.reply()?.atom;
    // An empty region means the whole window.
    conn.change_property32(PropMode::REPLACE, xid, atom, AtomEnum::CARDINAL, &[])?;
    conn.flush()?;
    Ok(())
}
