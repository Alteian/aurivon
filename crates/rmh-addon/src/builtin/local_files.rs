//! Local files addon - provides catalog and streams for local media

use crate::manifest::{AddonManifest, CatalogDescriptor, ContentType, Resource};
use crate::resources::{CatalogResponse, MetaItem, Stream};
use crate::traits::{Addon, AddonError, AddonResult, CatalogProvider, ExtraParams, StreamProvider};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use walkdir::WalkDir;

/// Video file extensions supported
const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", 
    "mpg", "mpeg", "ts", "m2ts", "vob", "3gp", "ogv",
];

/// A media file discovered on disk
#[derive(Debug, Clone)]
pub struct LocalMediaFile {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
}

/// Local files addon - scans and serves local media
pub struct LocalFilesAddon {
    manifest: AddonManifest,
    library_paths: Vec<PathBuf>,
    /// Cached media files (id -> file)
    cache: Arc<RwLock<HashMap<String, LocalMediaFile>>>,
}

impl LocalFilesAddon {
    pub fn new(library_paths: Vec<PathBuf>) -> Self {
        Self {
            manifest: AddonManifest::new(
                "org.aurivon.local-files",
                "Local Files",
                "1.0.0",
            )
            .with_description("Access your local media library")
            .with_resources(vec![Resource::Catalog, Resource::Stream])
            .with_types(vec![ContentType::Movie, ContentType::Series])
            .with_catalog(CatalogDescriptor::new(
                ContentType::Movie,
                "local-movies",
                "Local Movies",
            ))
            .with_catalog(CatalogDescriptor::new(
                ContentType::Series,
                "local-series",
                "Local Series",
            )),
            library_paths,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a library path
    pub fn add_library_path(&mut self, path: PathBuf) {
        self.library_paths.push(path);
    }

    /// Scan library paths for media files
    pub async fn scan(&self) -> Vec<LocalMediaFile> {
        let mut files = Vec::new();
        
        for library_path in &self.library_paths {
            if !library_path.exists() {
                tracing::warn!("Library path does not exist: {:?}", library_path);
                continue;
            }
            
            for entry in WalkDir::new(library_path)
                .follow_links(true)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                
                if !path.is_file() {
                    continue;
                }
                
                let extension = path.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase());
                
                if let Some(ext) = extension {
                    if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
                        let name = path.file_stem()
                            .and_then(|n| n.to_str())
                            .unwrap_or("Unknown")
                            .to_string();
                        
                        let size = entry.metadata()
                            .map(|m| m.len())
                            .unwrap_or(0);
                        
                        let id = format!("local:{}", path.display());
                        
                        files.push(LocalMediaFile {
                            id,
                            path: path.to_path_buf(),
                            name,
                            size,
                        });
                    }
                }
            }
        }
        
        // Update cache
        let mut cache = self.cache.write().await;
        cache.clear();
        for file in &files {
            cache.insert(file.id.clone(), file.clone());
        }
        
        tracing::info!("Scanned {} media files", files.len());
        files
    }

    /// Get a file by ID
    pub async fn get_file(&self, id: &str) -> Option<LocalMediaFile> {
        let cache = self.cache.read().await;
        cache.get(id).cloned()
    }

    /// Parse title to extract name and year
    fn looks_like_episode(path: &str, name: &str) -> bool {
        let lower = format!("{path}/{name}").to_lowercase();
        let n = name.to_lowercase();
        lower.contains("/season")
            || lower.contains("/series/")
            || lower.contains("/tv/")
            || lower.contains("/shows/")
            || n.contains("s0") && n.contains('e')
            || n.contains("s1") && n.contains('e')
            || n.contains("s2") && n.contains('e')
            || n.contains('x') && n.chars().any(|c| c.is_ascii_digit())
                && regex::Regex::new(r"(?i)\d+x\d+").ok().is_some_and(|re| re.is_match(&n))
            || regex::Regex::new(r"(?i)s\d{1,2}e\d{1,2}").ok().is_some_and(|re| re.is_match(&n))
    }

    fn parse_title(name: &str) -> (String, Option<u16>) {
        // Try to extract year from title like "Movie Name (2023)" or "Movie.Name.2023"
        let re = regex::Regex::new(r"^(.+?)[\.\s\-_]*\(?(\d{4})\)?[\.\s\-_]*").ok();
        
        if let Some(re) = re {
            if let Some(caps) = re.captures(name) {
                let title = caps.get(1).map(|m| m.as_str()).unwrap_or(name);
                let year = caps.get(2).and_then(|m| m.as_str().parse().ok());
                
                // Clean up title
                let clean_title = title
                    .replace('.', " ")
                    .replace('_', " ")
                    .trim()
                    .to_string();
                
                return (clean_title, year);
            }
        }
        
        (name.replace('.', " ").replace('_', " ").trim().to_string(), None)
    }
}

#[async_trait]
impl Addon for LocalFilesAddon {
    fn manifest(&self) -> &AddonManifest {
        &self.manifest
    }
}

#[async_trait]
impl CatalogProvider for LocalFilesAddon {
    async fn catalog(
        &self,
        content_type: &ContentType,
        _catalog_id: &str,
        extra: &ExtraParams,
    ) -> AddonResult<CatalogResponse> {
        let files = self.scan().await;
        
        let skip = extra.skip.unwrap_or(0) as usize;
        let limit = 100usize;
        
        // Filter and convert to MetaItems
        let metas: Vec<MetaItem> = files
            .into_iter()
            .filter(|f| {
                let path = f.path.to_string_lossy();
                let is_ep = Self::looks_like_episode(&path, &f.name);
                match content_type {
                    ContentType::Movie => !is_ep,
                    ContentType::Series => is_ep,
                    _ => true,
                }
            })
            .skip(skip)
            .take(limit)
            .map(|f| {
                let (title, year) = Self::parse_title(&f.name);
                MetaItem::new(f.id, content_type.to_string(), title)
                    .with_description(format!("Local file: {}", f.path.display()))
                    .with_year(year.unwrap_or(0))
            })
            .collect();

        Ok(CatalogResponse::new(metas))
    }
}

#[async_trait]
impl StreamProvider for LocalFilesAddon {
    async fn streams(
        &self,
        _content_type: &ContentType,
        id: &str,
    ) -> AddonResult<Vec<Stream>> {
        // Check if this is a local file ID
        if !id.starts_with("local:") {
            return Ok(vec![]);
        }

        let file = self.get_file(id).await
            .ok_or_else(|| AddonError::NotFound(format!("File not found: {}", id)))?;

        let stream = Stream::local_file(file.path.display().to_string())
            .with_name(self.manifest.name.clone())
            .with_title(file.name.clone());

        Ok(vec![stream])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_title() {
        let cases = vec![
            ("The.Matrix.1999.1080p.BluRay", ("The Matrix", Some(1999))),
            ("Inception (2010)", ("Inception", Some(2010))),
            ("Movie_Name_2023_HDRip", ("Movie Name", Some(2023))),
            ("No Year Here", ("No Year Here", None)),
        ];

        for (input, expected) in cases {
            let result = LocalFilesAddon::parse_title(input);
            assert_eq!(result.0, expected.0);
            assert_eq!(result.1, expected.1);
        }
    }
}