//! Windows 11's design (Fluent), which the launcher and Settings follow on Windows: Windows' own
//! greys in place of the palette's, and the accent you picked in Windows' settings. The palette
//! still decides light or dark.

use crate::Theme;
use slint::Color;

/// On Windows. TENDRIL_FLUENT=1 shows it elsewhere, to work on it without Windows.
pub fn on() -> bool {
    cfg!(windows) || std::env::var_os("TENDRIL_FLUENT").is_some()
}

/// Windows' default accent, as the two shades its controls use (see `accent`).
const DEFAULT_ACCENT: (u32, u32) = (0x60cdff, 0x005fb8);

/// Puts Windows' greys and accent into a window's Theme, after the palette was applied to it.
/// Returns the accent's two shades (shown on dark, shown on light).
pub fn apply(t: &Theme) -> (Color, Color) {
    t.set_fluent(true);
    let (on_dark, on_light) = accent().unwrap_or((rgb(DEFAULT_ACCENT.0), rgb(DEFAULT_ACCENT.1)));
    let dark = t.get_dark();
    let [sunken, bg, raised, line, ink, ink2, ink3] = if dark {
        [0x1c1c1c, 0x202020, 0x2d2d2d, 0x383838, 0xffffff, 0xcfcfcf, 0x9d9d9d]
    } else {
        [0xebebeb, 0xf3f3f3, 0xfbfbfb, 0xe0e0e0, 0x1b1b1b, 0x5d5d5d, 0x8a8a8a]
    };
    t.set_sunken(rgb(sunken));
    t.set_bg(rgb(bg));
    t.set_panel(rgb(bg));
    t.set_raised(rgb(raised));
    t.set_line(rgb(line));
    t.set_ink(rgb(ink));
    t.set_ink_2(rgb(ink2));
    t.set_ink_3(rgb(ink3));
    // Blue leads in Tendril's look; here the accent takes its place.
    t.set_blue(if dark { on_dark } else { on_light });
    (on_dark, on_light)
}

fn rgb(v: u32) -> Color {
    Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The accent you picked in Windows, as the two shades Windows itself uses for controls: a light
/// one shown on dark, a dark one shown on light (its accent palette's Light 2 and Dark 1).
#[cfg(windows)]
fn accent() -> Option<(Color, Color)> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RegGetValueW};
    use windows::core::w;
    let mut palette = [0u8; 32];
    let mut size = palette.len() as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent"),
            w!("AccentPalette"),
            RRF_RT_REG_BINARY,
            None,
            Some(palette.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    // Eight colours, four bytes each (red, green, blue, unused), lightest first.
    let shade = |i: usize| Color::from_rgb_u8(palette[i * 4], palette[i * 4 + 1], palette[i * 4 + 2]);
    Some((shade(1), shade(4)))
}

#[cfg(not(windows))]
fn accent() -> Option<(Color, Color)> {
    None
}
