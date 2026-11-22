//! Torrentio scraper
//!
//! Torrentio aggregates torrents from multiple sources.
//! We can use their API to get torrents for movies and shows.
//!
//! This is one of the most reliable sources as it's actively maintained.

use super::{Scraper, ScrapedResult, SearchQuery};
use crate::error::{ScraperError, ScraperResult};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, warn};

const TORRENTIO_BASE: &str = "https://torrentio.strem.fun";

/// Torrentio scraper configuration
#[derive(Debug, Clone)]
pub struct TorrentioConfig {
    /// Provider filter (e.g., "yts,eztv,rarbg,1337x,thepiratebay,kickasstorrents,torrentgalaxy")
    pub providers: Option<String>,
    /// Quality filter (e.g., "480p,720p,1080p,2160p")
    pub quality_filter: Option<String>,
    /// Sort order ("qualitysize", "quality", "size", "seeders")
    pub sort: Option<String>,
    /// Real-Debrid API key for RD-cached results
    pub debrid_api_key: Option<String>,
    /// Limit number of results
    pub limit: Option<u32>,
}

impl Default for TorrentioConfig {
    fn default() -> Self {
        Self {
            providers: None, // All providers
            quality_filter: None, // All qualities
            sort: Some("qualitysize".to_string()),
            debrid_api_key: None,
            limit: Some(50),
        }
    }
}

impl TorrentioConfig {
    pub fn with_debrid(mut self, api_key: impl Into<String>) -> Self {
        self.debrid_api_key = Some(api_key.into());
        self
    }

    /// Build the config path for the URL
    fn config_path(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref providers) = self.providers {
            parts.push(format!("providers={}", providers));
        }

        if let Some(ref quality) = self.quality_filter {
            parts.push(format!("qualityfilter={}", quality));
        }

        if let Some(ref sort) = self.sort {
            parts.push(format!("sort={}", sort));
        }

        // If we have a Real-Debrid key, add it for debrid-cached results
        if let Some(ref key) = self.debrid_api_key {
            parts.push(format!("realdebrid={}", key));
        }

        if let Some(limit) = self.limit {
            parts.push(format!("limit={}", limit));
        }

        if parts.is_empty() {
            String::new()
        } else {
            parts.join("|")
        }
    }
}

/// Torrentio scraper using their HTTP addon API
pub struct TorrentioScraper {
    client: Client,
    config: TorrentioConfig,
}

impl TorrentioScraper {
    pub fn new(config: TorrentioConfig) -> Self {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Build the stream URL for Torrentio API
    fn build_url(&self, content_type: &str, id: &str) -> String {
        let config_path = self.config.config_path();
        
        if config_path.is_empty() {
            format!("{}/stream/{}/{}.json", TORRENTIO_BASE, content_type, id)
        } else {
            format!("{}/{}/stream/{}/{}.json", TORRENTIO_BASE, config_path, content_type, id)
        }
    }
}

#[async_trait]
impl Scraper for TorrentioScraper {
    fn name(&self) -> &str {
        "Torrentio"
    }

    async fn search(&self, query: &SearchQuery) -> ScraperResult<Vec<ScrapedResult>> {
        // Build the ID (for series: tt1234567:1:1 for S01E01)
        let id = if query.content_type == "series" {
            let season = query.season.unwrap_or(1);
            let episode = query.episode.unwrap_or(1);
            format!("{}:{}:{}", query.imdb_id, season, episode)
        } else {
            query.imdb_id.clone()
        };

        let url = self.build_url(&query.content_type, &id);
        debug!("Torrentio URL: {}", url);

        let resp = self.client.get(&url).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            warn!("Torrentio returned status: {}", status);
            return Err(ScraperError::ScraperUnavailable(format!("Status {}", status)));
        }

        let data: TorrentioResponse = resp.json().await
            .map_err(|e| ScraperError::Parse(e.to_string()))?;

        let mut results = Vec::new();

        for stream in data.streams {
            // Extract info hash from the stream
            let info_hash = if let Some(ref ih) = stream.info_hash {
                ih.to_lowercase()
            } else if let Some(ref url) = stream.url {
                // Try to extract from magnet URL
                if let Some(hash) = super::extract_info_hash(url) {
                    hash
                } else {
                    continue;
                }
            } else {
                continue;
            };

            // Parse the title from Torrentio's format
            // They typically use: "Title\n👤 seeders 💾 size ⚙️ source"
            let title = stream.title.as_deref().unwrap_or(&stream.name);
            let clean_title = title.lines().next().unwrap_or(title);

            let trackers: Vec<String> = stream
                .sources
                .iter()
                .filter_map(|s| s.strip_prefix("tracker:"))
                .map(str::to_string)
                .collect();
            let mut result = ScrapedResult::from_hash(clean_title, &info_hash, "Torrentio").with_trackers(trackers);
            result.file_idx = stream.file_idx;
            
            // Parse seeders and size from the title
            if let Some(parsed) = parse_torrentio_title(title) {
                if let Some(seeders) = parsed.seeders {
                    result = result.with_seeders(seeders);
                }
                if let Some(size) = parsed.size {
                    result = result.with_size_str(size);
                }
            }

            // Check if this is marked as cached (RD)
            if title.contains("⚡") || title.contains("[RD+]") || title.contains("[RD]") {
                result.rd_cached = true;
            }

            // Extract languages
            if title.contains("🇬🇧") || title.contains("English") {
                result.languages.push("en".to_string());
            }
            if title.contains("🇪🇸") || title.contains("Spanish") {
                result.languages.push("es".to_string());
            }
            if title.contains("🇫🇷") || title.contains("French") {
                result.languages.push("fr".to_string());
            }
            if title.contains("🇩🇪") || title.contains("German") {
                result.languages.push("de".to_string());
            }

            results.push(result);
        }

        debug!("Torrentio found {} streams", results.len());
        Ok(results)
    }
}

/// Torrentio API response
#[derive(Debug, Deserialize)]
struct TorrentioResponse {
    streams: Vec<TorrentioStream>,
}

#[derive(Debug, Deserialize)]
struct TorrentioStream {
    name: String,
    title: Option<String>,
    #[serde(rename = "infoHash")]
    info_hash: Option<String>,
    url: Option<String>,
    #[serde(rename = "fileIdx")]
    file_idx: Option<u32>,
    /// "tracker:<url>" and "dht:<hash>" entries
    #[serde(default)]
    sources: Vec<String>,
}

/// Parsed Torrentio title info
struct ParsedTorrentioInfo {
    seeders: Option<u32>,
    size: Option<String>,
}

/// Parse Torrentio's formatted title
fn parse_torrentio_title(title: &str) -> Option<ParsedTorrentioInfo> {
    let mut info = ParsedTorrentioInfo {
        seeders: None,
        size: None,
    };

    // Look for seeders (👤 123)
    if let Some(idx) = title.find("👤") {
        let rest = &title[idx..];
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 2 {
            info.seeders = parts[1].parse().ok();
        }
    }

    // Look for size (💾 1.5 GB)
    if let Some(idx) = title.find("💾") {
        let rest = &title[idx..];
        // Find the size value (number + unit)
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 3 {
            info.size = Some(format!("{} {}", parts[1], parts[2]));
        }
    }

    Some(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_path() {
        let config = TorrentioConfig {
            providers: Some("yts,rarbg".to_string()),
            sort: Some("qualitysize".to_string()),
            debrid_api_key: Some("TEST_KEY".to_string()),
            ..Default::default()
        };
        
        let path = config.config_path();
        assert!(path.contains("providers=yts,rarbg"));
        assert!(path.contains("realdebrid=TEST_KEY"));
    }

    #[test]
    fn test_parse_torrentio_title() {
        let title = "The.Matrix.1999.1080p.BluRay\n👤 125 💾 2.5 GB ⚙️ YTS";
        let parsed = parse_torrentio_title(title).unwrap();
        assert_eq!(parsed.seeders, Some(125));
        assert_eq!(parsed.size, Some("2.5 GB".to_string()));
    }
}
