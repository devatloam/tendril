//! One Flint at a time, and `flint toggle` for desktops without global shortcuts (Wayland):
//! the running Flint listens on a socket in the runtime folder, and a second one hands it
//! a word ("toggle", "show", "hide") and leaves.

#[cfg(unix)]
mod unix {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::time::Duration;

    fn path() -> PathBuf {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let user = std::env::var("USER").unwrap_or_default();
        dir.join(format!("flint-{user}.sock"))
    }

    pub fn send(word: &str) -> std::io::Result<()> {
        let mut stream = UnixStream::connect(path())?;
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;
        stream.write_all(word.as_bytes())
    }

    /// Listens on a small thread and calls `on` with each word received.
    pub fn listen(on: impl Fn(String) + Send + 'static) -> std::io::Result<()> {
        let path = path();
        // Left behind by a Flint that didn't exit cleanly: nothing answered `send`, so it's stale.
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

#[cfg(not(unix))]
pub fn send(_: &str) -> std::io::Result<()> {
    Err(std::io::Error::other("not supported on this system yet; use the hotkey"))
}

#[cfg(not(unix))]
pub fn listen(_: impl Fn(String) + Send + 'static) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(unix))]
pub fn cleanup() {}
