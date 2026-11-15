//! Icons for the list, loaded the first time a row needs one and kept small: a 512px PNG
//! becomes 64px before it's stored, so a few hundred apps cost a few megabytes, not hundreds.

use crate::catalog::Icon;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::collections::HashMap;
use std::path::Path;

const SIZE: u32 = 64;
/// The neutral tint, used when an app has no icon of its own and gets the generic one.
const FALLBACK_TINT: i32 = 7;

const BUILTIN: &[(&str, &[u8])] = &[
    ("app", include_bytes!("../ui/icons/app.svg")),
    ("arrow", include_bytes!("../ui/icons/arrow.svg")),
    ("bolt", include_bytes!("../ui/icons/bolt.svg")),
    ("calculator", include_bytes!("../ui/icons/calculator.svg")),
    ("check", include_bytes!("../ui/icons/check.svg")),
    ("copy", include_bytes!("../ui/icons/copy.svg")),
    ("eraser", include_bytes!("../ui/icons/eraser.svg")),
    ("folder", include_bytes!("../ui/icons/folder.svg")),
    ("globe", include_bytes!("../ui/icons/globe.svg")),
    ("link", include_bytes!("../ui/icons/link.svg")),
    ("lock", include_bytes!("../ui/icons/lock.svg")),
    ("logout", include_bytes!("../ui/icons/logout.svg")),
    ("moon", include_bytes!("../ui/icons/moon.svg")),
    ("open", include_bytes!("../ui/icons/open.svg")),
    ("palette", include_bytes!("../ui/icons/palette.svg")),
    ("power", include_bytes!("../ui/icons/power.svg")),
    ("quit", include_bytes!("../ui/icons/quit.svg")),
    ("refresh", include_bytes!("../ui/icons/refresh.svg")),
    ("restart", include_bytes!("../ui/icons/restart.svg")),
    ("settings", include_bytes!("../ui/icons/settings.svg")),
];

#[derive(Default)]
pub struct Icons {
    loaded: HashMap<String, Option<Image>>,
}

impl Icons {
    pub fn builtin(&mut self, name: &str) -> Image {
        self.loaded
            .entry(format!("builtin:{name}"))
            .or_insert_with(|| {
                let svg = BUILTIN.iter().find(|(n, _)| *n == name).map_or(BUILTIN[0].1, |(_, svg)| svg);
                Image::load_from_svg_data(svg).ok()
            })
            .clone()
            .unwrap_or_default()
    }

    /// The image for a row, and the tint to draw it with.
    pub fn get(&mut self, icon: &Icon, tint: i32) -> (Image, i32) {
        let loaded = match icon {
            Icon::None => None,
            Icon::Builtin(name) => return (self.builtin(name), tint),
            Icon::File(path) => {
                let key = path.to_string_lossy().into_owned();
                self.loaded.entry(key).or_insert_with(|| load_file(path)).clone()
            }
            Icon::Shell(name) => self.loaded.entry(name.clone()).or_insert_with(|| load_shell(name)).clone(),
        };
        match loaded {
            Some(image) => (image, 0),
            None => (self.builtin("app"), FALLBACK_TINT),
        }
    }
}

fn load_file(path: &Path) -> Option<Image> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    match ext.as_str() {
        // Vectors stay vectors and are drawn at the size they're shown.
        "svg" | "svgz" => Image::load_from_path(path).ok(),
        #[cfg(target_os = "macos")]
        "icns" => crate::apps::icns(path).and_then(|(w, h, rgba)| from_rgba(w, h, rgba, false)),
        _ => {
            let image = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
            let image = if image.width() > SIZE || image.height() > SIZE { image.thumbnail(SIZE, SIZE) } else { image };
            let rgba = image.into_rgba8();
            let (w, h) = rgba.dimensions();
            from_rgba(w, h, rgba.into_raw(), false)
        }
    }
}

#[cfg(windows)]
fn load_shell(name: &str) -> Option<Image> {
    crate::apps::icon(name).and_then(|(w, h, rgba)| from_rgba(w, h, rgba, true))
}

#[cfg(not(windows))]
fn load_shell(_: &str) -> Option<Image> {
    None
}

fn from_rgba(w: u32, h: u32, rgba: Vec<u8>, premultiplied: bool) -> Option<Image> {
    if rgba.len() != (w * h * 4) as usize || w == 0 || h == 0 {
        return None;
    }
    let (w, h, rgba) = if w > SIZE || h > SIZE {
        let full = image::RgbaImage::from_raw(w, h, rgba)?;
        let small = image::imageops::thumbnail(&full, SIZE, SIZE);
        let (w, h) = small.dimensions();
        (w, h, small.into_raw())
    } else {
        (w, h, rgba)
    };
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, w, h);
    Some(if premultiplied { Image::from_rgba8_premultiplied(buffer) } else { Image::from_rgba8(buffer) })
}
