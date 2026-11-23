//! Scraper addon for Aurivon
//!
//! This addon scrapes torrents from various sources and resolves them
//! through Real-Debrid for instant cached streaming.

use crate::cache::ScraperCache;
use crate::debrid::{RealDebrid, RealDebridConfig};
use crate::error::{ScraperError, ScraperResult};
use crate::providers::{AllDebrid, TorBox};
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
    pub torbox_api_key: Option<String>,
    pub alldebrid_api_key: Option<String>,
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
            torbox_api_key: None,
            alldebrid_api_key: None,
            enable_torrentio: true,
            min_seeders: 0,
            // keep every result: the stream picker filters and sorts them
            max_results: 500,
        }
    }
}

impl ScraperAddonConfig {
    pub fn with_real_debrid(mut self, api_key: impl Into<String>) -> Self {
        self.real_debrid_api_key = Some(api_key.into());
        self
    }

    pub fn with_torbox(mut self, api_key: impl Into<String>) -> Self {
        self.torbox_api_key = Some(api_key.into());
        self
    }

    pub fn with_alldebrid(mut self, api_key: impl Into<String>) -> Self {
        self.alldebrid_api_key = Some(api_key.into());
        self
    }
}

/// Scraper addon that aggregates torrents and resolves through Real-Debrid
pub struct ScraperAddon {
    manifest: AddonManifest,
    config: ScraperAddonConfig,
    scrapers: Arc<RwLock<ScraperManager>>,
    real_debrid: Option<RealDebrid>,
    torbox: Option<TorBox>,
    alldebrid: Option<AllDebrid>,
    cache: ScraperCache,
    initialized: Arc<RwLock<bool>>,
}

impl ScraperAddon {
    pub fn new(config: ScraperAddonConfig) -> Self {
        let rd = config.real_debrid_api_key.as_ref().map(|key| {
            RealDebrid::new(RealDebridConfig::new(key.clone()))
        });
        let torbox = config.torbox_api_key.as_ref().filter(|k| !k.is_empty()).map(|k| TorBox::new(k.clone()));
        let alldebrid = config.alldebrid_api_key.as_ref().filter(|k| !k.is_empty()).map(|k| AllDebrid::new(k.clone()));

        Self {
            manifest: AddonManifest::new(
                "org.aurivon.scraper",
                "Scraper",
                "1.0.0",
            )
            .with_description("Scrapes torrents from multiple sources with Real-Debrid support")
            .with_resources(vec![Resource::Stream])
            .with_types(vec![ContentType::Movie, ContentType::Series]),
            config,
            scrapers: Arc::new(RwLock::new(ScraperManager::new())),
            real_debrid: rd,
            torbox,
            alldebrid,
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

    /// Search for streams using all scrapers (warnings discarded; prefer [`Self::search_report`]).
    pub async fn search(&self, query: &SearchQuery) -> ScraperResult<Vec<ScrapedResult>> {
        let (hits, _) = self.search_report(query).await?;
        Ok(hits)
    }

    /// Search scrapers and return soft-fail warnings (Torrentio down, etc.).
    pub async fn search_report(&self, query: &SearchQuery) -> ScraperResult<(Vec<ScrapedResult>, Vec<String>)> {
        self.init().await?;

        // Check cache first
        let cache_key = ScraperCache::search_key(
            &query.content_type,
            &query.imdb_id,
            query.season,
            query.episode,
        );

        if let Some(mut cached) = self.cache.get_results(&cache_key).await {
            debug!("Returning cached results for {}", cache_key);
            self.apply_known_rd(&mut cached).await;
            return Ok((cached, vec![]));
        }

        // Search all scrapers
        let scrapers = self.scrapers.read().await;
        let (mut results, warnings) = scrapers.search_all(query).await;

        // Real-Debrid cache status we already know (see `probe_rd`)
        self.apply_known_rd(&mut results).await;

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

        Ok((results, warnings))
    }

    /// Mark results whose Real-Debrid cache status was probed before
    async fn apply_known_rd(&self, results: &mut [ScrapedResult]) {
        if self.real_debrid.is_none() {
            return;
        }
        for r in results.iter_mut() {
            if let Some(cached) = self.cache.is_rd_cached(&r.info_hash).await {
                r.rd_cached = cached;
            }
        }
    }

    /// Ask Real-Debrid whether these torrents are cached (in parallel, a few
    /// at a time) and remember the answers. Returns info hash -> cached.
    ///
    /// Each probe adds the magnet to the RD account and deletes it again, so
    /// callers should only pass the handful of candidates they might play.
    pub async fn probe_rd(&self, items: &[(String, Option<u32>)]) -> HashMap<String, bool> {
        use futures::stream::{self, StreamExt};
        let mut out = HashMap::new();
        let Some(ref rd) = self.real_debrid else { return out };
        let mut todo = Vec::new();
        for (hash, idx) in items {
            match self.cache.is_rd_cached(hash).await {
                Some(c) => {
                    out.insert(hash.clone(), c);
                }
                None => todo.push((hash.clone(), *idx)),
            }
        }
        let probed: Vec<(String, Option<bool>)> = stream::iter(todo)
            .map(|(hash, idx)| async move {
                let r = rd.probe_cached(&hash, idx, false).await;
                if let Err(ref e) = r {
                    warn!("Real-Debrid cache probe failed for {hash}: {e}");
                }
                (hash, r.ok().map(|p| p.cached))
            })
            .buffer_unordered(3)
            .collect()
            .await;
        for (hash, cached) in probed {
            if let Some(c) = cached {
                self.cache.set_rd_cached(&hash, c).await;
                out.insert(hash, c);
            }
        }
        info!("Real-Debrid: {} of {} probed torrents cached", out.values().filter(|c| **c).count(), out.len());
        out
    }

    /// Get a direct streaming URL for a scraped result
    pub async fn resolve_stream(&self, result: &ScrapedResult) -> ScraperResult<String> {
        if let Some(ref rd) = self.real_debrid {
            if result.rd_cached {
                // Get direct streaming link from RD
                let stream = rd.get_stream_link(&result.info_hash, result.file_idx).await?;
                info!("Resolved RD stream: {} ({})", stream.filename, 
                      crate::scrapers::human_readable_size(stream.filesize));
                return Ok(stream.url);
            }
        }

        // Fallback debrid providers when RD did not mark the torrent cached
        if let Some(ref tb) = self.torbox {
            if let Ok(Some(url)) = tb.resolve_cached(&result.info_hash).await {
                return Ok(url);
            }
        }
        if let Some(ref ad) = self.alldebrid {
            if let Ok(Some(url)) = ad.resolve_cached(&result.info_hash).await {
                return Ok(url);
            }
        }

        // Not cached or no debrid - return magnet for torrent streaming
        Ok(result.magnet.clone())
    }

    /// Check if Real-Debrid is configured and working
    pub fn has_real_debrid(&self) -> bool {
        self.real_debrid.is_some()
    }

    pub fn has_any_debrid(&self) -> bool {
        self.real_debrid.is_some() || self.torbox.is_some() || self.alldebrid.is_some()
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
        assert_eq!(addon.manifest().id, "org.aurivon.scraper");
    }
}
