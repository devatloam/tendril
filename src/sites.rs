//! Websites' own icons for your quicklinks, so "GitHub repository" shows GitHub's mark.
//!
//! Fetched once per site, from the site itself, the first time a quicklink to it is listed:
//! its apple-touch-icon, else the biggest icon its home page names (an SVG is kept as it is),
//! else its favicon. Several sites are asked at once, each given a few seconds. Kept in the
//! cache folder at 128px, enough for the details pane at 200%, and looked up from memory after
//! that, so listing a quicklink never touches the disk. Nothing goes to a third party. A site
//! that has none is asked again after a week. "Website icons" in Appearance turns this off.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

const RETRY_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);
const MAX_ICON: u64 = 1024 * 1024;
const MAX_PAGE: u64 = 512 * 1024;
/// Kept at most this big; bigger ones are scaled down.
const SIZE: u32 = 128;
/// An icon this big is good enough to stop looking.
const GOOD_ENOUGH: u32 = 96;
/// Sites asked at the same time.
const AT_ONCE: usize = 6;
/// How long one site gets, all its requests together.
const LIMIT: Duration = Duration::from_secs(6);

static FETCHING: AtomicBool = AtomicBool::new(false);
/// Host → its kept icon, read from the folder the first time it's needed.
static KNOWN: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

pub fn dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("tendril").join("site-icons")
}

/// Where 0.2 and earlier kept them, at 64px. Cleared once, so they're fetched again sharper.
fn old_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("tendril").join("sites")
}

fn with_known<T>(f: impl FnOnce(&mut HashMap<String, PathBuf>) -> T) -> T {
    let mut known = KNOWN.lock().unwrap();
    let map = known.get_or_insert_with(|| {
        let mut map = HashMap::new();
        for entry in std::fs::read_dir(dir()).into_iter().flatten().flatten() {
            let path = entry.path();
            let kept = path.extension().is_some_and(|e| e == "png" || e == "svg");
            if let (true, Some(stem)) = (kept, path.file_stem()) {
                map.insert(stem.to_string_lossy().into_owned(), path);
            }
        }
        map
    });
    f(map)
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
    let host = host(url)?;
    with_known(|known| known.get(&host).cloned())
}

/// Deletes every kept icon. Quicklinks get theirs again the next time they're listed.
pub fn forget() -> std::io::Result<()> {
    *KNOWN.lock().unwrap() = None;
    match std::fs::remove_dir_all(dir()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Whether this site still needs asking: no icon kept, and not asked in the last week.
fn wanted(host: &str) -> bool {
    if with_known(|known| known.contains_key(host)) {
        return false;
    }
    let tried = std::fs::metadata(dir().join(format!("{host}.none"))).and_then(|m| m.modified()).ok();
    tried.is_none_or(|t| SystemTime::now().duration_since(t).unwrap_or_default() > RETRY_AFTER)
}

/// Fetches icons for any of these addresses' sites that don't have one yet, a few sites at a
/// time, then calls `done` on the UI thread if any arrived.
pub fn fetch(urls: &[String], done: impl FnOnce() + Send + 'static) {
    let mut hosts: Vec<String> = urls.iter().filter_map(|u| host(u)).filter(|h| wanted(h)).collect();
    hosts.sort();
    hosts.dedup();
    if hosts.is_empty() || FETCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("site-icons".into()).spawn(move || {
        let _ = std::fs::remove_dir_all(old_dir());
        let _ = std::fs::create_dir_all(dir());
        let mut any = false;
        for chunk in hosts.chunks(AT_ONCE) {
            let found: Vec<(&String, Option<Fetched>)> = std::thread::scope(|s| {
                let asked: Vec<_> = chunk.iter().map(|h| (h, s.spawn(move || get(h)))).collect();
                asked.into_iter().map(|(h, t)| (h, t.join().ok().flatten())).collect()
            });
            for (host, fetched) in found {
                let (bytes, ext) = match fetched {
                    Some(Fetched::Png(bytes)) => (bytes, "png"),
                    Some(Fetched::Svg(bytes)) => (bytes, "svg"),
                    None => {
                        let _ = std::fs::write(dir().join(format!("{host}.none")), b"");
                        continue;
                    }
                };
                let path = dir().join(format!("{host}.{ext}"));
                if std::fs::write(&path, bytes).is_ok() {
                    with_known(|known| known.insert(host.clone(), path));
                    any = true;
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

enum Fetched {
    Png(Vec<u8>),
    Svg(Vec<u8>),
}

fn download(agent: &ureq::Agent, url: &str, limit: u64) -> Option<Vec<u8>> {
    let mut response = agent.get(url).call().ok()?;
    response.body_mut().with_config().limit(limit).read_to_vec().ok()
}

/// The site's best icon: big enough to stay sharp, or the biggest it has.
fn get(host: &str) -> Option<Fetched> {
    let agent = crate::update::agent_with(LIMIT);
    let mut best: Option<(u32, Vec<u8>)> = None;
    let mut consider = |bytes: &[u8]| {
        if let Some((size, png)) = shrink(bytes)
            && best.as_ref().is_none_or(|b| size > b.0)
        {
            best = Some((size, png));
        }
        best.as_ref().is_some_and(|b| b.0 >= GOOD_ENOUGH)
    };
    // Usually 180px, and one request.
    if let Some(bytes) = download(&agent, &format!("https://{host}/apple-touch-icon.png"), MAX_ICON)
        && consider(&bytes)
    {
        return best.map(|(_, png)| Fetched::Png(png));
    }
    if let Some(page) = download(&agent, &format!("https://{host}/"), MAX_PAGE) {
        for link in icon_links(&String::from_utf8_lossy(&page), host) {
            let Some(bytes) = download(&agent, &link.url, MAX_ICON) else { continue };
            if link.svg {
                if is_svg(&bytes) {
                    return Some(Fetched::Svg(bytes));
                }
            } else if consider(&bytes) {
                return best.map(|(_, png)| Fetched::Png(png));
            }
        }
    }
    if let Some(bytes) = download(&agent, &format!("https://{host}/favicon.ico"), MAX_ICON) {
        consider(&bytes);
    }
    best.map(|(_, png)| Fetched::Png(png))
}

fn is_svg(bytes: &[u8]) -> bool {
    let start = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_lowercase();
    (start.contains("<svg") || start.contains("<?xml")) && slint::Image::load_from_svg_data(bytes).is_ok()
}

/// Decodes a PNG or ICO (an ICO's biggest picture) and keeps it at 128px or less, as PNG.
/// Returns its size before shrinking too.
fn shrink(bytes: &[u8]) -> Option<(u32, Vec<u8>)> {
    let image = image::load_from_memory(bytes).ok()?;
    let size = image.width().min(image.height());
    if size < 16 {
        return None;
    }
    let image =
        if image.width() > SIZE || image.height() > SIZE { image.resize(SIZE, SIZE, image::imageops::FilterType::Lanczos3) } else { image };
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some((size, out.into_inner()))
}

struct Link {
    url: String,
    size: u32,
    svg: bool,
}

/// The icons a page's `<link rel="icon">` tags name, best first: SVGs, then biggest.
fn icon_links(html: &str, host: &str) -> Vec<Link> {
    let lower = html.to_ascii_lowercase();
    let mut links = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<link") {
        let start = from + at;
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        let tag = &html[start..end];
        from = end;
        let rel = attr(tag, "rel").unwrap_or_default().to_lowercase();
        // mask-icon is a one-colour outline for Safari's pinned tabs.
        if !rel.split_whitespace().any(|r| r == "icon" || r == "apple-touch-icon") {
            continue;
        }
        let Some(href) = attr(tag, "href") else { continue };
        let url = resolve(&href.replace("&amp;", "&"), host);
        let kind = attr(tag, "type").unwrap_or_default().to_lowercase();
        let svg = kind.contains("svg") || url.split(['?', '#']).next().is_some_and(|u| u.to_lowercase().ends_with(".svg"));
        let size = attr(tag, "sizes").and_then(|s| s.split(['x', 'X']).next().and_then(|n| n.trim().parse().ok())).unwrap_or(0);
        links.push(Link { url, size, svg });
    }
    links.sort_by(|a, b| b.svg.cmp(&a.svg).then(b.size.cmp(&a.size)));
    links
}

/// An attribute's value in a tag, quoted or not.
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        if at == 0 || !bytes[at - 1].is_ascii_whitespace() {
            continue;
        }
        let mut j = from;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'=' {
            continue;
        }
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        let value = match bytes.get(j) {
            Some(&q @ (b'"' | b'\'')) => {
                let close = lower[j + 1..].find(q as char).map_or(tag.len(), |k| j + 1 + k);
                &tag[j + 1..close]
            }
            Some(_) => {
                // At the end of the tag, a "/" is the one in "/>", not part of the value.
                match lower[j..].find(char::is_whitespace) {
                    Some(k) => &tag[j..j + k],
                    None => tag[j..].trim_end_matches('/'),
                }
            }
            None => continue,
        };
        return Some(value.trim().to_string());
    }
    None
}

/// A link from a page on `host` as a whole address.
fn resolve(href: &str, host: &str) -> String {
    if href.starts_with("https://") || href.starts_with("http://") {
        href.to_string()
    } else if let Some(rest) = href.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = href.strip_prefix('/') {
        format!("https://{host}/{rest}")
    } else {
        format!("https://{host}/{href}")
    }
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
        let (size, small) = shrink(png.get_ref()).unwrap();
        assert_eq!(size, 180);
        let back = image::load_from_memory(&small).unwrap();
        assert_eq!((back.width(), back.height()), (128, 128));
        assert!(shrink(b"not an image").is_none());
    }

    #[test]
    fn reads_icon_links() {
        let html = r#"<head><link rel="stylesheet" href="/a.css">
            <LINK REL="icon" sizes="32x32" href="/favicon-32.png">
            <link rel='apple-touch-icon' sizes=180x180 href=https://cdn.example.com/touch.png>
            <link rel="mask-icon" href="/mask.svg">
            <link rel="icon" type="image/svg+xml" href="icon.svg?v=2&amp;x=1"></head>"#;
        let links = icon_links(html, "example.com");
        let urls: Vec<&str> = links.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(
            urls,
            ["https://example.com/icon.svg?v=2&x=1", "https://cdn.example.com/touch.png", "https://example.com/favicon-32.png"]
        );
        assert_eq!(links[1].size, 180);
        assert_eq!(resolve("//cdn.x.org/i.png", "x.org"), "https://cdn.x.org/i.png");
    }
}
