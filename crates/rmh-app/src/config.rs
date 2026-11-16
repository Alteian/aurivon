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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tmdb_api_key: std::env::var("TMDB_API_KEY").ok(),
            real_debrid_api_key: std::env::var("REAL_DEBRID_API_KEY").ok(),
            default_library_path: None,
            mpv_path: None,
            volume: None,
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
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create config directory")?;
        }
        info!("Saving config to {:?}", config_path);
        let content = serde_json::to_string_pretty(self).context("Failed to serialize config")?;
        std::fs::write(&config_path, content).context("Failed to write config file")?;
        Ok(())
    }

    pub fn mpv_path(&self) -> String {
        self.mpv_path.clone().filter(|p| !p.trim().is_empty()).unwrap_or_else(|| "mpv".to_string())
    }

    fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir().context("Could not determine config directory")?;
        Ok(config_dir.join("rustmediahub").join("config.json"))
    }
}
