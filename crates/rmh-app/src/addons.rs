//! Installable HTTP addons (manifest + catalog + stream) and the bundled catalog.

use crate::config::InstalledAddon;
use crate::service::{MediaService, ServiceResult};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::OnceLock;
use tracing::info;

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub resources: Vec<serde_json::Value>,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub catalogs: Vec<RemoteCatalog>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteCatalog {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct StreamResponse {
    #[serde(default)]
    streams: Vec<RemoteStream>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteStream {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub infoHash: Option<String>,
    #[serde(default)]
    pub fileIdx: Option<u32>,
    #[serde(default)]
    pub behaviorHints: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogResponse {
    #[serde(default)]
    metas: Vec<RemoteMeta>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteMeta {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub releaseInfo: Option<String>,
}

/// Entry in the shipped Aurivon addon catalog (`data/addon_catalog.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// `official` or `community`
    pub kind: String,
    #[serde(default)]
    pub manifest_url: Option<String>,
    /// Built-in feature key: `trakt-catalogs`, `torrentio`, …
    #[serde(default)]
    pub builtin: Option<String>,
    #[serde(default)]
    pub resources: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogFile {
    entries: Vec<CatalogEntry>,
}

fn bundled_catalog() -> &'static [CatalogEntry] {
    static CAT: OnceLock<Vec<CatalogEntry>> = OnceLock::new();
    CAT.get_or_init(|| {
        let raw = include_str!("../data/addon_catalog.json");
        serde_json::from_str::<CatalogFile>(raw)
            .map(|f| f.entries)
            .unwrap_or_default()
    })
}

fn normalize_base(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    if u.ends_with("/manifest.json") {
        u.trim_end_matches("/manifest.json").to_string()
    } else {
        u.to_string()
    }
}

fn looks_like_torrentio(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.contains("torrentio.strem.fun") || u.contains("torrentio.")
}

impl MediaService {
    pub fn addon_catalog_entries(&self) -> Vec<CatalogEntry> {
        bundled_catalog().to_vec()
    }

    pub fn list_addons(&self) -> Vec<InstalledAddon> {
        self.config().addons
    }

    pub fn builtin_addon_enabled(&self, builtin: &str) -> bool {
        let cfg = self.config();
        match builtin {
            "torrentio" => cfg.enable_torrentio,
            "trakt-catalogs" => cfg.enable_trakt_catalogs,
            _ => false,
        }
    }

    pub fn set_builtin_addon_enabled(&self, builtin: &str, enabled: bool) -> ServiceResult<()> {
        let mut cfg = self.config();
        match builtin {
            "torrentio" => cfg.enable_torrentio = enabled,
            "trakt-catalogs" => cfg.enable_trakt_catalogs = enabled,
            _ => return Err(format!("Unknown built-in addon: {builtin}")),
        }
        self.save_config(cfg)
    }

    pub fn install_addon(&self, url: String) -> impl Future<Output = ServiceResult<InstalledAddon>> + 'static {
        let http = self.inner.http.clone();
        let svc = self.clone();
        self.run(async move {
            let base = normalize_base(&url);
            let manifest_url = format!("{base}/manifest.json");
            let m: RemoteManifest = http
                .get(&manifest_url)
                .send()
                .await
                .map_err(|e| format!("Addon fetch failed: {e}"))?
                .error_for_status()
                .map_err(|e| format!("Addon HTTP error: {e}"))?
                .json()
                .await
                .map_err(|e| format!("Invalid addon manifest: {e}"))?;
            let installed = InstalledAddon {
                url: base.clone(),
                id: m.id.clone(),
                name: m.name.clone(),
                enabled: true,
            };
            let mut cfg = svc.config();
            cfg.addons.retain(|a| a.id != installed.id);
            cfg.addons.push(installed.clone());
            // Avoid double-querying Torrentio: prefer the remote instance
            if looks_like_torrentio(&base) {
                cfg.enable_torrentio = false;
            }
            svc.save_config(cfg)?;
            info!("Installed addon {} ({})", installed.name, installed.id);
            Ok(installed)
        })
    }

    pub fn install_catalog_entry(&self, entry_id: String) -> impl Future<Output = ServiceResult<InstalledAddon>> + 'static {
        let entry = bundled_catalog().iter().find(|e| e.id == entry_id).cloned();
        let svc = self.clone();
        self.run(async move {
            let entry = entry.ok_or_else(|| "Catalog entry not found".to_string())?;
            if let Some(builtin) = entry.builtin.as_deref() {
                svc.set_builtin_addon_enabled(builtin, true)?;
                return Ok(InstalledAddon {
                    url: format!("builtin:{builtin}"),
                    id: entry.id,
                    name: entry.name,
                    enabled: true,
                });
            }
            let url = entry.manifest_url.ok_or_else(|| "Catalog entry has no manifest URL".to_string())?;
            svc.install_addon(url).await
        })
    }

    pub fn remove_addon(&self, id: &str) -> ServiceResult<()> {
        let mut cfg = self.config();
        cfg.addons.retain(|a| a.id != id);
        self.save_config(cfg)
    }

    pub fn set_addon_enabled(&self, id: &str, enabled: bool) -> ServiceResult<()> {
        let mut cfg = self.config();
        if let Some(a) = cfg.addons.iter_mut().find(|a| a.id == id) {
            a.enabled = enabled;
        }
        self.save_config(cfg)
    }

    pub fn addon_catalog(
        &self,
        addon_id: String,
        content_type: String,
        catalog_id: String,
    ) -> impl Future<Output = ServiceResult<Vec<RemoteMeta>>> + 'static {
        let http = self.inner.http.clone();
        let addons = self.config().addons;
        self.run(async move {
            let addon = addons
                .into_iter()
                .find(|a| a.id == addon_id && a.enabled)
                .ok_or_else(|| "Addon not found or disabled".to_string())?;
            let url = format!("{}/catalog/{}/{}.json", addon.url.trim_end_matches('/'), content_type, catalog_id);
            let resp: CatalogResponse = http
                .get(&url)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|e| e.to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            Ok(resp.metas)
        })
    }

    /// Streams from every enabled remote addon for an IMDB content id
    pub fn addon_streams(
        &self,
        content_type: String,
        id: String,
    ) -> impl Future<Output = ServiceResult<Vec<(String, Vec<RemoteStream>)>>> + 'static {
        let http = self.inner.http.clone();
        let addons = self.config().addons;
        self.run(async move {
            let mut out = vec![];
            for a in addons.into_iter().filter(|a| a.enabled && !a.url.starts_with("builtin:")) {
                let url = format!("{}/stream/{}/{}.json", a.url.trim_end_matches('/'), content_type, id);
                match http.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        if let Ok(body) = resp.json::<StreamResponse>().await {
                            if !body.streams.is_empty() {
                                out.push((a.name, body.streams));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(out)
        })
    }
}
