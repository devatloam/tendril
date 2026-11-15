// No console window behind the launcher on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod appearance;
mod apps;
mod autostart;
mod calc;
mod catalog;
mod config;
mod hotkey;
mod icons;
mod ipc;
mod palettes;
mod paths;
mod search;
mod system;
mod update;
mod usage;
mod window;

slint::include_modules!();

use app::{App, with_app};
use std::process::ExitCode;

const HELP: &str = "\
tendril: a keyboard launcher

  tendril               start, or open the one that's running
  tendril --hidden      start without opening the window (for login items)
  tendril --appearance  start, or open the running one, on the Appearance settings
  tendril toggle        open or close the running Tendril; bind this on Wayland
  tendril show | hide
  tendril --version

Settings live in config.toml; \"Tendril Settings\" opens it.
";

fn main() -> ExitCode {
    let mut start_hidden = false;
    let mut appearance = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--appearance" => appearance = true,
            // Started again by the one before it (see app::RESTART): give it a moment to let go of
            // the hotkey and the socket.
            "--restarted" => std::thread::sleep(std::time::Duration::from_millis(300)),
            "toggle" | "show" | "hide" => {
                return match ipc::send(&arg) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("tendril: couldn't reach a running Tendril ({e})");
                        ExitCode::FAILURE
                    }
                };
            }
            "--hidden" | "--background" => start_hidden = true,
            "-V" | "--version" => {
                println!("tendril {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "-h" | "--help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("tendril: unknown argument {other}\n\n{HELP}");
                return ExitCode::from(2);
            }
        }
    }

    // Already running: bring that one up instead of starting a second.
    if ipc::send(if appearance { "appearance" } else { "show" }).is_ok() {
        return ExitCode::SUCCESS;
    }

    // Hiding destroys the window instead of keeping it, so its pixel buffer is freed while
    // you're not using it. Set before any thread starts.
    unsafe { std::env::set_var("SLINT_DESTROY_WINDOW_ON_HIDE", "1") };

    match run(start_hidden, appearance) {
        Ok(()) if app::RESTART.load(std::sync::atomic::Ordering::SeqCst) => {
            // Everything above has been dropped; start the next Tendril and leave.
            let me = std::env::current_exe().ok();
            let args: &[&str] = if app::RESTART_TO_APPEARANCE.load(std::sync::atomic::Ordering::SeqCst) {
                &["--restarted", "--appearance"]
            } else {
                &["--restarted"]
            };
            match me.map(|me| std::process::Command::new(me).args(args).spawn()) {
                Some(Ok(_)) => ExitCode::SUCCESS,
                _ => {
                    eprintln!("tendril: couldn't start again; start Tendril yourself");
                    ExitCode::FAILURE
                }
            }
        }
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tendril: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(start_hidden: bool, appearance: bool) -> Result<(), Box<dyn std::error::Error>> {
    let config = config::load();
    select_backend(config.appearance.blur)?;
    let ui = Launcher::new()?;
    ui.set_frameless(!cfg!(target_os = "macos"));
    let app = App::install(&ui, config);
    window::watch(&ui);
    app.rescan(false);

    let hotkeys = hotkey::Hotkeys::new();
    hotkeys.set(&app.config.borrow().hotkey);
    app.set_hotkeys(hotkeys);
    app.start_update_checks();

    ipc::listen(|word| {
        let _ = slint::invoke_from_event_loop(move || {
            with_app(|app| {
                let Some(ui) = app.ui.upgrade() else { return };
                match word.as_str() {
                    "toggle" => window::toggle(&ui),
                    "show" => window::show(&ui),
                    "hide" => window::hide(&ui),
                    "appearance" => {
                        window::show(&ui);
                        app.open_appearance();
                    }
                    _ => {}
                }
            })
        });
    })?;

    if start_hidden {
        println!("tendril: running; press {} to open", app.config.borrow().hotkey);
    } else {
        window::show(&ui);
        if appearance {
            app.open_appearance();
        }
    }
    slint::run_event_loop_until_quit()?;
    app.closing();
    ipc::cleanup();
    Ok(())
}

/// winit with the CPU renderer: no GPU context and its driver memory, and plenty fast for a
/// window this size since only what changed is redrawn. A see-through window needs the GPU
/// renderer everywhere but X11, so that's used when blur is on there; if it can't start (no
/// OpenGL), Tendril falls back to the CPU and a solid window.
fn select_backend(blur: bool) -> Result<(), slint::PlatformError> {
    let gpu = blur && window::needs_gpu_for_blur();
    let wanted = std::env::var("TENDRIL_RENDERER").unwrap_or_else(|_| if gpu { "femtovg" } else { "software" }.into());
    match selector(&wanted).select() {
        Ok(()) => {
            let gpu = wanted != "software";
            window::set_renderer(gpu, gpu || !window::needs_gpu_for_blur());
            Ok(())
        }
        Err(e) if wanted != "software" => {
            eprintln!("tendril: couldn't draw with the GPU ({e}); using a solid window");
            window::set_renderer(false, !window::needs_gpu_for_blur());
            selector("software").select()
        }
        Err(e) => Err(e),
    }
}

fn selector(renderer: &str) -> slint::BackendSelector {
    #[allow(unused_mut)]
    let mut selector = slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name(renderer.into())
        .with_winit_window_attributes_hook(window::attributes);
    #[cfg(target_os = "macos")]
    {
        use slint::winit_030::winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        // No Dock icon and no menu bar: Tendril is a panel, not an app you switch to.
        let mut builder = slint::winit_030::winit::event_loop::EventLoop::<slint::winit_030::SlintEvent>::with_user_event();
        builder.with_activation_policy(ActivationPolicy::Accessory);
        selector = selector.with_winit_event_loop_builder(builder);
    }
    selector
}
