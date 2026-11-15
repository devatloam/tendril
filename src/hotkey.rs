//! The keys that open Tendril from anywhere.
//!
//! Windows and macOS use the global-hotkey crate. On X11, Tendril grabs the key itself and its
//! thread sleeps until X has an event, where the crate checks for one every 50ms. On Wayland
//! there's no key to grab; bind `tendril toggle` in your desktop's settings.

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
    paused: std::cell::Cell<bool>,
    /// The keys config.toml asks for, when another app holds them (Alt+Space often is, by
    /// Raycast or PowerToys Run on Windows).
    refused: RefCell<Option<String>>,
}

impl Hotkeys {
    pub fn new() -> Hotkeys {
        let backend =
            Backend::new().map_err(|e| eprintln!("tendril: no global hotkey here ({e}); bind `tendril toggle` to a key instead")).ok();
        Hotkeys { backend, current: RefCell::new(None), paused: std::cell::Cell::new(false), refused: RefCell::new(None) }
    }

    /// Whether there's a global key to take here at all (not on Wayland).
    pub fn available(&self) -> bool {
        self.backend.is_some()
    }

    /// Takes `text` ("Alt+Space"), letting go of the previous keys if they changed. A broken
    /// setting is reported once, not on every open.
    pub fn set(&self, text: &str) {
        if self.current.borrow().as_deref() == Some(text) || self.refused.borrow().as_deref() == Some(text) {
            return;
        }
        match self.change(text) {
            Ok(()) => *self.refused.borrow_mut() = None,
            Err(e) => {
                eprintln!("tendril: couldn't take {text} ({e})");
                *self.refused.borrow_mut() = Some(text.to_string());
            }
        }
    }

    /// The keys that were asked for but another app holds, so they don't open Tendril.
    pub fn refused(&self) -> Option<String> {
        self.refused.borrow().clone()
    }

    /// Takes `text` now, saying why not if it can't; the keys held before are kept then.
    pub fn change(&self, text: &str) -> Result<(), String> {
        let Some(backend) = &self.backend else { return Err("there's no global key to take here".into()) };
        let hotkey: HotKey = text.parse().map_err(|e| format!("\"{text}\" isn't a key I understand ({e})"))?;
        let before = self.current.replace(Some(text.to_string()));
        // Taken even while paused, to find out now whether another app holds them; then let go
        // again until the pause ends.
        let taken = backend.set(hotkey);
        if taken.is_ok() && self.paused.get() {
            backend.clear();
        }
        if taken.is_ok() {
            *self.refused.borrow_mut() = None;
        }
        if let Err(e) = taken {
            // Put the old keys back.
            if let Some(old) = before.as_deref().and_then(|t| t.parse::<HotKey>().ok()) {
                let _ = backend.set(old);
            }
            *self.current.borrow_mut() = before;
            return Err(format!("another app is using those keys ({e})"));
        }
        Ok(())
    }

    /// Lets go of the keys for a while, so you can press them to record new ones.
    pub fn pause(&self) {
        if !self.paused.replace(true)
            && let Some(backend) = &self.backend
        {
            backend.clear();
        }
    }

    pub fn resume(&self) {
        if self.paused.replace(false)
            && let (Some(backend), Some(text)) = (&self.backend, self.current.borrow().clone())
            && let Ok(hotkey) = text.parse::<HotKey>()
        {
            let _ = backend.set(hotkey);
        }
    }
}

/// The shortcut for a key press from the window, as config.toml spells it ("Ctrl+Shift+Space"),
/// or None if it isn't one: it needs Ctrl, Alt or Super, except a function key on its own.
/// On macOS Slint reports ⌘ as `control` and Control as `meta`.
pub fn shortcut(text: &str, control: bool, alt: bool, shift: bool, meta: bool) -> Option<String> {
    let (cmd, ctrl) = if cfg!(target_os = "macos") { (control, meta) } else { (meta, control) };
    let key = key_name(text)?;
    let function = key.starts_with('F') && key.len() > 1 && key[1..].chars().all(|c| c.is_ascii_digit());
    if !(ctrl || alt || cmd || function) {
        return None;
    }
    let mut parts = Vec::new();
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    if cmd {
        parts.push(if cfg!(target_os = "macos") { "Cmd" } else { "Super" });
    }
    let combo = parts.into_iter().map(str::to_string).chain([key]).collect::<Vec<_>>().join("+");
    combo.parse::<HotKey>().ok().map(|_| combo)
}

fn key_name(text: &str) -> Option<String> {
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(match c {
        'a'..='z' | 'A'..='Z' => format!("Key{}", c.to_ascii_uppercase()),
        // Ctrl and a letter can arrive as the old control character for it (Ctrl+A is 1).
        '\u{1}'..='\u{1a}' => format!("Key{}", (b'A' + c as u8 - 1) as char),
        '0'..='9' => format!("Digit{c}"),
        ' ' => "Space".into(),
        ',' => "Comma".into(),
        '.' => "Period".into(),
        '/' => "Slash".into(),
        ';' => "Semicolon".into(),
        '\'' => "Quote".into(),
        '[' => "BracketLeft".into(),
        ']' => "BracketRight".into(),
        '\\' => "Backslash".into(),
        '-' => "Minus".into(),
        '=' => "Equal".into(),
        '`' => "Backquote".into(),
        // Slint's function keys, F1 to F12.
        '\u{F704}'..='\u{F70F}' => format!("F{}", c as u32 - 0xF704 + 1),
        _ => return None,
    })
}

/// The keys of a shortcut as they're shown on keycaps: "Ctrl+Shift+KeyK" is Ctrl, Shift, K.
pub fn keycaps(combo: &str) -> Vec<String> {
    let mac = cfg!(target_os = "macos");
    combo
        .split('+')
        .map(|part| {
            let p = part.trim();
            match p.to_lowercase().as_str() {
                "ctrl" | "control" => if mac { "⌃" } else { "Ctrl" }.to_string(),
                "alt" | "option" => if mac { "⌥" } else { "Alt" }.to_string(),
                "shift" => if mac { "⇧" } else { "Shift" }.to_string(),
                "super" | "meta" | "cmd" | "command" => if mac { "⌘" } else { "Super" }.to_string(),
                _ => p.strip_prefix("Key").or_else(|| p.strip_prefix("Digit")).unwrap_or(p).to_string(),
            }
        })
        .collect()
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
            eprintln!("tendril: shortcut {:?}", event.state);
            if event.state == HotKeyState::Pressed {
                pressed();
            }
        }));
        Ok(Backend { manager, current: RefCell::new(None) })
    }

    fn clear(&self) {
        if let Some(old) = self.current.take() {
            let gone = self.manager.unregister(old);
            eprintln!("tendril: let go of {old} ({gone:?})");
        }
    }

    fn set(&self, hotkey: HotKey) -> Result<(), String> {
        self.clear();
        let taken = self.manager.register(hotkey);
        eprintln!("tendril: took {hotkey} ({taken:?})");
        taken.map_err(|e| e.to_string())?;
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

        pub fn clear(&self) {
            if let Ok(mut grabbed) = self.grabbed.lock()
                && let Some((code, old)) = grabbed.take()
            {
                for lock in LOCKS {
                    let _ = self.conn.ungrab_key(code, self.root, ModMask::from(old | lock));
                }
                let _ = self.conn.flush();
            }
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
                let grab = self
                    .conn
                    .grab_key(false, self.root, ModMask::from(mods | lock), keycode, GrabMode::ASYNC, GrabMode::ASYNC)
                    .map_err(|e| e.to_string())
                    .and_then(|cookie| cookie.check().map_err(|e| format!("{e:?}")));
                if let Err(e) = grab {
                    // Another app holds them: let go of the variants already taken.
                    for lock in LOCKS {
                        let _ = self.conn.ungrab_key(keycode, self.root, ModMask::from(mods | lock));
                    }
                    let _ = self.conn.flush();
                    return Err(e);
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_from_key_presses() {
        let ctrl = !cfg!(target_os = "macos");
        assert_eq!(shortcut(" ", false, true, false, false).as_deref(), Some("Alt+Space"));
        assert_eq!(shortcut("k", ctrl, false, true, !ctrl).as_deref(), Some("Ctrl+Shift+KeyK"));
        assert_eq!(shortcut("\u{F70B}", false, false, false, false).as_deref(), Some("F8"));
        assert_eq!(shortcut("k", false, false, false, false), None, "a plain letter is typing, not a shortcut");
        assert_eq!(shortcut("k", false, false, true, false), None);
        if !cfg!(target_os = "macos") {
            assert_eq!(keycaps("Ctrl+Shift+KeyK"), ["Ctrl", "Shift", "K"]);
            assert_eq!(keycaps("Alt+Space"), ["Alt", "Space"]);
        }
    }
}
