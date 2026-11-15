//! The keys that open Flint from anywhere.
//!
//! Windows and macOS use the global-hotkey crate. On X11, Flint grabs the key itself and its
//! thread sleeps until X has an event, where the crate checks for one every 50ms. On Wayland
//! there's no key to grab; bind `flint toggle` in your desktop's settings.

use crate::app::with_app;
use crate::window;
use global_hotkey::hotkey::HotKey;
use std::cell::RefCell;

fn pressed() {
    let _ = slint::invoke_from_event_loop(|| {
        with_app(|app| {
            if let Some(ui) = app.ui.upgrade() {
                window::toggle(&ui);
            }
        })
    });
}

pub struct Hotkeys {
    backend: Option<Backend>,
    current: RefCell<Option<String>>,
}

impl Hotkeys {
    pub fn new() -> Hotkeys {
        let backend =
            Backend::new().map_err(|e| eprintln!("flint: no global hotkey here ({e}); bind `flint toggle` to a key instead")).ok();
        Hotkeys { backend, current: RefCell::new(None) }
    }

    /// Takes `text` ("Alt+Space"), letting go of the previous keys if they changed.
    pub fn set(&self, text: &str) {
        let Some(backend) = &self.backend else { return };
        if self.current.borrow().as_deref() == Some(text) {
            return;
        }
        // Remembered even when it fails, so a broken setting is reported once, not on every open.
        *self.current.borrow_mut() = Some(text.to_string());
        let hotkey: HotKey = match text.parse() {
            Ok(h) => h,
            Err(e) => return eprintln!("flint: hotkey \"{text}\" isn't one I understand ({e})"),
        };
        if let Err(e) = backend.set(hotkey) {
            eprintln!("flint: couldn't take {text} ({e}); another app may be using it");
        }
    }
}

#[cfg(not(target_os = "linux"))]
struct Backend {
    manager: global_hotkey::GlobalHotKeyManager,
    current: RefCell<Option<HotKey>>,
}

#[cfg(not(target_os = "linux"))]
impl Backend {
    fn new() -> Result<Backend, String> {
        use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
        let manager = global_hotkey::GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
        GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
            if event.state == HotKeyState::Pressed {
                pressed();
            }
        }));
        Ok(Backend { manager, current: RefCell::new(None) })
    }

    fn set(&self, hotkey: HotKey) -> Result<(), String> {
        if let Some(old) = self.current.take() {
            let _ = self.manager.unregister(old);
        }
        self.manager.register(hotkey).map_err(|e| e.to_string())?;
        *self.current.borrow_mut() = Some(hotkey);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
use x11::Backend;

#[cfg(target_os = "linux")]
mod x11 {
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use std::sync::{Arc, Mutex};
    use x11rb::connection::Connection;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask};
    use x11rb::rust_connection::RustConnection;

    /// The key and modifiers grabbed, as X numbers them.
    type Grabbed = Option<(u8, u16)>;

    pub struct Backend {
        conn: Arc<RustConnection>,
        root: u32,
        grabbed: Arc<Mutex<Grabbed>>,
    }

    /// Caps Lock and Num Lock change the modifier state, so each grab is made with and without them.
    const LOCKS: [u16; 4] = [0, 2, 16, 18];
    const RELEVANT: u16 = 1 | 4 | 8 | 64;

    impl Backend {
        pub fn new() -> Result<Backend, String> {
            if std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland") {
                return Err("Wayland doesn't let apps take keys for themselves".into());
            }
            let (conn, screen) = RustConnection::connect(None).map_err(|e| e.to_string())?;
            let root = conn.setup().roots[screen].root;
            let conn = Arc::new(conn);
            let grabbed: Arc<Mutex<Grabbed>> = Arc::default();
            let (c, g) = (conn.clone(), grabbed.clone());
            std::thread::Builder::new()
                .name("hotkey".into())
                .stack_size(128 * 1024)
                .spawn(move || listen(&c, &g))
                .map_err(|e| e.to_string())?;
            Ok(Backend { conn, root, grabbed })
        }

        pub fn set(&self, hotkey: HotKey) -> Result<(), String> {
            let keysym = keysym(hotkey.key).ok_or("that key isn't supported yet")?;
            let keycode = self.keycode(keysym).ok_or("no key on this keyboard makes that")?;
            let mods = modifiers(hotkey.mods);
            let mut grabbed = self.grabbed.lock().map_err(|e| e.to_string())?;
            if let Some((code, old)) = grabbed.take() {
                for lock in LOCKS {
                    let _ = self.conn.ungrab_key(code, self.root, ModMask::from(old | lock));
                }
            }
            for lock in LOCKS {
                self.conn
                    .grab_key(false, self.root, ModMask::from(mods | lock), keycode, GrabMode::ASYNC, GrabMode::ASYNC)
                    .map_err(|e| e.to_string())?
                    .check()
                    .map_err(|e| format!("{e:?}"))?;
            }
            *grabbed = Some((keycode, mods));
            Ok(())
        }

        fn keycode(&self, keysym: u32) -> Option<u8> {
            let setup = self.conn.setup();
            let (min, max) = (setup.min_keycode, setup.max_keycode);
            let map = self.conn.get_keyboard_mapping(min, max - min + 1).ok()?.reply().ok()?;
            let per = map.keysyms_per_keycode as usize;
            map.keysyms.chunks(per.max(1)).position(|syms| syms.contains(&keysym)).map(|i| min + i as u8)
        }
    }

    /// Blocks until X has something to say; no polling.
    fn listen(conn: &RustConnection, grabbed: &Mutex<Grabbed>) {
        let mut last_release = 0;
        while let Ok(event) = conn.wait_for_event() {
            match event {
                Event::KeyPress(e) => {
                    let wanted = grabbed.lock().ok().and_then(|g| *g);
                    // A held key repeats as a release and a press with the same timestamp.
                    let repeat = e.time == last_release;
                    if wanted == Some((e.detail, u16::from(e.state) & RELEVANT)) && !repeat {
                        super::pressed();
                    }
                }
                Event::KeyRelease(e) => last_release = e.time,
                _ => {}
            }
        }
    }

    fn modifiers(mods: Modifiers) -> u16 {
        let mut x = 0;
        if mods.contains(Modifiers::SHIFT) {
            x |= 1;
        }
        if mods.contains(Modifiers::CONTROL) {
            x |= 4;
        }
        if mods.contains(Modifiers::ALT) {
            x |= 8;
        }
        if mods.contains(Modifiers::META) {
            x |= 64;
        }
        x
    }

    fn keysym(code: Code) -> Option<u32> {
        use Code::*;
        let letters = [
            KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU,
            KeyV, KeyW, KeyX, KeyY, KeyZ,
        ];
        if let Some(i) = letters.iter().position(|&c| c == code) {
            return Some(0x61 + i as u32);
        }
        let digits = [Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9];
        if let Some(i) = digits.iter().position(|&c| c == code) {
            return Some(0x30 + i as u32);
        }
        let functions = [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12];
        if let Some(i) = functions.iter().position(|&c| c == code) {
            return Some(0xffbe + i as u32);
        }
        Some(match code {
            Space => 0x20,
            Enter => 0xff0d,
            Tab => 0xff09,
            Escape => 0xff1b,
            Backquote => 0x60,
            Minus => 0x2d,
            Equal => 0x3d,
            Comma => 0x2c,
            Period => 0x2e,
            Slash => 0x2f,
            Semicolon => 0x3b,
            Quote => 0x27,
            BracketLeft => 0x5b,
            BracketRight => 0x5d,
            Backslash => 0x5c,
            _ => return None,
        })
    }
}
