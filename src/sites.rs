//! Websites' own icons for your quicklinks, so "GitHub repository" shows GitHub's mark.
//!
//! Fetched once per site, from the site itself (its apple-touch-icon, else its favicon), on a
//! thread of its own the first time a quicklink to it is listed, then kept as a 64px PNG in the
//! cache folder. Nothing goes to a third party. A site that has none is asked again after a week.
//! "Website icons" in Appearance turns this off.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

const RETRY_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);
const MAX_ICON: u64 = 1024 * 1024;
static FETCHING: AtomicBool = AtomicBool::new(false);

fn dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("tendril").join("sites")
}

/// The host of a web address, template or not: `https://github.com/{raw}` is github.com.
pub fn host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next()?.to_lowercase();
    let ok = !host.is_empty() && host.contains('.') && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    ok.then_some(host)
}

/// The kept icon for this address's site, if there is one.
pub fn icon(url: &str) -> Option<PathBuf> {
    let path = dir().join(format!("{}.png", host(url)?));
    path.is_file().then_some(path)
}

/// Whether this site still needs asking: no icon kept, and not asked in the last week.
fn wanted(host: &str) -> bool {
    let base = dir();
    if base.join(format!("{host}.png")).is_file() {
        return false;
    }
    let tried = std::fs::metadata(base.join(format!("{host}.none"))).and_then(|m| m.modified()).ok();
    tried.is_none_or(|t| SystemTime::now().duration_since(t).unwrap_or_default() > RETRY_AFTER)
}

/// Fetches icons for any of these addresses' sites that don't have one yet, then calls `done`
/// on the UI thread if any arrived.
pub fn fetch(urls: &[String], done: impl FnOnce() + Send + 'static) {
    let mut hosts: Vec<String> = urls.iter().filter_map(|u| host(u)).filter(|h| wanted(h)).collect();
    hosts.sort();
    hosts.dedup();
    if hosts.is_empty() || FETCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("site-icons".into()).spawn(move || {
        let _ = std::fs::create_dir_all(dir());
        let mut any = false;
        for host in hosts {
            match get(&host) {
                Some(png) => any |= std::fs::write(dir().join(format!("{host}.png")), png).is_ok(),
                None => {
                    let _ = std::fs::write(dir().join(format!("{host}.none")), b"");
                }
            }
        }
        FETCHING.store(false, Ordering::SeqCst);
        if any {
            let _ = slint::invoke_from_event_loop(done);
        }
    });
    if spawned.is_err() {
        FETCHING.store(false, Ordering::SeqCst);
    }
}

/// The site's icon as a PNG, at most 64px.
fn get(host: &str) -> Option<Vec<u8>> {
    let agent = crate::update::agent();
    ["apple-touch-icon.png", "favicon.ico"].iter().find_map(|file| {
        let mut response = agent.get(&format!("https://{host}/{file}")).call().ok()?;
        let bytes = response.body_mut().with_config().limit(MAX_ICON).read_to_vec().ok()?;
        shrink(&bytes)
    })
}

/// Decodes a PNG or ICO and keeps it at 64px or less, as PNG.
pub fn shrink(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    if image.width() < 16 || image.height() < 16 {
        return None;
    }
    let image = image.thumbnail(64, 64);
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(host("https://github.com/{raw}").as_deref(), Some("github.com"));
        assert_eq!(host("http://user@Example.org:8080/x?q").as_deref(), Some("example.org"));
        assert_eq!(host("https://{query}.example.com"), None);
        assert_eq!(host("~/Documents"), None);
    }

    #[test]
    fn shrinks() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(180, 180, image::Rgba([20, 20, 20, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let small = shrink(png.get_ref()).unwrap();
        let back = image::load_from_memory(&small).unwrap();
        assert_eq!((back.width(), back.height()), (64, 64));
        assert!(shrink(b"not an image").is_none());
    }
}
