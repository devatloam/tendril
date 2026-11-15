//! Icons for the list, loaded the first time a row needs one and kept small: a 512px PNG
//! becomes 64px before it's stored, so a few hundred apps cost a few megabytes, not hundreds.

use crate::catalog::Icon;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::collections::HashMap;
use std::path::Path;

const SIZE: u32 = 64;
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
];

#[derive(Default)]
pub struct Icons {
    loaded: HashMap<String, Option<Image>>,
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

    /// The image for a row, its solid for when the row is selected, and the tint to draw it with.
    /// An app's own icon is the same either way.
    pub fn get(&mut self, icon: &Icon, tint: i32) -> (Image, Image, i32) {
        let loaded = match icon {
            Icon::None => None,
            Icon::Builtin(name) => return (self.builtin(name), self.builtin_solid(name), tint),
            Icon::File(path) => {
                let key = path.to_string_lossy().into_owned();
                self.loaded.entry(key).or_insert_with(|| load_file(path)).clone()
            }
            Icon::Shell(name) => self.loaded.entry(name.clone()).or_insert_with(|| load_shell(name)).clone(),
        };
        match loaded {
            Some(image) => (image.clone(), image, 0),
            None => (self.builtin("app"), self.builtin_solid("app"), FALLBACK_TINT),
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
