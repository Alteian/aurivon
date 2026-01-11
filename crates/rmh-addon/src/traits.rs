//! Addon traits for implementing custom addons

use crate::manifest::{AddonManifest, ContentType};
use crate::resources::{CatalogResponse, MetaItem, Stream, Subtitle};
use async_trait::async_trait;
use std::collections::HashMap;

/// Result type for addon operations
pub type AddonResult<T> = Result<T, AddonError>;

/// Error type for addon operations
#[derive(Debug, thiserror::Error)]
pub enum AddonError {
    #[error("Resource not supported: {0}")]
    NotSupported(String),
    
    #[error("Content not found: {0}")]
    NotFound(String),
    
    #[error("Network error: {0}")]
    Network(String),
    
    #[error("Parse error: {0}")]
    Parse(String),
    
    #[error("Configuration required")]
    ConfigurationRequired,
    
    #[error("Rate limited, retry after {0} seconds")]
    RateLimited(u32),
    
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Base trait for all addons
#[async_trait]
pub trait Addon: Send + Sync {
    /// Get the addon manifest
    fn manifest(&self) -> &AddonManifest;
    
    /// Unique addon identifier
    fn id(&self) -> &str {
        &self.manifest().id
    }
    
    /// Human-readable name
    fn name(&self) -> &str {
        &self.manifest().name
    }
    
    /// Check if addon is properly configured
    fn is_configured(&self) -> bool {
        true
    }
    
    /// Initialize the addon (called once on load)
    async fn initialize(&mut self) -> AddonResult<()> {
        Ok(())
    }
}

/// Extra parameters for catalog/search requests
#[derive(Debug, Clone, Default)]
pub struct ExtraParams {
    /// Search query
    pub search: Option<String>,
    /// Genre filter
    pub genre: Option<String>,
    /// Skip/offset for pagination
    pub skip: Option<u32>,
    /// Custom parameters
    pub custom: HashMap<String, String>,
}

impl ExtraParams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_search(mut self, query: impl Into<String>) -> Self {
        self.search = Some(query.into());
        self
    }

    pub fn with_genre(mut self, genre: impl Into<String>) -> Self {
        self.genre = Some(genre.into());
        self
    }

    pub fn with_skip(mut self, skip: u32) -> Self {
        self.skip = Some(skip);
        self
    }
}

/// Trait for addons that provide catalogs (browseable content lists)
#[async_trait]
pub trait CatalogProvider: Addon {
    /// Get catalog content
    /// 
    /// # Arguments
    /// * `content_type` - Type of content (movie, series, etc.)
    /// * `catalog_id` - ID of the catalog to fetch
    /// * `extra` - Extra parameters (search, filters, pagination)
    async fn catalog(
        &self,
        content_type: &ContentType,
        catalog_id: &str,
        extra: &ExtraParams,
    ) -> AddonResult<CatalogResponse>;
}

/// Trait for addons that provide metadata
#[async_trait]
pub trait MetaProvider: Addon {
    /// Get detailed metadata for a content item
    /// 
    /// # Arguments
    /// * `content_type` - Type of content
    /// * `id` - Content ID (e.g., IMDB ID, TMDB ID)
    async fn meta(
        &self,
        content_type: &ContentType,
        id: &str,
    ) -> AddonResult<MetaItem>;
}

/// Trait for addons that provide streams
#[async_trait]
pub trait StreamProvider: Addon {
    /// Get available streams for a content item
    /// 
    /// # Arguments
    /// * `content_type` - Type of content
    /// * `id` - Content ID
    async fn streams(
        &self,
        content_type: &ContentType,
        id: &str,
    ) -> AddonResult<Vec<Stream>>;
}

/// Trait for addons that provide subtitles
#[async_trait]
pub trait SubtitleProvider: Addon {
    /// Get available subtitles for a content item
    /// 
    /// # Arguments
    /// * `content_type` - Type of content
    /// * `id` - Content ID
    /// * `video_id` - Optional video/episode ID for series
    async fn subtitles(
        &self,
        content_type: &ContentType,
        id: &str,
        video_id: Option<&str>,
    ) -> AddonResult<Vec<Subtitle>>;
}

/// Combined addon that can provide multiple resources
/// This is a convenience struct for addons that implement multiple traits
pub struct CombinedAddon {
    manifest: AddonManifest,
    catalog: Option<Box<dyn CatalogProvider>>,
    meta: Option<Box<dyn MetaProvider>>,
    streams: Option<Box<dyn StreamProvider>>,
    subtitles: Option<Box<dyn SubtitleProvider>>,
}

impl CombinedAddon {
    pub fn new(manifest: AddonManifest) -> Self {
        Self {
            manifest,
            catalog: None,
            meta: None,
            streams: None,
            subtitles: None,
        }
    }

    pub fn with_catalog(mut self, provider: Box<dyn CatalogProvider>) -> Self {
        self.catalog = Some(provider);
        self
    }

    pub fn with_meta(mut self, provider: Box<dyn MetaProvider>) -> Self {
        self.meta = Some(provider);
        self
    }

    pub fn with_streams(mut self, provider: Box<dyn StreamProvider>) -> Self {
        self.streams = Some(provider);
        self
    }

    pub fn with_subtitles(mut self, provider: Box<dyn SubtitleProvider>) -> Self {
        self.subtitles = Some(provider);
        self
    }
}
