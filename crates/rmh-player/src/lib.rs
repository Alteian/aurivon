//! Video player integration using libmpv
//! 
//! This module uses the libmpv library to play video files directly.

use anyhow::{Context, Result};
use libmpv2::Mpv;
use std::path::Path;
use tracing::{info, debug};

/// Configuration for MPV player
#[derive(Debug, Clone)]
pub struct PlayerConfig {
    /// Hardware decoding mode (auto, vaapi, nvdec, etc.)
    pub hwdec: String,
    /// Start in fullscreen mode
    pub fullscreen: bool,
    /// Show on-screen controller
    pub osc: bool,
    /// Volume level (0-100)
    pub volume: u8,
    /// Resume from last position
    pub save_position: bool,
    /// Additional custom arguments
    pub extra_args: Vec<String>,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            hwdec: "auto".to_string(),
            fullscreen: true,
            osc: true,
            volume: 100,
            save_position: true,
            extra_args: Vec::new(),
        }
    }
}

/// Video player that manages MPV instance
pub struct Player {
    config: PlayerConfig,
    current_mpv: Option<Mpv>,
}

impl Player {
    /// Create a new player with the given configuration
    pub fn new(config: PlayerConfig) -> Self {
        Self {
            config,
            current_mpv: None,
        }
    }

    /// Create a player with default configuration
    pub fn with_defaults() -> Self {
        Self::new(PlayerConfig::default())
    }

    /// Update the player configuration
    pub fn set_config(&mut self, config: PlayerConfig) {
        self.config = config;
        // Apply runtime changes if player is active
        if let Some(ref mpv) = self.current_mpv {
            let _ = mpv.set_property("volume", self.config.volume as i64);
            let _ = mpv.set_property("osc", if self.config.osc { "yes" } else { "no" });
        }
    }

    /// Initialize a new MPV instance or return existing
    fn ensure_mpv(&mut self) -> Result<&Mpv> {
        if self.current_mpv.is_none() {
            let mpv = Mpv::new().map_err(|e| anyhow::anyhow!("Failed to initialize libmpv: {:?}", e))?;

            // Configure hardware acceleration
            mpv.set_property("hwdec", self.config.hwdec.as_str())
                .map_err(|e| anyhow::anyhow!("Failed to set hwdec: {:?}", e))?;

            // Configure windowing
            mpv.set_property("vo", "gpu").map_err(|e| anyhow::anyhow!("MPV setup error: {:?}", e))?;
            mpv.set_property("input-default-bindings", "yes").map_err(|e| anyhow::anyhow!("MPV setup error: {:?}", e))?;
            mpv.set_property("input-vo-keyboard", "yes").map_err(|e| anyhow::anyhow!("MPV setup error: {:?}", e))?;
            mpv.set_property("osc", if self.config.osc { "yes" } else { "no" }).map_err(|e| anyhow::anyhow!("MPV setup error: {:?}", e))?;
            mpv.set_property("fullscreen", if self.config.fullscreen { "yes" } else { "no" }).map_err(|e| anyhow::anyhow!("MPV setup error: {:?}", e))?;
            mpv.set_property("volume", self.config.volume as i64).map_err(|e| anyhow::anyhow!("MPV setup error: {:?}", e))?;

            // Network properties
            mpv.set_property("cache", "yes").map_err(|e| anyhow::anyhow!("MPV setup error: {:?}", e))?;
            mpv.set_property("cache-secs", 120).map_err(|e| anyhow::anyhow!("MPV setup error: {:?}", e))?;
            mpv.set_property("demuxer-max-bytes", "500MiB").map_err(|e| anyhow::anyhow!("MPV setup error: {:?}", e))?;

             // Behavior
            mpv.set_property("keep-open", "yes").map_err(|e| anyhow::anyhow!("MPV setup error: {:?}", e))?;
            mpv.set_property("force-window", "immediate").map_err(|e| anyhow::anyhow!("MPV setup error: {:?}", e))?;

            // Enable verbose terminal output for debugging
            // mpv.set_property("terminal", "yes").ok(); // Only works if we attach stdout/err
            mpv.set_property("msg-level", "all=v").ok(); // Set verbose logging
            mpv.set_property("terminal", "yes").ok(); // Attempt to print to terminal
            
            self.current_mpv = Some(mpv);
        }
        
        Ok(self.current_mpv.as_ref().unwrap())
    }

    /// Play a video file
    pub fn play(&mut self, file_path: &Path) -> Result<()> {
        self.play_at_position(file_path, None)
    }

    /// Play a video file starting at specific position (seconds)
    pub fn play_at_position(&mut self, file_path: &Path, start_time: Option<u64>) -> Result<()> {
        let path_str = file_path.to_str().context("Invalid path")?;
        info!("Playing file: {} (start: {:?})", path_str, start_time);
        
        // Ensure MPV is initialized
        let _ = self.ensure_mpv()?;
        let mpv = self.current_mpv.as_ref().unwrap();

        // Construct loadfile command args
        // "loadfile" "path" "replace" "options"
        let mut args = vec![path_str, "replace"];
        let start_arg;
        if let Some(t) = start_time {
            start_arg = format!("start={}", t);
            args.push(&start_arg);
        }

        mpv.command("loadfile", &args).map_err(|e| anyhow::anyhow!("Failed to load file: {:?}", e))?;
        
        Ok(())
    }

    /// Play a URL
    pub fn play_url(&mut self, url: &str) -> Result<()> {
        info!("Playing URL: {}", url);
        let _ = self.ensure_mpv()?;
        let mpv = self.current_mpv.as_ref().unwrap();
        
        mpv.command("loadfile", &[url, "replace"]).map_err(|e| anyhow::anyhow!("Failed to load URL: {:?}", e))?;
        Ok(())
    }

    /// Stop playback
    pub fn stop(&mut self) -> Result<()> {
        if let Some(ref mpv) = self.current_mpv {
            mpv.command("stop", &[]).map_err(|e| anyhow::anyhow!("Failed to stop: {:?}", e))?;
            // Destroy instance to close window
            self.current_mpv = None;
        }
        Ok(())
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // libmpv handles cleanup automatically when Mpv struct is dropped
    }
}

