//! Apps on macOS: the .app bundles in the Applications folders, with icons from their .icns.

use crate::catalog::{Icon, Item, Kind, Run};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Launch {
    bundle: PathBuf,
}

pub fn scan() -> Vec<Item> {
    let home = dirs::home_dir().unwrap_or_default();
    let roots = [
        PathBuf::from("/Applications"),
        PathBuf::from("/Applications/Utilities"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
        PathBuf::from("/System/Library/CoreServices/Applications"),
        home.join("Applications"),
    ];
    let mut items: Vec<Item> = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "app") {
                if let Some(item) = bundle(&path)
                    && !items.iter().any(|i| i.id == item.id)
                {
                    items.push(item);
                }
            } else if path.is_dir() && root == Path::new("/Applications") {
                // Folders of apps one level down, like /Applications/Adobe Photoshop/.
                if let Ok(inner) = std::fs::read_dir(&path) {
                    for e in inner.flatten() {
                        if e.path().extension().is_some_and(|e| e == "app")
                            && let Some(item) = bundle(&e.path())
                        {
                            items.push(item);
                        }
                    }
                }
            }
        }
    }
    items.push(finder());
    items
}

fn finder() -> Item {
    let path = PathBuf::from("/System/Library/CoreServices/Finder.app");
    bundle(&path).unwrap_or(Item {
        id: "app:com.apple.finder".into(),
        title: "Finder".into(),
        subtitle: String::new(),
        keywords: String::new(),
        kind: Kind::Application,
        icon: Icon::None,
        tint: 0,
        run: Run::App(Launch { bundle: path.clone() }),
        path: Some(path),
    })
}

fn bundle(path: &Path) -> Option<Item> {
    let info: plist::Dictionary = plist::from_file(path.join("Contents/Info.plist")).ok()?;
    let text = |k: &str| info.get(k).and_then(|v| v.as_string()).map(str::to_string);
    let name = text("CFBundleDisplayName")
        .or_else(|| text("CFBundleName"))
        .unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    if name.is_empty() {
        return None;
    }
    let id = text("CFBundleIdentifier").unwrap_or_else(|| path.to_string_lossy().into_owned());
    let icon = text("CFBundleIconFile").map(|f| {
        let file = if f.ends_with(".icns") { f } else { format!("{f}.icns") };
        path.join("Contents/Resources").join(file)
    });
    Some(Item {
        id: format!("app:{id}"),
        title: name,
        subtitle: String::new(),
        keywords: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        kind: Kind::Application,
        icon: icon.filter(|p| p.is_file()).map_or(Icon::None, Icon::File),
        tint: 0,
        run: Run::App(Launch { bundle: path.to_path_buf() }),
        path: Some(path.to_path_buf()),
    })
}

/// What launching it opens, for the details pane.
pub fn describe(launch: &Launch) -> Option<String> {
    Some(launch.bundle.to_string_lossy().into_owned())
}

pub fn launch(launch: &Launch) -> io::Result<()> {
    crate::system::spawn("open", &[&launch.bundle.to_string_lossy()])
}

/// The smallest size at least `px` across from an .icns file (or its biggest), as RGBA.
pub fn icns(path: &Path, px: u32) -> Option<(u32, u32, Vec<u8>)> {
    let file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let family = icns::IconFamily::read(file).ok()?;
    let mut types = family.available_icons();
    types.sort_by_key(|t| (t.pixel_width() < px, (t.pixel_width() as i64 - px as i64).abs()));
    let image = types.iter().find_map(|t| family.get_icon_with_type(*t).ok())?;
    let image = image.convert_to(icns::PixelFormat::RGBA);
    Some((image.width(), image.height(), image.into_data().into_vec()))
}
