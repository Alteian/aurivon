//! Persistent user configuration (`~/.config/aurivon/config.json`).

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

    /// Country for "where to stream" and provider filters (ISO 3166-1, e.g. "CZ")
    #[serde(default = "default_region")]
    pub region: String,
    /// Time zone for the release calendar (IANA name)
    #[serde(default = "default_tz")]
    pub timezone: String,

    /// OpenSubtitles.com API key (consumer) and optional account for more downloads
    #[serde(default)]
    pub opensubtitles_api_key: Option<String>,
    #[serde(default)]
    pub opensubtitles_username: Option<String>,
    #[serde(default)]
    pub opensubtitles_password: Option<String>,
    /// Fetch subtitles automatically when a file has none in a preferred language
    #[serde(default = "yes")]
    pub subtitles_auto: bool,

    /// Simkl app client id (https://simkl.com/settings/developer/)
    #[serde(default)]
    pub simkl_client_id: Option<String>,
    #[serde(default = "yes")]
    pub simkl_scrobble: bool,
    /// AniList API client id (https://anilist.co/settings/developer)
    #[serde(default)]
    pub anilist_client_id: Option<String>,
    #[serde(default = "yes")]
    pub anilist_sync: bool,

    /// Check GitHub releases on startup and every few hours
    #[serde(default = "yes")]
    pub auto_update: bool,
    /// Offer pre-releases (e.g. 0.3.0-beta.1) too
    #[serde(default)]
    pub update_prereleases: bool,
}

fn default_tz() -> String {
    "Europe/Prague".into()
}

fn default_region() -> String {
    "CZ".into()
}

/// ISO 639-2 ("cze", "eng") or 639-1 ("cs") to the 639-1 code OpenSubtitles uses
pub fn lang_code(code: &str) -> String {
    let c = code.trim().to_lowercase();
    match c.as_str() {
        "cze" | "ces" | "cz" => "cs".into(),
        "eng" => "en".into(),
        "slo" | "slk" => "sk".into(),
        "ger" | "deu" => "de".into(),
        "fre" | "fra" => "fr".into(),
        "spa" => "es".into(),
        "ita" => "it".into(),
        "pol" => "pl".into(),
        "hun" => "hu".into(),
        "rus" => "ru".into(),
        "ukr" => "uk".into(),
        "jpn" => "ja".into(),
        "kor" => "ko".into(),
        "por" => "pt-PT".into(),
        "chi" | "zho" => "zh-CN".into(),
        _ => c,
    }
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
            region: default_region(),
            timezone: default_tz(),
            opensubtitles_api_key: std::env::var("OPENSUBTITLES_API_KEY").ok(),
            opensubtitles_username: std::env::var("OPENSUBTITLES_USERNAME").ok(),
            opensubtitles_password: std::env::var("OPENSUBTITLES_PASSWORD").ok(),
            subtitles_auto: true,
            simkl_client_id: std::env::var("SIMKL_CLIENT_ID").ok(),
            simkl_scrobble: true,
            anilist_client_id: std::env::var("ANILIST_CLIENT_ID").ok(),
            anilist_sync: true,
            auto_update: true,
            update_prereleases: false,
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
        for (field, var) in [
            (&mut config.opensubtitles_api_key, "OPENSUBTITLES_API_KEY"),
            (&mut config.opensubtitles_username, "OPENSUBTITLES_USERNAME"),
            (&mut config.opensubtitles_password, "OPENSUBTITLES_PASSWORD"),
            (&mut config.simkl_client_id, "SIMKL_CLIENT_ID"),
            (&mut config.anilist_client_id, "ANILIST_CLIENT_ID"),
        ] {
            if field.as_deref().unwrap_or("").is_empty() {
                *field = std::env::var(var).ok();
            }
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

    /// Subtitle languages for OpenSubtitles, best first (from the subtitle
    /// language preference; Czech + English by default)
    pub fn subtitle_languages(&self) -> Vec<String> {
        let v: Vec<String> = self
            .sub_lang
            .as_deref()
            .unwrap_or("")
            .split(',')
            .filter(|l| !l.trim().is_empty())
            .map(lang_code)
            .collect();
        if v.is_empty() {
            vec!["cs".into(), "en".into()]
        } else {
            v
        }
    }

    pub fn opensubtitles_key(&self) -> Option<String> {
        self.opensubtitles_api_key.clone().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
    }

    pub fn simkl_id(&self) -> Option<String> {
        self.simkl_client_id.clone().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
    }

    pub fn anilist_id(&self) -> Option<String> {
        self.anilist_client_id.clone().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
    }

    pub fn mpv_path(&self) -> String {
        self.mpv_path.clone().filter(|p| !p.trim().is_empty()).unwrap_or_else(|| "mpv".to_string())
    }

    /// `~/.config/aurivon/.env`: API keys for the installed app (chmod 600)
    pub fn user_env_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("aurivon").join(".env"))
    }

    pub fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir().context("Could not determine config directory")?;
        Ok(config_dir.join("aurivon").join("config.json"))
    }
}
