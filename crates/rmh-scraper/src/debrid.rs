//! Real-Debrid API integration
//!
//! Real-Debrid is a premium service that:
//! - Caches popular torrents for instant streaming
//! - Provides high-speed direct download links
//! - Supports multiple hosters
//!
//! API Documentation: https://api.real-debrid.com/

use crate::error::{ScraperError, ScraperResult};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use tracing::{debug, info};

const RD_API_BASE: &str = "https://api.real-debrid.com/rest/1.0";

/// Real-Debrid configuration
#[derive(Debug, Clone)]
pub struct RealDebridConfig {
    pub api_key: String,
}

impl RealDebridConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
        }
    }
}

/// A stream resolved through Real-Debrid
#[derive(Debug, Clone)]
pub struct DebridStream {
    /// Direct download/stream URL
    pub url: String,
    /// Original filename
    pub filename: String,
    /// File size in bytes
    pub filesize: u64,
    /// MIME type
    pub mime_type: Option<String>,
    /// Whether this was from cache (instant) or had to be added
    pub from_cache: bool,
}

/// Real-Debrid API client
pub struct RealDebrid {
    client: Client,
    config: RealDebridConfig,
}

impl RealDebrid {
    pub fn new(config: RealDebridConfig) -> Self {
        let client = Client::builder()
            .user_agent("Aurivon/0.1.0")
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Verify the API key is valid
    pub async fn verify_api_key(&self) -> ScraperResult<UserInfo> {
        let url = format!("{}/user", RD_API_BASE);
        
        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send()
            .await?;

        if resp.status() == 401 {
            return Err(ScraperError::InvalidApiKey);
        }

        let user: UserInfo = resp.json().await
            .map_err(|e| ScraperError::Parse(e.to_string()))?;
        
        info!("Real-Debrid authenticated as: {} (Premium: {})", user.username, user.premium > 0);
        Ok(user)
    }

    /// Check if torrents are cached on Real-Debrid
    /// 
    /// Takes a list of info hashes and returns which ones are cached
    pub async fn check_instant_availability(&self, hashes: &[String]) -> ScraperResult<HashMap<String, CachedTorrent>> {
        if hashes.is_empty() {
            return Ok(HashMap::new());
        }

        // RD accepts up to 100 hashes per request
        let chunks: Vec<&[String]> = hashes.chunks(100).collect();
        let mut results = HashMap::new();

        for chunk in chunks {
            let hash_list = chunk.join("/");
            let url = format!("{}/torrents/instantAvailability/{}", RD_API_BASE, hash_list);

            debug!("Checking instant availability for {} hashes", chunk.len());

            let resp = self.client
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .send()
                .await?;

            if resp.status() == 401 {
                return Err(ScraperError::InvalidApiKey);
            }

            // Response is a map of hash -> availability info
            // Real-Debrid uses lowercase hashes
            // The response can be complex - some hashes might have empty arrays or different formats
            let text = resp.text().await
                .map_err(|e| ScraperError::Parse(format!("Failed to read instant availability response: {}", e)))?;
            
            // Try to parse as flexible JSON
            let data: HashMap<String, serde_json::Value> = serde_json::from_str(&text)
                .map_err(|e| ScraperError::Parse(format!("Failed to parse instant availability JSON: {}", e)))?;

            for (hash, value) in data {
                // Check if this hash has cached data
                if let Some(obj) = value.as_object() {
                    if let Some(rd_value) = obj.get("rd") {
                        if let Some(rd_array) = rd_value.as_array() {
                            if !rd_array.is_empty() {
                                // There's at least one cached variant
                                debug!("Hash {} is cached in RD", hash);
                                // Mark as cached - we'll figure out files later
                                results.insert(hash.to_lowercase(), CachedTorrent { files: vec![] });
                            }
                        }
                    }
                }
            }
        }

        info!("Found {} cached torrents out of {}", results.len(), hashes.len());
        Ok(results)
    }

    /// Add a magnet link to Real-Debrid
    pub async fn add_magnet(&self, magnet: &str) -> ScraperResult<String> {
        let url = format!("{}/torrents/addMagnet", RD_API_BASE);

        let mut form = HashMap::new();
        form.insert("magnet", magnet);

        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .form(&form)
            .send()
            .await?;

        if resp.status() == 401 {
            return Err(ScraperError::InvalidApiKey);
        }

        let data: AddMagnetResponse = resp.json().await
            .map_err(|e| ScraperError::Parse(e.to_string()))?;

        debug!("Added magnet to RD with ID: {}", data.id);
        Ok(data.id)
    }

    /// Select files to download from a torrent
    pub async fn select_files(&self, torrent_id: &str, file_ids: &[u32]) -> ScraperResult<()> {
        let url = format!("{}/torrents/selectFiles/{}", RD_API_BASE, torrent_id);

        let files_str = if file_ids.is_empty() {
            "all".to_string()
        } else {
            file_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")
        };

        let mut form = HashMap::new();
        form.insert("files", files_str);

        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .form(&form)
            .send()
            .await?;

        if resp.status() == 401 {
            return Err(ScraperError::InvalidApiKey);
        }

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ScraperError::RealDebrid(format!("Failed to select files: {}", text)));
        }

        Ok(())
    }

    /// Get torrent info (including links when ready)
    pub async fn get_torrent_info(&self, torrent_id: &str) -> ScraperResult<TorrentInfo> {
        let url = format!("{}/torrents/info/{}", RD_API_BASE, torrent_id);

        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send()
            .await?;

        if resp.status() == 401 {
            return Err(ScraperError::InvalidApiKey);
        }

        let info: TorrentInfo = resp.json().await
            .map_err(|e| ScraperError::Parse(e.to_string()))?;

        Ok(info)
    }

    /// Unrestrict a link to get a direct download URL
    pub async fn unrestrict_link(&self, link: &str) -> ScraperResult<DebridStream> {
        let url = format!("{}/unrestrict/link", RD_API_BASE);

        let mut form = HashMap::new();
        form.insert("link", link);

        let resp = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .form(&form)
            .send()
            .await?;

        if resp.status() == 401 {
            return Err(ScraperError::InvalidApiKey);
        }

        let data: UnrestrictResponse = resp.json().await
            .map_err(|e| ScraperError::Parse(e.to_string()))?;

        Ok(DebridStream {
            url: data.download,
            filename: data.filename,
            filesize: data.filesize,
            mime_type: data.mimeType,
            from_cache: true,
        })
    }

    /// Get a direct streaming link for a cached torrent
    /// 
    /// This is the main method for getting playable URLs
    pub async fn get_stream_link(
        &self,
        info_hash: &str,
        cached_info: Option<&CachedTorrent>,
    ) -> ScraperResult<DebridStream> {
        // First check if it's already in our downloads
        if let Some(link) = self.find_existing_download(info_hash).await? {
            return self.unrestrict_link(&link).await;
        }

        // Add the magnet and get link
        let magnet = format!("magnet:?xt=urn:btih:{}", info_hash);
        let torrent_id = self.add_magnet(&magnet).await?;

        // Select the video file(s)
        let file_ids = if let Some(cached) = cached_info {
            cached.files.iter()
                .filter(|f| is_video_file(&f.filename))
                .map(|f| f.id)
                .collect::<Vec<_>>()
        } else {
            vec![] // Select all
        };

        self.select_files(&torrent_id, &file_ids).await?;

        // Wait for links to be ready (should be instant for cached content)
        let info = self.get_torrent_info(&torrent_id).await?;

        if info.links.is_empty() {
            return Err(ScraperError::RealDebrid("No links available".into()));
        }

        // Unrestrict the first video link
        let link = &info.links[0];
        self.unrestrict_link(link).await
    }

    /// Find if we already have this torrent in our RD downloads
    async fn find_existing_download(&self, info_hash: &str) -> ScraperResult<Option<String>> {
        let url = format!("{}/torrents", RD_API_BASE);
        
        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send()
            .await?;

        if resp.status() == 401 {
            return Err(ScraperError::InvalidApiKey);
        }

        let torrents: Vec<TorrentListItem> = resp.json().await
            .map_err(|e| ScraperError::Parse(e.to_string()))?;

        let hash_lower = info_hash.to_lowercase();
        for torrent in torrents {
            if torrent.hash.to_lowercase() == hash_lower {
                if let Some(links) = torrent.links {
                    if !links.is_empty() {
                        return Ok(Some(links[0].clone()));
                    }
                }
            }
        }

        Ok(None)
    }
}

// Helper to check if filename is a video
fn is_video_file(filename: &str) -> bool {
    let video_extensions = [
        ".mkv", ".mp4", ".avi", ".mov", ".wmv", ".flv", ".webm", ".m4v", ".ts", ".m2ts"
    ];
    let lower = filename.to_lowercase();
    video_extensions.iter().any(|ext| lower.ends_with(ext))
}

// Find the best cached variant (one with largest video file)
fn find_best_variant(variants: &[HashMap<String, CachedFile>]) -> Option<CachedTorrent> {
    let mut best_size = 0u64;
    let mut best_files = Vec::new();

    for variant in variants {
        let video_files: Vec<CachedFileInfo> = variant.iter()
            .filter(|(_, f)| is_video_file(&f.filename))
            .map(|(id, f)| CachedFileInfo {
                id: id.parse().unwrap_or(0),
                filename: f.filename.clone(),
                filesize: f.filesize,
            })
            .collect();

        let total_size: u64 = video_files.iter().map(|f| f.filesize).sum();
        
        if total_size > best_size {
            best_size = total_size;
            best_files = video_files;
        }
    }

    if best_files.is_empty() {
        None
    } else {
        Some(CachedTorrent { files: best_files })
    }
}

// API Response types

#[derive(Debug, Deserialize)]
pub struct UserInfo {
    pub id: u64,
    pub username: String,
    pub email: String,
    pub premium: u64,
    #[serde(default)]
    pub expiration: String,
}

#[derive(Debug, Deserialize)]
struct InstantAvailability {
    rd: Option<Vec<HashMap<String, CachedFile>>>,
}

#[derive(Debug, Deserialize)]
struct CachedFile {
    filename: String,
    filesize: u64,
}

#[derive(Debug, Clone)]
pub struct CachedTorrent {
    pub files: Vec<CachedFileInfo>,
}

#[derive(Debug, Clone)]
pub struct CachedFileInfo {
    pub id: u32,
    pub filename: String,
    pub filesize: u64,
}

#[derive(Debug, Deserialize)]
struct AddMagnetResponse {
    id: String,
    uri: String,
}

#[derive(Debug, Deserialize)]
pub struct TorrentInfo {
    pub id: String,
    pub filename: String,
    pub hash: String,
    pub status: String,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default)]
    pub files: Vec<TorrentFile>,
}

#[derive(Debug, Deserialize)]
pub struct TorrentFile {
    pub id: u32,
    pub path: String,
    pub bytes: u64,
    pub selected: u8,
}

#[derive(Debug, Deserialize)]
struct TorrentListItem {
    id: String,
    hash: String,
    status: String,
    links: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct UnrestrictResponse {
    download: String,
    filename: String,
    filesize: u64,
    mimeType: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires valid API key
    async fn test_verify_api_key() {
        let config = RealDebridConfig::new("YOUR_API_KEY");
        let rd = RealDebrid::new(config);
        
        let result = rd.verify_api_key().await;
        assert!(result.is_ok());
    }
}
