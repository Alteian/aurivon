//! Installable Stremio-compatible HTTP addons (manifest + catalog + stream).

use crate::config::InstalledAddon;
use crate::service::{MediaService, ServiceResult};
use serde::Deserialize;
use std::future::Future;
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

fn normalize_base(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    if u.ends_with("/manifest.json") {
        u.trim_end_matches("/manifest.json").to_string()
    } else {
        u.to_string()
    }
}

impl MediaService {
    pub fn list_addons(&self) -> Vec<InstalledAddon> {
        self.config().addons
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
                url: base,
                id: m.id.clone(),
                name: m.name.clone(),
                enabled: true,
            };
            let mut cfg = svc.config();
            cfg.addons.retain(|a| a.id != installed.id);
            cfg.addons.push(installed.clone());
            svc.save_config(cfg)?;
            info!("Installed addon {} ({})", installed.name, installed.id);
            Ok(installed)
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

    /// Streams from every enabled remote addon for an IMDB / Stremio id
    pub fn addon_streams(
        &self,
        content_type: String,
        id: String,
    ) -> impl Future<Output = ServiceResult<Vec<(String, Vec<RemoteStream>)>>> + 'static {
        let http = self.inner.http.clone();
        let addons = self.config().addons;
        self.run(async move {
            let mut out = vec![];
            for a in addons.into_iter().filter(|a| a.enabled) {
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
