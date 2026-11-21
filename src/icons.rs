//! Icons for the list, loaded the first time a row needs one.
//!
//! Tendril's own glyphs and SVG app icons are vectors, drawn sharp at any size. Everything else
//! (PNG and ICO files, Windows' shell icons, macOS .icns) is loaded on a thread of its own, so
//! typing never waits on the disk or the shell, and scaled there to exactly the pixels it's
//! shown at with a proper filter: the CPU renderer samples images without smoothing, so one
//! drawn at any other size looks ragged or blocky. Every app's icon is made ahead in the
//! background (App::warm_up); a row whose icon isn't in yet shows a blank square for a frame or
//! two. Kept that small, a few hundred icons cost about a megabyte.

use crate::catalog::Icon;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};
/// The neutral tint, used when an app has no icon of its own and gets the generic one.
const FALLBACK_TINT: i32 = 7;

/// Tendril's own icons (scripts/icons.py draws them): name, outline, and solid for the selected row.
const BUILTIN: &[(&str, &[u8], &[u8])] = &[
    ("tendril", include_bytes!("../ui/icons/tendril.svg"), include_bytes!("../ui/icons/tendril-solid.svg")),
    ("app", include_bytes!("../ui/icons/app.svg"), include_bytes!("../ui/icons/app-solid.svg")),
    ("lock", include_bytes!("../ui/icons/lock.svg"), include_bytes!("../ui/icons/lock-solid.svg")),
    ("moon", include_bytes!("../ui/icons/moon.svg"), include_bytes!("../ui/icons/moon-solid.svg")),
    ("logout", include_bytes!("../ui/icons/logout.svg"), include_bytes!("../ui/icons/logout-solid.svg")),
    ("restart", include_bytes!("../ui/icons/restart.svg"), include_bytes!("../ui/icons/restart-solid.svg")),
    ("power", include_bytes!("../ui/icons/power.svg"), include_bytes!("../ui/icons/power-solid.svg")),
    ("settings", include_bytes!("../ui/icons/settings.svg"), include_bytes!("../ui/icons/settings-solid.svg")),
    ("refresh", include_bytes!("../ui/icons/refresh.svg"), include_bytes!("../ui/icons/refresh-solid.svg")),
    ("palette", include_bytes!("../ui/icons/palette.svg"), include_bytes!("../ui/icons/palette-solid.svg")),
    ("swatch", include_bytes!("../ui/icons/swatch.svg"), include_bytes!("../ui/icons/swatch-solid.svg")),
    ("quit", include_bytes!("../ui/icons/quit.svg"), include_bytes!("../ui/icons/quit-solid.svg")),
    ("globe", include_bytes!("../ui/icons/globe.svg"), include_bytes!("../ui/icons/globe-solid.svg")),
    ("link", include_bytes!("../ui/icons/link.svg"), include_bytes!("../ui/icons/link-solid.svg")),
    ("calculator", include_bytes!("../ui/icons/calculator.svg"), include_bytes!("../ui/icons/calculator-solid.svg")),
    ("copy", include_bytes!("../ui/icons/copy.svg"), include_bytes!("../ui/icons/copy-solid.svg")),
    ("file", include_bytes!("../ui/icons/file.svg"), include_bytes!("../ui/icons/file-solid.svg")),
    ("folder", include_bytes!("../ui/icons/folder.svg"), include_bytes!("../ui/icons/folder-solid.svg")),
    ("open", include_bytes!("../ui/icons/open.svg"), include_bytes!("../ui/icons/open-solid.svg")),
    ("eraser", include_bytes!("../ui/icons/eraser.svg"), include_bytes!("../ui/icons/eraser-solid.svg")),
    ("download", include_bytes!("../ui/icons/download.svg"), include_bytes!("../ui/icons/download-solid.svg")),
    ("arrow", include_bytes!("../ui/icons/arrow.svg"), include_bytes!("../ui/icons/arrow-solid.svg")),
    ("check", include_bytes!("../ui/icons/check.svg"), include_bytes!("../ui/icons/check-solid.svg")),
    ("clipboard", include_bytes!("../ui/icons/clipboard.svg"), include_bytes!("../ui/icons/clipboard-solid.svg")),
    ("windows", include_bytes!("../ui/icons/windows.svg"), include_bytes!("../ui/icons/windows-solid.svg")),
    ("flask", include_bytes!("../ui/icons/flask.svg"), include_bytes!("../ui/icons/flask-solid.svg")),
    ("blur", include_bytes!("../ui/icons/blur.svg"), include_bytes!("../ui/icons/blur-solid.svg")),
    ("window", include_bytes!("../ui/icons/window.svg"), include_bytes!("../ui/icons/window-solid.svg")),
    ("opacity", include_bytes!("../ui/icons/opacity.svg"), include_bytes!("../ui/icons/opacity-solid.svg")),
    ("sparkle", include_bytes!("../ui/icons/sparkle.svg"), include_bytes!("../ui/icons/sparkle-solid.svg")),
    ("gradient", include_bytes!("../ui/icons/gradient.svg"), include_bytes!("../ui/icons/gradient-solid.svg")),
    ("resize", include_bytes!("../ui/icons/resize.svg"), include_bytes!("../ui/icons/resize-solid.svg")),
    ("columns", include_bytes!("../ui/icons/columns.svg"), include_bytes!("../ui/icons/columns-solid.svg")),
    ("star", include_bytes!("../ui/icons/star.svg"), include_bytes!("../ui/icons/star-solid.svg")),
    ("number", include_bytes!("../ui/icons/number.svg"), include_bytes!("../ui/icons/number-solid.svg")),
    ("heading", include_bytes!("../ui/icons/heading.svg"), include_bytes!("../ui/icons/heading-solid.svg")),
    ("pointer", include_bytes!("../ui/icons/pointer.svg"), include_bytes!("../ui/icons/pointer-solid.svg")),
    ("login", include_bytes!("../ui/icons/login.svg"), include_bytes!("../ui/icons/login-solid.svg")),
    ("wave", include_bytes!("../ui/icons/wave.svg"), include_bytes!("../ui/icons/wave-solid.svg")),
    ("volume", include_bytes!("../ui/icons/volume.svg"), include_bytes!("../ui/icons/volume-solid.svg")),
    ("contrast", include_bytes!("../ui/icons/contrast.svg"), include_bytes!("../ui/icons/contrast-solid.svg")),
    ("trash", include_bytes!("../ui/icons/trash.svg"), include_bytes!("../ui/icons/trash-solid.svg")),
];

/// Point sizes icons are shown at: a list row, a favourite tile, the details pane.
pub const ROW: f32 = 20.0;
pub const TILE: f32 = 40.0;
pub const DETAIL: f32 = 56.0;
/// The most a picture's preview in the details pane can be, either way.
pub const PREVIEW: f32 = 200.0;

/// Pixels decoded on the icon thread, waiting to become an Image on the UI thread. Premultiplied.
pub struct Pixels {
    key: String,
    rgba: Option<(u32, u32, Vec<u8>)>,
}

struct Request {
    key: String,
    icon: Icon,
    px: u32,
    /// An icon is made exactly `px` across; a picture's preview at most that.
    exact: bool,
    /// Made ahead (warm), with no row waiting on it: goes after everything a row is waiting on.
    ahead: bool,
}

#[derive(Default)]
pub struct Icons {
    loaded: HashMap<String, Option<Image>>,
    /// Asked for on the icon thread and not back yet, and whether only ahead of time.
    waiting: HashMap<String, bool>,
    worker: Option<mpsc::Sender<Request>>,
    /// The window's scale: 1.5 at 150%, so a 20pt row icon is 30 pixels.
    scale: f32,
}

impl Icons {
    pub fn builtin(&mut self, name: &str) -> Image {
        self.drawn(name, false)
    }

    pub fn builtin_solid(&mut self, name: &str) -> Image {
        self.drawn(name, true)
    }

    fn drawn(&mut self, name: &str, solid: bool) -> Image {
        self.loaded
            .entry(format!("builtin:{name}:{solid}"))
            .or_insert_with(|| {
                let (_, outline, filled) = BUILTIN.iter().find(|(n, _, _)| *n == name).unwrap_or(&BUILTIN[1]);
                Image::load_from_svg_data(if solid { filled } else { outline }).ok()
            })
            .clone()
            .unwrap_or_default()
    }

    /// The window's scale changed (it opened on another screen): pixel sizes change with it.
    /// Icons loaded for the other scale are kept, for when it's back there. Returns whether the
    /// pixels change, so the rows can ask again.
    pub fn set_scale(&mut self, scale: f32) -> bool {
        if scale <= 0.0 {
            return false;
        }
        let before = self.scale();
        self.scale = scale;
        (scale - before).abs() > f32::EPSILON
    }

    pub fn has_scale(&self) -> bool {
        self.scale > 0.0
    }

    /// Until it's known, icons are made at 1.
    fn scale(&self) -> f32 {
        if self.scale > 0.0 { self.scale } else { 1.0 }
    }

    fn pixels(&self, points: f32) -> u32 {
        (points * self.scale()).round().max(1.0) as u32
    }

    /// The cache key for an icon loaded as pixels at this size, or None for one drawn as a vector.
    pub fn key(&self, icon: &Icon, points: f32) -> Option<String> {
        let px = self.pixels(points);
        match icon {
            Icon::File(path) if !is_vector(path) => Some(format!("{}@{px}", path.display())),
            Icon::Shell(name) => Some(format!("{name}@{px}")),
            _ => None,
        }
    }

    /// The image for a row, its solid for when the row is selected, and the tint to draw it
    /// with. An app's own icon is the same either way. `points` is the size it's shown at.
    pub fn get(&mut self, icon: &Icon, tint: i32, points: f32) -> (Image, Image, i32) {
        let loaded = match icon {
            Icon::None => None,
            Icon::Builtin(name) => return (self.builtin(name), self.builtin_solid(name), tint),
            Icon::File(path) if is_vector(path) => {
                let key = path.to_string_lossy().into_owned();
                self.loaded.entry(key).or_insert_with(|| Image::load_from_path(path).ok()).clone()
            }
            Icon::File(_) | Icon::Shell(_) => {
                let key = self.key(icon, points).expect("pixels have a key");
                match self.loaded.get(&key) {
                    Some(found) => found.clone(),
                    None => {
                        self.request(key, icon.clone(), self.pixels(points), true, false);
                        // Blank until it arrives; no square behind it either.
                        return (Image::default(), Image::default(), 0);
                    }
                }
            }
        };
        match loaded {
            Some(image) => (image.clone(), image, 0),
            None => (self.builtin("app"), self.builtin_solid("app"), FALLBACK_TINT),
        }
    }

    /// A picture file at most `PREVIEW` points either way, and its size in points; None until it
    /// arrives from the icon thread, or if it isn't a picture Tendril can read.
    pub fn preview(&mut self, path: &Path) -> Option<(Image, f32, f32)> {
        let icon = Icon::File(path.to_path_buf());
        let key = self.key(&icon, PREVIEW)?;
        match self.loaded.get(&key) {
            Some(found) => {
                let image = found.clone()?;
                let size = image.size();
                Some((image, size.width as f32 / self.scale(), size.height as f32 / self.scale()))
            }
            None => {
                self.request(key, icon, self.pixels(PREVIEW), false, false);
                None
            }
        }
    }

    /// Loads these icons at `points` in the background, if they aren't already, so they're
    /// there the first time a row shows them rather than arriving a moment after. They wait
    /// behind anything a row is waiting on.
    pub fn warm<'a>(&mut self, icons: impl IntoIterator<Item = &'a Icon>, points: f32) {
        if self.worker.is_none() {
            self.worker = start_worker();
        }
        // Without the thread they'd all load here, holding up the window.
        if self.worker.is_none() {
            return;
        }
        for icon in icons {
            if let Some(key) = self.key(icon, points)
                && !self.loaded.contains_key(&key)
            {
                self.request(key, icon.clone(), self.pixels(points), true, true);
            }
        }
    }

    fn request(&mut self, key: String, icon: Icon, px: u32, exact: bool, ahead: bool) {
        match self.waiting.get(&key) {
            // A row now waits on one only asked for ahead: ask again, to go first.
            Some(true) if !ahead => {}
            Some(_) => return,
            None => {}
        }
        self.waiting.insert(key.clone(), ahead);
        if self.worker.is_none() {
            self.worker = start_worker();
        }
        let sent = self.worker.as_ref().is_some_and(|w| w.send(Request { key: key.clone(), icon: icon.clone(), px, exact, ahead }).is_ok());
        if !sent {
            // No thread: load it here instead, as before.
            self.waiting.remove(&key);
            let rgba = load_pixels(&icon, px, exact);
            self.loaded.insert(key, rgba.and_then(|(w, h, p)| from_rgba(w, h, p)));
        }
    }

    /// Icons back from the thread. Returns the keys that arrived, for the rows to redraw.
    pub fn arrived(&mut self, batch: Vec<Pixels>) -> HashSet<String> {
        let mut keys = HashSet::new();
        for Pixels { key, rgba } in batch {
            self.waiting.remove(&key);
            self.loaded.insert(key.clone(), rgba.and_then(|(w, h, p)| from_rgba(w, h, p)));
            keys.insert(key);
        }
        keys
    }
}

fn is_vector(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"))
}

/// How long icons made ahead are held before being handed back together. Nothing on screen waits
/// on them (a row that wants one asks again, to the front), so seldom: each hand-back wakes the
/// window's thread, closed or not.
const AHEAD_BATCH: Duration = Duration::from_secs(1);

/// The icon thread: loads what rows are waiting on first, then icons made ahead. What rows are
/// waiting on is handed back as soon as it's all loaded, so a screenful arrives as one redraw
/// rather than thirty; icons made ahead every so often.
fn start_worker() -> Option<mpsc::Sender<Request>> {
    let (tx, rx) = mpsc::channel::<Request>();
    std::thread::Builder::new()
        .name("icons".into())
        .stack_size(1024 * 1024)
        .spawn(move || {
            let (mut now, mut ahead) = (VecDeque::new(), VecDeque::new());
            loop {
                if now.is_empty() && ahead.is_empty() {
                    let Ok(r) = rx.recv() else { return };
                    queue(r, &mut now, &mut ahead);
                }
                let (started, mut waited_on, mut batch) = (Instant::now(), false, Vec::new());
                loop {
                    for r in rx.try_iter() {
                        queue(r, &mut now, &mut ahead);
                    }
                    let Some(r) = now.pop_front().or_else(|| ahead.pop_front()) else { break };
                    waited_on |= !r.ahead;
                    batch.push(Pixels { key: r.key, rgba: load_pixels(&r.icon, r.px, r.exact) });
                    if now.is_empty() && (waited_on || started.elapsed() > AHEAD_BATCH) {
                        break;
                    }
                }
                let _ = slint::invoke_from_event_loop(move || crate::app::with_app(|a| a.icons_arrived(batch)));
            }
        })
        .ok()?;
    Some(tx)
}

fn queue(r: Request, now: &mut VecDeque<Request>, ahead: &mut VecDeque<Request>) {
    if r.ahead {
        ahead.push_back(r);
    } else {
        // Also asked for ahead: no need to make it twice.
        ahead.retain(|a| a.key != r.key);
        now.push_back(r);
    }
}

/// An icon as premultiplied RGBA, `px` across its longer side (at most that if not `exact`).
fn load_pixels(icon: &Icon, px: u32, exact: bool) -> Option<(u32, u32, Vec<u8>)> {
    let ((w, h, rgba), premultiplied) = match icon {
        Icon::File(path) => (load_file(path, px)?, false),
        Icon::Shell(name) => (load_shell(name, px)?, true),
        _ => return None,
    };
    fit(w, h, rgba, premultiplied, px, exact)
}

fn load_file(path: &Path, px: u32) -> Option<(u32, u32, Vec<u8>)> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    #[cfg(target_os = "macos")]
    if ext == "icns" {
        return crate::apps::icns(path, px);
    }
    let _ = (&ext, px);
    let image = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let rgba = image.into_rgba8();
    let (w, h) = rgba.dimensions();
    Some((w, h, rgba.into_raw()))
}

/// Asked for at twice the size: the shell hands back the next size up it has, and scaling 32
/// down to 30, say, blurs it, where scaling 64 or 256 down stays crisp.
#[cfg(windows)]
fn load_shell(name: &str, px: u32) -> Option<(u32, u32, Vec<u8>)> {
    crate::apps::icon(name, (px * 2).min(256))
}

#[cfg(not(windows))]
fn load_shell(_: &str, _: u32) -> Option<(u32, u32, Vec<u8>)> {
    None
}

/// Scales to fit `px` square, keeping the shape: exactly that big when `exact`, since the CPU
/// renderer stretches without smoothing (a 16px favicon in a 30px square came out blocky), and
/// otherwise never bigger than it is. Done on premultiplied pixels: see-through pixels are often
/// white underneath, and filtering them straight bled that white into the edges.
fn fit(w: u32, h: u32, mut rgba: Vec<u8>, premultiplied: bool, px: u32, exact: bool) -> Option<(u32, u32, Vec<u8>)> {
    if rgba.len() != (w as usize) * (h as usize) * 4 || w == 0 || h == 0 {
        return None;
    }
    if !premultiplied {
        for p in rgba.as_chunks_mut::<4>().0 {
            let a = u16::from(p[3]);
            for c in &mut p[..3] {
                *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
        }
    }
    let ratio = (px as f32 / w as f32).min(px as f32 / h as f32);
    let (nw, nh) = (((w as f32 * ratio).round() as u32).max(1), ((h as f32 * ratio).round() as u32).max(1));
    if (nw, nh) == (w, h) || (ratio > 1.0 && !exact) {
        return Some((w, h, rgba));
    }
    use image::imageops::FilterType;
    let filter = if ratio < 1.0 { FilterType::Lanczos3 } else { FilterType::CatmullRom };
    let mut out = image::imageops::resize(&image::RgbaImage::from_raw(w, h, rgba)?, nw, nh, filter).into_raw();
    // The filter overshoots at hard edges: colour brighter than its own alpha shows as a light fringe.
    for p in out.as_chunks_mut::<4>().0 {
        let a = p[3];
        for c in &mut p[..3] {
            *c = (*c).min(a);
        }
    }
    Some((nw, nh, out))
}

/// `fit` for a picture kept as a file: never bigger, and back to straight alpha, as PNG keeps it.
pub fn shrink(w: u32, h: u32, rgba: Vec<u8>, px: u32) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h, mut rgba) = fit(w, h, rgba, false, px, false)?;
    for p in rgba.as_chunks_mut::<4>().0 {
        let a = u16::from(p[3]);
        if a > 0 {
            for c in &mut p[..3] {
                *c = ((u16::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Some((w, h, rgba))
}

fn from_rgba(w: u32, h: u32, rgba: Vec<u8>) -> Option<Image> {
    if rgba.len() != (w as usize) * (h as usize) * 4 || w == 0 || h == 0 {
        return None;
    }
    Some(Image::from_rgba8_premultiplied(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, w, h)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_to_the_pixels_shown() {
        let big = vec![255u8; 256 * 128 * 4];
        let (w, h, p) = fit(256, 128, big, false, 30, true).unwrap();
        assert_eq!((w, h, p.len()), (30, 15, 30 * 15 * 4));
        // An icon grows to its square; a preview doesn't.
        assert_eq!(fit(16, 16, vec![255u8; 16 * 16 * 4], false, 30, true).map(|(w, h, _)| (w, h)), Some((30, 30)));
        assert_eq!(fit(16, 16, vec![255u8; 16 * 16 * 4], false, 30, false).map(|(w, h, _)| (w, h)), Some((16, 16)));
        assert!(fit(4, 4, vec![0; 3], false, 30, true).is_none());
    }

    #[test]
    fn no_light_fringe() {
        // An opaque black square on see-through white, the way many PNGs store it.
        let mut rgba = Vec::new();
        for y in 0..64 {
            for x in 0..64 {
                let inside = (16..48).contains(&x) && (16..48).contains(&y);
                rgba.extend(if inside { [0, 0, 0, 255] } else { [255, 255, 255, 0] });
            }
        }
        let (_, _, out) = fit(64, 64, rgba.clone(), false, 20, true).unwrap();
        for p in out.as_chunks::<4>().0 {
            assert!(p[..3].iter().all(|&c| c <= p[3]), "{p:?} is brighter than its alpha");
            assert!(p[..3].iter().all(|&c| c < 8), "{p:?}: white from the see-through part bled in");
        }
        // Kept as a file, it's straight alpha again: the square's edge stays black, not grey.
        let (w, h, kept) = shrink(64, 64, rgba, 20).unwrap();
        assert_eq!((w, h), (20, 20));
        for p in kept.as_chunks::<4>().0.iter().filter(|p| p[3] > 32) {
            assert!(p[..3].iter().all(|&c| c < 16), "{p:?}: white bled into the kept file");
        }
    }
}
