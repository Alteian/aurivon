//! Torrent streaming addon for RustMediaHub
//!
//! This addon provides stream resolution for magnet links and torrent files.

use rmh_addon::manifest::{AddonManifest, ContentType, Resource};
use rmh_addon::resources::Stream;
use rmh_addon::traits::{Addon, AddonError, AddonResult, StreamProvider};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::{TorrentEngine, TorrentEngineConfig};

/// Addon that provides torrent streaming capabilities
pub struct TorrentAddon {
    manifest: AddonManifest,
    engine: Arc<RwLock<TorrentEngine>>,
    initialized: Arc<RwLock<bool>>,
}

impl TorrentAddon {
    /// Create a new torrent addon
    pub fn new() -> Self {
        Self::with_config(TorrentEngineConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: TorrentEngineConfig) -> Self {
        Self {
            manifest: AddonManifest::new(
                "org.rustmediahub.torrent",
                "Torrent Streaming",
                "1.0.0",
            )
            .with_description("Stream torrents and magnet links directly")
            .with_resources(vec![Resource::Stream])
            .with_types(vec![ContentType::Movie, ContentType::Series]),
            engine: Arc::new(RwLock::new(TorrentEngine::new(config))),
            initialized: Arc::new(RwLock::new(false)),
        }
    }

    /// Initialize the torrent engine
    pub async fn init(&self) -> AddonResult<()> {
        let mut init = self.initialized.write().await;
        if *init {
            return Ok(());
        }

        let mut engine = self.engine.write().await;
        engine.initialize().await
            .map_err(|e| AddonError::Internal(e.to_string()))?;

        *init = true;
        Ok(())
    }

    /// Stream a magnet link directly
    pub async fn stream_magnet(&self, magnet: &str, file_index: Option<usize>) -> AddonResult<String> {
        self.init().await?;

        let engine = self.engine.read().await;
        let stream = engine.add_magnet(magnet, file_index).await
            .map_err(|e| AddonError::Internal(e.to_string()))?;

        Ok(stream.stream_url().to_string())
    }

    /// Get the local streaming server URL
    pub async fn server_url(&self) -> String {
        let engine = self.engine.read().await;
        engine.server_url()
    }
}

impl Default for TorrentAddon {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Addon for TorrentAddon {
    fn manifest(&self) -> &AddonManifest {
        &self.manifest
    }
}

#[async_trait]
impl StreamProvider for TorrentAddon {
    async fn streams(
        &self,
        _content_type: &ContentType,
        id: &str,
    ) -> AddonResult<Vec<Stream>> {
        // Check if this is a magnet link ID
        if !id.starts_with("magnet:") && !id.starts_with("torrent:") {
            return Ok(vec![]);
        }

        self.init().await?;

        let magnet = if id.starts_with("torrent:") {
            &id[8..] // Strip "torrent:" prefix
        } else {
            id
        };

        let engine = self.engine.read().await;
        
        let stream = engine.add_magnet(magnet, None).await
            .map_err(|e| AddonError::Internal(e.to_string()))?;

        let result = Stream::http(stream.stream_url())
            .with_name("Torrent Stream")
            .with_title(stream.info.filename.clone());

        Ok(vec![result])
    }
}
