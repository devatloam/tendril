//! One Tendril at a time, and `tendril toggle` for desktops without global shortcuts (Wayland):
//! the running Tendril listens on a socket (a Unix socket in the runtime folder, or on Windows
//! one on 127.0.0.1), and a second one hands it a word ("toggle", "show", "hide") and leaves.

#[cfg(unix)]
mod unix {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::time::Duration;

    fn path() -> PathBuf {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let user = std::env::var("USER").unwrap_or_default();
        dir.join(format!("tendril-{user}.sock"))
    }

    pub fn send(word: &str) -> std::io::Result<()> {
        let mut stream = UnixStream::connect(path())?;
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;
        stream.write_all(word.as_bytes())
    }

    /// Listens on a small thread and calls `on` with each word received.
    pub fn listen(on: impl Fn(String) + Send + 'static) -> std::io::Result<()> {
        let path = path();
        // Left behind by a Tendril that didn't exit cleanly: nothing answered `send`, so it's stale.
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        std::thread::Builder::new().name("ipc".into()).stack_size(64 * 1024).spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut word = String::new();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let _ = (&stream).take(64).read_to_string(&mut word);
                on(word.trim().to_string());
            }
        })?;
        Ok(())
    }

    pub fn cleanup() {
        let _ = std::fs::remove_file(path());
    }
}

#[cfg(unix)]
pub use unix::*;

/// Windows has no Unix sockets in std, so a socket on 127.0.0.1 stands in: the running Tendril
/// listens on a port of the system's choosing and writes it to a file only you can read. A second
/// Tendril reads it, says its word, and lets the running one come to the front (Windows only
/// lets a window take focus from the app in front with that app's say-so).
#[cfg(windows)]
mod windows {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::time::Duration;

    fn path() -> PathBuf {
        dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("tendril").join("instance.port")
    }

    pub fn send(word: &str) -> std::io::Result<()> {
        let port: u16 = std::fs::read_to_string(path())?.trim().parse().map_err(std::io::Error::other)?;
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(300))?;
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;
        // ASFW_ANY: whichever process, here the running Tendril, may take the foreground next.
        unsafe {
            let _ = ::windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(u32::MAX);
        }
        stream.write_all(format!("tendril:{word}").as_bytes())
    }

    pub fn listen(on: impl Fn(String) + Send + 'static) -> std::io::Result<()> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let path = path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, port.to_string())?;
        std::thread::Builder::new().name("ipc".into()).stack_size(64 * 1024).spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut text = String::new();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let _ = (&stream).take(64).read_to_string(&mut text);
                // Only words from another Tendril; anything else that finds the port is ignored.
                if let Some(word) = text.trim().strip_prefix("tendril:") {
                    on(word.to_string());
                }
            }
        })?;
        Ok(())
    }

    pub fn cleanup() {
        let _ = std::fs::remove_file(path());
    }
}

#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(any(unix, windows)))]
pub fn send(_: &str) -> std::io::Result<()> {
    Err(std::io::Error::other("not supported on this system yet; use the hotkey"))
}

#[cfg(not(any(unix, windows)))]
pub fn listen(_: impl Fn(String) + Send + 'static) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub fn cleanup() {}
