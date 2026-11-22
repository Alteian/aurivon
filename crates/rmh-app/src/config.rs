//! Persistent user configuration (`~/.config/aurivon/config.json`).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

/// Read `AURIVON_*` first, then the legacy `RMH_*` name (pre-rename).
pub fn env_alias(aurivon: &str, legacy_rmh: &str) -> Option<String> {
    for key in [aurivon, legacy_rmh] {
        if let Ok(v) = std::env::var(key) {
            let t = v.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

pub fn env_flag(aurivon: &str, legacy_rmh: &str) -> bool {
    matches!(env_alias(aurivon, legacy_rmh).as_deref(), Some("1") | Some("true") | Some("yes"))
}

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
    /// Trakt API app client id (https://trakt.tv/oauth/applications). Public; PKCE, no secret.
    #[serde(default)]
    pub trakt_client_id: Option<String>,
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

    /// Disk cap for cached posters, backdrops and seek previews (MB); the least
    /// recently used images are removed first. Library posters are kept.
    #[serde(default = "default_image_cache_mb")]
    pub image_cache_mb: u32,
    /// Max on-disk torrent cache (GB). Orphans are cleared on startup; active
    /// streams still need room for the file being played.
    #[serde(default = "default_torrent_cache_gb")]
    pub torrent_cache_gb: u32,
    /// Optional override for the torrent download directory
    #[serde(default)]
    pub torrent_dir: Option<PathBuf>,
    /// Cap for cached TMDB JSON responses (MB)
    #[serde(default = "default_tmdb_cache_mb")]
    pub tmdb_cache_mb: u32,
    /// Cap for release-calendar JSON (MB)
    #[serde(default = "default_calendar_cache_mb")]
    pub calendar_cache_mb: u32,
    /// Cap for downloaded OpenSubtitles files (MB)
    #[serde(default = "default_subs_cache_mb")]
    pub subs_cache_mb: u32,
    /// Prefer less network/disk work (no torrent warm-up until Play, fewer poster prefetches)
    #[serde(default)]
    pub low_power: bool,
    /// Hide watched titles in the Library grid
    #[serde(default)]
    pub library_hide_watched: bool,
    /// Library type filter: "all" | "movie" | "tv" | "other"
    #[serde(default = "default_library_type")]
    pub library_type_filter: String,
    /// Home rows that are visible (missing → shown). Keys: continue, up_next, watchlist, trakt_playback, trending, library
    #[serde(default = "default_home_rows")]
    pub home_rows: Vec<String>,
    /// Set once the streaming home became the default (v0.7.0); older configs
    /// are moved to it a single time, later choices stick
    #[serde(default)]
    pub home_stream_default: bool,
    /// Appearance: theme id (aurum, midnight, oled, aurora, light, violet)
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Accent override "#RRGGBB" (None = the theme's own accent)
    #[serde(default)]
    pub accent_color: Option<String>,
    /// Follow the desktop's light / dark preference (Light theme when light)
    #[serde(default)]
    pub theme_follow_system: bool,
    /// Card style on rows: "poster" | "landscape"
    #[serde(default = "default_card_style")]
    pub card_style: String,
    /// Card size: "s" | "m" | "l"
    #[serde(default = "default_card_size")]
    pub card_size: String,
    /// Navigation: "expanded" | "rail" | "top"
    #[serde(default = "default_nav_style")]
    pub nav_style: String,
    /// Installed HTTP addon base URLs (…/manifest.json resolved)
    #[serde(default)]
    pub addons: Vec<InstalledAddon>,
    /// Built-in Torrentio stream scraper (disable if you use a remote Torrentio addon)
    #[serde(default = "yes")]
    pub enable_torrentio: bool,
    /// Official Trakt catalogs addon (history / watchlist / recommendations rows)
    #[serde(default = "yes")]
    pub enable_trakt_catalogs: bool,
    /// TorBox API key (optional second debrid)
    #[serde(default)]
    pub torbox_api_key: Option<String>,
    /// AllDebrid API key
    #[serde(default)]
    pub alldebrid_api_key: Option<String>,
    /// Prefer Real-Debrid when the torrent cache is near its cap
    #[serde(default = "yes")]
    pub prefer_debrid_on_disk_pressure: bool,
    /// Pre-search streams for the next episode while one is playing
    #[serde(default = "yes")]
    pub next_episode_presearch: bool,
    /// Use TheIntroDB for skip-intro when chapters are missing
    #[serde(default = "yes")]
    pub introdb_skip: bool,
    /// Last Trakt watchlist sync (unix seconds)
    #[serde(default)]
    pub trakt_watchlist_synced_at: Option<i64>,
    /// Last Trakt watched sync (unix seconds)
    #[serde(default)]
    pub trakt_watched_synced_at: Option<i64>,
    /// Watchlist conflict policy when merging with Trakt
    #[serde(default)]
    pub trakt_watchlist_policy: TraktWatchlistPolicy,
    /// Default stream picker filters and sort
    #[serde(default)]
    pub stream_filter: crate::streams::StreamFilter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledAddon {
    pub url: String,
    pub id: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TraktWatchlistPolicy {
    /// Union of local + Trakt (default)
    #[default]
    Merge,
    /// Replace local with Trakt
    TraktWins,
    /// Keep local; only push adds to Trakt
    LocalWins,
}

fn default_torrent_cache_gb() -> u32 {
    8
}
fn default_tmdb_cache_mb() -> u32 {
    50
}
fn default_calendar_cache_mb() -> u32 {
    20
}
fn default_subs_cache_mb() -> u32 {
    50
}
fn default_library_type() -> String {
    "all".into()
}
fn default_theme() -> String {
    "aurum".into()
}
fn default_card_style() -> String {
    "poster".into()
}
fn default_card_size() -> String {
    "m".into()
}
fn default_nav_style() -> String {
    "expanded".into()
}
fn default_home_rows() -> Vec<String> {
    vec![
        "continue".into(),
        "up_next".into(),
        "watchlist".into(),
        "trakt_playback".into(),
        "trending".into(),
        "library".into(),
    ]
}

fn default_image_cache_mb() -> u32 {
    250
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
    /// Streaming-first: hero carousel + Continue/Up next + discovery rows
    #[default]
    Stream,
    /// Hero-first: a tall cinematic hero, then the same rows
    Cinematic,
    /// Dense: no hero, small cards, tight rows
    Compact,
    /// A: Continue Watching + Up Next
    Focus,
    /// B: Trending / Discover with a hero banner
    Discover,
    /// C: Library grid with a small Continue Watching row
    Library,
}

impl HomeLayout {
    pub const ALL: [HomeLayout; 6] =
        [HomeLayout::Stream, HomeLayout::Cinematic, HomeLayout::Compact, HomeLayout::Focus, HomeLayout::Discover, HomeLayout::Library];
    /// Layouts built on the streaming feed (hero + discovery rows)
    pub fn is_feed(self) -> bool {
        matches!(self, HomeLayout::Stream | HomeLayout::Cinematic | HomeLayout::Compact)
    }
    /// Short letter shown on the layout switcher
    pub fn letter(self) -> &'static str {
        match self {
            HomeLayout::Stream => "S",
            HomeLayout::Cinematic => "H",
            HomeLayout::Compact => "D",
            HomeLayout::Focus => "A",
            HomeLayout::Discover => "B",
            HomeLayout::Library => "C",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            HomeLayout::Stream => "Streaming",
            HomeLayout::Cinematic => "Cinematic",
            HomeLayout::Compact => "Compact",
            HomeLayout::Focus => "Continue & Up Next",
            HomeLayout::Discover => "Discover",
            HomeLayout::Library => "Library",
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoMode {
    /// mpv draws into a surface inside the app window (X11 / XWayland)
    Embedded,
    /// mpv opens its own window with its full on-screen controller
    Window,
}

impl Default for VideoMode {
    fn default() -> Self {
        // Embedded needs X11; Windows (and other non-Linux) use a separate mpv window.
        if cfg!(target_os = "linux") {
            Self::Embedded
        } else {
            Self::Window
        }
    }
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
            image_cache_mb: default_image_cache_mb(),
            torrent_cache_gb: default_torrent_cache_gb(),
            torrent_dir: None,
            tmdb_cache_mb: default_tmdb_cache_mb(),
            calendar_cache_mb: default_calendar_cache_mb(),
            subs_cache_mb: default_subs_cache_mb(),
            low_power: false,
            library_hide_watched: false,
            library_type_filter: default_library_type(),
            home_rows: default_home_rows(),
            home_stream_default: true,
            theme: default_theme(),
            accent_color: None,
            theme_follow_system: false,
            card_style: default_card_style(),
            card_size: default_card_size(),
            nav_style: default_nav_style(),
            addons: vec![],
            enable_torrentio: true,
            enable_trakt_catalogs: true,
            torbox_api_key: std::env::var("TORBOX_API_KEY").ok(),
            alldebrid_api_key: std::env::var("ALLDEBRID_API_KEY").ok(),
            prefer_debrid_on_disk_pressure: true,
            next_episode_presearch: true,
            introdb_skip: true,
            trakt_watchlist_synced_at: None,
            trakt_watched_synced_at: None,
            trakt_watchlist_policy: TraktWatchlistPolicy::default(),
            stream_filter: Default::default(),
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
        for (field, var) in [
            (&mut config.opensubtitles_api_key, "OPENSUBTITLES_API_KEY"),
            (&mut config.opensubtitles_username, "OPENSUBTITLES_USERNAME"),
            (&mut config.opensubtitles_password, "OPENSUBTITLES_PASSWORD"),
            (&mut config.simkl_client_id, "SIMKL_CLIENT_ID"),
            (&mut config.anilist_client_id, "ANILIST_CLIENT_ID"),
            (&mut config.torbox_api_key, "TORBOX_API_KEY"),
            (&mut config.alldebrid_api_key, "ALLDEBRID_API_KEY"),
        ] {
            if field.as_deref().unwrap_or("").is_empty() {
                *field = std::env::var(var).ok();
            }
        }
        if config.home_rows.is_empty() {
            config.home_rows = default_home_rows();
        }
        if !config.home_stream_default {
            config.home_layout = HomeLayout::Stream;
            config.home_stream_default = true;
        }
        Ok(config)
    }

    pub fn torbox_key(&self) -> Option<String> {
        self.torbox_api_key.clone().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
    }

    pub fn alldebrid_key(&self) -> Option<String> {
        self.alldebrid_api_key.clone().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
    }

    pub fn home_row_visible(&self, key: &str) -> bool {
        self.home_rows.iter().any(|r| r == key)
    }

    /// Trakt client id: Settings / `TRAKT_CLIENT_ID` override, else the baked-in public app id.
    pub fn trakt_id(&self) -> Option<String> {
        self.trakt_client_id
            .as_ref()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .or_else(|| Some(rmh_trakt::DEFAULT_CLIENT_ID.to_string()))
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
        if let Some(p) = self.mpv_path.clone().filter(|p| !p.trim().is_empty()) {
            return p;
        }
        // Windows installer / portable zip ships mpv next to aurivon.exe
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let bundled = dir.join("mpv").join(if cfg!(windows) { "mpv.exe" } else { "mpv" });
                if bundled.is_file() {
                    return bundled.to_string_lossy().into_owned();
                }
                let beside = dir.join(if cfg!(windows) { "mpv.exe" } else { "mpv" });
                if beside.is_file() {
                    return beside.to_string_lossy().into_owned();
                }
            }
        }
        if cfg!(windows) {
            "mpv.exe".into()
        } else {
            "mpv".into()
        }
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
