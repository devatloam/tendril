//! The palettes, the same ones Blight's dashboard offers in Account. Picked in config.toml.

use slint::Color;

pub struct Palette {
    pub key: &'static str,
    pub name: &'static str,
    pub note: &'static str,
    pub dark: bool,
    pub sunken: Color,
    pub bg: Color,
    pub panel: Color,
    pub raised: Color,
    pub line: Color,
    pub ink: Color,
    pub ink_2: Color,
    pub ink_3: Color,
    pub accent: Color,
    pub blue: Color,
    pub green: Color,
    pub red: Color,
    pub amber: Color,
    pub violet: Color,
}

const fn hex(rgb: u32) -> Color {
    Color::from_rgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub const PALETTES: &[Palette] = &[
    Palette {
        key: "ember",
        name: "Ember",
        note: "The original",
        dark: true,
        sunken: hex(0x1a1918),
        bg: hex(0x1f1e1d),
        panel: hex(0x262624),
        raised: hex(0x343330),
        line: hex(0x3a3835),
        ink: hex(0xf5f4ef),
        ink_2: hex(0xa3a098),
        ink_3: hex(0x87867f),
        accent: hex(0xd97757),
        blue: hex(0x6a9bcc),
        green: hex(0x93a877),
        red: hex(0xf06e64),
        amber: hex(0xe49c42),
        violet: hex(0xa99bd6),
    },
    Palette {
        key: "midnight",
        name: "Midnight",
        note: "Cool blue-black",
        dark: true,
        sunken: hex(0x111419),
        bg: hex(0x151920),
        panel: hex(0x1b2028),
        raised: hex(0x272e3a),
        line: hex(0x2b3240),
        ink: hex(0xe9edf4),
        ink_2: hex(0x9ba4b3),
        ink_3: hex(0x7d8696),
        accent: hex(0x7aa7e0),
        blue: hex(0x7aa7e0),
        green: hex(0x8fb38a),
        red: hex(0xef7b73),
        amber: hex(0xe3a656),
        violet: hex(0xaaa2e8),
    },
    Palette {
        key: "moss",
        name: "Moss",
        note: "Green-grey, calm",
        dark: true,
        sunken: hex(0x131713),
        bg: hex(0x171c17),
        panel: hex(0x1d231d),
        raised: hex(0x2a3229),
        line: hex(0x2e372d),
        ink: hex(0xeef1e8),
        ink_2: hex(0xa3ac9b),
        ink_3: hex(0x858e7d),
        accent: hex(0x9fb87f),
        blue: hex(0x79a5c4),
        green: hex(0x9fb87f),
        red: hex(0xec7468),
        amber: hex(0xdea24f),
        violet: hex(0xa99bd0),
    },
    Palette {
        key: "dusk",
        name: "Dusk",
        note: "Soft plum",
        dark: true,
        sunken: hex(0x17141b),
        bg: hex(0x1b1820),
        panel: hex(0x221e28),
        raised: hex(0x312b39),
        line: hex(0x342e3d),
        ink: hex(0xf3eff7),
        ink_2: hex(0xaaa2b4),
        ink_3: hex(0x8b8395),
        accent: hex(0xd08bb0),
        blue: hex(0x8aa5dc),
        green: hex(0x9db386),
        red: hex(0xf07a7a),
        amber: hex(0xe6a55a),
        violet: hex(0xb8a6e8),
    },
    Palette {
        key: "black",
        name: "Black",
        note: "True black for OLED phones",
        dark: true,
        sunken: hex(0x080808),
        bg: hex(0x000000),
        panel: hex(0x0f0f0f),
        raised: hex(0x1d1d1d),
        line: hex(0x242424),
        ink: hex(0xececec),
        ink_2: hex(0x9e9e9e),
        ink_3: hex(0x7c7c7c),
        accent: hex(0xd97757),
        blue: hex(0x6fa3d8),
        green: hex(0x96ad79),
        red: hex(0xf2716a),
        amber: hex(0xe8a046),
        violet: hex(0xad9fdc),
    },
    Palette {
        key: "paper",
        name: "Paper",
        note: "Soft light, no pure white",
        dark: false,
        sunken: hex(0xe6e3da),
        bg: hex(0xedeae2),
        panel: hex(0xf5f3ee),
        raised: hex(0xe0ddd3),
        line: hex(0xd8d4c9),
        ink: hex(0x2a2926),
        ink_2: hex(0x5c5a55),
        ink_3: hex(0x67655d),
        accent: hex(0xc4643f),
        blue: hex(0x4a7aa8),
        green: hex(0x6c8352),
        red: hex(0xb4463b),
        amber: hex(0xa0670f),
        violet: hex(0x7062ab),
    },
    Palette {
        key: "sepia",
        name: "Sepia",
        note: "Warm, less blue light",
        dark: false,
        sunken: hex(0xe3d9c3),
        bg: hex(0xeae1cc),
        panel: hex(0xf2eadb),
        raised: hex(0xdcd1b8),
        line: hex(0xd2c6ab),
        ink: hex(0x3b3226),
        ink_2: hex(0x6a5e4c),
        ink_3: hex(0x6a5e4f),
        accent: hex(0xb8613b),
        blue: hex(0x50789e),
        green: hex(0x6b7f47),
        red: hex(0xb0463a),
        amber: hex(0x9e6410),
        violet: hex(0x76609e),
    },
    Palette {
        key: "mist",
        name: "Mist",
        note: "Cool grey",
        dark: false,
        sunken: hex(0xdde1e7),
        bg: hex(0xe5e8ed),
        panel: hex(0xeef0f3),
        raised: hex(0xd7dce3),
        line: hex(0xcfd5dd),
        ink: hex(0x252a31),
        ink_2: hex(0x545c68),
        ink_3: hex(0x5d6470),
        accent: hex(0x3f74b5),
        blue: hex(0x3f74b5),
        green: hex(0x5f7f52),
        red: hex(0xb8443c),
        amber: hex(0x9a650f),
        violet: hex(0x6c60b0),
    },
];

pub fn find(key: &str) -> Option<&'static Palette> {
    PALETTES.iter().find(|p| p.key.eq_ignore_ascii_case(key))
}

pub fn apply(theme: &crate::Theme<'_>, p: &Palette) {
    theme.set_sunken(p.sunken);
    theme.set_bg(p.bg);
    theme.set_panel(p.panel);
    theme.set_raised(p.raised);
    theme.set_line(p.line);
    theme.set_ink(p.ink);
    theme.set_ink_2(p.ink_2);
    theme.set_ink_3(p.ink_3);
    theme.set_accent(p.accent);
    theme.set_blue(p.blue);
    theme.set_green(p.green);
    theme.set_red(p.red);
    theme.set_amber(p.amber);
    theme.set_violet(p.violet);
    theme.set_dark(p.dark);
}
