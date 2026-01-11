//! Scraper addon for RustMediaHub
//!
//! This addon scrapes torrents from various sources and resolves them
//! through Real-Debrid for instant cached streaming.

use crate::cache::ScraperCache;
use crate::debrid::{RealDebrid, RealDebridConfig};
use crate::error::{ScraperError, ScraperResult};
use crate::scrapers::{ScrapedResult, ScraperManager, SearchQuery};
use crate::scrapers::torrentio::{TorrentioScraper, TorrentioConfig};

use rmh_addon::manifest::{AddonManifest, ContentType, Resource};
use rmh_addon::resources::Stream;
use rmh_addon::traits::{Addon, AddonError, AddonResult, StreamProvider};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Configuration for the scraper addon
#[derive(Debug, Clone)]
pub struct ScraperAddonConfig {
    /// Real-Debrid API key
    pub real_debrid_api_key: Option<String>,
    /// Enable Torrentio scraper
    pub enable_torrentio: bool,
    /// Minimum seeders to show (for non-cached results)
    pub min_seeders: u32,
    /// Maximum results to return per source
    pub max_results: usize,
}

impl Default for ScraperAddonConfig {
    fn default() -> Self {
        Self {
            real_debrid_api_key: None,
            enable_torrentio: true,
            min_seeders: 0,
            max_results: 50,
        }
    }
}

impl ScraperAddonConfig {
    pub fn with_real_debrid(mut self, api_key: impl Into<String>) -> Self {
        self.real_debrid_api_key = Some(api_key.into());
        self
    }
}

/// Scraper addon that aggregates torrents and resolves through Real-Debrid
pub struct ScraperAddon {
    manifest: AddonManifest,
    config: ScraperAddonConfig,
    scrapers: Arc<RwLock<ScraperManager>>,
    real_debrid: Option<RealDebrid>,
    cache: ScraperCache,
    initialized: Arc<RwLock<bool>>,
}

impl ScraperAddon {
    pub fn new(config: ScraperAddonConfig) -> Self {
        let rd = config.real_debrid_api_key.as_ref().map(|key| {
            RealDebrid::new(RealDebridConfig::new(key.clone()))
        });

        Self {
            manifest: AddonManifest::new(
                "org.rustmediahub.scraper",
                "Scraper",
                "1.0.0",
            )
            .with_description("Scrapes torrents from multiple sources with Real-Debrid support")
            .with_resources(vec![Resource::Stream])
            .with_types(vec![ContentType::Movie, ContentType::Series]),
            config,
            scrapers: Arc::new(RwLock::new(ScraperManager::new())),
            real_debrid: rd,
            cache: ScraperCache::new(),
            initialized: Arc::new(RwLock::new(false)),
        }
    }

    /// Initialize the addon and verify API key
    pub async fn init(&self) -> ScraperResult<()> {
        let mut init = self.initialized.write().await;
        if *init {
            return Ok(());
        }

        // Verify Real-Debrid API key if configured
        if let Some(ref rd) = self.real_debrid {
            match rd.verify_api_key().await {
                Ok(user) => {
                    info!("Real-Debrid connected: {} (Premium: {})", user.username, user.premium > 0);
                }
                Err(ScraperError::InvalidApiKey) => {
                    warn!("Invalid Real-Debrid API key");
                }
                Err(e) => {
                    warn!("Could not verify Real-Debrid: {}", e);
                }
            }
        }

        // Add scrapers
        let mut scrapers = self.scrapers.write().await;
        
        if self.config.enable_torrentio {
            let torrentio_config = TorrentioConfig::default();
            // Note: We check RD cache ourselves, so don't pass key to Torrentio
            scrapers.add_scraper(TorrentioScraper::new(torrentio_config));
        }

        *init = true;
        Ok(())
    }

    /// Search for streams using all scrapers
    pub async fn search(&self, query: &SearchQuery) -> ScraperResult<Vec<ScrapedResult>> {
        self.init().await?;

        // Check cache first
        let cache_key = ScraperCache::search_key(
            &query.content_type,
            &query.imdb_id,
            query.season,
            query.episode,
        );

        if let Some(cached) = self.cache.get_results(&cache_key).await {
            debug!("Returning cached results for {}", cache_key);
            return Ok(cached);
        }

        // Search all scrapers
        let scrapers = self.scrapers.read().await;
        let mut results = scrapers.search_all(query).await;

        // Check Real-Debrid cache status
        if let Some(ref rd) = self.real_debrid {
            results = self.check_rd_cache(rd, results).await;
        }

        // Sort by score (RD cached + quality)
        results.sort_by(|a, b| b.score().cmp(&a.score()));

        // Filter by minimum seeders (except cached results)
        let results: Vec<ScrapedResult> = results
            .into_iter()
            .filter(|r| r.rd_cached || r.seeders.unwrap_or(0) >= self.config.min_seeders)
            .take(self.config.max_results)
            .collect();

        // Cache results
        self.cache.set_results(&cache_key, results.clone()).await;

        Ok(results)
    }

    /// Check which results are cached on Real-Debrid
    async fn check_rd_cache(&self, rd: &RealDebrid, mut results: Vec<ScrapedResult>) -> Vec<ScrapedResult> {
        // Collect hashes that need checking (skip already known)
        let mut hashes_to_check = Vec::new();
        let mut known_cached = HashMap::new();

        for result in &results {
            if let Some(cached) = self.cache.is_rd_cached(&result.info_hash).await {
                known_cached.insert(result.info_hash.clone(), cached);
            } else {
                hashes_to_check.push(result.info_hash.clone());
            }
        }

        // Check unknown hashes with RD
        if !hashes_to_check.is_empty() {
            debug!("Checking {} hashes with Real-Debrid", hashes_to_check.len());
            
            match rd.check_instant_availability(&hashes_to_check).await {
                Ok(cached_map) => {
                    // Update cache and known_cached
                    for hash in &hashes_to_check {
                        let is_cached = cached_map.contains_key(&hash.to_lowercase());
                        self.cache.set_rd_cached(hash, is_cached).await;
                        known_cached.insert(hash.clone(), is_cached);
                    }
                }
                Err(e) => {
                    warn!("Failed to check RD cache: {}", e);
                }
            }
        }

        // Update results with cache status
        for result in &mut results {
            if let Some(&cached) = known_cached.get(&result.info_hash) {
                result.rd_cached = cached;
            }
        }

        results
    }

    /// Get a direct streaming URL for a scraped result
    pub async fn resolve_stream(&self, result: &ScrapedResult) -> ScraperResult<String> {
        if let Some(ref rd) = self.real_debrid {
            if result.rd_cached {
                // Get direct streaming link from RD
                let stream = rd.get_stream_link(&result.info_hash, None).await?;
                info!("Resolved RD stream: {} ({})", stream.filename, 
                      crate::scrapers::human_readable_size(stream.filesize));
                return Ok(stream.url);
            }
        }

        // Not cached or no RD - return magnet for torrent streaming
        Ok(result.magnet.clone())
    }

    /// Check if Real-Debrid is configured and working
    pub fn has_real_debrid(&self) -> bool {
        self.real_debrid.is_some()
    }
}

// Make ScraperAddon usable as an addon

#[async_trait]
impl Addon for ScraperAddon {
    fn manifest(&self) -> &AddonManifest {
        &self.manifest
    }
}

#[async_trait]
impl StreamProvider for ScraperAddon {
    async fn streams(
        &self,
        content_type: &ContentType,
        id: &str,
    ) -> AddonResult<Vec<Stream>> {
        // Parse ID format: "tt1234567" or "tt1234567:1:1" for series
        let parts: Vec<&str> = id.split(':').collect();
        let imdb_id = parts.first().copied().unwrap_or(id);
        
        // Validate IMDB ID format
        if !imdb_id.starts_with("tt") {
            return Ok(vec![]);
        }

        let query = match content_type {
            ContentType::Movie => SearchQuery::movie(imdb_id, ""),
            ContentType::Series => {
                let season = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                let episode = parts.get(2).and_then(|e| e.parse().ok()).unwrap_or(1);
                SearchQuery::series(imdb_id, "", season, episode)
            }
            _ => return Ok(vec![]),
        };

        let results = self.search(&query).await
            .map_err(|e| AddonError::Internal(e.to_string()))?;

        // Convert to addon Stream format
        let mut streams = Vec::new();

        for result in results {
            // Build descriptive title
            let mut title_parts = vec![result.parsed.quality_string()];
            
            if let Some(ref size) = result.size_str {
                title_parts.push(size.clone());
            }
            
            if let Some(seeders) = result.seeders {
                title_parts.push(format!("👤 {}", seeders));
            }

            let title = title_parts.join(" | ");

            // Build stream name with source and cache status
            let name = if result.rd_cached {
                format!("⚡ {} [RD]", result.source)
            } else {
                result.source.clone()
            };

            // Resolve the stream URL
            let url = match self.resolve_stream(&result).await {
                Ok(url) => url,
                Err(e) => {
                    debug!("Failed to resolve stream: {}", e);
                    result.magnet.clone()
                }
            };

            let stream = if url.starts_with("magnet:") {
                // Torrent stream - needs rmh-torrent to handle
                Stream::torrent(&result.info_hash, None)
                    .with_title(&title)
                    .with_name(&name)
            } else {
                // Direct HTTP stream from Real-Debrid
                Stream::http(&url)
                    .with_title(&title)
                    .with_name(&name)
            };

            streams.push(stream);
        }

        Ok(streams)
    }
}

/// Helper to create a preconfigured scraper addon with Real-Debrid
pub fn create_scraper_addon(rd_api_key: Option<&str>) -> ScraperAddon {
    let mut config = ScraperAddonConfig::default();
    
    if let Some(key) = rd_api_key {
        config = config.with_real_debrid(key);
    }

    ScraperAddon::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_scraper_addon_creation() {
        let addon = create_scraper_addon(Some("test_key"));
        assert!(addon.has_real_debrid());
        assert_eq!(addon.manifest().id, "org.rustmediahub.scraper");
    }
}
