//! Icons for the list, loaded the first time a row needs one.
//!
//! Tendril's own glyphs and SVG app icons are vectors, drawn sharp at any size. Everything else
//! (PNG and ICO files, Windows' shell icons, macOS .icns) is loaded on a thread of its own, so
//! typing never waits on the disk or the shell, and scaled there to exactly the pixels it's
//! shown at with a proper filter: the CPU renderer samples images without smoothing, so one
//! drawn smaller than it is looks ragged. A row shows a blank square until its icon arrives,
//! usually within a frame or two. Kept that small, a few hundred icons cost about a megabyte.

use crate::catalog::Icon;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc;
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

/// Pixels decoded on the icon thread, waiting to become an Image on the UI thread.
pub struct Pixels {
    key: String,
    rgba: Option<(u32, u32, Vec<u8>, bool)>,
}

struct Request {
    key: String,
    icon: Icon,
    px: u32,
}

#[derive(Default)]
pub struct Icons {
    loaded: HashMap<String, Option<Image>>,
    /// Asked for on the icon thread and not back yet.
    waiting: HashSet<String>,
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
    pub fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale).abs() > f32::EPSILON {
            self.scale = scale;
            self.loaded.retain(|k, _| !k.contains('@'));
        }
    }

    fn pixels(&self, points: f32) -> u32 {
        (points * if self.scale > 0.0 { self.scale } else { 1.0 }).round().max(1.0) as u32
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
                        self.request(key, icon.clone(), self.pixels(points));
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
                let scale = if self.scale > 0.0 { self.scale } else { 1.0 };
                Some((image, size.width as f32 / scale, size.height as f32 / scale))
            }
            None => {
                self.request(key, icon, self.pixels(PREVIEW));
                None
            }
        }
    }

    fn request(&mut self, key: String, icon: Icon, px: u32) {
        if !self.waiting.insert(key.clone()) {
            return;
        }
        if self.worker.is_none() {
            self.worker = start_worker();
        }
        let sent = self.worker.as_ref().is_some_and(|w| w.send(Request { key: key.clone(), icon: icon.clone(), px }).is_ok());
        if !sent {
            // No thread: load it here instead, as before.
            self.waiting.remove(&key);
            let rgba = load_pixels(&icon, px);
            self.loaded.insert(key, rgba.and_then(|(w, h, p, pre)| from_rgba(w, h, p, pre)));
        }
    }

    /// Icons back from the thread. Returns the keys that arrived, for the rows to redraw.
    pub fn arrived(&mut self, batch: Vec<Pixels>) -> HashSet<String> {
        let mut keys = HashSet::new();
        for Pixels { key, rgba } in batch {
            self.waiting.remove(&key);
            self.loaded.insert(key.clone(), rgba.and_then(|(w, h, p, pre)| from_rgba(w, h, p, pre)));
            keys.insert(key);
        }
        keys
    }
}

fn is_vector(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("svgz"))
}

/// The icon thread: loads what's asked, and hands back everything loaded each time it runs out
/// of work, so a screenful of icons arrives as one redraw rather than thirty.
fn start_worker() -> Option<mpsc::Sender<Request>> {
    let (tx, rx) = mpsc::channel::<Request>();
    std::thread::Builder::new()
        .name("icons".into())
        .stack_size(1024 * 1024)
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut batch = Vec::new();
                let mut next = Some(first);
                while let Some(Request { key, icon, px }) = next {
                    batch.push(Pixels { key, rgba: load_pixels(&icon, px) });
                    next = rx.try_recv().ok();
                }
                let _ = slint::invoke_from_event_loop(move || crate::app::with_app(|a| a.icons_arrived(batch)));
            }
        })
        .ok()?;
    Some(tx)
}

/// An icon as RGBA at `px` square (or less, keeping its shape), and whether it's premultiplied.
fn load_pixels(icon: &Icon, px: u32) -> Option<(u32, u32, Vec<u8>, bool)> {
    let (w, h, rgba, premultiplied) = match icon {
        Icon::File(path) => {
            let (w, h, rgba) = load_file(path, px)?;
            (w, h, rgba, false)
        }
        Icon::Shell(name) => {
            let (w, h, rgba) = load_shell(name, px)?;
            (w, h, rgba, true)
        }
        _ => return None,
    };
    let (w, h, rgba) = fit(w, h, rgba, px)?;
    Some((w, h, rgba, premultiplied))
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

#[cfg(windows)]
fn load_shell(name: &str, px: u32) -> Option<(u32, u32, Vec<u8>)> {
    crate::apps::icon(name, px)
}

#[cfg(not(windows))]
fn load_shell(_: &str, _: u32) -> Option<(u32, u32, Vec<u8>)> {
    None
}

/// Scales to fit `px` square with a Lanczos filter, keeping the shape. Never scales up: a small
/// icon stays small and sharp rather than growing blurry.
fn fit(w: u32, h: u32, rgba: Vec<u8>, px: u32) -> Option<(u32, u32, Vec<u8>)> {
    if rgba.len() != (w as usize) * (h as usize) * 4 || w == 0 || h == 0 {
        return None;
    }
    if w <= px && h <= px {
        return Some((w, h, rgba));
    }
    let ratio = (px as f32 / w as f32).min(px as f32 / h as f32);
    let (nw, nh) = (((w as f32 * ratio).round() as u32).max(1), ((h as f32 * ratio).round() as u32).max(1));
    let full = image::RgbaImage::from_raw(w, h, rgba)?;
    let small = image::imageops::resize(&full, nw, nh, image::imageops::FilterType::Lanczos3);
    Some((nw, nh, small.into_raw()))
}

fn from_rgba(w: u32, h: u32, rgba: Vec<u8>, premultiplied: bool) -> Option<Image> {
    if rgba.len() != (w as usize) * (h as usize) * 4 || w == 0 || h == 0 {
        return None;
    }
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, w, h);
    Some(if premultiplied { Image::from_rgba8_premultiplied(buffer) } else { Image::from_rgba8(buffer) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_without_growing() {
        let big = vec![255u8; 256 * 128 * 4];
        let (w, h, p) = fit(256, 128, big, 30).unwrap();
        assert_eq!((w, h, p.len()), (30, 15, 30 * 15 * 4));
        let small = vec![255u8; 16 * 16 * 4];
        assert_eq!(fit(16, 16, small, 30).map(|(w, h, _)| (w, h)), Some((16, 16)));
        assert!(fit(4, 4, vec![0; 3], 30).is_none());
    }
}
