//! Apps on Windows: everything in the shell's Apps folder, which is what Start lists. That covers
//! Start menu shortcuts and Store apps alike, and Windows draws their icons for us.

use crate::catalog::{Icon, Item, Kind, Run};
use std::ffi::c_void;
use std::io;
use std::path::PathBuf;
use windows::Win32::Foundation::{HWND, SIZE};
use windows::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, GetObjectW, HGDIOBJ, ReleaseDC,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, IBindCtx};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItemImageFactory, KF_FLAG_DEFAULT, SHCreateItemFromParsingName,
    SHGetKnownFolderItem, SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Launch {
    target: String,
}

/// Start menu entries that aren't apps.
const NOT_APPS: &[&str] = &[".txt", ".chm", ".pdf", ".html", ".htm", ".url", ".ini", ".rtf", ".md", ".log"];

pub fn scan() -> Vec<Item> {
    let mut out = Vec::new();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let Ok(folder) = SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) else {
            return out;
        };
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None::<&IBindCtx>, &BHID_EnumItems) else {
            return out;
        };
        loop {
            let mut batch = [None];
            let mut fetched = 0u32;
            if items.Next(&mut batch, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = batch[0].take() else { break };
            let (Some(name), Some(parse)) = (display(&item, SIGDN_NORMALDISPLAY), display(&item, SIGDN_PARENTRELATIVEPARSING)) else {
                continue;
            };
            let lower = parse.to_lowercase();
            if name.to_lowercase().contains("uninstall") || NOT_APPS.iter().any(|ext| lower.ends_with(ext)) {
                continue;
            }
            let target = format!("shell:AppsFolder\\{parse}");
            let exe = parse.rsplit('\\').next().unwrap_or("").to_string();
            let on_disk = parse.chars().nth(1) == Some(':');
            out.push(Item {
                id: format!("app:{parse}"),
                title: name,
                subtitle: String::new(),
                keywords: exe,
                kind: Kind::Application,
                icon: Icon::Shell(target.clone()),
                tint: 0,
                run: Run::App(Launch { target }),
                path: on_disk.then(|| PathBuf::from(&parse)),
            });
        }
    }
    out
}

unsafe fn display(item: &IShellItem, kind: SIGDN) -> Option<String> {
    unsafe {
        let name = item.GetDisplayName(kind).ok()?;
        let text = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const c_void));
        text
    }
}

/// What launching it opens, for the details pane.
pub fn describe(launch: &Launch) -> Option<String> {
    Some(launch.target.clone())
}

pub fn launch(launch: &Launch) -> io::Result<()> {
    shell_open(&launch.target)
}

pub fn shell_open(target: &str) -> io::Result<()> {
    let target = HSTRING::from(target);
    let result = unsafe { ShellExecuteW(None, w!("open"), &target, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    // ShellExecute reports success as any value over 32.
    if result.0 as isize > 32 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

pub fn sleep() -> io::Result<()> {
    // Sleep, not hibernate, even where hibernation is turned on.
    if unsafe { SetSuspendState(false, false, false) } { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// A shell item's icon at `px` (or the next size up the shell has), as premultiplied RGBA.
pub fn icon(parsing_name: &str, px: u32) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let name = HSTRING::from(parsing_name);
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&name, None::<&IBindCtx>).ok()?;
        let bitmap = factory.GetImage(SIZE { cx: px as i32, cy: px as i32 }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok()?;
        let object = HGDIOBJ(bitmap.0);
        let mut info = BITMAP::default();
        let read = GetObjectW(object, size_of::<BITMAP>() as i32, Some(&mut info as *mut BITMAP as *mut c_void));
        let (w, h) = (info.bmWidth, info.bmHeight.abs());
        if read == 0 || w <= 0 || h <= 0 {
            let _ = DeleteObject(object);
            return None;
        }
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let lines = GetDIBits(dc, bitmap, 0, h as u32, Some(pixels.as_mut_ptr() as *mut c_void), &mut header, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(object);
        if lines == 0 {
            return None;
        }
        // BGRA to RGBA. Old icons without an alpha channel come back fully transparent; make them opaque.
        let opaque = pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 0);
        for p in pixels.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
            if opaque {
                p[3] = 255;
            }
        }
        Some((w as u32, h as u32, pixels))
    }
}

/// Windows 11 rounds a borderless window's corners and gives it a shadow only when asked.
pub fn round_corners(hwnd: isize) {
    let preference = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            HWND(hwnd as *mut c_void),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const c_void,
            size_of_val(&preference) as u32,
        );
    }
}
