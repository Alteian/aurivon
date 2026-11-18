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

    /// Is this torrent cached on Real-Debrid?
    ///
    /// `instantAvailability` was switched off by Real-Debrid (it answers
    /// `403 disabled_endpoint`), so this asks the way the web UI does: add the
    /// magnet, select the video file and see whether the torrent jumps straight
    /// to `downloaded`. Anything else means "not cached" and the torrent is
    /// deleted again. A cached torrent is kept when `keep` is set so its link
    /// can be unrestricted right away.
    pub async fn probe_cached(&self, info_hash: &str, file_idx: Option<u32>, keep: bool) -> ScraperResult<CacheProbe> {
        let magnet = format!("magnet:?xt=urn:btih:{}", info_hash);
        let id = match self.add_magnet(&magnet).await {
            Ok(id) => id,
            Err(ScraperError::RealDebrid(e)) if e.contains("infringing") || e.contains("451") => {
                debug!("RD refuses {info_hash}: {e}");
                return Ok(CacheProbe { blocked: true, ..Default::default() });
            }
            Err(e) => return Err(e),
        };
        let mut probe = CacheProbe::default();
        let mut selected = false;
        for _ in 0..8 {
            let info = match self.get_torrent_info(&id).await {
                Ok(i) => i,
                Err(e) => {
                    let _ = self.delete_torrent(&id).await;
                    return Err(e);
                }
            };
            match info.status.as_str() {
                "waiting_files_selection" if !selected => {
                    let ids: Vec<u32> = pick_video_file(&info.files, file_idx).into_iter().collect();
                    self.select_files(&id, &ids).await?;
                    selected = true;
                    continue;
                }
                "downloaded" => {
                    probe.cached = !info.links.is_empty();
                    probe.links = info.links;
                    break;
                }
                // RD is still fetching the metadata: give it a moment
                "magnet_conversion" | "queued" if !selected => {
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
                _ => break,
            }
        }
        if probe.cached && keep {
            probe.torrent_id = Some(id);
        } else if let Err(e) = self.delete_torrent(&id).await {
            debug!("Could not delete RD probe torrent {id}: {e}");
        }
        debug!("RD probe {info_hash}: cached={}", probe.cached);
        Ok(probe)
    }

    /// Remove a torrent from the Real-Debrid account
    pub async fn delete_torrent(&self, torrent_id: &str) -> ScraperResult<()> {
        let url = format!("{}/torrents/delete/{}", RD_API_BASE, torrent_id);
        let resp = self.client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send()
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ScraperError::RealDebrid(format!("delete failed: {}", resp.status())))
        }
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

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let code = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                .unwrap_or_default();
            return Err(ScraperError::RealDebrid(format!("addMagnet {} {}", status.as_u16(), code)));
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

    /// Direct streaming link for a torrent cached on Real-Debrid
    ///
    /// Fails with "not cached" when RD would have to download it first.
    pub async fn get_stream_link(&self, info_hash: &str, file_idx: Option<u32>) -> ScraperResult<DebridStream> {
        let probe = self.probe_cached(info_hash, file_idx, true).await?;
        if !probe.cached {
            return Err(ScraperError::RealDebrid("not cached on Real-Debrid".into()));
        }
        match probe.links.first() {
            Some(link) => self.unrestrict_link(link).await,
            None => Err(ScraperError::RealDebrid("No links available".into())),
        }
    }
}

/// Outcome of [`RealDebrid::probe_cached`]
#[derive(Debug, Clone, Default)]
pub struct CacheProbe {
    /// RD has the file and serves it instantly
    pub cached: bool,
    /// RD refused the torrent (e.g. `infringing_file`)
    pub blocked: bool,
    /// Kept RD torrent id (only when cached and asked to keep)
    pub torrent_id: Option<String>,
    /// Hoster links, unrestrict one to stream
    pub links: Vec<String>,
}

/// RD file id of the video to select: Torrentio's `fileIdx` (0-based) when it
/// points at a video, otherwise the largest video file
fn pick_video_file(files: &[TorrentFile], file_idx: Option<u32>) -> Option<u32> {
    let videos: Vec<&TorrentFile> = files.iter().filter(|f| is_video_file(&f.path)).collect();
    if let Some(idx) = file_idx {
        if let Some(f) = videos.iter().find(|f| f.id == idx + 1) {
            return Some(f.id);
        }
    }
    videos.iter().max_by_key(|f| f.bytes).map(|f| f.id)
}

// Helper to check if filename is a video
fn is_video_file(filename: &str) -> bool {
    let video_extensions = [
        ".mkv", ".mp4", ".avi", ".mov", ".wmv", ".flv", ".webm", ".m4v", ".ts", ".m2ts"
    ];
    let lower = filename.to_lowercase();
    video_extensions.iter().any(|ext| lower.ends_with(ext))
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

#[cfg(test)]
mod pick_tests {
    use super::*;

    fn f(id: u32, path: &str, bytes: u64) -> TorrentFile {
        TorrentFile { id, path: path.into(), bytes, selected: 0 }
    }

    #[test]
    fn picks_hinted_or_largest_video() {
        let files = vec![f(1, "/a/sample.mkv", 10), f(2, "/a/E01.mkv", 500), f(3, "/a/E02.mkv", 400), f(4, "/a/info.nfo", 1)];
        assert_eq!(pick_video_file(&files, Some(2)), Some(3));
        assert_eq!(pick_video_file(&files, None), Some(2));
        assert_eq!(pick_video_file(&files, Some(3)), Some(2)); // .nfo is not a video
        assert_eq!(pick_video_file(&[f(1, "x.txt", 1)], None), None);
    }
}
