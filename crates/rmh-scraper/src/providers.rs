//! Extra debrid providers (TorBox, AllDebrid) used alongside Real-Debrid.

use crate::error::{ScraperError, ScraperResult};
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, info};

/// Which debrid service produced a direct link
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebridProvider {
    RealDebrid,
    TorBox,
    AllDebrid,
}

impl DebridProvider {
    pub fn label(self) -> &'static str {
        match self {
            DebridProvider::RealDebrid => "Real-Debrid",
            DebridProvider::TorBox => "TorBox",
            DebridProvider::AllDebrid => "AllDebrid",
        }
    }
}

/// TorBox: create a torrent from a magnet and get a download link when cached
pub struct TorBox {
    client: Client,
    api_key: String,
}

impl TorBox {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
        }
    }

    /// Resolve a magnet/infohash to a direct HTTPS URL when TorBox has it cached
    pub async fn resolve_cached(&self, info_hash: &str) -> ScraperResult<Option<String>> {
        let magnet = format!("magnet:?xt=urn:btih:{info_hash}");
        let resp = self
            .client
            .post("https://api.torbox.app/v1/api/torrents/createtorrent")
            .bearer_auth(&self.api_key)
            .form(&[("magnet", magnet.as_str()), ("seed", "1"), ("allow_zip", "false")])
            .send()
            .await?;
        if !resp.status().is_success() {
            debug!("TorBox create: {}", resp.status());
            return Ok(None);
        }
        #[derive(Deserialize)]
        struct Create {
            data: Option<CreateData>,
            success: Option<bool>,
        }
        #[derive(Deserialize)]
        struct CreateData {
            torrent_id: Option<i64>,
            #[serde(default)]
            hash: Option<String>,
        }
        let body: Create = resp.json().await.map_err(|e| ScraperError::Parse(e.to_string()))?;
        let Some(id) = body.data.and_then(|d| d.torrent_id) else {
            return Ok(None);
        };
        // Request a download link for the first file
        let link = self
            .client
            .get("https://api.torbox.app/v1/api/torrents/requestdl")
            .bearer_auth(&self.api_key)
            .query(&[("token", self.api_key.as_str()), ("torrent_id", &id.to_string()), ("file_id", "0")])
            .send()
            .await?;
        if !link.status().is_success() {
            let _ = self
                .client
                .post("https://api.torbox.app/v1/api/torrents/controltorrent")
                .bearer_auth(&self.api_key)
                .json(&serde_json::json!({ "torrent_id": id, "operation": "delete" }))
                .send()
                .await;
            return Ok(None);
        }
        #[derive(Deserialize)]
        struct Dl {
            data: Option<String>,
        }
        let dl: Dl = link.json().await.map_err(|e| ScraperError::Parse(e.to_string()))?;
        if let Some(url) = dl.data.filter(|u| u.starts_with("http")) {
            info!("TorBox cached link for {info_hash}");
            Ok(Some(url))
        } else {
            Ok(None)
        }
    }
}

/// AllDebrid magnet unlock
pub struct AllDebrid {
    client: Client,
    api_key: String,
}

impl AllDebrid {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.into(),
        }
    }

    pub async fn resolve_cached(&self, info_hash: &str) -> ScraperResult<Option<String>> {
        let magnet = format!("magnet:?xt=urn:btih:{info_hash}");
        let resp = self
            .client
            .get("https://api.alldebrid.com/v4/magnet/upload")
            .query(&[("agent", "aurivon"), ("apikey", self.api_key.as_str()), ("magnets[]", magnet.as_str())])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        #[derive(Deserialize)]
        struct Outer {
            status: String,
            data: Option<Data>,
        }
        #[derive(Deserialize)]
        struct Data {
            magnets: Option<Vec<Mag>>,
        }
        #[derive(Deserialize)]
        struct Mag {
            id: Option<i64>,
            ready: Option<bool>,
            files: Option<Vec<File>>,
        }
        #[derive(Deserialize)]
        struct File {
            l: Option<String>,
            n: Option<String>,
        }
        let body: Outer = resp.json().await.map_err(|e| ScraperError::Parse(e.to_string()))?;
        if body.status != "success" {
            return Ok(None);
        }
        let mag = body.data.and_then(|d| d.magnets).and_then(|m| m.into_iter().next());
        let Some(m) = mag else { return Ok(None) };
        if m.ready != Some(true) {
            if let Some(id) = m.id {
                let _ = self
                    .client
                    .get("https://api.alldebrid.com/v4/magnet/delete")
                    .query(&[("agent", "aurivon"), ("apikey", self.api_key.as_str()), ("id", &id.to_string())])
                    .send()
                    .await;
            }
            return Ok(None);
        }
        let link = m.files.and_then(|f| f.into_iter().find_map(|x| x.l));
        if let Some(url) = link.filter(|u| u.starts_with("http")) {
            info!("AllDebrid cached link for {info_hash}");
            Ok(Some(url))
        } else {
            Ok(None)
        }
    }
}
