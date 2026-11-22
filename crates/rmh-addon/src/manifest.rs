//! Addon manifest definitions - HTTP addon protocol (manifest + catalog + stream)

use serde::{Deserialize, Serialize};
use semver::Version;

/// Addon manifest describing capabilities and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonManifest {
    /// Unique identifier (reverse domain notation recommended)
    pub id: String,
    
    /// Human-readable name
    pub name: String,
    
    /// Semantic version
    pub version: String,
    
    /// Description of what the addon provides
    pub description: String,
    
    /// Resources this addon provides: "catalog", "meta", "stream", "subtitles"
    pub resources: Vec<Resource>,
    
    /// Content types supported: "movie", "series", "channel", "tv"
    pub types: Vec<ContentType>,
    
    /// Catalog definitions (if resource includes "catalog")
    #[serde(default)]
    pub catalogs: Vec<CatalogDescriptor>,
    
    /// URL patterns for ID matching (optional)
    #[serde(default)]
    pub id_prefixes: Vec<String>,
    
    /// Addon icon URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    
    /// Background image URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    
    /// Contact email
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_email: Option<String>,
    
    /// Addon behavior configuration
    #[serde(default)]
    pub behavior_hints: BehaviorHints,
}

impl AddonManifest {
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            description: String::new(),
            resources: Vec::new(),
            types: Vec::new(),
            catalogs: Vec::new(),
            id_prefixes: Vec::new(),
            logo: None,
            background: None,
            contact_email: None,
            behavior_hints: BehaviorHints::default(),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_resources(mut self, resources: Vec<Resource>) -> Self {
        self.resources = resources;
        self
    }

    pub fn with_types(mut self, types: Vec<ContentType>) -> Self {
        self.types = types;
        self
    }

    pub fn with_catalog(mut self, catalog: CatalogDescriptor) -> Self {
        self.catalogs.push(catalog);
        self
    }

    /// Check if addon provides a specific resource
    pub fn provides(&self, resource: &Resource) -> bool {
        self.resources.contains(resource)
    }

    /// Check if addon supports a content type
    pub fn supports_type(&self, content_type: &ContentType) -> bool {
        self.types.contains(content_type)
    }

    /// Parse and validate the version
    pub fn parsed_version(&self) -> Option<Version> {
        Version::parse(&self.version).ok()
    }
}

/// Resource types an addon can provide
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Resource {
    Catalog,
    Meta,
    Stream,
    Subtitles,
}

/// Content types the addon handles
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Movie,
    Series,
    Channel,
    Tv,
    Anime,
    Other,
}

impl std::fmt::Display for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContentType::Movie => write!(f, "movie"),
            ContentType::Series => write!(f, "series"),
            ContentType::Channel => write!(f, "channel"),
            ContentType::Tv => write!(f, "tv"),
            ContentType::Anime => write!(f, "anime"),
            ContentType::Other => write!(f, "other"),
        }
    }
}

/// Describes a catalog that can be browsed
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDescriptor {
    /// Content type for this catalog
    #[serde(rename = "type")]
    pub content_type: ContentType,
    
    /// Unique catalog ID within this addon
    pub id: String,
    
    /// Display name
    pub name: String,
    
    /// Optional extra filters/search capabilities
    #[serde(default)]
    pub extra: Vec<ExtraDescriptor>,
}

impl CatalogDescriptor {
    pub fn new(content_type: ContentType, id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            content_type,
            id: id.into(),
            name: name.into(),
            extra: Vec::new(),
        }
    }

    pub fn with_search(mut self) -> Self {
        self.extra.push(ExtraDescriptor {
            name: "search".to_string(),
            is_required: false,
            options: None,
        });
        self
    }

    pub fn with_genre_filter(mut self, genres: Vec<String>) -> Self {
        self.extra.push(ExtraDescriptor {
            name: "genre".to_string(),
            is_required: false,
            options: Some(genres),
        });
        self
    }
}

/// Extra capabilities for a catalog (search, filters)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraDescriptor {
    /// Name of the extra parameter
    pub name: String,
    
    /// Whether this parameter is required
    #[serde(default)]
    pub is_required: bool,
    
    /// Possible values (for dropdowns/filters)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
}

/// Hints about addon behavior
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BehaviorHints {
    /// Addon content is adult-only
    #[serde(default)]
    pub adult: bool,
    
    /// Addon requires configuration before use
    #[serde(default)]
    pub configurable: bool,
    
    /// Configuration URL if configurable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_serialization() {
        let manifest = AddonManifest::new("org.example.test", "Test Addon", "1.0.0")
            .with_description("A test addon")
            .with_resources(vec![Resource::Catalog, Resource::Stream])
            .with_types(vec![ContentType::Movie, ContentType::Series])
            .with_catalog(CatalogDescriptor::new(ContentType::Movie, "top", "Top Movies"));

        let json = serde_json::to_string_pretty(&manifest).unwrap();
        assert!(json.contains("\"id\": \"org.example.test\""));
        assert!(json.contains("\"catalog\""));
    }
}
