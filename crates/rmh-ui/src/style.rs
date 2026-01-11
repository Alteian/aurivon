//! Custom styles for the application

use iced::Color;

/// Application theme colors
pub struct Theme;

impl Theme {
    // Background colors
    pub const BG_PRIMARY: Color = Color::from_rgb(0.08, 0.08, 0.12);
    pub const BG_SECONDARY: Color = Color::from_rgb(0.12, 0.12, 0.16);
    pub const BG_TERTIARY: Color = Color::from_rgb(0.16, 0.16, 0.20);
    
    // Accent colors
    pub const ACCENT: Color = Color::from_rgb(0.4, 0.6, 1.0);
    pub const ACCENT_HOVER: Color = Color::from_rgb(0.5, 0.7, 1.0);
    
    // Text colors
    pub const TEXT_PRIMARY: Color = Color::from_rgb(0.95, 0.95, 0.95);
    pub const TEXT_SECONDARY: Color = Color::from_rgb(0.7, 0.7, 0.7);
    pub const TEXT_MUTED: Color = Color::from_rgb(0.5, 0.5, 0.5);
    
    // Status colors
    pub const SUCCESS: Color = Color::from_rgb(0.4, 0.8, 0.4);
    pub const WARNING: Color = Color::from_rgb(0.9, 0.7, 0.2);
    pub const ERROR: Color = Color::from_rgb(0.9, 0.3, 0.3);
    
    // Card colors
    pub const CARD_BG: Color = Color::from_rgb(0.14, 0.14, 0.18);
    pub const CARD_BORDER: Color = Color::from_rgb(0.25, 0.25, 0.30);
    pub const CARD_HOVER: Color = Color::from_rgb(0.18, 0.18, 0.22);
}
