//! Opening, placing and closing the window. Closing destroys it (see main.rs), so between uses
//! Flint holds no window and no pixels, only the list it searches.

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

/// Whether turning blur on or off here needs Flint to start again with another renderer.
pub fn needs_restart(blur: bool) -> bool {
    needs_gpu_for_blur() && blur != GPU.load(Ordering::Relaxed)
}

/// A blur this soon after opening is the window manager settling, not you clicking away.
const SETTLE: Duration = Duration::from_millis(300);

pub fn attributes(attrs: WindowAttributes) -> WindowAttributes {
    let attrs = attrs.with_window_level(WindowLevel::AlwaysOnTop).with_resizable(false);
    #[cfg(target_os = "linux")]
    let attrs = {
        use winit::platform::wayland::WindowAttributesExtWayland;
        use winit::platform::x11::{WindowAttributesExtX11, WindowType};
        // Tiling window managers float dialogs; the class lets people write rules for it.
        let attrs = WindowAttributesExtX11::with_name(attrs, "flint", "Flint");
        let attrs = WindowAttributesExtWayland::with_name(attrs, "flint", "Flint");
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
        if let winit::event::WindowEvent::Focused(false) = event {
            slint::Timer::single_shot(Duration::ZERO, || {
                with_app(|app| {
                    let settled = app.shown_at.get().is_some_and(|t| t.elapsed() > SETTLE);
                    if app.config.borrow().hide_on_blur
                        && settled
                        && let Some(ui) = app.ui.upgrade()
                    {
                        hide(&ui);
                    }
                })
            });
        }
        EventResult::Propagate
    });
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
            eprintln!("flint: couldn't open the window: {e}");
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

/// `blur` is the blur style when the window is see-through.
fn put(w: &winit::window::Window, blur: Option<&str>, dark: bool) {
    if let Some(style) = blur {
        blur_behind(w, style, dark);
    }
    {
        if let Some(monitor) = w.primary_monitor().or_else(|| w.current_monitor()) {
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
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = w.window_handle()
                && let RawWindowHandle::Win32(h) = handle.as_raw()
            {
                crate::apps::round_corners(h.hwnd.get());
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
