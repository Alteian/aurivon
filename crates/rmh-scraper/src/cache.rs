//! Caching layer for scraped results

use moka::future::Cache;
use std::time::Duration;

use crate::scrapers::ScrapedResult;

/// Cache for scraped results to avoid repeated requests
pub struct ScraperCache {
    /// Cache keyed by search query (e.g., "movie:tt1234567")
    results: Cache<String, Vec<ScrapedResult>>,
    /// Cache keyed by info_hash for instant availability status
    cached_status: Cache<String, bool>,
}

impl ScraperCache {
    pub fn new() -> Self {
        Self {
            // Cache search results for 30 minutes
            results: Cache::builder()
                .time_to_live(Duration::from_secs(30 * 60))
                .max_capacity(1000)
                .build(),
            // Cache RD availability for 1 hour
            cached_status: Cache::builder()
                .time_to_live(Duration::from_secs(60 * 60))
                .max_capacity(10000)
                .build(),
        }
    }

    /// Generate a cache key for a search query
    pub fn search_key(content_type: &str, imdb_id: &str, season: Option<u32>, episode: Option<u32>) -> String {
        match (season, episode) {
            (Some(s), Some(e)) => format!("{}:{}:S{:02}E{:02}", content_type, imdb_id, s, e),
            (Some(s), None) => format!("{}:{}:S{:02}", content_type, imdb_id, s),
            _ => format!("{}:{}", content_type, imdb_id),
        }
    }

    /// Get cached search results
    pub async fn get_results(&self, key: &str) -> Option<Vec<ScrapedResult>> {
        self.results.get(key).await
    }

    /// Cache search results
    pub async fn set_results(&self, key: &str, results: Vec<ScrapedResult>) {
        self.results.insert(key.to_string(), results).await;
    }

    /// Check if a hash is known to be cached on RD
    pub async fn is_rd_cached(&self, info_hash: &str) -> Option<bool> {
        self.cached_status.get(&info_hash.to_lowercase()).await
    }

    /// Mark hashes as cached or not
    pub async fn set_rd_cached(&self, info_hash: &str, cached: bool) {
        self.cached_status.insert(info_hash.to_lowercase(), cached).await;
    }

    /// Clear all caches
    pub async fn clear(&self) {
        self.results.invalidate_all();
        self.cached_status.invalidate_all();
    }
}

impl Default for ScraperCache {
    fn default() -> Self {
        Self::new()
    }
}
