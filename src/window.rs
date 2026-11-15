//! Opening, placing and closing the window. Closing destroys it (see main.rs), so between uses
//! Flint holds no window and no pixels, only the list it searches.

use crate::Launcher;
use crate::app::with_app;
use slint::ComponentHandle;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
use std::cell::Cell;
use std::time::{Duration, Instant};
use winit::dpi::PhysicalPosition;
use winit::window::{WindowAttributes, WindowLevel};

thread_local! {
    /// Where the window went last time, so the next one is made there instead of jumping.
    static PLACE: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
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
            put(&w);
        }
    });
}

fn put(w: &winit::window::Window) {
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
