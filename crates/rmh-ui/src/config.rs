use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub tmdb_api_key: Option<String>,
    pub real_debrid_api_key: Option<String>,
    pub default_library_path: Option<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            tmdb_api_key: std::env::var("TMDB_API_KEY").ok(),
            real_debrid_api_key: std::env::var("REAL_DEBRID_API_KEY").ok(),
            default_library_path: None,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let config_path = Self::get_config_path()?;
        
        if !config_path.exists() {
            info!("No config file found at {:?}, using defaults", config_path);
            let config = Self::default();
            // Don't save yet, wait for user to change something or explicit save
            return Ok(config);
        }

        info!("Loading config from {:?}", config_path);
        let content = std::fs::read_to_string(&config_path)
            .context("Failed to read config file")?;
            
        let config: AppConfig = serde_json::from_str(&content)
            .context("Failed to parse config file")?;

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::get_config_path()?;
        
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create config directory")?;
        }

        info!("Saving config to {:?}", config_path);
        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize config")?;
            
        std::fs::write(&config_path, content)
            .context("Failed to write config file")?;

        Ok(())
    }

    fn get_config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
        
        Ok(config_dir.join("rustmediahub").join("config.json"))
    }
}
