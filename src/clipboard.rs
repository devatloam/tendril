//! Clipboard history: text you copy, kept so you can paste it again later.
//!
//! On X11 and Windows a thread sleeps until the system says the clipboard changed (XFixes, or a
//! clipboard format listener) and reads it then, so nothing polls. macOS and Wayland have no way
//! to be told, so there the clipboard is read when Tendril opens: you get your latest copy, not
//! every copy in between.
//!
//! Copies that password managers mark as secret are skipped where the system lets us see the
//! mark (KDE's hint on X11, Windows' exclusion formats). History is kept in clipboard.json
//! beside usage.toml, readable only by you, and turning it off deletes it.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

/// How many copies are kept.
const MAX_CLIPS: usize = 200;
/// Longer copies (a whole log file, say) aren't kept.
const MAX_BYTES: usize = 512 * 1024;

/// Whether copies are being kept. Read by the watcher thread before it reads anything.
pub static ON: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Clip {
    pub id: u64,
    pub text: String,
    /// Seconds since 1970 when it was (last) copied.
    pub at: u64,
}

#[derive(Default)]
pub struct History {
    /// Newest first.
    pub clips: Vec<Clip>,
}

pub fn path() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("tendril").join("clipboard.json")
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl History {
    pub fn load() -> History {
        let clips = std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        History { clips }
    }

    /// Keeps `text` as the newest copy. Copying something already kept moves it to the top.
    /// Returns whether anything changed.
    pub fn add(&mut self, text: String, at: u64) -> bool {
        if text.trim().is_empty() || text.len() > MAX_BYTES {
            return false;
        }
        if self.clips.first().is_some_and(|c| c.text == text) {
            return false;
        }
        self.clips.retain(|c| c.text != text);
        let id = self.clips.iter().map(|c| c.id).max().map_or(1, |m| m + 1);
        self.clips.insert(0, Clip { id, text, at });
        self.clips.truncate(MAX_CLIPS);
        true
    }

    pub fn remove(&mut self, id: u64) {
        self.clips.retain(|c| c.id != id);
    }

    pub fn get(&self, id: u64) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }

    pub fn save(&self) {
        let path = path();
        if self.clips.is_empty() {
            let _ = std::fs::remove_file(&path);
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let Ok(json) = serde_json::to_string(&self.clips) else { return };
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        if let Ok(mut file) = options.open(&path) {
            let _ = std::io::Write::write_all(&mut file, json.as_bytes());
        }
    }
}

/// What a copy is called in the list: its first line that has something on it, shortened.
pub fn title(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let mut out: String = line.chars().take(90).collect();
    if line.chars().count() > 90 {
        out.push('…');
    }
    out
}

/// Whether a copy is a web address, for its icon.
pub fn is_link(text: &str) -> bool {
    let t = text.trim();
    !t.contains(char::is_whitespace) && (t.starts_with("https://") || t.starts_with("http://"))
}

/// Starts the thread that hears about copies, where the system can tell us. Returns false where
/// it can't (macOS, Wayland), and the clipboard is read when Tendril opens instead.
pub fn watch() -> bool {
    imp::watch(|text| {
        let _ = slint::invoke_from_event_loop(move || crate::app::with_app(|a| a.copied(text)));
    })
}

/// The clipboard's text right now, for systems that can't tell us when it changes.
pub fn read() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// Whether Tendril can paste into the window you were in before it opened (by pressing
/// Ctrl V for you). Elsewhere, choosing a copy puts it on the clipboard for you to paste.
pub fn can_paste() -> bool {
    imp::can_paste()
}

/// Presses the paste keys in whatever window has focus once Tendril has closed.
pub fn paste_soon() {
    let _ = std::thread::Builder::new().name("paste".into()).spawn(|| {
        // Long enough for the window to go and focus to return to the one before.
        std::thread::sleep(std::time::Duration::from_millis(180));
        if let Err(e) = imp::press_paste() {
            eprintln!("tendril: couldn't paste: {e}");
        }
    });
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{MAX_BYTES, ON};
    use std::sync::atomic::Ordering;
    use x11rb::connection::Connection;
    use x11rb::protocol::Event;
    use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, CreateWindowAux, WindowClass};
    use x11rb::protocol::xtest::ConnectionExt as _;
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;
    use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

    fn x11() -> bool {
        std::env::var_os("DISPLAY").is_some() && std::env::var("XDG_SESSION_TYPE").map_or(true, |t| t != "wayland")
    }

    pub fn can_paste() -> bool {
        x11()
    }

    pub fn watch(on_copy: impl Fn(String) + Send + 'static) -> bool {
        if !x11() {
            return false;
        }
        let Ok((conn, screen)) = x11rb::connect(None) else { return false };
        let setup = || -> Result<_, Box<dyn std::error::Error>> {
            conn.xfixes_query_version(5, 0)?.reply()?;
            let root = conn.setup().roots[screen].root;
            let window = conn.generate_id()?;
            conn.create_window(COPY_DEPTH_FROM_PARENT, window, root, 0, 0, 1, 1, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new())?;
            let atom = |name: &[u8]| -> Result<u32, Box<dyn std::error::Error>> { Ok(conn.intern_atom(false, name)?.reply()?.atom) };
            let atoms = Atoms {
                clipboard: atom(b"CLIPBOARD")?,
                targets: atom(b"TARGETS")?,
                utf8: atom(b"UTF8_STRING")?,
                incr: atom(b"INCR")?,
                secret: atom(b"x-kde-passwordManagerHint")?,
                property: atom(b"TENDRIL_CLIPBOARD")?,
            };
            conn.xfixes_select_selection_input(window, atoms.clipboard, SelectionEventMask::SET_SELECTION_OWNER)?;
            conn.flush()?;
            Ok((window, atoms))
        };
        let Ok((window, atoms)) = setup() else { return false };
        std::thread::Builder::new()
            .name("clipboard".into())
            .stack_size(256 * 1024)
            .spawn(move || listen(&conn, window, &atoms, on_copy))
            .is_ok()
    }

    struct Atoms {
        clipboard: u32,
        targets: u32,
        utf8: u32,
        incr: u32,
        secret: u32,
        property: u32,
    }

    /// Sleeps until the clipboard changes hands, then asks the new owner what it holds (TARGETS)
    /// and, unless it's marked secret, for the text. Each answer arrives as its own event.
    fn listen(conn: &RustConnection, window: u32, atoms: &Atoms, on_copy: impl Fn(String)) {
        let ask = |target: u32| {
            let _ = conn.convert_selection(window, atoms.clipboard, target, atoms.property, CURRENT_TIME);
            let _ = conn.flush();
        };
        while let Ok(event) = conn.wait_for_event() {
            match event {
                Event::XfixesSelectionNotify(e) if e.owner != NONE && ON.load(Ordering::Relaxed) => ask(atoms.targets),
                Event::SelectionNotify(e) if e.property != NONE => {
                    let reply = conn
                        .get_property(true, window, atoms.property, AtomEnum::ANY, 0, (MAX_BYTES / 4 + 1) as u32)
                        .ok()
                        .and_then(|c| c.reply().ok());
                    let Some(reply) = reply else { continue };
                    if e.target == atoms.targets {
                        let offered: Vec<u32> = reply.value32().map(Iterator::collect).unwrap_or_default();
                        if offered.contains(&atoms.secret) {
                            continue;
                        }
                        let text = if offered.contains(&atoms.utf8) { atoms.utf8 } else { AtomEnum::STRING.into() };
                        ask(text);
                    } else if reply.type_ != atoms.incr && reply.bytes_after == 0 {
                        // INCR is how big copies come, piece by piece; those are past MAX_BYTES anyway.
                        on_copy(String::from_utf8_lossy(&reply.value).into_owned());
                    }
                }
                _ => {}
            }
        }
    }

    /// Ctrl V, pressed and let go through XTEST.
    pub fn press_paste() -> Result<(), String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let setup = conn.setup();
        let root = setup.roots[screen].root;
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = conn.get_keyboard_mapping(min, max - min + 1).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        let per = usize::from(map.keysyms_per_keycode.max(1));
        let code = |keysym: u32| {
            map.keysyms.chunks(per).position(|syms| syms.contains(&keysym)).map(|i| min + i as u8).ok_or("no key for it on this keyboard")
        };
        let (control, v) = (code(0xffe3)?, code(0x0076)?);
        let press = |kind: u8, key: u8| conn.xtest_fake_input(kind, key, CURRENT_TIME, root, 0, 0, 0).map(drop);
        let send = || -> Result<(), Box<dyn std::error::Error>> {
            press(2, control)?;
            press(2, v)?;
            press(3, v)?;
            press(3, control)?;
            conn.sync()?;
            Ok(())
        };
        send().map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
mod imp {
    use super::{MAX_BYTES, ON};
    use std::sync::OnceLock;
    use std::sync::atomic::Ordering;
    use windows::Win32::Foundation::{HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::DataExchange::{
        AddClipboardFormatListener, CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_CONTROL, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, HWND_MESSAGE, MSG, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE,
        WM_CLIPBOARDUPDATE, WNDCLASSW,
    };
    use windows::core::{PCWSTR, w};

    /// Windows' plain text, as UTF-16.
    const CF_UNICODETEXT: u32 = 13;

    type OnCopy = Box<dyn Fn(String) + Send + Sync>;
    static ON_COPY: OnceLock<OnCopy> = OnceLock::new();

    pub fn can_paste() -> bool {
        true
    }

    pub fn watch(on_copy: impl Fn(String) + Send + Sync + 'static) -> bool {
        if ON_COPY.set(Box::new(on_copy)).is_err() {
            return false;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("clipboard".into()).stack_size(256 * 1024).spawn(move || unsafe {
            let class = WNDCLASSW { lpfnWndProc: Some(window_proc), lpszClassName: w!("TendrilClipboard"), ..Default::default() };
            RegisterClassW(&class);
            let window = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("TendrilClipboard"),
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
            );
            let listening = window.is_ok_and(|w| AddClipboardFormatListener(w).is_ok());
            let _ = tx.send(listening);
            if !listening {
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                DispatchMessageW(&msg);
            }
        });
        spawned.is_ok() && rx.recv().unwrap_or(false)
    }

    unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if message == WM_CLIPBOARDUPDATE && ON.load(Ordering::Relaxed) {
            if let Some(text) = unsafe { read(window) }
                && let Some(on_copy) = ON_COPY.get()
            {
                on_copy(text);
            }
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    /// The clipboard's text, unless whoever copied it asked for it not to be kept.
    unsafe fn read(window: HWND) -> Option<String> {
        unsafe {
            let secret = [w!("ExcludeClipboardContentFromMonitorProcessing"), w!("Clipboard Viewer Ignore")];
            if secret.iter().any(|name| IsClipboardFormatAvailable(RegisterClipboardFormatW(*name)).is_ok()) {
                return None;
            }
            IsClipboardFormatAvailable(CF_UNICODETEXT).ok()?;
            // Whoever copied may still have it open for a moment.
            let mut opened = false;
            for _ in 0..5 {
                if OpenClipboard(Some(window)).is_ok() {
                    opened = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            if !opened {
                return None;
            }
            let text = (|| {
                let data = GetClipboardData(CF_UNICODETEXT).ok()?;
                let global = HGLOBAL(data.0);
                let size = GlobalSize(global);
                if size == 0 || size > MAX_BYTES * 2 {
                    return None;
                }
                let pointer = GlobalLock(global) as *const u16;
                if pointer.is_null() {
                    return None;
                }
                let wide = std::slice::from_raw_parts(pointer, size / 2);
                let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
                let text = String::from_utf16_lossy(&wide[..end]);
                let _ = GlobalUnlock(global);
                Some(text)
            })();
            let _ = CloseClipboard();
            text
        }
    }

    pub fn press_paste() -> Result<(), String> {
        let key = |vk, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() }, ..Default::default() },
            },
        };
        let inputs = [key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize == inputs.len() { Ok(()) } else { Err("Windows didn't take the key presses".into()) }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    pub fn can_paste() -> bool {
        false
    }

    pub fn watch(_on_copy: impl Fn(String) + Send + 'static) -> bool {
        false
    }

    pub fn press_paste() -> Result<(), String> {
        Err("pasting for you isn't done on macOS yet".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_copies_newest_first() {
        let mut h = History::default();
        assert!(h.add("one".into(), 1));
        assert!(h.add("two".into(), 2));
        assert!(!h.add("two".into(), 3), "the same copy twice in a row is one copy");
        assert!(h.add("one".into(), 4), "copying it again moves it to the top");
        assert_eq!(h.clips.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["one", "two"]);
        assert!(!h.add("   \n".into(), 5));
        for i in 0..MAX_CLIPS + 5 {
            h.add(format!("copy {i}"), 6);
        }
        assert_eq!(h.clips.len(), MAX_CLIPS);
        let id = h.clips[3].id;
        h.remove(id);
        assert!(h.get(id).is_none());
    }

    #[test]
    fn titles() {
        assert_eq!(title("\n\n  hello\nworld"), "hello");
        assert!(title(&"x".repeat(200)).ends_with('…'));
        assert!(is_link("https://example.com/a"));
        assert!(!is_link("see https://example.com"));
    }
}
