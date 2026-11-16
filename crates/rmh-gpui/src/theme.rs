//! Visual tokens: a cinematic dark palette (same token approach as Kalyreon).

#[derive(Clone, Copy)]
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
    pub active: u32,
    pub good: u32,
    pub warn: u32,
    pub danger: u32,
}

pub const DARK: Palette = Palette {
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
    active: 0x191B27,
    good: 0x5FD39A,
    warn: 0xF2B45C,
    danger: 0xF07178,
};

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
