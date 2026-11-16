//! Persistent user configuration (`~/.config/rustmediahub/config.json`).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub tmdb_api_key: Option<String>,
    pub real_debrid_api_key: Option<String>,
    pub default_library_path: Option<PathBuf>,
    /// mpv executable (defaults to `mpv` on PATH)
    #[serde(default)]
    pub mpv_path: Option<String>,
    /// Last used volume (0-130)
    #[serde(default)]
    pub volume: Option<f64>,
    /// Where video is drawn: inside the app window, or mpv's own window
    #[serde(default)]
    pub video_mode: VideoMode,
    /// Separate-window mode: start mpv fullscreen (the original behaviour)
    #[serde(default = "yes")]
    pub window_fullscreen: bool,
    /// Preferred audio / subtitle languages, e.g. "cze,eng" (mpv --alang/--slang)
    #[serde(default)]
    pub audio_lang: Option<String>,
    #[serde(default)]
    pub sub_lang: Option<String>,
    /// Match local files against TMDB after scanning (posters, details)
    #[serde(default = "yes")]
    pub auto_match: bool,
    /// Trakt API app credentials (https://trakt.tv/oauth/applications)
    #[serde(default)]
    pub trakt_client_id: Option<String>,
    #[serde(default)]
    pub trakt_client_secret: Option<String>,
    /// Scrobble playback to Trakt while watching
    #[serde(default = "yes")]
    pub trakt_scrobble: bool,
    /// Home screen layout
    #[serde(default)]
    pub home_layout: HomeLayout,
    /// Play the next local episode automatically when one ends
    #[serde(default = "yes")]
    pub autoplay_next: bool,
    /// Show mpv's own on-screen controller over the video too
    #[serde(default = "yes")]
    pub mpv_osc: bool,
}

/// Alternative home screens (switchable in Settings or on Home itself)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HomeLayout {
    /// A: Continue Watching + Up Next
    #[default]
    Focus,
    /// B: Trending / Discover with a hero banner
    Discover,
    /// C: Library grid with a small Continue Watching row
    Library,
}

impl HomeLayout {
    pub const ALL: [HomeLayout; 3] = [HomeLayout::Focus, HomeLayout::Discover, HomeLayout::Library];
    pub fn label(self) -> &'static str {
        match self {
            HomeLayout::Focus => "Continue & Up Next",
            HomeLayout::Discover => "Discover",
            HomeLayout::Library => "Library",
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoMode {
    /// mpv draws into a surface inside the app window (X11 / XWayland)
    #[default]
    Embedded,
    /// mpv opens its own window with its full on-screen controller
    Window,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tmdb_api_key: std::env::var("TMDB_API_KEY").ok(),
            real_debrid_api_key: std::env::var("REAL_DEBRID_API_KEY").ok(),
            default_library_path: None,
            mpv_path: None,
            volume: None,
            video_mode: VideoMode::default(),
            window_fullscreen: true,
            audio_lang: None,
            sub_lang: None,
            auto_match: true,
            trakt_client_id: std::env::var("TRAKT_CLIENT_ID").ok(),
            trakt_client_secret: std::env::var("TRAKT_CLIENT_SECRET").ok(),
            trakt_scrobble: true,
            home_layout: HomeLayout::default(),
            autoplay_next: true,
            mpv_osc: true,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;
        if !config_path.exists() {
            info!("No config file found at {:?}, using defaults", config_path);
            return Ok(Self::default());
        }
        info!("Loading config from {:?}", config_path);
        let content = std::fs::read_to_string(&config_path).context("Failed to read config file")?;
        let mut config: AppConfig = serde_json::from_str(&content).context("Failed to parse config file")?;
        // Environment (.env) fills in keys that are not saved in the config file
        if config.tmdb_api_key.as_deref().unwrap_or("").is_empty() {
            config.tmdb_api_key = std::env::var("TMDB_API_KEY").ok();
        }
        if config.real_debrid_api_key.as_deref().unwrap_or("").is_empty() {
            config.real_debrid_api_key = std::env::var("REAL_DEBRID_API_KEY").ok();
        }
        if config.trakt_client_id.as_deref().unwrap_or("").is_empty() {
            config.trakt_client_id = std::env::var("TRAKT_CLIENT_ID").ok();
        }
        if config.trakt_client_secret.as_deref().unwrap_or("").is_empty() {
            config.trakt_client_secret = std::env::var("TRAKT_CLIENT_SECRET").ok();
        }
        Ok(config)
    }

    /// Trakt client id + secret, when both are set
    pub fn trakt_credentials(&self) -> Option<(String, String)> {
        let id = self.trakt_client_id.clone().filter(|v| !v.trim().is_empty())?;
        let secret = self.trakt_client_secret.clone().filter(|v| !v.trim().is_empty())?;
        Some((id.trim().to_string(), secret.trim().to_string()))
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create config directory")?;
        }
        info!("Saving config to {:?}", config_path);
        let content = serde_json::to_string_pretty(self).context("Failed to serialize config")?;
        // Holds API keys: keep it private to the user
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o600)
                .open(&config_path)
                .context("Failed to write config file")?;
            f.write_all(content.as_bytes())?;
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600));
        }
        #[cfg(not(unix))]
        std::fs::write(&config_path, content).context("Failed to write config file")?;
        Ok(())
    }

    pub fn mpv_path(&self) -> String {
        self.mpv_path.clone().filter(|p| !p.trim().is_empty()).unwrap_or_else(|| "mpv".to_string())
    }

    pub fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir().context("Could not determine config directory")?;
        Ok(config_dir.join("rustmediahub").join("config.json"))
    }
}
