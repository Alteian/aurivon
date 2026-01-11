//! Addon registry for managing installed addons

use crate::manifest::{AddonManifest, ContentType};
use crate::resources::{CatalogResponse, MetaItem, Stream, Subtitle};
use crate::traits::{
    AddonError, AddonResult, CatalogProvider, ExtraParams, MetaProvider, 
    StreamProvider, SubtitleProvider,
};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

/// Registry for managing addons
pub struct AddonRegistry {
    /// All registered addons by ID
    addons: HashMap<String, RegisteredAddon>,
    /// Catalog providers
    catalog_providers: Vec<String>,
    /// Meta providers
    meta_providers: Vec<String>,
    /// Stream providers
    stream_providers: Vec<String>,
    /// Subtitle providers
    subtitle_providers: Vec<String>,
}

/// A registered addon with its capabilities
struct RegisteredAddon {
    manifest: AddonManifest,
    catalog: Option<Arc<dyn CatalogProvider>>,
    meta: Option<Arc<dyn MetaProvider>>,
    streams: Option<Arc<dyn StreamProvider>>,
    subtitles: Option<Arc<dyn SubtitleProvider>>,
    enabled: bool,
}

impl AddonRegistry {
    pub fn new() -> Self {
        Self {
            addons: HashMap::new(),
            catalog_providers: Vec::new(),
            meta_providers: Vec::new(),
            stream_providers: Vec::new(),
            subtitle_providers: Vec::new(),
        }
    }

    /// Register a catalog provider
    pub fn register_catalog<T: CatalogProvider + 'static>(&mut self, addon: T) {
        let id = addon.manifest().id.clone();
        let manifest = addon.manifest().clone();
        
        info!("Registering catalog addon: {} ({})", manifest.name, id);
        
        let addon_arc = Arc::new(addon);
        
        if let Some(existing) = self.addons.get_mut(&id) {
            existing.catalog = Some(addon_arc);
        } else {
            self.addons.insert(id.clone(), RegisteredAddon {
                manifest,
                catalog: Some(addon_arc),
                meta: None,
                streams: None,
                subtitles: None,
                enabled: true,
            });
        }
        
        if !self.catalog_providers.contains(&id) {
            self.catalog_providers.push(id);
        }
    }

    /// Register a meta provider
    pub fn register_meta<T: MetaProvider + 'static>(&mut self, addon: T) {
        let id = addon.manifest().id.clone();
        let manifest = addon.manifest().clone();
        
        info!("Registering meta addon: {} ({})", manifest.name, id);
        
        let addon_arc = Arc::new(addon);
        
        if let Some(existing) = self.addons.get_mut(&id) {
            existing.meta = Some(addon_arc);
        } else {
            self.addons.insert(id.clone(), RegisteredAddon {
                manifest,
                catalog: None,
                meta: Some(addon_arc),
                streams: None,
                subtitles: None,
                enabled: true,
            });
        }
        
        if !self.meta_providers.contains(&id) {
            self.meta_providers.push(id);
        }
    }

    /// Register a stream provider
    pub fn register_streams<T: StreamProvider + 'static>(&mut self, addon: T) {
        let id = addon.manifest().id.clone();
        let manifest = addon.manifest().clone();
        
        info!("Registering stream addon: {} ({})", manifest.name, id);
        
        let addon_arc = Arc::new(addon);
        
        if let Some(existing) = self.addons.get_mut(&id) {
            existing.streams = Some(addon_arc);
        } else {
            self.addons.insert(id.clone(), RegisteredAddon {
                manifest,
                catalog: None,
                meta: None,
                streams: Some(addon_arc),
                subtitles: None,
                enabled: true,
            });
        }
        
        if !self.stream_providers.contains(&id) {
            self.stream_providers.push(id);
        }
    }

    /// Register a subtitle provider
    pub fn register_subtitles<T: SubtitleProvider + 'static>(&mut self, addon: T) {
        let id = addon.manifest().id.clone();
        let manifest = addon.manifest().clone();
        
        info!("Registering subtitle addon: {} ({})", manifest.name, id);
        
        let addon_arc = Arc::new(addon);
        
        if let Some(existing) = self.addons.get_mut(&id) {
            existing.subtitles = Some(addon_arc);
        } else {
            self.addons.insert(id.clone(), RegisteredAddon {
                manifest,
                catalog: None,
                meta: None,
                streams: None,
                subtitles: Some(addon_arc),
                enabled: true,
            });
        }
        
        if !self.subtitle_providers.contains(&id) {
            self.subtitle_providers.push(id);
        }
    }

    /// Get all installed addon manifests
    pub fn list_addons(&self) -> Vec<&AddonManifest> {
        self.addons.values().map(|a| &a.manifest).collect()
    }

    /// Get addon by ID
    pub fn get_addon(&self, id: &str) -> Option<&AddonManifest> {
        self.addons.get(id).map(|a| &a.manifest)
    }

    /// Enable or disable an addon
    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        if let Some(addon) = self.addons.get_mut(id) {
            addon.enabled = enabled;
            info!("Addon {} {}", id, if enabled { "enabled" } else { "disabled" });
        }
    }

    /// Get all catalog descriptors from all addons
    pub fn get_all_catalogs(&self) -> Vec<(&str, &crate::manifest::CatalogDescriptor)> {
        self.addons
            .iter()
            .filter(|(_, a)| a.enabled && a.catalog.is_some())
            .flat_map(|(id, a)| {
                a.manifest.catalogs.iter().map(move |c| (id.as_str(), c))
            })
            .collect()
    }

    /// Fetch catalog from a specific addon
    pub async fn fetch_catalog(
        &self,
        addon_id: &str,
        content_type: &ContentType,
        catalog_id: &str,
        extra: &ExtraParams,
    ) -> AddonResult<CatalogResponse> {
        let addon = self.addons.get(addon_id)
            .ok_or_else(|| AddonError::NotFound(format!("Addon not found: {}", addon_id)))?;
        
        if !addon.enabled {
            return Err(AddonError::NotSupported("Addon is disabled".into()));
        }
        
        let provider = addon.catalog.as_ref()
            .ok_or_else(|| AddonError::NotSupported("Addon doesn't provide catalogs".into()))?;
        
        provider.catalog(content_type, catalog_id, extra).await
    }

    /// Fetch metadata from all providers (returns first successful result)
    pub async fn fetch_meta(
        &self,
        content_type: &ContentType,
        id: &str,
    ) -> AddonResult<MetaItem> {
        for addon_id in &self.meta_providers {
            if let Some(addon) = self.addons.get(addon_id) {
                if !addon.enabled {
                    continue;
                }
                if let Some(provider) = &addon.meta {
                    match provider.meta(content_type, id).await {
                        Ok(meta) => return Ok(meta),
                        Err(e) => {
                            debug!("Meta provider {} failed for {}: {}", addon_id, id, e);
                            continue;
                        }
                    }
                }
            }
        }
        Err(AddonError::NotFound(format!("No metadata found for {}", id)))
    }

    /// Fetch streams from all providers (aggregates all results)
    pub async fn fetch_streams(
        &self,
        content_type: &ContentType,
        id: &str,
    ) -> Vec<(String, Vec<Stream>)> {
        let mut results = Vec::new();
        
        for addon_id in &self.stream_providers {
            if let Some(addon) = self.addons.get(addon_id) {
                if !addon.enabled {
                    continue;
                }
                if let Some(provider) = &addon.streams {
                    match provider.streams(content_type, id).await {
                        Ok(streams) if !streams.is_empty() => {
                            results.push((addon.manifest.name.clone(), streams));
                        }
                        Ok(_) => {
                            debug!("Stream provider {} returned empty for {}", addon_id, id);
                        }
                        Err(e) => {
                            debug!("Stream provider {} failed for {}: {}", addon_id, id, e);
                        }
                    }
                }
            }
        }
        
        results
    }

    /// Fetch subtitles from all providers (aggregates all results)
    pub async fn fetch_subtitles(
        &self,
        content_type: &ContentType,
        id: &str,
        video_id: Option<&str>,
    ) -> Vec<Subtitle> {
        let mut all_subs = Vec::new();
        
        for addon_id in &self.subtitle_providers {
            if let Some(addon) = self.addons.get(addon_id) {
                if !addon.enabled {
                    continue;
                }
                if let Some(provider) = &addon.subtitles {
                    match provider.subtitles(content_type, id, video_id).await {
                        Ok(subs) => all_subs.extend(subs),
                        Err(e) => {
                            debug!("Subtitle provider {} failed: {}", addon_id, e);
                        }
                    }
                }
            }
        }
        
        all_subs
    }
}

impl Default for AddonRegistry {
    fn default() -> Self {
        Self::new()
    }
}
