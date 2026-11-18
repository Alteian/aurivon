//! Scraper implementations for various torrent sources
//!
//! Each scraper implements the `Scraper` trait to search for torrents

pub mod torrentio;

use crate::error::ScraperResult;
use crate::parser::{TorrentInfo, parse_torrent_title};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// A scraped torrent result
#[derive(Debug, Clone)]
pub struct ScrapedResult {
    /// Torrent title
    pub title: String,
    /// Magnet link or info hash
    pub magnet: String,
    /// Info hash (lowercase, 40 chars)
    pub info_hash: String,
    /// Parsed torrent information
    pub parsed: TorrentInfo,
    /// Source scraper name
    pub source: String,
    /// Number of seeders
    pub seeders: Option<u32>,
    /// Number of leechers
    pub leechers: Option<u32>,
    /// File size in bytes
    pub size: Option<u64>,
    /// File size as human readable string
    pub size_str: Option<String>,
    /// Whether this is cached on Real-Debrid
    pub rd_cached: bool,
    /// Languages (if known)
    pub languages: Vec<String>,
    /// Index of the video file inside the torrent (season packs, extras)
    pub file_idx: Option<u32>,
    /// Trackers announced for this torrent (peer discovery without waiting on DHT)
    pub trackers: Vec<String>,
}

impl ScrapedResult {
    /// Create a new scraped result from a magnet link
    pub fn from_magnet(title: &str, magnet: &str, source: &str) -> Option<Self> {
        let info_hash = extract_info_hash(magnet)?;
        let parsed = parse_torrent_title(title);

        Some(Self {
            title: title.to_string(),
            magnet: magnet.to_string(),
            info_hash,
            parsed,
            source: source.to_string(),
            seeders: None,
            leechers: None,
            size: None,
            size_str: None,
            rd_cached: false,
            languages: Vec::new(),
            file_idx: None,
            trackers: Vec::new(),
        })
    }

    /// Create from info hash directly
    pub fn from_hash(title: &str, info_hash: &str, source: &str) -> Self {
        let hash = info_hash.to_lowercase();
        let parsed = parse_torrent_title(title);
        
        Self {
            title: title.to_string(),
            magnet: format!("magnet:?xt=urn:btih:{}", hash),
            info_hash: hash,
            parsed,
            source: source.to_string(),
            seeders: None,
            leechers: None,
            size: None,
            size_str: None,
            rd_cached: false,
            languages: Vec::new(),
            file_idx: None,
            trackers: Vec::new(),
        }
    }

    /// Add trackers to the result and its magnet link
    pub fn with_trackers(mut self, trackers: Vec<String>) -> Self {
        for t in &trackers {
            self.magnet.push_str("&tr=");
            self.magnet.push_str(&url_encode(t));
        }
        self.trackers = trackers;
        self
    }

    pub fn with_seeders(mut self, seeders: u32) -> Self {
        self.seeders = Some(seeders);
        self.parsed.seeders = Some(seeders);
        self
    }

    pub fn with_leechers(mut self, leechers: u32) -> Self {
        self.leechers = Some(leechers);
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self.size_str = Some(human_readable_size(size));
        self
    }

    pub fn with_size_str(mut self, size_str: impl Into<String>) -> Self {
        self.size_str = Some(size_str.into());
        self
    }

    /// Quality score for sorting (higher = better)
    pub fn score(&self) -> u32 {
        let mut score = self.parsed.quality_score();
        
        // RD cached gets a major boost
        if self.rd_cached {
            score += 10000;
        }
        
        score
    }
}

/// Extract info_hash from a magnet link
pub fn extract_info_hash(magnet: &str) -> Option<String> {
    // Find xt=urn:btih: parameter
    let magnet_lower = magnet.to_lowercase();
    let start = magnet_lower.find("xt=urn:btih:")?;
    let hash_start = start + "xt=urn:btih:".len();
    
    // Find end (& or end of string)
    let rest = &magnet[hash_start..];
    let hash = rest.split('&').next()?;
    
    // Handle base32 encoded hashes (32 chars) vs hex (40 chars)
    let normalized = if hash.len() == 32 {
        // Base32 to hex conversion
        base32_to_hex(hash)?
    } else if hash.len() == 40 {
        hash.to_lowercase()
    } else {
        return None;
    };

    Some(normalized)
}

/// Convert base32 to hex (for some magnet links)
fn base32_to_hex(input: &str) -> Option<String> {
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let input_upper = input.to_uppercase();
    
    let mut bits = String::new();
    for c in input_upper.chars() {
        let val = alphabet.find(c)?;
        bits.push_str(&format!("{:05b}", val));
    }
    
    // Convert bits to hex
    let mut hex = String::new();
    for chunk in bits.as_bytes().chunks(4) {
        let nibble: String = chunk.iter().map(|&b| b as char).collect();
        if let Ok(val) = u8::from_str_radix(&nibble, 2) {
            hex.push_str(&format!("{:x}", val));
        }
    }
    
    if hex.len() == 40 {
        Some(hex)
    } else {
        None
    }
}

/// Format bytes as human-readable size
pub fn human_readable_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Search query for scrapers
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// IMDB ID (e.g., "tt1234567")
    pub imdb_id: String,
    /// Title for text-based searching
    pub title: String,
    /// Year for filtering
    pub year: Option<u16>,
    /// Season for TV shows
    pub season: Option<u32>,
    /// Episode for TV shows
    pub episode: Option<u32>,
    /// Content type ("movie" or "series")
    pub content_type: String,
}

impl SearchQuery {
    pub fn movie(imdb_id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            imdb_id: imdb_id.into(),
            title: title.into(),
            year: None,
            season: None,
            episode: None,
            content_type: "movie".to_string(),
        }
    }

    pub fn series(imdb_id: impl Into<String>, title: impl Into<String>, season: u32, episode: u32) -> Self {
        Self {
            imdb_id: imdb_id.into(),
            title: title.into(),
            year: None,
            season: Some(season),
            episode: Some(episode),
            content_type: "series".to_string(),
        }
    }

    pub fn with_year(mut self, year: u16) -> Self {
        self.year = Some(year);
        self
    }
}

/// Trait for implementing scrapers
#[async_trait]
pub trait Scraper: Send + Sync {
    /// Scraper name for identification
    fn name(&self) -> &str;

    /// Whether this scraper is enabled
    fn is_enabled(&self) -> bool { true }

    /// Search for torrents matching the query
    async fn search(&self, query: &SearchQuery) -> ScraperResult<Vec<ScrapedResult>>;
}

/// Manager for multiple scrapers
pub struct ScraperManager {
    scrapers: Vec<Arc<dyn Scraper>>,
    enabled_scrapers: RwLock<HashMap<String, bool>>,
}

impl ScraperManager {
    pub fn new() -> Self {
        Self {
            scrapers: Vec::new(),
            enabled_scrapers: RwLock::new(HashMap::new()),
        }
    }

    /// Add a scraper
    pub fn add_scraper<S: Scraper + 'static>(&mut self, scraper: S) {
        let name = scraper.name().to_string();
        self.scrapers.push(Arc::new(scraper));
        // Scrapers start enabled by default
        let _ = self.enabled_scrapers.get_mut().insert(name, true);
    }

    /// Enable or disable a scraper
    pub async fn set_enabled(&self, name: &str, enabled: bool) {
        let mut map = self.enabled_scrapers.write().await;
        map.insert(name.to_string(), enabled);
    }

    /// Search all enabled scrapers
    pub async fn search_all(&self, query: &SearchQuery) -> Vec<ScrapedResult> {
        let enabled = self.enabled_scrapers.read().await;
        let mut all_results = Vec::new();
        let mut handles = Vec::new();

        for scraper in &self.scrapers {
            let name = scraper.name().to_string();
            
            // Check if enabled
            if !enabled.get(&name).copied().unwrap_or(true) || !scraper.is_enabled() {
                continue;
            }

            let scraper = Arc::clone(scraper);
            let query = query.clone();
            
            handles.push(tokio::spawn(async move {
                match scraper.search(&query).await {
                    Ok(results) => {
                        debug!("Scraper {} found {} results", name, results.len());
                        results
                    }
                    Err(e) => {
                        warn!("Scraper {} failed: {}", name, e);
                        Vec::new()
                    }
                }
            }));
        }

        // Collect all results
        for handle in handles {
            if let Ok(results) = handle.await {
                all_results.extend(results);
            }
        }

        info!("Total scraped results: {}", all_results.len());
        all_results
    }

    /// Get list of available scrapers
    pub fn list_scrapers(&self) -> Vec<String> {
        self.scrapers.iter().map(|s| s.name().to_string()).collect()
    }
}

impl Default for ScraperManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_info_hash() {
        let magnet = "magnet:?xt=urn:btih:abc123def456abc123def456abc123def456abc1&dn=test";
        let hash = extract_info_hash(magnet);
        assert_eq!(hash, Some("abc123def456abc123def456abc123def456abc1".to_string()));
    }

    #[test]
    fn test_human_readable_size() {
        assert_eq!(human_readable_size(500), "500 B");
        assert_eq!(human_readable_size(1536), "1.50 KB");
        assert_eq!(human_readable_size(1_500_000_000), "1.40 GB");
    }
}

/// Percent-encode a value for a magnet query parameter
fn url_encode(v: &str) -> String {
    let mut out = String::with_capacity(v.len() * 3);
    for b in v.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tracker_tests {
    use super::*;

    #[test]
    fn trackers_go_into_magnet() {
        let r = ScrapedResult::from_hash("x", "ABCDEF0123456789ABCDEF0123456789ABCDEF01", "t")
            .with_trackers(vec!["udp://tracker.opentrackr.org:1337/announce".into()]);
        assert!(r.magnet.starts_with("magnet:?xt=urn:btih:abcdef"));
        assert!(r.magnet.ends_with("&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce"));
        assert_eq!(r.trackers.len(), 1);
    }
}
