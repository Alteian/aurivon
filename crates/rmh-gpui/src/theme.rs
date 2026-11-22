//! Aurivon brand system: design tokens + themes. Every page takes a `Palette`
//! (colours) and uses the `tokens` below for radii, spacing, type and motion,
//! so a theme switch restyles the whole app live. See BRANDING.md.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Palette {
    pub canvas: u32,
    pub sidebar: u32,
    pub surface: u32,
    pub surface_raised: u32,
    pub border: u32,
    pub border_soft: u32,
    pub text: u32,
    pub muted: u32,
    pub faint: u32,
    pub accent: u32,
    pub accent_soft: u32,
    pub on_accent: u32,
    /// Second stop of the brand gradient (logo tile, hero chip, progress)
    pub accent2: u32,
    pub active: u32,
    pub good: u32,
    pub warn: u32,
    pub danger: u32,
    /// Ratings / "gold" highlights
    pub gold: u32,
    /// Light theme (images keep dark scrims either way)
    pub light: bool,
}

/// Brand constants shared by all themes (also documented in BRANDING.md)
#[allow(dead_code)]
pub mod brand {
    /// Aurum gold (logo, wordmark, ratings)
    pub const GOLD: u32 = 0xF6C453;
    pub const GOLD_LIGHT: u32 = 0xFFF3C8;
    pub const GOLD_DEEP: u32 = 0xC98A16;
    /// Aurora gradient (teal → violet → magenta)
    pub const AURORA: [u32; 3] = [0x2EE6C5, 0x7C5CFF, 0xE35CFF];
    /// Scrim over backdrops (hero, details): always dark so text stays legible
    pub const SCRIM: u32 = 0x0A0B10;
}

/// Design tokens: one scale for the whole app
#[allow(dead_code)]
pub mod tokens {
    // radii (px)
    pub const RADIUS_SM: f32 = 6.0;
    pub const RADIUS_MD: f32 = 10.0;
    pub const RADIUS_LG: f32 = 14.0;
    pub const RADIUS_XL: f32 = 20.0;
    pub const RADIUS_PILL: f32 = 999.0;
    // spacing (4 px grid)
    pub const SPACE_1: f32 = 4.0;
    pub const SPACE_2: f32 = 8.0;
    pub const SPACE_3: f32 = 12.0;
    pub const SPACE_4: f32 = 16.0;
    pub const SPACE_6: f32 = 24.0;
    pub const SPACE_8: f32 = 32.0;
    pub const SPACE_10: f32 = 40.0;
    // type scale (px): caption, body-sm, body, title, headline, display
    pub const TYPE_XS: f32 = 12.0;
    pub const TYPE_SM: f32 = 14.0;
    pub const TYPE_MD: f32 = 16.0;
    pub const TYPE_LG: f32 = 20.0;
    pub const TYPE_XL: f32 = 28.0;
    pub const TYPE_DISPLAY: f32 = 40.0;
    // elevation: shadow blur (px) for card / raised / overlay
    pub const SHADOW_CARD: f32 = 8.0;
    pub const SHADOW_RAISED: f32 = 18.0;
    pub const SHADOW_OVERLAY: f32 = 36.0;
    // motion (ms)
    pub const MOTION_FAST: u64 = 120;
    pub const MOTION_BASE: u64 = 200;
    pub const MOTION_SLOW: u64 = 360;
    /// Hero carousel dwell time
    pub const HERO_DWELL_MS: u64 = 9000;
    // layout
    pub const SIDEBAR_W: f32 = 220.0;
    pub const RAIL_W: f32 = 76.0;
}

/// Aurum: warm black + gold (default)
pub const AURUM: Palette = Palette {
    canvas: 0x0B0A0F,
    sidebar: 0x0E0D13,
    surface: 0x17151E,
    surface_raised: 0x1F1C28,
    border: 0x2C2836,
    border_soft: 0x1F1C29,
    text: 0xF6F2EA,
    muted: 0x9C95A6,
    faint: 0x625C6E,
    accent: 0xF6C453,
    accent_soft: 0x2E2614,
    on_accent: 0x1A1205,
    accent2: 0xE0902A,
    active: 0x1C1A24,
    good: 0x5FD39A,
    warn: 0xF2B45C,
    danger: 0xF07178,
    gold: 0xF6C453,
    light: false,
};

/// Midnight: deep blue
pub const MIDNIGHT: Palette = Palette {
    canvas: 0x070B17,
    sidebar: 0x091022,
    surface: 0x0F1830,
    surface_raised: 0x15203D,
    border: 0x22305A,
    border_soft: 0x18244A,
    text: 0xEAF0FF,
    muted: 0x8D9BC2,
    faint: 0x56648C,
    accent: 0x5B9DFF,
    accent_soft: 0x14264A,
    on_accent: 0x04101F,
    accent2: 0x8F6BFF,
    active: 0x111C38,
    good: 0x5FD39A,
    warn: 0xF2B45C,
    danger: 0xF07178,
    gold: 0xF6C453,
    light: false,
};

/// OLED: true black, minimal chrome
pub const OLED: Palette = Palette {
    canvas: 0x000000,
    sidebar: 0x000000,
    surface: 0x0C0C0C,
    surface_raised: 0x141414,
    border: 0x222222,
    border_soft: 0x161616,
    text: 0xFFFFFF,
    muted: 0x9A9A9A,
    faint: 0x5E5E5E,
    accent: 0xF6C453,
    accent_soft: 0x221B0A,
    on_accent: 0x140E02,
    accent2: 0xFFFFFF,
    active: 0x121212,
    good: 0x5FD39A,
    warn: 0xF2B45C,
    danger: 0xF07178,
    gold: 0xF6C453,
    light: false,
};

/// Aurora: teal / violet night sky
pub const AURORA: Palette = Palette {
    canvas: 0x080C12,
    sidebar: 0x0A1018,
    surface: 0x101A24,
    surface_raised: 0x16222F,
    border: 0x23384A,
    border_soft: 0x182837,
    text: 0xECFBF8,
    muted: 0x8FB1B4,
    faint: 0x587479,
    accent: 0x2EE6C5,
    accent_soft: 0x0F2E2C,
    on_accent: 0x02171A,
    accent2: 0x9B6BFF,
    active: 0x122029,
    good: 0x5FD39A,
    warn: 0xF2B45C,
    danger: 0xF07178,
    gold: 0xF6C453,
    light: false,
};

/// Light: warm paper + deep gold
pub const LIGHT: Palette = Palette {
    canvas: 0xF7F5F0,
    sidebar: 0xEFECE4,
    surface: 0xFFFFFF,
    surface_raised: 0xF3F0E8,
    border: 0xDCD6C8,
    border_soft: 0xE7E2D6,
    text: 0x1B1822,
    muted: 0x5F5A6B,
    faint: 0x8F8A99,
    accent: 0xB7790B,
    accent_soft: 0xF5E7C6,
    on_accent: 0xFFFFFF,
    accent2: 0x7C5CFF,
    active: 0xE9E4D8,
    good: 0x1F9D64,
    warn: 0xB7791F,
    danger: 0xC9424B,
    gold: 0xB7790B,
    light: true,
};

/// Violet: the original Aurivon look (pre-0.7)
pub const VIOLET: Palette = Palette {
    canvas: 0x0A0B10,
    sidebar: 0x0D0F15,
    surface: 0x14161F,
    surface_raised: 0x1B1E2A,
    border: 0x262A38,
    border_soft: 0x1C1F2B,
    text: 0xF1F2F7,
    muted: 0x8C92A6,
    faint: 0x5A6075,
    accent: 0x8B7CFF,
    accent_soft: 0x221E3D,
    on_accent: 0x0B0920,
    accent2: 0x4CC9F0,
    active: 0x191B27,
    good: 0x5FD39A,
    warn: 0xF2B45C,
    danger: 0xF07178,
    gold: 0xF2B45C,
    light: false,
};

/// (id, label, palette): the order shown in Settings → Appearance
pub const THEMES: [(&str, &str, Palette); 6] = [
    ("aurum", "Aurum", AURUM),
    ("midnight", "Midnight", MIDNIGHT),
    ("oled", "OLED Black", OLED),
    ("aurora", "Aurora", AURORA),
    ("light", "Light", LIGHT),
    ("violet", "Violet (classic)", VIOLET),
];

/// Accent presets for the picker
pub const ACCENTS: [u32; 8] = [0xF6C453, 0xFF8A3D, 0xF0607A, 0xE35CFF, 0x8B7CFF, 0x5B9DFF, 0x2EE6C5, 0x5FD39A];

/// Kept for code that wants the classic palette
#[allow(dead_code)]
pub const DARK: Palette = VIOLET;

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| -> (f32, f32) { (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32) };
    let mut out = 0;
    for s in [16u32, 8, 0] {
        let (x, y) = ch(s);
        out |= ((x + (y - x) * t).round().clamp(0.0, 255.0) as u32) << s;
    }
    out
}

fn luminance(c: u32) -> f32 {
    let f = |s: u32| ((c >> s) & 0xFF) as f32 / 255.0;
    0.2126 * f(16) + 0.7152 * f(8) + 0.0722 * f(0)
}

pub fn parse_hex(s: &str) -> Option<u32> {
    let h = s.trim().trim_start_matches('#');
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok()).flatten()
}

/// The palette for the saved settings. `system_light`: the desktop prefers light.
pub fn palette_for(theme: &str, accent: Option<&str>, follow_system: bool, system_light: bool) -> Palette {
    let id = if follow_system { if system_light { "light" } else if theme == "light" { "aurum" } else { theme } } else { theme };
    let mut p = THEMES.iter().find(|(i, _, _)| *i == id).map(|(_, _, p)| *p).unwrap_or(AURUM);
    if let Some(a) = accent.and_then(parse_hex) {
        p.accent = a;
        p.accent_soft = mix(p.surface, a, if p.light { 0.22 } else { 0.18 });
        p.on_accent = if luminance(a) > 0.55 { 0x15110A } else { 0xFFFFFF };
    }
    p
}

/// Deterministic pair of tile colours for a title, so posters-less cards still look distinct.
pub fn tile_colors(seed: &str) -> (u32, u32) {
    const PAIRS: &[(u32, u32)] = &[
        (0x3B2F8F, 0x12101F),
        (0x1F5C7A, 0x0D1820),
        (0x7A2E4F, 0x1C0D15),
        (0x2F6B4F, 0x0D1A14),
        (0x7A5A1F, 0x1E160A),
        (0x4A3F8F, 0x15122A),
        (0x2E4F7A, 0x0D141E),
        (0x6B2F2F, 0x1A0D0D),
    ];
    let h = seed.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    PAIRS[(h as usize) % PAIRS.len()]
}

/// Up to two initials for a title
pub fn initials(title: &str) -> String {
    title
        .split_whitespace()
        .filter(|w| w.chars().next().is_some_and(|c| c.is_alphanumeric()))
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(|c| c.to_uppercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_resolve() {
        assert_eq!(palette_for("midnight", None, false, false), MIDNIGHT);
        assert_eq!(palette_for("nope", None, false, false), AURUM);
        assert_eq!(palette_for("oled", None, true, true), LIGHT);
        assert_eq!(palette_for("light", None, true, false), AURUM);
        let p = palette_for("aurum", Some("#2EE6C5"), false, false);
        assert_eq!(p.accent, 0x2EE6C5);
        assert_eq!(parse_hex("zz"), None);
    }
}
